use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use openchatcut_domain::{AssetKind, ProjectDocument, ProjectId, Scene, SceneId};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tower::ServiceExt;

#[derive(Clone)]
struct FakeProvider {
    submit_attempts: Arc<AtomicUsize>,
    base_url: String,
    mode: &'static str,
    complete: Arc<AtomicBool>,
}

#[derive(Clone)]
struct FakeVoiceProvider {
    calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct FakeImageProvider {
    calls: Arc<AtomicUsize>,
}

fn fixture_png() -> Vec<u8> {
    hex::decode("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360f8cfc000000301010018dd8db10000000049454e44ae426082").unwrap()
}

async fn generate_image(
    State(state): State<FakeImageProvider>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        headers.get(header::AUTHORIZATION).unwrap(),
        "Bearer private-image-key"
    );
    assert_eq!(payload["model"], "occ-image");
    assert_eq!(payload["response_format"], "b64_json");
    Json(json!({
        "data": [{ "b64_json": BASE64_STANDARD.encode(fixture_png()) }]
    }))
}

fn fixture_wav() -> Vec<u8> {
    let sample_rate = 24_000_u32;
    let frames = sample_rate;
    let data_size = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00");
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.resize(44 + data_size as usize, 0);
    bytes
}

async fn synthesize_voice(
    State(state): State<FakeVoiceProvider>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        headers.get(header::AUTHORIZATION).unwrap(),
        "Bearer private-voice-key"
    );
    assert!(headers.get("idempotency-key").is_some());
    assert_eq!(payload["model"], "occ-tts");
    assert_eq!(payload["input"], "Read the private voice fixture.");
    assert_eq!(payload["response_format"], "wav");
    ([(header::CONTENT_TYPE, "audio/wav")], fixture_wav())
}

async fn submit_generation(
    State(state): State<FakeProvider>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    assert!(headers.get("idempotency-key").is_some());
    assert!(payload.get("placement").is_none());
    let attempt = state.submit_attempts.fetch_add(1, Ordering::SeqCst);
    if state.mode == "auth" {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "bad credential" })),
        );
    }
    if state.mode == "timeout" {
        return (
            StatusCode::REQUEST_TIMEOUT,
            Json(json!({ "error": "provider request timed out" })),
        );
    }
    if state.mode == "rate" && attempt < 2 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate limited" })),
        );
    }
    (StatusCode::OK, Json(json!({ "id": "remote-1" })))
}

async fn poll_generation(State(state): State<FakeProvider>) -> Json<Value> {
    if state.mode == "pending" && !state.complete.load(Ordering::SeqCst) {
        return Json(json!({
            "status": "processing",
            "progress": 0.2,
            "pollAfterMs": 250,
        }));
    }
    Json(json!({
        "status": "succeeded",
        "outputs": [{ "url": format!("{}/generated.mp4", state.base_url) }],
    }))
}

async fn generated_mp4() -> impl IntoResponse {
    let bytes = hex::decode("000000186674797069736f6d0000020069736f6d69736f32").unwrap();
    ([(header::CONTENT_TYPE, "video/mp4")], bytes)
}

fn fake_media_worker_path(root: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        root.join("fake-media-worker.cmd")
    }
    #[cfg(not(windows))]
    {
        root.join("fake-media-worker.py")
    }
}

