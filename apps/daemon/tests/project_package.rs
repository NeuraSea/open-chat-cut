use std::io::Read;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use http_body_util::BodyExt;
use openchatcut_daemon::{
    AppState, Config, build_app,
    project_package::{PROJECT_PACKAGE_FORMAT, ProjectPackageManifest, extract_project_package},
    runtime::RuntimeDescriptor,
};
use openchatcut_domain::{
    Asset, AssetId, AssetKind, ProjectDocument, ProjectEnvelope, Sha256Digest,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zip::ZipArchive;

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
    .expect("project package export did not reach a terminal state")
}
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn request(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, "127.0.0.1:3210")
        .header(header::AUTHORIZATION, "Bearer package-test-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn project_package_rejects_zip_slip_entries_before_extraction() {
    let temp = tempfile::tempdir().unwrap();
    let package_path = temp.path().join("malicious.occproj");
    let output = std::fs::File::create(&package_path).unwrap();
    let mut archive = ZipWriter::new(output);
    archive
        .start_file("../escape", SimpleFileOptions::default())
        .unwrap();
    use std::io::Write;
    archive.write_all(b"owned").unwrap();
    archive.finish().unwrap();
    let extraction = temp.path().join("extract");
    std::fs::create_dir(&extraction).unwrap();

    let error = extract_project_package(
        tokio::fs::File::open(&package_path).await.unwrap(),
        &extraction,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("unsafe entry path"));
    assert!(!temp.path().join("escape").exists());
}

