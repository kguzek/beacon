//! Pure status logic, ported from the status page (`apps/web/src/api.ts`).
//! Shared fixtures in `fixtures/current/` keep the two implementations in
//! agreement; change both together.

use beacon_api::{Current, CurrentService, LocationConfig, Service, Status};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Allowed clock skew, in seconds, before a timestamp counts as "in the future".
const FUTURE_SKEW: f64 = 5.0;

/// One monitoring location as seen by a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observer {
    pub config: LocationConfig,
    /// Whether the latest request to this publisher succeeded.
    pub reachable: bool,
    /// The most recent successful snapshot, which may outlive reachability.
    pub current: Option<Current>,
}

/// A service's state combined across every location.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Healthy,
    /// At least one reachable location confirms the service working, while
    /// another is unreachable or finds it down or unknown.
    Mixed,
    Down,
    Unknown,
}

impl HealthState {
    pub const ALL: [Self; 4] = [Self::Healthy, Self::Mixed, Self::Down, Self::Unknown];

    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "online",
            Self::Mixed => "mixed",
            Self::Down => "down",
            Self::Unknown => "unknown",
        }
    }
}

/// The page-level verdict; each variant has one headline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overview {
    AllOnline,
    Mixed,
    PartialOutage,
    AllDown,
    Incomplete,
    Unavailable,
}

