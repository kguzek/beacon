use anyhow::{Context, Result, bail};
use beacon_publisher::{Snapshots, StateData, config::Config, router, spawn_refreshes};
use std::{
    io::{self, IsTerminal},
    path::Path,
    sync::{Arc, RwLock},
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .pretty()
        .with_target(false)
        .with_ansi(io::stderr().is_terminal())
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "starting Beacon publisher"
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.as_slice() {
        [] => "/etc/beacon/config.toml",
        [option, path] if option == "--config.path" => path,
        _ => bail!("usage: beacon-publisher [--config.path /path/config.toml]"),
    };
    let (config, services) = Config::load(Path::new(path))?;
    tracing::info!(
        location = %config.location.id,
        services = services.len(),
        allowed_origins = config.server.allowed_origins.len(),
        current_interval_seconds = config.refresh.current_seconds,
        day_interval_seconds = config.refresh.day_seconds,
        month_interval_seconds = config.refresh.month_seconds,
        "configuration loaded"
    );
    if config.server.allowed_origins.is_empty() {
        tracing::warn!(
            "no allowed origins configured; cross-origin browser requests will be blocked"
        );
    }
    let listener = tokio::net::TcpListener::bind(&config.server.listen)
        .await
        .context("cannot bind HTTP listener")?;
    let snapshots = Arc::new(RwLock::new(Snapshots::default()));
    let state = StateData {
        snapshots: snapshots.clone(),
        cache_seconds: config.server.cache_seconds,
    };
    let app = router(state, &config.server.allowed_origins);
    let tasks = spawn_refreshes(config, services, snapshots)?;
    tracing::info!(address = %listener.local_addr()?, "HTTP server listening; waiting for initial snapshots");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let mut terminate =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .unwrap();
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            tracing::info!("shutdown requested; draining HTTP connections");
        })
        .await?;
    for task in tasks {
        task.abort();
    }
    tracing::info!("Beacon publisher stopped");
    Ok(())
}
