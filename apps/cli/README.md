# beacon-cli

Shows a Beacon status page's current status in [Waybar](https://github.com/Alexays/Waybar), or prints it as JSON for scripts. It contacts the same publishers as the page, listed in the page's `locations.json`, and applies the same freshness and combining rules. History is left to the web page.

```sh
beacon-cli                                    # Waybar JSON for https://status.guzek.uk
beacon-cli --status-page-url https://status.example.org
beacon-cli --json | jq .summary.headline      # full snapshot
```

## Waybar

```jsonc
"custom/beacon": {
  "exec": "beacon-cli",
  "return-type": "json",
  "interval": 60,
  "on-click": "xdg-open https://status.guzek.uk"
}
```

The module text is a green ✓ when every service is online. Otherwise it shows coloured counts: `◐` mixed, `✕` down and `?` unknown, with `✓` for the services still online. The tooltip lists each monitoring location's own counts, followed by the names of services that aren't online.

Each update also sets:

- `class`: `healthy`, `mixed`, `down` or `unknown`, for CSS such as `#custom-beacon.down { … }`.
- `alt`: `all_online`, `mixed`, `partial_outage`, `all_down`, `incomplete` or `unavailable`, for `format-icons`.

If the status page can't be read, the module shows `?` and puts the error in its tooltip. The command still exits successfully, so Waybar keeps the module. With `--json`, errors go to stderr and the exit status is nonzero.

## Build

```sh
cargo build --release -p beacon-cli
```

Static Linux (x86_64 and aarch64 musl) and universal macOS executables are attached to `cli-v*` releases.
