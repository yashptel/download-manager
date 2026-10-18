# OpenDM Connect

The browser extension for OpenDM, for Chrome, Edge and Firefox (Manifest V3,
plain JavaScript, no build step). It hands downloads, links and media to the
app through the native messaging host `org.opendm.host` (`host/` in this
repository), which OpenDM registers with every installed browser each time it
starts.

## What it does

- **Captures downloads.** A download whose type is on the app's capture list
  is paused in the browser and offered to OpenDM. When OpenDM takes it, the
  browser's copy is cancelled and removed from its list; when OpenDM is not
  installed, is turned off or cannot start, the browser resumes its own
  download, so nothing is lost. Alt-click a link to keep that download in the
  browser. The cookie, referer and user agent of the browser's request go
  with it.
- **Right-click menu:** Download with OpenDM (a link, image, video or audio),
  Download selected links with OpenDM, Download all links with OpenDM.
- **Finds media** three ways: the network (any response with a media type or
  extension); `page.js`, which runs in the page's own JavaScript world and
  reads what the page's scripts fetch (playlists, and playlist URLs inside
  JSON answers) and playlists a player builds in memory as a blob: URL; and
  a scan of the page's inline scripts and video metadata tags. A playlist the
  page received keeps its content, so the app downloads exactly what the page
  saw. Quality playlists are hidden behind their master playlist.
- **Quality menu.** A **Download video** button appears over videos on hover;
  it opens a menu of every video quality (resolution, frame rate, codec and
  estimated size), audio-only tracks, and plain files. The toolbar popup
  shows the same list. The qualities come from OpenDM's engine, which reads
  the playlist; the chosen tracks are downloaded and muxed into MP4 (or MKV
  for VP9, AV1 and Opus), named after the page title.
- **Sites through yt-dlp.** For sites that never show the browser a file or
  playlist (YouTube streams through its own protocol), OpenDM asks yt-dlp
  about the page, sending this browser's cookies for the site so logged-in
  and age-restricted videos work. Its playlists and files join the same
  menu. The lookup starts when you hover a video, so the menu is usually
  ready by the click. OpenDM downloads yt-dlp and Deno (which yt-dlp needs
  for YouTube) into its data folder once the extension first connects, and
  lets yt-dlp update itself daily.
- **Starts OpenDM** in the background when there is something to hand over
  and it is not running. Opening the browser alone never starts it.

## Try it during development

`node scripts/try-extension.mjs chrome [url]` (or `firefox`) opens that browser
with a separate profile in `~/.opendm-dev` and the extension already loaded.
To use your own browser profile instead:

Build and start the app once (`scripts/build-macos.sh`, then open
`build/OpenDM.app`) so it registers the host. Then:

- **Chrome or Edge:** open `chrome://extensions` (or `edge://extensions`), turn
  on Developer mode, choose **Load unpacked** and pick this folder. The
  `key` in `manifest.json` fixes the extension ID at
  `nkhhoiejpomapjkeomdbflaicnnjclfj`, the ID the host allows.
- **Firefox:** open `about:debugging#/runtime/this-firefox`, choose **Load
  Temporary Add-on** and pick `manifest.json`. If the popup asks, allow
  access to all sites.

Store builds get their own IDs; add them to `CHROMIUM_EXTENSION_IDS` in
`host/src/lib.rs` before publishing.

## How it talks to the app

The background script keeps one native messaging port open while OpenDM runs;
an open port also keeps Chromium's service worker alive, so the app can push
rule changes. Messages are JSON; requests carry an `id` that the answer
repeats.

| Message | Answer |
| --- | --- |
| `{ type: "hello", start, browser, version }` | `{ type: "hello", app, version, rules }`. The first message on a port; with `start: false` the host answers `{ type: "error", code: "not-running" }` instead of starting OpenDM. |
| `{ type: "download", source, items: [...] }` | `{ result: "accepted" }` or `{ result: "fallback", reason }`. A captured download (`source: "download"`) is started by the app at once; it answers when the server does, and a refusal is a fallback. `source` is `download`, `link`, `links` or `media`; each item has `url` and optionally `referrer`, `cookie`, `userAgent`, `filename`, `filenameSource` (`browser`, `title` or `suggested`), `mime`, `size`, `headers` and `proxy` (the browser's proxy for that request, `http://host:port` or `socks5://host:port`: Firefox's per-request proxy, or in Chromium a fixed proxy set by another extension; never one that needs a login). |
| `{ type: "inspect", item }` | `{ result: "ready", presentation: { live, durationMs, tracks: [{ id, type, language, codec, width, height, bandwidth, frameRate }] } }` or `{ result: "failed", reason }`. `item` is a stream item as below. The app has its engine read the playlist with a hidden task that it removes again. |
| `{ type: "site", url, cookies }` | `{ result: "ready", videos: [{ title, duration, site, streams: [{ url, kind, headers }], files: [{ id, url, ext, vcodec, acodec, height, fps, tbr, size, language, headers }] }] }` or `{ result: "failed", reason, retry }`. yt-dlp's view of the page; `cookies` are the browser's cookies for the site. `retry` is set while yt-dlp is still being downloaded. |
| (pushed by the app) | `{ type: "rules", rules: { capture, extensions, excludedSites, minSize } }` whenever the settings change. |
| (asked by the app) `{ type: "cookies", id, url }` | The extension answers `{ type: "cookies", replyTo: id, cookie }`: the Cookie header it would send to `url` now. The app asks after a 401 or 403 and restarts the download once with the new cookies when they differ. |

Stream items add `mediaKind` (`hls` or `dash`), optionally `mediaManifest` (the
playlist text the page received, at most 2 MB), and for a chosen quality
`mediaVideo` and `mediaAudio` (`best`, `none` or a track ID from `inspect`)
and `mediaFormat` (`mp4` or `mkv`).

"Accepted" means OpenDM has the download; its own dialog decides the rest.
The rules come from the app's settings (`core/src/settings.rs`); until the
app has answered once, the extension uses the same defaults.

Chromium waits at most 15 seconds for an extension to name a download, so the
paused item is named at 12 seconds if OpenDM is still deciding. Firefox has
no such hook; the download is paused as soon as it is created.

## Tests

`tests/browser/run.sh` runs Google Chrome and Firefox against a local test site
with this extension, the host and `build/OpenDM.app`: capture with the app
starting itself, the forwarded cookie, referer and user agent, Alt-click,
a master playlist found inside a JSON answer, the quality menu (in the page
and the popup) with a 360p choice giving a 360p MP4, the page script running
in Firefox, and "all links".
