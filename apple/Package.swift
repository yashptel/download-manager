// swift-tools-version: 6.0
import PackageDescription

// The Rust core is linked as a static library inside an XCFramework, and its
// UniFFI Swift bindings are generated into Sources/OpenDMCore. Both come
// from scripts/build-macos.sh; run it once before building here.
let package = Package(
    name: "OpenDM",
    platforms: [.macOS(.v13)],
    targets: [
        .binaryTarget(name: "opendm_coreFFI", path: "Frameworks/opendm_coreFFI.xcframework"),
        .target(name: "OpenDMCore", dependencies: ["opendm_coreFFI"], path: "Sources/OpenDMCore"),
        .executableTarget(name: "OpenDM", dependencies: ["OpenDMCore"], path: "Sources/OpenDM"),
    ],
    swiftLanguageModes: [.v5]
)
