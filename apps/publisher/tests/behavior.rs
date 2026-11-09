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
    router, spawn_inventory_reload, spawn_refreshes,
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
    toml::from_str(include_str!("../../../examples/config.toml")).unwrap()
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
fn vpn_inventory_flag() {
    for (label, expected) in [
        ("", false),
        (", beacon_requires_vpn: 'false'", false),
        (", beacon_requires_vpn: 'true'", true),
    ] {
        let services = inventory(&format!(
            "- targets: [https://a.test, https://b.test]\n  labels: {{service: X{label}}}"
        ))
        .unwrap();
        assert_eq!(services.len(), 2);
        for service in services {
            assert_eq!(service.requires_vpn, expected);
            assert!(!service.hidden);
            assert_eq!(
                serde_json::to_value(&service).unwrap()["requires_vpn"],
                expected
            );
        }
    }
    for value in ["yes", "TRUE", "1", "''"] {
        assert!(inventory(&format!("- targets: [https://a.test]\n  labels: {{service: X, beacon_requires_vpn: {value}}}")).is_err());
    }
}

#[test]
fn startup_only_and_required_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    assert!(Config::load(&path).is_err());
    let services = dir.path().join("services.yaml");
    let toml = include_str!("../../../examples/config.toml")
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
    for step in [7, 0] {
        let mut invalid = config();
        invalid.refresh.day_step_seconds = step;
        assert!(invalid.validate().is_err());
    }
    let mut invalid = config();
    invalid.refresh.scrape_seconds = Some(120);
    assert!(
        invalid.validate().is_err(),
        "step shorter than scrape interval"
    );
    // Removed bucket settings fail loudly rather than being ignored.
    assert!(
        toml::from_str::<Config>(&format!(
            "{}\nhour_bucket_seconds = 30",
            include_str!("../../../examples/config.toml")
        ))
        .is_err()
    );
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
        parse(&encode(json!([series.clone()])), false, 1, &services)
            .unwrap()
            .len(),
        1
    );
    assert!(
        parse(
            &encode(json!([series.clone(), series])),
            false,
            1,
            &services
        )
        .is_err()
    );
    assert!(
        parse(
            br#"{"status":"error","error":"private"}"#,
            false,
            1,
            &services
        )
        .is_err()
    );
    assert!(parse(&encode(json!([])), true, 1, &services).is_err());
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
        let parsed = parse(&encode(series), true, 10, &services).unwrap();
        assert!(
            parse(
                &encode(json!([old.clone(), new.clone()])),
                true,
                2,
                &services
            )
            .is_err()
        );
        assert_eq!(
            parsed[&services[0].target],
            [(30.0, 1.0), (60.0, 1.0), (90.0, 0.0)]
        );
    }
    let mut conflicting = new;
    conflicting["values"][0][1] = json!("0");
    assert!(parse(&encode(json!([old, conflicting])), true, 10, &services).is_err());
}

