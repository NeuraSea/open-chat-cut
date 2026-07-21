use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use openchatcut_domain::{
    Actor, Asset, AssetId, AssetKind, AssetProvenance, EditTransaction, IdempotencyKey, Operation,
    ProjectId, Sha256Digest, TransactionId,
};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    sync::{Mutex, Notify, watch},
    task::JoinHandle,
};
use url::Url;

use crate::{
    api::classify_media,
    content_store::{DataLayout, HashedSource, hash_open_file, open_read_no_follow},
    persistence::{CommitResult, Database, JobRecord},
    remote_import::{
        download_media_with_policy_cancellable, download_public_html_cancellable,
        validate_remote_url,
    },
    server::EventBus,
    worker::{DirectWorkerOutcome, execute_direct_worker_request},
};

const MAX_HTML_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PUBLIC_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PUBLIC_ASSET_TOTAL_BYTES: u64 = 96 * 1024 * 1024;
const MAX_PUBLIC_ASSETS: usize = 8;
const MAX_SCREENSHOT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct WebCaptureManager {
    inner: Arc<WebCaptureInner>,
}

struct WebCaptureInner {
    database: Database,
    layout: DataLayout,
    media_worker_command: PathBuf,
    events: EventBus,
    wake: Notify,
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
    shutdown: watch::Sender<bool>,
    shutting_down: AtomicBool,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl WebCaptureManager {
    pub(crate) async fn start(
        database: Database,
        layout: DataLayout,
        media_worker_command: PathBuf,
        events: EventBus,
    ) -> Result<Self> {
        let (shutdown, receiver) = watch::channel(false);
        let manager = Self {
            inner: Arc::new(WebCaptureInner {
                database,
                layout,
                media_worker_command,
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

async fn run_loop(inner: Arc<WebCaptureInner>, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        match inner.database.claim_next_job("web_capture").await {
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
                tracing::error!(%error, "claim website capture job");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebCaptureInput {
    provider: String,
    kind: String,
    prompt: String,
    source_url: String,
}

#[derive(Debug)]
struct WebCaptureError {
    code: &'static str,
    message: String,
    cancelled: bool,
}

impl WebCaptureError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            cancelled: false,
        }
    }

    fn cancelled() -> Self {
        Self {
            code: "WEB_CAPTURE_CANCELLED",
            message: "website capture was cancelled".to_owned(),
            cancelled: true,
        }
    }

    fn json(&self) -> Value {
        json!({ "code": self.code, "message": self.message })
    }
}

async fn run_claimed_job(inner: &Arc<WebCaptureInner>, job: JobRecord) {
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
    let result = execute_web_capture(inner, &job, receiver).await;
    inner.active.lock().await.remove(&job.id);
    let interrupted = result
        .as_ref()
        .is_err_and(|error| error.cancelled && inner.shutting_down.load(Ordering::SeqCst));
    let updated = match result {
        Ok(output) => inner.database.complete_job(&job.id, &output).await,
        Err(_error) if interrupted => inner.database.requeue_interrupted_job(&job.id).await,
        Err(error) if error.cancelled => inner.database.mark_job_cancelled(&job.id).await,
        Err(error) => inner.database.fail_job(&job.id, &error.json()).await,
    };
    if !interrupted && let Err(error) = cleanup_web_capture_artifacts(inner, &job.id).await {
        tracing::warn!(job_id = %job.id, %error, "clean website capture artifacts");
    }
    match updated {
        Ok(job) => publish_job(&inner.events, &job),
        Err(error) => tracing::error!(job_id = %job.id, %error, "persist website capture result"),
    }
}

async fn execute_web_capture(
    inner: &WebCaptureInner,
    job: &JobRecord,
    mut cancellation: watch::Receiver<bool>,
) -> Result<Value, WebCaptureError> {
    let input: WebCaptureInput = serde_json::from_value(job.input.clone()).map_err(|_| {
        WebCaptureError::new(
            "INVALID_WEB_CAPTURE_JOB",
            "persisted website capture is invalid",
        )
    })?;
    if input.provider != "local-web-capture" || input.kind != "webCapture" {
        return Err(WebCaptureError::new(
            "INVALID_WEB_CAPTURE_JOB",
            "website capture provider or kind is invalid",
        ));
    }
    ensure_not_cancelled(&cancellation)?;
    let checkpoint = job
        .output
        .as_ref()
        .and_then(|value| value.get("checkpoint"));
    let staged = if checkpoint
        .and_then(|value| value.get("phase"))
        .and_then(Value::as_str)
        == Some("capture")
    {
        load_staged_capture(inner, job, checkpoint.expect("capture checkpoint exists")).await?
    } else {
        download_and_stage_capture(inner, job, &input, &mut cancellation).await?
    };
    ensure_not_cancelled(&cancellation)?;
    checkpoint_job(
        inner,
        job,
        0.76,
        "Rendering isolated offline website snapshot",
        checkpoint_for_layout(&inner.layout, &staged, "capture")?,
    )
    .await?;
    let request = json!({
        "jobId": job.id,
        "kind": "capture_web_page",
        "projectId": job.project_id,
        "inputPath": staged.html_path,
        "outputDir": "derived/web-capture",
        "options": {
            "sourceUrl": staged.source_url,
            "assetPaths": staged.assets.iter().map(|asset| &asset.path).collect::<Vec<_>>(),
        },
    });
    let outcome = execute_direct_worker_request(
        &inner.media_worker_command,
        &inner.layout.root,
        request,
        cancellation,
    )
    .await
    .map_err(|error| WebCaptureError::new("WEB_CAPTURE_WORKER_FAILED", safe_error(error)))?;
    let result = match outcome {
        DirectWorkerOutcome::Cancelled => return Err(WebCaptureError::cancelled()),
        DirectWorkerOutcome::Failed(error) => {
            return Err(WebCaptureError::new(
                "WEB_CAPTURE_WORKER_FAILED",
                safe_worker_error(&error),
            ));
        }
        DirectWorkerOutcome::Completed(result) => result,
    };
    let observation = verify_capture_result(inner, job, &staged, &result).await?;
    let materialized = materialize_capture(inner, job, &input, &staged, observation).await?;
    if let Err(error) = enqueue_capture_derivatives(inner, job, &materialized).await {
        tracing::warn!(job_id = %job.id, %error, "queue website capture derivatives");
    }
    Ok(json!({
        "provider": input.provider,
        "kind": input.kind,
        "sourceUrl": staged.source_url,
        "assets": materialized.assets,
        "revision": materialized.revision,
        "documentHash": materialized.document_hash,
        "replayed": materialized.replayed,
        "extraction": materialized.extraction,
        "security": {
            "htmlAndAssetsDownloadedByDaemon": true,
            "dnsPinnedPerRedirect": true,
            "chromiumNetworkAccess": "disabled",
            "javaScriptEnabled": false,
            "trust": "untrustedPublicWeb",
        },
    }))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedAssetCheckpoint {
    relative_path: String,
    sha256: String,
    byte_size: u64,
    source_name: String,
    source_url: String,
    mime_type: String,
}

struct StagedPublicAsset {
    path: PathBuf,
    hashed: HashedSource,
    source_name: String,
    source_url: String,
    mime_type: String,
}

struct StagedCapture {
    html_path: PathBuf,
    html_hashed: HashedSource,
    source_url: String,
    assets: Vec<StagedPublicAsset>,
}

async fn download_and_stage_capture(
    inner: &WebCaptureInner,
    job: &JobRecord,
    input: &WebCaptureInput,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<StagedCapture, WebCaptureError> {
    checkpoint_job(
        inner,
        job,
        0.02,
        "Downloading approved website HTML",
        json!({ "phase": "download" }),
    )
    .await?;
    let requested = Url::parse(&input.source_url).map_err(|_| {
        WebCaptureError::new("INVALID_WEB_CAPTURE_URL", "sourceUrl is not a valid URL")
    })?;
    validate_remote_url(&requested)
        .map_err(|error| WebCaptureError::new("INVALID_WEB_CAPTURE_URL", safe_error(error)))?;
    let stage_directory = capture_stage_directory(&inner.layout, &job.id);
    reset_private_directory(&stage_directory)
        .await
        .map_err(web_capture_io_error)?;
    let download = download_public_html_cancellable(
        &input.source_url,
        &inner.layout.temporary,
        MAX_HTML_BYTES,
        cancellation.clone(),
    )
    .await
    .map_err(|error| {
        if *cancellation.borrow() {
            WebCaptureError::cancelled()
        } else {
            WebCaptureError::new("WEB_CAPTURE_DOWNLOAD_FAILED", safe_error(error))
        }
    })?;
    ensure_not_cancelled(cancellation)?;
    let html_path = stage_directory.join("page.html");
    fs::rename(&download.temporary_path, &html_path)
        .await
        .map_err(web_capture_io_error)?;
    let html = fs::read_to_string(&html_path).await.map_err(|_| {
        WebCaptureError::new(
            "INVALID_WEB_CAPTURE_HTML",
            "website HTML must be valid UTF-8",
        )
    })?;
    let candidates = extract_public_asset_urls(&html, &download.final_url);
    let mut assets = Vec::new();
    let mut total_bytes = 0_u64;
    for candidate in candidates.into_iter().take(MAX_PUBLIC_ASSETS * 3) {
        ensure_not_cancelled(cancellation)?;
        if assets.len() == MAX_PUBLIC_ASSETS {
            break;
        }
        let downloaded = match download_media_with_policy_cancellable(
            candidate.as_str(),
            None,
            &inner.layout.temporary,
            MAX_PUBLIC_ASSET_BYTES,
            false,
            cancellation.clone(),
        )
        .await
        {
            Ok(downloaded) => downloaded,
            Err(_) if *cancellation.borrow() => return Err(WebCaptureError::cancelled()),
            Err(_) => continue,
        };
        let (kind, mime_type) = match classify_media(
            Path::new(&downloaded.source_name),
            &downloaded.hashed.prefix,
        ) {
            Ok(value) => value,
            Err(_) => {
                let _ = fs::remove_file(&downloaded.temporary_path).await;
                continue;
            }
        };
        if kind != AssetKind::Image {
            let _ = fs::remove_file(&downloaded.temporary_path).await;
            continue;
        }
        total_bytes = total_bytes
            .checked_add(downloaded.hashed.size)
            .ok_or_else(|| {
                WebCaptureError::new("WEB_CAPTURE_ASSET_LIMIT", "public asset size overflow")
            })?;
        if total_bytes > MAX_PUBLIC_ASSET_TOTAL_BYTES {
            let _ = fs::remove_file(&downloaded.temporary_path).await;
            break;
        }
        let mime_type = mime_type.unwrap_or("application/octet-stream").to_owned();
        let suffix = image_suffix(&mime_type);
        let destination = stage_directory.join(format!("asset-{:02}.{suffix}", assets.len()));
        fs::rename(&downloaded.temporary_path, &destination)
            .await
            .map_err(web_capture_io_error)?;
        assets.push(StagedPublicAsset {
            path: destination,
            hashed: downloaded.hashed,
            source_name: downloaded.source_name,
            source_url: redact_url(&downloaded.final_url),
            mime_type,
        });
    }
    let staged = StagedCapture {
        html_path,
        html_hashed: download.hashed,
        source_url: redact_url(&download.final_url),
        assets,
    };
    checkpoint_job(
        inner,
        job,
        0.72,
        "Website HTML and public images saved locally",
        checkpoint_for_layout(&inner.layout, &staged, "capture")?,
    )
    .await?;
    Ok(staged)
}

fn checkpoint_for_layout(
    layout: &DataLayout,
    staged: &StagedCapture,
    phase: &str,
) -> Result<Value, WebCaptureError> {
    let html_relative = staged
        .html_path
        .strip_prefix(&layout.root)
        .map_err(|_| {
            WebCaptureError::new("WEB_CAPTURE_STAGING_FAILED", "HTML escaped the data root")
        })?
        .to_string_lossy()
        .into_owned();
    let assets = staged
        .assets
        .iter()
        .map(|asset| {
            let relative_path = asset
                .path
                .strip_prefix(&layout.root)
                .map_err(|_| {
                    WebCaptureError::new(
                        "WEB_CAPTURE_STAGING_FAILED",
                        "public asset escaped the data root",
                    )
                })?
                .to_string_lossy()
                .into_owned();
            Ok(StagedAssetCheckpoint {
                relative_path,
                sha256: asset.hashed.sha256.clone(),
                byte_size: asset.hashed.size,
                source_name: asset.source_name.clone(),
                source_url: asset.source_url.clone(),
                mime_type: asset.mime_type.clone(),
            })
        })
        .collect::<Result<Vec<_>, WebCaptureError>>()?;
    Ok(json!({
        "phase": phase,
        "sourceUrl": staged.source_url,
        "htmlRelativePath": html_relative,
        "htmlSha256": staged.html_hashed.sha256,
        "htmlByteSize": staged.html_hashed.size,
        "assets": assets,
    }))
}

async fn load_staged_capture(
    inner: &WebCaptureInner,
    job: &JobRecord,
    checkpoint: &Value,
) -> Result<StagedCapture, WebCaptureError> {
    let expected_directory = capture_stage_directory(&inner.layout, &job.id);
    let html_relative = safe_relative_path(checkpoint, "htmlRelativePath")?;
    let html_path = inner.layout.root.join(&html_relative);
    if html_path != expected_directory.join("page.html") {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            "checkpointed HTML path is unexpected",
        ));
    }
    let html_hashed = verify_staged_file(
        &html_path,
        checkpoint.get("htmlSha256").and_then(Value::as_str),
        checkpoint.get("htmlByteSize").and_then(Value::as_u64),
        MAX_HTML_BYTES,
    )
    .await?;
    let source_url = bounded_checkpoint_string(checkpoint.get("sourceUrl"), 2_000, "sourceUrl")?;
    validate_checkpoint_url(&source_url)?;
    let asset_values = checkpoint
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            WebCaptureError::new(
                "WEB_CAPTURE_RESUME_REJECTED",
                "checkpoint has no public asset list",
            )
        })?;
    if asset_values.len() > MAX_PUBLIC_ASSETS {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            "checkpoint has too many public assets",
        ));
    }
    let mut assets = Vec::with_capacity(asset_values.len());
    for value in asset_values {
        let saved: StagedAssetCheckpoint = serde_json::from_value(value.clone()).map_err(|_| {
            WebCaptureError::new(
                "WEB_CAPTURE_RESUME_REJECTED",
                "checkpoint public asset metadata is invalid",
            )
        })?;
        validate_staged_asset_metadata(&saved)?;
        let relative = safe_relative_string(&saved.relative_path)?;
        let path = inner.layout.root.join(relative);
        if path.parent() != Some(expected_directory.as_path()) {
            return Err(WebCaptureError::new(
                "WEB_CAPTURE_RESUME_REJECTED",
                "checkpoint public asset path is unexpected",
            ));
        }
        let hashed = verify_staged_file(
            &path,
            Some(&saved.sha256),
            Some(saved.byte_size),
            MAX_PUBLIC_ASSET_BYTES,
        )
        .await?;
        let (kind, mime_type) = classify_media(Path::new(&saved.source_name), &hashed.prefix)
            .map_err(|error| {
                WebCaptureError::new("WEB_CAPTURE_RESUME_REJECTED", error.to_string())
            })?;
        if kind != AssetKind::Image || mime_type != Some(saved.mime_type.as_str()) {
            return Err(WebCaptureError::new(
                "WEB_CAPTURE_RESUME_REJECTED",
                "checkpoint public asset type changed",
            ));
        }
        assets.push(StagedPublicAsset {
            path,
            hashed,
            source_name: saved.source_name,
            source_url: saved.source_url,
            mime_type: saved.mime_type,
        });
    }
    Ok(StagedCapture {
        html_path,
        html_hashed,
        source_url,
        assets,
    })
}

async fn verify_staged_file(
    path: &Path,
    expected_hash: Option<&str>,
    expected_size: Option<u64>,
    maximum_bytes: u64,
) -> Result<HashedSource, WebCaptureError> {
    let mut file = open_read_no_follow(path).await.map_err(|_| {
        WebCaptureError::new(
            "WEB_CAPTURE_RESUME_MISSING",
            "checkpointed website capture input is missing",
        )
    })?;
    let hashed = hash_open_file(&mut file, maximum_bytes)
        .await
        .map_err(web_capture_io_error)?;
    if expected_hash != Some(hashed.sha256.as_str()) || expected_size != Some(hashed.size) {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESUME_TAMPERED",
            "checkpointed website capture input no longer matches its digest",
        ));
    }
    Ok(hashed)
}

