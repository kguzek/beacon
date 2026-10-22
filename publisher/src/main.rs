use anyhow::{Context, Result, bail};
use beacon_publisher::{Snapshots, StateData, config::Config, router, spawn_refreshes};
use std::{
    path::Path,
    sync::{Arc, RwLock},
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.as_slice() {
        [] => "/etc/beacon/config.toml",
        [option, path] if option == "--config.path" => path,
        _ => bail!("usage: beacon-publisher [--config.path /path/config.toml]"),
    };
    let (config, services) = Config::load(Path::new(path))?;
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
        })
        .await?;
    for task in tasks {
        task.abort();
    }
    Ok(())
}
