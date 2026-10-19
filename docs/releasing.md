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
   `OpenDM-<version>-x64-setup.exe`, `OpenDM-<version>.dmg` and
   `opendm-connect-<version>.zip` and attaches them to a draft release.
4. Try the installer and the disk image, edit the notes, and publish the
   draft. Running the workflow by hand from the Actions tab builds the same
   files without a release.

The same files build locally:

```sh
scripts/package-windows.ps1     # build\windows\OpenDM-<version>-x64-setup.exe
scripts/package-macos.sh        # build/OpenDM-<version>.dmg
python3 scripts/package-extension.py   # build/extension/opendm-connect-<version>.zip
```

## Updates

The Windows app asks GitHub for the latest published release when it
starts and once a day, and shows a newer one in its window with a link to
the release; Settings can check on demand or turn the daily check off.
Drafts and pre-releases are not offered. The check asks
`api.github.com/repos/yashptel/download-manager/releases/latest` without
signing in, so it finds nothing while the repository is private. The macOS
app has no update check yet; [Sparkle](https://sparkle-project.org) is the
usual way to add one.

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
workflow reads them from the `MACOS_CERTIFICATE` (base64 .p12),
`MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGN_IDENTITY`, `NOTARY_KEY` (base64
.p8), `NOTARY_KEY_ID` and `NOTARY_ISSUER` secrets.

## The browser extension

`opendm-connect-<version>.zip` is the package for all three stores. It
leaves out the manifest's `key`, which only fixes the ID of the unpacked
development copy.

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

