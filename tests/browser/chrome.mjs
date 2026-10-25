// Chrome test: Google Chrome + OpenDM Connect + opendm-host + OpenDM.
// Drives Chrome over raw CDP so the browser's own download handling stays in
// charge (Playwright would take downloads over).
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";


// Chrome itself: test builds are blocked from the network on this Mac. Chrome
// 137+ ignores --load-extension, so the extension is loaded over a DevTools pipe.
// CHROME can name another Chromium browser, such as Edge.
const WINDOWS = process.platform === "win32";
const CHROME = process.env.CHROME ?? (WINDOWS
  ? "C:/Program Files/Google/Chrome/Application/chrome.exe"
  : "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
const ID = "nkhhoiejpomapjkeomdbflaicnnjclfj";
const REPO = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
const ROOT = process.env.OPENDM_E2E_DIR ?? path.join(os.tmpdir(), "opendm-e2e");
const HOST = WINDOWS
  ? path.join(REPO, "build/windows/OpenDM-x64/opendm-host.exe")
  : REPO + "/build/OpenDM.app/Contents/MacOS/opendm-host";
// Each run starts clean in its own folder.
const RUN = ROOT + "/run-" + Date.now();
const DATA = RUN + "/data", PROFILE = RUN + "/profile", APP_DL = RUN + "/dl", BROWSER_DL = RUN + "/browser-dl";
const SITE = "http://127.0.0.1:8770";
const PORT = 9333;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
function check(name, ok, detail = "") {
  results.push({ name, ok, detail });
  console.log((ok ? "PASS " : "FAIL ") + name + (detail ? " — " + detail : ""));
}
async function until(what, ms, fn) {
  const end = Date.now() + ms;
  for (;;) {
    const value = await fn().catch(() => undefined);
    if (value) return value;
    if (Date.now() > end) return undefined;
    await sleep(200);
  }
}

class Cdp {
  static async open(url) {
    const cdp = new Cdp();
    cdp.ws = new WebSocket(url);
    cdp.next = 1;
    cdp.pending = new Map();
    cdp.ws.onmessage = (event) => {
      const message = JSON.parse(event.data);
      const waiter = cdp.pending.get(message.id);
      if (waiter) {
        cdp.pending.delete(message.id);
        message.error ? waiter.reject(new Error(message.error.message)) : waiter.resolve(message.result);
      }
    };
    await new Promise((resolve, reject) => ((cdp.ws.onopen = resolve), (cdp.ws.onerror = reject)));
    return cdp;
  }
  send(method, params = {}) {
    const id = this.next++;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }
  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    return result.result.value;
  }
  async screenshot(path) {
    const { data } = await this.send("Page.captureScreenshot", { format: "png" });
    fs.writeFileSync(path, Buffer.from(data, "base64"));
  }
  /** Elements with a class, closed shadow roots included, with their text and center. */
  async find(className) {
    const { root } = await this.send("DOM.getDocument", { depth: -1, pierce: true });
    const found = [];
    const textOf = (node) => (node.nodeType === 3 ? node.nodeValue : (node.children ?? []).map(textOf).join(" "));
    const walk = (node) => {
      const attributes = node.attributes ?? [];
      const classes = attributes[attributes.indexOf("class") + 1] ?? "";
      if (attributes.includes("class") && classes.split(" ").includes(className)) found.push(node);
      for (const child of [...(node.children ?? []), ...(node.shadowRoots ?? [])]) walk(child);
    };
    walk(root);
    return Promise.all(
      found.map(async (node) => {
        const { model } = await this.send("DOM.getBoxModel", { backendNodeId: node.backendNodeId });
        const [x1, y1, , , x3, y3] = model.border;
        return { text: textOf(node).replace(/\s+/g, " ").trim(), x: (x1 + x3) / 2, y: (y1 + y3) / 2 };
      }),
    );
  }
  async click(x, y) {
    await this.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
    await this.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
    await this.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  }
}

