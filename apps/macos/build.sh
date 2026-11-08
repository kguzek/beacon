#!/usr/bin/env bash
# Builds the macOS menu-bar companion.
#
#   ./build.sh bindings   Rust core -> Generated/ (xcframework + Swift wrappers)
#   ./build.sh app        bindings, then a universal build/Beacon.app
#
# Requires Xcode and: rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -euo pipefail

cd "$(dirname "$0")"
root="$(cd ../.. && pwd)"
generated="$PWD/Generated"
targets=(aarch64-apple-darwin x86_64-apple-darwin)
export MACOSX_DEPLOYMENT_TARGET=13.0

bindings() {
  for target in "${targets[@]}"; do
    cargo build --manifest-path "$root/Cargo.toml" --locked --release \
      -p beacon-ffi --lib --target "$target"
  done

  rm -rf "$generated"
  mkdir -p "$generated/include" "$generated/Sources/BeaconCore" "$generated/tmp"

  # Library mode reads the UniFFI metadata embedded in the built library.
  cargo run --manifest-path "$root/Cargo.toml" --locked --release \
    -p beacon-ffi --features bindgen --bin uniffi-bindgen-swift -- \
    "$root/target/${targets[0]}/release/libbeacon_ffi.dylib" "$generated/tmp" \
    --swift-sources --headers --modulemap \
    --module-name BeaconFFI --modulemap-filename module.modulemap
  mv "$generated/tmp/BeaconFFI.h" "$generated/tmp/module.modulemap" "$generated/include/"
  mv "$generated/tmp/BeaconCore.swift" "$generated/Sources/BeaconCore/"

  local libraries=()
  for target in "${targets[@]}"; do
    libraries+=("$root/target/$target/release/libbeacon_ffi.a")
  done
  lipo -create "${libraries[@]}" -output "$generated/tmp/libbeacon_ffi.a"
  xcodebuild -create-xcframework \
    -library "$generated/tmp/libbeacon_ffi.a" -headers "$generated/include" \
    -output "$generated/BeaconFFI.xcframework" >/dev/null
  rm -rf "$generated/tmp"
  echo "Generated bindings in $generated"
}

app() {
  bindings
  swift build -c release --arch arm64 --arch x86_64
  local bin_path
  bin_path="$(swift build -c release --arch arm64 --arch x86_64 --show-bin-path)"
  local bundle="$PWD/build/Beacon.app"
  rm -rf "$bundle"
  mkdir -p "$bundle/Contents/MacOS"
  cp "$bin_path/BeaconMenu" "$bundle/Contents/MacOS/BeaconMenu"
  cp Info.plist "$bundle/Contents/Info.plist"
  # Ad-hoc signature so the app launches on Apple silicon; not notarized.
  codesign --force --sign - "$bundle"
  echo "Built $bundle"
}

case "${1:-app}" in
  bindings) bindings ;;
  app) app ;;
  *)
    echo "usage: $0 [bindings|app]" >&2
    exit 2
    ;;
esac