struct CaptureObservation {
    screenshot_path: PathBuf,
    screenshot_hashed: HashedSource,
    width: u32,
    height: u32,
    extraction: Value,
}

async fn verify_capture_result(
    inner: &WebCaptureInner,
    job: &JobRecord,
    staged: &StagedCapture,
    result: &Value,
) -> Result<CaptureObservation, WebCaptureError> {
    let expected = capture_screenshot_path(&inner.layout, &job.id);
    let reported = result
        .get("screenshotPath")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            WebCaptureError::new(
                "WEB_CAPTURE_RESULT_REJECTED",
                "worker returned no screenshot path",
            )
        })?;
    let canonical_expected = fs::canonicalize(&expected)
        .await
        .map_err(web_capture_io_error)?;
    let canonical_reported = fs::canonicalize(reported)
        .await
        .map_err(web_capture_io_error)?;
    if canonical_expected != canonical_reported {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            "worker returned an unexpected screenshot path",
        ));
    }
    let mut screenshot = open_read_no_follow(&canonical_expected)
        .await
        .map_err(web_capture_io_error)?;
    let hashed = hash_open_file(&mut screenshot, MAX_SCREENSHOT_BYTES)
        .await
        .map_err(web_capture_io_error)?;
    if hashed.prefix.len() < 24 || !hashed.prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            "worker screenshot is not PNG",
        ));
    }
    let width = u32::from_be_bytes(hashed.prefix[16..20].try_into().expect("four PNG bytes"));
    let height = u32::from_be_bytes(hashed.prefix[20..24].try_into().expect("four PNG bytes"));
    if !(1..=16_384).contains(&width) || !(1..=16_384).contains(&height) {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            "worker screenshot dimensions are unsafe",
        ));
    }
    if result.get("width").and_then(Value::as_u64) != Some(width as u64)
        || result.get("height").and_then(Value::as_u64) != Some(height as u64)
        || result.get("sourceUrl").and_then(Value::as_str) != Some(staged.source_url.as_str())
        || result.get("publicAssetCount").and_then(Value::as_u64)
            != Some(staged.assets.len() as u64)
        || result.get("networkAccess").and_then(Value::as_str) != Some("disabled")
        || result.get("javaScriptEnabled").and_then(Value::as_bool) != Some(false)
        || result.get("sandboxOrigin").and_then(Value::as_str) != Some("about:blank")
        || result.get("renderer").and_then(Value::as_str) != Some("isolated-offline-chromium-v1")
    {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            "worker security or screenshot metadata does not match the request",
        ));
    }
    let title = bounded_result_text(result.get("title"), 300, "title")?;
    let description = bounded_result_text(result.get("description"), 800, "description")?;
    let selling_points = bounded_result_text_array(result.get("sellingPoints"), 24, 300)?;
    let brand_colors = bounded_brand_colors(result.get("brandColors"))?;
    let blocked_requests = result
        .get("blockedRequestCount")
        .and_then(Value::as_u64)
        .filter(|value| *value <= 100_000)
        .ok_or_else(|| {
            WebCaptureError::new(
                "WEB_CAPTURE_RESULT_REJECTED",
                "worker returned an invalid blocked request count",
            )
        })?;
    Ok(CaptureObservation {
        screenshot_path: canonical_expected,
        screenshot_hashed: hashed,
        width,
        height,
        extraction: json!({
            "title": title,
            "description": description,
            "sellingPoints": selling_points,
            "brandColors": brand_colors,
            "blockedRequestCount": blocked_requests,
            "renderer": "isolated-offline-chromium-v1",
            "trust": "untrustedPublicWeb",
        }),
    })
}

