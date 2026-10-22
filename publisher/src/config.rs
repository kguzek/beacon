use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use url::Url;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub location: Location,
    pub prometheus: Prometheus,
    pub services: Services,
    #[serde(default)]
    pub server: Server,
    #[serde(default)]
    pub refresh: Refresh,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prometheus {
    pub url: String,
    pub job: String,
    pub bearer_token: Option<String>,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "max_bytes")]
    pub max_response_bytes: usize,
}
fn timeout() -> u64 {
    20
}
fn max_bytes() -> usize {
    16 * 1024 * 1024
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Services {
    pub path: String,
}
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    pub listen: String,
    pub allowed_origins: Vec<String>,
    pub cache_seconds: u64,
}
impl Default for Server {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8080".into(),
            allowed_origins: vec![],
            cache_seconds: 15,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Refresh {
    pub current_seconds: u64,
    pub hour_seconds: u64,
    pub day_seconds: u64,
    pub month_seconds: u64,
    pub stale_seconds: u64,
    pub scrape_seconds: u64,
    pub hour_bucket_seconds: u64,
    pub day_bucket_seconds: u64,
    pub month_bucket_seconds: u64,
}
impl Default for Refresh {
    fn default() -> Self {
        Self {
            current_seconds: 30,
            hour_seconds: 30,
            day_seconds: 300,
            month_seconds: 1800,
            stale_seconds: 120,
            scrape_seconds: 30,
            hour_bucket_seconds: 30,
            day_bucket_seconds: 900,
            month_bucket_seconds: 21600,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub hidden: bool,
    pub hosting_location: Option<String>,
    #[serde(skip)]
    pub target: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    targets: Vec<String>,
    #[serde(default)]
    labels: HashMap<String, String>,
}

pub fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
}
fn flag(labels: &HashMap<String, String>, key: &str, default: bool) -> Result<bool> {
    match labels.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => anyhow::bail!("{key} must be a string true or false"),
    }
}
pub fn inventory(yaml: &str) -> Result<Vec<Service>> {
    let groups: Vec<Group> = serde_yaml::from_str(yaml).context("invalid file_sd YAML")?;
    let mut result = vec![];
    let mut ids = HashSet::new();
    for group in groups {
        let publish = flag(&group.labels, "beacon_publish", true)?;
        let hidden = flag(&group.labels, "beacon_hidden", false)?;
        ensure!(!group.targets.is_empty(), "empty target group");
        for target in group.targets {
            let url = Url::parse(&target).context("invalid inventory target URL")?;
            ensure!(
                ["http", "https"].contains(&url.scheme()) && url.host_str().is_some(),
                "only HTTP targets supported"
            );
            if !publish {
                continue;
            }
            let id = url.host_str().unwrap().to_lowercase();
            ensure!(ids.insert(id.clone()), "duplicate published hostname: {id}");
            let name = group
                .labels
                .get("service")
                .context("published target missing service display name")?
                .clone();
            ensure!(text(&name), "invalid display name");
            let hosting_location = group.labels.get("beacon_hosting_location").cloned();
            ensure!(
                hosting_location.as_deref().is_none_or(text),
                "invalid hosting location"
            );
            result.push(Service {
                id,
                name,
                hidden,
                hosting_location,
                target,
            });
        }
    }
    ensure!(
        !result.is_empty() && result.len() <= 500,
        "inventory must contain 1–500 published services"
    );
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}
impl Config {
    pub fn load(path: &Path) -> Result<(Self, Vec<Service>)> {
        Self::load_paths(&[path])
    }

    pub fn load_paths(paths: &[impl AsRef<Path>]) -> Result<(Self, Vec<Service>)> {
        ensure!(!paths.is_empty(), "at least one config path is required");
        let mut merged = toml::Value::Table(Default::default());
        for path in paths {
            let path = path.as_ref();
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("cannot read publisher TOML: {}", path.display()))?;
            let value = toml::from_str(&raw)
                .with_context(|| format!("invalid publisher TOML: {}", path.display()))?;
            merge(&mut merged, value);
        }
        let config: Self = merged
            .try_into()
            .context("invalid merged publisher config")?;
        config.validate()?;
        let yaml = std::fs::read_to_string(&config.services.path)
            .context("cannot read service inventory")?;
        Ok((config, inventory(&yaml)?))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            text(&self.location.id) && text(&self.location.name),
            "invalid location"
        );
        let url = Url::parse(&self.prometheus.url).context("invalid Prometheus URL")?;
        ensure!(
            ["http", "https"].contains(&url.scheme())
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "invalid Prometheus URL; use bearer_token for credentials"
        );
        ensure!(text(&self.prometheus.job), "job required");
        ensure!(
            (1..=120).contains(&self.prometheus.timeout_seconds),
            "timeout outside 1–120 seconds"
        );
        ensure!(
            (1024..=64 * 1024 * 1024).contains(&self.prometheus.max_response_bytes),
            "invalid response limit"
        );
        self.server
            .listen
            .parse::<std::net::SocketAddr>()
            .context("invalid listen address")?;
        ensure!(
            self.server.cache_seconds <= 60,
            "cache_seconds must not exceed 60"
        );
        for origin in &self.server.allowed_origins {
            let url = Url::parse(origin).context("invalid CORS origin")?;
            ensure!(
                ["http", "https"].contains(&url.scheme())
                    && url.origin().ascii_serialization() == *origin,
                "CORS entries must be exact HTTP origins"
            );
        }
        let r = &self.refresh;
        ensure!(
            [
                r.current_seconds,
                r.hour_seconds,
                r.day_seconds,
                r.month_seconds
            ]
            .iter()
            .all(|s| (1..=86400).contains(s)),
            "invalid refresh interval"
        );
        ensure!(
            (1..=3600).contains(&r.scrape_seconds) && (1..=86400).contains(&r.stale_seconds),
            "invalid freshness/scrape interval"
        );
        for (window, bucket) in [
            (3600, r.hour_bucket_seconds),
            (86400, r.day_bucket_seconds),
            (2592000, r.month_bucket_seconds),
        ] {
            ensure!(
                bucket >= r.scrape_seconds
                    && bucket <= window
                    && window % bucket == 0
                    && window / bucket <= 1000,
                "invalid history bucket width"
            );
        }
        Ok(())
    }
}

fn merge(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}
