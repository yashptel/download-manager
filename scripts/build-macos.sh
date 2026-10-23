#!/bin/sh
# Builds build/OpenDM.app: the Rust core, its Swift bindings, the AppKit app
# and the pinned aria2-next engine.
#
#   scripts/build-macos.sh               this Mac's architecture
#   scripts/build-macos.sh --universal   arm64 + x86_64
#
# Set CODESIGN_IDENTITY to sign with a Developer ID, timestamped as
# notarization requires; the default is ad hoc. scripts/package-macos.sh
# makes the disk image and notarizes it.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

archs=$(uname -m)
if [ "${1:-}" = "--universal" ]; then
    archs="arm64 x86_64"
fi
# MACOSX_DEPLOYMENT_TARGET is deliberately unset: it also applies to host
# proc-macros, which the Xcode 27 linker then emits in a form dyld rejects.
# Rust's default targets (macOS 11 arm64, 10.12 x86_64) are below the app's
# 13; bundled C code (SQLite) gets the minimum through its own flags.
export CFLAGS_aarch64_apple_darwin="-mmacosx-version-min=13.0"
export CFLAGS_x86_64_apple_darwin="-mmacosx-version-min=13.0"

work="$root/build/macos"
rm -rf "$work"
mkdir -p "$work/headers"

libraries=""
engines=""
for arch in $archs; do
    case "$arch" in
        arm64) triple=aarch64-apple-darwin ;;
        x86_64) triple=x86_64-apple-darwin ;;
        *) echo "Unsupported architecture $arch" >&2; exit 1 ;;
    esac
    cargo build --release -p opendm-core -p opendm-host --target "$triple"
    libraries="$libraries target/$triple/release/libopendm_core.a"
    hosts="${hosts:-} target/$triple/release/opendm-host"
    scripts/fetch-engine.sh "macos-$arch" > /dev/null
    engines="$engines vendor/aria2-next/bin/aria2-next-macos-$arch"
    bindings_library="target/$triple/release/libopendm_core.dylib"
done
lipo -create $libraries -output "$work/libopendm_core.a"
lipo -create $engines -output "$work/aria2-next"
lipo -create $hosts -output "$work/opendm-host"

# Swift bindings and the XCFramework the Swift package links against.
cargo run -q -p uniffi-bindgen -- generate --library "$bindings_library" --language swift --out-dir "$work/bindings"
cp "$work/bindings/opendm_coreFFI.h" "$work/headers/"
cp "$work/bindings/opendm_coreFFI.modulemap" "$work/headers/module.modulemap"
cp "$work/bindings/opendm_core.swift" apple/Sources/OpenDMCore/opendm_core.swift
rm -rf apple/Frameworks/opendm_coreFFI.xcframework
xcodebuild -create-xcframework -library "$work/libopendm_core.a" -headers "$work/headers" \
    -output apple/Frameworks/opendm_coreFFI.xcframework > /dev/null

# SwiftPM links through clang with --sysroot, so clang records the deployment
# target as the SDK version and AppKit runs the app in macOS 13 compatibility
# mode (old chrome, misaligned sidebar toolbar). -isysroot makes it read the
# real SDK version.
sdk=$(xcrun --sdk macosx --show-sdk-path)
# One build per architecture, joined with lipo: given several --arch, SwiftPM
# hands the build to Xcode's build system, which then cannot find the static
# library inside the XCFramework (seen with Xcode 26.6).
executables=""
for arch in $archs; do
    swift_flags="--triple $arch-apple-macosx13.0 -Xswiftc -Xclang-linker -Xswiftc -isysroot -Xswiftc -Xclang-linker -Xswiftc $sdk"
    swift build -c release --package-path apple $swift_flags
    executables="$executables $(swift build -c release --package-path apple $swift_flags --show-bin-path)/OpenDM"
done
lipo -create $executables -output "$work/OpenDM"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app="$root/build/OpenDM.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$work/OpenDM" "$app/Contents/MacOS/OpenDM"
cp "$work/aria2-next" "$app/Contents/MacOS/aria2-next"
cp "$work/opendm-host" "$app/Contents/MacOS/opendm-host"
cp LICENSE "$app/Contents/Resources/LICENSE"
cp apple/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
cp apple/MenuBar*.png "$app/Contents/Resources/"
cp apple/DockProgress*.png "$app/Contents/Resources/"
cp vendor/aria2-next/bin/COPYING "$app/Contents/Resources/aria2-next-COPYING"
sed "s/@VERSION@/$version/g" apple/Info.plist > "$app/Contents/Info.plist"

identity=${CODESIGN_IDENTITY:--}
timestamp=--timestamp
[ "$identity" = "-" ] && timestamp=--timestamp=none
codesign --force --options runtime $timestamp --sign "$identity" "$app/Contents/MacOS/aria2-next"
codesign --force --options runtime $timestamp --sign "$identity" "$app/Contents/MacOS/opendm-host"
codesign --force --options runtime $timestamp --sign "$identity" "$app"
echo "$app"
