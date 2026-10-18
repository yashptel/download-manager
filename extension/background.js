// OpenDM Connect: hands browser downloads, links and media to the OpenDM app
// through the native messaging host "org.opendm.host". The app owns the
// capture rules and pushes them here; a download the app cannot take stays
// in the browser.
"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const isFirefox = api.runtime.getURL("").startsWith("moz-extension:");
const HOST = "org.opendm.host";
const VERSION = api.runtime.getManifest().version;

// The app's defaults (Settings::default in core/src/settings.rs), used until
// it has sent its own rules once.
const DEFAULT_RULES = {
  capture: true,
  extensions: (
    "zip rar 7z tar gz tgz bz2 xz zst lz lzma cab exe msi msix appx dmg pkg iso img deb rpm apk " +
    "appimage bin mp4 mkv webm avi mov wmv flv m4v mpg mpeg 3gp m3u8 mpd mp3 m4a aac flac wav ogg " +
    "opus wma aiff pdf doc docx xls xlsx ppt pptx odt ods odp epub"
  ).split(" "),
  excludedSites: [],
  minSize: 0,
};

// Names without an extension still have a type.
const EXTENSION_FOR_MIME = {
  "application/zip": "zip",
  "application/x-zip-compressed": "zip",
  "application/vnd.rar": "rar",
  "application/x-rar-compressed": "rar",
  "application/x-7z-compressed": "7z",
  "application/gzip": "gz",
  "application/x-msdownload": "exe",
  "application/x-msi": "msi",
  "application/x-apple-diskimage": "dmg",
  "application/vnd.android.package-archive": "apk",
  "application/pdf": "pdf",
  "application/octet-stream": "bin",
  "video/mp4": "mp4",
  "video/webm": "webm",
  "audio/mpeg": "mp3",
};

const settings = { rules: DEFAULT_RULES, captureOn: true };
const ready = api.storage.local.get(["rules", "captureOn"]).then((stored) => {
  if (stored.rules) settings.rules = stored.rules;
  if (stored.captureOn === false) settings.captureOn = false;
});

function setRules(rules) {
  if (!rules || typeof rules !== "object") return;
  settings.rules = { ...DEFAULT_RULES, ...rules };
  api.storage.local.set({ rules: settings.rules });
}

// ─── Connection to the app ───────────────────────────────────────────────

const app = { port: null, opening: null, state: "idle", error: "", version: "", waiting: new Map(), nextId: 1 };

/** Opens the port to the host. With start, the host starts the app if needed. */
function openPort(start) {
  if (app.opening) return app.opening;
  let port;
  try {
    port = api.runtime.connectNative(HOST);
  } catch (error) {
    app.state = "error";
    app.error = error.message;
    return Promise.reject(error);
  }
  app.port = port;
  port.onMessage.addListener((message) => onAppMessage(port, message));
  port.onDisconnect.addListener(() => onDisconnect(port));
  const hello = { type: "hello", start, browser: isFirefox ? "firefox" : "chromium", version: VERSION };
  app.opening = post(port, hello, start ? 40_000 : 10_000).then(
    (answer) => {
      app.state = "connected";
      app.error = "";
      app.version = answer.version ?? "";
      setRules(answer.rules);
      return port;
    },
    (error) => {
      if (app.port === port) {
        app.port = null;
        try {
          port.disconnect();
        } catch {}
      }
      app.opening = null;
      throw error;
    },
  );
  return app.opening;
}

function post(port, message, timeoutMs) {
  const id = app.nextId++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => settle(new Error("OpenDM did not answer in time")), timeoutMs);
    const settle = (error, answer) => {
      clearTimeout(timer);
      app.waiting.delete(id);
      error ? reject(error) : resolve(answer);
    };
    app.waiting.set(id, settle);
    try {
      port.postMessage({ ...message, id });
    } catch (error) {
      settle(error);
    }
  });
}

function onAppMessage(port, message) {
  if (message.type === "error") {
    // From the host, just before it exits.
    app.state = message.code === "not-running" ? "not-running" : "error";
    app.error = message.error ?? "";
    return;
  }
  if (message.type === "rules") return setRules(message.rules);
  app.waiting.get(message.id)?.(null, message);
}

