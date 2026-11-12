//! UniFFI surface over `beacon-core` for the macOS and Android companions.
//!
//! Types mirror `beacon-core` as plain records so the core crate stays free
//! of FFI concerns. All logic lives in `beacon-core`; Swift and Kotlin only render.

uniffi::setup_scaffolding!();

use beacon_core as core;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum HealthState {
    Healthy,
    Mixed,
    Down,
    Unknown,
}

impl From<core::HealthState> for HealthState {
    fn from(state: core::HealthState) -> Self {
        match state {
            core::HealthState::Healthy => Self::Healthy,
            core::HealthState::Mixed => Self::Mixed,
            core::HealthState::Down => Self::Down,
            core::HealthState::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Overview {
    AllOnline,
    Mixed,
    PartialOutage,
    AllDown,
    Incomplete,
    Unavailable,
}

impl From<core::Overview> for Overview {
    fn from(overview: core::Overview) -> Self {
        match overview {
            core::Overview::AllOnline => Self::AllOnline,
            core::Overview::Mixed => Self::Mixed,
            core::Overview::PartialOutage => Self::PartialOutage,
            core::Overview::AllDown => Self::AllDown,
            core::Overview::Incomplete => Self::Incomplete,
            core::Overview::Unavailable => Self::Unavailable,
        }
    }
}

/// One location's own result for a service (no cross-location combining).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ServiceStatus {
    Healthy,
    Down,
    Unknown,
}

impl From<core::api::Status> for ServiceStatus {
    fn from(status: core::api::Status) -> Self {
        match status {
            core::api::Status::Healthy => Self::Healthy,
            core::api::Status::Down => Self::Down,
            core::api::Status::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Counts {
    pub healthy: u32,
    pub mixed: u32,
    pub down: u32,
    pub unknown: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ServiceState {
    pub id: String,
    pub name: String,
    pub state: HealthState,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ServiceProblem {
    pub id: String,
    pub name: String,
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PublisherStatus {
    pub id: String,
    pub name: String,
    pub url: String,
    pub reachable: bool,
    pub generated_at: Option<f64>,
    pub healthy: u32,
    pub down: u32,
    pub unknown: u32,
    pub problems: Vec<ServiceProblem>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Snapshot {
    pub title: String,
    pub page_url: String,
    pub checked_at: f64,
    pub overview: Overview,
    pub state: HealthState,
    pub headline: String,
    pub breakdown: String,
    pub counts: Counts,
    pub services: Vec<ServiceState>,
    pub publishers: Vec<PublisherStatus>,
}

impl From<core::Snapshot> for Snapshot {
    fn from(snapshot: core::Snapshot) -> Self {
        let summary = snapshot.summary;
        Self {
            title: snapshot.title,
            page_url: snapshot.page_url,
            checked_at: snapshot.checked_at,
            overview: summary.overview.into(),
            state: summary.state.into(),
            headline: summary.headline,
            breakdown: summary.breakdown,
            counts: Counts {
                healthy: summary.counts.healthy,
                mixed: summary.counts.mixed,
                down: summary.counts.down,
                unknown: summary.counts.unknown,
            },
            services: summary
                .services
                .into_iter()
                .map(|s| ServiceState {
                    id: s.id,
                    name: s.name,
                    state: s.state.into(),
                })
                .collect(),
            publishers: snapshot
                .publishers
                .into_iter()
                .map(|p| PublisherStatus {
                    id: p.id,
                    name: p.name,
                    url: p.url,
                    reachable: p.reachable,
                    generated_at: p.generated_at,
                    healthy: p.healthy,
                    down: p.down,
                    unknown: p.unknown,
                    problems: p
                        .problems
                        .into_iter()
                        .map(|problem| ServiceProblem {
                            id: problem.id,
                            name: problem.name,
                            status: problem.status.into(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// Flat, so each case carries just its `Display` message. A `message` field
/// would clash with `Throwable.message` in the Kotlin bindings.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
#[uniffi(flat_error)]
pub enum FetchError {
    InvalidUrl { message: String },
    Request { message: String },
    Invalid { message: String },
}

impl From<core::Error> for FetchError {
    fn from(error: core::Error) -> Self {
        let message = error.to_string();
        match error {
            core::Error::InvalidUrl(_) => Self::InvalidUrl { message },
            core::Error::Request(_) => Self::Request { message },
            core::Error::Invalid(_) => Self::Invalid { message },
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::InvalidUrl { message } | Self::Request { message } | Self::Invalid { message }) =
            self;
        f.write_str(message)
    }
}

impl std::error::Error for FetchError {}

#[uniffi::export]
pub fn default_status_page_url() -> String {
    core::DEFAULT_STATUS_PAGE_URL.into()
}

/// Fetches the page's `locations.json` and every publisher's current status.
/// Blocks the calling thread; call it off the main thread.
#[uniffi::export]
pub fn fetch_status(page_url: String) -> Result<Snapshot, FetchError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| FetchError::Request {
            message: e.to_string(),
        })?;
    runtime.block_on(async {
        let client = core::Client::new()?;
        Ok(client.snapshot(&page_url, core::now()).await?.into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_urls_fail_without_network() {
        assert!(matches!(
            fetch_status("ftp://example.org".into()),
            Err(FetchError::InvalidUrl { .. })
        ));
    }
}
