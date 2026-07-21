use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use openchatcut_domain::{
    Actor, Asset, AssetId, AssetKind, AssetProvenance, EditTransaction, IdempotencyKey, Operation,
    ProjectId, Sha256Digest, TransactionId,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    sync::{Mutex, Notify, watch},
    task::JoinHandle,
};

use crate::{
    api::classify_media,
    codex_agent::generate_image_with_codex,
    content_store::{DataLayout, HashedSource, hash_open_file, open_read_no_follow},
    persistence::{CommitResult, Database, JobRecord},
    server::EventBus,
    worker::place_generated_asset,
};

const MAX_CODEX_IMAGE_BYTES: u64 = 100 * 1024 * 1024;
const CODEX_IMAGE_MODEL: &str = "gpt-image-2";

#[derive(Clone)]
pub struct CodexImageManager {
    inner: Arc<CodexImageInner>,
}

struct CodexImageInner {
    database: Database,
    layout: DataLayout,
    codex_command: PathBuf,
    media_worker_available: bool,
    events: EventBus,
    wake: Notify,
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
    shutdown: watch::Sender<bool>,
    shutting_down: AtomicBool,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl CodexImageManager {
    pub(crate) async fn start(
        database: Database,
        layout: DataLayout,
        codex_command: PathBuf,
        media_worker_available: bool,
        events: EventBus,
    ) -> Result<Self> {
        let jobs_root = layout.temporary.join("codex-image-jobs");
        fs::create_dir_all(&jobs_root).await?;
        require_private_directory(&jobs_root).await?;
        let canonical_jobs_root = fs::canonicalize(&jobs_root).await?;
        if !canonical_jobs_root.starts_with(&layout.temporary) {
            bail!("Codex image job directory escapes the daemon temporary directory");
        }

        let (shutdown, receiver) = watch::channel(false);
        let manager = Self {
            inner: Arc::new(CodexImageInner {
                database,
                layout,
                codex_command,
                media_worker_available,
                events,
                wake: Notify::new(),
                active: Mutex::new(HashMap::new()),
                shutdown,
                shutting_down: AtomicBool::new(false),
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

    pub async fn cancel(&self, job_id: &str) {
        if let Some(sender) = self.inner.active.lock().await.get(job_id) {
            let _ = sender.send(true);
        }
        self.wake();
    }

    pub async fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        let _ = self.inner.shutdown.send(true);
        for sender in self.inner.active.lock().await.values() {
            let _ = sender.send(true);
        }
        self.wake();
        if let Some(task) = self.inner.task.lock().await.take() {
            let _ = task.await;
        }
    }
}

async fn run_loop(inner: Arc<CodexImageInner>, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        match inner
            .database
            .claim_next_job("codex_image_generation")
            .await
        {
            Ok(Some(job)) => run_claimed_job(&inner, job).await,
            Ok(None) => {
                tokio::select! {
                    _ = inner.wake.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() { break; }
                    }
                }
            }
            Err(error) => {
                tracing::error!(%error, "claim Codex image job");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CodexImageJobInput {
    provider: String,
    kind: String,
    model: Option<String>,
    prompt: String,
    seed: Option<String>,
    #[serde(default)]
    placement: Option<Value>,
    #[serde(default)]
    options: Map<String, Value>,
}

#[derive(Debug)]
struct CodexImageRunError {
    code: &'static str,
    message: String,
    retryable: bool,
    cancelled: bool,
}

impl CodexImageRunError {
    fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            cancelled: false,
        }
    }

    fn cancelled() -> Self {
        Self {
            code: "CODEX_IMAGE_CANCELLED",
            message: "Codex image generation was cancelled".to_owned(),
            retryable: false,
            cancelled: true,
        }
    }

    fn json(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "retryable": self.retryable,
        })
    }
}

async fn run_claimed_job(inner: &Arc<CodexImageInner>, job: JobRecord) {
    publish_job(&inner.events, &job);
    let (cancel, receiver) = watch::channel(false);
    inner
        .active
        .lock()
        .await
        .insert(job.id.clone(), cancel.clone());
    if inner
        .database
        .read_job(&job.id)
        .await
        .is_ok_and(|current| current.cancel_requested)
    {
        let _ = cancel.send(true);
    }

    let result = execute_codex_image_job(inner, &job, receiver).await;
    inner.active.lock().await.remove(&job.id);
    let interrupted = result
        .as_ref()
        .is_err_and(|error| error.cancelled && inner.shutting_down.load(Ordering::SeqCst));
    let explicitly_cancelled = result
        .as_ref()
        .is_err_and(|error| error.cancelled && !inner.shutting_down.load(Ordering::SeqCst));
    let updated = match result {
        Ok(output) => inner.database.complete_job(&job.id, &output).await,
        Err(_error) if interrupted => inner.database.requeue_interrupted_job(&job.id).await,
        Err(_error) if explicitly_cancelled => inner.database.mark_job_cancelled(&job.id).await,
        Err(error) => inner.database.fail_job(&job.id, &error.json()).await,
    };
    if !interrupted {
        if let Err(error) = remove_job_directory(&inner.layout, &job.id).await {
            tracing::warn!(job_id = %job.id, %error, "remove Codex image job directory");
        }
    }
    match updated {
        Ok(job) => publish_job(&inner.events, &job),
        Err(error) => tracing::error!(job_id = %job.id, %error, "persist Codex image result"),
    }
}

async fn execute_codex_image_job(
    inner: &CodexImageInner,
    job: &JobRecord,
    cancellation: watch::Receiver<bool>,
) -> Result<Value, CodexImageRunError> {
    let input: CodexImageJobInput = serde_json::from_value(job.input.clone()).map_err(|_| {
        CodexImageRunError::new(
            "CODEX_IMAGE_INVALID_JOB",
            "persisted Codex image job is invalid",
            false,
        )
    })?;
    if input.provider != "codex-image" || input.kind != "image" {
        return Err(CodexImageRunError::new(
            "CODEX_IMAGE_INVALID_JOB",
            "Codex image job has an invalid provider or kind",
            false,
        ));
    }
    if input
        .model
        .as_deref()
        .is_some_and(|model| model != CODEX_IMAGE_MODEL)
    {
        return Err(CodexImageRunError::new(
            "CODEX_IMAGE_MODEL_UNSUPPORTED",
            "Codex image generation uses gpt-image-2",
            false,
        ));
    }
    ensure_not_cancelled(&cancellation)?;

    let job_directory = job_directory(&inner.layout, &job.id);
    let checkpoint = job
        .output
        .as_ref()
        .and_then(|value| value.get("checkpoint"));
    let candidate = if checkpoint
        .and_then(|value| value.get("phase"))
        .and_then(Value::as_str)
        == Some("generated")
    {
        load_checkpointed_candidate(&job_directory, checkpoint.expect("checked above")).await?
    } else {
        reset_job_directory(&inner.layout, &job.id)
            .await
            .map_err(io_error)?;
        let running = inner
            .database
            .update_job_progress(&job.id, 0.02, Some("Generating image with Codex"))
            .await
            .map_err(database_error)?;
        publish_job(&inner.events, &running);
        let generated = generate_image_with_codex(
            &inner.codex_command,
            &job_directory,
            &input.prompt,
            cancellation.clone(),
        )
        .await
        .map_err(|error| {
            if *cancellation.borrow() {
                CodexImageRunError::cancelled()
            } else {
                CodexImageRunError::new("CODEX_IMAGE_GENERATION_FAILED", error.to_string(), true)
            }
        })?;
        let candidate =
            inspect_candidate(&job_directory, &generated.path, generated.revised_prompt).await?;
        let relative_path = candidate
            .path
            .strip_prefix(&job_directory)
            .map_err(|_| {
                CodexImageRunError::new(
                    "CODEX_IMAGE_PATH_REJECTED",
                    "generated image is outside its durable job directory",
                    false,
                )
            })?
            .to_string_lossy()
            .into_owned();
        let checkpointed = inner
            .database
            .checkpoint_job(
                &job.id,
                0.80,
                "Codex image saved; importing managed media",
                &json!({
                    "checkpoint": {
                        "phase": "generated",
                        "relativePath": relative_path,
                        "sha256": candidate.hashed.sha256,
                        "byteSize": candidate.hashed.size,
                        "mimeType": candidate.mime_type,
                        "revisedPrompt": candidate.revised_prompt,
                    }
                }),
            )
            .await
            .map_err(database_error)?;
        publish_job(&inner.events, &checkpointed);
        candidate
    };

    ensure_not_cancelled(&cancellation)?;
    let mut source = open_read_no_follow(&candidate.path)
        .await
        .map_err(io_error)?;
    let installed = inner
        .layout
        .put_hashed_media_file(&mut source, &candidate.hashed, MAX_CODEX_IMAGE_BYTES)
        .await
        .map_err(io_error)?;
    drop(source);

    let materialized =
        materialize_asset(inner, job, &input, &candidate, &installed.content.sha256).await;
    let mut materialized = match materialized {
        Ok(value) => value,
        Err(error) => {
            if installed.created
                && inner
                    .database
                    .content_hash_referenced(&installed.content.sha256)
                    .await
                    .is_ok_and(|referenced| !referenced)
            {
                let _ = inner
                    .layout
                    .remove_media_if_matches(&installed.content.sha256)
                    .await;
            }
            return Err(error);
        }
    };
    if let Some(placement) = place_generated_asset(
        &inner.database,
        &inner.events,
        job.project_id.as_deref().ok_or_else(|| {
            CodexImageRunError::new("CODEX_IMAGE_INVALID_JOB", "job has no project", false)
        })?,
        &job.id,
        &materialized.asset,
        input.placement.as_ref(),
    )
    .await
    .map_err(|error| {
        CodexImageRunError::new("CODEX_IMAGE_PLACEMENT_FAILED", error.to_string(), true)
    })? {
        materialized.revision = placement.revision;
        materialized.document_hash = placement.document_hash;
    }
    if inner.media_worker_available
        && let Err(error) = enqueue_derivatives(inner, job, &materialized.asset).await
    {
        tracing::warn!(job_id = %job.id, %error, "queue Codex image derivatives");
    }
    Ok(json!({
        "phase": "normalize",
        "provider": "codex-image",
        "model": CODEX_IMAGE_MODEL,
        "asset": materialized.asset,
        "revision": materialized.revision,
        "documentHash": materialized.document_hash,
        "replayed": materialized.replayed,
        "normalization": "validated-managed-copy-v1",
        "provenance": {
            "provider": "codex-image",
            "model": CODEX_IMAGE_MODEL,
            "prompt": input.prompt,
            "revisedPrompt": candidate.revised_prompt,
            "seed": input.seed,
            "codexAllowance": true,
        }
    }))
}

struct Candidate {
    path: PathBuf,
    hashed: HashedSource,
    mime_type: String,
    revised_prompt: Option<String>,
}

async fn inspect_candidate(
    job_directory: &Path,
    path: &Path,
    revised_prompt: Option<String>,
) -> Result<Candidate, CodexImageRunError> {
    let canonical_directory = fs::canonicalize(job_directory).await.map_err(io_error)?;
    let canonical_path = fs::canonicalize(path).await.map_err(io_error)?;
    if !canonical_path.starts_with(&canonical_directory) {
        return Err(CodexImageRunError::new(
            "CODEX_IMAGE_PATH_REJECTED",
            "generated image resolves outside its isolated directory",
            false,
        ));
    }
    let metadata = fs::symlink_metadata(path).await.map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CodexImageRunError::new(
            "CODEX_IMAGE_PATH_REJECTED",
            "generated image is not a regular non-symlink file",
            false,
        ));
    }
    let mut source = open_read_no_follow(path).await.map_err(io_error)?;
    let hashed = hash_open_file(&mut source, MAX_CODEX_IMAGE_BYTES)
        .await
        .map_err(io_error)?;
    let (kind, mime_type) = classify_media(path, &hashed.prefix).map_err(|error| {
        CodexImageRunError::new("CODEX_IMAGE_MEDIA_REJECTED", error.to_string(), false)
    })?;
    if kind != AssetKind::Image {
        return Err(CodexImageRunError::new(
            "CODEX_IMAGE_MEDIA_REJECTED",
            "Codex output is not a recognized raster image",
            false,
        ));
    }
    Ok(Candidate {
        path: canonical_path,
        hashed,
        mime_type: mime_type.unwrap_or("application/octet-stream").to_owned(),
        revised_prompt,
    })
}

