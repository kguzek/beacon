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
    collections::BTreeMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
};
use tracing::Instrument;

#[derive(Default)]
pub struct Snapshots {
    pub current: Option<Bytes>,
    pub hour: Option<Bytes>,
    pub day: Option<Bytes>,
    pub month: Option<Bytes>,
}
#[derive(Clone)]
pub struct StateData {
    /// The publisher's own location, also served at the unprefixed `/v1/` paths.
    pub snapshots: Arc<RwLock<Snapshots>>,
    pub cache_seconds: u64,
    pub locations: Arc<Locations>,
}
/// Every location served under `/v1/locations/`.
#[derive(Default)]
pub struct Locations {
    /// The serialized `/v1/locations` body; absent means the routes are not served.
    pub index: Option<Bytes>,
    /// Snapshots by location id, the own location included.
    pub by_id: BTreeMap<String, Arc<RwLock<Snapshots>>>,
}
impl StateData {
    pub fn new(config: &Config) -> Self {
        let snapshots = Arc::new(RwLock::new(Snapshots::default()));
        let mut by_id = BTreeMap::new();
        if config::path_id(&config.location.id) {
            by_id.insert(config.location.id.clone(), snapshots.clone());
        }
        for mirror in &config.mirrors {
            by_id.insert(mirror.id.clone(), Arc::default());
        }
        let index = beacon_api::LocationIndex {
            schema_version: 1,
            location: config.location.clone().into(),
            mirrors: config
                .mirrors
                .iter()
                .map(|m| beacon_api::Location {
                    id: m.id.clone(),
                    name: m.name.clone(),
                })
                .collect(),
        };
        Self {
            snapshots,
            cache_seconds: config.server.cache_seconds,
            locations: Arc::new(Locations {
                index: Some(Bytes::from(serde_json::to_vec(&index).unwrap())),
                by_id,
            }),
        }
    }
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
    match path.as_str() {
        "/healthz" => error(StatusCode::OK, r#"{"status":"ok"}"#),
        // Initial current availability latches readiness; later upstream failures do not undo it.
        "/readyz" if state.snapshots.read().unwrap().current.is_some() => {
            error(StatusCode::OK, r#"{"status":"ready"}"#)
        }
        "/readyz" => error(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"initial_snapshot_unavailable"}"#,
        ),
        "/v1/locations" => match &state.locations.index {
            Some(index) => cached(&state, index.clone()),
            None => error(StatusCode::NOT_FOUND, r#"{"error":"not_found"}"#),
        },
        _ => {
            let (snapshots, snapshot) = if let Some(rest) = path.strip_prefix("/v1/locations/") {
                let Some((id, snapshot)) = rest.split_once('/') else {
                    return error(StatusCode::NOT_FOUND, r#"{"error":"not_found"}"#);
                };
                match state.locations.by_id.get(id) {
                    Some(snapshots) => (snapshots, snapshot),
                    None => {
                        return error(StatusCode::NOT_FOUND, r#"{"error":"location_not_found"}"#);
                    }
                }
            } else if let Some(snapshot) = path.strip_prefix("/v1/") {
                (&state.snapshots, snapshot)
            } else {
                return error(StatusCode::NOT_FOUND, r#"{"error":"not_found"}"#);
            };
            let snapshots = snapshots.read().unwrap();
            let bytes = match snapshot {
                "current" => &snapshots.current,
                "history/hour" => &snapshots.hour,
                "history/day" => &snapshots.day,
                "history/month" => &snapshots.month,
                _ => return error(StatusCode::NOT_FOUND, r#"{"error":"not_found"}"#),
            };
            match bytes {
                Some(bytes) => cached(&state, bytes.clone()),
                None => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    r#"{"error":"snapshot_unavailable"}"#,
                ),
            }
        }
    }
}
fn cached(state: &StateData, body: Bytes) -> Response {
    Response::builder()
        .header("content-type", "application/json")
        .header(
            "cache-control",
            format!("public, max-age={}, must-revalidate", state.cache_seconds),
        )
        .header("x-content-type-options", "nosniff")
        .body(Body::from(body))
        .unwrap()
}
/// Starts the refresh loops for the own location and every mirror, plus one
/// inventory reload shared by all of them.
pub fn spawn_refreshes(
    config: Config,
    services: Vec<Service>,
    state: &StateData,
) -> anyhow::Result<Vec<tokio::task::JoinHandle<()>>> {
    let mut locations = vec![(config.clone(), state.snapshots.clone(), false)];
    for mirror in &config.mirrors {
        let snapshots = state.locations.by_id[&mirror.id].clone();
        locations.push((config.for_mirror(mirror), snapshots, true));
    }
    let mut upstreams = vec![];
    let mut tasks = vec![];
    for (config, snapshots, mirror) in locations {
        let upstream = Arc::new(prometheus::Prometheus::new(
            config.clone(),
            services.clone(),
        )?);
        upstreams.push(upstream.clone());
        let span = tracing::info_span!("location", id = %config.location.id);
        tasks.extend(spawn_location(config, upstream, snapshots, mirror, span));
    }
    tasks.push(spawn_inventory_reload(
        config.services.path.clone(),
        upstreams,
        Duration::from_secs(INVENTORY_POLL_SECONDS),
    ));
    Ok(tasks)
}
fn spawn_location(
    config: Config,
    upstream: Arc<prometheus::Prometheus>,
    snapshots: Arc<RwLock<Snapshots>>,
    mirror: bool,
    span: tracing::Span,
) -> Vec<tokio::task::JoinHandle<()>> {
    // Detected once per location and shared; current status never waits for it.
    let interval = Arc::new(tokio::sync::OnceCell::<u64>::new());
    let mut tasks = vec![];
    for snapshot in ["current", "hour", "day", "month"] {
        let upstream = upstream.clone();
        let snapshots = snapshots.clone();
        let interval = interval.clone();
        let refresh = config.refresh.clone();
        tasks.push(tokio::spawn(
            async move {
                let mut failures = 0u32;
                let mut health = RefreshHealth {
                    mirror,
                    ..Default::default()
                };
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
                            health.failed(
                                snapshot,
                                &error,
                                (seconds * 2u64.pow(failures)).min(86400),
                            );
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(
                        (seconds * 2u64.pow(failures)).min(86400),
                    ))
                    .await;
                }
            }
            .instrument(span.clone()),
        ));
    }
    tasks
}

const INVENTORY_POLL_SECONDS: u64 = 10;

/// Polls the inventory and swaps in valid changes; an invalid file keeps the previous inventory.
pub fn spawn_inventory_reload(
    path: String,
    upstreams: Vec<Arc<prometheus::Prometheus>>,
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
                    for upstream in &upstreams {
                        upstream.replace_services(services.clone());
                    }
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
    /// Readiness follows the own location only.
    mirror: bool,
}

impl RefreshHealth {
    fn succeeded(&mut self, snapshot: &str) {
        if self.failing {
            tracing::info!(snapshot, "snapshot refresh recovered");
        }
        if !self.available {
            tracing::info!(snapshot, "initial snapshot available");
            if snapshot == "current" && !self.mirror {
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
