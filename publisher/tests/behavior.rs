use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use beacon_publisher::{
    Snapshots, StateData,
    config::{Config, inventory},
    prometheus::{Prometheus, fresh, parse},
    router, spawn_refreshes,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tower::ServiceExt;

fn yaml() -> &'static str {
    r#"
- targets: ["https://example.org/private?token=secret"]
  labels: {service: "Example", secret_label: "never-public", beacon_label_group: " Websites "}
- targets: ["https://hidden.example.org"]
  labels: {service: "Hidden", beacon_hidden: "true"}
- targets: ["https://private.example.org"]
  labels: {beacon_publish: "false"}
"#
}
fn config() -> Config {
    toml::from_str(include_str!("../../examples/config.toml")).unwrap()
}

#[test]
fn inventory_contract() {
    let services = inventory(yaml()).unwrap();
    assert_eq!(services.len(), 2);
    assert_eq!(services[0].id, "example.org");
    assert_eq!(services[0].labels.get("group").unwrap(), "Websites");
    assert!(services[1].hidden);
    let public = serde_json::to_string(&services).unwrap();
    for private in [
        "token",
        "secret",
        "https://",
        "private.example",
        "secret_label",
    ] {
        assert!(!public.contains(private));
    }
    for invalid in [
        "[]",
        "- targets: [https://a.test]",
        "- targets: [https://a.test]\n  labels: {service: X, beacon_publish: yes}",
        "- targets: [ftp://a.test]\n  labels: {service: X}",
        "- targets: [https://a.test/x, https://a.test/y]\n  labels: {service: X}",
    ] {
        assert!(inventory(invalid).is_err(), "{invalid}");
    }
    assert_eq!(
        inventory("- targets: [https://EXAMPLE.org]\n  labels: {service: X}").unwrap()[0].id,
        "example.org"
    );
}

#[test]
fn public_inventory_labels() {
    let valid = inventory("- targets: [https://a.test, https://b.test]\n  labels: {service: X, beacon_label_group: ' Observability ', beacon_label_team: 'Ops'}").unwrap();
    assert_eq!(valid[0].labels, valid[1].labels);
    assert_eq!(valid[0].labels["group"], "Observability");
    for labels in [
        "beacon_label_group: ''".to_owned(),
        "beacon_label_group: '   '".to_owned(),
        "beacon_label_Group: X".to_owned(),
        "beacon_label_: X".to_owned(),
        "beacon_label_1group: X".to_owned(),
        "beacon_label_bad-key: X".to_owned(),
        format!("beacon_label_{}: X", "a".repeat(33)),
        format!("beacon_label_group: '{}'", "é".repeat(101)),
        r#"beacon_label_group: "a\nb""#.to_owned(),
        "'beacon_label_group ': X, beacon_label_group: Y".to_owned(),
        (0..17)
            .map(|i| format!("beacon_label_key{i}: X"))
            .collect::<Vec<_>>()
            .join(", "),
    ] {
        let yaml = format!("- targets: [https://a.test]\n  labels: {{service: X, {labels}}}");
        assert!(inventory(&yaml).is_err(), "{yaml}");
    }
    let boundary = inventory(&format!(
        "- targets: [https://a.test]\n  labels: {{service: X, beacon_label_{}: '{}'}}",
        "a".repeat(32),
        "é".repeat(100)
    ))
    .unwrap();
    assert_eq!(boundary[0].labels.len(), 1);
}

#[test]
fn startup_only_and_required_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    assert!(Config::load(&path).is_err());
    let services = dir.path().join("services.yaml");
    let toml = include_str!("../../examples/config.toml")
        .replace("/etc/beacon/services.yaml", services.to_str().unwrap());
    std::fs::write(&path, &toml).unwrap();
    assert!(Config::load(&path).is_err());
    std::fs::write(&services, yaml()).unwrap();
    let (loaded, inventory) = Config::load(&path).unwrap();
    std::fs::write(&services, "invalid").unwrap();
    std::fs::write(&path, "invalid").unwrap();
    assert_eq!(inventory.len(), 2);
    assert_eq!(loaded.location.id, "primary");
    assert!(Config::load(&path).is_err());
    assert!(Config::load(Path::new("/nonexistent/beacon.toml")).is_err());
    let mut invalid = config();
    invalid.refresh.day_bucket_seconds = 1;
    assert!(invalid.validate().is_err());
}

