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

async fn app_with_import_root() -> (axum::Router, TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let import_root = temp.path().join("authorized");
    std::fs::create_dir_all(&import_root).unwrap();
    let mut config = Config::for_test(temp.path().to_owned());
    config.authorized_import_roots = vec![std::fs::canonicalize(import_root).unwrap()];
    (app_from_config(&config, "managed-media-test").await, temp)
}

async fn app_from_config(config: &Config, instance_id: &str) -> axum::Router {
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: instance_id.to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-token".to_owned())
        .await
        .unwrap();
    build_app(state)
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
    let body = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

async fn create_project(app: &axum::Router) -> String {
    let (status, body) = json_request(
        app,
        "POST",
        "/api/v1/projects",
        json!({ "name": "Managed media", "idempotencyKey": "create-media-project" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["envelope"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn upload_request(
    app: &axum::Router,
    uri: &str,
    expected_revision: u64,
    idempotency_key: &str,
    bytes: &'static [u8],
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header("Idempotency-Key", idempotency_key)
                .header(
                    "X-OpenChatCut-Expected-Revision",
                    expected_revision.to_string(),
                )
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

#[tokio::test]
async fn browser_upload_streams_to_managed_content_and_can_be_read_after_commit() {
    let (app, _temp) = app_with_import_root().await;
    let project_id = create_project(&app).await;
    let source_bytes = b"RIFF\x04\x00\x00\x00WAVE";
    let uri = format!(
        "/api/v1/projects/{project_id}/media?assetId=browser-audio&name=dialogue.wav&durationTicks=48000&hasAudio=true"
    );
    let (status, uploaded) = upload_request(&app, &uri, 0, "browser-upload-1", source_bytes).await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    assert_eq!(uploaded["revision"], 1);
    assert_eq!(uploaded["asset"]["id"], "browser-audio");
    assert_eq!(uploaded["asset"]["kind"], "audio");
    assert_eq!(uploaded["asset"]["durationTicks"], 48_000);
    assert_eq!(uploaded["asset"]["managedMedia"]["source"], "browserUpload");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/projects/{project_id}/assets/browser-audio/content"
                ))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "audio/wav");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        &source_bytes[..]
    );

    let (status, replayed) = upload_request(&app, &uri, 0, "browser-upload-1", source_bytes).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["revision"], 1);
    assert_eq!(replayed["replayed"], true);

    let stale_uri =
        format!("/api/v1/projects/{project_id}/media?assetId=stale-audio&name=stale.wav");
    let (status, stale) =
        upload_request(&app, &stale_uri, 0, "browser-upload-stale", source_bytes).await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["error"]["code"], "revisionConflict");
}

#[tokio::test]
async fn browser_uploaded_media_survives_daemon_restart_without_browser_storage() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let app = app_from_config(&config, "managed-media-before-restart").await;
    let project_id = create_project(&app).await;
    let source_bytes = b"RIFF\x04\x00\x00\x00WAVE";
    let uri = format!(
        "/api/v1/projects/{project_id}/media?assetId=persistent-audio&name=persistent.wav&hasAudio=true"
    );
    let (status, uploaded) =
        upload_request(&app, &uri, 0, "browser-upload-persistent", source_bytes).await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    drop(app);

    let restarted = app_from_config(&config, "managed-media-after-restart").await;
    let (status, project) = json_request(
        &restarted,
        "GET",
        &format!("/api/v1/projects/{project_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["envelope"]["revision"], 1);
    assert_eq!(
        project["envelope"]["document"]["assets"][0]["id"],
        "persistent-audio"
    );

    let response = restarted
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/projects/{project_id}/assets/persistent-audio/content"
                ))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        &source_bytes[..]
    );
}

