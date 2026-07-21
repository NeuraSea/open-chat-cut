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

async fn json_request(app: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer timeline-audio-token")
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
async fn multi_clip_wav_uses_revision_pinned_timeline_audio_worker() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("timeline-audio-worker.py");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json, sys
from pathlib import Path
args = sys.argv[1:]
root = Path(args[args.index("--data-root") + 1])
request = json.load(sys.stdin)
assert request["kind"] == "timeline_audio_export"
plan = request["options"]["plan"]
assert plan["renderer"] == "ffmpeg-timeline-audio-v1"
assert plan["format"] == "wav"
assert len(request["options"]["audioInputs"]) == 2
destination = root / request["outputDir"] / request["options"]["outputFileName"]
destination.write_bytes(b"RIFF\x18\x00\x00\x00WAVEfmt revision-pinned-timeline-audio")
print(json.dumps({"jobId": request["jobId"], "type": "result", "result": {
  "outputPath": str(destination), "renderer": plan["renderer"],
  "revision": request["options"]["revision"], "audioSourceCount": 2
}}), flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(script);
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "timeline-audio-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "timeline-audio-token".to_owned())
        .await
        .unwrap();
    let stored = state.layout.put_media(b"fake audio fixture").await.unwrap();
    let app = build_app(state.clone());
    let (_, created) = json_request(
        &app,
        "/api/v1/projects",
        json!({ "name": "Audio export", "idempotencyKey": "create-audio-export" }),
    )
    .await;
    let project_id = created["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, committed) = json_request(
        &app,
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "build-audio-timeline",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "build-audio-timeline",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [
                { "type": "addAsset", "asset": {
                    "id": "dialogue-a", "name": "Dialogue A", "kind": "audio",
                    "contentHash": stored.sha256, "durationTicks": 240000,
                    "hasAudio": true,
                    "provenance": { "type": "imported", "sourceName": "a.wav" }
                }},
                { "type": "addAsset", "asset": {
                    "id": "dialogue-b", "name": "Dialogue B", "kind": "audio",
                    "contentHash": stored.sha256, "durationTicks": 240000,
                    "hasAudio": true,
                    "provenance": { "type": "imported", "sourceName": "b.wav" }
                }},
                { "type": "replaceSceneGraph", "currentSceneId": "scene-main", "scenes": [{
                    "id": "scene-main", "name": "Main", "isMain": true,
                    "tracks": [{
                        "id": "track-dialogue", "name": "Dialogue", "kind": "audio",
                        "muted": false, "hidden": false, "locked": false,
                        "items": [
                            { "id": "clip-a", "name": "A", "startTicks": 0,
                              "durationTicks": 120000, "enabled": true,
                              "content": { "type": "media", "assetId": "dialogue-a", "mediaKind": "audio" } },
                            { "id": "clip-b", "name": "B", "startTicks": 120000,
                              "durationTicks": 120000, "enabled": true,
                              "content": { "type": "media", "assetId": "dialogue-b", "mediaKind": "audio" } }
                        ]
                    }],
                    "bookmarks": []
                }]}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{committed}");

    let (status, queued) = json_request(
        &app,
        "/api/v1/tools/start_export",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 1,
                "format": "wav",
                "outputPath": "dialogue-mix.wav"
            },
            "idempotencyKey": "timeline-audio-export-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    assert_eq!(queued["data"]["renderer"], "ffmpeg-timeline-audio-v1");
    let job_id = queued["jobId"].as_str().unwrap();
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
    assert_eq!(completed.kind, "timeline_audio_export");
    assert_eq!(completed.output.unwrap()["verified"], true);

    state.web_capture.as_ref().unwrap().shutdown().await;
    state.worker.as_ref().unwrap().shutdown().await;
}
