use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use tokio::{net::TcpListener, sync::broadcast};

use crate::{
    api,
    auth::{AuthState, security_middleware},
    codex_image::CodexImageManager,
    config::Config,
    content_store::DataLayout,
    mg_runtime::MotionGraphicRuntime,
    native_jobs::NativeJobManager,
    persistence::Database,
    proposal::ProposalStore,
    provider::{ProviderManager, ProviderRegistry},
    runtime::{RuntimeDescriptor, RuntimeFiles},
    web_capture::WebCaptureManager,
    worker::WorkerManager,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEvent {
    pub sequence: u64,
    #[serde(rename = "type")]
    pub kind: String,
    pub occurred_at: DateTime<Utc>,
    pub data: Value,
}

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    pub layout: DataLayout,
    pub auth: AuthState,
    pub runtime: RuntimeDescriptor,
    pub editor_url: String,
    pub worker_editor_url: String,
    pub codex_command: Option<PathBuf>,
    pub authorized_import_roots: Arc<[PathBuf]>,
    pub worker: Option<WorkerManager>,
    pub provider_registry: ProviderRegistry,
    pub provider: Option<ProviderManager>,
    pub web_capture: Option<WebCaptureManager>,
    pub mg_runtime: Option<MotionGraphicRuntime>,
    pub codex_image: Option<CodexImageManager>,
    pub native_jobs: NativeJobManager,
    pub proposals: ProposalStore,
    pub events: broadcast::Sender<AppEvent>,
    event_bus: EventBus,
}

#[derive(Clone)]
pub(crate) struct EventBus {
    events: broadcast::Sender<AppEvent>,
    sequence: Arc<AtomicU64>,
}

impl EventBus {
    pub(crate) fn new(events: broadcast::Sender<AppEvent>) -> Self {
        Self {
            events,
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn publish(&self, kind: impl Into<String>, data: Value) {
        let event = AppEvent {
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed) + 1,
            kind: kind.into(),
            occurred_at: Utc::now(),
            data,
        };
        let _ = self.events.send(event);
    }
}

impl AppState {
    pub async fn initialize(
        config: &Config,
        runtime: RuntimeDescriptor,
        daemon_token: String,
    ) -> Result<Self> {
        let layout = DataLayout::initialize(&config.data_dir).await?;
        let database = Database::open(&config.database_path).await?;
        let recovered = database
            .recover_running_jobs()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if recovered > 0 {
            tracing::info!(recovered, "requeued interrupted jobs after daemon restart");
        }
        let auth = AuthState::new(
            daemon_token,
            config.allowed_origins.clone(),
            config.browser_session_ttl,
            config.secure_browser_cookie,
        );
        let (events, _) = broadcast::channel(256);
        let event_bus = EventBus::new(events.clone());
        let provider_registry = ProviderRegistry::load(&config.provider_config).await?;
        let worker = match &config.media_worker {
            Some(command) => Some(
                WorkerManager::start(
                    database.clone(),
                    command.clone(),
                    layout.root.clone(),
                    event_bus.clone(),
                    provider_registry.clone(),
                )
                .await?,
            ),
            None => None,
        };
        let provider = if provider_registry.has_external_provider()
            && let Some(media_worker_command) = &config.media_worker
        {
            Some(
                ProviderManager::start(
                    database.clone(),
                    layout.clone(),
                    provider_registry.clone(),
                    media_worker_command.clone(),
                    event_bus.clone(),
                )
                .await?,
            )
        } else {
            None
        };
        let web_capture = match &config.media_worker {
            Some(command) => Some(
                WebCaptureManager::start(
                    database.clone(),
                    layout.clone(),
                    command.clone(),
                    event_bus.clone(),
                )
                .await?,
            ),
            None => None,
        };
        let codex_image = match &config.codex_command {
            Some(command) => Some(
                CodexImageManager::start(
                    database.clone(),
                    layout.clone(),
                    command.clone(),
                    worker.is_some(),
                    event_bus.clone(),
                )
                .await?,
            ),
            None => None,
        };
        let mg_runtime = config
            .mg_runtime_node
            .clone()
            .zip(config.mg_runtime_cli.clone())
            .map(|(node, entrypoint)| MotionGraphicRuntime::new(node, entrypoint));
        let native_jobs =
            NativeJobManager::start(database.clone(), layout.clone(), event_bus.clone()).await?;
        Ok(Self {
            database,
            layout,
            auth,
            runtime,
            editor_url: config.editor_url.clone(),
            worker_editor_url: config.worker_editor_url.clone(),
            codex_command: config.codex_command.clone(),
            authorized_import_roots: config.authorized_import_roots.clone().into(),
            worker,
            provider_registry,
            provider,
            web_capture,
            mg_runtime,
            codex_image,
            native_jobs,
            proposals: ProposalStore::new(std::time::Duration::from_secs(15 * 60)),
            events,
            event_bus,
        })
    }

