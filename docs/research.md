# Research notes

Written before milestone 1. Rayburst was studied from its MIT-licensed source.
IDM was studied through the IDM Integration Module's published JavaScript
(behavior only; nothing is reused) and IDM's public help pages. The IDM desktop
app was not decompiled or disassembled.

## Rayburst (desktop and Rayburst Connect)

- Tauri 2: Rust backend, Vue frontend. Rust owns the engine, SQLite and policy.
- Engine supervision: explicit phases, bounded start attempts, automatic
  restart with a crash budget (3 crashes in 5 minutes), `stop-with-process`,
  session save on exit, `save-session`/`input-file` with periodic saves.
- Commands go over HTTP JSON-RPC; a WebSocket carries only aria2 push events.
  Progress is polled with one batched call (500 ms with a visible window, 2 s
  otherwise); push events trigger an immediate poll.
- Bundles its own engine fork, aria2-next, which adds `aria2.probe`, filename
  hints, HLS/DASH, ED2K and more.
- Filename precedence: explicit name, browser-resolved name,
  Content-Disposition, weaker hint, URL basename. Save folder: explicit choice,
  remembered folder, category rule, default.
- Browser handoff: authenticated local HTTP API (`POST /add` with URL, final
  URL, referer, cookie, user agent, filename and source, headers, MIME, size).
  The browser holds its download until the app answers added, accepted,
  cancelled or fallback; any error leaves the download to the browser.
  Chromium requires a filename within 15 s, so the extension suggests one at
  12 s and keeps the item paused. Alt-click bypasses capture.

## IDM Integration Module 6.43.1 (behavior)

- Connects to the app over a localhost WebSocket, falling back to the native
  messaging host; reconnects with backoff and sends a keepalive every 15 s.
- The app pushes capture rules (file types, excluded sites and URLs); the
  extension reports and the app decides.
- Captures from response headers (status, Content-Type,
  `Content-Disposition: attachment`, filename) and, as a fallback, pauses each
  new browser download and matches it to the observed request. A captured
  download is cancelled in the browser; a declined one is resumed.
- Sends URL, filename, size, MIME, referrer, cookies for the URL and user agent.
- A content script in every frame positions the video download button; an
  injected page script tags XHR requests to attribute media segments.

## IDM desktop UX (from its help pages)

- Download File Info dialog: Start Download, Download Later, category (sets the
  folder), editable name and location.
- Options: General, File Types, Connection, Save To, Downloads (start dialog,
  completion dialog and progress dialog can each be turned off), Proxy, Site
  Logins.

## Decisions

- Engine: aria2-next, pinned by version and SHA-256. Its HLS/DASH support,
  `aria2.probe` and filename hints remove the largest pieces of work. Its
  extensions are used from the task module only, so a fork or replacement stays
  contained.
- One WebSocket for commands and push events. Each tick asks only for active
  tasks; tasks named by an event, tasks that left the active list and unknown
  tasks get a full status in a second round trip. Only changed rows reach the UI.
- The RPC secret is passed in a 0600 config file deleted after connecting,
  never on the command line. The port is a free loopback port per launch.
- The aria2 GID is the task identity in history; it survives restarts through
  the session file. Finished tasks leave the engine and live in SQLite.
- Browser integration (milestone 3): a Rust native messaging host relays to the
  running app over a Unix socket or named pipe; no localhost HTTP port. The app
  owns capture rules; the hold, answer and fallback flow follows Rayburst.
- Our code is MIT. aria2-next (GPL-2.0-or-later) ships as a separate executable
  with its license and a link to the exact source tag.

## Milestones

1. Rust core, engine lifecycle, macOS list UI (done).
2. Windows app (WinUI 3, unpackaged) on the same core.
3. Browser extension (Chrome, Edge, Firefox; MV3) and native messaging host,
   including HLS/DASH and direct media detection.
4. Download File Info dialog, per-download progress window, the two toggles,
   General and Advanced settings, stream track picker.