async fn load_checkpointed_candidate(
    job_directory: &Path,
    checkpoint: &Value,
) -> Result<Candidate, CodexImageRunError> {
    let relative = checkpoint
        .get("relativePath")
        .and_then(Value::as_str)
        .context("Codex image checkpoint has no relativePath")
        .map_err(resume_error)?;
    let relative = PathBuf::from(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CodexImageRunError::new(
            "CODEX_IMAGE_RESUME_REJECTED",
            "Codex image checkpoint contains an unsafe path",
            false,
        ));
    }
    let revised_prompt = checkpoint
        .get("revisedPrompt")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let candidate = inspect_candidate(job_directory, &job_directory.join(relative), revised_prompt)
        .await
        .map_err(|error| {
            CodexImageRunError::new("CODEX_IMAGE_RESUME_MISSING", error.message, false)
        })?;
    let expected_hash = checkpoint.get("sha256").and_then(Value::as_str);
    let expected_size = checkpoint.get("byteSize").and_then(Value::as_u64);
    let expected_mime = checkpoint.get("mimeType").and_then(Value::as_str);
    if expected_hash != Some(candidate.hashed.sha256.as_str())
        || expected_size != Some(candidate.hashed.size)
        || expected_mime != Some(candidate.mime_type.as_str())
    {
        return Err(CodexImageRunError::new(
            "CODEX_IMAGE_RESUME_TAMPERED",
            "checkpointed Codex image no longer matches its durable digest",
            false,
        ));
    }
    Ok(candidate)
}

