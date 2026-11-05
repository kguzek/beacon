use crate::{
    config::{Config, Service},
    model::*,
};
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct Envelope {
    status: String,
    data: Data,
    #[serde(default)]
    warnings: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    result_type: String,
    result: Vec<Series>,
}
#[derive(Deserialize)]
struct Series {
    metric: HashMap<String, String>,
    value: Option<(f64, String)>,
    values: Option<Vec<(f64, String)>>,
}
pub type Samples = HashMap<String, Vec<(f64, f64)>>;

pub struct Prometheus {
    client: reqwest::Client,
    config: Config,
    services: Vec<Service>,
}
impl Prometheus {
    pub fn new(config: Config, services: Vec<Service>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                config.prometheus.timeout_seconds,
            ))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            config,
            services,
        })
    }
    fn selector(&self, metric: &str) -> String {
        format!(
            "{metric}{{job={}}}",
            serde_json::to_string(&self.config.prometheus.job).unwrap()
        )
    }
    async fn query(&self, query: &str, at: f64, kind: Kind) -> Result<Samples> {
        let range = match kind {
            Kind::Range { window, step } => Some((window, step)),
            _ => None,
        };
        let mut url = url::Url::parse(&self.config.prometheus.url)?;
        url.set_path(&format!(
            "{}/api/v1/{}",
            url.path().trim_end_matches('/'),
            if range.is_some() {
                "query_range"
            } else {
                "query"
            }
        ));
        let mut params = vec![
            ("query", query.to_owned()),
            (
                "timeout",
                format!("{}s", self.config.prometheus.timeout_seconds),
            ),
        ];
        if let Some((window, bucket)) = range {
            params.extend([
                ("start", (at - window as f64 + bucket as f64).to_string()),
                ("end", at.to_string()),
                ("step", bucket.to_string()),
            ]);
        } else {
            params.push(("time", at.to_string()));
        }
        let mut request = self.client.get(url).query(&params);
        if let Some(token) = &self.config.prometheus.bearer_token {
            request = request.bearer_auth(token);
        }
        // Never propagate reqwest errors (which may contain private URLs) to public responses/logs.
        let response = request.send().await.map_err(|error| {
            // Classify without logging reqwest's message, which can contain private URLs.
            anyhow::anyhow!(if error.is_timeout() {
                "upstream request timed out"
            } else if error.is_connect() {
                "upstream connection failed"
            } else {
                "upstream request failed"
            })
        })?;
        let status = response.status();
        ensure!(
            status.is_success(),
            "upstream HTTP status {}{}",
            status.as_u16(),
            if status.is_redirection() {
                "; redirects are disabled; configure the final Prometheus URL directly"
            } else {
                ""
            }
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= self.config.prometheus.max_response_bytes as u64),
            "upstream response too large"
        );
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow::anyhow!("upstream body failed"))?;
            ensure!(
                body.len() + chunk.len() <= self.config.prometheus.max_response_bytes,
                "upstream response too large"
            );
            body.extend_from_slice(&chunk);
        }
        let (matrix, max_points) = match kind {
            Kind::Vector => (false, 1),
            Kind::Matrix(max) => (true, max),
            Kind::Range { window, step } => (true, (window / step) as usize + 1),
        };
        parse(&body, matrix, max_points, &self.services)
    }
    /// Median spacing of `up` samples per target, then the median across targets.
    pub async fn detect_scrape_interval(&self, now: f64) -> Result<u64> {
        if let Some(seconds) = self.config.refresh.scrape_seconds {
            return Ok(seconds);
        }
        let samples = self
            .query(
                &format!("{}[{DETECT_WINDOW}s]", self.selector("up")),
                now,
                Kind::Matrix(DETECT_WINDOW as usize + 10),
            )
            .await?;
        let mut intervals: Vec<f64> = samples
            .values()
            .filter_map(|points| {
                let gaps: Vec<f64> = points.windows(2).map(|p| p[1].0 - p[0].0).collect();
                median(gaps)
            })
            .collect();
        intervals.retain(|v| *v > 0.0);
        let interval = median(intervals).context(
            "cannot detect scrape interval: no target has two recent scrapes; set refresh.scrape_seconds",
        )?;
        Ok((interval.round() as u64).clamp(1, 3600))
    }
    pub async fn current(&self, now: f64) -> Result<Vec<u8>> {
        let success = self.selector("probe_success");
        let up = self.selector("up");
        let duration = self.selector("probe_duration_seconds");
        // Sequential queries bound upstream concurrency to three independent refresh tasks.
        let values = self.query(&success, now, Kind::Vector).await?;
        let stamps = self
            .query(&format!("timestamp({success})"), now, Kind::Vector)
            .await?;
        let scrapes = self.query(&up, now, Kind::Vector).await?;
        let scrape_stamps = self
            .query(&format!("timestamp({up})"), now, Kind::Vector)
            .await?;
        let durations = self.query(&duration, now, Kind::Vector).await?;
        let duration_stamps = self
            .query(&format!("timestamp({duration})"), now, Kind::Vector)
            .await?;
        let stale = self.config.refresh.stale_seconds as f64;
        let services = self
            .services
            .iter()
            .map(|s| {
                let value = instant(&values, &s.target);
                let observed = instant(&stamps, &s.target);
                let scrape_time = instant(&scrape_stamps, &s.target);
                let valid = observed.is_some_and(|t| fresh(t, now, stale))
                    && scrape_time.is_some_and(|t| fresh(t, now, stale))
                    && instant(&scrapes, &s.target) == Some(1.0)
                    && observed.zip(scrape_time).is_some_and(|(p, u)| p >= u);
                let status = match (valid, value) {
                    (true, Some(1.0)) => Status::Healthy,
                    (true, Some(0.0)) => Status::Down,
                    _ => Status::Unknown,
                };
                let duration_seconds = if status != Status::Unknown
                    && instant(&duration_stamps, &s.target) == observed
                {
                    instant(&durations, &s.target).filter(|v| *v >= 0.0)
                } else {
                    None
                };
                CurrentService {
                    service: s.public.clone(),
                    status,
                    observed_at: observed,
                    expires_at: observed.map(|t| t + stale),
                    duration_seconds,
                }
            })
            .collect();
        Ok(serde_json::to_vec(&Current {
            schema_version: 1,
            location: self.config.location.clone().into(),
            generated_at: now,
            expires_at: now + stale,
            services,
        })?)
    }
    pub async fn history(&self, now: f64, period: &str, interval: u64) -> Result<Vec<u8>> {
        match period {
            "hour" => self.probes(now, interval).await,
            "day" => {
                let r = &self.config.refresh;
                self.timeline(now, 86400, r.day_step_seconds, r.day_seconds, interval)
                    .await
            }
            "month" => {
                let r = &self.config.refresh;
                self.timeline(
                    now,
                    2592000,
                    r.month_step_seconds,
                    r.month_seconds,
                    interval,
                )
                .await
            }
            _ => anyhow::bail!("invalid history period"),
        }
    }
    /// Every raw probe in the last hour, one entry per Blackbox scrape.
    async fn probes(&self, now: f64, interval: u64) -> Result<Vec<u8>> {
        let window = 3600;
        let max = Kind::Matrix((window / interval) as usize + 10);
        let success = self
            .query(
                &format!("{}[{window}s]", self.selector("probe_success")),
                now,
                max,
            )
            .await?;
        // Both metrics come from the same scrape, so their timestamps match exactly.
        let codes = self
            .query(
                &format!("{}[{window}s]", self.selector("probe_http_status_code")),
                now,
                max,
            )
            .await?;
        let expected = window as f64 / interval as f64;
        let services = self
            .services
            .iter()
            .map(|s| {
                let codes = codes.get(&s.target);
                let probes: Vec<Probe> = success
                    .get(&s.target)
                    .into_iter()
                    .flatten()
                    .filter(|(_, v)| *v == 0.0 || *v == 1.0)
                    .map(|&(at, value)| Probe {
                        at,
                        success: value == 1.0,
                        http_status_code: codes
                            .and_then(|c| {
                                c.get(c.partition_point(|p| p.0 < at)).filter(|p| p.0 == at)
                            })
                            .and_then(|p| http_code(p.1)),
                    })
                    .collect();
                let successes = probes.iter().filter(|p| p.success).count();
                HistoryService {
                    service: s.public.clone(),
                    observed_success_ratio: (!probes.is_empty())
                        .then(|| successes as f64 / probes.len() as f64),
                    coverage: (probes.len() as f64 / expected).min(1.0),
                    data: Probes { probes },
                }
            })
            .collect();
        let refresh = self.config.refresh.hour_interval(interval);
        Ok(serde_json::to_vec(&History {
            schema_version: 2,
            kind: HistoryKind::Probes,
            location: self.config.location.clone().into(),
            generated_at: now,
            expires_at: now + (refresh * 3) as f64,
            window_seconds: window,
            scrape_interval_seconds: interval,
            refresh_seconds: refresh,
            step_seconds: None,
            services,
        })?)
    }
    /// State timeline: fine steps classified, then merged into runs of equal state.
    async fn timeline(
        &self,
        now: f64,
        window: u64,
        step: u64,
        refresh: u64,
        interval: u64,
    ) -> Result<Vec<u8>> {
        ensure!(
            step >= interval,
            "history step shorter than the scrape interval; raise day/month_step_seconds"
        );
        let end = (now / step as f64).floor() * step as f64;
        let range = Kind::Range { window, step };
        let selector = self.selector("probe_success");
        // Metadata changes split a target into multiple series, including within
        // a step. Combine sample totals, not per-series averages or counts.
        let counts = self
            .query(
                &format!("sum by (instance) (count_over_time({selector}[{step}s]))"),
                end,
                range,
            )
            .await?;
        let sums = self
            .query(
                &format!("sum by (instance) (sum_over_time({selector}[{step}s]))"),
                end,
                range,
            )
            .await?;
        let last_code = format!(
            "last_over_time({}[{step}s])",
            self.selector("probe_http_status_code")
        );
        // Codes only for failing steps (sparse). Without an original sample
        // timestamp we cannot order differing codes from old/new label sets.
        let codes = self
            .query(
                &format!(
                    "(max by (instance) ({last_code}) == min by (instance) ({last_code})) \
                     and on (instance) (min by (instance) (min_over_time({selector}[{step}s])) == 0)"
                ),
                end,
                range,
            )
            .await?;
        let expected = step as f64 / interval as f64;
        let steps = window / step;
        let services = self
            .services
            .iter()
            .map(|s| {
                let mut segments: Vec<Segment> = vec![];
                let mut total = 0;
                let mut failed = 0;
                for i in 1..=steps {
                    let ends_at = end - window as f64 + (i * step) as f64;
                    let count = at(&counts, &s.target, ends_at)
                        .filter(|v| *v >= 0.0 && v.fract() == 0.0)
                        .unwrap_or(0.0);
                    let sum = at(&sums, &s.target, ends_at).filter(|v| (0.0..=count).contains(v));
                    let (samples, failures) = match sum {
                        Some(sum) => (count as u64, (count - sum).round() as u64),
                        None => (0, 0),
                    };
                    total += samples;
                    failed += failures;
                    let state = if failures > 0 {
                        Status::Down
                    } else if samples > 0 && samples as f64 >= expected * 0.8 {
                        Status::Healthy
                    } else {
                        Status::Unknown
                    };
                    let code = at(&codes, &s.target, ends_at)
                        .and_then(http_code)
                        .filter(|_| failures > 0);
                    match segments.last_mut() {
                        Some(last) if last.state == state => {
                            last.end = ends_at;
                            last.samples += samples;
                            last.failures += failures;
                            last.last_http_status_code = code.or(last.last_http_status_code);
                        }
                        _ => segments.push(Segment {
                            start: ends_at - step as f64,
                            end: ends_at,
                            state,
                            samples,
                            failures,
                            last_http_status_code: code,
                        }),
                    }
                }
                HistoryService {
                    service: s.public.clone(),
                    observed_success_ratio: (total > 0)
                        .then(|| (total - failed) as f64 / total as f64),
                    coverage: (total as f64 / (window as f64 / interval as f64)).min(1.0),
                    data: Segments { segments },
                }
            })
            .collect();
        Ok(serde_json::to_vec(&History {
            schema_version: 2,
            kind: HistoryKind::Timeline,
            location: self.config.location.clone().into(),
            generated_at: now,
            expires_at: now + (refresh * 3) as f64,
            window_seconds: window,
            scrape_interval_seconds: interval,
            refresh_seconds: refresh,
            step_seconds: Some(step),
            services,
        })?)
    }
}
const DETECT_WINDOW: u64 = 600;
#[derive(Clone, Copy)]
enum Kind {
    Vector,
    /// Instant query with a range selector: raw samples, capped per series.
    Matrix(usize),
    Range {
        window: u64,
        step: u64,
    },
}
fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}
fn http_code(value: f64) -> Option<u16> {
    ((100.0..=599.0).contains(&value) && value.fract() == 0.0).then_some(value as u16)
}
fn instant(samples: &Samples, target: &str) -> Option<f64> {
    samples.get(target)?.first().map(|p| p.1)
}
/// Matrix samples are sorted by time (see `parse`), so binary search suffices.
fn at(samples: &Samples, target: &str, time: f64) -> Option<f64> {
    let points = samples.get(target)?;
    let index = points.partition_point(|p| p.0 < time - 0.01);
    points
        .get(index)
        .filter(|p| (p.0 - time).abs() < 0.01)
        .map(|p| p.1)
}
pub fn fresh(time: f64, now: f64, stale: f64) -> bool {
    time <= now + 5.0 && now - time <= stale
}
pub fn parse(body: &[u8], range: bool, max_points: usize, services: &[Service]) -> Result<Samples> {
    let envelope: Envelope = serde_json::from_slice(body).context("invalid upstream JSON")?;
    ensure!(
        envelope.status == "success" && envelope.warnings.is_empty(),
        "upstream query failed or partial result"
    );
    ensure!(
        envelope.data.result_type == if range { "matrix" } else { "vector" },
        "unexpected upstream result type"
    );
    let mut result = HashMap::new();
    for series in envelope.data.result {
        let Some(target) = series.metric.get("instance") else {
            continue;
        };
        if !services.iter().any(|s| s.target == *target) {
            continue;
        }
        ensure!(
            range || !result.contains_key(target),
            "ambiguous series for inventory target"
        );
        let raw = if range {
            series.values.context("missing matrix samples")?
        } else {
            vec![series.value.context("missing vector sample")?]
        };
        ensure!(raw.len() <= max_points, "too many upstream samples");
        let mut points = Vec::new();
        let mut previous = f64::NEG_INFINITY;
        for (time, value) in raw {
            ensure!(
                time.is_finite() && time >= 0.0 && time > previous,
                "invalid upstream timestamps"
            );
            previous = time;
            let number: f64 = value.parse().context("invalid upstream sample")?;
            // Prometheus NaN/Inf are missing observations, never healthy.
            if number.is_finite() {
                points.push((time, number));
            }
        }
        result
            .entry(target.clone())
            .or_insert_with(Vec::new)
            .extend(points);
    }
    if range {
        for points in result.values_mut() {
            // Metadata changes create new series for the same target. Stitch their
            // history together, but never choose between conflicting observations.
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in points.windows(2) {
                ensure!(
                    pair[0].0 != pair[1].0 || pair[0].1 == pair[1].1,
                    "ambiguous series for inventory target"
                );
            }
            points.dedup_by(|a, b| a.0 == b.0);
            ensure!(points.len() <= max_points, "too many upstream samples");
        }
    }
    Ok(result)
}
