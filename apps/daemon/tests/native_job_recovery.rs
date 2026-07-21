use std::time::Duration;

use chrono::Utc;
use openchatcut_daemon::{AppState, Config, runtime::RuntimeDescriptor};
use openchatcut_domain::ProjectDocument;
use serde_json::json;

fn runtime(config: &Config, instance_id: &str) -> RuntimeDescriptor {
    RuntimeDescriptor {
        protocol_version: "1".to_owned(),
        instance_id: instance_id.to_owned(),
        api_base_url: "http://127.0.0.1:3210/api/v1".to_owned(),
        token_path: config.token_path.clone(),
        pid: std::process::id(),
        started_at: Utc::now(),
    }
}

#[tokio::test]
async fn daemon_restart_recovers_an_interrupted_native_project_package_export() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::for_test(temp.path().to_owned());
    let first = AppState::initialize(
        &config,
        runtime(&config, "native-recovery-before"),
        "native-recovery-token".to_owned(),
    )
    .await
    .unwrap();
    // Stop the normal claimer so this fixture can persist the exact
    // in-flight state produced by a hard daemon interruption.
    first.native_jobs.shutdown().await;
    let document = ProjectDocument::new("native-recovery-project".parse().unwrap(), "Recovery");
    first
        .database
        .create_project(
            document,
            "create-native-recovery",
            &json!({ "name": "Recovery" }),
        )
        .await
        .unwrap();
    let envelope = first
        .database
        .read_project("native-recovery-project")
        .await
        .unwrap();
    let job_input = json!({
        "outputDir": "exports",
        "outputFileName": "recovered.occproj",
        "allowOverwrite": false,
        "documentHash": envelope.document_hash,
        "assetHashes": [],
        "options": {
            "format": "project-package",
            "packageVersion": 1
        }
    });
    let (queued, _) = first
        .database
        .enqueue_job_idempotent(
            "project_package_export",
            "native-recovery-project",
            0,
            "native-recovery-export",
            &job_input,
        )
        .await
        .unwrap();
    let running = first
        .database
        .claim_job_by_id(&queued.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(running.state, "running");
    first.database.close().await;
    drop(first);

    let restarted = AppState::initialize(
        &config,
        runtime(&config, "native-recovery-after"),
        "native-recovery-token".to_owned(),
    )
    .await
    .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let current = restarted.database.read_job(&queued.id).await.unwrap();
            if current.state == "succeeded" {
                break current;
            }
            assert_ne!(current.state, "failed", "{:?}", current.error);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(completed.output.as_ref().unwrap()["recovered"], true);
    assert_eq!(completed.output.as_ref().unwrap()["revision"], 0);
    let package = restarted.layout.exports.join("recovered.occproj");
    assert!(package.is_file());
    let archive = zip::ZipArchive::new(std::fs::File::open(package).unwrap()).unwrap();
    assert_eq!(archive.len(), 2);

    restarted.native_jobs.shutdown().await;
    restarted.database.close().await;
}