struct MaterializedAsset {
    asset: Asset,
    revision: u64,
    document_hash: Value,
    replayed: bool,
}

async fn materialize_asset(
    inner: &CodexImageInner,
    job: &JobRecord,
    input: &CodexImageJobInput,
    candidate: &Candidate,
    digest: &str,
) -> Result<MaterializedAsset, CodexImageRunError> {
    let project_id = job.project_id.as_deref().ok_or_else(|| {
        CodexImageRunError::new("CODEX_IMAGE_INVALID_JOB", "job has no project", false)
    })?;
    let asset_id = format!("asset:generated:{}", job.id);
    let extension = match candidate.mime_type.as_str() {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        _ => "png",
    };
    let mut asset = Asset::new(
        AssetId::new(asset_id.clone()).map_err(domain_error)?,
        format!("Codex generated image.{extension}"),
        AssetKind::Image,
    );
    asset.content_hash = Some(Sha256Digest::new(digest.to_owned()).map_err(domain_error)?);
    asset.provenance = AssetProvenance::Generated {
        provider: "codex-image".to_owned(),
        model: CODEX_IMAGE_MODEL.to_owned(),
        prompt: input.prompt.clone(),
        seed: input.seed.clone(),
    };
    asset.extensions.insert(
        "managedMedia".to_owned(),
        json!({
            "byteSize": candidate.hashed.size,
            "mimeType": candidate.mime_type,
            "mimeEvidence": "magicBytes",
            "source": "codexImageGeneration",
        }),
    );
    asset.extensions.insert(
        "generation".to_owned(),
        json!({
            "jobId": job.id,
            "provider": "codex-image",
            "model": CODEX_IMAGE_MODEL,
            "prompt": input.prompt,
            "revisedPrompt": candidate.revised_prompt,
            "seed": input.seed,
            "parameters": input.options,
            "requestedRevision": job.revision,
            "codexAllowance": true,
        }),
    );

    for _ in 0..4 {
        let current = inner
            .database
            .read_project(project_id)
            .await
            .map_err(database_error)?;
        if let Some(existing) = current
            .document
            .assets
            .iter()
            .find(|candidate| candidate.id.as_str() == asset_id)
        {
            if existing
                .extensions
                .get("generation")
                .and_then(|value| value.get("jobId"))
                .and_then(Value::as_str)
                == Some(job.id.as_str())
            {
                return Ok(MaterializedAsset {
                    asset: existing.clone(),
                    revision: current.revision,
                    document_hash: serde_json::to_value(current.document_hash)
                        .map_err(domain_error)?,
                    replayed: true,
                });
            }
            return Err(CodexImageRunError::new(
                "CODEX_IMAGE_ASSET_CONFLICT",
                "generated asset ID belongs to another operation",
                false,
            ));
        }
        let edit = EditTransaction::new(
            TransactionId::new(format!("tx:job:{}:generated-asset", job.id))
                .map_err(domain_error)?,
            ProjectId::new(project_id).map_err(domain_error)?,
            current.revision,
            IdempotencyKey::new(format!("job:{}:materialize-generation", job.id))
                .map_err(domain_error)?,
            Actor::system(),
            vec![Operation::AddAsset {
                asset: asset.clone(),
            }],
        );
        match inner.database.commit(project_id, &edit).await {
            Ok(result) => {
                let (value, replayed) = match result {
                    CommitResult::Committed(value) => (value, false),
                    CommitResult::Replayed(value) => (value, true),
                };
                let revision = value
                    .pointer("/envelope/revision")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        CodexImageRunError::new(
                            "CODEX_IMAGE_MATERIALIZATION_FAILED",
                            "asset commit returned no revision",
                            false,
                        )
                    })?;
                let document_hash = value
                    .pointer("/envelope/documentHash")
                    .cloned()
                    .unwrap_or(Value::Null);
                if !replayed {
                    inner.events.publish(
                        "revision.changed",
                        json!({
                            "projectId": project_id,
                            "transactionId": edit.transaction_id,
                            "revision": revision,
                            "documentHash": document_hash,
                            "jobId": job.id,
                        }),
                    );
                    inner.events.publish(
                        "asset.changed",
                        json!({
                            "projectId": project_id,
                            "assetId": asset_id,
                            "status": "ready",
                            "jobId": job.id,
                        }),
                    );
                }
                return Ok(MaterializedAsset {
                    asset,
                    revision,
                    document_hash,
                    replayed,
                });
            }
            Err(error) if error.code == "revisionConflict" => continue,
            Err(error) => return Err(database_error(error)),
        }
    }
    Err(CodexImageRunError::new(
        "CODEX_IMAGE_REVISION_CONFLICT",
        "project kept changing while the generated image was imported",
        true,
    ))
}

