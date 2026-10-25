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
    async fn query(&self, query: &str, at: f64, range: Option<(u64, u64)>) -> Result<Samples> {
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
        parse(&body, range.is_some(), &self.services)
    }
    pub async fn current(&self, now: f64) -> Result<Vec<u8>> {
        let success = self.selector("probe_success");
        let up = self.selector("up");
        let duration = self.selector("probe_duration_seconds");
        // Sequential queries bound upstream concurrency to three independent refresh tasks.
        let values = self.query(&success, now, None).await?;
        let stamps = self
            .query(&format!("timestamp({success})"), now, None)
            .await?;
        let scrapes = self.query(&up, now, None).await?;
        let scrape_stamps = self.query(&format!("timestamp({up})"), now, None).await?;
        let durations = self.query(&duration, now, None).await?;
        let duration_stamps = self
            .query(&format!("timestamp({duration})"), now, None)
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
                    service: s.clone(),
                    status,
                    observed_at: observed,
                    expires_at: observed.map(|t| t + stale),
                    duration_seconds,
                }
            })
            .collect();
        Ok(serde_json::to_vec(&Current {
            schema_version: 1,
            location: self.config.location.clone(),
            generated_at: now,
            expires_at: now + stale,
            services,
        })?)
    }
    pub async fn history(&self, now: f64, period: &str) -> Result<Vec<u8>> {
        let r = &self.config.refresh;
        let (window, bucket, interval) = match period {
            "hour" => (3600, r.hour_bucket_seconds, r.hour_seconds),
            "day" => (86400, r.day_bucket_seconds, r.day_seconds),
            "month" => (2592000, r.month_bucket_seconds, r.month_seconds),
            _ => anyhow::bail!("invalid history period"),
        };
        let end = (now / bucket as f64).floor() * bucket as f64;
        let selector = self.selector("probe_success");
        // Metadata changes split a target into multiple series, including within
        // a bucket. Combine sample totals, not per-series averages or counts.
        let count_query = format!("sum by (instance) (count_over_time({selector}[{bucket}s]))");
        let averages = self
            .query(
                &format!(
                    "sum by (instance) (sum_over_time({selector}[{bucket}s])) / {count_query}"
                ),
                end,
                Some((window, bucket)),
            )
            .await?;
        let counts = self
            .query(&count_query, end, Some((window, bucket)))
            .await?;
        let expected = bucket as f64 / r.scrape_seconds as f64;
        let last_code = format!(
            "last_over_time({}[{bucket}s])",
            self.selector("probe_http_status_code")
        );
        // Without an original sample timestamp we cannot order differing codes
        // from old/new label sets. Omit that optional detail, not the history.
        let codes = self
            .query(
                &format!("max by (instance) ({last_code}) == min by (instance) ({last_code})"),
                end,
                Some((window, bucket)),
            )
            .await?;
        let services = self
            .services
            .iter()
            .map(|s| {
                let mut total = 0;
                let mut successes = 0.0;
                let buckets = (1..=window / bucket)
                    .map(|i| {
                        let ends_at = end - window as f64 + (i * bucket) as f64;
                        let count = at(&counts, &s.target, ends_at)
                            .filter(|v| *v >= 0.0 && v.fract() == 0.0)
                            .unwrap_or(0.0) as u64;
                        let ratio =
                            at(&averages, &s.target, ends_at).filter(|v| (0.0..=1.0).contains(v));
                        let samples = if ratio.is_some() { count } else { 0 };
                        total += samples;
                        successes += ratio.unwrap_or(0.0) * samples as f64;
                        Bucket {
                            ends_at,
                            observed_success_ratio: ratio.filter(|_| samples > 0),
                            coverage: (samples as f64 / expected).min(1.0),
                            samples,
                            last_http_status_code: at(&codes, &s.target, ends_at)
                                .filter(|v| {
                                    samples > 0 && (100.0..=599.0).contains(v) && v.fract() == 0.0
                                })
                                .map(|v| v as u16),
                        }
                    })
                    .collect();
                HistoryService {
                    service: s.clone(),
                    observed_success_ratio: (total > 0).then(|| successes / total as f64),
                    coverage: (total as f64 / (window as f64 / r.scrape_seconds as f64)).min(1.0),
                    buckets,
                }
            })
            .collect();
        Ok(serde_json::to_vec(&History {
            schema_version: 1,
            location: self.config.location.clone(),
            generated_at: now,
            expires_at: now + (interval * 3) as f64,
            window_seconds: window,
            bucket_seconds: bucket,
            services,
        })?)
    }
}
fn instant(samples: &Samples, target: &str) -> Option<f64> {
    samples.get(target)?.first().map(|p| p.1)
}
fn at(samples: &Samples, target: &str, time: f64) -> Option<f64> {
    samples
        .get(target)?
        .iter()
        .find(|p| (p.0 - time).abs() < 0.01)
        .map(|p| p.1)
}
pub fn fresh(time: f64, now: f64, stale: f64) -> bool {
    time <= now + 5.0 && now - time <= stale
}
pub fn parse(body: &[u8], range: bool, services: &[Service]) -> Result<Samples> {
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
        ensure!(raw.len() <= 1001, "too many upstream samples");
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
            ensure!(points.len() <= 1001, "too many upstream samples");
        }
    }
    Ok(result)
}
