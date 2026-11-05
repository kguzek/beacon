//! Wire types for the Beacon publisher HTTP API (`/v1/*`) and the status
//! page's `locations.json`. See `docs/API.md` for semantics.
//!
//! Fields that older publishers may omit deserialize with defaults, matching
//! the browser's tolerance.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A single observation's state. Consumers must still apply the freshness
/// rules: an expired `Healthy` or `Down` must be treated as `Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Healthy,
    Down,
    Unknown,
}

/// The monitoring location that produced a snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub id: String,
    pub name: String,
}

/// Public inventory metadata, shared by current and history responses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub hidden: bool,
    #[serde(default)]
    pub requires_vpn: bool,
    pub hosting_location: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// `GET /v1/current`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Current {
    pub schema_version: u8,
    pub location: Location,
    pub generated_at: f64,
    pub expires_at: f64,
    pub services: Vec<CurrentService>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrentService {
    #[serde(flatten)]
    pub service: Service,
    pub status: Status,
    pub observed_at: Option<f64>,
    pub expires_at: Option<f64>,
    pub duration_seconds: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    /// Hour: one entry per probe.
    Probes,
    /// Day and month: merged state segments.
    Timeline,
}

/// `GET /v1/history/{hour,day,month}`. `T` is [`Probes`] or [`Segments`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct History<T> {
    pub schema_version: u8,
    pub kind: HistoryKind,
    pub location: Location,
    pub generated_at: f64,
    pub expires_at: f64,
    pub window_seconds: u64,
    pub scrape_interval_seconds: u64,
    pub refresh_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_seconds: Option<u64>,
    pub services: Vec<HistoryService<T>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryService<T> {
    #[serde(flatten)]
    pub service: Service,
    pub observed_success_ratio: Option<f64>,
    pub coverage: f64,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probes {
    pub probes: Vec<Probe>,
}

/// One real Blackbox probe: a single `probe_success` sample from one scrape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub at: f64,
    pub success: bool,
    pub http_status_code: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segments {
    pub segments: Vec<Segment>,
}

/// A contiguous run of steps sharing one state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub state: Status,
    pub samples: u64,
    pub failures: u64,
    pub last_http_status_code: Option<u16>,
}

/// The status page's `locations.json`: which publishers to contact, in
/// priority order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocationsFile {
    #[serde(default = "default_title")]
    pub title: String,
    pub locations: Vec<LocationConfig>,
}

fn default_title() -> String {
    "Service status".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocationConfig {
    pub id: String,
    pub name: String,
    /// Publisher base URL; endpoints are resolved relative to it.
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) {
        let json = serde_json::to_string(value).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&json).unwrap(), value);
    }

    fn service() -> Service {
        Service {
            id: "example.org".into(),
            name: "Example".into(),
            hidden: false,
            requires_vpn: true,
            hosting_location: Some("primary".into()),
            labels: BTreeMap::from([("group".into(), "Websites".into())]),
        }
    }

    fn location() -> Location {
        Location {
            id: "hp".into(),
            name: "HP".into(),
        }
    }

    #[test]
    fn current_roundtrips() {
        roundtrip(&Current {
            schema_version: 1,
            location: location(),
            generated_at: 1.5,
            expires_at: 121.5,
            services: vec![CurrentService {
                service: service(),
                status: Status::Healthy,
                observed_at: Some(1.0),
                expires_at: Some(121.0),
                duration_seconds: None,
            }],
        });
    }

    #[test]
    fn histories_roundtrip() {
        roundtrip(&History {
            schema_version: 2,
            kind: HistoryKind::Probes,
            location: location(),
            generated_at: 0.0,
            expires_at: 45.0,
            window_seconds: 3600,
            scrape_interval_seconds: 15,
            refresh_seconds: 15,
            step_seconds: None,
            services: vec![HistoryService {
                service: service(),
                observed_success_ratio: Some(0.5),
                coverage: 1.0,
                data: Probes {
                    probes: vec![Probe {
                        at: 1.0,
                        success: false,
                        http_status_code: Some(503),
                    }],
                },
            }],
        });
        roundtrip(&History {
            schema_version: 2,
            kind: HistoryKind::Timeline,
            location: location(),
            generated_at: 0.0,
            expires_at: 900.0,
            window_seconds: 86400,
            scrape_interval_seconds: 15,
            refresh_seconds: 300,
            step_seconds: Some(60),
            services: vec![HistoryService {
                service: service(),
                observed_success_ratio: None,
                coverage: 0.0,
                data: Segments {
                    segments: vec![Segment {
                        start: 0.0,
                        end: 60.0,
                        state: Status::Unknown,
                        samples: 0,
                        failures: 0,
                        last_http_status_code: None,
                    }],
                },
            }],
        });
    }

    #[test]
    fn older_publishers_may_omit_optional_metadata() {
        let parsed: CurrentService = serde_json::from_value(json!({
            "id": "example.org", "name": "Example", "hidden": false,
            "hosting_location": null, "status": "down",
            "observed_at": null, "expires_at": null, "duration_seconds": null,
        }))
        .unwrap();
        assert!(!parsed.service.requires_vpn);
        assert!(parsed.service.labels.is_empty());
        assert_eq!(parsed.status, Status::Down);
    }

    #[test]
    fn locations_file_defaults_title() {
        let parsed: LocationsFile = serde_json::from_value(json!({
            "locations": [{ "id": "hp", "name": "HP", "url": "https://beacon-hp.example.org" }]
        }))
        .unwrap();
        assert_eq!(parsed.title, "Service status");
        roundtrip(&parsed);
    }

    #[test]
    fn example_locations_file_parses() {
        let parsed: LocationsFile =
            serde_json::from_str(include_str!("../../../locations.example.json")).unwrap();
        assert!(!parsed.locations.is_empty());
    }
}