function onDisconnect(port) {
  // Reading lastError keeps Chromium from logging it as unchecked.
  const reason = api.runtime.lastError?.message || port.error?.message || "";
  if (app.port !== port) return;
  app.port = null;
  app.opening = null;
  if (app.state === "connected") app.state = "not-running"; // The app quit.
  else if (app.state !== "not-running") {
    app.state = "error";
    app.error = reason || "Could not reach OpenDM";
  }
  const error = new Error(app.error || reason || "OpenDM disconnected");
  for (const settle of [...app.waiting.values()]) settle(error);
}

/** Sends a message to the app, starting it when start is set. */
async function ask(message, { start = true, timeoutMs = 20_000 } = {}) {
  let port;
  try {
    port = await openPort(false);
  } catch (error) {
    if (!start || app.state !== "not-running") throw error;
    port = await openPort(true);
  }
  return post(port, message, timeoutMs);
}

/** Hands items to the app; true when it took them. */
async function handOver(source, items) {
  const answer = await ask({ type: "download", source, items });
  if (answer.result !== "accepted") throw new Error(answer.reason || "OpenDM declined the download");
  return true;
}

// Learn the rules from a running app without starting it.
openPort(false).catch(() => {});

// ─── Request context: cookies, referer and headers ───────────────────────

const requests = new Map(); // url → { headers, tabId }
const MAX_REQUESTS = 400;

function rememberRequest(details) {
  if (details.tabId < 0 && details.type !== "main_frame") return;
  requests.delete(details.url);
  requests.set(details.url, { headers: details.requestHeaders ?? [], tabId: details.tabId });
  if (requests.size > MAX_REQUESTS) requests.delete(requests.keys().next().value);
}

// Chromium hides Cookie and Referer from listeners without extraHeaders;
// Firefox rejects the option.
api.webRequest.onSendHeaders.addListener(
  rememberRequest,
  { urls: ["<all_urls>"] },
  isFirefox ? ["requestHeaders"] : ["requestHeaders", "extraHeaders"],
);

function header(headers, name) {
  return headers?.find((h) => h.name.toLowerCase() === name)?.value ?? "";
}

async function cookieFor(url, headers, storeId) {
  const sent = header(headers, "cookie");
  if (sent) return sent;
  try {
    const cookies = await api.cookies.getAll(storeId ? { url, storeId } : { url });
    return cookies.map((c) => c.name + "=" + c.value).join("; ");
  } catch {
    return "";
  }
}

/** What the app needs to repeat a request the browser made or would make. */
async function requestItem(url, { referrer = "", headers, storeId } = {}) {
  const seen = headers ?? requests.get(url)?.headers;
  return {
    url,
    referrer: header(seen, "referer") || referrer,
    cookie: await cookieFor(url, seen, storeId),
    userAgent: header(seen, "user-agent") || navigator.userAgent,
  };
}

// ─── Capturing browser downloads ─────────────────────────────────────────

/** Alt-click leaves the next download (or the clicked link's) to the browser. */
const bypass = { until: 0, url: "" };

function basename(path) {
  return (path || "").split(/[\\/]/).pop();
}

function extensionOf(name) {
  const match = /\.([a-z0-9]{1,8})$/i.exec(name || "");
  return match ? match[1].toLowerCase() : "";
}

function hostOf(url) {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return "";
  }
}

