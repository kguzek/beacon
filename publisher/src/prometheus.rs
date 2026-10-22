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
    pub async fn history(&self, now: f64, month: bool) -> Result<Vec<u8>> {
        let r = &self.config.refresh;
        let (window, bucket, interval) = if month {
            (2592000, r.month_bucket_seconds, r.month_seconds)
        } else {
            (86400, r.day_bucket_seconds, r.day_seconds)
        };
        let end = (now / bucket as f64).floor() * bucket as f64;
        let selector = self.selector("probe_success");
        let averages = self
            .query(
                &format!("avg_over_time({selector}[{bucket}s])"),
                end,
                Some((window, bucket)),
            )
            .await?;
        let counts = self
            .query(
                &format!("count_over_time({selector}[{bucket}s])"),
                end,
                Some((window, bucket)),
            )
            .await?;
        let expected = bucket as f64 / r.scrape_seconds as f64;
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
            !result.contains_key(target),
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
        result.insert(target.clone(), points);
    }
    Ok(result)
}
