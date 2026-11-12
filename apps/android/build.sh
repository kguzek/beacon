#!/usr/bin/env bash
# Builds the Android companion.
#
#   ./build.sh bindings   Rust core -> generated/ (jniLibs, Kotlin bindings, host library for tests)
#   ./build.sh apk        bindings, then per-ABI release APKs in app/build/outputs/apk/release/
#
# Requires JDK 17+, the Android SDK (ANDROID_HOME) with the NDK version pinned
# in gradle/libs.versions.toml, cargo-ndk, and:
#   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
set -euo pipefail

cd "$(dirname "$0")"
root="$(cd ../.. && pwd)"
generated="$PWD/generated"
abis=(arm64-v8a armeabi-v7a x86_64)
# Matches minSdk in app/build.gradle.kts.
min_sdk=28

ndk_version="$(sed -n 's/^ndk = "\(.*\)"$/\1/p' gradle/libs.versions.toml)"
export ANDROID_NDK_HOME="${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}/ndk/$ndk_version"
if [[ ! -d "$ANDROID_NDK_HOME" ]]; then
  echo "NDK $ndk_version not installed: sdkmanager --install 'ndk;$ndk_version'" >&2
  exit 1
fi

bindings() {
  if ! command -v cargo-ndk > /dev/null; then
    echo "cargo-ndk not found: install it (e.g. cargo install cargo-ndk --locked)" >&2
    exit 1
  fi

  # Reproducible builds: keep machine-specific paths out of the libraries.
  # With the rust-src component installed, std's paths point into the local
  # toolchain; map them back to what they are without it.
  # Scoped to the Android targets, so host builds keep their own cache.
  local sysroot commit
  sysroot="$(rustc --print sysroot)"
  commit="$(rustc -vV | sed -n 's/^commit-hash: //p')"
  local remap="--remap-path-prefix=$root=/beacon"
  remap+=" --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
  remap+=" --remap-path-prefix=$sysroot/lib/rustlib/src/rust=/rustc/$commit"
  local triple
  for triple in AARCH64_LINUX_ANDROID ARMV7_LINUX_ANDROIDEABI X86_64_LINUX_ANDROID; do
    export "CARGO_TARGET_${triple}_RUSTFLAGS=$remap"
  done

  rm -rf "$generated"
  local targets=()
  for abi in "${abis[@]}"; do targets+=(-t "$abi"); done
  cargo ndk --manifest-path "$root/Cargo.toml" "${targets[@]}" -P "$min_sdk" \
    -o "$generated/jniLibs" build --locked --release -p beacon-ffi --lib

  # Host library, unstripped (dev profile): JVM unit tests call the real Rust
  # core through it, and UniFFI's library mode reads the bindings' metadata
  # from its symbols, which the stripped release libraries no longer have.
  # The metadata is the same for every target.
  cargo build --manifest-path "$root/Cargo.toml" --locked -p beacon-ffi --lib
  local host_lib=libbeacon_ffi.so
  [[ "$(uname)" == Darwin ]] && host_lib=libbeacon_ffi.dylib
  mkdir -p "$generated/host"
  cp "$root/target/debug/$host_lib" "$generated/host/"

  cargo run --manifest-path "$root/Cargo.toml" --locked \
    -p beacon-ffi --features bindgen --bin uniffi-bindgen -- \
    generate --library "$generated/host/$host_lib" \
    --language kotlin --out-dir "$generated/kotlin" --no-format
  echo "Generated bindings in $generated"
}

apk() {
  bindings
  ./gradlew --no-daemon assembleRelease
  echo "Built $PWD/app/build/outputs/apk/release/"
}

case "${1:-apk}" in
  bindings) bindings ;;
  apk) apk ;;
  *)
    echo "usage: $0 [bindings|apk]" >&2
    exit 2
    ;;
esac
