# Continuing on Windows

A prompt for the agent that continues the Windows app on a Windows machine.
Paste everything below the line.

---

Continue OpenDM's Windows app in this repository (C# + WinUI 3, unpackaged,
in `windows/OpenDM`, against the shared Rust core through UniFFI). Read
`README.md`, `windows/README.md`, `docs/research.md` and `extension/README.md`
first. Build with `scripts/build-windows.ps1`, which also regenerates the C#
bindings from the core, so new core APIs appear in C# after a build. Keep the
code simple: no extra layers, no MVVM framework. Match the macOS app's
behavior (in `apple/Sources/OpenDM`), not its look: use WinUI's own controls
and Fluent styling, dark mode, keyboard access.

## What the macOS app has that Windows still lacks

1. **Download File Info dialog** (`DownloadInfoDialog.swift`). Every new
   download, from the browser, from New Download or dropped on the list,
   goes through it unless Settings turn confirmation off
   (`settings.confirmDownloads`). Shows the file's icon, its name and size,
   then editable Name, Category (drop-down), Save to (folder + Choose…),
   "Always save <category> here" once a folder is chosen (writes the
   category's folder into settings), Quality (videos only:
   `NewDownload.mediaQuality`), Address, and Start Download / Download Later /
   Cancel (Enter and Esc).
   - Show it at once from `core.PlanDownload(download)`, then call
     `core.InspectDownload(download)` on a background thread (it asks the
     server, up to `20 s) and fill in the server's name and size unless the
     user already changed them. On failure show the message in red.
   - Changing the category moves the folder to `core.CategoryDir(category)`
     unless a folder was chosen.
   - On Start: set `filename` and `filenameSource = "user"` only if the name
     was edited, always set `category` and `dir` to what the dialog shows,
     then `core.AddDownloads([download], startPaused: later)`.
   - Several links handed over at once get a list dialog instead: checkboxes,
     name, address and category per file, Select All/None, "Save to: each
     file's category folder" or a chosen folder.
   - It should come to the front for a browser download. Windows' foreground
     rules may refuse; then show it without activation and flash the taskbar
     button.
2. **Progress window** (`ProgressWindow.swift`), one per download started
   from the dialog or resumed from the list, at most three at once, unless
   `settings.showProgressWindow` is off. `AddDownloads` now returns the new
   IDs at once. Title "34% name". Progress bar, Status, Downloaded ("x of y ·
   n%"), Speed, Time left, Connections; a collapsible Details with a segment
   map from `core.TaskDetails(id, cells: 120).segments` (0-100 per cell; the
   bytes arrive as a byte array) and the server host names; Pause/Resume and
   Cancel (Cancel pauses and closes; the download stays in the list, as in
   IDM). Poll `TaskDetails` once a second off the UI thread while open. When
   the row turns Complete, the window becomes "Download complete" with Open,
   Show in Folder and Close.
3. **Settings window** (`SettingsWindow.swift`), saved as values change
   (debounce about half a second, then `core.SetSettings`). Tabs:
   - General: download folder, category folders, downloads at a time
     (1-20), ask before starting, show progress window.
   - Browser: take over browser downloads, file types (space separated),
     sites to leave alone (one per line), smallest size in MB.
   - Categories: name, file types, folder (Choose…) per category; add and
     remove.
   - Connection: total and per-download speed limits (KB/s, 0 for none;
     settings store bytes/s), connections per server (1-16), split (1-64),
     smallest part ("1M"), tries (0 keeps trying), wait between tries,
     timeout, connect timeout, proxy, no-proxy hosts, user agent, headers for
     every download (one "Name: value" per line), and when the file exists:
     Rename / Overwrite / Fail (`FileExists`).
   - Scheduler: enabled, start "HH:MM", optional stop "HH:MM" (empty = don't
     pause), weekdays (`schedule.days`, 0 = Sunday; a byte array in C#).
   - Engine: raw aria2 options, one name=value per line
     (`settings.engineExtra`); the core reports options the engine rejects
     through the listener's `Error`.

## Check on Windows too

- **Video site support.** `core/src/sites.rs` downloads `yt-dlp.exe` (or
  `yt-dlp_arm64.exe`) and Deno into the data folder when the extension first
  connects, and unpacks Deno's zip with Windows' own `tar.exe`. This is
  untested on Windows. Confirm `%LOCALAPPDATA%` (or wherever the data folder
  is) ends up with `tools\yt-dlp.exe` and `tools\deno.exe`, and that the
  YouTube quality menu in the extension lists qualities. yt-dlp must run
  without a console window flashing (the core passes `CREATE_NO_WINDOW`).
- **Browser extension** on Edge and Chrome: load `extension/` unpacked
  (Developer mode, Load unpacked), confirm the host registry entries the app
  writes at launch, then try a captured download, the video button's quality
  menu on an HLS page (https://hlsjs.video-dev.org/demo/) and on YouTube.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets` pass on
  Windows.

Commit in small steps with plain messages and push to `main`.