const targets = async () => (await fetch("http://127.0.0.1:" + PORT + "/json/list")).json();
function serverRequests(path) {
  if (!fs.existsSync(ROOT + "/requests.log")) return [];
  return fs.readFileSync(ROOT + "/requests.log", "utf8").trim().split("\n").filter(Boolean).map(JSON.parse).filter((r) => r.path === path);
}
const engineRequests = (path) => serverRequests(path).filter((r) => !r.headers["Sec-Fetch-Dest"] && !r.headers["sec-fetch-dest"]);
const listFiles = (dir) => (fs.existsSync(dir) ? fs.readdirSync(dir, { recursive: true }).map((f) => String(f).replaceAll("\\", "/")) : []);
const videoHeight = (file) =>
  execFileSync("ffprobe", ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=height", "-of", "csv=p=0", file]).toString().trim();

// ─── Setup ──────────────────────────────────────────────────────────────
for (const dir of [DATA, PROFILE + "/Default", PROFILE + "/NativeMessagingHosts", APP_DL, BROWSER_DL]) fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(DATA + "/settings.json", JSON.stringify({ download_dir: APP_DL, confirm_downloads: false, show_progress_window: false }));
// Chrome on Windows cannot save to a folder written with mixed slashes.
fs.writeFileSync(PROFILE + "/Default/Preferences", JSON.stringify({ download: { default_directory: path.resolve(BROWSER_DL), prompt_for_download: false } }));
const hostManifest = PROFILE + "/NativeMessagingHosts/org.opendm.host.json";
fs.writeFileSync(
  hostManifest,
  JSON.stringify({
    name: "org.opendm.host", description: "OpenDM", type: "stdio",
    path: HOST,
    allowed_origins: ["chrome-extension://" + ID + "/"],
  }),
);
// Chromium on Windows finds hosts in the registry only; the app writes the
// same key at every launch.
if (WINDOWS) {
  for (const browser of ["Google\\Chrome", "Microsoft\\Edge"]) {
    execFileSync("reg", ["add", "HKCU\\Software\\" + browser + "\\NativeMessagingHosts\\org.opendm.host", "/ve", "/d", path.resolve(hostManifest), "/f"], { stdio: "ignore" });
  }
}
fs.writeFileSync(ROOT + "/requests.log", "");

const chrome = spawn(CHROME, [
  "--user-data-dir=" + PROFILE, "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=" + PORT,
  "--remote-debugging-pipe", "--enable-unsafe-extension-debugging", "--window-size=1100,800", "about:blank",
], { env: { ...process.env, OPENDM_DATA_DIR: DATA }, stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] });

/** Loads the unpacked extension through the DevTools pipe (fd 3 in, fd 4 out). */
function loadExtension() {
  return new Promise((resolve, reject) => {
    let buffer = "";
    chrome.stdio[4].on("data", (chunk) => {
      buffer += chunk.toString();
      for (let end; (end = buffer.indexOf("\0")) >= 0; buffer = buffer.slice(end + 1)) {
        const message = JSON.parse(buffer.slice(0, end));
        if (message.id === 1) message.error ? reject(new Error(message.error.message)) : resolve(message.result.id);
      }
    });
    // Allowed in Incognito, as a user can allow it in chrome://extensions.
    const params = { path: path.join(REPO, "extension"), enableInIncognito: true };
    chrome.stdio[3].write(JSON.stringify({ id: 1, method: "Extensions.loadUnpacked", params }) + "\0");
  });
}

