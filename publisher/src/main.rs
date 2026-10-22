use anyhow::{Context, Result, bail};
use beacon_publisher::{Snapshots, StateData, config::Config, router, spawn_refreshes};
use std::{
    path::Path,
    sync::{Arc, RwLock},
};

mod logging;

#[tokio::main]
async fn main() -> Result<()> {
    logging::init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "starting Beacon publisher"
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = config_paths(&args)?;
    let (config, services) = Config::load_paths(&paths)?;
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

fn config_paths(args: &[String]) -> Result<Vec<&Path>> {
    if args.is_empty() {
        return Ok(vec![Path::new("/etc/beacon/config.toml")]);
    }
    let mut paths = Vec::new();
    for pair in args.chunks(2) {
        match pair {
            [option, path] if option == "--config.path" && !path.starts_with("--") => {
                paths.push(Path::new(path));
            }
            _ => bail!("usage: beacon-publisher [--config.path /path/config.toml]..."),
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_arguments() {
        assert_eq!(
            config_paths(&[]).unwrap(),
            vec![Path::new("/etc/beacon/config.toml")]
        );
        let args = ["--config.path", "base.toml", "--config.path", "local.toml"].map(String::from);
        assert_eq!(
            config_paths(&args).unwrap(),
            vec![Path::new("base.toml"), Path::new("local.toml")]
        );
        for args in [
            vec!["--config.path"],
            vec!["--unknown", "config.toml"],
            vec!["--config.path", "--config.path"],
            vec!["--config.path", "base.toml", "local.toml"],
        ] {
            assert!(config_paths(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }
}
