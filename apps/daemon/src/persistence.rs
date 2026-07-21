use std::{collections::HashSet, path::Path, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use openchatcut_domain::{
    ActorKind, DomainError, EditTransaction, ProjectDocument, ProjectEnvelope, apply_transaction,
    canonical_document_hash, transaction_fingerprint, validate_transaction,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tokio::sync::Mutex;

use crate::error::ApiError;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    current_revision INTEGER NOT NULL CHECK (current_revision >= 0),
    current_document_json TEXT NOT NULL,
    current_document_hash_json TEXT NOT NULL,
    auto_apply INTEGER NOT NULL DEFAULT 0 CHECK (auto_apply IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS transactions (
    transaction_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    idempotency_key_json TEXT NOT NULL,
    fingerprint_json TEXT NOT NULL,
    base_revision INTEGER NOT NULL,
    committed_revision INTEGER NOT NULL,
    actor_json TEXT NOT NULL,
    operations_json TEXT NOT NULL,
    inverse_operations_json TEXT NOT NULL,
    response_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (project_id, idempotency_key_json)
);

CREATE TABLE IF NOT EXISTS revisions (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    parent_revision INTEGER,
    transaction_id TEXT,
    document_json TEXT NOT NULL,
    document_hash_json TEXT NOT NULL,
    operations_json TEXT NOT NULL,
    actor_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (project_id, revision)
);

CREATE TABLE IF NOT EXISTS named_versions (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    revision INTEGER NOT NULL,
    document_json TEXT NOT NULL,
    document_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (project_id, name)
);

CREATE TABLE IF NOT EXISTS idempotency_receipts (
    scope TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    response_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (scope, idempotency_key)
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
    input_json TEXT NOT NULL,
    output_json TEXT,
    error_json TEXT,
    message TEXT,
    revision INTEGER,
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS agent_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    provider TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_messages (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    role TEXT NOT NULL CHECK (role IN ('user', 'agent', 'error')),
    status TEXT NOT NULL CHECK (status IN ('streaming', 'completed', 'failed')),
    text TEXT NOT NULL,
    proposal_json TEXT,
    history_action_json TEXT,
    workflow_json TEXT,
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS revisions_project_created_idx
    ON revisions(project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS jobs_state_created_idx
    ON jobs(state, created_at);
CREATE INDEX IF NOT EXISTS jobs_project_created_idx
    ON jobs(project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS agent_sessions_project_updated_idx
    ON agent_sessions(project_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS agent_messages_session_created_idx
    ON agent_messages(session_id, ordinal);
"#;

#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub current_revision: u64,
    pub document_hash: Value,
    pub auto_apply: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionRecord {
    pub revision: u64,
    pub parent_revision: Option<u64>,
    pub transaction_id: Option<String>,
    pub document_hash: Value,
    pub operations: Value,
    pub actor: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryDirection {
    Undo,
    Redo,
}

impl HistoryDirection {
    const fn action(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }

    const fn marker_type(self) -> &'static str {
        match self {
            Self::Undo => "undoRevision",
            Self::Redo => "redoRevision",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HistoryEntry {
    revision: u64,
    before_revision: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub revision: u64,
    pub document_hash: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: String,
    pub project_id: Option<String>,
    pub kind: String,
    pub state: String,
    pub progress: f64,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub message: Option<String>,
    pub revision: Option<u64>,
    pub cancel_requested: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub provider: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageRecord {
    pub id: String,
    pub role: String,
    pub status: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_action: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRecord {
    #[serde(flatten)]
    pub summary: AgentSessionSummary,
    pub messages: Vec<AgentMessageRecord>,
}

#[derive(Debug)]
pub enum CommitResult {
    Committed(Value),
    Replayed(Value),
}

impl Database {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(8)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        let job_columns = sqlx::query("PRAGMA table_info(jobs)")
            .fetch_all(&pool)
            .await?;
        if !job_columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .is_ok_and(|name| name == "message")
        }) {
            sqlx::query("ALTER TABLE jobs ADD COLUMN message TEXT")
                .execute(&pool)
                .await?;
        }
        // Older local daemon databases predate the per-project Auto-Apply
        // policy. Keep upgrades non-destructive and default existing projects
        // to review-before-apply.
        let project_columns = sqlx::query("PRAGMA table_info(projects)")
            .fetch_all(&pool)
            .await?;
        if !project_columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .is_ok_and(|name| name == "auto_apply")
        }) {
            sqlx::query(
                "ALTER TABLE projects ADD COLUMN auto_apply INTEGER NOT NULL DEFAULT 0 CHECK (auto_apply IN (0, 1))",
            )
            .execute(&pool)
            .await?;
        }
        let agent_message_columns = sqlx::query("PRAGMA table_info(agent_messages)")
            .fetch_all(&pool)
            .await?;
        if !agent_message_columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .is_ok_and(|name| name == "history_action_json")
        }) {
            sqlx::query("ALTER TABLE agent_messages ADD COLUMN history_action_json TEXT")
                .execute(&pool)
                .await?;
        }
        if !agent_message_columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .is_ok_and(|name| name == "workflow_json")
        }) {
            sqlx::query("ALTER TABLE agent_messages ADD COLUMN workflow_json TEXT")
                .execute(&pool)
                .await?;
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE agent_messages SET status = 'failed', error_json = ?, updated_at = ? WHERE status = 'streaming'",
        )
        .bind(
            serde_json::to_string(&json!({
                "code": "agentTurnInterrupted",
                "message": "The daemon restarted before this Agent turn completed. Send the request again to continue."
            }))?,
        )
        .bind(&now)
        .execute(&pool)
        .await?;
        Ok(Self {
            pool,
            write_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn create_agent_session(
        &self,
        project_id: &str,
        session_id: &str,
        title: &str,
        provider: &str,
    ) -> Result<AgentSessionRecord, ApiError> {
        self.read_project(project_id).await?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO agent_sessions (id, project_id, title, provider, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(project_id)
        .bind(title)
        .bind(provider)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.read_agent_session(session_id).await
    }

    pub async fn list_agent_sessions(
        &self,
        project_id: &str,
    ) -> Result<Vec<AgentSessionSummary>, ApiError> {
        self.read_project(project_id).await?;
        let rows = sqlx::query(
            "SELECT id, project_id, title, provider, created_at, updated_at FROM agent_sessions WHERE project_id = ? ORDER BY updated_at DESC, id DESC LIMIT 100",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(agent_session_summary_from_row).collect()
    }

    pub async fn read_agent_session(
        &self,
        session_id: &str,
    ) -> Result<AgentSessionRecord, ApiError> {
        let row = sqlx::query(
            "SELECT id, project_id, title, provider, created_at, updated_at FROM agent_sessions WHERE id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("agent session", session_id))?;
        let summary = agent_session_summary_from_row(&row)?;
        let rows = sqlx::query(
            "SELECT id, role, status, text, proposal_json, history_action_json, workflow_json, error_json, created_at, updated_at FROM agent_messages WHERE session_id = ? ORDER BY ordinal, id",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        let messages = rows
            .iter()
            .map(agent_message_record_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AgentSessionRecord { summary, messages })
    }

    pub async fn begin_agent_turn(
        &self,
        session_id: &str,
        project_id: &str,
        provider: &str,
        user_message_id: &str,
        assistant_message_id: &str,
        instruction: &str,
    ) -> Result<(), ApiError> {
        let session = self.read_agent_session(session_id).await?;
        if session.summary.project_id != project_id {
            return Err(ApiError::conflict(
                "agent_session_project_mismatch",
                "the Agent session belongs to a different project",
                json!({ "sessionId": session_id, "projectId": project_id }),
            ));
        }
        let title = instruction
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(72)
            .collect::<String>();
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let next_ordinal: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM agent_messages WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO agent_messages (id, session_id, ordinal, role, status, text, created_at, updated_at) VALUES (?, ?, ?, 'user', 'completed', ?, ?, ?)",
        )
        .bind(user_message_id)
        .bind(session_id)
        .bind(next_ordinal)
        .bind(instruction)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO agent_messages (id, session_id, ordinal, role, status, text, created_at, updated_at) VALUES (?, ?, ?, 'agent', 'streaming', '', ?, ?)",
        )
        .bind(assistant_message_id)
        .bind(session_id)
        .bind(next_ordinal + 1)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE agent_sessions SET title = CASE WHEN title = 'New conversation' THEN ? ELSE title END, provider = ?, updated_at = ? WHERE id = ?",
        )
        .bind(if title.is_empty() { "New conversation" } else { &title })
        .bind(provider)
        .bind(&now)
        .bind(session_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn complete_agent_turn(
        &self,
        session_id: &str,
        assistant_message_id: &str,
        text: &str,
        proposal: Option<&Value>,
    ) -> Result<(), ApiError> {
        let now = Utc::now().to_rfc3339();
        let proposal_json = proposal
            .map(serde_json::to_string)
            .transpose()
            .map_err(ApiError::internal)?;
        let updated = sqlx::query(
            "UPDATE agent_messages SET status = 'completed', text = ?, proposal_json = ?, error_json = NULL, updated_at = ? WHERE id = ? AND session_id = ? AND role = 'agent'",
        )
        .bind(text)
        .bind(proposal_json)
        .bind(&now)
        .bind(assistant_message_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(ApiError::not_found("agent message", assistant_message_id));
        }
        sqlx::query("UPDATE agent_sessions SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_agent_history_action(
        &self,
        session_id: &str,
        assistant_message_id: &str,
        project_id: &str,
        expected_revision: u64,
        action: &str,
        proposal_id: Option<&str>,
    ) -> Result<(), ApiError> {
        if !matches!(action, "undo" | "redo") {
            return Err(ApiError::internal("invalid Agent history action"));
        }
        let row = sqlx::query(
            "SELECT s.project_id, m.proposal_json FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id WHERE m.id = ? AND m.session_id = ? AND m.role = 'agent'",
        )
        .bind(assistant_message_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("agent message", assistant_message_id))?;
        let bound_project_id: String = row.try_get("project_id")?;
        if bound_project_id != project_id {
            return Err(ApiError::conflict(
                "agent_history_project_mismatch",
                "the Agent history action belongs to a different project",
                json!({
                    "sessionId": session_id,
                    "messageId": assistant_message_id,
                    "projectId": project_id,
                }),
            ));
        }
        if let Some(proposal_id) = proposal_id {
            let proposal = row
                .try_get::<Option<String>, _>("proposal_json")?
                .ok_or_else(|| {
                    ApiError::conflict(
                        "agent_history_proposal_mismatch",
                        "the Agent message has no persisted proposal",
                        json!({ "proposalId": proposal_id }),
                    )
                })
                .and_then(|encoded| {
                    serde_json::from_str::<Value>(&encoded).map_err(ApiError::internal)
                })?;
            if proposal.get("proposalId").and_then(Value::as_str) != Some(proposal_id) {
                return Err(ApiError::conflict(
                    "agent_history_proposal_mismatch",
                    "the Agent message does not own the applied proposal",
                    json!({ "proposalId": proposal_id }),
                ));
            }
        }
        let now = Utc::now().to_rfc3339();
        let history_action = serde_json::to_string(&json!({
            "projectId": project_id,
            "expectedRevision": expected_revision,
            "action": action,
        }))
        .map_err(ApiError::internal)?;
        let updated = sqlx::query(
            "UPDATE agent_messages SET history_action_json = ?, updated_at = ? WHERE id = ? AND session_id = ? AND role = 'agent'",
        )
        .bind(history_action)
        .bind(&now)
        .bind(assistant_message_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(ApiError::not_found("agent message", assistant_message_id));
        }
        sqlx::query("UPDATE agent_sessions SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_agent_workflow_jobs(
        &self,
        session_id: &str,
        assistant_message_id: &str,
        proposal_id: &str,
        pinned_revision: u64,
        job_ids: &[String],
    ) -> Result<(), ApiError> {
        let workflow = serde_json::to_string(&json!({
            "proposalId": proposal_id,
            "pinnedRevision": pinned_revision,
            "jobIds": job_ids,
        }))
        .map_err(ApiError::internal)?;
        let now = Utc::now().to_rfc3339();
        let updated = sqlx::query(
            "UPDATE agent_messages SET workflow_json = ?, updated_at = ? WHERE id = ? AND session_id = ? AND role = 'agent'",
        )
        .bind(workflow)
        .bind(&now)
        .bind(assistant_message_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(ApiError::not_found("agent message", assistant_message_id));
        }
        sqlx::query("UPDATE agent_sessions SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn fail_agent_turn(
        &self,
        session_id: &str,
        assistant_message_id: &str,
        message: &str,
    ) -> Result<(), ApiError> {
        let now = Utc::now().to_rfc3339();
        let error_json = serde_json::to_string(&json!({
            "code": "agentTurnFailed",
            "message": message,
        }))
        .map_err(ApiError::internal)?;
        sqlx::query(
            "UPDATE agent_messages SET status = 'failed', text = ?, error_json = ?, updated_at = ? WHERE id = ? AND session_id = ? AND role = 'agent'",
        )
        .bind(message)
        .bind(error_json)
        .bind(&now)
        .bind(assistant_message_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        sqlx::query("UPDATE agent_sessions SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn read_persisted_agent_proposal(
        &self,
        proposal_id: &str,
    ) -> Result<Option<Value>, ApiError> {
        let rows = sqlx::query(
            "SELECT proposal_json FROM agent_messages WHERE proposal_json IS NOT NULL ORDER BY updated_at DESC LIMIT 256",
        )
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let encoded: String = row.try_get("proposal_json")?;
            let value: Value = serde_json::from_str(&encoded).map_err(ApiError::internal)?;
            if value.get("proposalId").and_then(Value::as_str) == Some(proposal_id) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectSummary>, ApiError> {
        let rows = sqlx::query(
            "SELECT id, name, current_revision, current_document_hash_json, auto_apply, created_at, updated_at FROM projects ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(project_summary_from_row).collect()
    }

    pub async fn read_project_summary(&self, project_id: &str) -> Result<ProjectSummary, ApiError> {
        let row = sqlx::query(
            "SELECT id, name, current_revision, current_document_hash_json, auto_apply, created_at, updated_at FROM projects WHERE id = ?",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("project", project_id))?;
        project_summary_from_row(row)
    }

    /// Toggle the project Agent Auto-Apply policy without changing the
    /// document revision. The expected revision is still CAS-checked so a UI
    /// cannot silently change policy on a stale project, and the setting write
    /// is idempotent just like document transactions.
    pub async fn set_project_auto_apply(
        &self,
        project_id: &str,
        expected_revision: u64,
        enabled: bool,
        idempotency_key: &str,
    ) -> Result<ProjectSummary, ApiError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        let scope = format!("project:{project_id}:auto-apply");
        let fingerprint = request_fingerprint(&json!({
            "projectId": project_id,
            "expectedRevision": expected_revision,
            "enabled": enabled,
        }));
        if self
            .read_receipt(&scope, idempotency_key, &fingerprint)
            .await?
            .is_some()
        {
            return self.read_project_summary(project_id).await;
        }

        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let head = sqlx::query("SELECT current_revision FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| ApiError::not_found("project", project_id))?;
        let current_revision: i64 = head.try_get("current_revision")?;
        if current_revision as u64 != expected_revision {
            transaction.rollback().await?;
            return Err(revision_conflict(
                expected_revision,
                &self.read_project(project_id).await?,
            ));
        }
        sqlx::query(
            "UPDATE projects SET auto_apply = ?, updated_at = ? WHERE id = ? AND current_revision = ?",
        )
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(Utc::now().to_rfc3339())
        .bind(project_id)
        .bind(expected_revision as i64)
        .execute(&mut *transaction)
        .await?;
        let response = json!({
            "projectId": project_id,
            "expectedRevision": expected_revision,
            "autoApply": enabled,
        });
        if let Err(error) = insert_receipt(
            &mut transaction,
            &scope,
            idempotency_key,
            &fingerprint,
            &response,
        )
        .await
        {
            transaction.rollback().await?;
            if self
                .read_receipt(&scope, idempotency_key, &fingerprint)
                .await?
                .is_some()
            {
                return self.read_project_summary(project_id).await;
            }
            return Err(error);
        }
        transaction.commit().await?;
        self.read_project_summary(project_id).await
    }

    pub async fn create_project(
        &self,
        document: ProjectDocument,
        idempotency_key: &str,
        request: &Value,
    ) -> Result<CommitResult, ApiError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        let scope = "create_project";
        let fingerprint = request_fingerprint(request);
        if let Some(value) = self
            .read_receipt(scope, idempotency_key, &fingerprint)
            .await?
        {
            return Ok(CommitResult::Replayed(mark_replayed(value)));
        }
        let envelope = ProjectEnvelope::new(document).map_err(domain_bad_request)?;
        let project_id = serialized_string(&envelope.document.id)?;
        let document = serde_json::to_string(&envelope.document).map_err(ApiError::internal)?;
        let hash = serde_json::to_string(&envelope.document_hash).map_err(ApiError::internal)?;
        let name = envelope.document.name.clone();
        let now = Utc::now().to_rfc3339();
        let response = json!({ "replayed": false, "envelope": &envelope });
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;

        // Claim the key before creating project rows. The unique insert is also
        // the first write in this transaction, serializing concurrent retries.
        let receipt = sqlx::query(
            "INSERT INTO idempotency_receipts (scope, idempotency_key, fingerprint, response_json, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(scope)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(serde_json::to_string(&response).map_err(ApiError::internal)?)
        .bind(&now)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = receipt {
            transaction.rollback().await?;
            if error
                .as_database_error()
                .is_some_and(|database| database.is_unique_violation())
                && let Some(value) = self
                    .read_receipt(scope, idempotency_key, &fingerprint)
                    .await?
            {
                return Ok(CommitResult::Replayed(mark_replayed(value)));
            }
            return Err(error.into());
        }
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO projects (id, name, current_revision, current_document_json, current_document_hash_json, created_at, updated_at) VALUES (?, ?, 0, ?, ?, ?, ?)",
        )
        .bind(&project_id)
        .bind(name)
        .bind(&document)
        .bind(&hash)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() == 0 {
            transaction.rollback().await?;
            return Err(ApiError::conflict(
                "project_exists",
                "a project with this id already exists",
                json!({ "projectId": project_id }),
            ));
        }
        sqlx::query(
            "INSERT INTO revisions (project_id, revision, parent_revision, transaction_id, document_json, document_hash_json, operations_json, actor_json, created_at) VALUES (?, 0, NULL, NULL, ?, ?, '[]', '{\"kind\":\"system\"}', ?)",
        )
        .bind(&project_id)
        .bind(document)
        .bind(hash)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CommitResult::Committed(response))
    }

    /// Restore a verified portable package without manufacturing intermediate
    /// revisions. The package's pinned revision and canonical document hash are
    /// preserved exactly; history before that snapshot is intentionally not
    /// claimed to exist.
    pub async fn import_project_envelope(
        &self,
        envelope: ProjectEnvelope,
        idempotency_key: &str,
        request: &Value,
    ) -> Result<CommitResult, ApiError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        if envelope.revision > i64::MAX as u64 {
            return Err(ApiError::bad_request(
                "invalid_package_revision",
                "package revision is outside the SQLite range",
            ));
        }
        let verified =
            ProjectEnvelope::new(envelope.document.clone()).map_err(domain_bad_request)?;
        if verified.document_hash != envelope.document_hash {
            return Err(ApiError::bad_request(
                "package_document_hash_mismatch",
                "package documentHash does not match its canonical document",
            ));
        }
        let scope = "import_project_package";
        let fingerprint = request_fingerprint(request);
        if let Some(value) = self
            .read_receipt(scope, idempotency_key, &fingerprint)
            .await?
        {
            return Ok(CommitResult::Replayed(mark_replayed(value)));
        }
        let project_id = serialized_string(&envelope.document.id)?;
        let document = serde_json::to_string(&envelope.document).map_err(ApiError::internal)?;
        let hash = serde_json::to_string(&envelope.document_hash).map_err(ApiError::internal)?;
        let response = json!({ "replayed": false, "envelope": &envelope });
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let receipt = sqlx::query(
            "INSERT INTO idempotency_receipts (scope, idempotency_key, fingerprint, response_json, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(scope)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(serde_json::to_string(&response).map_err(ApiError::internal)?)
        .bind(&now)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = receipt {
            transaction.rollback().await?;
            if error
                .as_database_error()
                .is_some_and(|database| database.is_unique_violation())
                && let Some(value) = self
                    .read_receipt(scope, idempotency_key, &fingerprint)
                    .await?
            {
                return Ok(CommitResult::Replayed(mark_replayed(value)));
            }
            return Err(error.into());
        }
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO projects (id, name, current_revision, current_document_json, current_document_hash_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&project_id)
        .bind(&envelope.document.name)
        .bind(envelope.revision as i64)
        .bind(&document)
        .bind(&hash)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() == 0 {
            transaction.rollback().await?;
            return Err(ApiError::conflict(
                "project_exists",
                "a project with this id already exists",
                json!({ "projectId": project_id }),
            ));
        }
        sqlx::query(
            "INSERT INTO revisions (project_id, revision, parent_revision, transaction_id, document_json, document_hash_json, operations_json, actor_json, created_at) VALUES (?, ?, NULL, NULL, ?, ?, '[]', '{\"kind\":\"system\"}', ?)",
        )
        .bind(&project_id)
        .bind(envelope.revision as i64)
        .bind(&document)
        .bind(&hash)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CommitResult::Committed(response))
    }

    pub async fn read_project(&self, project_id: &str) -> Result<ProjectEnvelope, ApiError> {
        let row = sqlx::query(
            "SELECT current_revision, current_document_json, current_document_hash_json FROM projects WHERE id = ?",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("project", project_id))?;
        envelope_from_row(&row)
    }

    pub async fn read_project_revision(
        &self,
        project_id: &str,
        revision: u64,
    ) -> Result<ProjectEnvelope, ApiError> {
        let row = sqlx::query(
            "SELECT revision AS current_revision, document_json AS current_document_json, document_hash_json AS current_document_hash_json FROM revisions WHERE project_id = ? AND revision = ?",
        )
        .bind(project_id)
        .bind(revision as i64)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "project revision",
                &format!("{project_id}@{revision}"),
            )
        })?;
        envelope_from_row(&row)
    }

    pub async fn undo_project(
        &self,
        project_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<Value, ApiError> {
        self.navigate_project_history(
            project_id,
            expected_revision,
            idempotency_key,
            HistoryDirection::Undo,
        )
        .await
    }

    pub async fn redo_project(
        &self,
        project_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<Value, ApiError> {
        self.navigate_project_history(
            project_id,
            expected_revision,
            idempotency_key,
            HistoryDirection::Redo,
        )
        .await
    }

    async fn navigate_project_history(
        &self,
        project_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
        direction: HistoryDirection,
    ) -> Result<Value, ApiError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        let scope = format!("project:{project_id}:history:{}", direction.action());
        let fingerprint = request_fingerprint(&json!({
            "projectId": project_id,
            "expectedRevision": expected_revision,
            "action": direction.action(),
        }));
        if let Some(value) = self
            .read_receipt(&scope, idempotency_key, &fingerprint)
            .await?
        {
            return Ok(mark_replayed(value));
        }

        let current = self.read_project(project_id).await?;
        if current.revision != expected_revision {
            return Err(revision_conflict(expected_revision, &current));
        }
        let (mut undo_stack, mut redo_stack) = self.history_stacks(project_id).await?;
        let entry = match direction {
            HistoryDirection::Undo => undo_stack.pop(),
            HistoryDirection::Redo => redo_stack.pop(),
        }
        .ok_or_else(|| {
            ApiError::conflict(
                match direction {
                    HistoryDirection::Undo => "nothing_to_undo",
                    HistoryDirection::Redo => "nothing_to_redo",
                },
                format!("there is no project revision to {}", direction.action()),
                json!({
                    "projectId": project_id,
                    "currentRevision": expected_revision,
                }),
            )
        })?;
        match direction {
            HistoryDirection::Undo => redo_stack.push(entry),
            HistoryDirection::Redo => undo_stack.push(entry),
        }
        let target_revision = match direction {
            HistoryDirection::Undo => entry.before_revision,
            HistoryDirection::Redo => entry.revision,
        };
        let target = self
            .read_project_revision(project_id, target_revision)
            .await?;
        let next_revision = expected_revision.checked_add(1).ok_or_else(|| {
            ApiError::bad_request(
                "revision_overflow",
                "project revision cannot be incremented",
            )
        })?;
        let response_envelope = ProjectEnvelope {
            document: target.document.clone(),
            revision: next_revision,
            document_hash: target.document_hash.clone(),
        };
        response_envelope.verify().map_err(ApiError::internal)?;
        let marker = json!({
            "type": direction.marker_type(),
            "sourceRevision": entry.revision,
            "beforeRevision": entry.before_revision,
            "restoredFromRevision": target_revision,
        });
        let response = json!({
            "replayed": false,
            "action": direction.action(),
            "sourceRevision": entry.revision,
            "restoredFromRevision": target_revision,
            "canUndo": !undo_stack.is_empty(),
            "canRedo": !redo_stack.is_empty(),
            "envelope": response_envelope,
        });
        let document_json = serde_json::to_string(&target.document).map_err(ApiError::internal)?;
        let document_hash_json =
            serde_json::to_string(&target.document_hash).map_err(ApiError::internal)?;
        let now = Utc::now().to_rfc3339();

        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE projects SET name = ?, current_revision = ?, current_document_json = ?, current_document_hash_json = ?, updated_at = ? WHERE id = ? AND current_revision = ?",
        )
        .bind(&target.document.name)
        .bind(next_revision as i64)
        .bind(&document_json)
        .bind(&document_hash_json)
        .bind(&now)
        .bind(project_id)
        .bind(expected_revision as i64)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            if let Some(value) = self
                .read_receipt(&scope, idempotency_key, &fingerprint)
                .await?
            {
                return Ok(mark_replayed(value));
            }
            return Err(revision_conflict(
                expected_revision,
                &self.read_project(project_id).await?,
            ));
        }
        sqlx::query(
            "INSERT INTO revisions (project_id, revision, parent_revision, transaction_id, document_json, document_hash_json, operations_json, actor_json, created_at) VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(next_revision as i64)
        .bind(expected_revision as i64)
        .bind(document_json)
        .bind(document_hash_json)
        .bind(json!([marker]).to_string())
        .bind(format!(
            "{{\"kind\":\"system\",\"label\":\"history_{}\"}}",
            direction.action()
        ))
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        insert_receipt(
            &mut transaction,
            &scope,
            idempotency_key,
            &fingerprint,
            &response,
        )
        .await?;
        transaction.commit().await?;
        Ok(response)
    }

    async fn history_stacks(
        &self,
        project_id: &str,
    ) -> Result<(Vec<HistoryEntry>, Vec<HistoryEntry>), ApiError> {
        let rows = sqlx::query(
            "SELECT revision, parent_revision, operations_json FROM revisions WHERE project_id = ? ORDER BY revision ASC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Err(ApiError::not_found("project", project_id));
        }
        let mut undo_stack = Vec::new();
        let mut redo_stack = Vec::new();
        for row in rows {
            let revision = row.try_get::<i64, _>("revision")? as u64;
            let parent_revision = row
                .try_get::<Option<i64>, _>("parent_revision")?
                .map(|value| value as u64);
            let operations: Value =
                serde_json::from_str(&row.try_get::<String, _>("operations_json")?)
                    .map_err(ApiError::internal)?;
            if let Some((marker_direction, entry)) = history_marker(&operations)? {
                let (source, destination) = match marker_direction {
                    HistoryDirection::Undo => (&mut undo_stack, &mut redo_stack),
                    HistoryDirection::Redo => (&mut redo_stack, &mut undo_stack),
                };
                let actual = source.pop().ok_or_else(|| {
                    ApiError::internal(format!(
                        "revision {revision} contains an invalid {} marker",
                        marker_direction.action()
                    ))
                })?;
                if actual != entry {
                    return Err(ApiError::internal(format!(
                        "revision {revision} history marker does not match the active history stack"
                    )));
                }
                destination.push(entry);
                continue;
            }
            if revision == 0 || parent_revision.is_none() {
                // A package import may preserve a non-zero pinned revision as
                // the local history root. It is a baseline, not an undoable
                // edit, because no earlier document exists in this database.
                continue;
            }
            let before_revision = parent_revision.expect("checked above");
            undo_stack.push(HistoryEntry {
                revision,
                before_revision,
            });
            redo_stack.clear();
        }
        Ok((undo_stack, redo_stack))
    }

    pub async fn delete_project(
        &self,
        project_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<CommitResult, ApiError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        let scope = format!("delete_project:{project_id}");
        let fingerprint = request_fingerprint(&json!({
            "projectId": project_id,
            "expectedRevision": expected_revision,
        }));
        if let Some(value) = self
            .read_receipt(&scope, idempotency_key, &fingerprint)
            .await?
        {
            return Ok(CommitResult::Replayed(mark_replayed(value)));
        }
        let response = json!({
            "replayed": false,
            "projectId": project_id,
            "deletedRevision": expected_revision,
        });
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let receipt = sqlx::query(
            "INSERT INTO idempotency_receipts (scope, idempotency_key, fingerprint, response_json, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&scope)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(serde_json::to_string(&response).map_err(ApiError::internal)?)
        .bind(&now)
        .execute(&mut *tx)
        .await;
        if let Err(error) = receipt {
            tx.rollback().await?;
            if error
                .as_database_error()
                .is_some_and(|database| database.is_unique_violation())
                && let Some(value) = self
                    .read_receipt(&scope, idempotency_key, &fingerprint)
                    .await?
            {
                return Ok(CommitResult::Replayed(mark_replayed(value)));
            }
            return Err(error.into());
        }
        let deleted = sqlx::query("DELETE FROM projects WHERE id = ? AND current_revision = ?")
            .bind(project_id)
            .bind(expected_revision as i64)
            .execute(&mut *tx)
            .await?;
        if deleted.rows_affected() == 0 {
            tx.rollback().await?;
            return match self.read_project(project_id).await {
                Ok(current) => Err(revision_conflict(expected_revision, &current)),
                Err(error) => Err(error),
            };
        }
        tx.commit().await?;
        Ok(CommitResult::Committed(response))
    }

    pub async fn validate(
        &self,
        project_id: &str,
        edit: &EditTransaction,
    ) -> Result<Value, ApiError> {
        let envelope = self.read_project(project_id).await?;
        ensure_transaction_project(project_id, edit)?;
        let report = validate_transaction(&envelope, edit).map_err(domain_edit_error)?;
        serde_json::to_value(report).map_err(ApiError::internal)
    }

    pub async fn commit(
        &self,
        project_id: &str,
        edit: &EditTransaction,
    ) -> Result<CommitResult, ApiError> {
        ensure_transaction_project(project_id, edit)?;
        let idempotency_key =
            serde_json::to_string(&edit.idempotency_key).map_err(ApiError::internal)?;
        let fingerprint =
            serde_json::to_string(&transaction_fingerprint(edit).map_err(domain_edit_error)?)
                .map_err(ApiError::internal)?;

        if let Some(replay) = self
            .transaction_replay(project_id, &idempotency_key, &fingerprint)
            .await?
        {
            return Ok(CommitResult::Replayed(mark_replayed(replay)));
        }

        let current = self.read_project(project_id).await?;
        let outcome = match apply_transaction(&current, edit) {
            Ok(outcome) => outcome,
            Err(DomainError::RevisionConflict { .. }) => {
                // An identical concurrent request may have committed between
                // the first receipt lookup and this head read. Recheck before
                // returning a false conflict to an idempotent retry.
                if let Some(replay) = self
                    .transaction_replay(project_id, &idempotency_key, &fingerprint)
                    .await?
                {
                    return Ok(CommitResult::Replayed(mark_replayed(replay)));
                }
                return Err(revision_conflict(edit.base_revision, &current));
            }
            Err(error) => return Err(domain_edit_error(error)),
        };
        let agent_checkpoint = if edit.actor.kind == ActorKind::Agent {
            Some(NamedVersion {
                id: uuid::Uuid::new_v4().to_string(),
                project_id: project_id.to_owned(),
                name: format!(
                    "Agent checkpoint before revision {}",
                    outcome.envelope.revision
                ),
                revision: current.revision,
                document_hash: serde_json::to_value(&current.document_hash)
                    .map_err(ApiError::internal)?,
                created_at: Utc::now(),
            })
        } else {
            None
        };
        let response = json!({
            "replayed": false,
            "envelope": &outcome.envelope,
            "inverseOperations": &outcome.inverse_operations,
            "changes": &outcome.changes,
            "agentCheckpoint": &agent_checkpoint,
        });
        let document_json =
            serde_json::to_string(&outcome.envelope.document).map_err(ApiError::internal)?;
        let document_hash_json =
            serde_json::to_string(&outcome.envelope.document_hash).map_err(ApiError::internal)?;
        let response_json = serde_json::to_string(&response).map_err(ApiError::internal)?;
        let transaction_id = serialized_string(&edit.transaction_id)?;
        let operations_json =
            serde_json::to_string(&edit.operations).map_err(ApiError::internal)?;
        let inverse_json =
            serde_json::to_string(&outcome.inverse_operations).map_err(ApiError::internal)?;
        let actor_json = serde_json::to_string(&edit.actor).map_err(ApiError::internal)?;
        let now = Utc::now().to_rfc3339();

        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(row) = sqlx::query(
            "SELECT fingerprint_json, response_json FROM transactions WHERE project_id = ? AND idempotency_key_json = ?",
        )
        .bind(project_id)
        .bind(&idempotency_key)
        .fetch_optional(&mut *transaction)
        .await?
        {
            let existing_fingerprint: String = row.try_get("fingerprint_json")?;
            let existing_response: String = row.try_get("response_json")?;
            transaction.rollback().await?;
            if existing_fingerprint != fingerprint {
                return Err(idempotency_conflict(project_id));
            }
            let replay = serde_json::from_str(&existing_response).map_err(ApiError::internal)?;
            return Ok(CommitResult::Replayed(mark_replayed(replay)));
        }

        let updated = sqlx::query(
            "UPDATE projects SET name = ?, current_revision = ?, current_document_json = ?, current_document_hash_json = ?, updated_at = ? WHERE id = ? AND current_revision = ?",
        )
        .bind(&outcome.envelope.document.name)
        .bind(outcome.envelope.revision as i64)
        .bind(&document_json)
        .bind(&document_hash_json)
        .bind(&now)
        .bind(project_id)
        .bind(edit.base_revision as i64)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            if let Some(replay) = self
                .transaction_replay(project_id, &idempotency_key, &fingerprint)
                .await?
            {
                return Ok(CommitResult::Replayed(mark_replayed(replay)));
            }
            let latest = self.read_project(project_id).await?;
            return Err(revision_conflict(edit.base_revision, &latest));
        }

        if let Some(checkpoint) = &agent_checkpoint {
            sqlx::query(
                "INSERT INTO named_versions (id, project_id, name, revision, document_json, document_hash_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&checkpoint.id)
            .bind(project_id)
            .bind(&checkpoint.name)
            .bind(checkpoint.revision as i64)
            .bind(serde_json::to_string(&current.document).map_err(ApiError::internal)?)
            .bind(serde_json::to_string(&current.document_hash).map_err(ApiError::internal)?)
            .bind(checkpoint.created_at.to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        }

        sqlx::query(
            "INSERT INTO revisions (project_id, revision, parent_revision, transaction_id, document_json, document_hash_json, operations_json, actor_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(outcome.envelope.revision as i64)
        .bind(edit.base_revision as i64)
        .bind(&transaction_id)
        .bind(document_json)
        .bind(document_hash_json)
        .bind(&operations_json)
        .bind(&actor_json)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let receipt = sqlx::query(
            "INSERT INTO transactions (transaction_id, project_id, idempotency_key_json, fingerprint_json, base_revision, committed_revision, actor_json, operations_json, inverse_operations_json, response_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(transaction_id)
        .bind(project_id)
        .bind(idempotency_key)
        .bind(fingerprint)
        .bind(edit.base_revision as i64)
        .bind(outcome.envelope.revision as i64)
        .bind(actor_json)
        .bind(operations_json)
        .bind(inverse_json)
        .bind(response_json)
        .bind(now)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = receipt {
            transaction.rollback().await?;
            if error
                .as_database_error()
                .is_some_and(|database| database.is_unique_violation())
            {
                if let Some(replay) = self
                    .transaction_replay(
                        project_id,
                        &serde_json::to_string(&edit.idempotency_key)
                            .map_err(ApiError::internal)?,
                        &serde_json::to_string(
                            &transaction_fingerprint(edit).map_err(domain_edit_error)?,
                        )
                        .map_err(ApiError::internal)?,
                    )
                    .await?
                {
                    return Ok(CommitResult::Replayed(mark_replayed(replay)));
                }
                return Err(ApiError::conflict(
                    "transaction_id_reused",
                    "the transaction id has already been used",
                    json!({ "transactionId": serialized_string(&edit.transaction_id)? }),
                ));
            }
            return Err(error.into());
        }
        transaction.commit().await?;
        Ok(CommitResult::Committed(response))
    }

    /// Check idempotent replay and revision/domain validity before a caller
    /// performs an external side effect such as installing media bytes.
    pub async fn preflight_commit(
        &self,
        project_id: &str,
        edit: &EditTransaction,
    ) -> Result<Option<Value>, ApiError> {
        ensure_transaction_project(project_id, edit)?;
        let idempotency_key =
            serde_json::to_string(&edit.idempotency_key).map_err(ApiError::internal)?;
        let fingerprint =
            serde_json::to_string(&transaction_fingerprint(edit).map_err(domain_edit_error)?)
                .map_err(ApiError::internal)?;
        if let Some(replay) = self
            .transaction_replay(project_id, &idempotency_key, &fingerprint)
            .await?
        {
            return Ok(Some(mark_replayed(replay)));
        }
        let current = self.read_project(project_id).await?;
        match apply_transaction(&current, edit) {
            Ok(_) => {}
            Err(DomainError::RevisionConflict { .. }) => {
                if let Some(replay) = self
                    .transaction_replay(project_id, &idempotency_key, &fingerprint)
                    .await?
                {
                    return Ok(Some(mark_replayed(replay)));
                }
                return Err(revision_conflict(edit.base_revision, &current));
            }
            Err(error) => return Err(domain_edit_error(error)),
        }
        Ok(None)
    }

    /// Conservative reference check used only when rolling back a newly
    /// installed content blob after a failed CAS. Historical revisions count
    /// as references because Undo/restore must remain lossless.
    pub async fn content_hash_referenced(&self, digest: &str) -> Result<bool, ApiError> {
        Ok(self.referenced_content_hashes().await?.contains(digest))
    }

    /// Return every managed digest that must survive maintenance. In addition
    /// to current documents, revision history and named versions, queued/running
    /// job payloads are scanned conservatively so in-flight work cannot lose a
    /// source or checkpoint before it is materialized.
    pub async fn referenced_content_hashes(&self) -> Result<HashSet<String>, ApiError> {
        let mut referenced = HashSet::new();
        let rows = sqlx::query(
            "SELECT current_document_json AS document_json FROM projects
             UNION ALL
             SELECT document_json FROM revisions
             UNION ALL
             SELECT document_json FROM named_versions",
        )
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let document: ProjectDocument =
                serde_json::from_str(&row.try_get::<String, _>("document_json")?)
                    .map_err(ApiError::internal)?;
            let value = serde_json::to_value(document).map_err(ApiError::internal)?;
            collect_sha256_strings(&value, &mut referenced);
        }
        let jobs = sqlx::query(
            "SELECT input_json, output_json, error_json FROM jobs WHERE state IN ('queued', 'running')",
        )
        .fetch_all(&self.pool)
        .await?;
        for row in jobs {
            for column in ["input_json", "output_json", "error_json"] {
                let encoded = row.try_get::<Option<String>, _>(column)?;
                if let Some(encoded) = encoded {
                    let value: Value =
                        serde_json::from_str(&encoded).map_err(ApiError::internal)?;
                    collect_sha256_strings(&value, &mut referenced);
                }
            }
        }
        Ok(referenced)
    }

    async fn transaction_replay(
        &self,
        project_id: &str,
        idempotency_key: &str,
        fingerprint: &str,
    ) -> Result<Option<Value>, ApiError> {
        let row = sqlx::query(
            "SELECT fingerprint_json, response_json FROM transactions WHERE project_id = ? AND idempotency_key_json = ?",
        )
        .bind(project_id)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        let existing_fingerprint: String = row.try_get("fingerprint_json")?;
        if existing_fingerprint != fingerprint {
            return Err(idempotency_conflict(project_id));
        }
        let response: String = row.try_get("response_json")?;
        Ok(Some(
            serde_json::from_str(&response).map_err(ApiError::internal)?,
        ))
    }

    pub async fn list_revisions(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<RevisionRecord>, ApiError> {
        self.ensure_project(project_id).await?;
        let rows = sqlx::query(
            "SELECT revision, parent_revision, transaction_id, document_hash_json, operations_json, actor_json, created_at FROM revisions WHERE project_id = ? ORDER BY revision DESC LIMIT ?",
        )
        .bind(project_id)
        .bind(limit.clamp(1, 500) as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(revision_from_row).collect()
    }

    pub async fn list_versions(&self, project_id: &str) -> Result<Vec<NamedVersion>, ApiError> {
        self.ensure_project(project_id).await?;
        let rows = sqlx::query(
            "SELECT id, project_id, name, revision, document_hash_json, created_at FROM named_versions WHERE project_id = ? ORDER BY created_at DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(version_from_row).collect()
    }

    pub async fn create_version(
        &self,
        project_id: &str,
        name: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<Value, ApiError> {
        if name.trim().is_empty() || name.chars().count() > 120 {
            return Err(ApiError::bad_request(
                "invalid_version_name",
                "version name must contain 1 to 120 characters",
            ));
        }
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        let scope = format!("project:{project_id}:version");
        let fingerprint =
            request_fingerprint(&json!({ "name": name, "expectedRevision": expected_revision }));
        if let Some(value) = self
            .read_receipt(&scope, idempotency_key, &fingerprint)
            .await?
        {
            return Ok(mark_replayed(value));
        }
        let row = sqlx::query(
            "SELECT current_revision, current_document_json, current_document_hash_json FROM projects WHERE id = ?",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("project", project_id))?;
        let envelope = envelope_from_row(&row)?;
        if envelope.revision != expected_revision {
            return Err(revision_conflict(expected_revision, &envelope));
        }
        let version = NamedVersion {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            name: name.trim().to_owned(),
            revision: envelope.revision,
            document_hash: serde_json::to_value(&envelope.document_hash)
                .map_err(ApiError::internal)?,
            created_at: Utc::now(),
        };
        let response = json!({ "replayed": false, "version": &version });
        let _write_guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let head_guard = sqlx::query(
            "UPDATE projects SET updated_at = updated_at WHERE id = ? AND current_revision = ?",
        )
        .bind(project_id)
        .bind(expected_revision as i64)
        .execute(&mut *tx)
        .await?;
        if head_guard.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(revision_conflict(
                expected_revision,
                &self.read_project(project_id).await?,
            ));
        }
        let insert = sqlx::query(
            "INSERT INTO named_versions (id, project_id, name, revision, document_json, document_hash_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&version.id)
        .bind(project_id)
        .bind(&version.name)
        .bind(version.revision as i64)
        .bind(serde_json::to_string(&envelope.document).map_err(ApiError::internal)?)
        .bind(serde_json::to_string(&envelope.document_hash).map_err(ApiError::internal)?)
        .bind(version.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await;
        if let Err(error) = insert {
            tx.rollback().await?;
            if error
                .as_database_error()
                .is_some_and(|database| database.is_unique_violation())
            {
                if let Some(value) = self
                    .read_receipt(&scope, idempotency_key, &fingerprint)
                    .await?
                {
                    return Ok(mark_replayed(value));
                }
                return Err(ApiError::conflict(
                    "version_name_exists",
                    "a named version with this name already exists",
                    json!({ "name": name }),
                ));
            }
            return Err(error.into());
        }
        insert_receipt(&mut tx, &scope, idempotency_key, &fingerprint, &response).await?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn restore_version(
        &self,
        project_id: &str,
        version_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<Value, ApiError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        let scope = format!("project:{project_id}:restore");
        let fingerprint = request_fingerprint(
            &json!({ "versionId": version_id, "expectedRevision": expected_revision }),
        );
        if let Some(value) = self
            .read_receipt(&scope, idempotency_key, &fingerprint)
            .await?
        {
            return Ok(mark_replayed(value));
        }
        let current = self.read_project(project_id).await?;
        if current.revision != expected_revision {
            return Err(revision_conflict(expected_revision, &current));
        }
        let row =
            sqlx::query("SELECT document_json FROM named_versions WHERE id = ? AND project_id = ?")
                .bind(version_id)
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| ApiError::not_found("version", version_id))?;
        let document_json: String = row.try_get("document_json")?;
        let document: ProjectDocument =
            serde_json::from_str(&document_json).map_err(ApiError::internal)?;
        let validated = ProjectEnvelope::new(document).map_err(domain_bad_request)?;
        let next_revision = expected_revision.checked_add(1).ok_or_else(|| {
            ApiError::bad_request(
                "revision_overflow",
                "project revision cannot be incremented",
            )
        })?;
        let hash_json =
            serde_json::to_string(&validated.document_hash).map_err(ApiError::internal)?;
        let response_envelope = json!({
            "document": &validated.document,
            "revision": next_revision,
            "documentHash": &validated.document_hash,
        });
        let response = json!({ "replayed": false, "envelope": response_envelope, "restoredVersionId": version_id });
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE projects SET name = ?, current_revision = ?, current_document_json = ?, current_document_hash_json = ?, updated_at = ? WHERE id = ? AND current_revision = ?",
        )
        .bind(&validated.document.name)
        .bind(next_revision as i64)
        .bind(&document_json)
        .bind(&hash_json)
        .bind(&now)
        .bind(project_id)
        .bind(expected_revision as i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            tx.rollback().await?;
            if let Some(value) = self
                .read_receipt(&scope, idempotency_key, &fingerprint)
                .await?
            {
                return Ok(mark_replayed(value));
            }
            return Err(revision_conflict(
                expected_revision,
                &self.read_project(project_id).await?,
            ));
        }
        sqlx::query(
            "INSERT INTO revisions (project_id, revision, parent_revision, transaction_id, document_json, document_hash_json, operations_json, actor_json, created_at) VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(next_revision as i64)
        .bind(expected_revision as i64)
        .bind(document_json)
        .bind(hash_json)
        .bind(json!([{ "type": "restoreVersion", "versionId": version_id }]).to_string())
        .bind("{\"kind\":\"system\",\"label\":\"version_restore\"}")
        .bind(now)
        .execute(&mut *tx)
        .await?;
        insert_receipt(&mut tx, &scope, idempotency_key, &fingerprint, &response).await?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn list_jobs(
        &self,
        project_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<JobRecord>, ApiError> {
        let rows = if let Some(project_id) = project_id {
            sqlx::query("SELECT * FROM jobs WHERE project_id = ? ORDER BY created_at DESC LIMIT ?")
                .bind(project_id)
                .bind(limit.clamp(1, 500) as i64)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT * FROM jobs ORDER BY created_at DESC LIMIT ?")
                .bind(limit.clamp(1, 500) as i64)
                .fetch_all(&self.pool)
                .await?
        };
        rows.into_iter().map(job_from_row).collect()
    }

    pub async fn read_job(&self, job_id: &str) -> Result<JobRecord, ApiError> {
        let row = sqlx::query("SELECT * FROM jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("job", job_id))?;
        job_from_row(row)
    }

    pub async fn active_job_ids(&self, project_id: &str) -> Result<Vec<String>, ApiError> {
        let rows = sqlx::query(
            "SELECT id FROM jobs WHERE project_id = ? AND state IN ('queued', 'running')",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(ApiError::from))
            .collect()
    }

    pub async fn active_job_ids_global(&self) -> Result<Vec<String>, ApiError> {
        let rows = sqlx::query(
            "SELECT id FROM jobs WHERE state IN ('queued', 'running') ORDER BY created_at, id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("id").map_err(ApiError::from))
            .collect()
    }

    /// Internal worker API. Public HTTP callers cannot enqueue arbitrary jobs;
    /// capabilities add purpose-built, validated job creation paths instead.
    pub async fn enqueue_job(
        &self,
        kind: &str,
        project_id: Option<&str>,
        revision: Option<u64>,
        input: &Value,
    ) -> Result<JobRecord, ApiError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "INSERT INTO jobs (id, project_id, kind, state, progress, input_json, revision, created_at, updated_at) VALUES (?, ?, ?, 'queued', 0, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(kind)
        .bind(serde_json::to_string(input).map_err(ApiError::internal)?)
        .bind(revision.map(|revision| revision as i64))
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.read_job(&id).await
    }

    pub async fn enqueue_job_idempotent(
        &self,
        kind: &str,
        project_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
        input: &Value,
    ) -> Result<(JobRecord, bool), ApiError> {
        self.enqueue_job_idempotent_inner(
            kind,
            project_id,
            expected_revision,
            idempotency_key,
            input,
            true,
        )
        .await
    }

    /// Queue read-only work against an immutable historical revision. Unlike
    /// editing and generation jobs, previewing or exporting revision N remains
    /// valid after the project head advances to N+1.
    pub async fn enqueue_pinned_job_idempotent(
        &self,
        kind: &str,
        project_id: &str,
        revision: u64,
        idempotency_key: &str,
        input: &Value,
    ) -> Result<(JobRecord, bool), ApiError> {
        self.enqueue_job_idempotent_inner(kind, project_id, revision, idempotency_key, input, false)
            .await
    }

    async fn enqueue_job_idempotent_inner(
        &self,
        kind: &str,
        project_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
        input: &Value,
        require_current_head: bool,
    ) -> Result<(JobRecord, bool), ApiError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(ApiError::bad_request(
                "invalid_idempotency_key",
                "idempotency key must contain 1 to 200 bytes",
            ));
        }
        let (scope, fingerprint) = job_receipt_identity(kind, project_id, expected_revision, input);
        if let Some(job) = self
            .find_idempotent_job(kind, project_id, expected_revision, idempotency_key, input)
            .await?
        {
            return Ok((job, true));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let receipt = json!({ "jobId": id });
        let _write_guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        if require_current_head {
            let head_guard = sqlx::query(
                "UPDATE projects SET updated_at = updated_at WHERE id = ? AND current_revision = ?",
            )
            .bind(project_id)
            .bind(expected_revision as i64)
            .execute(&mut *tx)
            .await?;
            if head_guard.rows_affected() == 0 {
                tx.rollback().await?;
                return Err(revision_conflict(
                    expected_revision,
                    &self.read_project(project_id).await?,
                ));
            }
        } else {
            let pinned_exists = sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM revisions WHERE project_id = ? AND revision = ?",
            )
            .bind(project_id)
            .bind(expected_revision as i64)
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
            if !pinned_exists {
                tx.rollback().await?;
                // Preserve the existing not-found/revision error contract.
                self.read_project_revision(project_id, expected_revision)
                    .await?;
                return Err(ApiError::internal(
                    "pinned revision disappeared while its job was queued",
                ));
            }
        }
        let claimed = sqlx::query(
            "INSERT INTO idempotency_receipts (scope, idempotency_key, fingerprint, response_json, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&scope)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(serde_json::to_string(&receipt).map_err(ApiError::internal)?)
        .bind(&now)
        .execute(&mut *tx)
        .await;
        if let Err(error) = claimed {
            tx.rollback().await?;
            if error
                .as_database_error()
                .is_some_and(|database| database.is_unique_violation())
                && let Some(receipt) = self
                    .read_receipt(&scope, idempotency_key, &fingerprint)
                    .await?
            {
                let job_id = receipt
                    .get("jobId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::internal("job receipt has no jobId"))?;
                return Ok((self.read_job(job_id).await?, true));
            }
            return Err(error.into());
        }
        sqlx::query(
            "INSERT INTO jobs (id, project_id, kind, state, progress, input_json, revision, created_at, updated_at) VALUES (?, ?, ?, 'queued', 0, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(kind)
        .bind(serde_json::to_string(input).map_err(ApiError::internal)?)
        .bind(expected_revision as i64)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((self.read_job(&id).await?, false))
    }

    /// Check whether an exact job request has already been accepted. Callers
    /// use this before non-idempotent preflight checks such as output-path
    /// existence so a completed retry still returns the original job.
    pub async fn find_idempotent_job(
        &self,
        kind: &str,
        project_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
        input: &Value,
    ) -> Result<Option<JobRecord>, ApiError> {
        let (scope, fingerprint) = job_receipt_identity(kind, project_id, expected_revision, input);
        let Some(receipt) = self
            .read_receipt(&scope, idempotency_key, &fingerprint)
            .await?
        else {
            return Ok(None);
        };
        let job_id = receipt
            .get("jobId")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::internal("job receipt has no jobId"))?;
        Ok(Some(self.read_job(job_id).await?))
    }

    pub async fn recover_running_jobs(&self) -> Result<u64, ApiError> {
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "UPDATE jobs SET state = 'cancelled', message = 'Cancelled during daemon restart', updated_at = ?, finished_at = ? WHERE state = 'running' AND cancel_requested = 1",
        )
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        let result = sqlx::query(
            "UPDATE jobs SET state = 'queued', progress = 0, message = 'Recovered after daemon restart', started_at = NULL, updated_at = ? WHERE state = 'running' AND cancel_requested = 0",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn claim_next_job(&self, kind: &str) -> Result<Option<JobRecord>, ApiError> {
        let _write_guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id FROM jobs WHERE state = 'queued' AND cancel_requested = 0 AND kind = ? ORDER BY created_at, id LIMIT 1",
        )
        .bind(kind)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let id: String = row.try_get("id")?;
        let now = Utc::now().to_rfc3339();
        let updated = sqlx::query(
            "UPDATE jobs SET state = 'running', started_at = ?, updated_at = ?, message = 'Worker started' WHERE id = ? AND state = 'queued' AND cancel_requested = 0",
        )
        .bind(&now)
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        tx.commit().await?;
        Ok(Some(self.read_job(&id).await?))
    }

    /// Claim one queued job by ID for tests and explicitly targeted recovery.
    pub async fn claim_job_by_id(&self, job_id: &str) -> Result<Option<JobRecord>, ApiError> {
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        let updated = sqlx::query(
            "UPDATE jobs SET state = 'running', started_at = ?, updated_at = ?, message = 'Daemon started' WHERE id = ? AND state = 'queued' AND cancel_requested = 0",
        )
        .bind(&now)
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(None);
        }
        Ok(Some(self.read_job(job_id).await?))
    }

    pub async fn update_job_progress(
        &self,
        job_id: &str,
        progress: f64,
        message: Option<&str>,
    ) -> Result<JobRecord, ApiError> {
        if !progress.is_finite() || !(0.0..=1.0).contains(&progress) {
            return Err(ApiError::bad_request(
                "invalid_job_progress",
                "job progress must be between zero and one",
            ));
        }
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "UPDATE jobs SET progress = ?, message = ?, updated_at = ? WHERE id = ? AND state = 'running'",
        )
        .bind(progress)
        .bind(message)
        .bind(now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        self.read_job(job_id).await
    }

    /// Persist a durable provider checkpoint before the next remote side
    /// effect. `recover_running_jobs` deliberately preserves `output_json`, so
    /// a daemon restart can resume polling/downloading from the recorded remote
    /// job ID instead of submitting (and potentially charging) twice.
    pub async fn checkpoint_job(
        &self,
        job_id: &str,
        progress: f64,
        message: &str,
        checkpoint: &Value,
    ) -> Result<JobRecord, ApiError> {
        if !progress.is_finite() || !(0.0..=1.0).contains(&progress) {
            return Err(ApiError::bad_request(
                "invalid_job_progress",
                "job progress must be between zero and one",
            ));
        }
        let encoded = serde_json::to_string(checkpoint).map_err(ApiError::internal)?;
        if encoded.len() > 1024 * 1024 {
            return Err(ApiError::bad_request(
                "provider_checkpoint_too_large",
                "provider checkpoint exceeds the 1 MiB durable-state limit",
            ));
        }
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        let updated = sqlx::query(
            "UPDATE jobs SET progress = ?, message = ?, output_json = ?, updated_at = ? WHERE id = ? AND state = 'running'",
        )
        .bind(progress)
        .bind(message)
        .bind(encoded)
        .bind(now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(ApiError::conflict(
                "job_not_running",
                "the job cannot be checkpointed because it is not running",
                json!({ "jobId": job_id }),
            ));
        }
        self.read_job(job_id).await
    }

    pub async fn complete_job(&self, job_id: &str, output: &Value) -> Result<JobRecord, ApiError> {
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "UPDATE jobs SET state = 'succeeded', progress = 1, output_json = ?, error_json = NULL, message = 'Complete', updated_at = ?, finished_at = ? WHERE id = ? AND state = 'running'",
        )
        .bind(serde_json::to_string(output).map_err(ApiError::internal)?)
        .bind(&now)
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        self.read_job(job_id).await
    }

    pub async fn fail_job(&self, job_id: &str, error: &Value) -> Result<JobRecord, ApiError> {
        self.fail_job_with_output(job_id, error, None).await
    }

    pub async fn fail_job_with_output(
        &self,
        job_id: &str,
        error: &Value,
        output: Option<&Value>,
    ) -> Result<JobRecord, ApiError> {
        let now = Utc::now().to_rfc3339();
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Worker failed");
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "UPDATE jobs SET state = 'failed', output_json = COALESCE(?, output_json), error_json = ?, message = ?, updated_at = ?, finished_at = ? WHERE id = ? AND state = 'running'",
        )
        .bind(
            output
                .map(serde_json::to_string)
                .transpose()
                .map_err(ApiError::internal)?,
        )
        .bind(serde_json::to_string(error).map_err(ApiError::internal)?)
        .bind(message)
        .bind(&now)
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        self.read_job(job_id).await
    }

    pub async fn mark_job_cancelled(&self, job_id: &str) -> Result<JobRecord, ApiError> {
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "UPDATE jobs SET state = 'cancelled', cancel_requested = 1, message = 'Cancelled', updated_at = ?, finished_at = ? WHERE id = ? AND state IN ('queued', 'running')",
        )
        .bind(&now)
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        self.read_job(job_id).await
    }

    /// Return a job interrupted by daemon shutdown to the durable queue while
    /// retaining its checkpoint. An explicit user cancellation always wins.
    pub async fn requeue_interrupted_job(&self, job_id: &str) -> Result<JobRecord, ApiError> {
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "UPDATE jobs SET state = CASE WHEN cancel_requested = 1 THEN 'cancelled' ELSE 'queued' END, message = CASE WHEN cancel_requested = 1 THEN 'Cancelled' ELSE 'Interrupted by daemon shutdown; queued to resume' END, started_at = NULL, finished_at = CASE WHEN cancel_requested = 1 THEN ? ELSE NULL END, updated_at = ? WHERE id = ? AND state = 'running'",
        )
        .bind(&now)
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        self.read_job(job_id).await
    }

    pub async fn request_job_cancel(&self, job_id: &str) -> Result<JobRecord, ApiError> {
        let now = Utc::now().to_rfc3339();
        let _write_guard = self.write_lock.lock().await;
        let updated = sqlx::query(
            "UPDATE jobs SET cancel_requested = 1, state = CASE WHEN state = 'queued' THEN 'cancelled' ELSE state END, finished_at = CASE WHEN state = 'queued' THEN ? ELSE finished_at END, updated_at = ? WHERE id = ? AND state IN ('queued', 'running')",
        )
        .bind(&now)
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            let job = self.read_job(job_id).await?;
            if matches!(job.state.as_str(), "succeeded" | "failed" | "cancelled") {
                return Ok(job);
            }
        }
        self.read_job(job_id).await
    }

    async fn ensure_project(&self, project_id: &str) -> Result<(), ApiError> {
        let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_one(&self.pool)
            .await?;
        if exists == 0 {
            Err(ApiError::not_found("project", project_id))
        } else {
            Ok(())
        }
    }

    async fn read_receipt(
        &self,
        scope: &str,
        key: &str,
        fingerprint: &str,
    ) -> Result<Option<Value>, ApiError> {
        let row = sqlx::query(
            "SELECT fingerprint, response_json FROM idempotency_receipts WHERE scope = ? AND idempotency_key = ?",
        )
        .bind(scope)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        let existing: String = row.try_get("fingerprint")?;
        if existing != fingerprint {
            return Err(ApiError::conflict(
                "idempotency_key_reused",
                "the idempotency key was already used for a different request",
                json!({ "scope": scope }),
            ));
        }
        let response: String = row.try_get("response_json")?;
        Ok(Some(
            serde_json::from_str(&response).map_err(ApiError::internal)?,
        ))
    }
}

async fn insert_receipt(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    scope: &str,
    key: &str,
    fingerprint: &str,
    response: &Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO idempotency_receipts (scope, idempotency_key, fingerprint, response_json, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(scope)
    .bind(key)
    .bind(fingerprint)
    .bind(serde_json::to_string(response).map_err(ApiError::internal)?)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn history_marker(
    operations: &Value,
) -> Result<Option<(HistoryDirection, HistoryEntry)>, ApiError> {
    let Some(operation) = operations
        .as_array()
        .filter(|operations| operations.len() == 1)
        .and_then(|operations| operations.first())
    else {
        return Ok(None);
    };
    let direction = match operation.get("type").and_then(Value::as_str) {
        Some("undoRevision") => HistoryDirection::Undo,
        Some("redoRevision") => HistoryDirection::Redo,
        _ => return Ok(None),
    };
    let revision = operation
        .get("sourceRevision")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::internal("history marker has no sourceRevision"))?;
    let before_revision = operation
        .get("beforeRevision")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::internal("history marker has no beforeRevision"))?;
    Ok(Some((
        direction,
        HistoryEntry {
            revision,
            before_revision,
        },
    )))
}

fn job_receipt_identity(
    kind: &str,
    project_id: &str,
    expected_revision: u64,
    input: &Value,
) -> (String, String) {
    (
        format!("project:{project_id}:job:{kind}"),
        request_fingerprint(&json!({
            "kind": kind,
            "projectId": project_id,
            "expectedRevision": expected_revision,
            "input": input,
        })),
    )
}

fn envelope_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ProjectEnvelope, ApiError> {
    let document_json: String = row.try_get("current_document_json")?;
    let document: ProjectDocument =
        serde_json::from_str(&document_json).map_err(ApiError::internal)?;
    let revision: i64 = row.try_get("current_revision")?;
    let stored_hash: Value =
        serde_json::from_str(&row.try_get::<String, _>("current_document_hash_json")?)
            .map_err(ApiError::internal)?;
    let document_hash = canonical_document_hash(&document).map_err(ApiError::internal)?;
    let actual_hash = serde_json::to_value(&document_hash).map_err(ApiError::internal)?;
    if actual_hash != stored_hash {
        return Err(ApiError::internal(
            "stored project document hash does not match its content",
        ));
    }
    let envelope = ProjectEnvelope {
        document,
        revision: revision as u64,
        document_hash,
    };
    envelope.verify().map_err(ApiError::internal)?;
    Ok(envelope)
}

fn project_summary_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ProjectSummary, ApiError> {
    Ok(ProjectSummary {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        current_revision: row.try_get::<i64, _>("current_revision")? as u64,
        document_hash: serde_json::from_str(
            &row.try_get::<String, _>("current_document_hash_json")?,
        )
        .map_err(ApiError::internal)?,
        auto_apply: row.try_get::<i64, _>("auto_apply")? != 0,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
        updated_at: parse_timestamp(row.try_get("updated_at")?)?,
    })
}

fn revision_from_row(row: sqlx::sqlite::SqliteRow) -> Result<RevisionRecord, ApiError> {
    Ok(RevisionRecord {
        revision: row.try_get::<i64, _>("revision")? as u64,
        parent_revision: row
            .try_get::<Option<i64>, _>("parent_revision")?
            .map(|value| value as u64),
        transaction_id: row.try_get("transaction_id")?,
        document_hash: parse_json_column(&row, "document_hash_json")?,
        operations: parse_json_column(&row, "operations_json")?,
        actor: parse_json_column(&row, "actor_json")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}

fn version_from_row(row: sqlx::sqlite::SqliteRow) -> Result<NamedVersion, ApiError> {
    Ok(NamedVersion {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        name: row.try_get("name")?,
        revision: row.try_get::<i64, _>("revision")? as u64,
        document_hash: parse_json_column(&row, "document_hash_json")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}

fn job_from_row(row: sqlx::sqlite::SqliteRow) -> Result<JobRecord, ApiError> {
    Ok(JobRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        kind: row.try_get("kind")?,
        state: row.try_get("state")?,
        progress: row.try_get("progress")?,
        input: parse_json_column(&row, "input_json")?,
        output: parse_optional_json_column(&row, "output_json")?,
        error: parse_optional_json_column(&row, "error_json")?,
        message: row.try_get("message")?,
        revision: row
            .try_get::<Option<i64>, _>("revision")?
            .map(|value| value as u64),
        cancel_requested: row.try_get::<i64, _>("cancel_requested")? != 0,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
        updated_at: parse_timestamp(row.try_get("updated_at")?)?,
        started_at: parse_optional_timestamp(row.try_get("started_at")?)?,
        finished_at: parse_optional_timestamp(row.try_get("finished_at")?)?,
    })
}

fn agent_session_summary_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<AgentSessionSummary, ApiError> {
    Ok(AgentSessionSummary {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        title: row.try_get("title")?,
        provider: row.try_get("provider")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
        updated_at: parse_timestamp(row.try_get("updated_at")?)?,
    })
}

fn agent_message_record_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<AgentMessageRecord, ApiError> {
    Ok(AgentMessageRecord {
        id: row.try_get("id")?,
        role: row.try_get("role")?,
        status: row.try_get("status")?,
        text: row.try_get("text")?,
        proposal: parse_optional_json_column(row, "proposal_json")?,
        history_action: parse_optional_json_column(row, "history_action_json")?,
        workflow: parse_optional_json_column(row, "workflow_json")?,
        error: parse_optional_json_column(row, "error_json")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
        updated_at: parse_timestamp(row.try_get("updated_at")?)?,
    })
}

fn parse_json_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Value, ApiError> {
    serde_json::from_str(&row.try_get::<String, _>(column)?).map_err(ApiError::internal)
}

fn parse_optional_json_column(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<Value>, ApiError> {
    row.try_get::<Option<String>, _>(column)?
        .map(|value| serde_json::from_str(&value).map_err(ApiError::internal))
        .transpose()
}

fn parse_timestamp(value: String) -> Result<DateTime<Utc>, ApiError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(ApiError::internal)
}

fn parse_optional_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>, ApiError> {
    value.map(parse_timestamp).transpose()
}

fn ensure_transaction_project(project_id: &str, edit: &EditTransaction) -> Result<(), ApiError> {
    let transaction_project = serialized_string(&edit.project_id)?;
    if transaction_project == project_id {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "project_id_mismatch",
            "transaction project id does not match the route",
        )
        .with_details(json!({
            "routeProjectId": project_id,
            "transactionProjectId": transaction_project,
        })))
    }
}

fn serialized_string(value: &impl Serialize) -> Result<String, ApiError> {
    serde_json::to_value(value)
        .map_err(ApiError::internal)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ApiError::internal("domain identifier did not serialize as a string"))
}

fn mark_replayed(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("replayed".to_owned(), Value::Bool(true));
    }
    value
}

fn request_fingerprint(value: &Value) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(value).expect("JSON value always serializes"),
    ))
}

fn collect_sha256_strings(value: &Value, output: &mut HashSet<String>) {
    match value {
        Value::String(value)
            if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            output.insert(value.to_ascii_lowercase());
        }
        Value::Array(values) => {
            for value in values {
                collect_sha256_strings(value, output);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_sha256_strings(value, output);
            }
        }
        _ => {}
    }
}

fn revision_conflict(expected: u64, current: &ProjectEnvelope) -> ApiError {
    ApiError::conflict(
        "revisionConflict",
        "the project changed after this edit was prepared",
        json!({
            "expectedRevision": expected,
            "currentRevision": current.revision,
            "currentDocumentHash": current.document_hash,
        }),
    )
}

fn idempotency_conflict(project_id: &str) -> ApiError {
    ApiError::conflict(
        "idempotency_key_reused",
        "the idempotency key was already used for a different transaction",
        json!({ "projectId": project_id }),
    )
}

fn domain_bad_request(error: DomainError) -> ApiError {
    let details = serde_json::to_value(&error).unwrap_or_else(|_| json!({}));
    ApiError::bad_request(error.error_code(), error.to_string()).with_details(details)
}

fn domain_edit_error(error: DomainError) -> ApiError {
    let details = serde_json::to_value(&error).unwrap_or_else(|_| json!({}));
    if error.is_conflict() {
        ApiError::conflict(error.error_code(), error.to_string(), details)
    } else {
        ApiError::bad_request(error.error_code(), error.to_string()).with_details(details)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn database_uses_wal_and_creates_job_queue() {
        let temp = tempfile::tempdir().unwrap();
        let database = Database::open(&temp.path().join("daemon.sqlite3"))
            .await
            .unwrap();
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&database.pool)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        let job = database
            .enqueue_job("test.noop", None, None, &json!({ "value": 1 }))
            .await
            .unwrap();
        assert_eq!(job.state, "queued");
        let cancelled = database.request_job_cancel(&job.id).await.unwrap();
        assert_eq!(cancelled.state, "cancelled");
        assert!(cancelled.cancel_requested);

        let running = database
            .enqueue_job("transcription", None, None, &json!({ "value": 2 }))
            .await
            .unwrap();
        let claimed = database
            .claim_next_job("transcription")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.id, running.id);
        assert_eq!(claimed.state, "running");
        let progressed = database
            .update_job_progress(&claimed.id, 0.5, Some("Halfway"))
            .await
            .unwrap();
        assert_eq!(progressed.progress, 0.5);
        assert_eq!(progressed.message.as_deref(), Some("Halfway"));
        assert_eq!(database.recover_running_jobs().await.unwrap(), 1);
        assert_eq!(
            database.read_job(&claimed.id).await.unwrap().state,
            "queued"
        );
        let reclaimed = database
            .claim_next_job("transcription")
            .await
            .unwrap()
            .unwrap();
        let completed = database
            .complete_job(&reclaimed.id, &json!({ "words": [] }))
            .await
            .unwrap();
        assert_eq!(completed.state, "succeeded");
        assert_eq!(completed.output, Some(json!({ "words": [] })));
    }
}
