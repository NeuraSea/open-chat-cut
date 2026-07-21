use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use url::Url;

pub const DEFAULT_PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone, Parser)]
#[command(name = "openchatcutd", about = "Local OpenChatCut project daemon")]
pub struct Cli {
    /// Loopback address on which the API listens.
    #[arg(long, env = "OPENCHATCUT_BIND", default_value = "127.0.0.1:3210")]
    pub bind: SocketAddr,

    /// Explicit constrained-container mode. This permits an unspecified
    /// container interface only when the published host port remains bound to
    /// loopback by the supplied Compose profile.
    #[arg(long, env = "OPENCHATCUT_CONTAINERIZED", default_value_t = false)]
    pub containerized: bool,

    /// Persistent daemon data directory.
    #[arg(long, env = "OPENCHATCUT_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Runtime descriptor read by local clients.
    #[arg(long, env = "OPENCHATCUT_RUNTIME_DESCRIPTOR")]
    pub runtime_descriptor: Option<PathBuf>,

    /// File containing the daemon bearer token.
    #[arg(long, env = "OPENCHATCUT_TOKEN_PATH")]
    pub token_path: Option<PathBuf>,

    /// Private JSON file containing generation-provider endpoints and keys.
    /// The daemon is the only component that reads this file; the Web editor
    /// and STDIO MCP bridge only receive redacted provider descriptors.
    #[arg(long, env = "OPENCHATCUT_PROVIDER_CONFIG")]
    pub provider_config: Option<PathBuf>,

