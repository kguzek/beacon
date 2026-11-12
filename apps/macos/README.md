# Beacon for macOS

A menu-bar companion for a Beacon status page. Its icon shows the page's overall verdict, using the same colours as the web page:

| Icon | Meaning |
| --- | --- |
| green Beacon logo | All services online |
| half green, half hollow amber Beacon logo | Service status mixed: some locations confirm services working, others don't |
| red cross | Partial outage or all services down |
| grey question mark | Status incomplete or unavailable |
| grey Beacon logo | Checking status |

Click the icon to see each monitoring location's own counts and the services it doesn't currently confirm as healthy.

It polls `https://status.guzek.uk` every 60 seconds. To use another page:

```sh
defaults write uk.guzek.beacon.menu statusPageURL https://status.example.org
```

## Build

The status logic comes from the shared Rust crate `beacon-core`, exposed to Swift through UniFFI (`crates/beacon-ffi`). This requires Xcode and Rust, plus both macOS targets:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
./build.sh bindings   # Rust core -> Generated/ (xcframework + Swift wrappers)
swift test
./build.sh app        # universal build/Beacon.app, ad-hoc signed
```

Run `./build.sh bindings` again after changing `crates/beacon-core` or `crates/beacon-ffi`. Zipped app bundles are attached to `macos-v*` releases. They aren't notarized, so open the app with right-click → Open the first time.
