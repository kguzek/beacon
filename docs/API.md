# Beacon API v1

Each Beacon publisher reads a service inventory and periodically collects results from Prometheus. The API serves those cached results; opening the status page or calling an endpoint never triggers a monitoring query.

The API is public. Private target URLs, credentials and unrelated scrape labels are not included in responses. Publishers and the browser keep snapshots only in memory, not on disk.

## Configuration references

- [Publisher settings](../examples/config.toml): Prometheus connection, inventory path, HTTP server, refresh intervals and freshness limits.
- [Service inventory example](../examples/services.yaml): service names, visibility and grouping labels.
- [Startup arguments](../README.md#publisher-configuration): use `--config.path` to select or layer configuration files. With no arguments, the publisher reads `/etc/beacon/config.toml`.
- [Browser locations](../web/public/locations.json): the monitoring locations the page contacts, in priority order.

For example, start a publisher with a base configuration and local overrides:

```sh
beacon-publisher --config.path config.toml --config.path config.local.toml
```

Configuration and inventory are loaded at startup. Restart or recreate the publisher after editing either file. Unknown TOML settings fail startup.

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
| `beacon_hosting_location` | `beacon_hosting_location: "primary"` | Absent | Adds a public “hosted at” description. It does not choose which observers monitor the service or how it is grouped. |
| `beacon_label_group` | `beacon_label_group: "Observability"` | Absent | Places the service in a named, collapsible group on the page. |
| `beacon_label_<key>` | `beacon_label_team: "Platform"` | Absent | Publishes additional organizational metadata under `labels`, without the `beacon_label_` prefix. Only `group` currently affects page organization. |

`beacon_publish` and `beacon_hidden` accept only the exact strings `"true"` and `"false"`. Publication opt-out takes precedence over page visibility.

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
| `/v1/history/hour` | Last hour, in 30-second buckets. | 30 seconds |
| `/v1/history/day` | Last 24 hours, in 15-minute buckets. | 5 minutes |
| `/v1/history/month` | Last 30 days, in 6-hour buckets. | 30 minutes |
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
- Upstream requests have timeouts and streamed byte limits. Each serialized snapshot is limited to 32 MiB; each history service has at most 1,000 buckets.

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

All three history endpoints use the same envelope and inventory metadata as `/v1/current`. They replace current observation fields with historical summaries and buckets.

The following is an excerpt; a real response also includes `schema_version`, `location`, `generated_at` and `expires_at`, plus every bucket, including empty ones:

```json
{
  "window_seconds": 86400,
  "bucket_seconds": 900,
  "services": [
    {
      "id": "example.org",
      "name": "Example website",
      "hidden": false,
      "hosting_location": null,
      "labels": { "group": "Websites" },
      "observed_success_ratio": 0.95,
      "coverage": 0.5,
      "buckets": [
        {
          "ends_at": 1700000100,
          "observed_success_ratio": 0.95,
          "coverage": 0.5,
          "samples": 15,
          "last_http_status_code": 200
        }
      ]
    }
  ]
}
```

### Reading the numbers

| Field | Meaning | Example |
| --- | --- | --- |
| `observed_success_ratio` | Fraction of received checks that succeeded; `null` if none were received. Service summaries are weighted by sample count. | `0.95` means 95% of received checks succeeded, **not** 95% wall-clock uptime. |
| `coverage` | Received checks divided by expected checks, capped at 1. | `0.5` means approximately half the expected checks were received. |
| `samples` | Number of probe samples received in a bucket. | `15` means 15 checks contributed to that bucket. |
| `ends_at` | End of the bucket, in Unix seconds. | A 900-second bucket covers the preceding 15 minutes. |
| `last_http_status_code` | Most recent valid HTTP response code in the bucket. | `503` means the last recorded response was 503, not that every check returned 503. |

Coverage is `min(samples / (bucket_seconds / scrape_seconds), 1)`; service summary coverage uses the whole window. It is an estimate: scrape timing jitter, configuration changes and gaps affect it. Missing or failed scrapes reduce coverage; they are never counted as successful probes.

`last_http_status_code` is `null` for empty buckets, missing metrics, no HTTP response (code 0), or values outside integer 100–599. Older publishers may omit this field. Historical health is based on success ratio and coverage, not the HTTP code alone.

### Windows, buckets and expiry

| Window | Duration | Default bucket width | Default bucket count |
| --- | --- | --- | --- |
| Hour | 1 hour | 30 seconds | 120 |
| Day | 24 hours | 15 minutes | 96 |
| Month | 30 days | 6 hours | 120 |

Window durations and routes are fixed; bucket widths are configurable at startup. For one check per hour-view bucket, set `hour_bucket_seconds` to the actual `scrape_seconds` (both default to 30).

Only completed buckets are queried, aligned to UTC epoch multiples. Each bucket excludes its start and includes its end. The publisher runs `avg_over_time(probe_success[width])`, `count_over_time(...)` and `last_over_time(probe_http_status_code[width])` through Prometheus `query_range` for the configured job.

History expires at its generation time plus **three times its refresh interval**, independently of current data. The browser does not display expired history as fresh history.

History-bar colors mean:

- **Unknown / gray:** coverage is below 80%, or no success ratio is available.
- **Healthy / green:** sufficient coverage and every received check succeeded.
- **Down / red:** sufficient coverage and at least one received check failed.

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
