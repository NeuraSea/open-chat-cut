use std::{path::PathBuf, process::Stdio, time::Duration};

use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

const MAX_REQUEST_BYTES: usize = 512 * 1024;
const MAX_STDOUT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: u64 = 64 * 1024;
const MAX_IR_VALUES: usize = 50_000;
const MAX_IR_DEPTH: usize = 100;
const COMPILE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct MotionGraphicRuntime {
    node: PathBuf,
    entrypoint: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CompiledMotionGraphic {
    pub ir: Value,
    pub stats: Value,
    pub asset_ids: Vec<String>,
    pub security: Value,
}

#[derive(Debug, Error)]
pub enum MotionGraphicRuntimeError {
    #[error("{message}")]
    Validation {
        code: String,
        message: String,
        path: Option<String>,
    },
    #[error("advanced motion graphic compiler is unavailable: {0}")]
    Unavailable(String),
}

impl MotionGraphicRuntimeError {
    pub fn validation_details(&self) -> Option<Value> {
        match self {
            Self::Validation {
                code,
                message,
                path,
            } => Some(json!({
                "code": code,
                "message": message,
                "path": path,
            })),
            Self::Unavailable(_) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeResponse {
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<RuntimeValidationError>,
}

#[derive(Debug, Deserialize)]
struct RuntimeValidationError {
    code: String,
    message: String,
    #[serde(default)]
    path: Option<String>,
}

impl MotionGraphicRuntime {
    pub fn new(node: PathBuf, entrypoint: PathBuf) -> Self {
        Self { node, entrypoint }
    }

    pub async fn compile_jsx(
        &self,
        source: &str,
        width: u32,
        height: u32,
        duration_seconds: f64,
        fps: f64,
    ) -> Result<CompiledMotionGraphic, MotionGraphicRuntimeError> {
        let request = serde_json::to_vec(&json!({
            "mode": "jsx",
            "definition": source,
            "context": {
                "width": width,
                "height": height,
                "durationSeconds": duration_seconds,
                "fps": fps,
            }
        }))
        .map_err(unavailable)?;
        if request.len() > MAX_REQUEST_BYTES {
            return Err(MotionGraphicRuntimeError::Validation {
                code: "MG_INPUT_LIMIT".to_owned(),
                message: "Advanced motion graphic request is too large".to_owned(),
                path: None,
            });
        }

        let mut command = Command::new(&self.node);
        command
            .arg(&self.entrypoint)
            .current_dir(
                self.entrypoint
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(".")),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env_clear();
        for key in ["PATH", "HOME", "TMPDIR", "TEMP", "SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let mut child = command.spawn().map_err(unavailable)?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| unavailable("MG compiler stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| unavailable("MG compiler stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| unavailable("MG compiler stderr unavailable"))?;
        let stdout_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut limited = stdout.take(MAX_STDOUT_BYTES + 1);
            limited.read_to_end(&mut bytes).await.map(|_| bytes)
        });
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut limited = stderr.take(MAX_STDERR_BYTES + 1);
            limited.read_to_end(&mut bytes).await.map(|_| bytes)
        });
        stdin.write_all(&request).await.map_err(unavailable)?;
        stdin.shutdown().await.map_err(unavailable)?;
        drop(stdin);

        let status = match tokio::time::timeout(COMPILE_TIMEOUT, child.wait()).await {
            Ok(result) => result.map_err(unavailable)?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(unavailable("compiler timed out"));
            }
        };
        let stdout = stdout_task
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
        let stderr = stderr_task
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
        if stdout.len() as u64 > MAX_STDOUT_BYTES || stderr.len() as u64 > MAX_STDERR_BYTES {
            return Err(unavailable("compiler output exceeded its limit"));
        }
        let response: RuntimeResponse = serde_json::from_slice(&stdout)
            .map_err(|_| unavailable("compiler emitted invalid JSON"))?;
        if !response.ok {
            let error = response
                .error
                .ok_or_else(|| unavailable("compiler rejected input without an error"))?;
            return Err(MotionGraphicRuntimeError::Validation {
                code: bounded_text(error.code, 96, "MG_VALIDATION_FAILED"),
                message: bounded_text(error.message, 1_024, "Advanced motion graphic is invalid"),
                path: error.path.map(|path| bounded_text(path, 512, "$")),
            });
        }
        if !status.success() {
            return Err(unavailable("compiler exited unsuccessfully"));
        }
        let result = response
            .result
            .ok_or_else(|| unavailable("compiler returned no result"))?;
        verify_compiled_result(result, width, height, duration_seconds, fps)
    }
}

fn verify_compiled_result(
    result: Value,
    width: u32,
    height: u32,
    duration_seconds: f64,
    fps: f64,
) -> Result<CompiledMotionGraphic, MotionGraphicRuntimeError> {
    let object = result
        .as_object()
        .ok_or_else(|| unavailable("compiler result is not an object"))?;
    let ir = object
        .get("ir")
        .cloned()
        .ok_or_else(|| unavailable("compiler result has no safe IR"))?;
    let root = ir
        .as_object()
        .ok_or_else(|| unavailable("compiler safe IR is not an object"))?;
    if root.get("version").and_then(Value::as_u64) != Some(1)
        || root.get("kind").and_then(Value::as_str) != Some("jsxSafeIr")
        || root.get("width").and_then(Value::as_u64) != Some(width.into())
        || root.get("height").and_then(Value::as_u64) != Some(height.into())
        || !number_matches(root.get("durationSeconds"), duration_seconds)
        || !number_matches(root.get("fps"), fps)
    {
        return Err(unavailable(
            "compiler safe IR context does not match the request",
        ));
    }
    let mut visited = 0;
    validate_bounded_json(&ir, 0, &mut visited)?;

    let stats = object
        .get("stats")
        .cloned()
        .ok_or_else(|| unavailable("compiler result has no validation statistics"))?;
    let ast_nodes = stats
        .get("astNodes")
        .and_then(Value::as_u64)
        .ok_or_else(|| unavailable("compiler result has invalid AST statistics"))?;
    if ast_nodes == 0 || ast_nodes > 20_000 {
        return Err(unavailable("compiler AST statistics exceed safe bounds"));
    }

    let asset_ids = object
        .get("assetIds")
        .and_then(Value::as_array)
        .ok_or_else(|| unavailable("compiler result has no asset list"))?;
    if asset_ids.len() > 1_024 {
        return Err(unavailable("compiler returned too many asset references"));
    }
    let mut asset_ids = asset_ids
        .iter()
        .map(|value| {
            let id = value
                .as_str()
                .ok_or_else(|| unavailable("compiler returned a non-string asset reference"))?;
            if !id.starts_with("asset:")
                || id.len() > 262
                || id.chars().any(|character| character.is_control())
            {
                return Err(unavailable("compiler returned an invalid asset reference"));
            }
            Ok(id.to_owned())
        })
        .collect::<Result<Vec<_>, MotionGraphicRuntimeError>>()?;
    asset_ids.sort();
    asset_ids.dedup();

    let security = object
        .get("security")
        .cloned()
        .ok_or_else(|| unavailable("compiler result has no security metadata"))?;
    if security.get("sourceExecuted").and_then(Value::as_bool) != Some(false)
        || security.get("networkAccess").and_then(Value::as_str) != Some("disabled")
        || security.get("fileAccess").and_then(Value::as_str) != Some("disabled")
        || security.get("sandboxOrigin").and_then(Value::as_str) != Some("opaque")
        || security.get("interpreter").and_then(Value::as_str)
            != Some("deterministic-allowlisted-ir-v1")
    {
        return Err(unavailable("compiler returned invalid security metadata"));
    }
    Ok(CompiledMotionGraphic {
        ir,
        stats,
        asset_ids,
        security,
    })
}

fn validate_bounded_json(
    value: &Value,
    depth: usize,
    visited: &mut usize,
) -> Result<(), MotionGraphicRuntimeError> {
    if depth > MAX_IR_DEPTH {
        return Err(unavailable("compiler safe IR exceeds its depth limit"));
    }
    *visited += 1;
    if *visited > MAX_IR_VALUES {
        return Err(unavailable("compiler safe IR exceeds its value limit"));
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
        Value::String(value) => {
            if value.len() > 256 * 1024 {
                return Err(unavailable("compiler safe IR contains an oversized string"));
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_bounded_json(value, depth + 1, visited)?;
            }
        }
        Value::Object(values) => {
            for forbidden in ["__proto__", "prototype", "constructor"] {
                if values.contains_key(forbidden) {
                    return Err(unavailable("compiler safe IR contains a forbidden key"));
                }
            }
            for value in values.values() {
                validate_bounded_json(value, depth + 1, visited)?;
            }
        }
    }
    Ok(())
}

fn number_matches(value: Option<&Value>, expected: f64) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|actual| (actual - expected).abs() <= 0.000_001)
}

fn bounded_text(value: String, max: usize, fallback: &str) -> String {
    if value.is_empty() || value.chars().any(char::is_control) {
        return fallback.to_owned();
    }
    value.chars().take(max).collect()
}

fn unavailable(error: impl std::fmt::Display) -> MotionGraphicRuntimeError {
    MotionGraphicRuntimeError::Unavailable(error.to_string())
}