#[test]
fn layered_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.toml");
    let local = dir.path().join("local.toml");
    let last = dir.path().join("last.toml");
    let services = dir.path().join("services.yaml");
    std::fs::write(&services, yaml()).unwrap();
    // The base is incomplete and contains values that only become valid after merging.
    std::fs::write(
        &base,
        r#"
[location]
id = "primary"
[prometheus]
url = "not a URL"
job = "blackbox_http"
[services]
path = "/nonexistent/services.yaml"
[server]
cache_seconds = 100
allowed_origins = ["https://old.example.org"]
[refresh]
current_seconds = 42
"#,
    )
    .unwrap();
    std::fs::write(
        &local,
        format!(
            r#"
[location]
name = "Local"
[prometheus]
url = "http://localhost:9090"
[services]
path = {services:?}
[server]
cache_seconds = 20
allowed_origins = ["https://new.example.org"]
[refresh]
day_seconds = 600
"#,
        ),
    )
    .unwrap();
    std::fs::write(&last, "[server]\ncache_seconds = 5\nallowed_origins = []").unwrap();
    assert!(Config::load(&base).is_err());
    assert!(Config::load(&local).is_err());
    let (merged, inventory) = Config::load_paths(&[&base, &local]).unwrap();
    assert_eq!(merged.location.id, "primary");
    assert_eq!(merged.location.name, "Local");
    assert_eq!(merged.prometheus.job, "blackbox_http");
    assert_eq!(merged.prometheus.timeout_seconds, 20);
    assert_eq!(merged.server.listen, "0.0.0.0:8080");
    assert_eq!(merged.server.allowed_origins, ["https://new.example.org"]);
    assert_eq!(merged.refresh.current_seconds, 42);
    assert_eq!(merged.refresh.day_seconds, 600);
    assert_eq!(inventory.len(), 2);
    let (merged, _) = Config::load_paths(&[&base, &local, &last]).unwrap();
    assert_eq!(merged.server.cache_seconds, 5);
    assert!(merged.server.allowed_origins.is_empty());
    assert!(Config::load_paths(&[&local, &base]).is_err());

    for invalid in [
        "not valid TOML",
        "unknown_field = true",
        "[server]\ncache_seconds = 100",
        "[server]\ncache_seconds = 'wrong type'",
        "[services]\npath = '/nonexistent/inventory.yaml'",
    ] {
        std::fs::write(&last, invalid).unwrap();
        assert!(Config::load_paths(&[&base, &local, &last]).is_err());
    }
    assert!(Config::load_paths(&[&base, &local, &dir.path().join("missing.toml")]).is_err());
    assert!(Config::load_paths(&[] as &[&Path]).is_err());
}

