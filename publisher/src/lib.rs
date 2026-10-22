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
        "/v1/current" | "/v1/history/day" | "/v1/history/month" => {
            let bytes = match path.as_str() {
                "/v1/current" => &snapshots.current,
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
    let mut tasks = vec![];
    for (kind, seconds) in [
        (0, config.refresh.current_seconds),
        (1, config.refresh.day_seconds),
        (2, config.refresh.month_seconds),
    ] {
        let upstream = upstream.clone();
        let snapshots = snapshots.clone();
        tasks.push(tokio::spawn(async move {
            let mut failures = 0u32;
            loop {
                let started = std::time::Instant::now();
                let result = match kind {
                    0 => upstream.current(now()).await,
                    _ => upstream.history(now(), kind == 2).await,
                };
                match result {
                    Ok(body) if body.len() <= 32 * 1024 * 1024 => {
                        let mut cache = snapshots.write().unwrap();
                        let slot = match kind {
                            0 => &mut cache.current,
                            1 => &mut cache.day,
                            _ => &mut cache.month,
                        };
                        *slot = Some(Bytes::from(body));
                        failures = 0;
                        tracing::info!(
                            snapshot = kind,
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "snapshot refreshed"
                        );
                    }
                    _ => {
                        failures = (failures + 1).min(6);
                        // Never log raw upstream errors, labels, URLs, or credentials.
                        tracing::warn!(
                            snapshot = kind,
                            failures,
                            "snapshot refresh failed; retaining previous bytes"
                        );
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
