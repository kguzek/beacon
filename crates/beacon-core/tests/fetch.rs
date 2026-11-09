use axum::{Json, Router, http::StatusCode, routing::get};
use beacon_core::{Client, Error, HealthState, Overview};
use serde_json::{Value, json};

fn current(location: &str, now: f64, status: &str) -> Value {
    json!({
        "schema_version": 1,
        "location": { "id": location, "name": location },
        "generated_at": now,
        "expires_at": now + 120.0,
        "services": [
            { "id": "a.org", "name": "A", "hidden": false, "hosting_location": null,
              "labels": {}, "status": "healthy", "observed_at": now,
              "expires_at": now + 120.0, "duration_seconds": 0.1 },
            { "id": "b.org", "name": "B", "hidden": false, "hosting_location": null,
              "status": status, "observed_at": now,
              "expires_at": now + 120.0, "duration_seconds": null }
        ]
    })
}

/// Serves a status page under `/page/` with publishers `ok`, `down`, `dead`
/// (HTTP 503) and `liar` (reports another location's identity).
async fn serve(now: f64) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let entries: Vec<Value> = ["ok", "down", "dead", "liar"]
        .map(|id| json!({ "id": id, "name": id.to_uppercase(), "url": format!("{base}/{id}/") }))
        .into();
    let locations = json!({ "title": "Test status", "locations": entries });
    let app = Router::new()
        .route(
            "/page/locations.json",
            get(move || async move { Json(locations) }),
        )
        .route(
            "/bad/locations.json",
            get(|| async { Json(json!({ "locations": [] })) }),
        )
        .route(
            "/ok/v1/current",
            get(move || async move { Json(current("ok", now, "healthy")) }),
        )
        .route(
            "/down/v1/current",
            get(move || async move { Json(current("down", now, "down")) }),
        )
        .route(
            "/dead/v1/current",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        )
        .route(
            "/liar/v1/current",
            get(move || async move { Json(current("ok", now, "healthy")) }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

#[tokio::test]
async fn snapshot_combines_reachable_and_failing_publishers() {
    let now = beacon_core::now();
    let base = serve(now).await;
    let snapshot = Client::new()
        .unwrap()
        .snapshot(&format!("{base}/page"), now)
        .await
        .unwrap();

    assert_eq!(snapshot.title, "Test status");
    let reachable: Vec<_> = snapshot
        .publishers
        .iter()
        .map(|p| (p.id.as_str(), p.reachable))
        .collect();
    assert_eq!(
        reachable,
        [
            ("ok", true),
            ("down", true),
            ("dead", false),
            ("liar", false)
        ]
    );
    let ok = &snapshot.publishers[0];
    assert_eq!((ok.healthy, ok.down, ok.unknown), (2, 0, 0));
    let down = &snapshot.publishers[1];
    assert_eq!((down.healthy, down.down), (1, 1));
    assert_eq!(down.problems[0].name, "B");
    assert_eq!(snapshot.publishers[2].unknown, 2);

    // Reachable `ok` confirms everything while others don't: mixed.
    assert_eq!(snapshot.summary.overview, Overview::Mixed);
    assert_eq!(snapshot.summary.state, HealthState::Mixed);
    assert_eq!(snapshot.summary.counts.mixed, 2);
}

#[tokio::test]
async fn invalid_or_missing_locations_fail_the_page() {
    let base = serve(beacon_core::now()).await;
    let client = Client::new().unwrap();
    assert!(matches!(
        client.locations(&format!("{base}/bad")).await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        client.locations(&format!("{base}/missing")).await,
        Err(Error::Request(_))
    ));
    assert!(matches!(
        client.locations("mailto:someone@example.org").await,
        Err(Error::InvalidUrl(_))
    ));
}