function isExcluded(url) {
  const host = hostOf(url);
  return settings.rules.excludedSites.some((site) => {
    const name = String(site).toLowerCase().replace(/^[a-z]+:\/\//, "").split("/")[0];
    return name && (host === name || host.endsWith("." + name));
  });
}

function shouldCapture(item) {
  const rules = settings.rules;
  if (!settings.captureOn || !rules.capture || item.state !== "in_progress") return false;
  if (item.incognito || item.byExtensionId) return false;
  const url = item.finalUrl || item.url;
  if (!/^(https?|ftp):/i.test(url)) return false; // blob:, data:, file:
  if (Date.now() < bypass.until && (!bypass.url || bypass.url === item.url || bypass.url === url)) return false;
  if (isExcluded(url) || (item.referrer && isExcluded(item.referrer))) return false;
  let path = "";
  try {
    path = new URL(url).pathname;
  } catch {}
  const extension =
    extensionOf(basename(item.filename)) || extensionOf(path) || EXTENSION_FOR_MIME[(item.mime || "").split(";")[0]] || "";
  if (!rules.extensions.includes(extension)) return false;
  const size = item.totalBytes > 0 ? item.totalBytes : item.fileSize;
  return !(rules.minSize > 0 && size > 0 && size < rules.minSize);
}

/** Pauses the browser's download, offers it to the app, and cancels or resumes it. */
async function takeOver(item) {
  await api.downloads.pause(item.id).catch(() => {});
  let taken = false;
  try {
    const request = requests.get(item.url) ?? requests.get(item.finalUrl);
    const base = await requestItem(item.url, { referrer: item.referrer, headers: request?.headers });
    const name = basename(item.filename);
    taken = await handOver("download", [
      {
        ...base,
        filename: name || undefined,
        filenameSource: name ? "browser" : undefined,
        mime: item.mime || undefined,
        size: item.totalBytes > 0 ? item.totalBytes : undefined,
      },
    ]);
  } catch (error) {
    noteError(error);
  }
  if (taken) {
    await api.downloads.cancel(item.id).catch(noteError);
    await api.downloads.erase({ id: item.id }).catch(noteError);
  } else {
    await api.downloads.resume(item.id).catch(() => {});
  }
  return taken;
}

if (api.downloads.onDeterminingFilename) {
  // Chromium waits up to 15 s for a name and keeps downloading meanwhile, so
  // the item is paused, and named at 12 s if the app is still deciding.
  api.downloads.onDeterminingFilename.addListener((item, suggest) => {
    if (!shouldCapture(item)) return;
    let named = false;
    const name = () => {
      if (named) return;
      named = true;
      try {
        suggest();
      } catch {}
    };
    const deadline = setTimeout(name, 12_000);
    ready.then(() => takeOver(item)).then((taken) => {
      clearTimeout(deadline);
      if (!taken) name();
    });
    return true;
  });
} else {
  // Firefox: the download exists before any filename hook could run.
  api.downloads.onCreated.addListener((item) => {
    ready.then(() => shouldCapture(item) && takeOver(item));
  });
}

// ─── Context menu ────────────────────────────────────────────────────────

function createMenus() {
  Promise.resolve(api.contextMenus.removeAll()).then(() => {
    api.contextMenus.create({ id: "link", title: "Download with OpenDM", contexts: ["link", "image", "video", "audio"] });
    api.contextMenus.create({ id: "selection", title: "Download selected links with OpenDM", contexts: ["selection"] });
    api.contextMenus.create({ id: "page", title: "Download all links with OpenDM", contexts: ["page", "frame"] });
  });
}
api.runtime.onInstalled.addListener(createMenus);
api.runtime.onStartup?.addListener(createMenus);

/** Runs in the page: the links on it, or only those in the selection. */
function collectLinks(selectionOnly) {
  const selection = getSelection();
  const ranges = [];
  for (let i = 0; i < (selection?.rangeCount ?? 0); i++) ranges.push(selection.getRangeAt(i));
  const seen = new Set();
  const links = [];
  for (const link of document.querySelectorAll("a[href], area[href]")) {
    if (selectionOnly && !ranges.some((range) => range.intersectsNode(link))) continue;
    const url = link.href;
    if (!/^(https?|ftp):/i.test(url) || seen.has(url)) continue;
    seen.add(url);
    links.push(url);
  }
  return links;
}

api.contextMenus.onClicked.addListener(async (info, tab) => {
  try {
    const referrer = info.frameUrl || info.pageUrl || tab?.url || "";
    const storeId = tab?.cookieStoreId;
    if (info.menuItemId === "link") {
      const url = info.linkUrl || info.srcUrl || "";
      if (!/^(https?|ftp):/i.test(url)) {
        // A video drawn from a blob: the stream the page loaded is the download.
        if (tab && (info.mediaType === "video" || info.mediaType === "audio")) return await downloadBestMedia(tab.id);
        throw new Error("This link cannot be downloaded");
      }
      return await handOver("link", [await requestItem(url, { referrer, storeId })]);
    }
    const [result] = await api.scripting.executeScript({
      target: { tabId: tab.id, frameIds: [info.frameId ?? 0] },
      func: collectLinks,
      args: [info.menuItemId === "selection"],
    });
    const links = result?.result ?? [];
    if (!links.length) throw new Error("No links found");
    const items = await Promise.all(links.map((url) => requestItem(url, { referrer, storeId })));
    await handOver("links", items);
  } catch (error) {
    noteError(error);
  }
});

// ─── Media on the page ───────────────────────────────────────────────────
//
// Two sources: the network (any response with a media type or extension)
// and page.js (playlists the page's scripts fetch, find in JSON or build in
// memory). Streams get their qualities from the app, which has the engine
// read the playlist.

// loaded: the page fetched it, as opposed to only naming it in a script or
// JSON answer (which can list every video on a site).
const media = new Map(); // tabId → [{ url, kind, size, headers, at, loaded, content, variant, page, probe }]
const MAX_MEDIA = 60;
const FILE_KINDS = new Set(["mp4", "webm", "mkv", "mov", "m4v", "flv", "avi", "mp3", "m4a", "ogg", "opus", "wav", "flac"]);
const SEGMENTS = new Set(["ts", "m4s", "m4f", "cmfv", "cmfa", "key"]);
const PROBE_TTL = 120_000;

/** "HLS", "DASH", a file type such as "MP4", or "" when it is not media. */
function mediaKind(url, contentType) {
  let extension = "";
  try {
    extension = extensionOf(new URL(url).pathname);
  } catch {
    return "";
  }
  const type = (contentType || "").split(";")[0].trim().toLowerCase();
  if (extension === "m3u8" || /mpegurl$/.test(type)) return "HLS";
  if (extension === "mpd" || type === "application/dash+xml") return "DASH";
  if (SEGMENTS.has(extension) || type === "video/mp2t" || type === "video/iso.segment") return "";
  if (FILE_KINDS.has(extension)) return extension.toUpperCase();
  if (/^(video|audio)\/[a-z0-9.+-]+$/.test(type)) return type.split("/")[1].replace(/^x-/, "").toUpperCase();
  return "";
}

const isStream = (entry) => entry.kind === "HLS" || entry.kind === "DASH";

function responseSize(details) {
  const range = header(details.responseHeaders, "content-range");
  const total = /\/(\d+)$/.exec(range);
  if (total) return Number(total[1]);
  return details.statusCode === 200 ? Number(header(details.responseHeaders, "content-length")) || 0 : 0;
}

/** Adds a find to the tab's list, or fills in what an earlier find lacked. */
function addMedia(tabId, found) {
  const list = media.get(tabId) ?? [];
  const known = list.find((entry) => entry.url === found.url);
  if (known) {
    known.size = Math.max(known.size, found.size ?? 0);
    if (found.loaded && !known.loaded) {
      known.loaded = true;
      showCount(tabId);
    }
    if (found.content && !known.content) {
      known.content = found.content;
      known.variant = found.variant;
      known.probe = null;
      showCount(tabId);
    }
    return;
  }
  list.push({ size: 0, headers: requests.get(found.url)?.headers ?? [], at: Date.now(), ...found });
  if (list.length > MAX_MEDIA) list.shift();
  media.set(tabId, list);
  showCount(tabId);
}

api.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.statusCode >= 400 || details.type === "main_frame") return;
    const kind = mediaKind(details.url, header(details.responseHeaders, "content-type"));
    if (!kind) return;
    const size = responseSize(details);
    // Short sounds and previews are not worth listing.
    if (kind !== "HLS" && kind !== "DASH" && size > 0 && size < 512 * 1024) return;
    addMedia(details.tabId, { url: details.url, kind, size, loaded: true });
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"],
);