impl Overview {
    pub fn headline(self) -> &'static str {
        match self {
            Self::AllOnline => "All services online",
            Self::Mixed => "Service status mixed",
            Self::PartialOutage => "Partial outage",
            Self::AllDown => "All services down",
            Self::Incomplete => "Service status incomplete",
            Self::Unavailable => "Service status unavailable",
        }
    }

    /// The single state that best represents this verdict, e.g. for an icon.
    pub fn state(self) -> HealthState {
        match self {
            Self::AllOnline => HealthState::Healthy,
            Self::Mixed => HealthState::Mixed,
            Self::PartialOutage | Self::AllDown => HealthState::Down,
            Self::Incomplete | Self::Unavailable => HealthState::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub healthy: u32,
    pub mixed: u32,
    pub down: u32,
    pub unknown: u32,
}

impl Counts {
    pub fn get(&self, state: HealthState) -> u32 {
        match state {
            HealthState::Healthy => self.healthy,
            HealthState::Mixed => self.mixed,
            HealthState::Down => self.down,
            HealthState::Unknown => self.unknown,
        }
    }

    fn add(&mut self, state: HealthState) {
        *match state {
            HealthState::Healthy => &mut self.healthy,
            HealthState::Mixed => &mut self.mixed,
            HealthState::Down => &mut self.down,
            HealthState::Unknown => &mut self.unknown,
        } += 1;
    }

    pub fn total(&self) -> u32 {
        self.healthy + self.mixed + self.down + self.unknown
    }

    /// Nonzero counts, e.g. `"12 online · 1 down"`.
    pub fn breakdown(&self) -> String {
        HealthState::ALL
            .iter()
            .filter(|&&state| self.get(state) > 0)
            .map(|&state| format!("{} {}", self.get(state), state.label()))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServiceState {
    pub id: String,
    pub name: String,
    pub state: HealthState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub overview: Overview,
    pub state: HealthState,
    pub headline: String,
    pub breakdown: String,
    pub counts: Counts,
    /// Visible services in display order.
    pub services: Vec<ServiceState>,
}

/// One location's own view, without combining across locations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublisherStatus {
    pub id: String,
    pub name: String,
    pub url: String,
    pub reachable: bool,
    /// When the publisher built the snapshot in use, if any.
    pub generated_at: Option<f64>,
    pub healthy: u32,
    pub down: u32,
    pub unknown: u32,
    /// Visible services this location does not currently confirm healthy.
    pub problems: Vec<ServiceProblem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServiceProblem {
    pub id: String,
    pub name: String,
    pub status: Status,
}

/// Applies the API's freshness rules: missing, expired or future-dated data
/// is unknown, whatever the cached status says.
pub fn effective_status(
    service: Option<&CurrentService>,
    snapshot: Option<&Current>,
    now: f64,
) -> Status {
    let (Some(service), Some(snapshot)) = (service, snapshot) else {
        return Status::Unknown;
    };
    let fresh = snapshot.generated_at <= now + FUTURE_SKEW
        && now < snapshot.expires_at
        && service
            .observed_at
            .is_some_and(|at| at <= now + FUTURE_SKEW)
        && service.expires_at.is_some_and(|at| now < at);
    if fresh {
        service.status
    } else {
        Status::Unknown
    }
}

fn observation<'a>(observer: &'a Observer, id: &str) -> Option<&'a CurrentService> {
    observer
        .current
        .as_ref()?
        .services
        .iter()
        .find(|service| service.service.id == id)
}

fn observed_status(observer: &Observer, id: &str, now: f64) -> Status {
    effective_status(observation(observer, id), observer.current.as_ref(), now)
}

/// Every service any location reports, minus those hidden at any location,
/// sorted by display name.
pub fn visible_inventory(observers: &[Observer]) -> Vec<Service> {
    let mut order = Vec::new();
    let mut inventory: HashMap<&str, Service> = HashMap::new();
    let mut hidden = HashSet::new();
    for item in observers
        .iter()
        .filter_map(|observer| observer.current.as_ref())
        .flat_map(|current| &current.services)
        .map(|service| &service.service)
    {
        if item.hidden {
            hidden.insert(item.id.as_str());
        }
        match inventory.get_mut(item.id.as_str()) {
            None => {
                order.push(item.id.as_str());
                inventory.insert(&item.id, item.clone());
            }
            Some(existing) => {
                existing.requires_vpn |= item.requires_vpn;
                if let (None, Some(group)) =
                    (existing.labels.get("group"), item.labels.get("group"))
                {
                    existing.labels.insert("group".into(), group.clone());
                }
            }
        }
    }
    let mut services: Vec<Service> = order
        .into_iter()
        .filter(|id| !hidden.contains(id))
        .filter_map(|id| inventory.remove(id))
        .collect();
    services.sort_by(|a, b| {
        (a.name.to_lowercase(), &a.name, &a.id).cmp(&(b.name.to_lowercase(), &b.name, &b.id))
    });
    services
}

pub fn combined_status(id: &str, observers: &[Observer], now: f64) -> HealthState {
    let confirmed: Vec<bool> = observers
        .iter()
        .map(|observer| observer.reachable && observed_status(observer, id, now) == Status::Healthy)
        .collect();
    if confirmed.iter().any(|&c| c) && !confirmed.iter().all(|&c| c) {
        return HealthState::Mixed;
    }
    let states: Vec<Status> = observers
        .iter()
        .map(|observer| observed_status(observer, id, now))
        .collect();
    if states.contains(&Status::Down) {
        HealthState::Down
    } else if !states.is_empty() && states.iter().all(|&s| s == Status::Healthy) {
        HealthState::Healthy
    } else {
        HealthState::Unknown
    }
}

/// Combines every location into a page-level summary of visible services.
pub fn summarize(observers: &[Observer], now: f64) -> Summary {
    let inventory = visible_inventory(observers);
    let mut counts = Counts::default();
    let services: Vec<ServiceState> = inventory
        .iter()
        .map(|service| {
            let state = combined_status(&service.id, observers, now);
            counts.add(state);
            ServiceState {
                id: service.id.clone(),
                name: service.name.clone(),
                state,
            }
        })
        .collect();
    let total = counts.total();
    // Scope-wide mixed: some location confirms every service working while
    // another is unreachable or reports something that isn't working.
    let all_healthy_from: Vec<bool> = observers
        .iter()
        .map(|observer| {
            observer.reachable
                && total > 0
                && inventory
                    .iter()
                    .all(|service| observed_status(observer, &service.id, now) == Status::Healthy)
        })
        .collect();
    let scope_mixed = all_healthy_from.iter().any(|&h| h) && !all_healthy_from.iter().all(|&h| h);
    let overview = if scope_mixed {
        Overview::Mixed
    } else if total == 0 || counts.unknown == total {
        Overview::Unavailable
    } else if counts.healthy == total {
        Overview::AllOnline
    } else if counts.down == total {
        Overview::AllDown
    } else if counts.down > 0 {
        Overview::PartialOutage
    } else if counts.mixed > 0 {
        Overview::Mixed
    } else {
        Overview::Incomplete
    };
    Summary {
        overview,
        state: overview.state(),
        headline: overview.headline().into(),
        breakdown: counts.breakdown(),
        counts,
        services,
    }
}

/// Each location's own results for the visible services, in config order.
pub fn publisher_statuses(observers: &[Observer], now: f64) -> Vec<PublisherStatus> {
    let inventory = visible_inventory(observers);
    observers
        .iter()
        .map(|observer| {
            let mut status = PublisherStatus {
                id: observer.config.id.clone(),
                name: observer.config.name.clone(),
                url: observer.config.url.clone(),
                reachable: observer.reachable,
                generated_at: observer.current.as_ref().map(|c| c.generated_at),
                healthy: 0,
                down: 0,
                unknown: 0,
                problems: vec![],
            };
            for service in &inventory {
                let state = observed_status(observer, &service.id, now);
                match state {
                    Status::Healthy => status.healthy += 1,
                    Status::Down => status.down += 1,
                    Status::Unknown => status.unknown += 1,
                }
                if state != Status::Healthy {
                    status.problems.push(ServiceProblem {
                        id: service.id.clone(),
                        name: service.name.clone(),
                        status: state,
                    });
                }
            }
            status
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn service(id: &str, status: Status, hidden: bool) -> CurrentService {
        CurrentService {
            service: Service {
                id: id.into(),
                name: id.to_uppercase(),
                hidden,
                requires_vpn: false,
                hosting_location: None,
                labels: BTreeMap::new(),
            },
            status,
            observed_at: Some(90.0),
            expires_at: Some(190.0),
            duration_seconds: None,
        }
    }

    fn observer(id: &str, services: Option<Vec<CurrentService>>) -> Observer {
        Observer {
            config: LocationConfig {
                id: id.into(),
                name: id.to_uppercase(),
                url: format!("https://{id}.example.org"),
            },
            reachable: services.is_some(),
            current: services.map(|services| Current {
                schema_version: 1,
                location: beacon_api::Location {
                    id: id.into(),
                    name: id.to_uppercase(),
                },
                generated_at: 100.0,
                expires_at: 200.0,
                services,
            }),
        }
    }

    #[test]
    fn freshness_boundaries() {
        let o = observer("a", Some(vec![service("x", Status::Healthy, false)]));
        let current = o.current.as_ref();
        let s = current.map(|c| &c.services[0]);
        assert_eq!(effective_status(s, current, 150.0), Status::Healthy);
        // Service observation expires at 190, before the snapshot.
        assert_eq!(effective_status(s, current, 190.0), Status::Unknown);
        // Generated more than five seconds in the future.
        assert_eq!(effective_status(s, current, 94.9), Status::Unknown);
        assert_eq!(effective_status(s, current, 95.0), Status::Healthy);
        assert_eq!(effective_status(None, current, 150.0), Status::Unknown);
    }

    #[test]
    fn publisher_statuses_count_each_location_separately() {
        let observers = [
            observer(
                "a",
                Some(vec![
                    service("x", Status::Healthy, false),
                    service("y", Status::Down, false),
                    service("h", Status::Down, true),
                ]),
            ),
            observer("b", None),
        ];
        let statuses = publisher_statuses(&observers, 150.0);
        assert_eq!(
            (statuses[0].healthy, statuses[0].down, statuses[0].unknown),
            (1, 1, 0)
        );
        assert_eq!(statuses[0].problems[0].id, "y");
        assert!(!statuses[1].reachable);
        assert_eq!(statuses[1].unknown, 2);
        assert_eq!(statuses[1].generated_at, None);
    }

    #[test]
    fn breakdown_lists_nonzero_counts_in_order() {
        let counts = Counts {
            healthy: 3,
            mixed: 0,
            down: 1,
            unknown: 2,
        };
        assert_eq!(counts.breakdown(), "3 online · 1 down · 2 unknown");
    }
}
