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

const media = new Map(); // tabId → [{ url, kind, size, headers, at }]
const MAX_MEDIA = 60;
const FILE_KINDS = new Set(["mp4", "webm", "mkv", "mov", "m4v", "flv", "avi", "mp3", "m4a", "ogg", "opus", "wav", "flac"]);
const SEGMENTS = new Set(["ts", "m4s", "m4f", "cmfv", "cmfa", "key"]);

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

function responseSize(details) {
  const range = header(details.responseHeaders, "content-range");
  const total = /\/(\d+)$/.exec(range);
  if (total) return Number(total[1]);
  return details.statusCode === 200 ? Number(header(details.responseHeaders, "content-length")) || 0 : 0;
}

api.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.statusCode >= 400 || details.type === "main_frame") return;
    const kind = mediaKind(details.url, header(details.responseHeaders, "content-type"));
    if (!kind) return;
    const size = responseSize(details);
    // Short sounds and previews are not worth listing.
    if (kind !== "HLS" && kind !== "DASH" && size > 0 && size < 512 * 1024) return;
    const list = media.get(details.tabId) ?? [];
    const known = list.find((entry) => entry.url === details.url);
    if (known) {
      known.size = Math.max(known.size, size);
      return;
    }
    list.push({ url: details.url, kind, size, headers: requests.get(details.url)?.headers ?? [], at: Date.now() });
    if (list.length > MAX_MEDIA) list.shift();
    media.set(details.tabId, list);
    showCount(details.tabId);
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"],
);

function clearMedia(tabId) {
  if (media.delete(tabId)) showCount(tabId);
}

// A new page in the tab; a link that turns into a download is not one.
api.tabs.onUpdated.addListener((tabId, change) => {
  if (change.status === "loading" && change.url) clearMedia(tabId);
});
api.tabs.onRemoved.addListener((tabId) => media.delete(tabId));

function showCount(tabId) {
  const count = media.get(tabId)?.length ?? 0;
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

async function downloadMedia(tabId, entry) {
  const tab = await api.tabs.get(tabId).catch(() => null);
  const item = await requestItem(entry.url, { referrer: tab?.url ?? "", headers: entry.headers, storeId: tab?.cookieStoreId });
  const headers = entry.headers.filter((h) => ["origin", "authorization"].includes(h.name.toLowerCase()) || /^x-/i.test(h.name));
  const filename = titleName(tab?.title, entry.kind);
  await handOver("media", [{ ...item, headers, filename, filenameSource: filename ? "title" : undefined }]);
}

/** A stream manifest beats loose files; among files, the largest. */
function bestMedia(tabId) {
  const list = media.get(tabId) ?? [];
  return list.find((entry) => entry.kind === "HLS" || entry.kind === "DASH") ?? [...list].sort((a, b) => b.size - a.size)[0];
}

async function downloadBestMedia(tabId, src) {
  if (src && /^https?:/i.test(src)) {
    const known = media.get(tabId)?.find((entry) => entry.url === src);
    return downloadMedia(tabId, known ?? { url: src, kind: mediaKind(src, "") || "MP4", size: 0, headers: requests.get(src)?.headers ?? [] });
  }
  const best = bestMedia(tabId);
  if (!best) throw new Error("No video found yet. Play it for a moment, then try again.");
  return downloadMedia(tabId, best);
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
    case "video":
      await downloadBestMedia(sender.tab.id, message.src);
      return { ok: true };
    case "state": {
      await ready;
      if (app.state !== "connected") await openPort(false).catch(() => {});
      const list = (media.get(message.tabId) ?? []).map(({ url, kind, size }) => ({ url, kind, size }));
      return {
        ok: true,
        state: app.state,
        error: app.error,
        version: app.version,
        captureOn: settings.captureOn,
        appCapture: settings.rules.capture,
        lastError,
        media: list,
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
    case "download-media": {
      const entry = media.get(message.tabId)?.find((item) => item.url === message.url);
      if (!entry) throw new Error("That stream is gone; reload the page");
      await downloadMedia(message.tabId, entry);
      return { ok: true };
    }
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
