pub mod config;
pub mod model;
pub mod prometheus;

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
};
use bytes::Bytes;
use config::{Config, Service};
use std::{
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
};

#[derive(Default)]
pub struct Snapshots {
    pub current: Option<Bytes>,
    pub hour: Option<Bytes>,
    pub day: Option<Bytes>,
    pub month: Option<Bytes>,
}
#[derive(Clone)]
pub struct StateData {
    pub snapshots: Arc<RwLock<Snapshots>>,
    pub cache_seconds: u64,
}
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
pub fn router(state: StateData, origins: &[String]) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(
            origins.iter().map(|s| s.parse::<HeaderValue>().unwrap()),
        ))
        .allow_methods([Method::GET]);
    Router::new()
        .fallback(any(serve))
        .with_state(state)
        .layer(tower::limit::ConcurrencyLimitLayer::new(128))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(5),
        ))
        .layer(cors)
}
fn error(code: StatusCode, message: &'static str) -> Response {
    (
        code,
        [
            ("content-type", "application/json"),
            ("cache-control", "no-store"),
        ],
        message,
    )
        .into_response()
}
async fn serve(State(state): State<StateData>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            r#"{"error":"query_parameters_not_supported"}"#,
        );
    }
    if request.method() != Method::GET {
        return error(
            StatusCode::METHOD_NOT_ALLOWED,
            r#"{"error":"method_not_allowed"}"#,
        );
    }
    let path = request.uri().path().to_owned();
    if axum::body::to_bytes(request.into_body(), 0).await.is_err() {
        return error(
            StatusCode::BAD_REQUEST,
            r#"{"error":"request_body_not_supported"}"#,
        );
    }
    let snapshots = state.snapshots.read().unwrap();
    match path.as_str() {
        "/healthz" => error(StatusCode::OK, r#"{"status":"ok"}"#),
        // Initial current availability latches readiness; later upstream failures do not undo it.
        "/readyz" if snapshots.current.is_some() => error(StatusCode::OK, r#"{"status":"ready"}"#),
        "/readyz" => error(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"initial_snapshot_unavailable"}"#,
        ),
        "/v1/current" | "/v1/history/hour" | "/v1/history/day" | "/v1/history/month" => {
            let bytes = match path.as_str() {
                "/v1/current" => &snapshots.current,
                "/v1/history/hour" => &snapshots.hour,
                "/v1/history/day" => &snapshots.day,
                _ => &snapshots.month,
            };
            match bytes {
                Some(bytes) => Response::builder()
                    .header("content-type", "application/json")
                    .header(
                        "cache-control",
                        format!("public, max-age={}, must-revalidate", state.cache_seconds),
                    )
                    .header("x-content-type-options", "nosniff")
                    .body(Body::from(bytes.clone()))
                    .unwrap(),
                None => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    r#"{"error":"snapshot_unavailable"}"#,
                ),
            }
        }
        _ => error(StatusCode::NOT_FOUND, r#"{"error":"not_found"}"#),
    }
}
pub fn spawn_refreshes(
    config: Config,
    services: Vec<Service>,
    snapshots: Arc<RwLock<Snapshots>>,
) -> anyhow::Result<Vec<tokio::task::JoinHandle<()>>> {
    let upstream = Arc::new(prometheus::Prometheus::new(config.clone(), services)?);
    // Detected once and shared; current status never waits for it.
    let interval = Arc::new(tokio::sync::OnceCell::<u64>::new());
    let mut tasks = vec![spawn_inventory_reload(
        config.services.path.clone(),
        upstream.clone(),
        Duration::from_secs(INVENTORY_POLL_SECONDS),
    )];
    for snapshot in ["current", "hour", "day", "month"] {
        let upstream = upstream.clone();
        let snapshots = snapshots.clone();
        let interval = interval.clone();
        let refresh = config.refresh.clone();
        tasks.push(tokio::spawn(async move {
            let mut failures = 0u32;
            let mut health = RefreshHealth::default();
            loop {
                let mut seconds = match snapshot {
                    "current" => refresh.current_seconds,
                    "day" => refresh.day_seconds,
                    "month" => refresh.month_seconds,
                    _ => refresh.hour_seconds.unwrap_or(10),
                };
                let result = if snapshot == "current" {
                    upstream.current(now()).await
                } else {
                    match interval
                        .get_or_try_init(|| async {
                            let detected = upstream.detect_scrape_interval(now()).await?;
                            tracing::info!(
                                scrape_interval_seconds = detected,
                                configured = refresh.scrape_seconds.is_some(),
                                "scrape interval established"
                            );
                            anyhow::Ok(detected)
                        })
                        .await
                    {
                        Ok(&interval) => {
                            if snapshot == "hour" {
                                seconds = refresh.hour_interval(interval);
                            }
                            upstream.history(now(), snapshot, interval).await
                        }
                        Err(error) => Err(error),
                    }
                };
                let result = result.and_then(|body| {
                    anyhow::ensure!(
                        body.len() <= 32 * 1024 * 1024,
                        "snapshot exceeds size limit"
                    );
                    Ok(body)
                });
                match result {
                    Ok(body) => {
                        let mut cache = snapshots.write().unwrap();
                        let slot = match snapshot {
                            "current" => &mut cache.current,
                            "day" => &mut cache.day,
                            "month" => &mut cache.month,
                            _ => &mut cache.hour,
                        };
                        *slot = Some(Bytes::from(body));
                        failures = 0;
                        health.succeeded(snapshot);
                    }
                    Err(error) => {
                        failures = (failures + 1).min(6);
                        health.failed(snapshot, &error, (seconds * 2u64.pow(failures)).min(86400));
                    }
                }
                tokio::time::sleep(Duration::from_secs(
                    (seconds * 2u64.pow(failures)).min(86400),
                ))
                .await;
            }
        }));
    }
    Ok(tasks)
}

