# Releasing OpenDM

A release is a version number, a tag and a draft GitHub release with the
Windows installer, the macOS disk image and the browser extension package.

## Cut a release

1. Set the version in `Cargo.toml` (`[workspace.package] version`). The
   Windows app, its installer and the macOS app's `Info.plist` take it from
   there. The extension has its own version in `extension/manifest.json`;
   bump it when the extension changed.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. The [Release workflow](../.github/workflows/release.yml) builds
   `OpenDM-<version>-x64-setup.exe`, `OpenDM-<version>-arm64-setup.exe`,
   `OpenDM-<version>.dmg` and `opendm-connect-<version>.zip` and attaches them
   to a draft release.
4. Try the installer and the disk image, edit the notes, and publish the
   draft. Running the workflow by hand from the Actions tab builds the same
   files without a release.

The same files build locally:

```sh
scripts/package-windows.ps1     # build\windows\OpenDM-<version>-x64-setup.exe
scripts/package-windows.ps1 -Arch arm64   # needs MSVC's ARM64 build tools
scripts/package-macos.sh        # build/OpenDM-<version>.dmg
python3 scripts/package-extension.py   # build/extension/opendm-connect-<version>.zip
```

## Updates

Both apps ask GitHub for the latest published release when they start and
once a day, and offer a newer one: Windows in a bar in its window, macOS in a
sheet with Download, Later and Skip This Version (shown when the window is).
Settings can check on demand or turn the daily check off. Drafts and
pre-releases are not offered. The check asks
`api.github.com/repos/yashptel/download-manager/releases/latest` without
signing in, so it finds nothing while the repository is private. Neither app
installs updates itself; [Sparkle](https://sparkle-project.org) would add
that on macOS, with a signing key and an appcast feed.

The installer upgrades in place: it stops a running OpenDM from the same
folder, replaces the files and keeps the history, settings and logins.

## Signing

Unsigned builds work, but Windows SmartScreen warns about an unknown
publisher and macOS refuses to open an app that is not notarized unless the
user overrides it in System Settings.

**Windows.** `scripts\package-windows.ps1` signs the app's binaries, setup
and its uninstaller, with an RFC 3161 timestamp, when `OPENDM_SIGN_CERT`
holds the thumbprint of a code-signing certificate in
`Cert:\CurrentUser\My` (`OPENDM_SIGN_TIMESTAMP` overrides the timestamp
server). That includes certificates whose key sits on a token or in a cloud
key service that installs into the store. In GitHub Actions the workflow
imports a `.pfx` from the `WINDOWS_CERTIFICATE` and
`WINDOWS_CERTIFICATE_PASSWORD` secrets; a certificate that cannot be
exported needs the signing service's own step instead.

**macOS.** Needs an Apple Developer Program membership.
`CODESIGN_IDENTITY` ("Developer ID Application: …") signs the app and the
disk image; `NOTARY_PROFILE` (a profile saved with `xcrun notarytool
store-credentials`) or `NOTARY_KEY`, `NOTARY_KEY_ID` and `NOTARY_ISSUER`
(an App Store Connect API key) notarize the image and staple the ticket. The
workflow reads them from the `APPLE_DEVELOPER_ID_P12_BASE64` (base64 .p12),
`APPLE_DEVELOPER_ID_P12_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY_P8`
(the .p8 text, or base64), `APPLE_API_KEY_ID` and `APPLE_API_ISSUER` secrets.

## The browser extension

`scripts/package-extension.py` writes two packages. Both leave out the
manifest's `key`, which only fixes the ID of the unpacked development copy.

- `opendm-connect-<version>.zip` for Edge.
- `opendm-connect-<version>-firefox.zip` for addons.mozilla.org, without the
  `service_worker` key Firefox ignores.
- `opendm-connect-<version>-chrome.zip` for the Chrome Web Store (item
  `mjebgoilgooegcpfbmjnapamjpcnhcbg`). It leaves YouTube's videos alone,
  as that store's policy requires: the packager fills
  `MEDIA_BLOCKED_HOSTS` in `background.js` and `content.js`.

- **Firefox** (addons.mozilla.org): the add-on keeps the ID
  `connect@opendm.org` from its manifest, which the native host already
  allows. `npx web-ext lint --source-dir extension` runs the store's
  checks.
- **Chrome Web Store and Edge Add-ons** each assign their own extension ID.
  After the first upload, add both IDs to `CHROMIUM_EXTENSION_IDS` in
  `host/src/lib.rs`, so the native host lets the store copies in, and ship
  apps with that change before announcing the extension.
- The store listings ask what the permissions are for and how data is
  used: the extension sends download addresses, cookies and headers only to
  the OpenDM app on the same computer.

