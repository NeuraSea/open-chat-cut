#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use openchatcut_domain::{AssetKind, AssetProvenance, ProjectDocument, ProjectId, Scene, SceneId};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn local_voice_job_materializes_a_managed_generated_asset() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("fake-voice-worker.py");
    std::fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import json, sys, wave
from pathlib import Path
args = sys.argv[1:]
root = Path(args[args.index('--data-root') + 1])
request = json.load(sys.stdin)
assert request['kind'] == 'synthesize_voice'
destination = root / request['outputDir'] / f"{request['jobId']}.wav"
destination.parent.mkdir(parents=True, exist_ok=True)
with wave.open(str(destination), 'wb') as output:
    output.setnchannels(1)
    output.setsampwidth(2)
    output.setframerate(24000)
    output.writeframes(b'\0\0' * 240)
print(json.dumps({'jobId': request['jobId'], 'type': 'progress', 'progress': 0.75, 'message': 'Synthesized'}), flush=True)
print(json.dumps({'jobId': request['jobId'], 'type': 'result', 'result': {'generatedAssetPath': str(destination), 'engine': 'piper', 'model': 'test-voice', 'mimeType': 'audio/wav'}}), flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(worker);
    std::fs::create_dir_all(config.provider_config.parent().unwrap()).unwrap();
    std::fs::write(
        &config.provider_config,
        serde_json::to_vec(&json!({
            "localVoice": {
                "engine": "piper",
                "modelPath": "models/test-voice.onnx"
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
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "voice-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-daemon-token".to_owned())
        .await
        .unwrap();
    let mut document = ProjectDocument::new(ProjectId::new("voice-project").unwrap(), "Voice");
    let mut scene = Scene::new(SceneId::new("scene-main").unwrap(), "Main");
    scene.is_main = true;
    document.scenes.push(scene);
    document.current_scene_id = Some(SceneId::new("scene-main").unwrap());
    state
        .database
        .create_project(
            document,
            "create-voice-project",
            &json!({ "name": "Voice" }),
        )
        .await
        .unwrap();
    let app = build_app(state.clone());
    let invalid = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tools/generate_asset")
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "idempotencyKey": "voice-generation-invalid-placement",
                        "arguments": {
                            "projectId": "voice-project",
                            "expectedRevision": 0,
                            "kind": "voice",
                            "provider": "local-voice",
                            "prompt": "Do not queue this.",
                            "confirm": true,
                            "options": {
                                "placement": {
                                    "startSeconds": 0,
                                    "durationSeconds": 0
                                }
                            }
                        }
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert!(
        state
            .database
            .list_jobs(Some("voice-project"), 10)
            .await
            .unwrap()
            .is_empty()
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tools/generate_asset")
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "idempotencyKey": "voice-generation-1",
                        "arguments": {
                            "projectId": "voice-project",
                            "expectedRevision": 0,
                            "kind": "voice",
                            "provider": "local-voice",
                            "prompt": "Read this sentence.",
                            "confirm": true,
                            "options": {
                                "speed": 1.1,
                                "placement": {
                                    "startSeconds": 1.5,
                                    "durationSeconds": 2.0,
                                    "name": "Generated narration"
                                }
                            }
                        }
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap();
    let job = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let job = state.database.read_job(job_id).await.unwrap();
            if matches!(job.state.as_str(), "succeeded" | "failed") {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(job.state, "succeeded", "{:?}", job.error);
    let project = state.database.read_project("voice-project").await.unwrap();
    assert_eq!(project.revision, 2);
    let asset = project.document.assets.first().unwrap();
    assert_eq!(asset.kind, AssetKind::Audio);
    assert!(asset.has_audio);
    assert!(matches!(
        asset.provenance,
        AssetProvenance::Generated { ref provider, ref prompt, .. }
            if provider == "local-voice" && prompt == "Read this sentence."
    ));
    assert!(
        state
            .layout
            .media_content(asset.content_hash.as_ref().unwrap().as_str())
            .await
            .unwrap()
            .is_some()
    );
    let placed_items = project
        .document
        .scenes
        .iter()
        .flat_map(|scene| scene.tracks.iter())
        .flat_map(|track| track.items.iter())
        .filter(|item| item.content.asset_id() == Some(&asset.id))
        .collect::<Vec<_>>();
    assert_eq!(placed_items.len(), 1);
    assert_eq!(placed_items[0].start_ticks, 180_000);
    assert_eq!(placed_items[0].duration_ticks, 240_000);
    assert_eq!(placed_items[0].name, "Generated narration");
    assert_eq!(
        placed_items[0].extensions["generatedPlacement"]["jobId"],
        job_id
    );

    state.web_capture.as_ref().unwrap().shutdown().await;
    state.worker.as_ref().unwrap().shutdown().await;
}