/// serde_json's default float parsing is not exactly round-tripping.
fn close(value: &Value, expected: f64) {
    assert!(
        (value.as_f64().unwrap() - expected).abs() < 1e-12,
        "{value} != {expected}"
    );
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
    const TARGET: &str = "https://example.org/private?token=secret";
    let series = |values: Vec<Value>| json!([{"metric": {"instance": TARGET, "private": "secret"}, "values": values}]);
    let matrix = |result: Value| {
        (
            StatusCode::OK,
            Json(json!({"status": "success", "data": {"resultType": "matrix", "result": result}})),
        )
    };
    if params.contains_key("step") {
        let start: f64 = params["start"].parse().unwrap();
        let step: f64 = params["step"].parse().unwrap();
        let points = ((now - start) / step).round() as usize + 1;
        let per_step = (step / 15.0).to_string();
        if mode == 6 {
            // A metadata change within one step: old series has one success,
            // new series has two failures. Totals must give 3 samples, 2 failures.
            let value = if query.starts_with("sum by (instance) (count_over_time(") {
                "3"
            } else if query.starts_with("sum by (instance) (sum_over_time(") {
                "1"
            } else {
                // The last HTTP codes differ (200 and 503); comparison omits it.
                return matrix(json!([]));
            };
            return matrix(series(vec![json!([start, value])]));
        }
        // Steps 0..5 have no samples (unknown), step n-3 has one failure (down).
        let values = (0..points)
            .filter(|i| *i >= 5)
            .map(|i| {
                let at = start + i as f64 * step;
                let value = if query.starts_with("sum by (instance) (count_over_time(") {
                    per_step.clone()
                } else if query.starts_with("sum by (instance) (sum_over_time(") {
                    ((step / 15.0) - if i == points - 3 { 1.0 } else { 0.0 }).to_string()
                } else {
                    match mode {
                        1 => "0",
                        2 => "600",
                        3 => "200.5",
                        _ => "503",
                    }
                    .to_owned()
                };
                json!([at, value])
            })
            .collect();
        return matrix(series(values));
    }
    if let Some(window) = query
        .strip_suffix("s]")
        .and_then(|q| q.rsplit_once('['))
        .map(|(_, w)| w.parse::<f64>().unwrap())
    {
        // Raw samples: one scrape every 15 seconds, the second-to-last failing.
        if mode == 4 {
            return matrix(json!([]));
        }
        let count = (window / 15.0) as usize;
        let values = (0..count)
            .map(|i| {
                let at = now - (count - 1 - i) as f64 * 15.0;
                let failed = i == count - 2;
                let value = if query.starts_with("probe_http_status_code") {
                    match (mode, failed) {
                        (2, _) => "600",
                        (_, true) => "503",
                        _ => "200",
                    }
                } else if query.starts_with("probe_success") && failed {
                    "0"
                } else {
                    "1"
                };
                json!([at, value])
            })
            .collect();
        return matrix(series(values));
    }
    let result = if mode == 4 {
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
        json!([{"metric":{"instance":TARGET,"private":"secret"},"value":[now,value]}])
    };
    (
        StatusCode::OK,
        Json(json!({"status":"success","data":{"resultType":"vector","result":result}})),
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
async fn scrape_interval_detection() {
    let (mut config, mock, task) = mock().await;
    let upstream = Prometheus::new(config.clone(), inventory(yaml()).unwrap()).unwrap();
    assert_eq!(
        upstream.detect_scrape_interval(3000000.0).await.unwrap(),
        15
    );
    mock.mode.store(4, Ordering::SeqCst);
    let error = upstream
        .detect_scrape_interval(3000000.0)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("scrape_seconds"));
    config.refresh.scrape_seconds = Some(30);
    let requests = mock.requests.load(Ordering::SeqCst);
    let configured = Prometheus::new(config, inventory(yaml()).unwrap()).unwrap();
    assert_eq!(
        configured.detect_scrape_interval(3000000.0).await.unwrap(),
        30
    );
    assert_eq!(mock.requests.load(Ordering::SeqCst), requests);
    task.abort();
}

#[tokio::test]
async fn hour_history_lists_every_probe() {
    let (config, mock, task) = mock().await;
    let upstream = Prometheus::new(config, inventory(yaml()).unwrap()).unwrap();
    let now = 3000000.0;
    let value: Value =
        serde_json::from_slice(&upstream.history(now, "hour", 15).await.unwrap()).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["kind"], "probes");
    assert_eq!(value["window_seconds"], 3600);
    assert_eq!(value["scrape_interval_seconds"], 15);
    assert_eq!(value["refresh_seconds"], 15);
    assert!(value.get("step_seconds").is_none());
    let service = &value["services"][0];
    let probes = service["probes"].as_array().unwrap();
    assert_eq!(probes.len(), 240);
    assert_eq!(service["coverage"], 1.0);
    close(&service["observed_success_ratio"], 239.0 / 240.0);
    assert_eq!(probes[239]["at"], now);
    assert_eq!(probes[239]["success"], true);
    assert_eq!(probes[239]["http_status_code"], 200);
    assert_eq!(probes[238]["success"], false);
    assert_eq!(probes[238]["http_status_code"], 503);
    assert_eq!(value["services"][1]["probes"], json!([]));
    assert!(value["services"][1]["observed_success_ratio"].is_null());
    assert_eq!(value["services"][1]["coverage"], 0.0);
    // More raw samples than one per scrape interval are rejected.
    assert!(upstream.history(now, "hour", 60).await.is_err());
    mock.mode.store(2, Ordering::SeqCst);
    let value: Value =
        serde_json::from_slice(&upstream.history(now, "hour", 15).await.unwrap()).unwrap();
    assert!(value["services"][0]["probes"][238]["http_status_code"].is_null());
    task.abort();
}

