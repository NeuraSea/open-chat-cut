use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail};
use openchatcut_domain::{
    NleFormat, ProjectEnvelope, SubtitleFormat, TrackId, export_nle_xml, export_subtitle,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    sync::{Mutex, Notify, watch},
    task::JoinHandle,
};

use crate::{
    content_store::{DataLayout, hash_open_file, open_read_no_follow},
    persistence::{Database, JobRecord},
    project_package::{MAX_PACKAGE_BYTES, create_project_package},
    server::EventBus,
};

const MAX_NATIVE_TEXT_EXPORT_BYTES: u64 = 128 * 1024 * 1024;

/// Runs daemon-native exports from the same durable SQLite queue used by the
/// media worker. API requests only enqueue work, so closing the browser cannot
/// cancel subtitle, NLE, or project-package delivery. A hard daemon stop leaves
/// the job recoverable and startup recovery requeues it before this loop runs.
#[derive(Clone)]
pub struct NativeJobManager {
    inner: Arc<NativeJobInner>,
}

struct NativeJobInner {
    database: Database,
    layout: DataLayout,
    events: EventBus,
    wake: Notify,
    shutdown: watch::Sender<bool>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl NativeJobManager {
    pub(crate) async fn start(
        database: Database,
        layout: DataLayout,
        events: EventBus,
    ) -> Result<Self> {
        let (shutdown, receiver) = watch::channel(false);
        let manager = Self {
            inner: Arc::new(NativeJobInner {
                database,
                layout,
                events,
                wake: Notify::new(),
                shutdown,
                task: Mutex::new(None),
            }),
        };
        let inner = manager.inner.clone();
        *manager.inner.task.lock().await = Some(tokio::spawn(async move {
            run_loop(inner, receiver).await;
        }));
        Ok(manager)
    }

    pub fn wake(&self) {
        self.inner.wake.notify_one();
    }

    pub fn cancel(&self) {
        // Cancellation is persisted on the job row. Waking the loop ensures a
        // queued cancellation reaches a terminal state without waiting for the
        // periodic fallback poll.
        self.wake();
    }

    pub async fn shutdown(&self) {
        let _ = self.inner.shutdown.send(true);
        self.wake();
        if let Some(task) = self.inner.task.lock().await.take() {
            // Native writers install atomically. Let an in-flight write finish;
            // a hard process stop is recovered from SQLite on the next start.
            let _ = task.await;
        }
    }
}

async fn run_loop(inner: Arc<NativeJobInner>, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        match claim_next_native_job(&inner.database).await {
            Ok(Some(job)) => {
                inner.events.publish("job.changed", json!({ "job": &job }));
                finish_native_job(&inner.database, &inner.layout, &inner.events, job).await;
            }
            Ok(None) => {
                tokio::select! {
                    _ = inner.wake.notified() => {}
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() { break; }
                    }
                }
            }
            Err(error) => {
                tracing::error!(%error, "claim daemon-native export job");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

async fn claim_next_native_job(
    database: &Database,
) -> Result<Option<JobRecord>, crate::error::ApiError> {
    for kind in [
        "subtitle_export",
        "nle_xml_export",
        "project_package_export",
    ] {
        if let Some(job) = database.claim_next_job(kind).await? {
            return Ok(Some(job));
        }
    }
    Ok(None)
}

async fn finish_native_job(
    database: &Database,
    layout: &DataLayout,
    events: &EventBus,
    job: JobRecord,
) {
    let label = match job.kind.as_str() {
        "subtitle_export" => "Rendering caption delivery from pinned revision",
        "nle_xml_export" => "Rendering editable NLE handoff from pinned revision",
        "project_package_export" => "Packaging pinned project and managed media",
        _ => "Running daemon-native export",
    };
    let job = match database
        .update_job_progress(&job.id, 0.05, Some(label))
        .await
    {
        Ok(updated) => {
            events.publish("job.changed", json!({ "job": &updated }));
            updated
        }
        Err(error) => {
            tracing::warn!(job_id = %job.id, %error, "persist native export progress");
            job
        }
    };
    let outcome = execute_native_job(database, layout, &job).await;
    let updated = match outcome {
        Ok(output) => database.complete_job(&job.id, &output).await,
        Err(error) => match database.read_job(&job.id).await {
            Ok(current) if current.cancel_requested => database.mark_job_cancelled(&job.id).await,
            _ => {
                tracing::warn!(job_id = %job.id, %error, "daemon-native export failed");
                database
                    .fail_job(
                        &job.id,
                        &json!({
                            "code": "NATIVE_EXPORT_FAILED",
                            "message": error.to_string(),
                        }),
                    )
                    .await
            }
        },
    };
    match updated {
        Ok(job) => events.publish("job.changed", json!({ "job": job })),
        Err(error) => tracing::error!(job_id = %job.id, %error, "finish daemon-native export"),
    }
}

async fn execute_native_job(
    database: &Database,
    layout: &DataLayout,
    job: &JobRecord,
) -> Result<Value> {
    ensure_not_cancelled(database, job).await?;
    let project_id = job
        .project_id
        .as_deref()
        .context("native job has no projectId")?;
    let revision = job.revision.context("native job has no pinned revision")?;
    let envelope = database
        .read_project_revision(project_id, revision)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    verify_pinned_document(job, &envelope)?;
    match job.kind.as_str() {
        "subtitle_export" => recover_subtitle(database, layout, job, &envelope).await,
        "nle_xml_export" => recover_nle(database, layout, job, &envelope).await,
        "project_package_export" => recover_package(database, layout, job, &envelope).await,
        other => bail!("unsupported daemon-native job kind {other}"),
    }
}

async fn recover_subtitle(
    database: &Database,
    layout: &DataLayout,
    job: &JobRecord,
    envelope: &ProjectEnvelope,
) -> Result<Value> {
    let format: SubtitleFormat = serde_json::from_value(
        job.input
            .pointer("/options/format")
            .cloned()
            .context("subtitle job has no format")?,
    )?;
    let track_id = job
        .input
        .pointer("/options/captionTrackId")
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value::<TrackId>)
        .transpose()?;
    let content = export_subtitle(&envelope.document, format, track_id.as_ref())?;
    let bytes = content.as_bytes();
    verify_planned_content(job, bytes)?;
    ensure_not_cancelled(database, job).await?;
    let path = install_or_accept_same_bytes(layout, job, bytes).await?;
    Ok(json!({
        "outputPath": path,
        "format": format,
        "revision": envelope.revision,
        "documentHash": envelope.document_hash,
        "sha256": hex::encode(Sha256::digest(bytes)),
        "byteSize": bytes.len(),
        "renderer": "rust-caption-export-v1",
        "verified": true,
        "recovered": true,
    }))
}

async fn recover_nle(
    database: &Database,
    layout: &DataLayout,
    job: &JobRecord,
    envelope: &ProjectEnvelope,
) -> Result<Value> {
    let format: NleFormat = serde_json::from_value(
        job.input
            .pointer("/options/format")
            .cloned()
            .context("NLE job has no format")?,
    )?;
    let mut asset_file_uris = BTreeMap::new();
    for asset in &envelope.document.assets {
        let Some(digest) = &asset.content_hash else {
            continue;
        };
        let content = layout
            .media_content(digest.as_str())
            .await?
            .with_context(|| format!("managed media {} is missing", asset.id))?;
        let uri = url::Url::from_file_path(&content.path)
            .map_err(|_| anyhow::anyhow!("managed media path is not a file URI"))?;
        asset_file_uris.insert(asset.id.to_string(), uri.to_string());
    }
    let exported = export_nle_xml(&envelope.document, format, &asset_file_uris)?;
    let bytes = exported.content.as_bytes();
    verify_planned_content(job, bytes)?;
    ensure_not_cancelled(database, job).await?;
    let path = install_or_accept_same_bytes(layout, job, bytes).await?;
    Ok(json!({
        "outputPath": path,
        "format": format,
        "revision": envelope.revision,
        "documentHash": envelope.document_hash,
        "sha256": hex::encode(Sha256::digest(bytes)),
        "byteSize": bytes.len(),
        "renderer": format.renderer(),
        "mediaClipCount": exported.media_clip_count,
        "unsupportedItemIds": exported.unsupported_item_ids,
        "verified": true,
        "recovered": true,
    }))
}

async fn recover_package(
    database: &Database,
    layout: &DataLayout,
    job: &JobRecord,
    envelope: &ProjectEnvelope,
) -> Result<Value> {
    verify_package_asset_snapshot(job, envelope)?;
    let package = create_project_package(layout, envelope).await?;
    if let Err(error) = ensure_not_cancelled(database, job).await {
        let _ = tokio::fs::remove_file(&package.temporary_path).await;
        return Err(error);
    }
    let file_name = output_file_name(job)?;
    let allow_overwrite = allow_overwrite(job);
    let destination = layout.exports.join(file_name);
    let path = match layout
        .install_export_file(&package.temporary_path, file_name, allow_overwrite)
        .await
    {
        Ok(path) => path,
        Err(_error)
            if !allow_overwrite
                && existing_hash(&destination, MAX_PACKAGE_BYTES).await?
                    == Some(package.sha256.clone()) =>
        {
            let _ = tokio::fs::remove_file(&package.temporary_path).await;
            destination
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&package.temporary_path).await;
            return Err(error);
        }
    };
    Ok(json!({
        "outputPath": path,
        "format": "project-package",
        "packageVersion": 1,
        "revision": envelope.revision,
        "documentHash": envelope.document_hash,
        "sha256": package.sha256,
        "byteSize": package.byte_size,
        "mediaCount": package.media_count,
        "renderer": "openchatcut-project-package-v1",
        "verified": true,
        "recovered": true,
    }))
}

fn verify_pinned_document(job: &JobRecord, envelope: &ProjectEnvelope) -> Result<()> {
    if job.input.get("documentHash") != Some(&serde_json::to_value(&envelope.document_hash)?) {
        bail!("native job document hash does not match its pinned revision");
    }
    Ok(())
}

fn verify_package_asset_snapshot(job: &JobRecord, envelope: &ProjectEnvelope) -> Result<()> {
    let actual = envelope
        .document
        .assets
        .iter()
        .map(|asset| json!({ "assetId": asset.id, "contentHash": asset.content_hash }))
        .collect::<Vec<_>>();
    if job.input.get("assetHashes") != Some(&Value::Array(actual)) {
        bail!("project package asset snapshot does not match its pinned revision");
    }
    Ok(())
}

fn verify_planned_content(job: &JobRecord, bytes: &[u8]) -> Result<()> {
    let hash = hex::encode(Sha256::digest(bytes));
    if job.input.get("contentSha256").and_then(Value::as_str) != Some(hash.as_str())
        || job.input.get("contentBytes").and_then(Value::as_u64) != Some(bytes.len() as u64)
    {
        bail!("regenerated native export does not match its durable plan");
    }
    Ok(())
}

async fn ensure_not_cancelled(database: &Database, job: &JobRecord) -> Result<()> {
    let current = database
        .read_job(&job.id)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if current.cancel_requested {
        bail!("native export was cancelled");
    }
    Ok(())
}

async fn install_or_accept_same_bytes(
    layout: &DataLayout,
    job: &JobRecord,
    bytes: &[u8],
) -> Result<PathBuf> {
    let file_name = output_file_name(job)?;
    let destination = layout.exports.join(file_name);
    let allow_overwrite = allow_overwrite(job);
    match layout
        .install_export_bytes(file_name, bytes, allow_overwrite)
        .await
    {
        Ok(path) => Ok(path),
        Err(_error)
            if !allow_overwrite
                && existing_hash(&destination, MAX_NATIVE_TEXT_EXPORT_BYTES).await?
                    == Some(hex::encode(Sha256::digest(bytes))) =>
        {
            Ok(destination)
        }
        Err(error) => Err(error),
    }
}

fn output_file_name(job: &JobRecord) -> Result<&str> {
    job.input
        .get("outputFileName")
        .and_then(Value::as_str)
        .context("native job has no output file name")
}

fn allow_overwrite(job: &JobRecord) -> bool {
    job.input
        .get("allowOverwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn existing_hash(path: &std::path::Path, maximum_bytes: u64) -> Result<Option<String>> {
    let mut file = match open_read_no_follow(path).await {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let hashed = hash_open_file(&mut file, maximum_bytes).await?;
    Ok(Some(hashed.sha256))
}
