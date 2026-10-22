// OpenDM Connect: hands browser downloads, links and media to the OpenDM app
// through the native messaging host "org.opendm.host". The app owns the
// capture rules and pushes them here; a download the app cannot take stays
// in the browser.
"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const isFirefox = api.runtime.getURL("").startsWith("moz-extension:");
const HOST = "org.opendm.host";
const VERSION = api.runtime.getManifest().version;

// Sites whose videos and audio are left alone. Empty here; the Chrome Web
// Store package lists YouTube (scripts/package-extension.py), whose policy
// does not allow downloading from it. The packager replaces this exact line.
const MEDIA_BLOCKED_HOSTS = [];
function mediaBlocked(url) {
  let host = "";
  try {
    host = new URL(url).hostname;
  } catch {
    return false;
  }
  return MEDIA_BLOCKED_HOSTS.some((blocked) => host === blocked || host.endsWith("." + blocked));
}

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
  if (message.type === "cookies") return answerCookies(port, message);
  app.waiting.get(message.id)?.(null, message);
}

/** The app asks which cookies this browser would send to a URL now, to
 * retry a download the server refused with older ones. */
async function answerCookies(port, message) {
  const cookie = typeof message.url === "string" && /^https?:/i.test(message.url) ? await cookieFor(message.url, []) : "";
  try {
    port.postMessage({ type: "cookies", replyTo: message.id, cookie });
  } catch {}
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
  requests.set(details.url, { headers: details.requestHeaders ?? [], tabId: details.tabId, proxy: proxyAddress(details.proxyInfo) });
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
    proxy: requests.get(url)?.proxy ?? (await extensionProxy(url)),
  };
}

// ─── The browser's proxy ─────────────────────────────────────────────────
//
// A VPN or proxy extension sends the browser's requests through its proxy,
// and a site may refuse the same request coming straight from the app. The
// app goes the same way when it can: a plain HTTP or SOCKS5 proxy. A proxy
// that needs a login cannot be shared, as only the extension knows it.

/** "http://host:port" for Firefox's per-request proxyInfo. */
function proxyAddress(info) {
  if (!info || info.username || !info.host || !info.port) return undefined;
  const scheme = { http: "http", socks: "socks5" }[info.type];
  return scheme ? scheme + "://" + bracketed(info.host) + ":" + info.port : undefined;
}

function bracketed(host) {
  return host.includes(":") && !host.startsWith("[") ? "[" + host + "]" : host;
}

/** In Chromium, the fixed proxy another extension put in place, unless the
 * URL bypasses it. A PAC script cannot be evaluated here. */
async function extensionProxy(url) {
  if (isFirefox || !api.proxy?.settings) return undefined;
  let config;
  try {
    config = await api.proxy.settings.get({});
  } catch {
    return undefined;
  }
  if (!/^controlled_by_(other|this)_extensions?$/.test(config?.levelOfControl ?? "")) return undefined;
  const value = config.value ?? {};
  if (value.mode !== "fixed_servers" || !value.rules) return undefined;
  const target = new URL(url);
  const rules = value.rules;
  if (bypasses(target.hostname, rules.bypassList ?? [])) return undefined;
  const server =
    rules.singleProxy ??
    (target.protocol === "https:" ? rules.proxyForHttps : target.protocol === "http:" ? rules.proxyForHttp : undefined) ??
    rules.fallbackProxy;
  if (!server?.host) return undefined;
  const scheme = { http: "http", socks5: "socks5" }[server.scheme ?? "http"];
  const port = server.port ?? (scheme === "http" ? 80 : 1080);
  return scheme ? scheme + "://" + bracketed(server.host) + ":" + port : undefined;
}

/** Chromium's bypass rules for a host: patterns such as "*.example.com",
 * "<local>", and loopback addresses unless "<-loopback>" is listed. */
