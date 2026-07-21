#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

fn tool_request(tool: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1/tools/{tool}"))
        .header(header::HOST, "127.0.0.1:3210")
        .header(header::AUTHORIZATION, "Bearer codex-image-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

async fn body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn codex_image_job_is_sandboxed_durable_and_materialized_as_managed_media() {
    let temp = tempfile::tempdir().unwrap();
    let fake_codex = temp.path().join("fake-codex-image.py");
    std::fs::write(
        &fake_codex,
        r#"#!/usr/bin/env python3
import base64, json, os, sys, time
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
for line in sys.stdin:
    message = json.loads(line)
    if message.get("method") == "initialize":
        assert message["params"]["capabilities"]["experimentalApi"] is True
        print(json.dumps({"id": message["id"], "result": {"userAgent": "fake"}}), flush=True)
    elif message.get("method") == "thread/start":
        params = message["params"]
        assert params["sandbox"] == "workspace-write"
        assert params["approvalPolicy"] == "never"
        assert params["ephemeral"] is True
        assert os.path.realpath(params["cwd"]) == os.path.realpath(os.getcwd())
        assert "Never read credentials" in params["developerInstructions"]
        print(json.dumps({"id": message["id"], "result": {"thread": {"id": "thread-image"}}}), flush=True)
    elif message.get("method") == "turn/start":
        params = message["params"]
        policy = params["sandboxPolicy"]
        assert policy["type"] == "workspaceWrite"
        assert policy["networkAccess"] is False
        assert len(policy["writableRoots"]) == 1
        assert os.path.realpath(policy["writableRoots"][0]) == os.path.realpath(os.getcwd())
        prompt = params["input"][0]["text"]
        assert "$imagegen" in prompt
        if "CANCEL_FIXTURE" in prompt:
            print(json.dumps({"id": message["id"], "result": {"turn": {"id": "turn-image-cancel"}}}), flush=True)
            time.sleep(30)
            continue
        if "PATH_ESCAPE_FIXTURE" in prompt:
            output = os.path.join(os.path.dirname(os.getcwd()), "outside.png")
        else:
            output = os.path.join(os.getcwd(), "generated.png")
        with open(output, "wb") as target:
            target.write(PNG)
        print(json.dumps({"id": message["id"], "result": {"turn": {"id": "turn-image"}}}), flush=True)
        print(json.dumps({"method": "item/completed", "params": {"threadId": "thread-image", "turnId": "turn-image", "item": {"id": "image-test", "type": "imageGeneration", "status": "completed", "result": "ignored-inline-result", "revisedPrompt": "A revised safe image prompt", "savedPath": output}}}), flush=True)
        print(json.dumps({"method": "turn/completed", "params": {"threadId": "thread-image", "turn": {"id": "turn-image", "status": "completed", "items": []}}}), flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut config = Config::for_test(temp.path().to_owned());
    config.codex_command = Some(fake_codex);
    let state = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "codex-image-test".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "codex-image-token".to_owned(),
    )
    .await
    .unwrap();
    let app = build_app(state.clone());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/projects")
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer codex-image-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "Codex Images",
                        "idempotencyKey": "create-codex-image-project"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body(response).await;
    let project_id = created["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = app
        .clone()
        .oneshot(tool_request(
            "list_generators",
            json!({ "arguments": { "kind": "image" } }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let generators = body(response).await;
    let codex = generators["data"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["id"] == "codex-image")
        .unwrap();
    assert_eq!(codex["availability"]["state"], "available");
    assert_eq!(codex["models"], json!(["gpt-image-2"]));

    let response = app
        .clone()
        .oneshot(tool_request(
            "generate_asset",
            json!({
                "idempotencyKey": "generate-codex-image",
                "arguments": {
                    "projectId": project_id,
                    "expectedRevision": 0,
                    "kind": "image",
                    "provider": "codex-image",
                    "model": "gpt-image-2",
                    "prompt": "A clean editorial product illustration",
                    "confirm": true,
                    "options": {}
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let accepted = body(response).await;
    let job_id = accepted["jobId"].as_str().unwrap().to_owned();

    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = state.database.read_job(&job_id).await.unwrap();
            if matches!(job.state.as_str(), "succeeded" | "failed" | "cancelled") {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert_eq!(completed.output.as_ref().unwrap()["model"], "gpt-image-2");
    assert_eq!(
        completed.output.as_ref().unwrap()["provenance"]["revisedPrompt"],
        "A revised safe image prompt"
    );

    let envelope = state.database.read_project(&project_id).await.unwrap();
    assert_eq!(envelope.revision, 1);
    assert_eq!(envelope.document.assets.len(), 1);
    let asset = &envelope.document.assets[0];
    assert_eq!(asset.kind, openchatcut_domain::AssetKind::Image);
    assert_eq!(
        asset.provenance,
        openchatcut_domain::AssetProvenance::Generated {
            provider: "codex-image".to_owned(),
            model: "gpt-image-2".to_owned(),
            prompt: "A clean editorial product illustration".to_owned(),
            seed: None,
        }
    );
    assert_eq!(
        asset.extensions["generation"]["revisedPrompt"],
        "A revised safe image prompt"
    );
    let digest = asset.content_hash.as_ref().unwrap().as_str();
    let stored = state.layout.media_content(digest).await.unwrap().unwrap();
    assert_eq!(stored.size, 68);

    // A compromised/malformed app-server response cannot make the daemon
    // import a file from the parent directory, even when it looks like PNG.
    let response = app
        .clone()
        .oneshot(tool_request(
            "generate_asset",
            json!({
                "idempotencyKey": "generate-codex-image-path-escape",
                "arguments": {
                    "projectId": project_id,
                    "expectedRevision": 1,
                    "kind": "image",
                    "provider": "codex-image",
                    "prompt": "PATH_ESCAPE_FIXTURE",
                    "confirm": true
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let rejected_job_id = body(response).await["jobId"].as_str().unwrap().to_owned();
    let rejected = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = state.database.read_job(&rejected_job_id).await.unwrap();
            if job.state == "failed" {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        rejected.error.unwrap()["code"],
        "CODEX_IMAGE_GENERATION_FAILED"
    );
    assert_eq!(
        state
            .database
            .read_project(&project_id)
            .await
            .unwrap()
            .revision,
        1
    );

    let response = app
        .clone()
        .oneshot(tool_request(
            "generate_asset",
            json!({
                "idempotencyKey": "generate-codex-image-cancel",
                "arguments": {
                    "projectId": project_id,
                    "expectedRevision": 1,
                    "kind": "image",
                    "provider": "codex-image",
                    "prompt": "CANCEL_FIXTURE",
                    "confirm": true
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cancelled_job_id = body(response).await["jobId"].as_str().unwrap().to_owned();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = state.database.read_job(&cancelled_job_id).await.unwrap();
            if job.state == "running" && job.progress >= 0.02 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/jobs/{cancelled_job_id}/cancel"))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer codex-image-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if state
                .database
                .read_job(&cancelled_job_id)
                .await
                .unwrap()
                .state
                == "cancelled"
            {
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
            .read_project(&project_id)
            .await
            .unwrap()
            .revision,
        1
    );

    state.codex_image.as_ref().unwrap().shutdown().await;
    state.native_jobs.shutdown().await;
}

#[tokio::test]
async fn daemon_restart_imports_a_checkpointed_codex_image_without_generating_twice() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let initial = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "codex-image-checkpoint-initial".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "checkpoint-initial-token".to_owned(),
    )
    .await
    .unwrap();
    initial
        .database
        .create_project(
            openchatcut_domain::ProjectDocument::new(
                openchatcut_domain::ProjectId::new("codex-image-resume-project").unwrap(),
                "Resume image",
            ),
            "create-codex-image-resume-project",
            &json!({ "name": "Resume image" }),
        )
        .await
        .unwrap();
    let input = json!({
        "provider": "codex-image",
        "kind": "image",
        "model": "gpt-image-2",
        "prompt": "A checkpointed image",
        "seed": null,
        "options": {},
    });
    let (queued, _) = initial
        .database
        .enqueue_job_idempotent(
            "codex_image_generation",
            "codex-image-resume-project",
            0,
            "checkpointed-codex-image",
            &input,
        )
        .await
        .unwrap();
    initial
        .database
        .claim_job_by_id(&queued.id)
        .await
        .unwrap()
        .unwrap();

    let directory_id = hex::encode(Sha256::digest(queued.id.as_bytes()));
    let job_directory = initial
        .layout
        .temporary
        .join("codex-image-jobs")
        .join(&directory_id[..32]);
    std::fs::create_dir_all(&job_directory).unwrap();
    let image_path = job_directory.join("generated.png");
    let png = base64_fixture_png();
    std::fs::write(&image_path, &png).unwrap();
    let digest = hex::encode(Sha256::digest(&png));
    initial
        .database
        .checkpoint_job(
            &queued.id,
            0.8,
            "Codex image saved; importing managed media",
            &json!({
                "checkpoint": {
                    "phase": "generated",
                    "relativePath": "generated.png",
                    "sha256": digest,
                    "byteSize": png.len(),
                    "mimeType": "image/png",
                    "revisedPrompt": "A checkpointed revised prompt",
                }
            }),
        )
        .await
        .unwrap();
    initial
        .database
        .requeue_interrupted_job(&queued.id)
        .await
        .unwrap();
    initial.native_jobs.shutdown().await;
    initial.database.close().await;

    let invocation_marker = temp.path().join("codex-was-invoked");
    let fake_codex = temp.path().join("must-not-run-codex.py");
    std::fs::write(
        &fake_codex,
        format!(
            "#!/usr/bin/env python3\nfrom pathlib import Path\nPath({:?}).write_text('invoked')\nraise SystemExit(91)\n",
            invocation_marker
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut restarted_config = Config::for_test(temp.path().to_owned());
    restarted_config.codex_command = Some(fake_codex);
    let restarted = AppState::initialize(
        &restarted_config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "codex-image-checkpoint-restarted".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: restarted_config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "checkpoint-restarted-token".to_owned(),
    )
    .await
    .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let job = restarted.database.read_job(&queued.id).await.unwrap();
            if matches!(job.state.as_str(), "succeeded" | "failed") {
                break job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert!(!invocation_marker.exists());
    let envelope = restarted
        .database
        .read_project("codex-image-resume-project")
        .await
        .unwrap();
    assert_eq!(envelope.revision, 1);
    assert_eq!(
        envelope.document.assets[0].extensions["generation"]["revisedPrompt"],
        "A checkpointed revised prompt"
    );
    assert!(
        restarted
            .layout
            .media_content(&digest)
            .await
            .unwrap()
            .is_some()
    );
    restarted.codex_image.as_ref().unwrap().shutdown().await;
    restarted.native_jobs.shutdown().await;
}

fn base64_fixture_png() -> Vec<u8> {
    // A complete 1x1 PNG, decoded without adding a test-only crate.
    hex::decode(concat!(
        "89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c",
        "020000000b4944415478da6364f80f00010501012718e3660000000049454e44",
        "ae426082"
    ))
    .unwrap()
}
