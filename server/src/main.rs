use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use tributary::config::Config;
use tributary::sources::Registry;
use tributary::{Workspace, api, db};

#[derive(Parser)]
#[command(about = "Tributary: a unified task dashboard server")]
struct Args {
    /// Path to the configuration file.
    #[arg(long, short, default_value = "tributary.toml")]
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
    let app = api::router(workspace.clone());

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("failed to listen on {listen}"))?;
    info!("listening on http://{listen}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            info!("shutting down");
            workspace.shutdown();
        })
        .await?;
    Ok(())
}

/// Resolves on Ctrl-C or SIGTERM. In a container the server is PID 1, which
/// ignores SIGTERM (sent by `docker stop`) unless it is handled.
async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to listen for SIGTERM");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}
