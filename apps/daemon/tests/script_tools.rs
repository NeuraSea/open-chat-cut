use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

async fn app() -> (axum::Router, TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "script-tools-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-token".to_owned())
        .await
        .unwrap();
    (build_app(state), temp)
}

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
                .header(header::AUTHORIZATION, "Bearer test-token")
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

async fn project_with_transcript(app: &axum::Router) -> String {
    let (_, created) = json_request(
        app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Script", "idempotencyKey": "create-script" }),
    )
    .await;
    let project_id = created["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, seeded) = json_request(
        app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "seed-transcript",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "seed-transcript",
            "actor": { "kind": "system" },
            "operations": [{
                "type": "upsertTranscript",
                "transcript": {
                    "id": "transcript-1",
                    "language": "en",
                    "speakers": [{ "id": "speaker-1", "label": "Speaker" }],
                    "words": [
                        {
                            "id": "word-1",
                            "spokenText": "hello",
                            "displayText": "Hello",
                            "startTicks": 0,
                            "endTicks": 60000,
                            "speakerId": "speaker-1"
                        },
                        {
                            "id": "word-2",
                            "spokenText": "um",
                            "displayText": "um",
                            "startTicks": 60000,
                            "endTicks": 120000,
                            "speakerId": "speaker-1"
                        },
                        {
                            "id": "word-3",
                            "spokenText": "world",
                            "displayText": "world",
                            "startTicks": 120000,
                            "endTicks": 180000,
                            "speakerId": "speaker-1"
                        }
                    ],
                    "segments": [{
                        "id": "utterance-1",
                        "wordIds": ["word-1", "word-2", "word-3"],
                        "speakerId": "speaker-1"
                    }]
                }
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{seeded}");
    assert_eq!(seeded["envelope"]["revision"], 1);
    project_id
}

async fn project_with_reorderable_story(app: &axum::Router) -> String {
    let (_, created) = json_request(
        app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Reorder", "idempotencyKey": "create-reorder" }),
    )
    .await;
    let project_id = created["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, seeded) = json_request(
        app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "seed-reorder-story",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "seed-reorder-story",
            "actor": { "kind": "system" },
            "operations": [
                {
                    "type": "addAsset",
                    "asset": {
                        "id": "asset-reorder",
                        "name": "Dialogue.wav",
                        "kind": "audio",
                        "durationTicks": 180000,
                        "provenance": { "type": "imported" }
                    }
                },
                {
                    "type": "addScene",
                    "scene": {
                        "id": "scene-reorder",
                        "name": "Main",
                        "isMain": true,
                        "tracks": [{
                            "id": "track-reorder",
                            "name": "Dialogue",
                            "kind": "audio",
                            "items": [
                                {
                                    "id": "item-first",
                                    "name": "First",
                                    "startTicks": 0,
                                    "durationTicks": 120000,
                                    "sourceRange": { "inTicks": 0, "outTicks": 120000 },
                                    "sourceDurationTicks": 180000,
                                    "linkGroupId": "link-first",
                                    "enabled": true,
                                    "content": {
                                        "type": "media",
                                        "assetId": "asset-reorder",
                                        "mediaKind": "audio"
                                    }
                                },
                                {
                                    "id": "item-second",
                                    "name": "Second",
                                    "startTicks": 120000,
                                    "durationTicks": 60000,
                                    "sourceRange": { "inTicks": 120000, "outTicks": 180000 },
                                    "sourceDurationTicks": 180000,
                                    "linkGroupId": "link-second",
                                    "enabled": true,
                                    "content": {
                                        "type": "media",
                                        "assetId": "asset-reorder",
                                        "mediaKind": "audio"
                                    }
                                }
                            ]
                        }]
                    }
                },
                {
                    "type": "upsertTranscript",
                    "transcript": {
                        "id": "transcript-reorder",
                        "assetId": "asset-reorder",
                        "language": "en",
                        "words": [
                            { "id": "word-first", "spokenText": "first", "displayText": "First", "startTicks": 0, "endTicks": 60000 },
                            { "id": "word-phrase", "spokenText": "phrase", "displayText": "phrase", "startTicks": 60000, "endTicks": 120000 },
                            { "id": "word-second", "spokenText": "second", "displayText": "Second", "startTicks": 120000, "endTicks": 180000 }
                        ],
                        "segments": [
                            { "id": "utterance-first", "wordIds": ["word-first", "word-phrase"] },
                            { "id": "utterance-second", "wordIds": ["word-second"] }
                        ]
                    }
                },
                {
                    "type": "upsertStorySequence",
                    "sequence": {
                        "id": "story-reorder",
                        "transcriptId": "transcript-reorder",
                        "clips": [
                            {
                                "id": "clip-first",
                                "wordIds": ["word-first", "word-phrase"],
                                "timelineStartTicks": 0,
                                "sourceStartTicks": 0,
                                "sourceEndTicks": 120000,
                                "linkGroupId": "link-first"
                            },
                            {
                                "id": "clip-second",
                                "wordIds": ["word-second"],
                                "timelineStartTicks": 120000,
                                "sourceStartTicks": 120000,
                                "sourceEndTicks": 180000,
                                "linkGroupId": "link-second"
                            }
                        ]
                    }
                }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{seeded}");
    project_id
}

#[tokio::test]
async fn read_and_reviewed_script_edit_use_domain_transcript_revision() {
    let (app, _temp) = app().await;
    let project_id = project_with_transcript(&app).await;
    let (status, script) = json_request(
        &app,
        "POST",
        "/api/v1/tools/read_script",
        json!({
            "arguments": {
                "projectId": project_id,
                "includeSuggestions": true
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{script}");
    assert_eq!(script["data"]["transcript"]["revision"], 1);
    assert_eq!(
        script["data"]["transcript"]["utterances"][0]["words"][0]["startMs"],
        0
    );
    assert_eq!(
        script["data"]["transcript"]["utterances"][0]["words"][1]["endMs"],
        1000
    );
    assert_eq!(
        script["data"]["domainTranscript"]["words"][1]["spokenText"],
        "um"
    );
    assert_eq!(
        script["data"]["cleanupAnalysis"]["summary"]["fillerCount"],
        1
    );
    assert_eq!(
        script["data"]["cleanupAnalysis"]["suggestions"][0]["wordIds"],
        json!(["word-2"])
    );
    assert_eq!(
        script["data"]["cleanupAnalysis"]["suggestions"][0]["recommended"],
        true
    );

    let (status, validated) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 1,
                "dryRun": true,
                "edit": {
                    "kind": "auto_cleanup"
                }
            },
            "idempotencyKey": "validate-delete-filler"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validated}");
    let proposal_id = validated["proposal"]["proposalId"]
        .as_str()
        .unwrap()
        .to_owned();
    let operations = validated["proposal"]["payload"].clone();
    assert_eq!(operations[0]["type"], "setTranscriptWordsDeleted");
    assert_eq!(validated["proposal"]["warnings"][0]["severity"], "danger");

    let (status, applied) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 1,
                "proposalId": proposal_id,
                "operations": operations,
                "confirm": true
            },
            "idempotencyKey": "apply-delete-filler"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["revision"], 2);

    let (_, active_script) = json_request(
        &app,
        "POST",
        "/api/v1/tools/read_script",
        json!({ "arguments": { "projectId": project_id } }),
    )
    .await;
    assert_eq!(
        active_script["data"]["transcript"]["utterances"][0]["words"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (_, complete_script) = json_request(
        &app,
        "POST",
        "/api/v1/tools/read_script",
        json!({
            "arguments": { "projectId": project_id, "includeDeleted": true }
        }),
    )
    .await;
    assert_eq!(
        complete_script["data"]["transcript"]["utterances"][0]["words"][1]["deleted"],
        true
    );
    assert_eq!(
        complete_script["data"]["transcript"]["utterances"][0]["words"][1]["spokenText"],
        "um"
    );
}

#[tokio::test]
async fn script_reorder_moves_transcript_story_and_linked_timeline_as_one_revision() {
    let (app, _temp) = app().await;
    let project_id = project_with_reorderable_story(&app).await;
    let (status, validated) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 1,
                "dryRun": true,
                "edit": {
                    "kind": "reorder_words",
                    "utteranceIds": ["utterance-second", "utterance-first"]
                }
            },
            "idempotencyKey": "validate-reorder-story"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validated}");
    assert_eq!(
        validated["proposal"]["payload"][0]["type"],
        "reorderTranscriptSegments"
    );
    assert_eq!(
        validated["proposal"]["payload"][1]["type"],
        "reorderStoryClips"
    );
    let proposal_id = validated["proposal"]["proposalId"]
        .as_str()
        .unwrap()
        .to_owned();
    let operations = validated["proposal"]["payload"].clone();

    let (status, applied) = json_request(
        &app,
        "POST",
        "/api/v1/tools/apply_script_edit",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 1,
                "proposalId": proposal_id,
                "operations": operations,
                "confirm": true
            },
            "idempotencyKey": "apply-reorder-story"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["revision"], 2);
    let document = &applied["data"]["envelope"]["document"];
    assert_eq!(
        document["transcripts"][0]["segments"][0]["id"],
        "utterance-second"
    );
    assert_eq!(
        document["storySequences"][0]["clips"][0]["id"],
        "clip-second"
    );
    assert_eq!(
        document["storySequences"][0]["clips"][1]["timelineStartTicks"],
        60000
    );
    let items = document["scenes"][0]["tracks"][0]["items"]
        .as_array()
        .unwrap();
    let first = items
        .iter()
        .find(|item| item["id"] == "item-first")
        .unwrap();
    let second = items
        .iter()
        .find(|item| item["id"] == "item-second")
        .unwrap();
    assert_eq!(second["startTicks"], 0);
    assert_eq!(first["startTicks"], 60000);
    assert_eq!(first["sourceRange"]["inTicks"], 0);
    assert_eq!(second["sourceRange"]["inTicks"], 120000);

    let (_, script) = json_request(
        &app,
        "POST",
        "/api/v1/tools/read_script",
        json!({ "arguments": { "projectId": project_id } }),
    )
    .await;
    assert_eq!(
        script["data"]["transcript"]["utterances"][0]["id"],
        "utterance-second"
    );

    let (status, undone) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/undo"),
        json!({
            "expectedRevision": 2,
            "idempotencyKey": "undo-reorder-story"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["envelope"]["revision"], 3);
    assert_eq!(
        undone["envelope"]["document"]["transcripts"][0]["segments"][0]["id"],
        "utterance-first"
    );
    assert_eq!(
        undone["envelope"]["document"]["storySequences"][0]["clips"][0]["id"],
        "clip-first"
    );
    assert_eq!(
        undone["envelope"]["document"]["scenes"][0]["tracks"][0]["items"][0]["startTicks"],
        0
    );
}