const INVENTORY_POLL_SECONDS: u64 = 10;

/// Polls the inventory and swaps in valid changes; an invalid file keeps the previous inventory.
pub fn spawn_inventory_reload(
    path: String,
    upstream: Arc<prometheus::Prometheus>,
    poll: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last = tokio::fs::read_to_string(&path).await.ok();
        let mut unreadable = false;
        loop {
            tokio::time::sleep(poll).await;
            let yaml = match tokio::fs::read_to_string(&path).await {
                Ok(yaml) => yaml,
                Err(_) => {
                    if !unreadable {
                        tracing::warn!(
                            "cannot read service inventory; keeping the previous inventory"
                        );
                    }
                    unreadable = true;
                    continue;
                }
            };
            unreadable = false;
            if last.as_deref() == Some(yaml.as_str()) {
                continue;
            }
            match config::inventory(&yaml) {
                Ok(services) => {
                    tracing::info!(services = services.len(), "service inventory reloaded");
                    upstream.replace_services(services);
                }
                Err(error) => {
                    tracing::warn!(%error, "invalid service inventory; keeping the previous inventory");
                }
            }
            last = Some(yaml);
        }
    })
}

/// Logs transitions only: repeated successes and failures are intentionally silent.
#[derive(Default)]
struct RefreshHealth {
    available: bool,
    failing: bool,
}

impl RefreshHealth {
    fn succeeded(&mut self, snapshot: &str) {
        if self.failing {
            tracing::info!(snapshot, "snapshot refresh recovered");
        }
        if !self.available {
            tracing::info!(snapshot, "initial snapshot available");
            if snapshot == "current" {
                tracing::info!("publisher ready");
            }
        }
        self.available = true;
        self.failing = false;
    }

    fn failed(&mut self, snapshot: &str, error: &anyhow::Error, retry_seconds: u64) {
        if !self.failing {
            // Display only our sanitized top-level error, never its raw source chain.
            tracing::warn!(
                snapshot,
                error = %error,
                previous_snapshot_available = self.available,
                retry_seconds,
                "snapshot refresh failing; retries use backoff; further failures suppressed until recovery"
            );
        }
        self.failing = true;
    }
}

#[cfg(test)]
mod logging_tests {
    use super::RefreshHealth;

    #[test]
    fn refresh_health_transitions() {
        let mut health = RefreshHealth::default();
        let error = anyhow::anyhow!("upstream request failed");
        assert!(!health.available && !health.failing);

        for _ in 0..2 {
            health.failed("current", &error, 60);
            assert!(!health.available && health.failing);
        }
        for _ in 0..2 {
            health.succeeded("current");
            assert!(health.available && !health.failing);
        }

        health.failed("current", &error, 60);
        assert!(health.available && health.failing);
        health.succeeded("current");
        assert!(health.available && !health.failing);
    }
}