    pub fn publish(&self, kind: impl Into<String>, data: Value) {
        // No receivers is expected when the editor is closed; commits must not fail.
        self.event_bus.publish(kind, data);
    }
}

pub fn build_app(state: AppState) -> Router {
    let auth = state.auth.clone();
    Router::new()
        .route("/health", get(api::health))
        .route("/api/v1/session/bootstrap", post(api::bootstrap_session))
        .route("/api/v1/status", get(api::status))
        .route(
            "/api/v1/projects",
            get(api::list_projects).post(api::create_project),
        )
        .route(
            "/api/v1/projects/{project_id}",
            get(api::read_project).delete(api::delete_project),
        )
        .route(
            "/api/v1/projects/{project_id}/settings/auto-apply",
            post(api::set_project_auto_apply),
        )
        .route(
            "/api/v1/projects/{project_id}/transactions/validate",
            post(api::validate_transaction),
        )
        .route(
            "/api/v1/projects/{project_id}/transactions",
            post(api::commit_transaction),
        )
        .route(
            "/api/v1/projects/{project_id}/media",
            post(api::upload_managed_media),
        )
        .route(
            "/api/v1/projects/{project_id}/assets/{asset_id}/content",
            get(api::read_managed_media),
        )
        .route(
            "/api/v1/projects/{project_id}/assets/{asset_id}/derivatives/{derivative_kind}",
            get(api::read_media_derivative),
        )
        .route(
            "/api/v1/projects/{project_id}/revisions",
            get(api::list_revisions),
        )
        .route(
            "/api/v1/projects/{project_id}/revisions/{revision}",
            get(api::read_project_revision),
        )
        .route(
            "/api/v1/projects/{project_id}/undo",
            post(api::undo_project),
        )
        .route(
            "/api/v1/projects/{project_id}/redo",
            post(api::redo_project),
        )
        .route(
            "/api/v1/projects/{project_id}/agent/sessions",
            get(api::list_agent_sessions).post(api::create_agent_session),
        )
        .route(
            "/api/v1/agent/sessions/{session_id}",
            get(api::read_agent_session),
        )
        .route(
            "/api/v1/projects/{project_id}/versions",
            get(api::list_versions).post(api::create_version),
        )
        .route(
            "/api/v1/projects/{project_id}/restore",
            post(api::restore_version),
        )
        .route("/api/v1/jobs", get(api::list_jobs))
        .route("/api/v1/jobs/{job_id}", get(api::read_job))
        .route(
            "/api/v1/jobs/{job_id}/artifact",
            get(api::read_job_artifact),
        )
        .route("/api/v1/jobs/{job_id}/cancel", post(api::cancel_job))
        .route("/api/v1/maintenance/media-gc", post(api::media_gc))
        .route("/api/v1/events", get(api::events))
        .route("/api/v1/events/ws", get(api::websocket_events))
        .route("/api/v1/tools/{tool_name}", post(api::dispatch_tool))
        .fallback(api::route_not_found)
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(auth, security_middleware))
        .with_state(state)
}

pub async fn run(config: Config) -> Result<()> {
    if !config.bind.ip().is_loopback()
        && !(config.containerized && config.bind.ip().is_unspecified())
    {
        anyhow::bail!(
            "refusing to bind daemon to non-loopback address {}",
            config.bind
        );
    }
    let listener = TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("bind {}", config.bind))?;
    let bound_address = listener.local_addr()?;
    let daemon_token = hex::encode(rand::random::<[u8; 32]>());
    let runtime_files = RuntimeFiles::install(&config, bound_address, &daemon_token)?;
    let state =
        match AppState::initialize(&config, runtime_files.descriptor.clone(), daemon_token).await {
            Ok(state) => state,
            Err(error) => {
                runtime_files.cleanup();
                return Err(error);
            }
        };
    let database = state.database.clone();
    let worker = state.worker.clone();
    let provider = state.provider.clone();
    let web_capture = state.web_capture.clone();
    let codex_image = state.codex_image.clone();
    let native_jobs = state.native_jobs.clone();
    let app = build_app(state);

    tracing::info!(
        address = %bound_address,
        descriptor = %config.runtime_descriptor.display(),
        "OpenChatCut daemon ready"
    );
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    if let Some(codex_image) = codex_image {
        codex_image.shutdown().await;
    }
    if let Some(provider) = provider {
        provider.shutdown().await;
    }
    if let Some(web_capture) = web_capture {
        web_capture.shutdown().await;
    }
    if let Some(worker) = worker {
        worker.shutdown().await;
    }
    native_jobs.shutdown().await;
    database.close().await;
    runtime_files.cleanup();
    result.context("daemon server failed")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "install Ctrl-C handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown requested");
}
