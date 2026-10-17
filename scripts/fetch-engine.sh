#!/bin/sh
# Downloads the pinned aria2-next build for a platform and verifies its SHA-256.
# Usage: scripts/fetch-engine.sh [macos-arm64|macos-x86_64|windows-x86_64.exe|windows-arm64.exe]
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
vendor="$root/vendor/aria2-next"
version=$(cat "$vendor/VERSION")

platform=${1:-}
if [ -z "$platform" ]; then
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) platform=macos-arm64 ;;
        Darwin-x86_64) platform=macos-x86_64 ;;
        *) echo "Pass a platform: macos-arm64, macos-x86_64, windows-x86_64.exe or windows-arm64.exe" >&2; exit 1 ;;
    esac
fi

asset="aria2-next-$version-$platform"
expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$vendor/checksums.sha256")
if [ -z "$expected" ]; then
    echo "No checksum for $asset in vendor/aria2-next/checksums.sha256" >&2
    exit 1
fi

mkdir -p "$vendor/bin"
target="$vendor/bin/aria2-next-$platform"
if [ -f "$target" ] && [ "$(shasum -a 256 "$target" | cut -d' ' -f1)" = "$expected" ]; then
    exit 0
fi

base="https://github.com/AnInsomniacy/aria2-next/releases/download/v$version"
curl -fsSL -o "$target.part" "$base/$asset"
actual=$(shasum -a 256 "$target.part" | cut -d' ' -f1)
if [ "$actual" != "$expected" ]; then
    echo "Checksum mismatch for $asset: expected $expected, got $actual" >&2
    exit 1
fi
mv "$target.part" "$target"
chmod +x "$target"
curl -fsSL -o "$vendor/bin/COPYING" "https://raw.githubusercontent.com/AnInsomniacy/aria2-next/v$version/COPYING"
echo "$target"