#[tokio::test]
async fn day_and_month_history_are_state_timelines() {
    let (config, mock, task) = mock().await;
    let upstream = Prometheus::new(config, inventory(yaml()).unwrap()).unwrap();
    let now = 3000010.0;
    for (period, window, step) in [("day", 86400.0, 60.0), ("month", 2592000.0, 1800.0)] {
        let value: Value =
            serde_json::from_slice(&upstream.history(now, period, 15).await.unwrap()).unwrap();
        assert_eq!(value["kind"], "timeline");
        assert_eq!(value["step_seconds"], step);
        assert_eq!(value["services"][0]["labels"], json!({"group": "Websites"}));
        let end = (now / step).floor() * step;
        let segments = value["services"][0]["segments"].as_array().unwrap();
        let states: Vec<_> = segments
            .iter()
            .map(|s| s["state"].as_str().unwrap())
            .collect();
        assert_eq!(
            states,
            ["unknown", "healthy", "down", "healthy"],
            "{period}"
        );
        // Contiguous, covering the whole window.
        assert_eq!(segments[0]["start"], end - window);
        assert_eq!(segments[3]["end"], end);
        for pair in segments.windows(2) {
            assert_eq!(pair[0]["end"], pair[1]["start"]);
        }
        assert_eq!(
            segments[0]["end"].as_f64().unwrap() - segments[0]["start"].as_f64().unwrap(),
            5.0 * step
        );
        assert_eq!(segments[0]["samples"], 0);
        assert_eq!(
            segments[2]["end"].as_f64().unwrap() - segments[2]["start"].as_f64().unwrap(),
            step
        );
        assert_eq!(segments[2]["failures"], 1);
        assert_eq!(segments[2]["last_http_status_code"], 503);
        assert!(segments[1]["last_http_status_code"].is_null());
        assert_eq!(segments[3]["samples"], 2.0 * step / 15.0);
        let samples = (window / step - 5.0) * step / 15.0;
        close(
            &value["services"][0]["observed_success_ratio"],
            (samples - 1.0) / samples,
        );
        assert_eq!(value["services"][0]["coverage"], samples / (window / 15.0));
        let hidden = &value["services"][1];
        assert_eq!(hidden["segments"].as_array().unwrap().len(), 1);
        assert_eq!(hidden["segments"][0]["state"], "unknown");
        assert!(hidden["observed_success_ratio"].is_null());
    }
    for mode in [1, 2, 3] {
        mock.mode.store(mode, Ordering::SeqCst);
        let value: Value =
            serde_json::from_slice(&upstream.history(now, "day", 15).await.unwrap()).unwrap();
        assert!(value["services"][0]["segments"][2]["last_http_status_code"].is_null());
    }
    // A step must hold at least one scrape.
    assert!(upstream.history(now, "day", 120).await.is_err());
    task.abort();
}

#[tokio::test]
async fn history_aggregates_steps_crossing_metadata_changes() {
    let (config, mock, task) = mock().await;
    mock.mode.store(6, Ordering::SeqCst);
    let upstream = Prometheus::new(config, inventory(yaml()).unwrap()).unwrap();
    for period in ["day", "month"] {
        let value: Value =
            serde_json::from_slice(&upstream.history(3000000.0, period, 15).await.unwrap())
                .unwrap();
        let service = &value["services"][0];
        let segment = &service["segments"][0];
        assert_eq!(segment["state"], "down");
        assert_eq!(segment["samples"], 3);
        assert_eq!(segment["failures"], 2);
        assert!(segment["last_http_status_code"].is_null());
        assert_eq!(service["observed_success_ratio"], 1.0 / 3.0);
        assert_eq!(service["segments"][1]["state"], "unknown");
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
            cache.current.is_none()
                || cache.hour.is_none()
                || cache.day.is_none()
                || cache.month.is_none()
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
    assert!(upstream.history(100000.0, "day", 15).await.is_err());
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

#[tokio::test]
async fn inventory_reloads_valid_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("services.yaml");
    std::fs::write(&path, yaml()).unwrap();
    let upstream = Arc::new(Prometheus::new(config(), inventory(yaml()).unwrap()).unwrap());
    let poll = std::time::Duration::from_millis(20);
    let task = spawn_inventory_reload(path.to_str().unwrap().to_owned(), upstream.clone(), poll);
    let settle = || tokio::time::sleep(poll * 5);
    settle().await;
    assert_eq!(upstream.services().len(), 2);
    std::fs::write(
        &path,
        format!(
            "{}- targets: [\"https://new.example.org\"]\n  labels: {{service: \"New\"}}\n",
            yaml()
        ),
    )
    .unwrap();
    settle().await;
    let ids: Vec<_> = upstream.services().iter().map(|s| s.id.clone()).collect();
    assert_eq!(
        ids,
        ["example.org", "hidden.example.org", "new.example.org"]
    );
    for broken in [
        "invalid",
        "- targets: [https://a.test, https://a.test/x]\n  labels: {service: X}",
    ] {
        std::fs::write(&path, broken).unwrap();
        settle().await;
        assert_eq!(upstream.services().len(), 3, "{broken}");
    }
    std::fs::remove_file(&path).unwrap();
    settle().await;
    assert_eq!(upstream.services().len(), 3);
    std::fs::write(&path, yaml()).unwrap();
    settle().await;
    assert_eq!(upstream.services().len(), 2);
    task.abort();
}
