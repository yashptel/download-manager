# <img src="docs/icon.svg" width="40" height="40" align="top" alt=""> OpenDM

An open-source download manager for macOS and Windows: IDM's browser-capture
workflow and dialogs, with native UIs on each platform. OpenDM is a working name.

| Path | What it is |
| --- | --- |
| `core/` | Rust core shared by both apps through UniFFI: aria2-next process supervision, JSON-RPC over WebSocket, task list, SQLite history, settings and file-type categories |
| `apple/` | macOS app (Swift, AppKit) as a Swift package |
| `windows/` | Windows app (C#, WinUI 3, unpackaged); see [windows/README.md](windows/README.md) |
| `vendor/aria2-next/` | Pinned engine version and checksums; binaries are downloaded, not committed |
| `scripts/` | Build, engine fetch, icon and test-server scripts |
| `docs/` | Research notes, design decisions and the icon |

The browser extension and the native messaging host come in later milestones;
see [docs/research.md](docs/research.md).

The icon is drawn by `scripts/make-icons.py` (Python with Pillow), which writes
`docs/icon.svg`, the Windows app icon, title-bar PNGs and light/dark tray icons,
and the macOS `.icns`.

## Build the macOS app

Requires Xcode 16 or later and a Rust toolchain with the `aarch64-apple-darwin`
and `x86_64-apple-darwin` targets.

```sh
scripts/build-macos.sh              # build/OpenDM.app for this Mac
scripts/build-macos.sh --universal  # arm64 + x86_64
```

The script downloads the pinned aria2-next build and checks its SHA-256,
builds the Rust core, generates the Swift bindings and an XCFramework, builds
the Swift package and assembles a signed (ad hoc by default) app bundle. After
one run, `apple/Package.swift` also opens and builds in Xcode.

For development, `OPENDM_DATA_DIR` points the app at a scratch data folder and
`OPENDM_ENGINE` at another engine binary:

```sh
open -n --env OPENDM_DATA_DIR=/tmp/opendm-dev build/OpenDM.app
```

## Test

```sh
cargo test -p opendm-core
```

The `engine` integration test runs the real aria2-next binary (fetched by
`scripts/fetch-engine.sh`). It kills the engine with SIGKILL in the middle
of a download, checks that it restarts and the file completes byte-for-byte,
that removing a download deletes its partial file, and that history survives
reopening the core.

`scripts/test-server.py` serves throttled files with Range support, for
trying the app with many live downloads.

On macOS with 5,060 rows and 60 live downloads, the list spends 0.1 ms per
progress tick with the live rows off-screen, about 3 ms with 21 live rows on
screen, and 4–7 ms when sorted by speed (every tick re-sorts; Cards 3.9 ms,
List 5.2 ms, Columns 7.0 ms on average). Switching between the Cards, List
and Columns styles takes 50–100 ms at that size.
On Windows with 5,123 rows and 60 live downloads it is 0.1 ms, 1.9 ms with 16
live rows on screen and 5.5 ms sorted by speed; see
[windows/README.md](windows/README.md#measure).

## License

OpenDM is MIT licensed. The bundled aria2-next engine is GPL-2.0-or-later and
runs as a separate process; see [vendor/aria2-next/README.md](vendor/aria2-next/README.md).