#[tokio::test]
async fn authorized_media_is_copied_committed_inspected_and_replayed() {
    let (app, temp) = app_with_import_root().await;
    let source = temp.path().join("authorized/dialogue.wav");
    let source_bytes = b"RIFF\x04\x00\x00\x00WAVE";
    tokio::fs::write(&source, source_bytes).await.unwrap();
    let project_id = create_project(&app).await;
    let request = json!({
        "arguments": {
            "projectId": project_id,
            "expectedRevision": 0,
            "path": source,
            "mode": "managed"
        },
        "idempotencyKey": "import-dialogue-1"
    });

    let (status, imported) = json_request(
        &app,
        "POST",
        "/api/v1/tools/import_local_media",
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    assert_eq!(imported["ok"], true);
    assert_eq!(imported["revision"], 1);
    assert_eq!(imported["data"]["asset"]["kind"], "audio");
    assert_eq!(imported["data"]["asset"]["hasAudio"], true);
    let asset_id = imported["data"]["asset"]["id"].as_str().unwrap().to_owned();
    let digest = imported["data"]["asset"]["contentHash"]
        .as_str()
        .unwrap()
        .to_owned();
    let stored = temp
        .path()
        .join("data/media/sha256")
        .join(&digest[..2])
        .join(&digest[2..]);
    assert_eq!(tokio::fs::read(stored).await.unwrap(), source_bytes);

    let (status, inspected) = json_request(
        &app,
        "POST",
        "/api/v1/tools/inspect_media",
        json!({ "arguments": { "projectId": project_id, "assetId": asset_id } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inspected}");
    assert_eq!(inspected["data"]["revision"], 1);
    assert_eq!(inspected["data"]["managedContent"]["available"], true);
    assert_eq!(
        inspected["data"]["managedContent"]["byteSize"],
        source_bytes.len() as u64
    );
    assert_eq!(
        inspected["data"]["technicalMetadata"]["status"],
        "notProbed"
    );

    let (status, replayed) =
        json_request(&app, "POST", "/api/v1/tools/import_local_media", request).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["revision"], 1);
    assert_eq!(replayed["data"]["commit"]["replayed"], true);
    assert_eq!(replayed["data"]["asset"]["id"], asset_id);
}

#[tokio::test]
async fn linked_media_requires_confirmation_streams_only_unchanged_authorized_bytes_and_is_nonportable()
 {
    let (app, temp) = app_with_import_root().await;
    let source = temp.path().join("authorized/linked.wav");
    let source_bytes = b"RIFF\x04\x00\x00\x00WAVE";
    tokio::fs::write(&source, source_bytes).await.unwrap();
    let project_id = create_project(&app).await;
    let base = json!({
        "arguments": {
            "projectId": project_id,
            "expectedRevision": 0,
            "path": source,
            "mode": "linked"
        },
        "idempotencyKey": "link-dialogue-1"
    });
    let (status, confirmation) = json_request(
        &app,
        "POST",
        "/api/v1/tools/import_local_media",
        base.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED, "{confirmation}");
    assert_eq!(confirmation["error"]["code"], "confirmation_required");

    let mut approved = base;
    approved["arguments"]["confirmLinkedRisk"] = json!(true);
    let (status, imported) =
        json_request(&app, "POST", "/api/v1/tools/import_local_media", approved).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    assert_eq!(imported["data"]["managed"], false);
    assert_eq!(imported["data"]["linked"], true);
    assert_eq!(imported["data"]["portable"], false);
    assert!(imported["data"]["asset"]["contentHash"].is_null());
    assert_eq!(imported["data"]["asset"]["linkedFile"]["portable"], false);
    let asset_id = imported["data"]["asset"]["id"].as_str().unwrap();
    let digest = imported["data"]["asset"]["linkedFile"]["fingerprintSha256"]
        .as_str()
        .unwrap();
    assert!(
        !temp
            .path()
            .join("data/media/sha256")
            .join(&digest[..2])
            .join(&digest[2..])
            .exists(),
        "linked import must not copy bytes into the managed library"
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/projects/{project_id}/assets/{asset_id}/content"
                ))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        &source_bytes[..]
    );

    let (status, validation) = json_request(
        &app,
        "POST",
        "/api/v1/tools/validate_project",
        json!({
            "arguments": {
                "projectId": project_id,
                "revision": 1,
                "target": "project-package"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validation}");
    assert_eq!(validation["data"]["report"]["valid"], false);
    assert!(
        validation["data"]["report"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "linked_asset_not_portable")
    );

    tokio::fs::write(&source, b"RIFF\x05\x00\x00\x00WAVEX")
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/projects/{project_id}/assets/{asset_id}/content"
                ))
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let changed: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(changed["error"]["code"], "linked_file_changed");
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_from_authorized_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let (app, temp) = app_with_import_root().await;
    let outside = temp.path().join("outside.wav");
    tokio::fs::write(&outside, b"must not be copied")
        .await
        .unwrap();
    let link = temp.path().join("authorized/escape.wav");
    symlink(&outside, &link).unwrap();
    let project_id = create_project(&app).await;

    let (status, rejected) = json_request(
        &app,
        "POST",
        "/api/v1/tools/import_local_media",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "path": link,
                "mode": "managed"
            },
            "idempotencyKey": "escape-attempt-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{rejected}");
    assert_eq!(rejected["error"]["code"], "import_path_not_authorized");

    let (_, project) = json_request(
        &app,
        "GET",
        &format!("/api/v1/projects/{project_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(project["envelope"]["revision"], 0);
    assert!(
        project["envelope"]["document"]["assets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn mime_spoof_and_stale_revision_do_not_install_content() {
    use sha2::{Digest, Sha256};

    let (app, temp) = app_with_import_root().await;
    let project_id = create_project(&app).await;
    let spoof = temp.path().join("authorized/not-audio.wav");
    let spoof_bytes = b"<!doctype html><script>alert(1)</script>";
    tokio::fs::write(&spoof, spoof_bytes).await.unwrap();
    let (status, rejected) = json_request(
        &app,
        "POST",
        "/api/v1/tools/import_local_media",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "path": spoof,
            },
            "idempotencyKey": "mime-spoof-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{rejected}");
    assert_eq!(rejected["error"]["code"], "unsafe_active_media");
    let spoof_digest = hex::encode(Sha256::digest(spoof_bytes));
    assert!(
        !temp
            .path()
            .join("data/media/sha256")
            .join(&spoof_digest[..2])
            .join(&spoof_digest[2..])
            .exists()
    );

    let (_, renamed) = json_request(
        &app,
        "POST",
        &format!("/api/v1/projects/{project_id}/transactions"),
        json!({
            "transactionId": "advance-before-import",
            "projectId": project_id,
            "baseRevision": 0,
            "idempotencyKey": "advance-before-import",
            "actor": { "kind": "user", "id": "local-user" },
            "operations": [{ "type": "setProjectName", "name": "Advanced" }]
        }),
    )
    .await;
    assert_eq!(renamed["envelope"]["revision"], 1);
    let stale = temp.path().join("authorized/stale.wav");
    let stale_bytes = b"RIFF\x04\x00\x00\x00WAVE";
    tokio::fs::write(&stale, stale_bytes).await.unwrap();
    let (status, conflict) = json_request(
        &app,
        "POST",
        "/api/v1/tools/import_local_media",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "path": stale,
            },
            "idempotencyKey": "stale-import-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"]["code"], "revisionConflict");
    let stale_digest = hex::encode(Sha256::digest(stale_bytes));
    assert!(
        !temp
            .path()
            .join("data/media/sha256")
            .join(&stale_digest[..2])
            .join(&stale_digest[2..])
            .exists()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn even_an_in_root_leaf_symlink_is_not_followed() {
    use std::os::unix::fs::symlink;

    let (app, temp) = app_with_import_root().await;
    let target = temp.path().join("authorized/target.wav");
    tokio::fs::write(&target, b"RIFF\x04\x00\x00\x00WAVE")
        .await
        .unwrap();
    let link = temp.path().join("authorized/link.wav");
    symlink(&target, &link).unwrap();
    let project_id = create_project(&app).await;
    let (status, rejected) = json_request(
        &app,
        "POST",
        "/api/v1/tools/import_local_media",
        json!({
            "arguments": {
                "projectId": project_id,
                "expectedRevision": 0,
                "path": link,
            },
            "idempotencyKey": "in-root-symlink-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{rejected}");
    assert_eq!(
        rejected["error"]["code"],
        "import_source_symlink_or_unreadable"
    );
}
