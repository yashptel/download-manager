#!/bin/sh
# Builds build/OpenDM-<version>.dmg: the universal OpenDM.app in a disk image
# with a link to Applications.
#
# Signing and notarization, all optional:
#   CODESIGN_IDENTITY   "Developer ID Application: …"; signs the app (through
#                       build-macos.sh) and the image. The default is ad hoc.
#   NOTARY_PROFILE      a notarytool keychain profile (xcrun notarytool
#                       store-credentials), or
#   NOTARY_KEY, NOTARY_KEY_ID, NOTARY_ISSUER
#                       an App Store Connect API key (.p8 path), its ID and
#                       issuer, as CI uses.
# With either, the image is notarized and the ticket stapled to it.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

scripts/build-macos.sh --universal
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app="$root/build/OpenDM.app"
dmg="$root/build/OpenDM-$version.dmg"
stage="$root/build/dmg"
rm -rf "$stage" "$dmg"
mkdir -p "$stage"
ditto "$app" "$stage/OpenDM.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname OpenDM -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg" > /dev/null
rm -rf "$stage"

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$dmg"
fi
if [ -n "${NOTARY_PROFILE:-}" ]; then
    xcrun notarytool submit "$dmg" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$dmg"
elif [ -n "${NOTARY_KEY:-}" ]; then
    xcrun notarytool submit "$dmg" --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER" --wait
    xcrun stapler staple "$dmg"
fi
echo "$dmg"

