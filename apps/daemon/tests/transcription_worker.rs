#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn json_request(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer worker-test-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn transcription_tool_runs_configured_worker_and_persists_result() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("transcription-worker.py");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json, sys, time
from pathlib import Path
request = json.load(sys.stdin)
source = Path(request["inputPath"])
source_hash = source.parent.name + source.name
assert request["options"]["diarization"] is True
assert request["options"]["minSpeakers"] == 1
assert request["options"]["maxSpeakers"] == 2
print(json.dumps({"jobId": request["jobId"], "type": "progress", "progress": 0.4, "message": "Transcribing"}), flush=True)
time.sleep(0.25)
print(json.dumps({"jobId": request["jobId"], "type": "result", "result": {"sourceSha256": source_hash, "language": "en", "words": [{"id": "word-1", "spokenText": "hello", "displayText": "hello", "startMs": 0, "endMs": 500, "speakerId": "speaker_1"}, {"id": "word-filler", "spokenText": "um", "displayText": "um", "startMs": 500, "endMs": 650, "speakerId": "speaker_1"}, {"id": "word-2", "spokenText": "there", "displayText": "there", "startMs": 700, "endMs": 1000, "speakerId": "speaker_1"}, {"id": "word-3", "spokenText": "world", "displayText": "world", "startMs": 2000, "endMs": 2500, "speakerId": "speaker_1"}], "utterances": [{"id": "utterance-1", "speakerId": "speaker_1", "wordIds": ["word-1", "word-filler", "word-2"]}, {"id": "utterance-2", "speakerId": "speaker_1", "wordIds": ["word-3"]}]}}), flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(script);
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "transcription-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "worker-test-token".to_owned())
        .await
        .unwrap();
    let stored = state.layout.put_media(b"fake audio fixture").await.unwrap();
    let app = build_app(state.clone());

    let (_, created) = json_request(
        &app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Transcript", "idempotencyKey": "create-transcript" }),
    )
    .await;
    let project_id = created["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, committed) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "add-audio",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "add-audio",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{
                "type": "addAsset",
                "asset": {
                    "id": "asset-audio",
                    "name": "Fixture audio",
                    "kind": "audio",
                    "contentHash": stored.sha256,
                    "hasAudio": true,
                    "durationTicks": 480000,
                    "provenance": { "type": "imported", "sourceName": "fixture.wav" }
                }
            }, {
                "type": "addScene",
                "scene": {
                    "id": "scene-transcript",
                    "name": "Main",
                    "isMain": true,
                    "tracks": [{
                        "id": "track-dialogue",
                        "name": "Dialogue",
                        "kind": "audio",
                        "muted": false,
                        "hidden": false,
                        "locked": false,
                        "items": [{
                            "id": "item-dialogue",
                            "name": "Fixture audio",
                            "startTicks": 0,
                            "durationTicks": 480000,
                            "sourceDurationTicks": 480000,
                            "enabled": true,
                            "content": {
                                "type": "media",
                                "assetId": "asset-audio",
                                "mediaKind": "audio"
                            }
                        }]
                    }],
                    "bookmarks": []
                },
                "index": null
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{committed}");
    assert_eq!(committed["envelope"]["revision"], 1);

    let (status, queued) = json_request(
        &app,
        "POST",
        "/api/v1/tools/start_transcription",
        json!({
            "arguments": {
                "projectId": project_id,
                "assetId": "asset-audio",
                "expectedRevision": 1,
                "language": "en",
                "engine": "faster-whisper",
                "diarization": true,
                "minSpeakers": 1,
                "maxSpeakers": 2
            },
            "idempotencyKey": "transcribe-fixture"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    assert_eq!(queued["ok"], true);
    let job_id = queued["jobId"].as_str().unwrap();
    let (status, concurrent_edit) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "rename-during-transcription",
            "projectId": project_id,
            "baseRevision": 1,
            "idempotencyKey": "rename-during-transcription",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{ "type": "setProjectName", "name": "Renamed while transcribing" }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{concurrent_edit}");
    assert_eq!(concurrent_edit["envelope"]["revision"], 2);
    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = state.database.read_job(job_id).await.unwrap();
            if job.state == "succeeded" {
                break job;
            }
            assert_ne!(job.state, "failed", "{:?}", job.error);
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let output = completed.output.unwrap();
    assert_eq!(output["words"][0]["spokenText"], "hello");
    assert_eq!(output["materialization"]["revision"], 3);
    let (status, script) = json_request(
        &app,
        "POST",
        "/api/v1/tools/read_script",
        json!({ "arguments": { "projectId": project_id } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{script}");
    assert_eq!(script["data"]["transcript"]["revision"], 3);
    assert_eq!(
        script["data"]["transcript"]["utterances"][0]["words"][0]["spokenText"],
        "hello"
    );
    assert_eq!(
        script["data"]["transcript"]["utterances"][0]["speakerId"],
        "speaker_1"
    );
    let materialized_project = state.database.read_project(&project_id).await.unwrap();
    assert_eq!(materialized_project.document.story_sequences.len(), 1);
    assert_eq!(
        materialized_project.document.story_sequences[0].clips[0].word_ids[0].as_str(),
        "word-1"
    );
    let dialogue_item = &materialized_project.document.scenes[0].tracks[0].items[0];
    assert_eq!(
        dialogue_item.link_group_id.as_ref(),
        Some(&materialized_project.document.story_sequences[0].clips[0].link_group_id)
    );
    assert_eq!(dialogue_item.source_range.unwrap().in_ticks, 0);
    assert_eq!(dialogue_item.source_range.unwrap().out_ticks, 240_000);
    assert_eq!(
        materialized_project.document.story_sequences[0].clips[1].timeline_start_ticks,
        240_000
    );

    let (status, validated) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 3,
                "dryRun": true,
                "edit": {
                    "kind": "close_gaps",
                    "thresholdMs": 800,
                    "targetGapMs": 100
                }
            },
            "idempotencyKey": "validate-compress-transcribed-pause"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validated}");
    assert_eq!(
        validated["proposal"]["payload"][0]["type"],
        "closeStoryGaps"
    );
    let proposal_id = validated["proposal"]["proposalId"]
        .as_str()
        .unwrap()
        .to_owned();
    let operations = validated["proposal"]["payload"].clone();
    let (status, compressed) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 3,
                "proposalId": proposal_id,
                "operations": operations,
                "confirm": true
            },
            "idempotencyKey": "apply-compress-transcribed-pause"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{compressed}");
    assert_eq!(compressed["revision"], 4);
    let compressed_document = &compressed["data"]["envelope"]["document"];
    assert_eq!(
        compressed_document["storySequences"][0]["clips"][0]["sourceEndTicks"],
        132_000
    );
    assert_eq!(
        compressed_document["storySequences"][0]["clips"][1]["timelineStartTicks"],
        132_000
    );
    assert_eq!(
        compressed_document["scenes"][0]["tracks"][0]["items"][0]["durationTicks"],
        132_000
    );
    assert_eq!(
        compressed_document["scenes"][0]["tracks"][0]["items"][1]["startTicks"],
        132_000
    );

    let (status, undone) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/undo"),
        json!({
            "expectedRevision": 4,
            "idempotencyKey": "undo-compress-transcribed-pause"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["envelope"]["revision"], 5);
    assert_eq!(
        undone["envelope"]["document"]["storySequences"][0]["clips"][0]["sourceEndTicks"],
        240_000
    );
    assert_eq!(
        undone["envelope"]["document"]["storySequences"][0]["clips"][1]["timelineStartTicks"],
        240_000
    );

    let (status, batch_validated) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 5,
                "dryRun": true,
                "edits": [
                    {
                        "kind": "delete_words",
                        "wordIds": ["word-filler"]
                    },
                    {
                        "kind": "close_gaps",
                        "thresholdMs": 800,
                        "targetGapMs": 100
                    },
                    {
                        "kind": "add_captions",
                        "options": {
                            "presetId": "studio-clean",
                            "wordHighlight": true
                        }
                    }
                ]
            },
            "idempotencyKey": "validate-filler-pause-captions"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{batch_validated}");
    assert_eq!(
        batch_validated["proposal"]["payload"][0]["type"],
        "setTranscriptWordsDeleted"
    );
    assert_eq!(
        batch_validated["proposal"]["payload"][1]["type"],
        "closeStoryGaps"
    );
    assert_eq!(
        batch_validated["proposal"]["payload"][2]["type"],
        "addTrack"
    );
    assert_eq!(
        batch_validated["proposal"]["payload"][3]["type"],
        "insertItem"
    );
    let batch_proposal_id = batch_validated["proposal"]["proposalId"]
        .as_str()
        .unwrap()
        .to_owned();
    let batch_operations = batch_validated["proposal"]["payload"].clone();
    let (status, batch_applied) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 5,
                "proposalId": batch_proposal_id,
                "operations": batch_operations,
                "confirm": true
            },
            "idempotencyKey": "apply-filler-pause-captions"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{batch_applied}");
    assert_eq!(batch_applied["revision"], 6);
    let batch_document = &batch_applied["data"]["envelope"]["document"];
    assert_eq!(
        batch_document["transcripts"][0]["words"][1]["deleted"],
        true
    );
    assert_eq!(
        batch_document["storySequences"][0]["clips"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        batch_document["storySequences"][0]["clips"][2]["timelineStartTicks"],
        108_000
    );
    let tracks = batch_document["scenes"][0]["tracks"].as_array().unwrap();
    let caption_track = tracks
        .iter()
        .find(|track| track["kind"] == "caption")
        .expect("caption track committed in the same revision");
    let caption = &caption_track["items"][0]["content"]["caption"];
    assert_eq!(caption["presetId"], "studio-clean");
    assert_eq!(
        caption["wordIds"],
        json!(["word-1", "word-2", "word-3"]),
        "caption reconciliation removes the deleted filler anchor"
    );
    let dialogue_track = tracks
        .iter()
        .find(|track| track["id"] == "track-dialogue")
        .unwrap();
    assert_eq!(dialogue_track["items"].as_array().unwrap().len(), 3);
    assert_eq!(dialogue_track["items"][0]["durationTicks"], 60_000);
    assert_eq!(dialogue_track["items"][1]["startTicks"], 60_000);
    assert_eq!(dialogue_track["items"][1]["sourceRange"]["inTicks"], 84_000);
    assert_eq!(
        dialogue_track["items"][1]["sourceRange"]["outTicks"],
        132_000
    );
    assert_eq!(dialogue_track["items"][2]["startTicks"], 108_000);

    let (status, batch_undone) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/undo"),
        json!({
            "expectedRevision": 6,
            "idempotencyKey": "undo-filler-pause-captions"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{batch_undone}");
    assert_eq!(batch_undone["envelope"]["revision"], 7);
    assert_eq!(
        batch_undone["envelope"]["document"], undone["envelope"]["document"],
        "one undo restores the exact pre-batch project document"
    );

    let (status, second) = json_request(
        &app,
        "POST",
        "/api/v1/tools/start_transcription",
        json!({
            "arguments": {
                "projectId": project_id,
                "assetId": "asset-audio",
                "expectedRevision": 7,
                "language": "en",
                "engine": "faster-whisper",
                "diarization": true,
                "minSpeakers": 1,
                "maxSpeakers": 2
            },
            "idempotencyKey": "retranscribe-fixture"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let second_job_id = second["jobId"].as_str().unwrap();
    let (status, manual_edit) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "manual-transcript-edit",
            "projectId": project_id,
            "baseRevision": 7,
            "idempotencyKey": "manual-transcript-edit",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{
                "type": "setTranscriptDisplayText",
                "transcriptId": output["materialization"]["transcriptId"],
                "wordId": "word-1",
                "displayText": "Manually corrected"
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{manual_edit}");
    assert_eq!(manual_edit["envelope"]["revision"], 8);
    let failed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = state.database.read_job(second_job_id).await.unwrap();
            if job.state == "failed" {
                break job;
            }
            assert_ne!(job.state, "succeeded", "worker overwrote a manual edit");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        failed.error.as_ref().unwrap()["code"],
        "TRANSCRIPT_MATERIALIZATION_FAILED"
    );
    assert_eq!(failed.output.unwrap()["words"][0]["spokenText"], "hello");
    let (_, preserved) = json_request(
        &app,
        "POST",
        "/api/v1/tools/read_script",
        json!({ "arguments": { "projectId": project_id } }),
    )
    .await;
    assert_eq!(
        preserved["data"]["transcript"]["utterances"][0]["words"][0]["displayText"],
        "Manually corrected"
    );
    state.web_capture.as_ref().unwrap().shutdown().await;
    state.worker.as_ref().unwrap().shutdown().await;
}