#[tokio::test]
async fn project_package_rejects_noncanonical_digest_names() {
    let temp = tempfile::tempdir().unwrap();
    let package_path = temp.path().join("uppercase-digest.occproj");
    let output = std::fs::File::create(&package_path).unwrap();
    let mut archive = ZipWriter::new(output);
    archive
        .start_file(
            format!("media/sha256/{}", "A".repeat(64)),
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .unwrap();
    use std::io::Write;
    archive.write_all(b"not canonical").unwrap();
    archive.finish().unwrap();

    let error = extract_project_package(
        tokio::fs::File::open(&package_path).await.unwrap(),
        temp.path(),
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("invalid SHA-256 digest"),
        "{error:#}"
    );
}

#[tokio::test]
async fn project_package_contains_pinned_envelope_and_deduplicated_managed_media() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::for_test(temp.path().to_owned());
    std::fs::create_dir_all(config.data_dir.join("exports")).unwrap();
    config.authorized_import_roots =
        vec![std::fs::canonicalize(config.data_dir.join("exports")).unwrap()];
    let state = AppState::initialize(
        &config,
        RuntimeDescriptor {
            protocol_version: "1".to_owned(),
            instance_id: "package-test".to_owned(),
            api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
            token_path: config.token_path.clone(),
            pid: std::process::id(),
            started_at: Utc::now(),
        },
        "package-test-token".to_owned(),
    )
    .await
    .unwrap();
    let media_bytes = b"RIFF\x04\x00\x00\x00WAVE";
    let stored = state.layout.put_media(media_bytes).await.unwrap();
    let contact_sheet_bytes = b"\xff\xd8\xffportable-contact-sheet";
    let stored_contact_sheet = state.layout.put_media(contact_sheet_bytes).await.unwrap();
    let mut document = ProjectDocument::new("package-project".parse().unwrap(), "Package");
    for (id, name) in [("asset:one", "One.wav"), ("asset:two", "Two.wav")] {
        let mut asset = Asset::new(AssetId::new(id).unwrap(), name, AssetKind::Audio);
        asset.content_hash = Some(Sha256Digest::new(stored.sha256.clone()).unwrap());
        asset.has_audio = true;
        document.assets.push(asset);
    }
    document.assets[0].extensions.insert(
        "derivatives".to_owned(),
        json!({
            "contactSheet": {
                "contentHash": stored_contact_sheet.sha256,
                "mimeType": "image/jpeg"
            }
        }),
    );
    state
        .database
        .create_project(document, "create-package", &json!({ "name": "Package" }))
        .await
        .unwrap();
    let app = build_app(state.clone());
    let body = json!({
        "idempotencyKey": "package-export-1",
        "arguments": {
            "projectId": "package-project",
            "expectedRevision": 0,
            "format": "project-package",
            "outputPath": "portable.occproj",
            "allowOverwrite": false
        }
    });
    let response = app
        .clone()
        .oneshot(request("/api/v1/tools/start_export", body.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(matches!(
        response["data"]["job"]["state"].as_str(),
        Some("queued" | "running" | "succeeded")
    ));
    let completed = wait_for_job(&state, response["jobId"].as_str().unwrap()).await;
    assert_eq!(completed.state, "succeeded", "{:?}", completed.error);
    assert_eq!(completed.output.as_ref().unwrap()["mediaCount"], 2);

    let package_path = state.layout.exports.join("portable.occproj");
    let mut archive = ZipArchive::new(std::fs::File::open(&package_path).unwrap()).unwrap();
    assert_eq!(archive.len(), 4);
    let manifest: ProjectPackageManifest = {
        let mut value = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut value)
            .unwrap();
        serde_json::from_str(&value).unwrap()
    };
    assert_eq!(manifest.format, PROJECT_PACKAGE_FORMAT);
    assert_eq!(manifest.revision, 0);
    assert_eq!(manifest.media.len(), 2);
    assert_eq!(
        manifest
            .media
            .iter()
            .find(|media| media.sha256 == stored.sha256)
            .unwrap()
            .asset_ids,
        ["asset:one", "asset:two"]
    );
    assert_eq!(
        manifest
            .media
            .iter()
            .find(|media| media.sha256 == stored_contact_sheet.sha256)
            .unwrap()
            .asset_ids,
        ["asset:one#derivative:contactSheet"]
    );
    let envelope: ProjectEnvelope = {
        let mut value = String::new();
        archive
            .by_name("project/envelope.json")
            .unwrap()
            .read_to_string(&mut value)
            .unwrap();
        serde_json::from_str(&value).unwrap()
    };
    assert_eq!(envelope.document.assets.len(), 2);
    let mut packaged_media = Vec::new();
    archive
        .by_name(&format!("media/sha256/{}", stored.sha256))
        .unwrap()
        .read_to_end(&mut packaged_media)
        .unwrap();
    assert_eq!(packaged_media, media_bytes);
    let mut packaged_contact_sheet = Vec::new();
    archive
        .by_name(&format!("media/sha256/{}", stored_contact_sheet.sha256))
        .unwrap()
        .read_to_end(&mut packaged_contact_sheet)
        .unwrap();
    assert_eq!(packaged_contact_sheet, contact_sheet_bytes);
    drop(archive);

    let replay = app
        .clone()
        .oneshot(request("/api/v1/tools/start_export", body))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    let replay: Value =
        serde_json::from_slice(&replay.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(replay["data"]["replayed"], true);

    let original = state
        .database
        .read_project("package-project")
        .await
        .unwrap();
    state
        .database
        .delete_project("package-project", 0, "delete-before-package-import")
        .await
        .unwrap();
    let import_body = json!({
        "idempotencyKey": "package-import-1",
        "arguments": {
            "path": package_path,
            "confirm": true
        }
    });
    let imported = app
        .clone()
        .oneshot(request(
            "/api/v1/tools/import_project_package",
            import_body.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(imported.status(), StatusCode::OK);
    let imported: Value =
        serde_json::from_slice(&imported.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(imported["data"]["projectId"], "package-project");
    assert_eq!(imported["data"]["revision"], 0);
    assert_eq!(imported["data"]["mediaCount"], 2);
    let restored = state
        .database
        .read_project("package-project")
        .await
        .unwrap();
    assert_eq!(restored, original);
    assert!(
        state
            .layout
            .media_content(
                restored.document.assets[0]
                    .content_hash
                    .as_ref()
                    .unwrap()
                    .as_str()
            )
            .await
            .unwrap()
            .is_some()
    );
    let restored_contact_sheet =
        restored.document.assets[0].extensions["derivatives"]["contactSheet"]["contentHash"]
            .as_str()
            .unwrap();
    assert!(
        state
            .layout
            .media_content(restored_contact_sheet)
            .await
            .unwrap()
            .is_some()
    );

    let replayed_import = app
        .oneshot(request("/api/v1/tools/import_project_package", import_body))
        .await
        .unwrap();
    assert_eq!(replayed_import.status(), StatusCode::OK);
    let replayed_import: Value = serde_json::from_slice(
        &replayed_import
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    assert_eq!(replayed_import["data"]["replayed"], true);
}