    /// Loopback Web editor origin returned to native clients.
    #[arg(
        long,
        env = "OPENCHATCUT_EDITOR_URL",
        default_value = "http://127.0.0.1:3100"
    )]
    pub editor_url: String,

    /// Explicit HTTPS origin for a single-user hosted deployment behind an
    /// authenticated reverse proxy. This never changes the daemon's own bind
    /// or Host-header restrictions.
    #[arg(long, env = "OPENCHATCUT_HOSTED_ORIGIN")]
    pub hosted_origin: Option<String>,

    /// Optional editor origin reachable by the headless worker. Native mode
    /// leaves this unset; the constrained Compose profile uses the internal
    /// service origin while still returning the loopback editor URL to users.
    #[arg(long, env = "OPENCHATCUT_WORKER_EDITOR_URL")]
    pub worker_editor_url: Option<String>,

    /// Optional local JSON-stdio media worker executable.
    #[arg(long, env = "OPENCHATCUT_MEDIA_WORKER")]
    pub media_worker: Option<PathBuf>,

    /// Repository-owned advanced motion-graphic validator/compiler entrypoint.
    /// JSX source is parsed into non-executable safe IR; it is never evaluated
    /// by this process.
    #[arg(long, env = "OPENCHATCUT_MG_RUNTIME_CLI")]
    pub mg_runtime_cli: Option<PathBuf>,

    /// Node.js executable used only to run the repository-owned MG compiler.
    #[arg(long, env = "OPENCHATCUT_NODE_COMMAND", default_value = "node")]
    pub node_command: PathBuf,

    /// Codex CLI used only through its app-server protocol. The daemon never
    /// reads Codex credential files; authentication remains owned by Codex.
    #[arg(long, env = "OPENCHATCUT_CODEX_COMMAND", default_value = "codex")]
    pub codex_command: PathBuf,

    /// Host directory from which managed local media may be imported. May be
    /// repeated. No host file is readable through the API unless it is below
    /// one of these explicitly authorized roots.
    #[arg(
        long = "authorized-import-root",
        env = "OPENCHATCUT_IMPORT_ROOTS",
        value_delimiter = ','
    )]
    pub authorized_import_roots: Vec<PathBuf>,

    /// Additional browser origin. May be repeated; origins must be loopback
    /// except for explicit internal service origins in constrained-container mode.
    #[arg(
        long = "allowed-origin",
        env = "OPENCHATCUT_ALLOWED_ORIGINS",
        value_delimiter = ','
    )]
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub containerized: bool,
    pub home_dir: PathBuf,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub runtime_descriptor: PathBuf,
    pub token_path: PathBuf,
    pub provider_config: PathBuf,
    pub editor_url: String,
    pub worker_editor_url: String,
    pub media_worker: Option<PathBuf>,
    pub mg_runtime_node: Option<PathBuf>,
    pub mg_runtime_cli: Option<PathBuf>,
    pub codex_command: Option<PathBuf>,
    pub authorized_import_roots: Vec<PathBuf>,
    pub allowed_origins: HashSet<String>,
    pub browser_session_ttl: Duration,
    pub secure_browser_cookie: bool,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        if !cli.bind.ip().is_loopback() && !(cli.containerized && cli.bind.ip().is_unspecified()) {
            bail!("refusing to bind to non-loopback address {}", cli.bind);
        }

        let home_dir = std::env::var_os("OPENCHATCUT_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|path| path.join(".openchatcut")))
            .context("could not determine OpenChatCut home; set OPENCHATCUT_HOME")?;
        let data_dir = cli.data_dir.unwrap_or_else(|| home_dir.join("data"));
        let runtime_descriptor = cli
            .runtime_descriptor
            .unwrap_or_else(|| home_dir.join("runtime.json"));
        let token_path = cli
            .token_path
            .unwrap_or_else(|| home_dir.join("daemon.token"));
        let provider_config = cli
            .provider_config
            .unwrap_or_else(|| home_dir.join("providers.json"));
        let editor_url = if let Some(origin) = &cli.hosted_origin {
            if !cli.containerized {
                bail!("OPENCHATCUT_HOSTED_ORIGIN requires constrained container mode");
            }
            validate_hosted_origin(origin)?;
            normalize_origin(origin)?
        } else {
            validate_loopback_origin(&cli.editor_url)?;
            normalize_origin(&cli.editor_url)?
        };
        let worker_editor_url = match cli.worker_editor_url {
            Some(origin) => {
                validate_browser_origin(&origin, cli.containerized)?;
                normalize_origin(&origin)?
            }
            None => editor_url.clone(),
        };
        let media_worker = cli.media_worker.and_then(|worker| {
            let resolved = resolve_executable(&worker);
            if resolved.is_none() {
                tracing::warn!(path = %worker.display(), "configured media worker is not executable; capability disabled");
            }
            resolved
        });
        let mg_runtime_cli = cli.mg_runtime_cli.and_then(|entrypoint| {
            let resolved = resolve_regular_file(&entrypoint);
            if resolved.is_none() {
                tracing::warn!(path = %entrypoint.display(), "configured MG runtime entrypoint is not a regular file; capability disabled");
            }
            resolved
        });
        let mg_runtime_node = if mg_runtime_cli.is_some() {
            let resolved = resolve_executable(&cli.node_command);
            if resolved.is_none() {
                tracing::warn!(path = %cli.node_command.display(), "Node.js is unavailable; advanced MG capability disabled");
            }
            resolved
        } else {
            None
        };
        let codex_command = resolve_executable(&cli.codex_command);
        if codex_command.is_none() {
            tracing::warn!(path = %cli.codex_command.display(), "Codex CLI is unavailable; editor Agent capability disabled");
        }
        let authorized_import_roots = canonical_import_roots(cli.authorized_import_roots)?;

        let mut origins = HashSet::from([
            "http://127.0.0.1:3100".to_owned(),
            "http://localhost:3100".to_owned(),
        ]);
        origins.insert(editor_url.clone());
        for origin in cli.allowed_origins {
            validate_browser_origin(&origin, cli.containerized)?;
            origins.insert(normalize_origin(&origin)?);
        }

        Ok(Self {
            bind: cli.bind,
            containerized: cli.containerized,
            home_dir,
            database_path: data_dir.join("openchatcut.sqlite3"),
            data_dir,
            runtime_descriptor,
            token_path,
            provider_config,
            editor_url,
            worker_editor_url,
            media_worker,
            mg_runtime_node,
            mg_runtime_cli,
            codex_command,
            authorized_import_roots,
            allowed_origins: origins,
            browser_session_ttl: Duration::from_secs(15 * 60),
            secure_browser_cookie: cli.hosted_origin.is_some(),
        })
    }

    pub fn for_test(root: PathBuf) -> Self {
        let home_dir = root.join("home");
        let data_dir = root.join("data");
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            containerized: false,
            database_path: data_dir.join("openchatcut.sqlite3"),
            runtime_descriptor: home_dir.join("runtime.json"),
            token_path: home_dir.join("daemon.token"),
            provider_config: home_dir.join("providers.json"),
            editor_url: "http://127.0.0.1:3100".to_owned(),
            worker_editor_url: "http://127.0.0.1:3100".to_owned(),
            media_worker: None,
            mg_runtime_node: None,
            mg_runtime_cli: None,
            codex_command: None,
            authorized_import_roots: Vec::new(),
            home_dir,
            data_dir,
            allowed_origins: HashSet::from([
                "http://127.0.0.1:3100".to_owned(),
                "http://localhost:3100".to_owned(),
            ]),
            browser_session_ttl: Duration::from_secs(15 * 60),
            secure_browser_cookie: false,
        }
    }

    pub fn api_base_url(&self, bound_address: SocketAddr) -> String {
        let host = match bound_address.ip() {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        format!("http://{host}:{}/api/v1", bound_address.port())
    }
}

fn resolve_regular_file(value: &std::path::Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(value).ok()?;
    std::fs::metadata(&canonical)
        .ok()?
        .is_file()
        .then_some(canonical)
}

fn canonical_import_roots(values: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut roots = values
        .into_iter()
        .map(|root| {
            let canonical = std::fs::canonicalize(&root).with_context(|| {
                format!("authorized import root {} is not readable", root.display())
            })?;
            if !std::fs::metadata(&canonical)?.is_dir() {
                bail!(
                    "authorized import root {} is not a directory",
                    root.display()
                );
            }
            Ok(canonical)
        })
        .collect::<Result<Vec<_>>>()?;
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn resolve_executable(value: &std::path::Path) -> Option<PathBuf> {
    let has_path = value.is_absolute() || value.components().count() > 1;
    let candidates = if has_path {
        vec![value.to_owned()]
    } else {
        std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .flat_map(|directory| executable_names(&directory.join(value)))
                    .collect()
            })
            .unwrap_or_default()
    };
    candidates.into_iter().find_map(|candidate| {
        let metadata = std::fs::metadata(&candidate).ok()?;
        if !metadata.is_file() || !is_executable(&metadata) {
            return None;
        }
        std::fs::canonicalize(candidate).ok()
    })
}