struct MaterializedCapture {
    assets: Vec<Asset>,
    revision: u64,
    document_hash: Value,
    replayed: bool,
    extraction: Value,
}

async fn enqueue_capture_derivatives(
    inner: &WebCaptureInner,
    job: &JobRecord,
    materialized: &MaterializedCapture,
) -> Result<()> {
    let project_id = job
        .project_id
        .as_deref()
        .context("website capture has no project")?;
    for (index, asset) in materialized.assets.iter().enumerate() {
        let Some(digest) = asset.content_hash.as_ref() else {
            continue;
        };
        let Some(content) = inner.layout.media_content(digest.as_str()).await? else {
            continue;
        };
        let input = json!({
            "assetId": asset.id,
            "assetContentHash": digest,
            "inputPath": content.path,
            "outputDir": "derived/media",
            "materializeDerivatives": true,
            "options": { "assetKind": "image" },
        });
        let (derivative_job, _) = inner
            .database
            .enqueue_job_idempotent(
                "media_derivatives",
                project_id,
                materialized.revision,
                &format!("web-capture-derivatives:{}:{index}", job.id),
                &input,
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        publish_job(&inner.events, &derivative_job);
    }
    Ok(())
}

async fn materialize_capture(
    inner: &WebCaptureInner,
    job: &JobRecord,
    input: &WebCaptureInput,
    staged: &StagedCapture,
    observation: CaptureObservation,
) -> Result<MaterializedCapture, WebCaptureError> {
    let project_id = job.project_id.as_deref().ok_or_else(|| {
        WebCaptureError::new("INVALID_WEB_CAPTURE_JOB", "website capture has no project")
    })?;
    let mut screenshot_file = open_read_no_follow(&observation.screenshot_path)
        .await
        .map_err(web_capture_io_error)?;
    let screenshot_installed = inner
        .layout
        .put_hashed_media_file(
            &mut screenshot_file,
            &observation.screenshot_hashed,
            MAX_SCREENSHOT_BYTES,
        )
        .await
        .map_err(web_capture_io_error)?;
    drop(screenshot_file);
    let _ = fs::remove_file(&observation.screenshot_path).await;

    let mut installed_assets = Vec::with_capacity(staged.assets.len());
    for staged_asset in &staged.assets {
        let mut file = open_read_no_follow(&staged_asset.path)
            .await
            .map_err(web_capture_io_error)?;
        let installed = inner
            .layout
            .put_hashed_media_file(&mut file, &staged_asset.hashed, MAX_PUBLIC_ASSET_BYTES)
            .await
            .map_err(web_capture_io_error)?;
        installed_assets.push(installed);
    }

    let screenshot_id = format!("asset:web-capture:{}", job.id);
    let title = observation
        .extraction
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("Website");
    let mut screenshot = Asset::new(
        AssetId::new(screenshot_id.clone()).map_err(domain_error)?,
        format!("{title} website snapshot"),
        AssetKind::Image,
    );
    screenshot.content_hash =
        Some(Sha256Digest::new(screenshot_installed.content.sha256.clone()).map_err(domain_error)?);
    screenshot.width = Some(observation.width);
    screenshot.height = Some(observation.height);
    screenshot.provenance = AssetProvenance::Generated {
        provider: input.provider.clone(),
        model: "chromium-offline-snapshot-v1".to_owned(),
        prompt: input.prompt.clone(),
        seed: None,
    };
    let public_asset_ids = staged
        .assets
        .iter()
        .enumerate()
        .map(|(index, _)| format!("asset:web-capture:{}:media:{index}", job.id))
        .collect::<Vec<_>>();
    screenshot.extensions.insert(
        "managedMedia".to_owned(),
        json!({
            "byteSize": screenshot_installed.content.size,
            "mimeType": "image/png",
            "mimeEvidence": "workerAndDaemonValidation",
            "source": "isolatedWebCapture",
        }),
    );
    screenshot.extensions.insert(
        "webCapture".to_owned(),
        json!({
            "jobId": job.id,
            "sourceUrl": staged.source_url,
            "extraction": observation.extraction,
            "publicAssetIds": public_asset_ids,
            "requestedRevision": job.revision,
            "trust": "untrustedPublicWeb",
            "networkPolicy": "daemon-download-chromium-offline-v1",
        }),
    );

    let mut assets = vec![screenshot];
    for (index, (staged_asset, installed)) in staged
        .assets
        .iter()
        .zip(installed_assets.iter())
        .enumerate()
    {
        let mut asset = Asset::new(
            AssetId::new(format!("asset:web-capture:{}:media:{index}", job.id))
                .map_err(domain_error)?,
            staged_asset.source_name.clone(),
            AssetKind::Image,
        );
        asset.content_hash =
            Some(Sha256Digest::new(installed.content.sha256.clone()).map_err(domain_error)?);
        asset.provenance = AssetProvenance::Imported {
            source_name: Some(staged_asset.source_name.clone()),
        };
        asset.extensions.insert(
            "managedMedia".to_owned(),
            json!({
                "byteSize": installed.content.size,
                "mimeType": staged_asset.mime_type,
                "mimeEvidence": "downloadSignatureValidation",
                "source": "websitePublicAsset",
            }),
        );
        asset.extensions.insert(
            "webCaptureSource".to_owned(),
            json!({
                "jobId": job.id,
                "pageUrl": staged.source_url,
                "assetUrl": staged_asset.source_url,
                "redirectsValidated": true,
                "trust": "untrustedPublicWeb",
            }),
        );
        assets.push(asset);
    }

    let expected_ids = assets
        .iter()
        .map(|asset| asset.id.to_string())
        .collect::<HashSet<_>>();
    for _ in 0..4 {
        let current = inner
            .database
            .read_project(project_id)
            .await
            .map_err(|error| {
                WebCaptureError::new("WEB_CAPTURE_COMMIT_FAILED", error.to_string())
            })?;
        let existing = current
            .document
            .assets
            .iter()
            .filter(|asset| expected_ids.contains(asset.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !existing.is_empty() {
            if existing.len() == expected_ids.len()
                && existing.iter().all(|asset| {
                    asset
                        .extensions
                        .get(if asset.id.as_str() == screenshot_id {
                            "webCapture"
                        } else {
                            "webCaptureSource"
                        })
                        .and_then(|value| value.get("jobId"))
                        .and_then(Value::as_str)
                        == Some(job.id.as_str())
                })
            {
                return Ok(MaterializedCapture {
                    assets: existing,
                    revision: current.revision,
                    document_hash: serde_json::to_value(current.document_hash)
                        .map_err(domain_error)?,
                    replayed: true,
                    extraction: observation.extraction,
                });
            }
            return Err(WebCaptureError::new(
                "WEB_CAPTURE_ASSET_CONFLICT",
                "website capture asset IDs are already occupied",
            ));
        }
        let edit = EditTransaction::new(
            TransactionId::new(format!("tx:job:{}:web-capture", job.id)).map_err(domain_error)?,
            ProjectId::new(project_id).map_err(domain_error)?,
            current.revision,
            IdempotencyKey::new(format!("job:{}:materialize-web-capture", job.id))
                .map_err(domain_error)?,
            Actor::system(),
            assets
                .iter()
                .cloned()
                .map(|asset| Operation::AddAsset { asset })
                .collect(),
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
                        WebCaptureError::new(
                            "WEB_CAPTURE_COMMIT_FAILED",
                            "commit returned no revision",
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
                }
                return Ok(MaterializedCapture {
                    assets,
                    revision,
                    document_hash,
                    replayed,
                    extraction: observation.extraction,
                });
            }
            Err(error) if error.code == "revisionConflict" => continue,
            Err(error) => {
                return Err(WebCaptureError::new(
                    "WEB_CAPTURE_COMMIT_FAILED",
                    error.to_string(),
                ));
            }
        }
    }
    Err(WebCaptureError::new(
        "WEB_CAPTURE_REVISION_CONFLICT",
        "project kept changing while website assets were materialized",
    ))
}

fn extract_public_asset_urls(html: &str, base_url: &Url) -> Vec<Url> {
    let document = Html::parse_document(html);
    let element_selector = Selector::parse("img[src], source[src], video[poster]")
        .expect("static public asset selector is valid");
    let meta_selector =
        Selector::parse("meta[property='og:image'], meta[name='twitter:image'], link[rel~='icon']")
            .expect("static metadata selector is valid");
    let mut values = Vec::new();
    for element in document.select(&element_selector) {
        if let Some(value) = element
            .value()
            .attr("src")
            .or_else(|| element.value().attr("poster"))
        {
            values.push(value);
        }
    }
    for element in document.select(&meta_selector) {
        if let Some(value) = element
            .value()
            .attr("content")
            .or_else(|| element.value().attr("href"))
        {
            values.push(value);
        }
    }
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter_map(|value| base_url.join(value.trim()).ok())
        .filter_map(|mut url| {
            url.set_fragment(None);
            validate_remote_url(&url).ok()?;
            seen.insert(url.as_str().to_owned()).then_some(url)
        })
        .collect()
}

async fn checkpoint_job(
    inner: &WebCaptureInner,
    job: &JobRecord,
    progress: f64,
    message: &str,
    checkpoint: Value,
) -> Result<(), WebCaptureError> {
    let updated = inner
        .database
        .checkpoint_job(
            &job.id,
            progress.clamp(0.0, 0.99),
            message,
            &json!({ "checkpoint": checkpoint }),
        )
        .await
        .map_err(|error| {
            WebCaptureError::new("WEB_CAPTURE_CHECKPOINT_FAILED", error.to_string())
        })?;
    publish_job(&inner.events, &updated);
    Ok(())
}

fn capture_stage_directory(layout: &DataLayout, job_id: &str) -> PathBuf {
    let digest = hex::encode(Sha256::digest(job_id.as_bytes()));
    layout.temporary.join("web-capture").join(&digest[..32])
}

fn capture_screenshot_path(layout: &DataLayout, job_id: &str) -> PathBuf {
    layout
        .root
        .join("derived/web-capture")
        .join(format!("{job_id}.png"))
}

async fn reset_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!("website capture staging path is not a directory")
        }
        Ok(_) => fs::remove_dir_all(path).await?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(path).await?;
    Ok(())
}

