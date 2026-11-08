//! Shared with `apps/web/tests/conformance.test.ts`: the page and the desktop
//! clients must agree on every case in `fixtures/current/`.

use beacon_core::{Counts, HealthState, Observer, combined_status, summarize, visible_inventory};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
struct Fixture {
    description: String,
    now: f64,
    observers: Vec<Observer>,
    expected: Expected,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Expected {
    services: BTreeMap<String, HealthState>,
    headline: String,
    counts: Counts,
    breakdown: String,
}

#[test]
fn shared_fixtures_match_the_status_page() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/current");
    let mut checked = 0;
    for entry in fs::read_dir(&directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let fixture: Fixture = serde_json::from_str(&fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for observer in &fixture.observers {
            if let Some(current) = &observer.current {
                beacon_core::validate_current(current, &observer.config).unwrap();
            }
        }
        let summary = summarize(&fixture.observers, fixture.now);
        let actual = Expected {
            services: visible_inventory(&fixture.observers)
                .iter()
                .map(|s| {
                    let state = combined_status(&s.id, &fixture.observers, fixture.now);
                    (s.id.clone(), state)
                })
                .collect(),
            headline: summary.headline.clone(),
            counts: summary.counts,
            breakdown: summary.breakdown.clone(),
        };
        assert_eq!(actual, fixture.expected, "fixture: {}", fixture.description);
        let listed: BTreeMap<_, _> = summary
            .services
            .iter()
            .map(|s| (s.id.clone(), s.state))
            .collect();
        assert_eq!(
            listed, fixture.expected.services,
            "fixture: {}",
            fixture.description
        );
        checked += 1;
    }
    assert!(checked > 0, "no fixtures found in {}", directory.display());
}
