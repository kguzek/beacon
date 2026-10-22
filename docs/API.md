# Beacon API v1

All timestamps are Unix seconds (JSON numbers, potentially fractional). Durations are seconds, not HTTP-only latency. Public JSON is constructed from typed fields, never upstream JSON. Successful responses are immutable cached bytes. Errors are small JSON objects with an `error` code; never private diagnostics. No endpoint refreshes upstream data. No persistence exists.

Machine-readable shapes: [current](schema/current.schema.json), [history](schema/history.schema.json), [frontend observer settings](schema/locations.schema.json). Generated with `pnpm schema` from the browser's runtime validators. Timestamp freshness, duplicate identity rejection and other semantic rules below are additional to structural JSON Schema validation.

## Routes

Only GET without any query string or body is supported. Other methods return 405, unexpected query parameters 400, unknown paths 404. CORS preflight OPTIONS is handled by middleware for configured exact frontend origins; it is not authentication. The API is public regardless of CORS.

| Route | Availability | Default refresh |
| --- | --- | --- |
| `/v1/current` | 503 `snapshot_unavailable` before first valid refresh | 30s |
| `/v1/history/day` | Same, independently initialized | 300s |
| `/v1/history/hour` | Same, independently initialized; 1 hour in 30s buckets | 30s |
| `/v1/history/month` | Same, independently initialized | 1800s |
| `/healthz` | 200 while HTTP process is alive | No upstream check |
| `/readyz` | 503 until first current snapshot, then 200 until restart | Latched |

Current-only readiness deliberately does not wait on expensive history. Upstream failure never makes liveness fail or reverses initial readiness. Each refresh loop is sequential, starts immediately, and sleeps after completion. Repeated failures exponentially increase that loop's delay (factor 2, capped exponent 6 and delay 24h). The previous successful bytes and timestamps are retained. Three loops allow at most three concurrent upstream requests. Requests have timeouts and streamed byte limits; serialized snapshots are limited to 32 MiB each. At most 500 published services and 1000 buckets per history view are accepted.

Successful responses have `Cache-Control: public, max-age=N, must-revalidate` (default N=15, max 60). Consumers MUST evaluate embedded expiry timestamps; HTTP receipt time and cache age never establish monitoring freshness. Error/health responses use `no-store`.

## Current

```json
{
  "schema_version": 1,
  "location": {"id":"primary", "name":"Primary server"},
  "generated_at": 1700000000,
  "expires_at": 1700000120,
  "services": [{
    "id": "example.org",
    "name": "Example website",
    "hidden": false,
    "hosting_location": "secondary",
    "status": "healthy",
    "observed_at": 1699999990,
    "expires_at": 1700000110,
    "duration_seconds": 0.125
  }]
}
```

`hosting_location` is optional metadata represented as null when absent. It never limits the observer's inventory. Every published service appears, including missing observations and page-hidden services. Publication opt-outs are entirely absent. `hidden` is a presentation instruction, NOT secrecy.

`status` is `healthy` for `probe_success=1`, `down` for `probe_success=0`, otherwise `unknown`. Both success and `up` must have fresh source timestamps, `up=1`, and the probe observation must not precede the latest scrape. `timestamp(metric)` obtains actual source time, not the instant-query evaluation time. Duplicate series for an exact target fail the refresh rather than choosing a potentially wrong series. Nonfinite samples are missing. Missing data affects only that service. Prometheus warnings/partial responses, invalid envelopes or ambiguous series fail the whole refresh.

`observed_at` is the source probe timestamp, possibly stale even in a freshly generated snapshot. Null means no usable observation. Per-service expiry = observation + configured stale threshold. Snapshot expiry = generation + threshold. Consumers must render **unknown** when either expires, the observation is missing, or timestamps are implausibly in the future (tolerance 5s). This applies equally to healthy and failed probes. Cached `status` does not mutate merely because time passes. `duration_seconds` is total Blackbox probe duration, available only for a valid current observation with matching source timestamp.

