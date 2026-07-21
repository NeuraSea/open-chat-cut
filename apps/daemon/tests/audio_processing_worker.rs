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
                .header(header::AUTHORIZATION, "Bearer audio-test-token")
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
async fn audio_cleanup_materializes_a_reversible_managed_asset_after_concurrent_edits() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("audio-worker.py");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json, sys, time, wave
from pathlib import Path
args = sys.argv[1:]
root = Path(args[args.index("--data-root") + 1])
request = json.load(sys.stdin)
assert request["kind"] == "denoise"
destination = root / request["outputDir"] / f'{request["jobId"]}.wav'
destination.parent.mkdir(parents=True, exist_ok=True)
with wave.open(str(destination), "wb") as output:
    output.setnchannels(1)
    output.setsampwidth(2)
    output.setframerate(48000)
    output.writeframes(b"\x00\x00" * 4800)
print(json.dumps({"jobId": request["jobId"], "type": "progress", "progress": 0.5, "message": "Cleaning"}), flush=True)
time.sleep(0.2)
print(json.dumps({"jobId": request["jobId"], "type": "result", "result": {"derivedAssetPath": str(destination), "reversible": True}}), flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(script);
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "audio-processing-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "audio-test-token".to_owned())
        .await
        .unwrap();
    let source = state
        .layout
        .put_media(b"RIFF\x04\x00\x00\x00WAVE")
        .await
        .unwrap();
    let app = build_app(state.clone());
    let (_, created) = json_request(
        &app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Audio cleanup", "idempotencyKey": "create-audio" }),
    )
    .await;
    let project_id = created["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, committed) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "add-source-audio",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "add-source-audio",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{
                "type": "addAsset",
                "asset": {
                    "id": "asset-source-audio",
                    "name": "Source audio",
                    "kind": "audio",
                    "contentHash": source.sha256,
                    "hasAudio": true,
                    "provenance": { "type": "imported", "sourceName": "source.wav" }
                }
            }]
        }),
    )
    .await;
    assert_eq!(committed["envelope"]["revision"], 1);

    let request = json!({
        "arguments": {
            "projectId": project_id,
            "expectedRevision": 1,
            "assetId": "asset-source-audio",
            "operation": "denoise",
            "options": { "untrustedFilter": "movie=/etc/passwd" }
        },
        "idempotencyKey": "denoise-source-1"
    });
    let (status, queued) =
        json_request(&app, "POST", "/api/v1/tools/process_audio", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    let job_id = queued["jobId"].as_str().unwrap();
    let derived_asset_id = queued["data"]["derivedAssetId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        queued["data"]["job"]["input"]["options"]["filter"],
        "highpass=f=80,afftdn=nf=-25"
    );
    assert_eq!(queued["data"]["job"]["input"]["options"]["engine"], "auto");
    assert!(
        queued["data"]["job"]["input"]["options"]
            .get("untrustedFilter")
            .is_none()
    );

    let (_, concurrent) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "rename-during-audio-cleanup",
            "projectId": project_id,
            "baseRevision": 1,
            "idempotencyKey": "rename-during-audio-cleanup",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{ "type": "setProjectName", "name": "Still editable" }]
        }),
    )
    .await;
    assert_eq!(concurrent["envelope"]["revision"], 2);

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
    let materialization = &completed.output.as_ref().unwrap()["materialization"];
    assert_eq!(materialization["revision"], 3);
    assert_eq!(materialization["asset"]["id"], derived_asset_id);
    assert_eq!(materialization["asset"]["provenance"]["type"], "derived");
    assert_eq!(
        materialization["asset"]["provenance"]["parentAssetId"],
        "asset-source-audio"
    );
    assert_eq!(materialization["asset"]["derivedAudio"]["reversible"], true);
    let derived_hash = materialization["asset"]["contentHash"].as_str().unwrap();
    assert_ne!(derived_hash, source.sha256);
    assert!(
        state
            .layout
            .media_content(derived_hash)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        !state
            .layout
            .root
            .join(format!("derived/audio/{job_id}.wav"))
            .exists()
    );

    let (status, replayed) =
        json_request(&app, "POST", "/api/v1/tools/process_audio", request).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["jobId"], job_id);
    assert_eq!(replayed["data"]["replayed"], true);
    state.web_capture.as_ref().unwrap().shutdown().await;
    state.worker.as_ref().unwrap().shutdown().await;
}