async fn enqueue_derivatives(
    inner: &CodexImageInner,
    parent_job: &JobRecord,
    asset: &Asset,
) -> Result<(), crate::error::ApiError> {
    let project_id = parent_job
        .project_id
        .as_deref()
        .ok_or_else(|| crate::error::ApiError::internal("Codex image job has no project"))?;
    let Some(digest) = asset.content_hash.as_ref() else {
        return Ok(());
    };
    let Some(content) = inner
        .layout
        .media_content(digest.as_str())
        .await
        .map_err(crate::error::ApiError::internal)?
    else {
        return Ok(());
    };
    let current = inner.database.read_project(project_id).await?;
    if !current.document.assets.iter().any(|candidate| {
        candidate.id == asset.id
            && candidate.content_hash.as_ref().map(|hash| hash.as_str()) == Some(digest.as_str())
    }) {
        return Ok(());
    }
    let input = json!({
        "assetId": asset.id,
        "assetContentHash": digest,
        "inputPath": content.path,
        "outputDir": "derived/media",
        "materializeDerivatives": true,
        "options": { "assetKind": "image" },
    });
    let (job, _) = inner
        .database
        .enqueue_job_idempotent(
            "media_derivatives",
            project_id,
            current.revision,
            &format!("codex-image-derivatives:{}", parent_job.id),
            &input,
        )
        .await?;
    inner.events.publish("job.changed", json!({ "job": job }));
    Ok(())
}