/** Finds from page.js, through content.js. */
function addPageMedia(tabId, frameUrl, items) {
  for (const item of items.slice(0, 100)) {
    const url = String(item?.url ?? "");
    const blob = url.startsWith("blob:");
    if (!(/^https?:/i.test(url) || blob) || !["hls", "dash"].includes(item.kind)) continue;
    const content = typeof item.content === "string" && item.content.length <= 2 * 1024 * 1024 ? item.content : undefined;
    // A playlist in memory can only be downloaded from what the page gave us.
    if (blob && !content) continue;
    addMedia(tabId, {
      url,
      kind: item.kind.toUpperCase(),
      content,
      loaded: Boolean(content),
      variant: item.variant === true,
      page: blob ? frameUrl : undefined,
    });
  }
}

function clearMedia(tabId) {
  if (media.delete(tabId)) showCount(tabId);
}

// A new page in the tab; a link that turns into a download is not one.
api.tabs.onUpdated.addListener((tabId, change) => {
  if (change.status === "loading" && change.url) clearMedia(tabId);
});
api.tabs.onRemoved.addListener((tabId) => media.delete(tabId));

/** The finds worth showing: what the page loaded, when it loaded a stream.
 * A player that loaded a master playlist also loads one of its quality
 * playlists, which the master already lists, and its segments. */
