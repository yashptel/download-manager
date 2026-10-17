# OpenDM

An open-source download manager for macOS and Windows: IDM's browser-capture
workflow and dialogs, with native UIs on each platform. OpenDM is a working name.

| Path | What it is |
| --- | --- |
| `core/` | Rust core shared by both apps through UniFFI: aria2-next process supervision, JSON-RPC over WebSocket, task list, SQLite history, settings and file-type categories |
| `apple/` | macOS app (Swift, AppKit) as a Swift package |
| `vendor/aria2-next/` | Pinned engine version and checksums; binaries are downloaded, not committed |
| `scripts/` | Build, engine fetch and test-server scripts |
| `docs/` | Research notes and design decisions |

Windows (WinUI 3), the browser extension and the native messaging host come in
later milestones; see [docs/research.md](docs/research.md).

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

## License

OpenDM is MIT licensed. The bundled aria2-next engine is GPL-2.0-or-later and
runs as a separate process; see [vendor/aria2-next/README.md](vendor/aria2-next/README.md).
