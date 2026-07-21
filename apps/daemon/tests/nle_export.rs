use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{AppState, Config, build_app, runtime::RuntimeDescriptor};
use openchatcut_domain::{
    Asset, AssetId, AssetKind, ItemContent, ItemId, MediaKind, ProjectDocument, Scene, SceneId,
    Sha256Digest, TimelineItem, Track, TrackId, TrackKind,
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn wait_for_job(
    state: &AppState,
    job_id: &str,
) -> openchatcut_daemon::persistence::JobRecord {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let job = state.database.read_job(job_id).await.unwrap();
            if matches!(job.state.as_str(), "succeeded" | "failed" | "cancelled") {
                return job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("native export did not reach a terminal state")
}

fn request(body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/tools/start_export")
        .header(header::HOST, "127.0.0.1:3210")
        .header(header::AUTHORIZATION, "Bearer test-daemon-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn premiere_and_resolve_xml_are_revision_pinned_native_exports() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let runtime = RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: "nle-test".to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    let state = AppState::initialize(&config, runtime, "test-daemon-token".to_owned())
        .await
        .unwrap();
    let stored = state
        .layout
        .put_media(&hex::decode("000000186674797069736f6d0000020069736f6d69736f32").unwrap())
        .await
        .unwrap();
    let mut document = ProjectDocument::new("nle-project".parse().unwrap(), "NLE Project");
    let mut asset = Asset::new(
        AssetId::new("asset:source").unwrap(),
        "Source & Master.mp4",
        AssetKind::Video,
    );
    asset.content_hash = Some(Sha256Digest::new(stored.sha256).unwrap());
    asset.duration_ticks = Some(240_000);
    asset.has_audio = true;
    document.assets.push(asset);
    let mut scene = Scene::new(SceneId::new("scene:main").unwrap(), "Main");
    scene.is_main = true;
    let mut video = Track::new(
        TrackId::new("track:video").unwrap(),
        "Video",
        TrackKind::Video,
    );
    let mut text = Track::new(TrackId::new("track:text").unwrap(), "Text", TrackKind::Text);
    video.items.push(TimelineItem::new(
        ItemId::new("item:clip").unwrap(),
        "Source clip",
        0,
        120_000,
        ItemContent::Media {
            asset_id: AssetId::new("asset:source").unwrap(),
            media_kind: MediaKind::Video,
        },
    ));
    text.items.push(TimelineItem::new(
        ItemId::new("item:title").unwrap(),
        "Editable title",
        0,
        60_000,
        ItemContent::Text {
            text: "Title".to_owned(),
        },
    ));
    scene.tracks.push(video);
    scene.tracks.push(text);
    document.current_scene_id = Some(scene.id.clone());
    document.scenes.push(scene);
    state
        .database
        .create_project(document, "create-nle", &json!({ "name": "NLE Project" }))
        .await
        .unwrap();
    let app = build_app(state.clone());

    for (format, file_name, root_element) in [
        ("premiere-xml", "premiere.xml", "<xmeml"),
        ("resolve-xml", "resolve.xml", "<fcpxml"),
    ] {
        let body = json!({
            "idempotencyKey": format!("export-{format}"),
            "arguments": {
                "projectId": "nle-project",
                "expectedRevision": 0,
                "format": format,
                "outputPath": file_name,
                "allowOverwrite": false
            }
        });
        let response = app.clone().oneshot(request(body.clone())).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(matches!(
            value["data"]["job"]["state"].as_str(),
            Some("queued" | "running" | "succeeded")
        ));
        assert_eq!(value["data"]["pinnedRevision"], 0);
        assert_eq!(value["data"]["warnings"][0]["itemIds"][0], "item:title");
        let completed = wait_for_job(&state, value["jobId"].as_str().unwrap()).await;
        assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
        let xml = tokio::fs::read_to_string(state.layout.exports.join(file_name))
            .await
            .unwrap();
        assert!(xml.contains(root_element));
        assert!(xml.contains("Source &amp; Master.mp4"));
        assert!(xml.contains("file://"));

        let replay = app.clone().oneshot(request(body)).await.unwrap();
        assert_eq!(replay.status(), StatusCode::OK);
        let replay: Value =
            serde_json::from_slice(&replay.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(replay["data"]["replayed"], true);
    }
}
