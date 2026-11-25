use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use dashboard::config::Config;
use dashboard::sources::Registry;
use dashboard::{Workspace, api, db};

#[derive(Parser)]
#[command(about = "Unified task dashboard server")]
struct Args {
    /// Path to the configuration file.
    #[arg(long, short, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let pool = db::connect(&config.server.database)
        .await
        .with_context(|| format!("failed to open {}", config.server.database.display()))?;
    let listen = config.server.listen;

    let workspace = Workspace::start(config, pool, Registry::builtin()).await?;
    let app = api::router(workspace);

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("failed to listen on {listen}"))?;
    info!("listening on http://{listen}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