async fn cleanup_web_capture_artifacts(inner: &WebCaptureInner, job_id: &str) -> Result<()> {
    let stage = capture_stage_directory(&inner.layout, job_id);
    match fs::symlink_metadata(&stage).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!("website capture staging path is not a directory")
        }
        Ok(_) => fs::remove_dir_all(stage).await?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let screenshot = capture_screenshot_path(&inner.layout, job_id);
    match fs::symlink_metadata(&screenshot).await {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
            fs::remove_file(screenshot).await?;
        }
        Ok(_) => bail!("website capture output path is not a file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn safe_relative_path(value: &Value, field: &str) -> Result<PathBuf, WebCaptureError> {
    let value = value.get(field).and_then(Value::as_str).ok_or_else(|| {
        WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            format!("checkpoint has no {field}"),
        )
    })?;
    safe_relative_string(value)
}

fn safe_relative_string(value: &str) -> Result<PathBuf, WebCaptureError> {
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            "checkpoint contains an unsafe path",
        ));
    }
    Ok(path)
}

fn validate_staged_asset_metadata(value: &StagedAssetCheckpoint) -> Result<(), WebCaptureError> {
    if value.sha256.len() != 64
        || !value.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || value.byte_size == 0
        || value.byte_size > MAX_PUBLIC_ASSET_BYTES
        || !valid_text(&value.source_name, 255, false)
        || !valid_text(&value.source_url, 2_000, false)
        || !value.mime_type.starts_with("image/")
        || !valid_text(&value.mime_type, 100, false)
    {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            "checkpoint public asset metadata is outside safe bounds",
        ));
    }
    validate_checkpoint_url(&value.source_url)?;
    Ok(())
}

