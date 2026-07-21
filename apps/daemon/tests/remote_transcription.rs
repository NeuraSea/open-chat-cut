#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    http::{HeaderMap, Request, StatusCode, header},
    routing::post,
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tower::ServiceExt;

async fn json_request(app: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer remote-asr-test-token")
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
async fn auto_transcription_uses_private_new_api_asr_and_materializes_aligned_words() {
    let temp = tempfile::tempdir().unwrap();
    let (capture_sender, mut capture_receiver) = mpsc::unbounded_channel();
    let capture_sender = Arc::new(Mutex::new(capture_sender));
    let fake_asr = Router::new().route(
        "/v1/audio/transcriptions",
        post({
            let capture_sender = capture_sender.clone();
            move |headers: HeaderMap, body: Bytes| {
                let capture_sender = capture_sender.clone();
                async move {
                    capture_sender
                        .lock()
                        .await
                        .send((headers, body.to_vec()))
                        .unwrap();
                    Json(json!({
                        "language": "en",
                        "segments": [{ "start": 0.0, "end": 1.1, "text": "Open Chat Cut" }],
                        "words": [
                            { "word": "Open", "start": 0.0, "end": 0.3 },
                            { "word": " Chat", "start": 0.35, "end": 0.65 },
                            { "word": " Cut", "start": 0.7, "end": 1.05 }
                        ]
                    }))
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let asr_task = tokio::spawn(async move {
        axum::serve(listener, fake_asr).await.unwrap();
    });

    let mut config = Config::for_test(temp.path().to_owned());
    std::fs::create_dir_all(config.provider_config.parent().unwrap()).unwrap();
    std::fs::write(
        &config.provider_config,
        serde_json::to_vec_pretty(&json!({
            "newApiAsr": {
                "baseUrl": format!("http://{address}/v1"),
                "apiKey": "private-asr-test-key",
                "defaultModel": "occ-asr",
                "submitPath": "audio/transcriptions",
                "allowPrivateBaseUrl": true
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(
        &config.provider_config,
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let worker = temp.path().join("worker.py");
    std::fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import json, sys
if "--capabilities" in sys.argv:
    print(json.dumps({"schemaVersion": 1, "platform": {"system": "test", "machine": "test"}, "ffmpegAvailable": True, "videoEncoding": {"requested": "cpu", "selected": "cpu", "accelerated": False, "fallbackReason": None, "adapters": []}}))
    raise SystemExit(0)
raise SystemExit(91)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    config.media_worker = Some(worker);
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "remote-asr-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "remote-asr-test-token".to_owned())
        .await
        .unwrap();
    let source_bytes = b"remote transcription fixture";
    let stored = state.layout.put_media(source_bytes).await.unwrap();
    let app = build_app(state.clone());

    let (_, created) = json_request(
        &app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Remote transcript", "idempotencyKey": "create-remote-asr" }),
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
            "transactionId": "add-remote-asr-audio",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "add-remote-asr-audio",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{
                "type": "addAsset",
                "asset": {
                    "id": "asset-remote-asr",
                    "name": "Private ASR fixture",
                    "kind": "audio",
                    "contentHash": stored.sha256,
                    "hasAudio": true,
                    "durationTicks": 240000,
                    "provenance": { "type": "imported", "sourceName": "fixture.wav" }
                }
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{committed}");

    let (status, queued) = json_request(
        &app,
        "POST",
        "/api/v1/tools/start_transcription",
        json!({
            "arguments": {
                "projectId": project_id,
                "assetId": "asset-remote-asr",
                "expectedRevision": 1,
                "language": "en",
                "engine": "auto"
            },
            "idempotencyKey": "transcribe-private-asr"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    assert_eq!(
        queued["data"]["job"]["input"]["options"]["engine"],
        "new-api-asr"
    );
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
    assert_eq!(
        completed.output.as_ref().unwrap()["words"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let materialized = state.database.read_project(&project_id).await.unwrap();
    let transcript = &materialized.document.transcripts[0];
    assert_eq!(transcript.words[0].spoken_text, "Open");
    assert_eq!(transcript.words[2].end_ticks, 126_000);
    assert_eq!(
        transcript.extensions["transcriptionEngine"]["provider"],
        "new-api-asr"
    );

    let (headers, body) = tokio::time::timeout(Duration::from_secs(1), capture_receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        headers.get(header::AUTHORIZATION).unwrap(),
        "Bearer private-asr-test-key"
    );
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("multipart/form-data; boundary=openchatcut-")
    );
    assert!(
        body.windows(b"occ-asr".len())
            .any(|window| window == b"occ-asr")
    );
    assert!(
        body.windows(source_bytes.len())
            .any(|window| window == source_bytes)
    );

    state.worker.as_ref().unwrap().shutdown().await;
    asr_task.abort();
}