## History

All three history routes share the current envelope and inventory metadata. Instead of current status fields they contain:

```json
{
  "window_seconds": 86400,
  "bucket_seconds": 900,
  "services": [{
    "id": "example.org", "name": "Example website", "hidden": false,
    "hosting_location": null,
    "observed_success_ratio": 0.95,
    "coverage": 0.5,
    "buckets": [{"ends_at":1700000100,"observed_success_ratio":0.95,"coverage":0.5,"samples":15,"last_http_status_code":200}]
  }]
}
```

The real response has all buckets, including empty ones. Hour = rolling 1h / 30-second buckets (120); day = rolling 24h / 15-minute buckets (96); month = rolling 30d / 6-hour buckets (120). Widths are configurable at startup, windows and routes are fixed. Set `hour_bucket_seconds` to the actual `scrape_seconds` for one check per bucket (30s by default). Only completed buckets are queried, aligned to UTC epoch multiples. Prometheus `avg_over_time(probe_success[width])` and `count_over_time(...)` run through `query_range` once each, for all targets in the configured job. Range windows are left-open/right-closed per Prometheus semantics.

`observed_success_ratio` is the sample-weighted fraction of observed successes, null with no samples. It is NOT wall-clock uptime. `coverage = min(samples / (bucket_width / configured_scrape_interval), 1)`; summary coverage uses the whole window. Coverage is an estimate: interval jitter, scrape configuration changes and gaps affect it; count alone does not prove continuous monitoring. Missing/failed scrapes produce no probe samples and reduce coverage, never fabricated successes. Gray frontend buckets have coverage <80% or no ratio. Other buckets are green only at 100% observed success, otherwise red. Exact numeric values are in bucket tooltips and summaries. History expiry = generation + 3 × its refresh interval, independent of current. Expired history is not rendered as fresh history.

`last_http_status_code` is the most recent `probe_http_status_code` sample in the bucket, queried using `last_over_time(...)`. It is null for empty buckets, missing metrics, no HTTP response (code 0), or invalid codes outside integer 100–599. Older publishers may omit it. This is the last code, not a claim that every check in the bucket returned it; bucket state is derived from success ratio and coverage, not the code.

## Inventory and matching

Native Prometheus file_sd YAML groups: `targets` string array plus optional string-valued `labels`. Only HTTP/HTTPS URLs are accepted. Public identity is lowercase URL hostname (port/path/query/credentials omitted). Duplicate published hostnames are rejected, including different ports or paths. Published targets require a nonempty `service` display name. Names/location text is bounded to 200 bytes and rejects control characters. Inventory must contain 1–500 published services. Unknown TOML keys fail startup.

Labels: `beacon_publish` defaults to `"true"`, `beacon_hidden` defaults to `"false"`; only exact string `"true"`/`"false"` accepted. Opt-out takes precedence. `beacon_hosting_location` optionally identifies the hosting server. Other labels, including Blackbox module selection, remain private. Prometheus `instance` must equal the inventory target URL **byte for byte**, selected within the explicit TOML `prometheus.job`. Standard Blackbox relabeling copies `__param_target` to `instance`; customized monitoring must preserve this or adapt its relabeling. Missing matches are unknown, not deleted or healthy. Beacon never calls the targets API or Blackbox directly.

## Browser policy

The page groups by service, with one history row per configured observer and a compact observer reachability list above the services. It makes no aggregate all/any/quorum health claim. It unions inventories from all successful in-memory snapshots to retain rows under deployment skew and represents omitted observations as unknown. Hidden at any observer means not shown anywhere in this page (conservative skew policy). There is no hardcoded service list. Observer reachability, snapshot freshness and observed status remain distinct; an unreachable observer may still have valid short-lived observations. All unreachable observers trigger the monitoring-failure banner, not a mass service-down claim. No localStorage, IndexedDB, service worker, or external snapshot fallback is used.