fn job_directory(layout: &DataLayout, job_id: &str) -> PathBuf {
    let digest = hex::encode(Sha256::digest(job_id.as_bytes()));
    layout
        .temporary
        .join("codex-image-jobs")
        .join(&digest[..32])
}

async fn reset_job_directory(layout: &DataLayout, job_id: &str) -> Result<()> {
    remove_job_directory(layout, job_id).await?;
    let directory = job_directory(layout, job_id);
    fs::create_dir_all(&directory).await?;
    require_private_directory(&directory).await?;
    let root = fs::canonicalize(layout.temporary.join("codex-image-jobs")).await?;
    let canonical = fs::canonicalize(&directory).await?;
    if !canonical.starts_with(root) {
        bail!("Codex image job directory escapes its private root");
    }
    Ok(())
}

async fn remove_job_directory(layout: &DataLayout, job_id: &str) -> Result<()> {
    let directory = job_directory(layout, job_id);
    let metadata = match fs::symlink_metadata(&directory).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("Codex image job path is not a regular directory");
    }
    let root = fs::canonicalize(layout.temporary.join("codex-image-jobs")).await?;
    let canonical = fs::canonicalize(&directory).await?;
    if !canonical.starts_with(root) {
        bail!("Codex image job path escapes its private root");
    }
    fs::remove_dir_all(directory).await?;
    Ok(())
}

#[cfg(unix)]
async fn require_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn require_private_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn ensure_not_cancelled(cancellation: &watch::Receiver<bool>) -> Result<(), CodexImageRunError> {
    if *cancellation.borrow() {
        Err(CodexImageRunError::cancelled())
    } else {
        Ok(())
    }
}

fn publish_job(events: &EventBus, job: &JobRecord) {
    events.publish("job.changed", json!({ "job": job }));
}

fn database_error(error: impl std::fmt::Display) -> CodexImageRunError {
    CodexImageRunError::new(
        "CODEX_IMAGE_MATERIALIZATION_FAILED",
        error.to_string(),
        false,
    )
}

fn domain_error(error: impl std::fmt::Display) -> CodexImageRunError {
    database_error(error)
}

fn io_error(error: impl std::fmt::Display) -> CodexImageRunError {
    CodexImageRunError::new("CODEX_IMAGE_IO_FAILED", error.to_string(), true)
}

fn resume_error(error: impl std::fmt::Display) -> CodexImageRunError {
    CodexImageRunError::new("CODEX_IMAGE_RESUME_REJECTED", error.to_string(), false)
}
