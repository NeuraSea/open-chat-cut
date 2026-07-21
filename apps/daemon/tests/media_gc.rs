use std::{
    fs::{FileTimes, OpenOptions},
    time::{Duration, SystemTime},
};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use openchatcut_domain::{Asset, AssetId, AssetKind, ProjectDocument, ProjectId, Sha256Digest};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

async fn app() -> (axum::Router, AppState, TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "test-instance".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-daemon-token".to_owned())
        .await
        .unwrap();
    (build_app(state.clone()), state, temp)
}

async fn gc_request(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/maintenance/media-gc")
                .header(header::HOST, "127.0.0.1:3210")
                .header(header::AUTHORIZATION, "Bearer test-daemon-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn age(path: &std::path::Path, duration: Duration) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_times(FileTimes::new().set_modified(SystemTime::now().checked_sub(duration).unwrap()))
        .unwrap();
}

#[tokio::test]
async fn asset_gc_preserves_history_references_and_requires_an_idle_queue() {
    let (app, state, _temp) = app().await;
    let referenced = state.layout.put_media(b"referenced media").await.unwrap();
    let orphan = state.layout.put_media(b"orphan media").await.unwrap();
    age(&referenced.path, Duration::from_secs(3 * 60 * 60));
    age(&orphan.path, Duration::from_secs(3 * 60 * 60));

    let mut document = ProjectDocument::new(ProjectId::new("project:gc").unwrap(), "GC");
    let mut asset = Asset::new(
        AssetId::new("asset:referenced").unwrap(),
        "Referenced",
        AssetKind::Video,
    );
    asset.content_hash = Some(Sha256Digest::new(referenced.sha256.clone()).unwrap());
    document.assets.push(asset);
    state
        .database
        .create_project(document, "create-gc-project", &json!({ "name": "GC" }))
        .await
        .unwrap();

    let (status, preview) = gc_request(&app, json!({ "confirm": false, "minAgeHours": 1 })).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["dryRun"], true);
    assert_eq!(preview["inventory"]["referencedCount"], 1);
    assert_eq!(preview["inventory"]["candidateCount"], 1);
    assert_eq!(preview["inventory"]["candidateHashes"][0], orphan.sha256);
    assert!(
        state
            .layout
            .media_content(&orphan.sha256)
            .await
            .unwrap()
            .is_some()
    );

    let (status, collected) = gc_request(&app, json!({ "confirm": true, "minAgeHours": 1 })).await;
    assert_eq!(status, StatusCode::OK, "{collected}");
    assert_eq!(collected["removedCount"], 1);
    assert!(
        state
            .layout
            .media_content(&orphan.sha256)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .layout
            .media_content(&referenced.sha256)
            .await
            .unwrap()
            .is_some()
    );

    let second_orphan = state.layout.put_media(b"second orphan").await.unwrap();
    age(&second_orphan.path, Duration::from_secs(3 * 60 * 60));
    state
        .database
        .enqueue_job("test.noop", Some("project:gc"), Some(0), &json!({}))
        .await
        .unwrap();
    let (status, blocked) = gc_request(&app, json!({ "confirm": true, "minAgeHours": 1 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["error"]["code"], "asset_gc_jobs_active");
    assert!(
        state
            .layout
            .media_content(&second_orphan.sha256)
            .await
            .unwrap()
            .is_some()
    );
}
