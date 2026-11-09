// swift-tools-version:5.9
// Run ./build.sh first: it builds the Rust core into Generated/.
import PackageDescription

let package = Package(
    name: "BeaconMenu",
    platforms: [.macOS(.v13)],
    targets: [
        // Static Rust library (crates/beacon-ffi) plus its C header.
        .binaryTarget(name: "BeaconFFI", path: "Generated/BeaconFFI.xcframework"),
        // UniFFI-generated Swift wrappers around BeaconFFI.
        .target(
            name: "BeaconCore",
            dependencies: ["BeaconFFI"],
            path: "Generated/Sources/BeaconCore",
            linkerSettings: [
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
        .executableTarget(
            name: "BeaconMenu",
            dependencies: ["BeaconCore"],
            path: "Sources/BeaconMenu"
        ),
        .testTarget(
            name: "BeaconMenuTests",
            dependencies: ["BeaconMenu", "BeaconCore"],
            path: "Tests/BeaconMenuTests"
        ),
    ]
)
