//! Current-status logic shared by Beacon's desktop clients (`beacon-cli` and
//! the macOS companion): fetch a status page's publishers, apply the API's
//! freshness rules and combine results across locations.
//!
//! History is deliberately out of scope; the web page remains the place for it.

mod fetch;
mod status;

pub use beacon_api as api;
pub use fetch::{
    Client, DEFAULT_STATUS_PAGE_URL, Error, Snapshot, endpoint, locations_url, validate_current,
    validate_locations,
};
pub use status::{
    Counts, HealthState, Observer, Overview, PublisherStatus, ServiceProblem, ServiceState,
    Summary, combined_status, effective_status, publisher_statuses, summarize, visible_inventory,
};

/// The current time as Unix seconds.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}
