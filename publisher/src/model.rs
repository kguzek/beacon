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
#[derive(Serialize)]
pub struct Bucket {
    pub ends_at: f64,
    pub observed_success_ratio: Option<f64>,
    pub coverage: f64,
    pub samples: u64,
    pub last_http_status_code: Option<u16>,
}
#[derive(Serialize)]
pub struct HistoryService {
    #[serde(flatten)]
    pub service: Service,
    pub observed_success_ratio: Option<f64>,
    pub coverage: f64,
    pub buckets: Vec<Bucket>,
}
#[derive(Serialize)]
pub struct History {
    pub schema_version: u8,
    pub location: Location,
    pub generated_at: f64,
    pub expires_at: f64,
    pub window_seconds: u64,
    pub bucket_seconds: u64,
    pub services: Vec<HistoryService>,
}
