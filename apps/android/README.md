# Beacon for Android

Keeps a Beacon status page's current verdict in the status bar. Tap the notification to open the page; expand it to see each monitoring location's counts and the services it doesn't currently confirm as healthy.

The status bar shows notification icons in one colour, so each verdict has its own shape:

| Icon | Chip | Meaning |
| --- | --- | --- |
| ✓ | `OK` | All services online |
| half beacon | `2 mixed` | Service status mixed: some locations confirm services working, others don't |
| ✕ | `2 down` | Partial outage |
| ! | `All down` | All services down |
| ? | `5 ?`, `?` | Status incomplete, or the page can't be read |
| … | `…` | Checking status |

On Android 16 and later, the notification is a live update with a status-bar chip showing the icon and short text. The settings screen can switch the chip to icon only or turn it off, and can hide the notification while everything is online.

On first launch, enter the status page's URL and tap **Test**, then **Save**. Beacon checks every minute by default (30 seconds to 5 minutes), and at most every 5 minutes while the screen is off. In deep sleep, Android delays checks further, to roughly every 10–15 minutes; the notification shows when the status was last checked. The settings screen offers shortcuts when notifications, live updates, exact alarms or unrestricted battery use are missing.

## Build

The status logic comes from the shared Rust crate `beacon-core`, exposed to Kotlin through UniFFI (`crates/beacon-ffi`). This requires JDK 17 or later, the Android SDK (`ANDROID_HOME`) with the NDK version pinned in `gradle/libs.versions.toml`, [cargo-ndk](https://github.com/bbqsrc/cargo-ndk), and the Android Rust targets:

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk --locked
./build.sh bindings      # Rust core -> generated/ (jniLibs, Kotlin bindings, host library for tests)
./gradlew spotlessCheck lint testDebugUnitTest assembleDebug
./build.sh apk           # per-ABI release APKs in app/build/outputs/apk/release/
```

Run `./build.sh bindings` again after changing `crates/beacon-core` or `crates/beacon-ffi`. Gradle runs on the JDK pinned in `gradle/gradle-daemon-jvm.properties` and downloads it if it isn't installed.

Release APKs are unsigned unless `BEACON_KEYSTORE`, `BEACON_KEYSTORE_PASSWORD`, `BEACON_KEY_ALIAS` and `BEACON_KEY_PASSWORD` are set. Signed per-ABI APKs are attached to `android-v*` releases on GitHub; the tag must match `appVersion` in `app/build.gradle.kts`. CD reads the keystore from the `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD` secrets.

### Reproducible builds

Builds are reproducible, so F-Droid can ship APKs with our signature. Same inputs give byte-identical APKs:

- Rust 1.96.0, `--locked`, stripped release libraries, and source paths remapped to `/beacon`, `/cargo` and `/rustc/<commit>`.
- The NDK version in `gradle/libs.versions.toml`, used by both cargo-ndk and Gradle.
- The Gradle distribution (by checksum) and daemon JDK (Temurin 21) are pinned.
- No VCS info or Google-encrypted dependency metadata in the APK.

Each ABI has its own APK and version code: `versionCode = (major × 10000 + minor × 100 + patch) × 10 + abi`, with 1 for armeabi-v7a, 2 for arm64-v8a and 3 for x86_64.

## Notes

- There's no Google Play build. Outside Play, the app can use `USE_EXACT_ALARM` and ask for unrestricted battery use.
- There's no foreground service. A chain of exact alarms re-posts the notification, and a WorkManager job every 15 minutes restarts the chain if it was lost.
- Requests go through Rust's `reqwest` with bundled web PKI roots, so user-installed CAs aren't trusted, and Android's cleartext policy doesn't apply: `http://` pages work.
