//! Fetching a status page's publishers, mirroring the browser's requests.

use crate::status::{Observer, PublisherStatus, Summary, publisher_statuses, summarize};
use beacon_api::{Current, LocationConfig, LocationsFile};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::HashSet, fmt, time::Duration};
use url::Url;

/// Default page for clients that are not told otherwise.
pub const DEFAULT_STATUS_PAGE_URL: &str = "https://status.guzek.uk";

const TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_LOCATIONS: usize = 20;
const MAX_MIRRORS: usize = MAX_LOCATIONS - 1;
const MAX_SERVICES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The status page URL itself is not usable.
    InvalidUrl(String),
    /// A request failed or returned a non-success status.
    Request(String),
    /// A response was not a valid Beacon document.
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(message) => write!(f, "invalid status page URL: {message}"),
            Self::Request(message) => write!(f, "request failed: {message}"),
            Self::Invalid(message) => write!(f, "invalid response: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// Everything a desktop client shows for one status page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub title: String,
    pub page_url: String,
    /// Unix seconds at which the results were evaluated.
    pub checked_at: f64,
    pub summary: Summary,
    pub publishers: Vec<PublisherStatus>,
}

impl Snapshot {
    pub fn new(title: String, page_url: String, observers: &[Observer], now: f64) -> Self {
        Self {
            title,
            page_url,
            checked_at: now,
            summary: summarize(observers, now),
            publishers: publisher_statuses(observers, now),
        }
    }
}

/// `locations.json` next to the page, as the browser resolves it.
pub fn locations_url(page_url: &str) -> Result<Url, Error> {
    let mut url = Url::parse(page_url.trim()).map_err(|e| Error::InvalidUrl(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::InvalidUrl("only HTTP(S) URLs are supported".into()));
    }
    url.set_query(None);
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url.join("locations.json")
        .map_err(|e| Error::InvalidUrl(e.to_string()))
}

/// A publisher endpoint, joined the same way as the page's `endpoint()`.
pub fn endpoint(base: &str, path: &str) -> String {
    format!("{}{path}", base.strip_suffix('/').unwrap_or(base))
}

/// A location id usable as one URL path segment without escaping, as publishers require
/// for mirrored locations.
fn path_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._~-".contains(&c))
}

/// Where to request a location's `path` (such as `/current`), in order: its own
/// publisher's unprefixed endpoint, then each mirror's `/v1/locations/{id}` endpoint.
pub fn location_endpoints(config: &LocationConfig, path: &str) -> Vec<String> {
    let mut endpoints = vec![endpoint(&config.url, &format!("/v1{path}"))];
    if path_id(&config.id) {
        endpoints.extend(
            config
                .mirrors
                .iter()
                .map(|mirror| endpoint(mirror, &format!("/v1/locations/{}{path}", config.id))),
        );
    }
    endpoints
}

/// reqwest's own message omits the cause (DNS, TLS, timeout…); append it.
fn describe(error: reqwest::Error) -> Error {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        message = format!("{message}: {cause}");
        source = cause.source();
    }
    Error::Request(message)
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new() -> Result<Self, Error> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("beacon-client/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Request(e.to_string()))?;
        Ok(Self { http })
    }

    async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, Error> {
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(describe)?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(describe)? {
            if body.len() + chunk.len() > MAX_BYTES {
                return Err(Error::Invalid("response too large".into()));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|e| Error::Invalid(e.to_string()))
    }

    pub async fn locations(&self, page_url: &str) -> Result<LocationsFile, Error> {
        let file: LocationsFile = self.get(locations_url(page_url)?.as_str()).await?;
        validate_locations(&file)?;
        Ok(file)
    }

    /// The location's current snapshot from the first of its publishers that returns a
    /// valid one, or the last publisher's error.
    pub async fn current(&self, config: &LocationConfig) -> Result<Current, Error> {
        let mut last = None;
        for url in location_endpoints(config, "/current") {
            match self.get::<Current>(&url).await {
                Ok(current) => match validate_current(&current, config) {
                    Ok(()) => return Ok(current),
                    Err(error) => last = Some(error),
                },
                Err(error) => last = Some(error),
            }
        }
        Err(last.expect("at least the location's own publisher is tried"))
    }

    /// Requests every location in parallel. A failing location is reported
    /// as unreachable rather than failing the whole page.
    pub async fn observe(&self, locations: &[LocationConfig]) -> Vec<Observer> {
        futures_util::future::join_all(locations.iter().map(|config| async move {
            let current = self.current(config).await.ok();
            Observer {
                config: config.clone(),
                reachable: current.is_some(),
                current,
            }
        }))
        .await
    }

    /// Fetches `locations.json` and every publisher, then evaluates at `now`.
    pub async fn snapshot(&self, page_url: &str, now: f64) -> Result<Snapshot, Error> {
        let file = self.locations(page_url).await?;
        let observers = self.observe(&file.locations).await;
        Ok(Snapshot::new(file.title, page_url.into(), &observers, now))
    }
}

fn nonempty(value: &str) -> bool {
    !value.is_empty()
}