fn validate_checkpoint_url(value: &str) -> Result<(), WebCaptureError> {
    let parsed = Url::parse(value).map_err(|_| {
        WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            "checkpoint contains an invalid source URL",
        )
    })?;
    validate_remote_url(&parsed).map_err(|_| {
        WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            "checkpoint contains an unsafe source URL",
        )
    })?;
    if parsed.query().is_some() {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            "checkpoint source URLs must be redacted",
        ));
    }
    Ok(())
}

fn bounded_checkpoint_string(
    value: Option<&Value>,
    maximum: usize,
    field: &str,
) -> Result<String, WebCaptureError> {
    let value = value.and_then(Value::as_str).ok_or_else(|| {
        WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            format!("checkpoint has no {field}"),
        )
    })?;
    if !valid_text(value, maximum, false) {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESUME_REJECTED",
            format!("checkpoint {field} is invalid"),
        ));
    }
    Ok(value.to_owned())
}

fn bounded_result_text(
    value: Option<&Value>,
    maximum: usize,
    field: &str,
) -> Result<String, WebCaptureError> {
    let value = value.and_then(Value::as_str).ok_or_else(|| {
        WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            format!("worker returned no {field}"),
        )
    })?;
    if !valid_text(value, maximum, true) {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            format!("worker {field} is outside safe bounds"),
        ));
    }
    Ok(value.to_owned())
}