function bypasses(host, list) {
  const loopback = host === "localhost" || host.endsWith(".localhost") || /^127\./.test(host) || host === "[::1]";
  if (loopback && !list.includes("<-loopback>")) return true;
  return list.some((rule) => {
    if (rule === "<local>") return !host.includes(".");
    if (rule.startsWith("<")) return false;
    const pattern = rule.replace(/^[a-z]+:\/\//i, "").replace(/:\d+$/, "").toLowerCase();
    if (pattern.startsWith("*.")) return host.endsWith(pattern.slice(1)) || host === pattern.slice(2);
    if (pattern.startsWith(".")) return host.endsWith(pattern);
    return host === pattern;
  });
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
  if (item.byExtensionId) return false;
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
  const taken = await offer(item);
  if (taken) {
    await api.downloads.cancel(item.id).catch(noteError);
    await api.downloads.erase({ id: item.id }).catch(noteError);
  } else {
    await api.downloads.resume(item.id).catch(() => {});
  }
  return taken;
}

/** Offers a download to the app; true when it took it. */
async function offer(item) {
  try {
    const request = requests.get(item.url) ?? requests.get(item.finalUrl);
    const base = await requestItem(item.url, { referrer: item.referrer, headers: request?.headers, storeId: await storeOf(item) });
    const name = basename(item.filename);
    return await handOver("download", [
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
    return false;
  }
}

/** The cookie store of a download's window. Firefox names it on the item;
 * Chromium keeps one store for all Incognito windows. */
async function storeOf(item) {
  if (item.cookieStoreId || !item.incognito) return item.cookieStoreId;
  const [stores, tabs] = await Promise.all([api.cookies.getAllCookieStores(), api.tabs.query({})]);
  const privateTabs = new Set(tabs.filter((tab) => tab.incognito).map((tab) => tab.id));
  return stores.find((store) => store.tabIds.some((id) => privateTabs.has(id)))?.id;
}

// Types a browser shows in the page when they come without "attachment".
const INLINE = /^(text|image|video|audio)\/|^application\/(pdf|json|xml|xhtml\+xml|javascript|ecmascript)$|[+](xml|json)$/;

/** The name in a Content-Disposition header, or "". */
function dispositionName(value) {
  const encoded = /filename\*\s*=\s*[^']*'[^']*'([^;]+)/i.exec(value);
  if (encoded) {
    try {
      return decodeURIComponent(encoded[1].trim());
    } catch {}
  }
  const plain = /filename\s*=\s*(?:"([^"]*)"|([^;]+))/i.exec(value);
  return plain ? (plain[1] ?? plain[2]).trim() : "";
}

/** A response the browser will save rather than show, as a download item
 * for shouldCapture, or null. */
function responseDownload(details) {
  if (details.method !== "GET" || ![200, 206].includes(details.statusCode)) return null;
  const disposition = header(details.responseHeaders, "content-disposition");
  const mime = header(details.responseHeaders, "content-type").split(";")[0].trim().toLowerCase();
  if (!/^\s*attachment/i.test(disposition) && (!mime || INLINE.test(mime))) return null;
  return {
    state: "in_progress",
    url: details.url,
    finalUrl: details.url,
    filename: dispositionName(disposition),
    mime,
    totalBytes: responseSize(details),
    referrer: header(requests.get(details.url)?.headers, "referer") || details.originUrl || "",
    incognito: details.incognito,
    cookieStoreId: details.cookieStoreId,
  };
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
  // A download Firefox asks where to save exists only after its Save dialog,
  // too late to take over. So, as IDM's Firefox add-on does, the response is
  // caught before Firefox decides what to do with it. Needs webRequestBlocking,
  // which only the Firefox package asks for.
  try {
    api.webRequest.onHeadersReceived.addListener(
      (details) => {
        const item = responseDownload(details);
        if (!item) return {};
        return ready.then(async () => {
          if (!shouldCapture(item)) return {};
          if (await offer(item)) return { cancel: true };
          // Declined: Firefox keeps it, and onCreated must not offer it again.
          bypass.until = Date.now() + 5_000;
          bypass.url = item.url;
          return {};
        });
      },
      { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] },
      ["blocking", "responseHeaders"],
    );
  } catch (error) {
    noteError(error);
  }
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
  if (mediaBlocked(found.url)) return;
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

// A new page in the tab, including a site that changes pages in script
// (YouTube); a link that turns into a download does not change the URL.
const tabPages = new Map(); // tabId → page URL without its #fragment
api.tabs.onUpdated.addListener((tabId, change) => {
  if (!change.url) return;
  const page = change.url.split("#")[0];
  if (tabPages.get(tabId) !== page) clearMedia(tabId);
  tabPages.set(tabId, page);
});
api.tabs.onRemoved.addListener((tabId) => {
  media.delete(tabId);
  tabPages.delete(tabId);
});

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
 * Without an extension it is only the name: the engine adds the container's. */
function titleName(title, extension) {
  const name = (title || "").replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 120);
  if (!name) return undefined;
  return extension ? name + "." + extension : name;
}

/** "hls", "dash", "collection" (files the engine joins) or undefined for a plain file. */
const mediaKindOf = (entry) => (isStream(entry) ? entry.kind.toLowerCase() : entry.site?.files ? "collection" : undefined);

/** What the app needs to fetch this find again, as the page did. */
async function mediaItem(tab, entry) {
  const url = entry.page ?? entry.url;
  // yt-dlp's finds come with the headers the site wants; the app drops the
  // ones the engine sets itself.
  const seen = entry.site?.headers ?? entry.headers;
  const item = await requestItem(url, { referrer: tab?.url ?? "", headers: seen, storeId: tab?.cookieStoreId });
  const headers = entry.site
    ? seen
    : seen.filter((h) => ["origin", "authorization"].includes(h.name.toLowerCase()) || /^x-/i.test(h.name));
  const kind = mediaKindOf(entry);
  const filename = titleName(entry.site?.title || tab?.title, kind ? undefined : entry.kind.toLowerCase());
  const media =
    kind === "collection"
      ? { mediaKind: kind, mediaManifest: JSON.stringify(entry.site.files) }
      : kind
        ? { mediaKind: kind, mediaManifest: await manifestText(entry) }
        : {};
  return { ...item, headers, filename, filenameSource: filename ? "title" : undefined, ...media };
}

/** The stream's playlist, fetched once and then always the same text.
 *
 * The engine names tracks after their playlist URLs, and some servers sign
 * those URLs afresh on every request (YouTube does), so a playlist fetched
 * again for the download would not have the track the menu offered. The
 * menu's probe and the download both read this one copy instead. */
async function manifestText(entry) {
  if (entry.content) return entry.content;
  entry.fetching ??= fetch(entry.url, { credentials: "include" })
    .then((response) => (response.ok ? response.text() : undefined))
    .then((text) => (text && text.length <= 2 * 1024 * 1024 ? text : undefined))
    .catch(() => undefined);
  const text = await entry.fetching;
  if (text) entry.content ??= text;
  return entry.content || undefined;
}

/** The stream's tracks, read by the app's engine; kept for a while. */
function inspect(tab, entry) {
  if (entry.probe && Date.now() - entry.probe.at < PROBE_TTL) return entry.probe.promise;
  const promise = mediaItem(tab, entry).then((item) => ask({ type: "inspect", item }, { timeoutMs: 40_000 }));
  entry.probe = { at: Date.now(), promise };
  promise.catch(() => entry.probe?.promise === promise && (entry.probe = null));
  return promise;
}

// ─── Sites through yt-dlp ────────────────────────────────────────────────
//
// Sites like YouTube never show the browser a playlist or file. The app
// asks yt-dlp about the page, with this browser's cookies for the site so
// that what the user can watch is what yt-dlp can see.

const sites = new Map(); // page URL → { at, ttl, promise }
const SITE_TTL = 10 * 60_000;

/** "youtube.com" for "www.youtube.com", "bbc.co.uk" for "www.bbc.co.uk". */
function siteDomain(host) {
  const parts = host.split(".");
  const keep = parts.length > 2 && parts.at(-1).length === 2 && parts.at(-2).length <= 3 ? 3 : 2;
  return parts.slice(-keep).join(".");
}

async function siteCookies(tab, url) {
  try {
    const domain = siteDomain(new URL(url).hostname);
    const cookies = await api.cookies.getAll(tab?.cookieStoreId ? { domain, storeId: tab.cookieStoreId } : { domain });
    return cookies.slice(0, 500).map(({ domain, hostOnly, path, secure, expirationDate, name, value }) => ({
      domain, hostOnly, path, secure, expirationDate, name, value,
    }));
  } catch {
    return [];
  }
}

/** yt-dlp's videos for a page, looked up once and kept a while. */
function siteLookup(tab, url) {
  if (!/^https?:/i.test(url)) return Promise.resolve([]);
  const cached = sites.get(url);
  if (cached && Date.now() - cached.at < cached.ttl) return cached.promise;
  const record = { at: Date.now(), ttl: SITE_TTL };
  record.promise = siteCookies(tab, url)
    .then((cookies) => ask({ type: "site", url, cookies }, { timeoutMs: 120_000 }))
    .then((answer) => {
      if (answer.result !== "ready") {
        if (answer.retry) record.ttl = 0;
        throw new Error(answer.reason || "yt-dlp could not read this page");
      }
      return answer.videos;
    });
  // A failure is kept briefly: a page yt-dlp does not know stays unknown,
  // but OpenDM may simply have been starting.
  record.promise.catch(() => (record.ttl = Math.min(record.ttl, 30_000)));
  sites.set(url, record);
  if (sites.size > 50) sites.delete(sites.keys().next().value);
  return record.promise;
}

const hasCodec = (codec) => Boolean(codec) && codec !== "none";
const CODEC_ORDER = ["H.264", "VP9", "AV1"]; // What plays in most places first.

/** Adds yt-dlp's playlists and files for a page to the tab's finds.
 *
 * With a playlist, the qualities come from it, and only complete files (sound
 * included) add anything. Without one (YouTube seen through a signed-in
 * browser gives yt-dlp separate video and audio files), each picture quality
 * is a video file the engine joins with the video's own soundtrack. */
function addSiteVideo(tabId, video, page) {
  const title = video.title || "";
  // What yt-dlp knows about each picture size: exact file sizes (a stream's
  // playlist only states peak bitrates) and frame rates.
  const known = {};
  for (const f of video.files) {
    if (!hasCodec(f.vcodec) || hasCodec(f.acodec) || !f.width) continue;
    known[f.width + "x" + f.height + "|" + codecName(f.vcodec, "video")] = { size: f.size ?? 0, fps: f.fps ?? 0 };
  }
  const audioSize = Math.max(0, ...video.files.filter((f) => !hasCodec(f.vcodec) && f.ext === "m4a").map((f) => f.size ?? 0));
  for (const stream of video.streams) {
    const site = { page, title, headers: stream.headers, known, audioSize, language: video.language || "" };
    addMedia(tabId, { url: stream.url, kind: stream.kind.toUpperCase(), loaded: true, site });
  }
  // "-drc" files are the same audio with its loudness evened out.
  const files = video.files.filter((file) => !String(file.id).endsWith("-drc") && file.ext && /^https?:/i.test(file.url || ""));
  const pictures = files.filter((f) => hasCodec(f.vcodec));
  // The soundtracks, the video's own language first, then the best of each.
  const audios = files
    .filter((f) => !hasCodec(f.vcodec) && hasCodec(f.acodec))
    .sort((a, b) => (b.preference ?? 0) - (a.preference ?? 0) || (b.tbr ?? 0) - (a.tbr ?? 0) || (b.ext === "m4a") - (a.ext === "m4a"));
  const soundtrack = audios[0];
  const add = (file, group, label, detail, extra = {}) =>
    addMedia(tabId, {
      url: file.url,
      kind: file.ext.toUpperCase(),
      size: extra.size ?? file.size ?? 0,
      loaded: true,
      site: { page, title, headers: file.headers, group, label, detail, height: file.height ?? 0, ...extra },
    });
  const quality = (f) => (f.height ? qualityName(f.width, f.height) + (f.fps > 31 ? Math.round(f.fps) : "") : f.ext.toUpperCase());
  const codecRank = (f) => {
    const rank = CODEC_ORDER.indexOf(codecName(f.vcodec, "video"));
    return rank < 0 ? CODEC_ORDER.length : rank;
  };
  // One entry per picture quality: a complete file over one needing sound,
  // then the codec that plays in most places.
  const seen = new Set();
  for (const f of pictures.sort(
    (a, b) =>
      (b.height ?? 0) * (b.width ?? 0) - (a.height ?? 0) * (a.width ?? 0) ||
      hasCodec(b.acodec) - hasCodec(a.acodec) ||
      codecRank(a) - codecRank(b) ||
      (b.fps ?? 0) - (a.fps ?? 0) ||
      (b.tbr ?? 0) - (a.tbr ?? 0),
  )) {
    const complete = hasCodec(f.acodec);
    if (!complete && video.streams.length) continue;
    const label = quality(f);
    if (seen.has(label)) continue;
    seen.add(label);
    const codec = codecName(f.vcodec, "video");
    if (complete) {
      add(f, "Video", label, [f.ext.toUpperCase(), codec, formatSize(f.size)].filter(Boolean).join(" · "));
      continue;
    }
    // The engine joins the picture with the soundtrack into one file.
    const sound = soundtrack ? [{ type: "audio", url: soundtrack.url, size: soundtrack.size ?? 0 }] : [];
    const format = containerFor(f.vcodec + "," + (soundtrack?.acodec ?? ""));
    const size = (f.size ?? 0) + (soundtrack?.size ?? 0);
    const detail = [format.toUpperCase(), codec, formatSize(size)].filter(Boolean).join(" · ");
    const choice = { url: f.url, video: "video", audio: sound.length ? "audio" : "none", format, quality: label + " · " + detail };
    add(f, sound.length ? "Video" : "Video (no sound)", label, detail, { size, files: [{ type: "video", url: f.url, size: f.size ?? 0 }, ...sound], choice });
  }
  // Audio on its own, the best of each language; a playlist already has it.
  if (video.streams.length) return;
  const heard = new Set();
  for (const f of audios) {
    const language = f.language || "";
    if (heard.has(language)) continue;
    heard.add(language);
    const original = f === soundtrack && audios.some((other) => (other.language || "") !== language);
    const detail = [original ? "Original" : "", f.tbr ? Math.round(f.tbr) + " kbps" : "", codecName(f.acodec, "audio"), formatSize(f.size)].filter(Boolean).join(" · ");
    add(f, "Audio only", language || "Audio", detail);
  }
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

/** MP4 plays everywhere; the engine writes WebM-family codecs (VP9, AV1,
 * Opus, ...) correctly only into MKV. */
function containerFor(codecs) {
  return /vp0?[89]|av01|opus|vorbis|theora/i.test(codecs) ? "mkv" : "mp4";
}

function streamName(url) {
  try {
    const parts = new URL(url).pathname.split("/").filter(Boolean);
    return decodeURIComponent(parts.pop() || new URL(url).hostname);
  } catch {
    return url;
  }
}

const QUALITY_STEPS = [144, 240, 360, 480, 720, 1080, 1440, 2160, 4320];

/** A picture size as video sites name it: by the 16:9 size it matches, so
 * a wide 3840×1920 video is "2160p" and a 1080×1920 phone video "1080p". */
function qualityName(width, height) {
  if (!height) return "";
  const long = Math.max(width || 0, height);
  const short = Math.min(width || height, height);
  const lines = Math.max(short, Math.round((long * 9) / 16));
  const step = QUALITY_STEPS.find((p) => Math.abs(p - lines) / p < 0.04);
  return (step ?? lines) + "p";
}

/** Menu groups for one stream: one entry per picture quality, then audio
 * on its own, one entry per language.
 *
 * Where a quality comes in both H.264 and VP9, H.264 goes first because it
 * plays everywhere, except for streams found through yt-dlp (YouTube): the
 * engine (aria2-next 3.0.6) fails to remux YouTube's H.264 segments, whose
 * first packets carry no timestamps, while VP9 works. */
function streamGroups(entry, presentation) {
  const seconds = presentation.live ? 0 : presentation.durationMs / 1000;
  const estimate = (bandwidth) => (seconds && bandwidth ? "≈ " + formatSize((bandwidth * seconds) / 8) : "");
  const audios = presentation.tracks.filter((t) => t.type === "audio").sort((a, b) => b.bandwidth - a.bandwidth);
  const bestAudio = audios[0];
  const known = entry.site?.known ?? {};
  const preferred = entry.site ? "VP9" : "H.264";
  // The video's own language, not a dubbed one, goes with the picture.
  const original = entry.site?.language;
  const withAudio = original && audios.some((t) => t.language === original) ? original : "best";
  const seen = new Set();
  const video = presentation.tracks
    .filter((t) => t.type === "video" || t.type === "muxed")
    .map((track) => ({ track, codec: codecName(track.codec, "video"), facts: known[track.width + "x" + track.height + "|" + codecName(track.codec, "video")] }))
    .sort(
      (a, b) =>
        b.track.height * b.track.width - a.track.height * a.track.width ||
        (b.codec === preferred) - (a.codec === preferred) ||
        b.track.frameRate - a.track.frameRate ||
        b.track.bandwidth - a.track.bandwidth,
    )
    .flatMap(({ track, codec, facts }) => {
      const rate = Math.max(track.frameRate, facts?.fps ?? 0);
      const label = track.height ? qualityName(track.width, track.height) + (rate > 31 ? Math.round(rate) : "") : kbps(track.bandwidth);
      if (seen.has(label)) return [];
      seen.add(label);
      const audioBandwidth = track.type === "video" ? bestAudio?.bandwidth ?? 0 : 0;
      const size = facts?.size ? "≈ " + formatSize(facts.size + (track.type === "video" ? entry.site.audioSize : 0)) : estimate(track.bandwidth + audioBandwidth);
      const detail = [presentation.live ? "Live" : "", codec, size].filter(Boolean).join(" · ");
      const format = containerFor(track.codec + "," + (bestAudio?.codec ?? ""));
      return [{ label, detail, choice: { url: entry.url, video: track.id, audio: withAudio, format, quality: label + " · " + detail } }];
    });
  const heard = new Set();
  const audio = [...audios].sort((a, b) => (b.language === original) - (a.language === original)).flatMap((track) => {
    const codec = codecName(track.codec, "audio");
    // The best bitrate of each language.
    if (heard.has(track.language)) return [];
    heard.add(track.language);
    // Short labels keep the menu's label column narrow; the group says it is audio.
    const isOriginal = track.language === original && audios.some((t) => t.language !== original);
    const detail = [isOriginal ? "Original" : "", track.bandwidth ? kbps(track.bandwidth) : "", codec, estimate(track.bandwidth)].filter(Boolean).join(" · ");
    return [{
      label: track.language || "Audio",
      detail,
      choice: { url: entry.url, video: "none", audio: track.id, format: containerFor(track.codec), quality: ["Audio only", track.language, detail].filter(Boolean).join(" · ") },
    }];
  });
  return [
    { title: "Video", options: video },
    { title: "Audio only", options: audio },
  ].filter((group) => group.options.length);
}

/** Everything the tab's video menu offers. src is the hovered video's own
 * URL and manifest the playlist its player names, when it does; page is the
 * page to ask yt-dlp about when it is not the tab's. */
async function formats(tabId, src, page, manifest = "") {
  const tab = await api.tabs.get(tabId).catch(() => null);
  if (mediaBlocked(page || tab?.url || "")) {
    return { title: tab?.title ?? "", groups: [], note: "This version of OpenDM Connect does not download videos from this site." };
  }
  const pageUrl = page || tab?.url || "";
  // yt-dlp's finds for other pages (videos hovered on this one) stay out.
  const forPage = (entry) => !entry.site || entry.site.page === pageUrl;
  // yt-dlp first, since it adds finds. With finds from the page already in
  // hand, a slow answer is not worth waiting long for.
  const ownFinds = visibleMedia(tabId).some(forPage);
  let siteNote = "";
  const site = siteLookup(tab, pageUrl).catch((error) => {
    siteNote = error.message;
    return [];
  });
  // yt-dlp usually answers in ten seconds. A slower page gets what is known so
  // far and a note; the lookup goes on and the next click finds it done.
  const settled = await Promise.race([site.then(() => true), new Promise((resolve) => setTimeout(() => resolve(false), ownFinds ? 4000 : 15_000))]);
  const videos = settled ? await site : [];
  // A fresh answer replaces the page's earlier one: its URLs are signed anew.
  if (videos.length) {
    const fresh = new Set(videos.flatMap((video) => [...video.streams, ...video.files].map((f) => f.url)));
    media.set(tabId, (media.get(tabId) ?? []).filter((entry) => !entry.site || entry.site.page !== pageUrl || fresh.has(entry.url)));
  }
  for (const video of videos) addSiteVideo(tabId, video, pageUrl);

  const list = visibleMedia(tabId).filter(forPage).reverse(); // Newest first.
  if (src && /^https?:/i.test(src) && !list.some((entry) => entry.url === src)) {
    const kind = mediaKind(src, "") || "MP4";
    addMedia(tabId, { url: src, kind, loaded: true });
    list.unshift(media.get(tabId).find((entry) => entry.url === src));
  }
  // A player that names its playlist gets a menu for that video alone; the
  // tab's other finds belong to other videos on the page.
  const own = manifest && /^https?:/i.test(manifest) && mediaKind(manifest, "") ? manifest : "";
  if (own && !(media.get(tabId) ?? []).some((entry) => entry.url === own)) {
    addMedia(tabId, { url: own, kind: mediaKind(own, ""), loaded: true });
  }
  // yt-dlp's files come in menu order already; the page's own finds are capped.
  const siteFiles = list.filter((entry) => !isStream(entry) && entry.site).reverse();
  const pageFiles = list
    .filter((entry) => !isStream(entry) && !entry.site && (!own || entry.url === src))
    .sort((a, b) => (b.url === src) - (a.url === src) || b.size - a.size);
  const files = [...siteFiles, ...pageFiles.slice(0, 12)];

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
  const streams = [...(media.get(tabId) ?? [])]
    .reverse()
    .filter((entry) => isStream(entry) && !entry.variant && forPage(entry) && (!own || entry.url === own));
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
  const addTo = (title, options) => {
    const group = groups.find((g) => g.title === title);
    if (group) group.options.push(...options);
    else if (options.length) groups.push({ title, options });
  };
  shown.forEach((p, index) => {
    const own = streamGroups(p.entry, p.answer.presentation);
    if (index === 0) own.forEach((g) => addTo(g.title, g.options));
    else addTo("Another stream · " + hostOf(p.entry.url), own.flatMap((g) => g.options));
  });
  const failed = probed.find((p) => p.answer.result !== "ready");
  if (!shown.length && failed) {
    // The engine could not read it here; it may still download.
    addTo("Stream", [
      { label: "Best quality", detail: failed.entry.kind + " · " + (failed.answer.reason || "qualities unknown"), choice: { url: failed.entry.url, video: "best", audio: "best" } },
    ]);
  }
  for (const entry of files) {
    const option = entry.site
      ? { label: entry.site.label, detail: entry.site.detail, choice: entry.site.choice ?? { url: entry.url } }
      : { label: entry.kind, detail: [formatSize(entry.size), streamName(entry.url)].filter(Boolean).join(" · "), choice: { url: entry.url } };
    addTo(entry.site?.group ?? "Files", [option]);
  }
  // Why nothing was found, when yt-dlp knows and the page is one it handles.
  const note = groups.length ? "" : !settled ? "OpenDM is still reading this page. Try again in a moment." : /unsupported url/i.test(siteNote) ? "" : siteNote;
  return { title: tab?.title ?? "", groups, note };
}

async function downloadChoice(tabId, choice) {
  const entry = media.get(tabId)?.find((item) => item.url === choice?.url);
  if (!entry) throw new Error("That video is gone; reload the page");
  const tab = await api.tabs.get(tabId).catch(() => null);
  const item = await mediaItem(tab, entry);
  if (mediaKindOf(entry)) {
    Object.assign(item, { mediaVideo: choice.video, mediaAudio: choice.audio, mediaFormat: choice.format, mediaQuality: choice.quality });
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

/** The page a video belongs to: the one it previews, or the embedded
 * player's own page (a YouTube player on a blog), else the tab's. */
function videoPage(message, sender) {
  if (typeof message.link === "string" && /^https?:/i.test(message.link)) return message.link;
  return sender.frameId ? sender.url : "";
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
      return { ok: true, ...(await formats(tabId, message.src, videoPage(message, sender), typeof message.manifest === "string" ? message.manifest : "")) };
    }
    case "prefetch":
      // Hovering a video: look the page up before the click, if OpenDM runs.
      if (sender.tab && app.state === "connected") formats(sender.tab.id, "", videoPage(message, sender)).catch(() => {});
      return { ok: true };
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
        hasVideo: await api.scripting
          .executeScript({ target: { tabId: message.tabId, allFrames: true }, func: () => Boolean(document.querySelector("video")) })
          .then((results) => results.some((result) => result.result), () => false),
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
