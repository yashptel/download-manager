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
- **Finds media.** HLS and DASH playlists and direct audio or video files the
  page loads are listed per tab in the toolbar popup, with a count on the
  toolbar button. A **Download video** button appears over videos on hover.
  Streams are saved as MP4, named after the page title; the engine downloads
  the segments and remuxes them.
- **Starts OpenDM** in the background when there is something to hand over
  and it is not running. Opening the browser alone never starts it.

## Try it during development

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
| `{ type: "download", source, items: [...] }` | `{ result: "accepted" }` or `{ result: "fallback", reason }`. `source` is `download`, `link`, `links` or `media`; each item has `url` and optionally `referrer`, `cookie`, `userAgent`, `filename`, `filenameSource` (`browser`, `title` or `suggested`), `mime`, `size` and `headers`. |
| (pushed by the app) | `{ type: "rules", rules: { capture, extensions, excludedSites, minSize } }` whenever the settings change. |

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
HLS detection and an MP4 result, the video button, and "all links".