#[cfg(windows)]
fn executable_names(value: &std::path::Path) -> Vec<PathBuf> {
    if value.extension().is_some() {
        return vec![value.to_owned()];
    }
    let extensions = std::env::var_os("PATHEXT")
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| ".EXE;.CMD;.BAT;.COM".to_owned());
    extensions
        .split(';')
        .map(|extension| value.with_extension(extension.trim_start_matches('.')))
        .collect()
}

#[cfg(not(windows))]
fn executable_names(value: &std::path::Path) -> Vec<PathBuf> {
    vec![value.to_owned()]
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    true
}

pub fn normalize_origin(value: &str) -> Result<String> {
    let parsed = Url::parse(value).with_context(|| format!("invalid origin {value:?}"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("origin must be an http(s) scheme/host/port tuple: {value}");
    }
    parsed.host_str().context("origin has no host")?;
    Ok(parsed.origin().ascii_serialization())
}

pub fn validate_loopback_origin(value: &str) -> Result<()> {
    let parsed = Url::parse(value).with_context(|| format!("invalid origin {value:?}"))?;
    if parsed.scheme() != "http" {
        bail!("only http loopback origins are supported");
    }
    let host = parsed.host_str().context("origin has no host")?;
    let ip_host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let loopback = host.eq_ignore_ascii_case("localhost")
        || ip_host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if !loopback {
        bail!("refusing non-loopback browser origin {value}");
    }
    normalize_origin(value)?;
    Ok(())
}

fn validate_browser_origin(value: &str, containerized: bool) -> Result<()> {
    if validate_loopback_origin(value).is_ok() {
        return Ok(());
    }
    if !containerized {
        return validate_loopback_origin(value);
    }

    let parsed = Url::parse(value).with_context(|| format!("invalid origin {value:?}"))?;
    if parsed.scheme() != "http" {
        bail!("container service origins must use http");
    }
    normalize_origin(value)?;
    let host = parsed.host_str().context("origin has no host")?;
    let ip_host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = ip_host.parse::<IpAddr>() {
        let private = match ip {
            IpAddr::V4(ip) => ip.is_private() || ip.is_link_local(),
            IpAddr::V6(ip) => ip.is_unique_local() || ip.is_unicast_link_local(),
        };
        if private {
            return Ok(());
        }
        bail!("refusing public container browser origin {value}");
    }

    let single_label = !host.contains('.')
        && !host.is_empty()
        && host.len() <= 63
        && !host.starts_with(['-', '_'])
        && !host.ends_with(['-', '_'])
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if !single_label {
        bail!("container service origin must use a private address or single-label service name");
    }
    Ok(())
}

fn validate_hosted_origin(value: &str) -> Result<()> {
    let parsed = Url::parse(value).with_context(|| format!("invalid hosted origin {value:?}"))?;
    if parsed.scheme() != "https" {
        bail!("hosted origin must use https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("hosted origin must not contain credentials");
    }
    normalize_origin(value)?;
    let host = parsed.host_str().context("hosted origin has no host")?;
    let ip_host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    if host.eq_ignore_ascii_case("localhost")
        || ip_host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    {
        bail!("hosted origin must not use a loopback host");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback_origins_only() {
        assert!(validate_loopback_origin("http://127.0.0.1:3100").is_ok());
        assert!(validate_loopback_origin("http://localhost:3100").is_ok());
        assert!(validate_loopback_origin("http://[::1]:3100").is_ok());
        assert!(validate_loopback_origin("https://localhost:3100").is_err());
        assert!(validate_loopback_origin("http://example.com:3100").is_err());
        assert!(validate_loopback_origin("http://localhost:3100/path").is_err());
    }

    #[test]
    fn container_mode_only_accepts_internal_service_origins() {
        assert!(validate_browser_origin("http://web:3000", true).is_ok());
        assert!(validate_browser_origin("http://10.0.0.8:3000", true).is_ok());
        assert!(validate_browser_origin("http://[fd00::8]:3000", true).is_ok());
        assert!(validate_browser_origin("http://example.com:3000", true).is_err());
        assert!(validate_browser_origin("http://8.8.8.8:3000", true).is_err());
        assert!(validate_browser_origin("https://web:3000", true).is_err());
        assert!(validate_browser_origin("http://web:3000", false).is_err());
    }

    #[test]
    fn hosted_origin_is_explicit_https_only() {
        assert!(validate_hosted_origin("https://cut.example.com").is_ok());
        assert!(validate_hosted_origin("http://cut.example.com").is_err());
        assert!(validate_hosted_origin("https://localhost:8443").is_err());
        assert!(validate_hosted_origin("https://cut.example.com/path").is_err());
    }
}
