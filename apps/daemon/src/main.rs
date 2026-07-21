use anyhow::Result;
use clap::Parser;
use openchatcut_daemon::{Config, config::Cli};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("openchatcut_daemon=info,tower_http=info")),
        )
        .with_target(false)
        .init();
    let config = Config::from_cli(Cli::parse())?;
    openchatcut_daemon::run(config).await
}
