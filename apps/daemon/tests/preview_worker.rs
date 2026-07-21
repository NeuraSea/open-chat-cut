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
                .header(header::AUTHORIZATION, "Bearer preview-test-token")
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
async fn preview_frames_are_persistent_verified_idempotent_and_historically_pinned() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("preview-worker.py");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import base64, hashlib, json, sys, time
from pathlib import Path
args = sys.argv[1:]
root = Path(args[args.index("--data-root") + 1])
request = json.load(sys.stdin)
assert request["kind"] == "render_preview_frames"
assert request["options"]["revision"] == 1
output = root / request["outputDir"]
output.mkdir(parents=True, exist_ok=True)
png = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9WlQZVQAAAAASUVORK5CYII=")
frames = []
print(json.dumps({"jobId": request["jobId"], "type": "progress", "progress": 0.5, "message": "Rendering"}), flush=True)
time.sleep(0.2)
for index, ticks in enumerate(request["options"]["timesTicks"]):
    destination = output / f'{request["jobId"]}-{index:03}.png'
    destination.write_bytes(png)
    frames.append({"path": str(destination), "timeTicks": ticks, "sha256": hashlib.sha256(png).hexdigest()})
print(json.dumps({"jobId": request["jobId"], "type": "result", "result": {
    "documentHash": request["options"]["documentHash"], "frames": frames
}}), flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(script);
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "preview-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "preview-test-token".to_owned())
        .await
        .unwrap();
    let app = build_app(state.clone());
    let (_, created) = json_request(
        &app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Preview", "idempotencyKey": "create-preview" }),
    )
    .await;
    let project_id = created["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, timeline) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "build-preview-timeline",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "build-preview-timeline",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{
                "type": "replaceSceneGraph",
                "currentSceneId": "scene-main",
                "scenes": [{
                    "id": "scene-main",
                    "name": "Main",
                    "isMain": true,
                    "tracks": [{
                        "id": "track-text",
                        "name": "Text",
                        "kind": "text",
                        "muted": false,
                        "hidden": false,
                        "locked": false,
                        "items": [{
                            "id": "item-text",
                            "name": "Title",
                            "startTicks": 0,
                            "durationTicks": 120000,
                            "enabled": true,
                            "content": { "type": "text", "text": "Pinned" }
                        }]
                    }],
                    "bookmarks": []
                }]
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{timeline}");
    assert_eq!(timeline["envelope"]["revision"], 1);

    let (status, validation) = json_request(
        &app,
        "POST",
        "/api/v1/tools/validate_project",
        json!({
            "arguments": {
                "projectId": project_id,
                "revision": 1,
                "target": "mp4"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validation}");
    assert_eq!(validation["data"]["report"]["valid"], true);
    assert_eq!(
        validation["data"]["report"]["renderer"],
        "headless-scene-graph-v1"
    );

    let request = json!({
        "arguments": {
            "projectId": project_id,
            "revision": 1,
            "timesSeconds": [0.25],
            "width": 320
        }
    });
    let (status, queued) = json_request(
        &app,
        "POST",
        "/api/v1/tools/render_preview_frames",
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    assert_eq!(queued["data"]["pinnedRevision"], 1);
    let job_id = queued["jobId"].as_str().unwrap().to_owned();

    // Advance the project while Chromium is rendering. The immutable revision
    // remains valid and the preview job must not be converted into a conflict.
    let (status, renamed) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "rename-during-preview",
            "projectId": project_id,
            "baseRevision": 1,
            "idempotencyKey": "rename-during-preview",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{ "type": "setProjectName", "name": "New head" }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["envelope"]["revision"], 2);

    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = state.database.read_job(&job_id).await.unwrap();
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
    let output = completed.output.unwrap();
    assert_eq!(output["revision"], 1);
    assert_eq!(output["frames"].as_array().unwrap().len(), 1);
    assert_eq!(output["frames"][0]["width"], 1);
    assert_eq!(output["frames"][0]["height"], 1);

    let (status, replayed) =
        json_request(&app, "POST", "/api/v1/tools/render_preview_frames", request).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["jobId"], job_id);
    assert_eq!(replayed["data"]["replayed"], true);
    state.web_capture.as_ref().unwrap().shutdown().await;
    state.worker.as_ref().unwrap().shutdown().await;
}