#[test]
fn upstream_validation_and_freshness() {
    let services = inventory(yaml()).unwrap();
    let series =
        json!({"metric":{"instance":services[0].target,"sensitive":"secret"},"value":[100,"1"]});
    let encode = |result: Value| {
        serde_json::to_vec(
            &json!({"status":"success","data":{"resultType":"vector","result":result}}),
        )
        .unwrap()
    };
    assert_eq!(
        parse(&encode(json!([series.clone()])), false, &services)
            .unwrap()
            .len(),
        1
    );
    assert!(parse(&encode(json!([series.clone(), series])), false, &services).is_err());
    assert!(parse(br#"{"status":"error","error":"private"}"#, false, &services).is_err());
    assert!(parse(&encode(json!([])), true, &services).is_err());
    assert!(fresh(99.0, 100.0, 10.0));
    assert!(!fresh(50.0, 100.0, 10.0));
    assert!(!fresh(110.0, 100.0, 10.0));
}

#[test]
fn historical_series_survive_grouping_label_changes() {
    let services = inventory(yaml()).unwrap();
    let old = json!({
        "metric": {"instance": services[0].target},
        "values": [[30, "1"], [60, "1"]]
    });
    let new = json!({
        "metric": {"instance": services[0].target, "beacon_label_group": "Websites"},
        "values": [[60, "1"], [90, "0"]]
    });
    let encode = |result: Value| {
        serde_json::to_vec(
            &json!({"status": "success", "data": {"resultType": "matrix", "result": result}}),
        )
        .unwrap()
    };
    // Input order does not affect stitching, and the overlapping bucket is counted once.
    for series in [
        json!([old.clone(), new.clone()]),
        json!([new.clone(), old.clone()]),
    ] {
        let parsed = parse(&encode(series), true, &services).unwrap();
        assert_eq!(
            parsed[&services[0].target],
            [(30.0, 1.0), (60.0, 1.0), (90.0, 0.0)]
        );
    }
    let mut conflicting = new;
    conflicting["values"][0][1] = json!("0");
    assert!(parse(&encode(json!([old, conflicting])), true, &services).is_err());
}

#[derive(Clone)]
struct Mock {
    requests: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    mode: Arc<AtomicUsize>,
    fail_history: Arc<AtomicBool>,
}
async fn upstream(
    State(mock): State<Mock>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    mock.requests.fetch_add(1, Ordering::SeqCst);
    if mock.fail.load(Ordering::SeqCst)
        || (params.contains_key("step") && mock.fail_history.load(Ordering::SeqCst))
    {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"PRIVATE_ERROR"})),
        );
    }
    let query = &params["query"];
    let now: f64 = params
        .get("time")
        .or(params.get("end"))
        .unwrap()
        .parse()
        .unwrap();
    let mode = mock.mode.load(Ordering::SeqCst);
    if mode == 5 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let result = if params.contains_key("step") {
        let start: f64 = params["start"].parse().unwrap();
        let step: f64 = params["step"].parse().unwrap();
        let value = if query.starts_with("count_") {
            (step / 30.0 / 2.0).ceil().to_string()
        } else if query.starts_with("last_over_time(probe_http_status_code") {
            match mode {
                1 => "0",
                2 => "600",
                3 => "200.5",
                _ => "503",
            }
            .to_owned()
        } else {
            "0.5".to_owned()
        };
        let mut values = vec![];
        let mut at = start;
        while at <= now {
            values.push(json!([at, value]));
            at += step;
        }
        json!([{"metric":{"instance":"https://example.org/private?token=secret","private":"secret"},"values":values}])
    } else if mode == 4 {
        json!([])
    } else {
        let value = if query.starts_with("timestamp(") {
            (now - if mode == 2 { 1000.0 } else { 0.0 }).to_string()
        } else if query.starts_with("probe_duration") {
            "0.125".into()
        } else if (mode == 1 && query.starts_with("probe_success"))
            || (mode == 3 && query.starts_with("up"))
        {
            "0".into()
        } else {
            "1".into()
        };
        json!([{"metric":{"instance":"https://example.org/private?token=secret","private":"secret"},"value":[now,value]}])
    };
    (
        StatusCode::OK,
        Json(
            json!({"status":"success","data":{"resultType":if params.contains_key("step") {"matrix"} else {"vector"},"result":result}}),
        ),
    )
}
async fn mock() -> (Config, Mock, tokio::task::JoinHandle<()>) {
    let mock = Mock {
        requests: Arc::default(),
        fail: Arc::default(),
        mode: Arc::default(),
        fail_history: Arc::default(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = config();
    config.prometheus.url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/api/v1/query", get(upstream))
        .route("/api/v1/query_range", get(upstream))
        .with_state(mock.clone());
    let task = tokio::spawn(async {
        axum::serve(listener, app).await.unwrap();
    });
    (config, mock, task)
}
#[tokio::test]
async fn current_missing_failed_stale_and_sanitized() {
    let (config, mock, task) = mock().await;
    let upstream = Prometheus::new(config, inventory(yaml()).unwrap()).unwrap();
    for (mode, expected) in [
        (0, "healthy"),
        (1, "down"),
        (2, "unknown"),
        (3, "unknown"),
        (4, "unknown"),
    ] {
        mock.mode.store(mode, Ordering::SeqCst);
        let bytes = upstream.current(10000.0).await.unwrap();
        let result: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result["services"][0]["status"], expected);
        assert_eq!(
            result["services"][0]["labels"],
            json!({"group": "Websites"})
        );
        assert_eq!(result["services"][1]["status"], "unknown");
        assert_eq!(result["services"].as_array().unwrap().len(), 2);
        if mode == 0 {
            assert_eq!(result["services"][0]["duration_seconds"], 0.125);
        }
        let text = String::from_utf8(bytes).unwrap();
        for private in ["secret", "https://", "private.example", "PRIVATE_ERROR"] {
            assert!(!text.contains(private));
        }
    }
    mock.fail.store(true, Ordering::SeqCst);
    assert!(upstream.current(10000.0).await.is_err());
    task.abort();
}
#[tokio::test]
async fn history_coverage_and_fixed_buckets() {
    let (config, mock, task) = mock().await;
    let upstream = Prometheus::new(config, inventory(yaml()).unwrap()).unwrap();
    for (period, buckets) in [("hour", 120), ("day", 96), ("month", 120)] {
        let value: Value =
            serde_json::from_slice(&upstream.history(3000000.0, period).await.unwrap()).unwrap();
        assert_eq!(
            value["services"][0]["coverage"],
            if period == "hour" { 1.0 } else { 0.5 }
        );
        if period == "hour" {
            assert_eq!(value["window_seconds"], 3600);
            assert_eq!(value["bucket_seconds"], 30);
        }
        assert_eq!(value["services"][0]["observed_success_ratio"], 0.5);
        assert_eq!(value["services"][0]["labels"], json!({"group": "Websites"}));
        assert_eq!(
            value["services"][0]["buckets"].as_array().unwrap().len(),
            buckets
        );
        assert_eq!(value["services"][1]["coverage"], 0.0);
        assert!(value["services"][1]["observed_success_ratio"].is_null());
        assert_eq!(
            value["services"][0]["buckets"][0]["last_http_status_code"],
            503
        );
        assert!(value["services"][1]["buckets"][0]["last_http_status_code"].is_null());
    }
    for mode in [1, 2, 3] {
        mock.mode.store(mode, Ordering::SeqCst);
        let value: Value =
            serde_json::from_slice(&upstream.history(3000000.0, "day").await.unwrap()).unwrap();
        assert!(value["services"][0]["buckets"][0]["last_http_status_code"].is_null());
    }
    task.abort();
}
#[tokio::test]
async fn cached_http_and_failure_retention() {
    let (mut config, mock, task) = mock().await;
    config.refresh.current_seconds = 1;
    let state = StateData {
        snapshots: Arc::new(RwLock::new(Snapshots::default())),
        cache_seconds: 15,
    };
    let app = router(state.clone(), &["https://status.example.org".into()]);
    for (path, method, status) in [
        ("/healthz", "GET", 200),
        ("/readyz", "GET", 503),
        ("/v1/current", "GET", 503),
        ("/v1/history/hour", "GET", 503),
        ("/v1/current?query=secret", "GET", 400),
        ("/healthz?", "GET", 400),
        ("/v1/current", "POST", 405),
        ("/v1/current", "HEAD", 405),
        ("/query", "GET", 404),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
    }
    assert_eq!(mock.requests.load(Ordering::SeqCst), 0);
    let tasks =
        spawn_refreshes(config, inventory(yaml()).unwrap(), state.snapshots.clone()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while {
            let cache = state.snapshots.read().unwrap();
            cache.current.is_none() || cache.day.is_none() || cache.month.is_none()
        } {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let before = state.snapshots.read().unwrap().current.clone().unwrap();
    mock.fail.store(true, Ordering::SeqCst);
    let count = mock.requests.load(Ordering::SeqCst);
    let requests = (0..100).map(|_| async {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/current")
                    .header("origin", "https://status.example.org")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["access-control-allow-origin"],
            "https://status.example.org"
        );
    });
    futures_util::future::join_all(requests).await;
    assert_eq!(mock.requests.load(Ordering::SeqCst), count);
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    assert_eq!(
        state.snapshots.read().unwrap().current.as_ref().unwrap(),
        &before
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for task in tasks {
        task.abort();
    }
    task.abort();
}

#[tokio::test]
async fn history_failure_does_not_block_current_and_upstream_limits_apply() {
    let (mut config, mock, task) = mock().await;
    mock.fail_history.store(true, Ordering::SeqCst);
    let upstream = Prometheus::new(config.clone(), inventory(yaml()).unwrap()).unwrap();
    assert!(upstream.history(100000.0, "day").await.is_err());
    assert!(upstream.current(100000.0).await.is_ok());
    config.prometheus.max_response_bytes = 32;
    let limited = Prometheus::new(config.clone(), inventory(yaml()).unwrap()).unwrap();
    assert!(limited.current(100000.0).await.is_err());
    config.prometheus.max_response_bytes = 1024 * 1024;
    config.prometheus.timeout_seconds = 1;
    mock.mode.store(5, Ordering::SeqCst);
    let timed = Prometheus::new(config, inventory(yaml()).unwrap()).unwrap();
    let started = std::time::Instant::now();
    assert!(timed.current(100000.0).await.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    task.abort();
}