function visibleMedia(tabId) {
  const list = media.get(tabId) ?? [];
  const hasMaster = list.some((entry) => entry.kind === "HLS" && entry.content && !entry.variant);
  const streams = list.filter(isStream);
  const playing = streams.some((entry) => entry.loaded);
  const folders = streams.map((entry) => entry.url.replace(/[?#].*$/, "").replace(/[^/]*$/, ""));
  return list.filter((entry) => {
    if (isStream(entry)) return (!playing || entry.loaded) && !(hasMaster && entry.variant);
    return !folders.some((folder) => entry.url.startsWith(folder));
  });
}

function showCount(tabId) {
  const count = visibleMedia(tabId).length;
  api.action.setBadgeBackgroundColor({ tabId, color: "#0a84ff" }).catch(() => {});
  api.action.setBadgeText({ tabId, text: count ? String(count) : "" }).catch(() => {});
}

/** A file name from the page title, which the engine treats as a weak hint.
 * For a stream it is only the name: the engine adds the container's extension. */
function titleName(title, kind) {
  const name = (title || "").replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 120);
  if (!name) return undefined;
  return kind === "HLS" || kind === "DASH" ? name : name + "." + kind.toLowerCase();
}

/** What the app needs to fetch this find again, as the page did. */
async function mediaItem(tab, entry) {
  const url = entry.page ?? entry.url;
  const item = await requestItem(url, { referrer: tab?.url ?? "", headers: entry.headers, storeId: tab?.cookieStoreId });
  const headers = entry.headers.filter((h) => ["origin", "authorization"].includes(h.name.toLowerCase()) || /^x-/i.test(h.name));
  const filename = titleName(tab?.title, entry.kind);
  const stream = isStream(entry)
    ? { mediaKind: entry.kind.toLowerCase(), mediaManifest: entry.content || undefined }
    : {};
  return { ...item, headers, filename, filenameSource: filename ? "title" : undefined, ...stream };
}

/** The stream's tracks, read by the app's engine; kept for a while. */
async function inspect(tab, entry) {
  if (entry.probe && Date.now() - entry.probe.at < PROBE_TTL) return entry.probe.answer;
  const answer = await ask({ type: "inspect", item: await mediaItem(tab, entry) }, { timeoutMs: 40_000 });
  entry.probe = { at: Date.now(), answer };
  return answer;
}

// ─── Quality menu ────────────────────────────────────────────────────────

const CODECS = [
  [/^(avc1|avc3)/, "H.264"],
  [/^(hvc1|hev1)/, "HEVC"],
  [/^(vp09|vp9)/, "VP9"],
  [/^vp8/, "VP8"],
  [/^av01/, "AV1"],
  [/^mp4a\.40\.(5|29)/, "HE-AAC"],
  [/^mp4a/, "AAC"],
  [/^opus/, "Opus"],
  [/^ec-3/, "E-AC-3"],
  [/^ac-3/, "AC-3"],
  [/^flac/, "FLAC"],
  [/^vorbis/, "Vorbis"],
];

function codecName(codecs, wanted) {
  for (const codec of codecs.split(",").map((c) => c.trim().toLowerCase())) {
    const name = CODECS.find(([pattern]) => pattern.test(codec))?.[1];
    const video = ["H.264", "HEVC", "VP9", "VP8", "AV1"].includes(name);
    if (name && video === (wanted === "video")) return name;
  }
  return "";
}

function formatSize(bytes) {
  if (!bytes) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return value.toFixed(value < 10 && unit > 0 ? 1 : 0) + " " + units[unit];
}

const kbps = (bandwidth) => Math.round(bandwidth / 1000) + " kbps";

/** MP4 plays everywhere; codecs it cannot carry go into MKV. */
function containerFor(codecs) {
  return /vp8|vorbis|theora/i.test(codecs) ? "mkv" : "mp4";
}

function streamName(url) {
  try {
    const parts = new URL(url).pathname.split("/").filter(Boolean);
    return decodeURIComponent(parts.pop() || new URL(url).hostname);
  } catch {
    return url;
  }
}

/** Menu groups for one stream: every video quality, then audio on its own. */
function streamGroups(entry, presentation) {
  const seconds = presentation.live ? 0 : presentation.durationMs / 1000;
  const estimate = (bandwidth) => (seconds && bandwidth ? "≈ " + formatSize((bandwidth * seconds) / 8) : "");
  const audios = presentation.tracks.filter((t) => t.type === "audio").sort((a, b) => b.bandwidth - a.bandwidth);
  const bestAudio = audios[0];
  const seen = new Set();
  const video = presentation.tracks
    .filter((t) => t.type === "video" || t.type === "muxed")
    .sort((a, b) => b.height - a.height || b.frameRate - a.frameRate || b.bandwidth - a.bandwidth)
    .flatMap((track) => {
      const fps = track.frameRate > 31 ? Math.round(track.frameRate) : "";
      const label = track.height ? track.height + "p" + fps : kbps(track.bandwidth);
      const codec = codecName(track.codec, "video");
      if (seen.has(label + codec)) return [];
      seen.add(label + codec);
      const audioBandwidth = track.type === "video" ? bestAudio?.bandwidth ?? 0 : 0;
      return [{
        label,
        detail: [presentation.live ? "Live" : "", codec, estimate(track.bandwidth + audioBandwidth)].filter(Boolean).join(" · "),
        choice: { url: entry.url, video: track.id, audio: "best", format: containerFor(track.codec + "," + (bestAudio?.codec ?? "")) },
      }];
    });
  const heard = new Set();
  const audio = audios.flatMap((track) => {
    const codec = codecName(track.codec, "audio");
    const key = track.language + codec + kbps(track.bandwidth);
    if (heard.has(key)) return [];
    heard.add(key);
    return [{
      label: track.language ? "Audio · " + track.language : "Audio",
      detail: [track.bandwidth ? kbps(track.bandwidth) : "", codec, estimate(track.bandwidth)].filter(Boolean).join(" · "),
      choice: { url: entry.url, video: "none", audio: track.id, format: containerFor(track.codec) },
    }];
  });
  return [
    { title: "Video", options: video },
    { title: "Audio only", options: audio },
  ].filter((group) => group.options.length);
}

/** Everything the tab's video menu offers. src is the hovered video's own URL. */
async function formats(tabId, src) {
  const tab = await api.tabs.get(tabId).catch(() => null);
  const list = [...visibleMedia(tabId)].reverse(); // Newest first.
  if (src && /^https?:/i.test(src) && !list.some((entry) => entry.url === src)) {
    const kind = mediaKind(src, "") || "MP4";
    addMedia(tabId, { url: src, kind, loaded: true });
    list.unshift(media.get(tabId).find((entry) => entry.url === src));
  }
  const files = list.filter((entry) => !isStream(entry)).sort((a, b) => (b.url === src) - (a.url === src) || b.size - a.size).slice(0, 6);

  const probe = (entries) =>
    Promise.all(
      entries.slice(0, 4).map(async (entry) => {
        try {
          return { entry, answer: await inspect(tab, entry) };
        } catch (error) {
          return { entry, answer: { result: "failed", reason: error.message } };
        }
      }),
    );
  // The streams the player loaded come first. When none of them can be read
  // (often DRM on the top qualities only), the page may name others that can.
  const streams = [...(media.get(tabId) ?? [])].reverse().filter((entry) => isStream(entry) && !entry.variant);
  let probed = await probe(streams.filter((entry) => entry.loaded));
  if (!probed.some((p) => p.answer.result === "ready")) {
    probed = [...probed, ...(await probe(streams.filter((entry) => !entry.loaded)))];
  }
  const ready = probed.filter((p) => p.answer.result === "ready");
  const qualities = (p) => p.answer.presentation.tracks.filter((t) => t.type !== "audio").length;
  // A playlist of one quality next to a full one is a part of it, or an ad.
  const most = Math.max(0, ...ready.map(qualities));
  const shown = ready
    .filter((p) => most < 2 || qualities(p) >= 2)
    .sort((a, b) => Boolean(b.entry.loaded) - Boolean(a.entry.loaded) || qualities(b) - qualities(a));

  const groups = [];
  shown.forEach((p, index) => {
    const own = streamGroups(p.entry, p.answer.presentation);
    if (index === 0) groups.push(...own);
    else groups.push({ title: "Another stream · " + hostOf(p.entry.url), options: own.flatMap((g) => g.options) });
  });
  const failed = probed.find((p) => p.answer.result !== "ready");
  if (!shown.length && failed) {
    // The engine could not read it here; it may still download.
    groups.push({
      title: "Stream",
      options: [{ label: "Best quality", detail: failed.entry.kind + " · " + (failed.answer.reason || "qualities unknown"), choice: { url: failed.entry.url, video: "best", audio: "best" } }],
    });
  }
  if (files.length) {
    groups.push({
      title: "Files",
      options: files.map((entry) => ({
        label: entry.kind,
        detail: [formatSize(entry.size), streamName(entry.url)].filter(Boolean).join(" · "),
        choice: { url: entry.url },
      })),
    });
  }
  return { title: tab?.title ?? "", groups };
}

async function downloadChoice(tabId, choice) {
  const entry = media.get(tabId)?.find((item) => item.url === choice?.url);
  if (!entry) throw new Error("That video is gone; reload the page");
  const tab = await api.tabs.get(tabId).catch(() => null);
  const item = await mediaItem(tab, entry);
  if (isStream(entry)) {
    Object.assign(item, { mediaVideo: choice.video, mediaAudio: choice.audio, mediaFormat: choice.format });
  }
  await handOver("media", [item]);
}

/** The context menu on a video: the best stream at its best quality. */
async function downloadBestMedia(tabId) {
  const list = visibleMedia(tabId);
  const best = list.findLast(isStream) ?? [...list].sort((a, b) => b.size - a.size)[0];
  if (!best) throw new Error("No video found yet. Play it for a moment, then try again.");
  await downloadChoice(tabId, { url: best.url });
}

// ─── Errors, popup and page messages ─────────────────────────────────────

let lastError = "";

function noteError(error) {
  lastError = error?.message || String(error);
  console.warn("OpenDM:", lastError);
}

async function onMessage(message, sender) {
  switch (message?.type) {
    case "bypass":
      bypass.until = Date.now() + 5_000;
      bypass.url = message.url || "";
      return { ok: true };
    case "page-media":
      if (sender.tab && Array.isArray(message.items)) addPageMedia(sender.tab.id, sender.url ?? sender.tab.url, message.items);
      return { ok: true };
    case "formats": {
      // The popup names the tab; the page's own menu is in sender.tab.
      const tabId = message.tabId ?? sender.tab?.id;
      return { ok: true, ...(await formats(tabId, message.src)) };
    }
    case "download-choice":
      await downloadChoice(message.tabId ?? sender.tab?.id, message.choice);
      return { ok: true };
    case "state": {
      await ready;
      if (app.state !== "connected") await openPort(false).catch(() => {});
      return {
        ok: true,
        state: app.state,
        error: app.error,
        version: app.version,
        captureOn: settings.captureOn,
        appCapture: settings.rules.capture,
        lastError,
        mediaCount: visibleMedia(message.tabId).length,
        hostAccess: await api.permissions.contains({ origins: ["<all_urls>"] }),
      };
    }
    case "start":
      await openPort(true);
      return { ok: true };
    case "set-capture":
      settings.captureOn = Boolean(message.on);
      await api.storage.local.set({ captureOn: settings.captureOn });
      return { ok: true };
    default:
      return { ok: false, error: "unknown message" };
  }
}

api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  onMessage(message, sender).then(sendResponse, (error) => {
    noteError(error);
    sendResponse({ ok: false, error: error.message });
  });
  return true;
});
