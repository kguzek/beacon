use crate::config::{Location, Service};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Healthy,
    Down,
    Unknown,
}
#[derive(Serialize)]
pub struct CurrentService {
    #[serde(flatten)]
    pub service: Service,
    pub status: Status,
    pub observed_at: Option<f64>,
    pub expires_at: Option<f64>,
    pub duration_seconds: Option<f64>,
}
#[derive(Serialize)]
pub struct Current {
    pub schema_version: u8,
    pub location: Location,
    pub generated_at: f64,
    pub expires_at: f64,
    pub services: Vec<CurrentService>,
}
/// One real Blackbox probe: a single `probe_success` sample from one scrape.
#[derive(Serialize)]
pub struct Probe {
    pub at: f64,
    pub success: bool,
    pub http_status_code: Option<u16>,
}
/// A contiguous run of steps sharing one state.
#[derive(Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub state: Status,
    pub samples: u64,
    pub failures: u64,
    pub last_http_status_code: Option<u16>,
}
#[derive(Serialize)]
pub struct HistoryService<T> {
    #[serde(flatten)]
    pub service: Service,
    pub observed_success_ratio: Option<f64>,
    pub coverage: f64,
    #[serde(flatten)]
    pub data: T,
}
#[derive(Serialize)]
pub struct Probes {
    pub probes: Vec<Probe>,
}
#[derive(Serialize)]
pub struct Segments {
    pub segments: Vec<Segment>,
}
#[derive(Serialize)]
pub struct History<T> {
    pub schema_version: u8,
    pub kind: &'static str,
    pub location: Location,
    pub generated_at: f64,
    pub expires_at: f64,
    pub window_seconds: u64,
    pub scrape_interval_seconds: u64,
    pub refresh_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_seconds: Option<u64>,
    pub services: Vec<HistoryService<T>>,
}