fn bounded_result_text_array(
    value: Option<&Value>,
    maximum_items: usize,
    maximum_length: usize,
) -> Result<Vec<String>, WebCaptureError> {
    let values = value.and_then(Value::as_array).ok_or_else(|| {
        WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            "worker returned no selling point array",
        )
    })?;
    if values.len() > maximum_items {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            "worker returned too many selling points",
        ));
    }
    values
        .iter()
        .map(|value| bounded_result_text(Some(value), maximum_length, "selling point"))
        .collect()
}

fn bounded_brand_colors(value: Option<&Value>) -> Result<Vec<String>, WebCaptureError> {
    let values = bounded_result_text_array(value, 12, 100)?;
    if values.iter().any(|value| !valid_color(value)) {
        return Err(WebCaptureError::new(
            "WEB_CAPTURE_RESULT_REJECTED",
            "worker returned an invalid brand color",
        ));
    }
    Ok(values)
}

fn valid_color(value: &str) -> bool {
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8)
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    value == "transparent"
        || ["rgb(", "rgba(", "hsl(", "hsla("]
            .iter()
            .any(|prefix| value.starts_with(prefix) && value.ends_with(')'))
}

fn valid_text(value: &str, maximum: usize, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty())
        && value.len() <= maximum
        && !value.chars().any(char::is_control)
}

