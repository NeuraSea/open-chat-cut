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
                .header(header::AUTHORIZATION, "Bearer export-test-token")
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
async fn export_is_revision_pinned_persistent_atomic_and_idempotent() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("export-worker.py");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json, sys
from pathlib import Path
args = sys.argv[1:]
root = Path(args[args.index("--data-root") + 1])
request = json.load(sys.stdin)
assert request["kind"] == "export"
assert request["options"]["plan"]["renderer"] == "ffmpeg-single-source-v1"
destination = root / request["outputDir"] / request["options"]["outputFileName"]
temporary = destination.with_suffix(destination.suffix + ".part")
temporary.write_bytes(b"\x00\x00\x00\x18ftypisomrevision-pinned-export")
temporary.replace(destination)
print(json.dumps({"jobId": request["jobId"], "type": "progress", "progress": 0.75, "message": "Encoding"}), flush=True)
print(json.dumps({"jobId": request["jobId"], "type": "result", "result": {"outputPath": str(destination), "renderer": request["options"]["plan"]["renderer"]}}), flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(script);
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "export-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "export-test-token".to_owned())
        .await
        .unwrap();
    let stored = state.layout.put_media(b"fake video fixture").await.unwrap();
    let app = build_app(state.clone());
    let (_, created) = json_request(
        &app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Export", "idempotencyKey": "create-export" }),
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
            "transactionId": "build-export-timeline",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "build-export-timeline",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [
                {
                    "type": "addAsset",
                    "asset": {
                        "id": "asset-video",
                        "name": "Fixture video",
                        "kind": "video",
                        "contentHash": stored.sha256,
                        "durationTicks": 3600000,
                        "width": 1920,
                        "height": 1080,
                        "hasAudio": true,
                        "provenance": { "type": "imported", "sourceName": "fixture.mp4" }
                    }
                },
                {
                    "type": "replaceSceneGraph",
                    "currentSceneId": "scene-main",
                    "scenes": [{
                        "id": "scene-main",
                        "name": "Main",
                        "isMain": true,
                        "tracks": [{
                            "id": "track-video",
                            "name": "Video",
                            "kind": "video",
                            "muted": false,
                            "hidden": false,
                            "locked": false,
                            "items": [{
                                "id": "item-video",
                                "name": "Fixture video",
                                "startTicks": 0,
                                "durationTicks": 3600000,
                                "enabled": true,
                                "content": {
                                    "type": "media",
                                    "assetId": "asset-video",
                                    "mediaKind": "video"
                                }
                            }]
                        }],
                        "bookmarks": []
                    }]
                }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{committed}");
    assert_eq!(committed["envelope"]["revision"], 1);

    // Export is read-only and is pinned to an immutable historical envelope.
    // A concurrent editor commit must not force it to follow the newer head or
    // fail with a revision conflict.
    let (status, advanced) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "advance-after-export-plan",
            "projectId": project_id,
            "baseRevision": 1,
            "idempotencyKey": "advance-after-export-plan",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{
                "type": "setProjectName",
                "name": "Newer editor head"
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{advanced}");
    assert_eq!(advanced["envelope"]["revision"], 2);

    let request = json!({
        "arguments": {
            "projectId": project_id,
            "expectedRevision": 1,
            "format": "mp4",
            "outputPath": "fixture-1080p30.mp4",
            "allowOverwrite": false,
            "settings": {
                "resolution": { "width": 1920, "height": 1080 },
                "fps": 30,
                "range": { "startSeconds": 0, "endSeconds": 30 }
            }
        },
        "idempotencyKey": "export-fixture-1"
    });
    let (status, queued) =
        json_request(&app, "POST", "/api/v1/tools/start_export", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    assert_eq!(queued["data"]["pinnedRevision"], 1);
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
    assert_eq!(completed.revision, Some(1));
    assert_eq!(
        tokio::fs::read(state.layout.exports.join("fixture-1080p30.mp4"))
            .await
            .unwrap(),
        b"\x00\x00\x00\x18ftypisomrevision-pinned-export"
    );

    let artifact = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/jobs/{job_id}/artifact"))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer export-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(artifact.status(), StatusCode::OK);
    assert_eq!(artifact.headers()[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(
        artifact.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"fixture-1080p30.mp4\""
    );
    assert_eq!(
        artifact.into_body().collect().await.unwrap().to_bytes(),
        &b"\x00\x00\x00\x18ftypisomrevision-pinned-export"[..]
    );

    // A retry after the output exists must return the original receipt instead
    // of being rejected by the no-overwrite guard.
    let (status, replayed) =
        json_request(&app, "POST", "/api/v1/tools/start_export", request).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["jobId"], job_id);
    assert_eq!(replayed["data"]["replayed"], true);

    let (status, collision) = json_request(
        &app,
        "POST",
        "/api/v1/tools/start_export",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 1,
                "format": "mp4",
                "outputPath": "fixture-1080p30.mp4"
            },
            "idempotencyKey": "export-fixture-collision"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{collision}");
    assert_eq!(collision["error"]["code"], "export_output_exists");

    let (status, traversal) = json_request(
        &app,
        "POST",
        "/api/v1/tools/start_export",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 1,
                "format": "mp4",
                "outputPath": "../escaped.mp4"
            },
            "idempotencyKey": "export-path-traversal"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{traversal}");
    assert_eq!(traversal["error"]["code"], "invalid_export_output_path");
    assert!(!temp.path().join("escaped.mp4").exists());

    // A completed receipt is not enough to authorize an unsafe filesystem
    // replacement. Download reopens the expected export without following a
    // symlink and rejects it even though the persisted output was verified.
    let artifact_path = state.layout.exports.join("fixture-1080p30.mp4");
    std::fs::remove_file(&artifact_path).unwrap();
    std::os::unix::fs::symlink(temp.path().join("outside.mp4"), &artifact_path).unwrap();
    let unsafe_artifact = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/jobs/{job_id}/artifact"))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer export-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsafe_artifact.status(), StatusCode::CONFLICT);
    state.web_capture.as_ref().unwrap().shutdown().await;
    state.worker.as_ref().unwrap().shutdown().await;
}
