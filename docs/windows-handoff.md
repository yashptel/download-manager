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

Status, 19 October 2026: everything in the next three sections is in the
Windows app, with the Cards and List styles, and the checks in "Check on
Windows too" have been run in Chrome, Edge and Firefox; see
`windows/README.md` for what was checked and how.

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
   - The download is already running while the dialog asks, as in IDM, so
     the server is asked once. A captured download arrives with
     `download.taskId` set (the core started it and waited for the
     server's first answer; a refusal never reaches the app, the browser
     keeps it). Any other http(s)/ftp download: call
     `core.AddDownloads([download], startPaused: false)` when the dialog
     opens and keep the ID. Streams and magnet links start only on Start.
   - Show it at once from `core.PlanDownload(download)`, then follow the
     task's row: its name (unless the user typed one), category and folder
     (unless changed), size, and its error in red if it fails. While no
     size or data has arrived, show "Checking…".
   - Changing the category moves the folder to `core.CategoryDir(category)`
     unless a folder was chosen.
   - Start: if the typed name or the folder differs from the row, call
     `core.SetDestination(id, dir, name)` (null for unchanged); the file
     moves there when it completes. Resume it if it failed, then show the
     progress window. Download Later: `core.Pause([id])` and the same
     `SetDestination`. Cancel and the close button:
     `core.Remove([id], deleteFiles: true)`.
   - Without a running task (streams, magnets): on Start set `filename` and
     `filenameSource = "user"` only if the name was edited, always set
     `category` and `dir`, then `core.AddDownloads([download], startPaused: later)`.
   - With confirmation off, downloads that have a `taskId` are already
     running: only show their progress windows.
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

## Logins for sites that ask (HTTP 401)

`CoreListener` has a new method, `LoginNeeded(id, url)`, so the Windows
listener must implement it. The core calls it after a 401 that fresh browser
cookies did not fix. The macOS app (`SiteLogins.swift`):

- Tries the saved login whose site covers the URL (same host; a site such
  as `example.com/private` also needs the path to start with `/private`;
  the longest match wins), once per download, with
  `core.SetLogin(id, user, password)`. The engine sends it as Basic or
  Digest, whichever the server asks for.
- Otherwise, or when that login failed, shows "Login Required": "<host>
  asks for a user name and password", the file name, User name, Password,
  "Remember for <host>" (on by default), Cancel and Log In. One dialog per
  download at a time; Cancel leaves it failed.
- Keeps saved logins in the system credential store, never in the settings
  file: on Windows, Credential Manager (`CredWrite`/`CredRead`/
  `CredEnumerate` with a target such as `OpenDM site login:<site>:<user>`).
- Settings gets a Logins tab between Connection and Scheduler: Site and User
  name columns, + (site, user name, password) and −.

## Refresh download address

`CoreListener` also has `NewAddress(id, download)`. The list's context menu
has "Refresh Download Address…" for one unfinished download
(`AddressRefresh.swift`):

- `core.WaitForAddress(id)` returns the page the download came from (or
  null); it throws for streams and finished downloads, with a message to
  show. Open the page in the default browser and show a small, non-floating
  panel: "Waiting for a new address for “name”", what to do ("Start the
  same download there again, and OpenDM takes its new address and continues
  from 16 KB of 6.3 MB"), and Cancel (`core.StopWaitingForAddress()`).
- The next download a browser hands over arrives at `NewAddress` instead
  of `BrowserDownloads`. Ask "Is this the new address for “name”?" with the
  new name, size and address (and a warning when the sizes differ): "Use
  This Address" calls `core.RefreshAddress(id, download)` and shows the
  progress window; "Download It Separately" handles it as a new download.

## Check on Windows too

- **Wording the macOS app settled on.** Time left in the progress window
  reads "4 min 47 sec", "2 hr 5 min", "3 days 4 hr"; the list says
  "4 min left". A download with nothing yet says "Not started" only while
  queued or paused; a failed one shows its error alone. The several-files
  dialog says "1 of 1 file selected", its button "Start Downloads" for more
  than one, and Select All / Select None turn off when they would change
  nothing. The finished window is titled "Download Complete". Resume All
  and Pause All in the tray menu are off when nothing would change, and the
  menu has New Download…. The core now words connection errors itself
  ("Could not connect to example.com:443", "Could not find the server
  example.com", "The server stopped responding").
- **Open at login and the tray.** General has an "Open at login" switch on
  macOS (it registers the app with `SMAppService`). On Windows, add the same
  switch: write `"<path to OpenDM.exe>" --background` to
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (value name
  `OpenDM`), and read that value back for the switch's state, so the
  registry stays the only record of it. With `--background` the app starts
  in the tray with no window, as it already does when the extension starts
  it. Closing the window keeps it in the tray; Exit in the tray menu quits.
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
