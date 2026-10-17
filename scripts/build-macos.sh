#!/bin/sh
# Builds build/OpenDM.app: the Rust core, its Swift bindings, the AppKit app
# and the pinned aria2-next engine.
#
#   scripts/build-macos.sh               this Mac's architecture
#   scripts/build-macos.sh --universal   arm64 + x86_64
#
# Set CODESIGN_IDENTITY to sign with a Developer ID; the default is ad hoc.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

archs=$(uname -m)
if [ "${1:-}" = "--universal" ]; then
    archs="arm64 x86_64"
fi
export MACOSX_DEPLOYMENT_TARGET=13.0

work="$root/build/macos"
rm -rf "$work"
mkdir -p "$work/headers"

libraries=""
engines=""
swift_archs=""
for arch in $archs; do
    case "$arch" in
        arm64) triple=aarch64-apple-darwin ;;
        x86_64) triple=x86_64-apple-darwin ;;
        *) echo "Unsupported architecture $arch" >&2; exit 1 ;;
    esac
    cargo build --release -p opendm-core --target "$triple"
    libraries="$libraries target/$triple/release/libopendm_core.a"
    scripts/fetch-engine.sh "macos-$arch" > /dev/null
    engines="$engines vendor/aria2-next/bin/aria2-next-macos-$arch"
    swift_archs="$swift_archs --arch $arch"
    bindings_library="target/$triple/release/libopendm_core.dylib"
done
lipo -create $libraries -output "$work/libopendm_core.a"
lipo -create $engines -output "$work/aria2-next"

# Swift bindings and the XCFramework the Swift package links against.
cargo run -q -p uniffi-bindgen -- generate --library "$bindings_library" --language swift --out-dir "$work/bindings"
cp "$work/bindings/opendm_coreFFI.h" "$work/headers/"
cp "$work/bindings/opendm_coreFFI.modulemap" "$work/headers/module.modulemap"
cp "$work/bindings/opendm_core.swift" apple/Sources/OpenDMCore/opendm_core.swift
rm -rf apple/Frameworks/opendm_coreFFI.xcframework
xcodebuild -create-xcframework -library "$work/libopendm_core.a" -headers "$work/headers" \
    -output apple/Frameworks/opendm_coreFFI.xcframework > /dev/null

swift build -c release --package-path apple $swift_archs
binaries=$(swift build -c release --package-path apple $swift_archs --show-bin-path)

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app="$root/build/OpenDM.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binaries/OpenDM" "$app/Contents/MacOS/OpenDM"
cp "$work/aria2-next" "$app/Contents/MacOS/aria2-next"
cp LICENSE "$app/Contents/Resources/LICENSE"
cp vendor/aria2-next/bin/COPYING "$app/Contents/Resources/aria2-next-COPYING"
sed "s/@VERSION@/$version/g" apple/Info.plist > "$app/Contents/Info.plist"

identity=${CODESIGN_IDENTITY:--}
codesign --force --options runtime --sign "$identity" "$app/Contents/MacOS/aria2-next"
codesign --force --options runtime --sign "$identity" "$app"
echo "$app"
