//! Waybar custom-module output (`"return-type": "json"`), using Pango markup.

use beacon_core::{Error, HealthState, Overview, PublisherStatus, Snapshot};
use serde::Serialize;

/// Colours from the status page's palette.
fn colour(state: HealthState) -> &'static str {
    match state {
        HealthState::Healthy => "#7dd3b0",
        HealthState::Mixed => "#e8c37a",
        HealthState::Down => "#ed7c8b",
        HealthState::Unknown => "#8491a7",
    }
}

fn glyph(state: HealthState) -> &'static str {
    match state {
        HealthState::Healthy => "✓",
        HealthState::Mixed => "◐",
        HealthState::Down => "✕",
        HealthState::Unknown => "?",
    }
}

fn state_class(state: HealthState) -> &'static str {
    match state {
        HealthState::Healthy => "healthy",
        HealthState::Mixed => "mixed",
        HealthState::Down => "down",
        HealthState::Unknown => "unknown",
    }
}

fn overview_alt(overview: Overview) -> &'static str {
    match overview {
        Overview::AllOnline => "all_online",
        Overview::Mixed => "mixed",
        Overview::PartialOutage => "partial_outage",
        Overview::AllDown => "all_down",
        Overview::Incomplete => "incomplete",
        Overview::Unavailable => "unavailable",
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Output {
    pub text: String,
    pub tooltip: String,
    /// The combined state, for styling: `healthy`, `mixed`, `down`, `unknown`.
    pub class: &'static str,
    /// The page-level verdict, e.g. `partial_outage`, usable in `format-icons`.
    pub alt: &'static str,
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn span(state: HealthState, text: &str) -> String {
    format!(r#"<span foreground="{}">{}</span>"#, colour(state), text)
}

fn publisher_line(publisher: &PublisherStatus) -> String {
    let name = escape(&publisher.name);
    if !publisher.reachable {
        return format!("{} {name}: unreachable", span(HealthState::Unknown, "×"));
    }
    let counts = [
        (publisher.healthy, "online"),
        (publisher.down, "down"),
        (publisher.unknown, "unknown"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect::<Vec<_>>()
    .join(" · ");
    let state = if publisher.down > 0 {
        HealthState::Down
    } else if publisher.unknown > 0 {
        HealthState::Unknown
    } else {
        HealthState::Healthy
    };
    let counts = if counts.is_empty() {
        "no services".into()
    } else {
        counts
    };
    format!("{} {name}: {counts}", span(state, "●"))
}

pub fn render(snapshot: &Snapshot) -> Output {
    let summary = &snapshot.summary;
    let text = if summary.overview == Overview::AllOnline {
        span(HealthState::Healthy, glyph(HealthState::Healthy))
    } else if summary.counts.total() == 0 {
        span(HealthState::Unknown, glyph(HealthState::Unknown))
    } else {
        HealthState::ALL
            .iter()
            .filter(|&&state| summary.counts.get(state) > 0)
            .map(|&state| {
                span(
                    state,
                    &format!("{}{}", glyph(state), summary.counts.get(state)),
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut lines = vec![format!(
        "<b>{}</b>: {}",
        escape(&snapshot.title),
        escape(&summary.headline)
    )];
    if !summary.breakdown.is_empty() {
        lines.push(summary.breakdown.clone());
    }
    lines.push(String::new());
    lines.extend(snapshot.publishers.iter().map(publisher_line));
    let mut problems = vec![];
    for state in [HealthState::Down, HealthState::Mixed, HealthState::Unknown] {
        let names: Vec<String> = summary
            .services
            .iter()
            .filter(|service| service.state == state)
            .map(|service| escape(&service.name))
            .collect();
        if !names.is_empty() {
            let label = state.label();
            let label = format!("{}{}", label[..1].to_uppercase(), &label[1..]);
            problems.push(format!("{}: {}", span(state, &label), names.join(", ")));
        }
    }
    if !problems.is_empty() {
        lines.push(String::new());
        lines.extend(problems);
    }

    Output {
        text,
        tooltip: lines.join("\n"),
        class: state_class(summary.state),
        alt: overview_alt(summary.overview),
    }
}

/// Keeps the module visible (and explains why) when the page can't be read.
pub fn render_error(page_url: &str, error: &Error) -> Output {
    Output {
        text: span(HealthState::Unknown, glyph(HealthState::Unknown)),
        tooltip: format!(
            "<b>Beacon</b>: status unavailable\n{}\n\n{}",
            escape(page_url),
            escape(&error.to_string())
        ),
        class: state_class(HealthState::Unknown),
        alt: "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beacon_core::{
        Observer,
        api::{Current, CurrentService, Location, LocationConfig, Service, Status},
    };

    fn observer(id: &str, statuses: Option<&[Status]>) -> Observer {
        let services = statuses.map(|statuses| {
            statuses
                .iter()
                .enumerate()
                .map(|(i, &status)| CurrentService {
                    service: Service {
                        id: format!("s{i}.org"),
                        name: format!("<S{i}> & co"),
                        hidden: false,
                        requires_vpn: false,
                        hosting_location: None,
                        labels: Default::default(),
                    },
                    status,
                    observed_at: Some(100.0),
                    expires_at: Some(200.0),
                    duration_seconds: None,
                })
                .collect()
        });
        Observer {
            config: LocationConfig {
                id: id.into(),
                name: id.to_uppercase(),
                url: format!("https://{id}.example.org"),
            },
            reachable: services.is_some(),
            current: services.map(|services| Current {
                schema_version: 1,
                location: Location {
                    id: id.into(),
                    name: id.into(),
                },
                generated_at: 100.0,
                expires_at: 200.0,
                services,
            }),
        }
    }

    fn snapshot(observers: &[Observer]) -> Snapshot {
        Snapshot::new("Status".into(), "https://x".into(), observers, 150.0)
    }

    #[test]
    fn all_online_is_a_single_check() {
        let healthy = [Status::Healthy, Status::Healthy];
        let output = render(&snapshot(&[
            observer("hp", Some(&healthy)),
            observer("pi", Some(&healthy)),
        ]));
        assert_eq!(output.text, r##"<span foreground="#7dd3b0">✓</span>"##);
        assert_eq!((output.class, output.alt), ("healthy", "all_online"));
        assert!(
            output
                .tooltip
                .starts_with("<b>Status</b>: All services online\n2 online\n")
        );
    }

    #[test]
    fn outage_shows_counts_publishers_and_escaped_names() {
        let output = render(&snapshot(&[
            observer("hp", Some(&[Status::Healthy, Status::Down])),
            observer("pi", None),
        ]));
        // pi is unreachable, so s0 is mixed; hp reports s1 down. No location
        // confirms everything, so the page shows a partial outage.
        assert_eq!(
            output.text,
            r##"<span foreground="#e8c37a">◐1</span> <span foreground="#ed7c8b">✕1</span>"##
        );
        assert_eq!((output.class, output.alt), ("down", "partial_outage"));
        assert!(output.tooltip.contains("HP: 1 online · 1 down"));
        assert!(output.tooltip.contains("PI: unreachable"));
        assert!(output.tooltip.contains("&lt;S1&gt; &amp; co"));
        assert!(!output.tooltip.contains("<S1>"));
    }

    #[test]
    fn errors_keep_the_module_visible() {
        let output = render_error("https://x", &Error::Request("timed out".into()));
        assert_eq!((output.class, output.alt), ("unknown", "error"));
        assert!(output.tooltip.contains("request failed: timed out"));
        let json = serde_json::to_value(&output).unwrap();
        assert!(json["text"].is_string() && json["tooltip"].is_string());
    }
}