fn install_fake_media_worker(root: &Path) -> PathBuf {
    let python_path = root.join("fake-media-worker.py");
    fs::write(
        &python_path,
        r#"#!/usr/bin/env python3
import json
import pathlib
import sys
import time
import wave

request = json.loads(sys.stdin.read())
job_id = request["jobId"]
if request.get("kind") != "normalize_generated_media":
    print(json.dumps({
        "jobId": job_id,
        "type": "error",
        "error": {"code": "UNSUPPORTED_TEST_JOB", "message": "fixture only normalizes provider media"},
    }), flush=True)
    raise SystemExit(0)

root = pathlib.Path(sys.argv[sys.argv.index("--data-root") + 1])
block = root / "tmp" / "block-provider-normalization"
started = root / "tmp" / "provider-normalizer-started"
started.parent.mkdir(parents=True, exist_ok=True)
started.write_text(job_id, encoding="utf-8")
while block.exists():
    time.sleep(0.05)
requested_kind = request.get("options", {}).get("requestedKind")
extension = ".wav" if requested_kind == "voice" else ".png" if requested_kind == "image" else ".mp4"
output = root / "derived" / "provider-normalized" / (job_id + extension)
output.parent.mkdir(parents=True, exist_ok=True)
if requested_kind == "voice":
    with wave.open(str(output), "wb") as stream:
        stream.setnchannels(1)
        stream.setsampwidth(2)
        stream.setframerate(48000)
        stream.writeframes(b"\x00\x00" * 4800)
elif requested_kind == "image":
    output.write_bytes(bytes.fromhex("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360f8cfc000000301010018dd8db10000000049454e44ae426082"))
else:
    output.write_bytes(bytes.fromhex("000000186674797069736f6d0000020069736f6d69736f32"))
print(json.dumps({"jobId": job_id, "type": "progress", "progress": 0.5}), flush=True)
print(json.dumps({
    "jobId": job_id,
    "type": "result",
    "result": {
        "normalizedPath": str(output.resolve()),
        "requestedKind": requested_kind,
        "mimeType": "audio/wav" if requested_kind == "voice" else "image/png" if requested_kind == "image" else "video/mp4",
        "normalization": "ffmpeg-pcm-s24le-48k-v1" if requested_kind == "voice" else "ffmpeg-png-v1" if requested_kind == "image" else "ffmpeg-h264-aac-v1",
        "width": 1 if requested_kind == "image" else None,
        "height": 1 if requested_kind == "image" else None,
        "hasAudio": requested_kind == "voice",
    },
}), flush=True)
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&python_path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    fs::write(
        fake_media_worker_path(root),
        "@echo off\r\npython \"%~dp0fake-media-worker.py\" %*\r\n",
    )
    .unwrap();
    fake_media_worker_path(root)
}

async fn shutdown_state(state: &AppState) {
    if let Some(provider) = &state.provider {
        provider.shutdown().await;
    }
    if let Some(web_capture) = &state.web_capture {
        web_capture.shutdown().await;
    }
    if let Some(worker) = &state.worker {
        worker.shutdown().await;
    }
}

async fn configured_app(
    mode: &'static str,
) -> (
    axum::Router,
    AppState,
    TempDir,
    tokio::task::JoinHandle<()>,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}");
    let attempts = Arc::new(AtomicUsize::new(0));
    let complete = Arc::new(AtomicBool::new(false));
    let fake = FakeProvider {
        submit_attempts: attempts.clone(),
        base_url: base_url.clone(),
        mode,
        complete: complete.clone(),
    };
    let provider_app = Router::new()
        .route("/v1/tasks", post(submit_generation))
        .route("/v1/tasks/remote-1", get(poll_generation))
        .route("/generated.mp4", get(generated_mp4))
        .with_state(fake);
    let server = tokio::spawn(async move {
        axum::serve(listener, provider_app).await.unwrap();
    });

    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(install_fake_media_worker(temp.path()));
    std::fs::create_dir_all(config.provider_config.parent().unwrap()).unwrap();
    std::fs::write(
        &config.provider_config,
        serde_json::to_vec_pretty(&json!({
            "seedanceCompatible": {
                "baseUrl": format!("{base_url}/v1"),
                "apiKey": "test-provider-key",
                "defaultModel": "test-video-model",
                "allowPrivateBaseUrl": true
            }
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &config.provider_config,
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "provider-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-daemon-token".to_owned())
        .await
        .unwrap();
    let mut document =
        ProjectDocument::new(ProjectId::new("provider-project").unwrap(), "Provider");
    let scene_id = SceneId::new("scene-main").unwrap();
    let mut scene = Scene::new(scene_id.clone(), "Main");
    scene.is_main = true;
    document.current_scene_id = Some(scene_id);
    document.scenes.push(scene);
    state
        .database
        .create_project(
            document,
            "create-provider-project",
            &json!({ "name": "Provider" }),
        )
        .await
        .unwrap();
    (
        build_app(state.clone()),
        state,
        temp,
        server,
        attempts,
        complete,
    )
}

async fn configured_voice_app() -> (
    axum::Router,
    AppState,
    TempDir,
    tokio::task::JoinHandle<()>,
    Arc<AtomicUsize>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = calls.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/audio/speech", post(synthesize_voice))
                .with_state(FakeVoiceProvider {
                    calls: server_calls,
                }),
        )
        .await
        .unwrap();
    });

    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(install_fake_media_worker(temp.path()));
    std::fs::create_dir_all(config.provider_config.parent().unwrap()).unwrap();
    std::fs::write(
        &config.provider_config,
        serde_json::to_vec_pretty(&json!({
            "newApiVoice": {
                "baseUrl": format!("http://{address}/v1"),
                "apiKey": "private-voice-key",
                "defaultModel": "occ-tts",
                "submitPath": "audio/speech",
                "allowPrivateBaseUrl": true
            }
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &config.provider_config,
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    let state = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "private-voice-test".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "test-daemon-token".to_owned(),
    )
    .await
    .unwrap();
    let mut document =
        ProjectDocument::new(ProjectId::new("provider-project").unwrap(), "Provider");
    let scene_id = SceneId::new("scene-main").unwrap();
    let mut scene = Scene::new(scene_id.clone(), "Main");
    scene.is_main = true;
    document.current_scene_id = Some(scene_id);
    document.scenes.push(scene);
    state
        .database
        .create_project(
            document,
            "create-private-voice-project",
            &json!({ "name": "Provider" }),
        )
        .await
        .unwrap();
    (build_app(state.clone()), state, temp, server, calls)
}

async fn configured_image_app() -> (
    axum::Router,
    AppState,
    TempDir,
    tokio::task::JoinHandle<()>,
    Arc<AtomicUsize>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = calls.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/images/generations", post(generate_image))
                .with_state(FakeImageProvider {
                    calls: server_calls,
                }),
        )
        .await
        .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(install_fake_media_worker(temp.path()));
    std::fs::create_dir_all(config.provider_config.parent().unwrap()).unwrap();
    std::fs::write(
        &config.provider_config,
        serde_json::to_vec_pretty(&json!({
            "newApiImage": {
                "baseUrl": format!("http://{address}/v1"),
                "apiKey": "private-image-key",
                "defaultModel": "occ-image",
                "submitPath": "images/generations",
                "allowPrivateBaseUrl": true
            }
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &config.provider_config,
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    let state = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "private-image-test".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "test-daemon-token".to_owned(),
    )
    .await
    .unwrap();
    let document = ProjectDocument::new(ProjectId::new("provider-project").unwrap(), "Provider");
    state
        .database
        .create_project(
            document,
            "create-private-image-project",
            &json!({ "name": "Provider" }),
        )
        .await
        .unwrap();
    (build_app(state.clone()), state, temp, server, calls)
}

fn tool_request(body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/tools/generate_asset")
        .header(header::HOST, "127.0.0.1:3210")
        .header(header::AUTHORIZATION, "Bearer test-daemon-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn provider_job_retries_429_checkpoints_and_materializes_local_media() {
    let (app, state, _temp, server, attempts, _complete) = configured_app("rate").await;
    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "generate-provider-1",
            "arguments": {
                "projectId": "provider-project",
                "expectedRevision": 0,
                "kind": "video",
                "provider": "seedance-compatible",
                "model": "test-video-model",
                "prompt": "A calm abstract animation",
                "confirm": true,
                "options": {
                    "seed": 42,
                    "placement": {
                        "startSeconds": 2,
                        "durationSeconds": 4,
                        "name": "Generated B-roll"
                    }
                }
            }
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap();

    let completed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let job = state.database.read_job(job_id).await.unwrap();
            if matches!(job.state.as_str(), "succeeded" | "failed" | "cancelled") {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    assert_eq!(completed.output.as_ref().unwrap()["remoteId"], "remote-1");
    assert_eq!(
        completed.output.as_ref().unwrap()["normalization"],
        "ffmpeg-h264-aac-v1"
    );

    let envelope = state
        .database
        .read_project("provider-project")
        .await
        .unwrap();
    assert_eq!(envelope.revision, 2);
    assert_eq!(envelope.document.assets.len(), 1);
    let asset = &envelope.document.assets[0];
    assert_eq!(
        asset.provenance,
        openchatcut_domain::AssetProvenance::Generated {
            provider: "seedance-compatible".to_owned(),
            model: "test-video-model".to_owned(),
            prompt: "A calm abstract animation".to_owned(),
            seed: Some("42".to_owned()),
        }
    );
    let digest = asset.content_hash.as_ref().unwrap().as_str();
    assert!(state.layout.media_content(digest).await.unwrap().is_some());
    let placed_items = envelope
        .document
        .scenes
        .iter()
        .flat_map(|scene| scene.tracks.iter())
        .flat_map(|track| track.items.iter())
        .filter(|item| item.content.asset_id() == Some(&asset.id))
        .collect::<Vec<_>>();
    assert_eq!(placed_items.len(), 1);
    assert_eq!(placed_items[0].name, "Generated B-roll");
    assert_eq!(placed_items[0].start_ticks, 240_000);
    assert_eq!(placed_items[0].duration_ticks, 480_000);

    shutdown_state(&state).await;
    server.abort();
}

#[tokio::test]
async fn private_new_api_voice_is_confirmed_synthesized_normalized_and_materialized() {
    let (app, state, _temp, server, calls) = configured_voice_app().await;
    let arguments = json!({
        "projectId": "provider-project",
        "expectedRevision": 0,
        "kind": "voice",
        "provider": "new-api-voice",
        "model": "occ-tts",
        "prompt": "Read the private voice fixture.",
        "options": {
            "voice": "alloy",
            "language": "English"
        }
    });
    let unconfirmed = app
        .clone()
        .oneshot(tool_request(json!({
            "idempotencyKey": "private-voice-unconfirmed",
            "arguments": arguments.clone()
        })))
        .await
        .unwrap();
    assert_eq!(unconfirmed.status(), StatusCode::PRECONDITION_REQUIRED);
    let unconfirmed: Value =
        serde_json::from_slice(&unconfirmed.into_body().collect().await.unwrap().to_bytes())
            .unwrap();
    assert_eq!(
        unconfirmed["error"]["details"]["estimatedCost"]["amountMicros"],
        0
    );
    assert_eq!(
        unconfirmed["error"]["details"]["externalData"],
        json!(["prompt", "provider options"])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut confirmed = arguments;
    confirmed["confirm"] = json!(true);
    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "private-voice-confirmed",
            "arguments": confirmed
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
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
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(completed.output.as_ref().unwrap()["remoteId"], job_id);
    assert_eq!(
        completed.output.as_ref().unwrap()["normalization"],
        "ffmpeg-pcm-s24le-48k-v1"
    );
    let envelope = state
        .database
        .read_project("provider-project")
        .await
        .unwrap();
    assert_eq!(envelope.document.assets.len(), 1);
    assert_eq!(envelope.document.assets[0].kind, AssetKind::Audio);
    assert_eq!(
        envelope.document.assets[0].provenance,
        openchatcut_domain::AssetProvenance::Generated {
            provider: "new-api-voice".to_owned(),
            model: "occ-tts".to_owned(),
            prompt: "Read the private voice fixture.".to_owned(),
            seed: None,
        }
    );
    shutdown_state(&state).await;
    server.abort();
}

#[tokio::test]
async fn private_new_api_image_is_confirmed_decoded_normalized_and_materialized() {
    let (app, state, _temp, server, calls) = configured_image_app().await;
    let arguments = json!({
        "projectId": "provider-project",
        "expectedRevision": 0,
        "kind": "image",
        "provider": "new-api-image",
        "model": "occ-image",
        "prompt": "Generate a private image fixture.",
        "options": { "size": "512x512", "seed": 7 }
    });
    let unconfirmed = app
        .clone()
        .oneshot(tool_request(json!({
            "idempotencyKey": "private-image-unconfirmed",
            "arguments": arguments.clone()
        })))
        .await
        .unwrap();
    assert_eq!(unconfirmed.status(), StatusCode::PRECONDITION_REQUIRED);
    let unconfirmed: Value =
        serde_json::from_slice(&unconfirmed.into_body().collect().await.unwrap().to_bytes())
            .unwrap();
    assert_eq!(
        unconfirmed["error"]["details"]["estimatedCost"]["amountMicros"],
        0
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut confirmed = arguments;
    confirmed["confirm"] = json!(true);
    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "private-image-confirmed",
            "arguments": confirmed
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
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
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        completed.output.as_ref().unwrap()["normalization"],
        "ffmpeg-png-v1"
    );
    let envelope = state
        .database
        .read_project("provider-project")
        .await
        .unwrap();
    assert_eq!(envelope.document.assets.len(), 1);
    assert_eq!(envelope.document.assets[0].kind, AssetKind::Image);
    assert_eq!(envelope.document.assets[0].width, Some(1));
    assert_eq!(
        envelope.document.assets[0].provenance,
        openchatcut_domain::AssetProvenance::Generated {
            provider: "new-api-image".to_owned(),
            model: "occ-image".to_owned(),
            prompt: "Generate a private image fixture.".to_owned(),
            seed: Some("7".to_owned()),
        }
    );
    shutdown_state(&state).await;
    server.abort();
}

#[tokio::test]
async fn provider_generation_requires_explicit_confirmation_before_submission() {
    let (app, state, _temp, server, attempts, _complete) = configured_app("rate").await;
    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "generate-provider-unconfirmed",
            "arguments": {
                "projectId": "provider-project",
                "expectedRevision": 0,
                "kind": "video",
                "provider": "seedance-compatible",
                "prompt": "Do not submit this",
                "confirm": false
            }
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
    assert!(
        state
            .database
            .list_jobs(Some("provider-project"), 10)
            .await
            .unwrap()
            .is_empty()
    );
    shutdown_state(&state).await;
    server.abort();
}

#[tokio::test]
async fn provider_401_is_not_retried_and_is_persisted_without_response_secrets() {
    let (app, state, _temp, server, attempts, _complete) = configured_app("auth").await;
    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "generate-provider-auth",
            "arguments": {
                "projectId": "provider-project",
                "expectedRevision": 0,
                "kind": "video",
                "provider": "seedance-compatible",
                "prompt": "Authentication failure fixture",
                "confirm": true
            }
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap();
    let failed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = state.database.read_job(job_id).await.unwrap();
            if job.state == "failed" {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let error = failed.error.unwrap();
    assert_eq!(error["code"], "PROVIDER_AUTH_FAILED");
    assert_eq!(error["httpStatus"], 401);
    assert!(!error.to_string().contains("bad credential"));
    assert!(!error.to_string().contains("test-provider-key"));
    shutdown_state(&state).await;
    server.abort();
}

#[tokio::test]
async fn provider_408_is_retried_then_persisted_as_a_retryable_timeout() {
    let (app, state, _temp, server, attempts, _complete) = configured_app("timeout").await;
    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "generate-provider-timeout",
            "arguments": {
                "projectId": "provider-project",
                "expectedRevision": 0,
                "kind": "video",
                "provider": "seedance-compatible",
                "prompt": "Timeout classification fixture",
                "confirm": true
            }
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap();
    let failed = tokio::time::timeout(std::time::Duration::from_secs(12), async {
        loop {
            let job = state.database.read_job(job_id).await.unwrap();
            if job.state == "failed" {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 5);
    let error = failed.error.unwrap();
    assert_eq!(error["code"], "PROVIDER_TIMEOUT");
    assert_eq!(error["httpStatus"], 408);
    assert_eq!(error["retryable"], true);
    assert_eq!(
        state
            .database
            .read_project("provider-project")
            .await
            .unwrap()
            .revision,
        0
    );
    shutdown_state(&state).await;
    server.abort();
}

#[tokio::test]
async fn cancelling_a_polling_provider_job_reaches_a_durable_terminal_state() {
    let (app, state, _temp, server, _attempts, _complete) = configured_app("pending").await;
    let response = app
        .clone()
        .oneshot(tool_request(json!({
            "idempotencyKey": "generate-provider-cancel",
            "arguments": {
                "projectId": "provider-project",
                "expectedRevision": 0,
                "kind": "video",
                "provider": "seedance-compatible",
                "prompt": "Cancellation fixture",
                "confirm": true
            }
        })))
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = state.database.read_job(job_id).await.unwrap();
            if job.state == "running"
                && job
                    .output
                    .as_ref()
                    .and_then(|value| value.pointer("/checkpoint/remoteId"))
                    .and_then(Value::as_str)
                    == Some("remote-1")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let cancel = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/jobs/{job_id}/cancel"))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cancel.status(), StatusCode::OK);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if state.database.read_job(job_id).await.unwrap().state == "cancelled" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        state
            .database
            .read_project("provider-project")
            .await
            .unwrap()
            .revision,
        0
    );
    shutdown_state(&state).await;
    server.abort();
}

#[tokio::test]
async fn daemon_restart_resumes_remote_polling_without_a_second_submit() {
    let (app, state, temp, server, attempts, complete) = configured_app("pending").await;
    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "generate-provider-resume",
            "arguments": {
                "projectId": "provider-project",
                "expectedRevision": 0,
                "kind": "video",
                "provider": "seedance-compatible",
                "prompt": "Restart recovery fixture",
                "confirm": true
            }
        })))
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap().to_owned();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = state.database.read_job(&job_id).await.unwrap();
            if job.state == "running"
                && job
                    .output
                    .as_ref()
                    .and_then(|value| value.pointer("/checkpoint/remoteId"))
                    .and_then(Value::as_str)
                    == Some("remote-1")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    shutdown_state(&state).await;
    let interrupted = state.database.read_job(&job_id).await.unwrap();
    assert_eq!(interrupted.state, "queued");
    assert_eq!(
        interrupted
            .output
            .as_ref()
            .and_then(|value| value.pointer("/checkpoint/remoteId"))
            .and_then(Value::as_str),
        Some("remote-1")
    );

    complete.store(true, Ordering::SeqCst);
    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(fake_media_worker_path(temp.path()));
    let restarted = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "provider-test-restarted".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "test-daemon-token-restarted".to_owned(),
    )
    .await
    .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = restarted.database.read_job(&job_id).await.unwrap();
            if matches!(job.state.as_str(), "succeeded" | "failed") {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(
        restarted
            .database
            .read_project("provider-project")
            .await
            .unwrap()
            .revision,
        1
    );
    shutdown_state(&restarted).await;
    server.abort();
}

#[tokio::test]
async fn daemon_restart_normalizes_checkpointed_media_without_provider_network() {
    let (app, state, temp, server, attempts, _complete) = configured_app("ready").await;
    let block = state.layout.temporary.join("block-provider-normalization");
    fs::create_dir_all(block.parent().unwrap()).unwrap();
    fs::write(&block, b"block first normalizer").unwrap();

    let response = app
        .oneshot(tool_request(json!({
            "idempotencyKey": "generate-provider-normalize-resume",
            "arguments": {
                "projectId": "provider-project",
                "expectedRevision": 0,
                "kind": "video",
                "provider": "seedance-compatible",
                "model": "test-video-model",
                "prompt": "Resume from the locally staged provider media",
                "confirm": true,
                "options": {
                    "placement": {
                        "startSeconds": 3,
                        "durationSeconds": 2,
                        "name": "Recovered generated clip"
                    }
                }
            }
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let job_id = body["jobId"].as_str().unwrap().to_owned();

    let staged_path = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = state.database.read_job(&job_id).await.unwrap();
            if job.state == "running"
                && job
                    .output
                    .as_ref()
                    .and_then(|value| value.pointer("/checkpoint/phase"))
                    .and_then(Value::as_str)
                    == Some("normalize")
                && state
                    .layout
                    .temporary
                    .join("provider-normalization")
                    .is_dir()
            {
                let relative = job
                    .output
                    .as_ref()
                    .and_then(|value| value.pointer("/checkpoint/relativePath"))
                    .and_then(Value::as_str)
                    .unwrap();
                break state.layout.root.join(relative);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(staged_path.is_file());

    state.provider.as_ref().unwrap().shutdown().await;
    assert_eq!(
        state.database.read_job(&job_id).await.unwrap().state,
        "queued"
    );
    state.web_capture.as_ref().unwrap().shutdown().await;
    state.worker.as_ref().unwrap().shutdown().await;
    server.abort();
    fs::remove_file(&block).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.media_worker = Some(fake_media_worker_path(temp.path()));
    let restarted = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "provider-normalize-test-restarted".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "test-daemon-token-normalize-restarted".to_owned(),
    )
    .await
    .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = restarted.database.read_job(&job_id).await.unwrap();
            if matches!(job.state.as_str(), "succeeded" | "failed") {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while staged_path.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let envelope = restarted
        .database
        .read_project("provider-project")
        .await
        .unwrap();
    assert_eq!(envelope.revision, 2);
    let asset = envelope.document.assets.first().unwrap();
    let placed_items = envelope
        .document
        .scenes
        .iter()
        .flat_map(|scene| scene.tracks.iter())
        .flat_map(|track| track.items.iter())
        .filter(|item| item.content.asset_id() == Some(&asset.id))
        .collect::<Vec<_>>();
    assert_eq!(placed_items.len(), 1);
    assert_eq!(placed_items[0].name, "Recovered generated clip");
    assert_eq!(placed_items[0].start_ticks, 360_000);
    assert_eq!(placed_items[0].duration_ticks, 240_000);
    shutdown_state(&restarted).await;
}
