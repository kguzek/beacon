# Beacon API v1

Each Beacon publisher reads a service inventory and periodically collects results from Prometheus. The API serves those cached results; opening the status page or calling an endpoint never triggers a monitoring query.

The API is public. Private target URLs, credentials and unrelated scrape labels are not included in responses. Publishers and the browser keep snapshots only in memory, not on disk.

## Configuration references

- [Publisher settings](../examples/config.toml): Prometheus connection, inventory path, HTTP server, refresh intervals and freshness limits.
- [Service inventory example](../examples/services.yaml): service names, visibility and grouping labels.
- [Startup arguments](../README.md#publisher-configuration): use `--config.path` to select or layer configuration files. With no arguments, the publisher reads `/etc/beacon/config.toml`.
- [Browser locations example](../locations.example.json): copy to `apps/web/public/locations.json` to configure the monitoring locations the page contacts, in priority order.

For example, start a publisher with a base configuration and local overrides:

```sh
beacon-publisher --config.path config.toml --config.path config.local.toml
```

Configuration is loaded at startup; restart or recreate the publisher after editing it. Unknown TOML settings fail startup. The inventory is checked every 10 seconds and reloaded when its content changes. An invalid or unreadable inventory is logged once and the previous one stays in use. Changes reach each snapshot (current, hour, day, month) on its next refresh.

## Inventory and matching

The inventory uses Prometheus `file_sd` YAML: a list of entries, each containing `targets` and optional string-valued `labels`. The publisher reads the file specified by `services.path` in its TOML configuration.

```yaml
- targets: ["https://grafana.example.org/api/health"]
  labels:
    service: "Grafana"
    beacon_label_group: "Observability"
    beacon_hosting_location: "primary"
```

This produces a service with public ID `grafana.example.org`, display name `Grafana`, group `Observability` and hosting location `primary`. The full target URL is not published.

### Target rules

- Only HTTP and HTTPS targets are supported.
- A service's public ID is its lowercase hostname. For example, `https://EXAMPLE.org:8443/health?token=secret` becomes `example.org`; the port, path, query and credentials are omitted.
- Published hostnames must be unique. Defining both `https://example.org/` and `https://example.org/health` fails startup because both have ID `example.org`.
- The inventory must contain 1–500 published services. Publication opt-outs do not count toward this limit.
- Labels on an entry apply to **every target in that entry**.

For monitoring results to match, Prometheus's `instance` label must equal the **full inventory target URL byte for byte**, within the configured `prometheus.job`. Standard Blackbox relabeling copies `__param_target` to `instance`; custom relabeling must preserve that match.

A missing result makes a service unknown; it does not remove the service. Beacon does not call the Prometheus targets API or probe services directly.

## Inventory labels

### Label reference

Quote boolean values in YAML: use `"false"`, not `false`. All inventory label values must be strings.

| Label | Example | Default | Effect |
| --- | --- | --- | --- |
| `service` | `service: "Grafana"` | Required for published targets | Sets the public display name. |
| `beacon_publish` | `beacon_publish: "false"` | `"true"` | Excludes the target from **all API responses** and the page. |
| `beacon_hidden` | `beacon_hidden: "true"` | `"false"` | Keeps the service in the API but hides it from the page, search and summaries. |
| `beacon_requires_vpn` | `beacon_requires_vpn: "true"` | `"false"` | Keeps the service visible and adds a shield-lock VPN badge beside its name. Publishes the boolean `requires_vpn` in current and history responses; does not change health calculations or enforce access restrictions. |
| `beacon_hosting_location` | `beacon_hosting_location: "primary"` | Absent | Adds a public “hosted at” description. It does not choose which observers monitor the service or how it is grouped. |
| `beacon_label_group` | `beacon_label_group: "Observability"` | Absent | Places the service in a named, collapsible group on the page. |
| `beacon_label_<key>` | `beacon_label_team: "Platform"` | Absent | Publishes additional organizational metadata under `labels`, without the `beacon_label_` prefix. Only `group` currently affects page organization. |

`beacon_publish`, `beacon_hidden` and `beacon_requires_vpn` accept only the exact strings `"true"` and `"false"`. Publication opt-out takes precedence over page visibility. Older publishers may omit `requires_vpn`; the browser treats it as false. If any observer marks a service as requiring VPN, the page shows the badge.

Display names and hosting-location values must be nonempty, contain no control characters and fit within 200 UTF-8 bytes.

### Hiding a service versus keeping it private

Use `beacon_hidden: "true"` when a service should remain available through the public API but not appear on the page:

```yaml
- targets: ["https://api.example.org/health"]
  labels:
    service: "Partner API"
    beacon_hidden: "true"
    beacon_label_group: "Integrations"
```

The API still includes `api.example.org`, its name, status and public labels. **Hidden does not mean private.**

Use `beacon_publish: "false"` to exclude a target entirely from Beacon's public output:

```yaml
- targets: ["https://internal.example.org/health"]
  labels:
    beacon_publish: "false"
```

An opted-out target does not need a `service` display name. It may still be monitored by Prometheus independently of Beacon.

### Public organizational labels

The `beacon_label_` prefix is an explicit **publication opt-in**. Its values are public, even for services with `beacon_hidden: "true"`. Never put secrets in these labels.

For example, this inventory entry:

```yaml
- targets: ["https://grafana.example.org"]
  labels:
    service: "Grafana"
    beacon_label_group: " Observability "
    beacon_label_team: "Platform"
    module: "http_2xx"
    internal_owner: "ops-private"
```

produces this metadata in both current and history responses:

```json
{
  "id": "grafana.example.org",
  "name": "Grafana",
  "hidden": false,
  "hosting_location": null,
  "labels": {
    "group": "Observability",
    "team": "Platform"
  }
}
```

The prefix is removed, surrounding whitespace is trimmed, and display casing is preserved. `module` and `internal_owner` remain private because they are not explicitly public labels.

New publishers return `"labels": {}` when no public organizational labels are configured. Older publishers may omit `labels`; the browser treats that as an empty map. An absent hosting location is returned as `"hosting_location": null`.

### Public-label validation

Invalid explicitly public metadata fails publisher startup rather than being silently dropped. These rules also apply to entries with `beacon_publish: "false"`.

| Rule | Valid example | Invalid example |
| --- | --- | --- |
| Keys, after removing `beacon_label_` and trimming whitespace, match `[a-z][a-z0-9_]*`. | `beacon_label_team_name: "Platform"` | `beacon_label_Team: "Platform"` or `beacon_label_team-name: "Platform"` |
| Keys are at most 32 ASCII bytes. | `beacon_label_environment: "Production"` | A suffix longer than 32 characters. |
| Each entry has at most 16 public organizational labels. | `group`, `team` and `environment` together. | 17 `beacon_label_*` labels on one entry. |
| Values, after trimming, are nonempty and at most 200 UTF-8 bytes. | `beacon_label_group: " Websites "` becomes `"Websites"`. | `beacon_label_group: "   "` |
| Values contain no control characters. | `beacon_label_group: "Websites"` | A value containing an embedded newline or tab. |
| Normalized keys are unique. | Separate `group` and `team` keys. | `beacon_label_group` and `"beacon_label_group "` on the same entry. |

Omitting `beacon_label_group` is valid. Explicitly setting it to `""` is not. UTF-8 byte limits mean non-ASCII characters may use more than one byte each.

### Grouping examples

Separate entries with the same group value share one container:

```yaml
- targets: ["https://grafana.example.org"]
  labels:
    service: "Grafana"
    beacon_label_group: "Observability"

- targets: ["https://prometheus.example.org"]
  labels:
    service: "Prometheus"
    beacon_label_group: "Observability"

- targets: ["https://example.org"]
  labels:
    service: "Website"
```

Here, Grafana and Prometheus appear under **Observability**; Website appears under **Other services**.

- Group values are case-sensitive: `"Observability"` and `"observability"` are separate groups.
- If no visible service has a group, the page stays flat rather than adding a redundant “Other services” container.
- Hosting location and group are independent. A service can have `beacon_hosting_location: "primary"` and `beacon_label_group: "Communication"`.
- Other public keys, such as `beacon_label_team: "Platform"`, are published but do not create groups or search filters.

## HTTP endpoints

| Route | Purpose | Default refresh |
| --- | --- | --- |
| `/v1/current` | Latest service observations. | 30 seconds |
| `/v1/history/hour` | Every probe from the last hour. | Scrape interval (at least 10 seconds) |
| `/v1/history/day` | Last 24 hours as a state timeline. | 5 minutes |
| `/v1/history/month` | Last 30 days as a state timeline. | 30 minutes |
| `/healthz` | HTTP process is alive; does not check Prometheus. | Not applicable |
| `/readyz` | At least one current snapshot has been collected since startup. | Not applicable |

Each snapshot endpoint returns HTTP 503 with error code `snapshot_unavailable` until its own first successful refresh. History does not block current data or readiness. `/readyz` returns 503 before the first current snapshot and 200 afterward, even if later refreshes fail; readiness is not a freshness guarantee.

### Requests, caching and failures

- Use GET requests with no query string or body. Other methods return 405, unexpected query parameters return 400, and unknown paths return 404.
- CORS preflight OPTIONS is handled for configured exact frontend origins. CORS is not authentication: the API remains public.
- Successful responses are cached JSON snapshots with `Cache-Control: public, max-age=N, must-revalidate`. `N` defaults to 15 seconds and cannot exceed 60. Errors and health responses use `no-store`.
- Errors contain an `error` code, not private diagnostic details.
- Four independent refresh loops handle current, hour, day and month. Each starts immediately, runs requests sequentially and waits after completion. At most four upstream requests run concurrently.
- Failed refreshes retain the last successful response, including its original timestamps. Retry delays double after each consecutive failure, up to 64 times the configured interval and at most 24 hours. A successful refresh resets the delay.
- Upstream requests have timeouts and streamed byte limits. Each serialized snapshot is limited to 32 MiB. Hour histories hold at most one hour of raw samples per service; timelines are limited to 10,000 steps per window.

**Always check the JSON expiry timestamps.** Receiving a successful HTTP response—even from a reachable publisher—does not mean its observations are still fresh.

## Current status response

`GET /v1/current` returns an envelope describing the publisher and a `services` array:

```json
{
  "schema_version": 1,
  "location": { "id": "primary", "name": "Primary server" },
  "generated_at": 1700000000,
  "expires_at": 1700000120,
  "services": [
    {
      "id": "example.org",
      "name": "Example website",
      "hidden": false,
      "hosting_location": "secondary",
      "labels": { "group": "Websites" },
      "status": "healthy",
      "observed_at": 1699999990,
      "expires_at": 1700000110,
      "duration_seconds": 0.125
    }
  ]
}
```

All timestamps are Unix seconds and may be fractional. Durations are in seconds: `0.125` means 125 ms of total Blackbox probe time, not just HTTP latency.

### Status and freshness

| Field | Meaning |
| --- | --- |
| Top-level `generated_at` | When the publisher built this snapshot. |
| Top-level `expires_at` | When the snapshot stops being usable as current data. |
| Service `status` | `healthy` for a valid `probe_success=1`, `down` for a valid `probe_success=0`, otherwise `unknown`. |
| Service `observed_at` | Source timestamp of the probe result, or `null` when unavailable. It can be old even in a newly generated snapshot. |
| Service `expires_at` | Observation timestamp plus the configured stale threshold, or `null` when unavailable. |
| Service `duration_seconds` | Total probe duration for a valid observation with a matching source timestamp, otherwise `null`. |

Snapshot expiry is its generation time plus the same stale threshold. Consumers must show **unknown** when:

- The snapshot or service observation has expired.
- The observation or its expiry is missing.
- The snapshot generation or observation time is more than 5 seconds in the future.

These rules apply to both healthy and down observations. A cached `"status": "healthy"` or `"status": "down"` does not change inside the JSON when it expires; the consumer must apply the freshness rules.

Every published service appears, including hidden services and services without observations. Opted-out services are absent.

### Prometheus result validation

Both `probe_success` and `up` must have fresh source timestamps, `up` must equal 1, and the probe observation must not precede the latest scrape. The publisher uses `timestamp(metric)` to obtain source timestamps rather than instant-query evaluation times.

Nonfinite samples are treated as missing. Missing results affect only their service. Duplicate series for an exact target, Prometheus warnings or partial responses, and invalid response envelopes fail the entire refresh rather than publishing potentially misleading results.

## History response

All three history endpoints use the same `location`, `generated_at` and `expires_at` envelope and inventory metadata as `/v1/current`, with `"schema_version": 2`. Instead of current observation fields, each service has a summary plus either a probe list (hour) or a state timeline (day and month). `kind` tells them apart.

### Scrape interval

History is interpreted against the Blackbox job's real scrape interval. By default the publisher measures it at startup by querying `up{job="…"}[10m]`: it takes each target's median gap between scrapes, then the median across targets, rounded to whole seconds. Current status does not wait for detection. History endpoints return 503 until it succeeds, and detection is retried with the usual backoff. It fails if no target has two scrapes in the last 10 minutes, for example with scrape intervals over five minutes. In that case, set `refresh.scrape_seconds` explicitly, which also skips detection. The interval is established once per process: restart the publisher after changing the Prometheus scrape interval.

Every history response includes `scrape_interval_seconds` and `refresh_seconds`, the publisher's refresh cadence for that endpoint, which the browser uses to schedule polling.

### Hour: one entry per probe

```json
{
  "kind": "probes",
  "window_seconds": 3600,
  "scrape_interval_seconds": 15,
  "refresh_seconds": 15,
  "services": [
    {
      "id": "example.org",
      "name": "Example website",
      "observed_success_ratio": 0.9958,
      "coverage": 1,
      "probes": [
        { "at": 1699996415, "success": true, "http_status_code": 200 },
        { "at": 1699996430, "success": false, "http_status_code": 503 }
      ]
    }
  ]
}
```

Each entry is one real Blackbox probe: one `probe_success` sample, stored by one Prometheus scrape of `/probe`. `at` is that scrape's timestamp. `http_status_code` comes from the `probe_http_status_code` sample with the same timestamp. It is `null` when that sample is missing, 0 (no HTTP response) or outside integer 100–599. The publisher reads raw samples with instant queries for `probe_success{job="…"}[1h]` and `probe_http_status_code{job="…"}[1h]`. It does not re-aggregate them.

The page draws one bar per probe. Where the gap between consecutive probes, or since the last probe, reaches 1.5 scrape intervals, it adds a gray **missed** bar for each expected check that never arrived, so gaps keep their place in time.

### Day and month: state timelines

```json
{
  "kind": "timeline",
  "window_seconds": 86400,
  "scrape_interval_seconds": 15,
  "refresh_seconds": 300,
  "step_seconds": 60,
  "services": [
    {
      "id": "example.org",
      "observed_success_ratio": 0.9997,
      "coverage": 0.98,
      "segments": [
        { "start": 1699913700, "end": 1699998000, "state": "healthy", "samples": 5620, "failures": 0, "last_http_status_code": null },
        { "start": 1699998000, "end": 1699998120, "state": "down", "samples": 8, "failures": 3, "last_http_status_code": 503 },
        { "start": 1699998120, "end": 1700000100, "state": "healthy", "samples": 132, "failures": 0, "last_http_status_code": null }
      ]
    }
  ]
}
```

The publisher classifies every `step_seconds` step of the window, then merges consecutive steps with the same state into one segment. Segments are contiguous and cover the whole window, unknown periods included, so a segment's width on the page is proportional to `end − start`.

| State | Rule for each step |
| --- | --- |
| **Down / red** | At least one received check failed. |
| **Healthy / green** | No failures, and at least 80% of the expected checks (`step_seconds / scrape_interval_seconds`) were received. |
| **Unknown / gray** | Anything else, including steps with no checks. |

A real failed probe always marks its step down, even when other checks in that step are missing. `samples` and `failures` total the segment's checks. `last_http_status_code` is the most recent valid HTTP code from the segment's failing steps, and is always `null` for healthy and unknown segments. It is also `null` when label changes leave multiple series with differing last codes in the same step, since their order cannot be established.

Steps are aligned to UTC epoch multiples of `step_seconds`; each excludes its start and includes its end. For each step the publisher runs `sum by (instance) (count_over_time(probe_success[step]))` and `sum by (instance) (sum_over_time(...))` through `query_range`, plus a sparse HTTP-code query restricted to failing steps. Counts are aggregated by target (`instance`) across label changes, so a step crossing a metadata change keeps both sets of checks.

| Window | Duration | Setting | Default step | Default steps |
| --- | --- | --- | --- | --- |
| Day | 24 hours | `day_step_seconds` | 1 minute | 1,440 |
| Month | 30 days | `month_step_seconds` | 30 minutes | 1,440 |

A step must divide its window evenly, be at least the scrape interval, and give at most 10,000 steps. A step of at least twice the scrape interval avoids gray slivers caused by scrape jitter. Shorter steps are finer but produce larger Prometheus responses (each step is one point for each target); raise `prometheus.max_response_bytes` for large inventories with fine steps.

### Summary fields

| Field | Meaning | Example |
| --- | --- | --- |
| `observed_success_ratio` | Fraction of received checks in the window that succeeded; `null` if none were received. | `0.95` means 95% of received checks succeeded, **not** 95% wall-clock uptime. |
| `coverage` | Received checks divided by expected checks (`window_seconds / scrape_interval_seconds`), capped at 1. | `0.5` means about half the expected checks were received. |

Missing or failed scrapes reduce coverage; they are never counted as successful probes.

### Expiry and polling

History expires at its generation time plus **three times its refresh interval**, independently of current data. The browser does not display expired history as fresh history.

The browser schedules each observer’s streams independently, using `refresh_seconds`, snapshot generation times and the measured publication cadence rather than a fixed offset from page load. A short settling margin and bounded one-second retries allow for snapshots still being generated. Failures or expired snapshots fall back to the normal polling interval. “Last checked” still uses each probe’s actual `observed_at` timestamp: synchronizing snapshot retrieval does not make an older probe result newly checked.

### Upgrading from schema version 1

Version 1 history used fixed buckets (`bucket_seconds`, `buckets`) configured with `hour_bucket_seconds`, `day_bucket_seconds` and `month_bucket_seconds`. Those settings have been removed, and leaving them in a configuration file fails startup. `scrape_seconds` and `hour_seconds` are now optional. During deployment skew, the browser still accepts version 1 responses and draws their buckets as a state timeline. A bucket is unknown when its coverage is below 80%, healthy when every check succeeded, and down otherwise.

## Browser policy

### Inventory and metadata priority

The browser combines current inventories from all configured observers, deduplicating by public hostname. It retains the last successful inventory in memory during a publisher outage. If any observer marks a service hidden, the page excludes it everywhere—before search, counts and summaries.

When observers disagree, their order in `locations.json` determines metadata priority:

- **Display name:** use the first configured observer whose current snapshot contains the service.
- **Group:** use the first configured observer containing an explicit group. An older publisher without labels does not suppress a later observer's group.

A service appears only once, even during deployment skew. Retained metadata does not establish fresh health. Historical responses do not change group membership.

### Combined service health

Each service retains a separate observation and history row for every configured observer. Its combined state uses freshness-aware observations:

| Observations | Combined state |
| --- | --- |
| At least one fresh down observation. | **Down** |
| Every configured observer has a fresh healthy observation. | **Healthy** (shown as online in summaries) |
| Anything else, including missing or expired results. | **Unknown** |

A down state means a failure was observed from at least one location, not proof of a worldwide outage. A failed publisher request does not immediately invalidate a still-fresh observation; it becomes unknown when it expires.

The overview counts all page-visible services, regardless of search, collapsed groups or selected history window. Its headline follows these rules after initial loading:

| Condition | Headline |
| --- | --- |
| Empty inventory, or every service unknown. | Service status unavailable |
| Every service healthy. | All services online |
| Every service down. | All services down |
| Some services down, but not all. | Partial outage |
| No services down, but some unknown. | Service status incomplete |

Initial current requests show “Checking service status…”. Configuration errors keep their dedicated error message. Counts always retain unknown states alongside outages.

### Monitoring availability

Monitor cards describe whether a publisher is reachable, not whether its services are healthy. A reachable publisher may still contain expired observations.

Partial monitoring failures stay on the cards. When all locations are unreachable, a separate alert says “Status updates are temporarily unavailable.” Retained fresh observations can still support the service overview while this alert is visible. Monitoring failure never becomes an “All services down” claim.

### Groups and search

- Groups default to expanded; collapse preferences persist across reloads and synchronize across tabs.
- Named groups and services sort alphabetically, with stable identity tie-breakers. Ungrouped services appear last under “Other services”; this fallback is separate from an explicitly named group with the same title.
- Search matches service display names and public hostnames using case-insensitive substring matching, with surrounding query whitespace ignored. It does not match group titles or arbitrary label values.
- Searching reveals matching services in collapsed groups without changing the saved collapse state. Empty groups disappear, but group health and totals still describe the full group, not just matches.
- Ctrl+K or Cmd+K focuses and selects the search text; Escape clears it.
- The animated search placeholder uses only page-visible service names. Hidden services are excluded from placeholders, results and counts.

The browser stores only category collapse preferences in localStorage (`beacon:collapsed-groups`), synchronized across tabs. Searching temporarily reveals matching categories without changing that preference. Invalid or unavailable storage leaves the page usable. The reusable `useLocalStorage(key, initialValue, schema)` hook validates JSON preferences, persists reactive changes and cleans up synchronization listeners on disposal. Monitoring snapshots are never persisted; there is no IndexedDB, service worker or external snapshot fallback.

## Machine-readable schemas

- [Current response](schema/current.schema.json)
- [History response](schema/history.schema.json)
- [Browser locations configuration](schema/locations.schema.json)

Regenerate these from the browser's runtime validators with `pnpm schema`. Structural schema validation is not sufficient on its own: consumers must also apply the freshness rules, duplicate-service-ID rejection and public-label byte limits described above. Inventory loading additionally rejects duplicate normalized public-label keys.
