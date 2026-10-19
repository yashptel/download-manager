# OpenDM privacy policy

Last updated: October 19, 2026

OpenDM is a free, open-source download manager for macOS and Windows.
OpenDM Connect is its browser extension for Chrome, Edge and Firefox. Neither
one has an account, analytics, advertising or a server of its own. This page
explains what the extension and the app handle and where that data goes.

## What OpenDM Connect handles

The extension's one job is to hand downloads from the browser to the OpenDM
app on the same computer. To do that it handles:

- **Download details.** The address of each download you start, choose from
  the video menu or send with "Download with OpenDM", the page it came from
  (the referrer), the page's title for naming videos, and the request headers
  and user agent the download needs.
- **Cookies for that download.** When a download is handed over, the
  extension reads the cookies for that download's own address, so files that
  need you to be signed in download the same way they would in the browser.
- **Media the page plays.** To list a video's qualities, the extension notes
  the addresses of media files and streaming playlists a tab loads, while the
  tab is open.
- **The browser's proxy setting**, read only, so OpenDM uses the same proxy.
- **Two settings**, stored in the browser: whether download capture is on,
  and the capture rules (file types, excluded sites, minimum size).

## Where it goes

All of the above goes to one place: the OpenDM app on your own computer,
through the browser's native messaging. The extension sends nothing to the
developer or to anyone else, and it does not sell, share or transfer data.

The OpenDM app then connects to the servers you download from, sending them
the requests the download needs, as the browser would. It keeps a download
history and its settings on your computer, and saved site logins in the
system keychain (macOS) or credential store (Windows).

## Other connections the app makes

- **Update check.** Once a day, unless turned off in Settings, the app asks
  GitHub for the latest OpenDM release. The request carries no personal data
  beyond what any web request does, such as your IP address.
- **Video site support.** To read some video sites, the app downloads the
  open-source tools yt-dlp and Deno from their official GitHub releases and
  runs them on your computer for the pages you ask about.

## Removing your data

Uninstalling the extension removes its settings. The app's history, settings
and logins can be cleared from within the app, or by deleting its data
folder (`~/Library/Application Support/OpenDM` on macOS,
`%LOCALAPPDATA%\OpenDM` on Windows).

## Contact

This policy is published at https://opendm.squiwo.com/privacy.

Questions about this policy: open an issue at
https://github.com/yashptel/download-manager/issues or email
yashptel2899@gmail.com.