try {
  const loadedId = await loadExtension();
  check("extension loads with the fixed ID", loadedId === ID, loadedId);
  const worker = await until("the extension", 15000, async () => (await targets()).find((t) => t.type === "service_worker" && t.url.includes(ID)));
  const sw = await Cdp.open(worker.webSocketDebuggerUrl);
  const pageTarget = (await targets()).find((t) => t.type === "page");
  const page = await Cdp.open(pageTarget.webSocketDebuggerUrl);
  await page.send("Page.enable");
  // Records each toolbar icon the extension shows. The worker's extension
  // bindings need the runtime domain enabled first.
  await sw.send("Runtime.enable");
  await sw.evaluate("globalThis.icons = []; const setIcon = chrome.action.setIcon.bind(chrome.action); chrome.action.setIcon = (details) => (icons.push(details.path), setIcon(details)); true");

  // 1. A captured download starts OpenDM (not running yet) in the background.
  await page.send("Page.navigate", { url: SITE + "/index.html?auto=zip" });
  const saved = await until("the app's file", 45000, async () => listFiles(APP_DL).find((f) => f.endsWith("Quarterly Report.zip")));
  check("captured download is saved by OpenDM", Boolean(saved), saved);
  if (saved) check("file is complete", fs.statSync(APP_DL + "/" + saved).size === 6291456);
  const engine = engineRequests("/files/report.zip").at(-1);
  const browser = serverRequests("/files/report.zip").findLast((r) => r.headers["Sec-Fetch-Dest"] || r.headers["sec-fetch-dest"]);
  check("engine sends the browser's cookie", engine?.headers.Cookie?.includes("session=abc123"), engine?.headers.Cookie);
  check("engine sends the page as referer", engine?.headers.Referer === SITE + "/index.html?auto=zip", engine?.headers.Referer);
  check("engine sends the browser's user agent", /Chrome\//.test(engine?.headers["User-Agent"] ?? ""), engine?.headers["User-Agent"]);
  check("engine sends the browser's language", Boolean(browser?.headers["Accept-Language"]) && engine?.headers["Accept-Language"] === browser?.headers["Accept-Language"], engine?.headers["Accept-Language"]);
  const icons = await until("the toolbar icon fill", 5000, async () => {
    const shown = await sw.evaluate("icons.map((path) => path[16])");
    return shown.length === 27 ? shown : undefined;
  });
  check("taking the download fills the toolbar icon, then restores it",
    icons?.[0] === "icons/fill/0-16.png" && icons?.[25] === "icons/fill/25-16.png" && icons?.[26] === "icons/icon-16.png",
    JSON.stringify(icons ?? (await sw.evaluate("icons.map((path) => path[16])"))));
  const leftover = () => sw.evaluate("chrome.downloads.search({}).then(items => items.map(i => JSON.stringify({ id: i.id, name: i.filename, state: i.state, error: i.error, paused: i.paused })))");
  const dropped = await until("the browser to drop its copy", 5000, async () => (await leftover()).length === 0);
  check("browser dropped its copy", Boolean(dropped), (await leftover()).join(" ") + " lastError=" + (await sw.evaluate("lastError")));
  check("no file in the browser's folder", listFiles(BROWSER_DL).length === 0, listFiles(BROWSER_DL).join(", "));

  // 1b. A server that refuses connections past its limit (pixeldrain) still gets the whole file.
  const limitedSource = fs.readFileSync(ROOT + "/site/files/limited.bin");
  await page.send("Page.navigate", { url: SITE + "/index.html?auto=limited" });
  const limited = await until("the whole limited file", 60000, async () => {
    const name = listFiles(APP_DL).find((f) => f.endsWith("limited.mp4"));
    return name && fs.readFileSync(APP_DL + "/" + name).equals(limitedSource) && name;
  });
  const refusals = serverRequests("/files/limited.bin?download").length;
  check("a connection-limited download completes", Boolean(limited), (limited || "incomplete") + " after " + refusals + " requests");

  // 2. Alt-click keeps the download in the browser.
  await page.send("Page.navigate", { url: SITE + "/index.html?auto=alt" });
  const kept = await until("the browser's download", 20000, async () => {
    const items = await sw.evaluate("chrome.downloads.search({ state: 'complete' }).then(items => items.map(i => i.filename))");
    return items.find((name) => name.includes("Quarterly Report"));
  });
  check("Alt-click download stays in the browser", Boolean(kept), kept ?? (await leftover()).join(" "));
  const zips = listFiles(APP_DL).filter((f) => f.includes("Quarterly Report"));
  check("OpenDM did not get the Alt-click download", zips.length === 1, zips.join(", "));

  // 2b. A private window's download is captured too, with that window's cookies.
  // Alt-click left the next download of the same link to the browser for 5 s.
  await sleep(5000);
  const privateWindow = await sw.evaluate("chrome.windows.create({ incognito: true, focused: false, url: '" + SITE + "/index.html?private' }).then((w) => w.id)");
  const privateTarget = await until("the private page", 5000, async () => (await targets()).find((t) => t.type === "page" && t.url.endsWith("?private")));
  const privatePage = await Cdp.open(privateTarget.webSocketDebuggerUrl);
  await until("the private page to load", 5000, () => privatePage.evaluate("document.readyState === 'complete'"));
  const engineBefore = engineRequests("/files/report.zip").length;
  await privatePage.evaluate("document.cookie = 'private=yes; path=/'; document.getElementById('zip').click(); true");
  const privateCookie = await until("the engine's request", 30000, async () =>
    engineRequests("/files/report.zip").slice(engineBefore).map((r) => r.headers.Cookie ?? "").find((cookie) => cookie.includes("private=yes")));
  check("a private window's download goes to OpenDM with that window's cookies", Boolean(privateCookie), (privateCookie ?? "no request") + " lastError=" + (await sw.evaluate("lastError")));
  const privateLeft = () => sw.evaluate("chrome.downloads.search({}).then((items) => items.filter((i) => i.incognito).map((i) => i.state))");
  check("the private window drops its copy", Boolean(await until("the private copy to go", 5000, async () => (await privateLeft()).length === 0)), JSON.stringify(await privateLeft()));
  privatePage.ws.close();
  await sw.evaluate("chrome.windows.remove(" + privateWindow + ")");
  await page.send("Page.bringToFront");

  // 3. The page's script finds the master playlist inside a JSON answer and
  //    loads it plus one quality, as a player does.
  await page.send("Page.navigate", { url: SITE + "/index.html" });
  const tabId = await until("the tab", 5000, () => sw.evaluate("chrome.tabs.query({ url: '" + SITE + "/*' }).then(tabs => tabs[0]?.id)"));
  const found = await until("media", 8000, async () => {
    const list = await sw.evaluate("(media.get(" + tabId + ") ?? []).map(e => [e.kind, e.url, Boolean(e.content), Boolean(e.variant)].join(' '))");
    return list.some((e) => /master\.m3u8\?token=t1 true false$/.test(e)) && list.some((e) => /720\/stream\.m3u8 true true$/.test(e)) && list;
  });
  check("master playlist is found with its content, its quality playlist marked", Boolean(found), JSON.stringify(found));
  const visible = await sw.evaluate("visibleMedia(" + tabId + ").map(e => e.url)");
  check("only the master playlist is listed", visible.length === 1 && visible[0].includes("master.m3u8"), JSON.stringify(visible));
  const badge = await sw.evaluate("chrome.action.getBadgeText({ tabId: " + tabId + " })");
  check("toolbar badge counts it", badge === "1", "badge " + JSON.stringify(badge));

  // 4. The quality menu: OpenDM's engine reads the playlist.
  const menu = JSON.parse(await sw.evaluate("formats(" + tabId + ").then(r => JSON.stringify(r))"));
  const labels = menu.groups.flatMap((g) => g.options.map((o) => g.title + ": " + o.label + " (" + o.detail + ")"));
  check("menu lists both qualities", labels.some((l) => l.startsWith("Video: 720p")) && labels.some((l) => l.startsWith("Video: 360p")), labels.join(", "));

  // The popup, opened in a tab for the screenshot.
  const popupTarget = await (await fetch("http://127.0.0.1:" + PORT + "/json/new?chrome-extension://" + ID + "/popup.html?tab=" + tabId, { method: "PUT" })).json();
  const popup = await Cdp.open(popupTarget.webSocketDebuggerUrl);
  await popup.send("Page.enable");
  await popup.send("Emulation.setDeviceMetricsOverride", { width: 340, height: 420, deviceScaleFactor: 2, mobile: false });
  const popupText = await until("the popup's qualities", 10000, async () => {
    const text = await popup.evaluate("document.body.innerText");
    return text.includes("720p") && text;
  });
  await popup.screenshot(ROOT + "/popup.png");
  check("popup shows the connection and the qualities", Boolean(popupText) && popupText.includes("Connected") && popupText.includes("360p"), String(popupText).replace(/\n+/g, " | "));
  await fetch("http://127.0.0.1:" + PORT + "/json/close/" + popupTarget.id);

  // 5. Picking 360p gives a 360p MP4 named after the page.
  const low = menu.groups[0].options.find((o) => o.label === "360p");
  await sw.evaluate("downloadChoice(" + tabId + ", " + JSON.stringify(low.choice) + ").then(() => 'ok')");
  const video = await until("the stream's file", 30000, async () => listFiles(APP_DL).find((f) => f === "Video/Big Buck Test Video.mp4"));
  check("HLS stream is saved as an MP4 named after the page", Boolean(video), listFiles(APP_DL).join(", "));
  if (video) check("it is the 360p quality", videoHeight(APP_DL + "/" + video) === "360", videoHeight(APP_DL + "/" + video));

  // 6. The video button opens the same menu; its file entry downloads the clip.
  await page.send("Page.bringToFront");
  const box = await page.evaluate("(() => { const r = document.getElementById('video').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()");
  await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: box.x + box.w / 2, y: box.y + box.h / 2 });
  await sleep(400);
  const shown = await page.evaluate("getComputedStyle(document.querySelector('opendm-connect')).display");
  check("video button appears on hover", shown === "block", shown);
  const [main] = await page.find("main");
  await page.click(main.x, main.y);
  const options = await until("the menu", 10000, async () => {
    const found = await page.find("option");
    return found.length && found;
  });
  await sleep(300); // The menu's opening animation.
  await page.screenshot(ROOT + "/video-menu.png");
  check("video button opens the quality menu", Boolean(options) && options.some((o) => o.text.startsWith("720p")), (options || []).map((o) => o.text).join(" | "));
  const file = (options || []).find((o) => o.text.startsWith("WEBM"));
  if (file) await page.click(file.x, file.y);
  const clip = await until("the clip", 20000, async () => listFiles(APP_DL).find((f) => f.endsWith(".webm")));
  check("the menu's file entry sends the video to OpenDM", Boolean(clip), clip);
  await sleep(1600); // The button's "Sent to OpenDM" flash ignores clicks.

  // 6b. The button also appears over a video inside a player's shadow root.
  await page.evaluate("document.getElementById('shadow-player').scrollIntoView()");
  const shadowBox = await page.evaluate("(() => { const r = document.getElementById('shadow-player').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()");
  await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: shadowBox.x + shadowBox.w / 2, y: shadowBox.y + shadowBox.h / 2 });
  await sleep(400);
  const overShadow = await page.evaluate("(() => { const host = document.querySelector('opendm-connect'); const r = host.getBoundingClientRect(); const p = document.getElementById('shadow-player').getBoundingClientRect(); return getComputedStyle(host).display === 'block' && r.top >= p.top && r.bottom <= p.bottom; })()");
  check("video button appears over a video in a shadow root", overShadow === true, String(overShadow));
  // Its menu is for its own video, not the page's other stream.
  const [shadowMain] = await page.find("main");
  await page.click(shadowMain.x, shadowMain.y);
  const ownOptions = await until("the shadow player's menu", 15000, async () => {
    const found = await page.find("option");
    return found.length && found;
  });
  const ownLabels = (ownOptions || []).map((o) => o.text);
  check("the menu is for the hovered player's own stream", ownLabels.some((t) => t.startsWith("360p")) && !ownLabels.some((t) => t.startsWith("720p")), ownLabels.join(" | "));
  await page.click(shadowMain.x, shadowMain.y);
  await sleep(300);

  // 6c. The pill's close button hides it on this page; the popup's switch turns it off everywhere.
  const pillShown = () => page.evaluate("getComputedStyle(document.querySelector('opendm-connect')).display");
  const hover = async (id) => {
    const b = await page.evaluate("(() => { const e = document.getElementById('" + id + "'); e.scrollIntoView({ block: 'center' }); const r = e.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()");
    await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: b.x + 5, y: b.y + 5 });
    await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: b.x + b.w / 2, y: b.y + b.h / 2 });
    await sleep(400);
  };
  await hover("shadow-player");
  const [closer] = await page.find("close");
  if (closer) await page.click(closer.x, closer.y);
  await sleep(300);
  check("the close button hides the video button", Boolean(closer) && (await pillShown()) === "none", closer ? await pillShown() : "no close button");
  await sleep(200);
  await hover("video");
  check("it stays hidden on the same page", (await pillShown()) === "none", await pillShown());
  await page.send("Page.reload");
  await sleep(1500);
  await sw.evaluate("chrome.storage.local.set({ videoButton: false }).then(() => 'ok')");
  await hover("video");
  const offState = await page.evaluate("(() => { const h = document.querySelector('opendm-connect'); return h ? getComputedStyle(h).display : 'none'; })()");
  check("the popup's switch turns the video button off", offState === "none", offState);
  await sw.evaluate("chrome.storage.local.set({ videoButton: true }).then(() => 'ok')");
  await sleep(200);
  await hover("video");
  check("and back on", (await pillShown()) === "block", await pillShown());

  // 7. "Download all links" hands over every link on the page.
  const links = await sw.evaluate(
    "chrome.scripting.executeScript({ target: { tabId: " + tabId + " }, func: collectLinks, args: [false] }).then(([r]) => r.result)",
  );
  check("all links are collected", links.length === 2, JSON.stringify(links));
  const handed = await sw.evaluate("Promise.all(" + JSON.stringify(links) + ".map(u => requestItem(u, { referrer: '" + SITE + "/' }))).then(items => handOver('links', items))");
  check("OpenDM accepts the links", handed === true);

  // 8. A proxy another extension (a VPN) put in place goes with the item;
  // the extension itself plays that extension here.
  await sw.evaluate("chrome.proxy.settings.set({ value: { mode: 'fixed_servers', rules: { singleProxy: { scheme: 'socks5', host: '10.9.8.7', port: 1080 }, bypassList: ['*.internal.example'] } }, scope: 'regular' }).then(() => 'ok')");
  const proxied = await sw.evaluate("requestItem('https://files.example.com/a.zip').then(i => i.proxy)");
  check("the browser's proxy goes with a download", proxied === "socks5://10.9.8.7:1080", proxied);
  const bypassed = await sw.evaluate("requestItem('https://cdn.internal.example/a.zip').then(i => i.proxy ?? 'none')");
  check("an address the proxy skips goes without it", bypassed === "none", bypassed);
  await sw.evaluate("chrome.proxy.settings.clear({ scope: 'regular' }).then(() => 'ok')");
  const cleared = await sw.evaluate("requestItem('https://files.example.com/a.zip').then(i => i.proxy ?? 'none')");
  check("without one, no proxy is sent", cleared === "none", cleared);
} catch (error) {
  check("test ran to the end", false, error.stack);
} finally {
  chrome.kill();
}
const failed = results.filter((r) => !r.ok).length;
console.log("\n" + (results.length - failed) + " passed, " + failed + " failed");
process.exit(failed ? 1 : 0);