pub fn validate_locations(file: &LocationsFile) -> Result<(), Error> {
    let invalid = |message: &str| Err(Error::Invalid(format!("locations.json: {message}")));
    if file.locations.is_empty() || file.locations.len() > MAX_LOCATIONS {
        return invalid("expected 1–20 locations");
    }
    let mut ids = HashSet::new();
    for location in &file.locations {
        if !nonempty(&location.id) || !nonempty(&location.name) {
            return invalid("location id and name are required");
        }
        if !ids.insert(location.id.as_str()) {
            return invalid("duplicate observer IDs");
        }
        if location.mirrors.len() > MAX_MIRRORS {
            return invalid("at most 19 mirrors per location");
        }
        if !location.mirrors.is_empty() && !path_id(&location.id) {
            return invalid("mirrored location ids use only letters, digits, '.', '_', '~' or '-'");
        }
        for base in std::iter::once(&location.url).chain(&location.mirrors) {
            let url = Url::parse(base).map_err(|e| Error::Invalid(e.to_string()))?;
            if !matches!(url.scheme(), "http" | "https") {
                return invalid("only HTTP API URLs are supported");
            }
        }
    }
    Ok(())
}

pub fn validate_current(current: &Current, config: &LocationConfig) -> Result<(), Error> {
    let invalid = |message: &str| Err(Error::Invalid(format!("{}: {message}", config.id)));
    let time = |t: f64| t.is_finite() && t >= 0.0;
    if current.schema_version != 1 {
        return invalid("unsupported schema version");
    }
    if current.location.id != config.id {
        return invalid("observer identity mismatch");
    }
    if !time(current.generated_at) || !time(current.expires_at) {
        return invalid("invalid snapshot timestamps");
    }
    if current.services.len() > MAX_SERVICES {
        return invalid("too many services");
    }
    let mut ids = HashSet::new();
    for item in &current.services {
        let s = &item.service;
        if !nonempty(&s.id) || !nonempty(&s.name) {
            return invalid("service id and name are required");
        }
        if !ids.insert(s.id.as_str()) {
            return invalid("duplicate service IDs");
        }
        let times = [item.observed_at, item.expires_at, item.duration_seconds];
        if times.into_iter().flatten().any(|t| !time(t)) {
            return invalid("invalid service timestamps");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_url_is_relative_to_the_page() {
        let url = |page| locations_url(page).unwrap().to_string();
        assert_eq!(
            url("https://status.example.org"),
            "https://status.example.org/locations.json"
        );
        assert_eq!(
            url("https://example.org/status?x=1#top"),
            "https://example.org/status/locations.json"
        );
        assert_eq!(
            url("https://example.org/status/"),
            "https://example.org/status/locations.json"
        );
        assert!(locations_url("ftp://example.org").is_err());
        assert!(locations_url("not a url").is_err());
    }

    fn location(mirrors: &[&str]) -> LocationConfig {
        LocationConfig {
            id: "pi".into(),
            name: "Pi".into(),
            url: "https://beacon-pi.example.org".into(),
            mirrors: mirrors.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn location_endpoints_try_mirrors_after_the_own_publisher() {
        assert_eq!(
            location_endpoints(&location(&[]), "/current"),
            ["https://beacon-pi.example.org/v1/current"]
        );
        assert_eq!(
            location_endpoints(
                &location(&[
                    "https://beacon-box.example.org/",
                    "https://beacon-hp.example.org"
                ]),
                "/history/day"
            ),
            [
                "https://beacon-pi.example.org/v1/history/day",
                "https://beacon-box.example.org/v1/locations/pi/history/day",
                "https://beacon-hp.example.org/v1/locations/pi/history/day",
            ]
        );
        let mut unsafe_id = location(&["https://beacon-box.example.org"]);
        unsafe_id.id = "p i".into();
        assert_eq!(location_endpoints(&unsafe_id, "/current").len(), 1);
    }

    #[test]
    fn mirrors_are_validated() {
        let file = |location| LocationsFile {
            title: "Status".into(),
            locations: vec![location],
        };
        assert!(validate_locations(&file(location(&["https://beacon-box.example.org"]))).is_ok());
        assert!(validate_locations(&file(location(&["ftp://beacon-box.example.org"]))).is_err());
        let mut unsafe_id = location(&["https://beacon-box.example.org"]);
        unsafe_id.id = "p/i".into();
        assert!(validate_locations(&file(unsafe_id)).is_err());
        let many: Vec<String> = (0..20)
            .map(|i| format!("https://m{i}.example.org"))
            .collect();
        let mut too_many = location(&[]);
        too_many.mirrors = many;
        assert!(validate_locations(&file(too_many)).is_err());
    }

    #[test]
    fn endpoint_tolerates_trailing_slash() {
        assert_eq!(
            endpoint("https://a.org/", "/v1/current"),
            "https://a.org/v1/current"
        );
        assert_eq!(
            endpoint("https://a.org", "/v1/current"),
            "https://a.org/v1/current"
        );
    }
}