fn image_suffix(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        _ => "image",
    }
}

fn redact_url(url: &Url) -> String {
    let mut value = url.clone();
    value.set_query(None);
    value.set_fragment(None);
    value.to_string()
}

fn ensure_not_cancelled(cancellation: &watch::Receiver<bool>) -> Result<(), WebCaptureError> {
    if *cancellation.borrow() {
        Err(WebCaptureError::cancelled())
    } else {
        Ok(())
    }
}

fn safe_error(error: impl std::fmt::Display) -> String {
    error
        .to_string()
        .chars()
        .filter(|character| !character.is_control())
        .take(500)
        .collect()
}

fn safe_worker_error(error: &Value) -> String {
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .filter(|value| valid_text(value, 100, false))
        .unwrap_or("WEB_CAPTURE_WORKER_REJECTED");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(safe_error)
        .unwrap_or_else(|| "media worker rejected the website capture".to_owned());
    format!("{code}: {message}")
}

fn web_capture_io_error(error: impl std::fmt::Display) -> WebCaptureError {
    WebCaptureError::new("WEB_CAPTURE_IO_FAILED", safe_error(error))
}

fn domain_error(error: impl std::fmt::Display) -> WebCaptureError {
    WebCaptureError::new("WEB_CAPTURE_COMMIT_FAILED", safe_error(error))
}

fn publish_job(events: &EventBus, job: &JobRecord) {
    events.publish("job.changed", json!({ "job": job }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_http_public_asset_candidates_without_string_parsing_html() {
        let base = Url::parse("https://example.com/products/page").unwrap();
        let urls = extract_public_asset_urls(
            r#"<html><head><meta property="og:image" content="/og.png"></head>
            <body><img src="hero.webp"><img src="data:image/png;base64,bad">
            <video poster="https://cdn.example.org/poster.jpg"></video></body></html>"#,
            &base,
        );
        assert_eq!(
            urls.iter().map(Url::as_str).collect::<Vec<_>>(),
            vec![
                "https://example.com/products/hero.webp",
                "https://cdn.example.org/poster.jpg",
                "https://example.com/og.png",
            ]
        );
    }

    #[test]
    fn validates_worker_brand_colors_strictly() {
        assert!(valid_color("#aabbcc"));
        assert!(valid_color("rgb(1, 2, 3)"));
        assert!(!valid_color("url(https://example.com/track)"));
        assert!(!valid_color("#nothex"));
    }
}
