# Beacon

Beacon turns existing Prometheus and Blackbox Exporter checks into a public status page. It shows how services are doing now and over time, without exposing your monitoring setup or running a second system to check the same services.

## How it works

A small publisher runs at each monitoring location. It reads the service list already used by Prometheus, collects results on a schedule, and makes selected status information available through a public API. Private target details and unrelated metrics stay private, and page visits don’t trigger extra Prometheus queries.

The status page brings those results together in the browser, with searchable services and optional collapsible groups from inventory labels. Each service retains detail from every monitoring location. Any fresh failure makes the combined state down; online requires fresh confirmation from every location. Old or missing results are unknown rather than outdated healthy or down states.

The page itself is entirely static, so it can live on a CDN or any static hosting service. There’s no central application server or separate database to maintain.

## Stack

- **Frontend:** Vue 3, Vite, and TypeScript, managed with pnpm.
- **Publisher:** Rust with Axum and Tokio.
- **Monitoring:** Your existing Prometheus and Blackbox Exporter.
- **Containers:** OCI images, with Containerfiles for both components.

## Repository

- `web/` — the public status page.
- `publisher/` — the status API deployed at each monitoring location.
- `examples/` — sample publisher configuration and service inventory.
- `compose.yaml` — a deployment template for the publisher.

To use Beacon, configure each publisher with its Prometheus address and shared service inventory, then add the public publisher URLs to `web/public/locations.json`. Build the page with `pnpm build` and deploy `web/dist/` to your static host.

See the [API documentation](docs/API.md) for configuration options and response formats.

## Publisher containers

Publisher images are available at `ghcr.io/<owner>/<repository>/publisher:latest` for Linux AMD64 and 32-bit ARMv7 (`armv7l`). Your container engine selects the matching architecture automatically. Commit-specific tags are also available; ARM64 is not currently supported.

## Publisher configuration

The publisher reads `/etc/beacon/config.toml` by default. Repeat `--config.path` to layer local overrides over a base configuration:

```sh
beacon-publisher --config.path config.toml --config.path config.local.toml
```

Files are merged in order: nested tables are merged, while later values (including entire arrays) replace earlier ones. Override files can contain only the fields they change. Defaults and validation apply to the final merged configuration, and the service inventory is loaded from its resulting `services.path`. Explicit paths replace the default path; every specified file must exist and contain valid TOML.

## Publisher logs

The publisher writes readable logs to stderr at `info` level by default (colorized in a terminal). Set `RUST_LOG` to override the filter, for example `RUST_LOG=beacon_publisher=debug`.

Logs cover startup/configuration, HTTP listener availability, initial snapshots and readiness, refresh failure/recovery transitions, and graceful shutdown. Repeated successful refreshes and consecutive failures are silent. Each snapshot stream (`current`, `day`, `month`) reports its first failure with a safe diagnostic and initial retry delay; retries continue with backoff until recovery. A retained snapshot may expire during an outage even though the publisher's readiness remains latched. Upstream URLs, credentials, response bodies, and raw error chains are not logged.
