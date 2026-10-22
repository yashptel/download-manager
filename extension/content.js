// OpenDM Connect page script: Alt-click bypass, the finds from page.js, and
// the video button with its quality menu.
"use strict";

(() => {
  if (globalThis.__opendmConnect) return;
  globalThis.__opendmConnect = true;
  const api = globalThis.browser ?? globalThis.chrome;
  const send = (message) => api.runtime.sendMessage(message).catch((error) => ({ ok: false, error: error.message }));

  // Alt-click keeps the download in the browser. Reported on mouse-down, so
  // the background knows before the browser creates the download.
  addEventListener(
    "mousedown",
    (event) => {
      if (!event.altKey) return;
      const link = event.target instanceof Element ? event.target.closest("a[href]") : null;
      send({ type: "bypass", url: link?.href ?? "" });
    },
    true,
  );

  // ─── Streams the page's scripts load (page.js) ──────────────────────────

  let pending = [];
  function found(item) {
    pending.push(item);
    if (pending.length === 1) setTimeout(() => send({ type: "page-media", items: pending.splice(0) }), 200);
  }
  document.addEventListener("opendm-media", (event) => {
    try {
      const item = JSON.parse(event.detail);
      if (typeof item?.url === "string") found(item);
    } catch {}
  });

  // Playlist URLs written into the page itself: player configs in inline
  // scripts and the video tags of the social-preview metadata.
  const URL_IN_TEXT = /https?:\/\/[^\s"'<>\\]+?\.(?:m3u8|mpd)(?:\?[^\s"'<>\\]*)?/gi;
  function scanDocument() {
    let text = "";
    for (const script of document.querySelectorAll("script:not([src])")) {
      text += script.textContent + "\n";
      if (text.length > 4 * 1024 * 1024) break;
    }
    for (const meta of document.querySelectorAll('meta[property^="og:video"], meta[name^="twitter:player:stream"]')) {
      text += " " + (meta.content || "");
    }
    const plain = text.replace(/\\\//g, "/").replace(/\\u0026/gi, "&");
    const seen = new Set();
    for (const [url] of plain.matchAll(URL_IN_TEXT)) {
      if (seen.size >= 20) break;
      if (seen.has(url)) continue;
      seen.add(url);
      found({ url, kind: /\.mpd/i.test(url) ? "dash" : "hls" });
    }
  }
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", scanDocument, { once: true });
  else scanDocument();

  // ─── Video button and quality menu ──────────────────────────────────────

  // As in background.js: empty here, filled in by the Chrome Web Store package.
  const MEDIA_BLOCKED_HOSTS = [];
  const blockedHere = MEDIA_BLOCKED_HOSTS.some((blocked) => location.hostname === blocked || location.hostname.endsWith("." + blocked));
  let host = null;
  let button = null;
  let closeButton = null;
  let menu = null;
  let current = null;
  let hideTimer = 0;
  let lastMove = 0;
  let busy = false;
  let asked = ""; // The page the button's open question is about.
  let ticket = 0;
  let prefetched = "";
  let videoButtonOn = true;
  let closedOn = "";
  const pageUrl = () => location.href.split("#")[0];

  api.storage.local.get("videoButton").then((stored) => (videoButtonOn = stored.videoButton !== false));
  api.storage.onChanged.addListener((changes, area) => {
    if (area !== "local" || !("videoButton" in changes)) return;
    videoButtonOn = changes.videoButton.newValue !== false;
    if (!videoButtonOn) dismiss();
  });

  /** The video large enough to matter under the pointer. The event's path
   * reaches into open shadow roots, where players built as web components
   * (Reddit's) keep their video and the document's own list does not. */
  function videoAt(event) {
    const under = (video) => {
      const box = video.getBoundingClientRect();
      return box.width >= 200 && box.height >= 120 && event.clientX >= box.left && event.clientX <= box.right && event.clientY >= box.top && event.clientY <= box.bottom;
    };
    for (const node of event.composedPath()) {
      if (node instanceof HTMLVideoElement && under(node)) return node;
      // The pointer is often over the player's controls, not the video itself.
      const inside = node instanceof Element && node.shadowRoot ? [...node.shadowRoot.querySelectorAll("video")].find(under) : null;
      if (inside) return inside;
    }
    return [...document.getElementsByTagName("video")].find(under) ?? null;
  }

  const STYLE = `
    .row {
      position: relative; display: flex;
      font: 500 12.5px/1.3 -apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
      -webkit-font-smoothing: antialiased;
    }
    .main, .close {
      height: 30px; border: 0; color: #fff; background: rgba(28, 28, 30, 0.62);
      box-shadow: 0 0 0 0.5px rgba(255, 255, 255, 0.28) inset, 0 4px 14px rgba(0, 0, 0, 0.28);
      -webkit-backdrop-filter: blur(18px) saturate(1.6); backdrop-filter: blur(18px) saturate(1.6);
      cursor: pointer; opacity: 0; transform: translateY(-4px);
      transition: opacity 0.16s ease-out, transform 0.16s ease-out, background 0.12s;
    }
    .main {
      display: flex; align-items: center; gap: 6px; margin-left: auto;
      padding: 0 12px 0 10px; border-radius: 15px; font: inherit;
    }
    .close {
      width: 30px; margin-left: 4px; padding: 0; border-radius: 50%; font: inherit;
    }
    .main.shown, .close.shown { opacity: 1; transform: none; }
    .main:hover, .main.open { background: rgba(10, 132, 255, 0.85); }
    .close:hover { background: rgba(255, 255, 255, 0.22); }
    .main:active, .close:active { transform: scale(0.97); }
    svg { width: 14px; height: 14px; flex: none; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
    .menu {
      position: absolute; top: 36px; right: 34px; width: 300px; max-height: min(62vh, 440px); overflow-y: auto;
      padding: 5px; border-radius: 12px; color: #fff; background: rgba(28, 28, 30, 0.88);
      box-shadow: 0 0 0 0.5px rgba(255, 255, 255, 0.22) inset, 0 12px 34px rgba(0, 0, 0, 0.4);
      -webkit-backdrop-filter: blur(24px) saturate(1.8); backdrop-filter: blur(24px) saturate(1.8);
      transform-origin: top right; animation: open 0.14s ease-out;
    }
    @keyframes open { from { opacity: 0; transform: scale(0.97) translateY(-3px); } }
    .group { padding: 7px 9px 3px; font-size: 11px; font-weight: 600; color: rgba(255, 255, 255, 0.55); }
    /* One grid for the whole menu, so the details of every option start at
       the same place whatever the label's length. */
    .options { display: grid; grid-template-columns: max-content minmax(0, 1fr); column-gap: 12px; }
    .options > .group, .options > .note { grid-column: 1 / -1; }
    .option {
      grid-column: 1 / -1; display: grid; grid-template-columns: subgrid; align-items: baseline;
      padding: 6px 9px; border: 0; border-radius: 7px;
      font: inherit; text-align: left; color: inherit; background: transparent; cursor: pointer;
    }
    .option:hover, .option:focus-visible { background: rgba(10, 132, 255, 0.9); outline: none; }
    .label { font-weight: 600; white-space: nowrap; font-variant-numeric: tabular-nums; }
    .detail { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: rgba(255, 255, 255, 0.68); font-weight: 400; font-variant-numeric: tabular-nums; }
    .option:hover .detail, .option:focus-visible .detail { color: rgba(255, 255, 255, 0.9); }
    .note { padding: 8px 9px; color: rgba(255, 255, 255, 0.75); font-weight: 400; }
    @media (prefers-reduced-motion: reduce) { .main, .close, .menu { transition: none; animation: none; } }
  `;

  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text) node.textContent = text;
    return node;
  }

  function build() {
    host = document.createElement("opendm-connect");
    host.style.cssText = "all: initial; position: fixed; z-index: 2147483647; display: none;";
    const shadow = host.attachShadow({ mode: "closed" });
    // Built with DOM calls: pages with Trusted Types reject innerHTML.
    const style = element("style");
    style.textContent = STYLE;
    button = element("button", "main");
    button.type = "button";
    button.title = "Download with OpenDM";
    button.setAttribute("aria-haspopup", "menu");
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 16 16");
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", "M8 2v8M4.5 6.8 8 10.3l3.5-3.5M3 13.5h10");
    svg.append(path);
    button.append(svg, element("span", "", "Download video"));
    closeButton = element("button", "close");
    closeButton.type = "button";
    closeButton.title = "Hide on this page";
    closeButton.setAttribute("aria-label", "Hide on this page");
    const closeSvg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    closeSvg.setAttribute("viewBox", "0 0 16 16");
    const closePath = document.createElementNS("http://www.w3.org/2000/svg", "path");
    closePath.setAttribute("d", "M5 5l6 6M11 5l-6 6");
    closeSvg.append(closePath);
    closeButton.append(closeSvg);
    const row = element("div", "row");
    row.append(button, closeButton);
    shadow.append(style, row);
    button.addEventListener("click", onClick);
    closeButton.addEventListener("click", onClose);
    // Keep the page's player from treating clicks here as its own.
    for (const type of ["click", "mousedown", "mouseup", "pointerdown", "pointerup", "dblclick", "wheel", "keydown"]) {
      row.addEventListener(type, (event) => event.stopPropagation());
    }
    row.addEventListener("keydown", onKey);
    host.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    host.addEventListener("mouseleave", () => !menu && !busy && (hideTimer = setTimeout(hide, 1200)));
    (document.body ?? document.documentElement).appendChild(host);
  }

  function place() {
    const box = current.getBoundingClientRect();
    host.style.top = Math.max(box.top + 10, 6) + "px";
    host.style.right = Math.max(innerWidth - box.right + 10, 6) + "px";
  }

  function show(video) {
    if (!host || !host.isConnected) build();
    if (menu && video !== current) closeMenu();
    const link = linkFor(video);
    if (prefetched !== location.href + link) {
      prefetched = location.href + link;
      send({ type: "prefetch", link });
    }
    current = video;
    place();
    host.style.display = "block";
    requestAnimationFrame(() => {
      button.classList.add("shown");
      closeButton.classList.add("shown");
    });
    clearTimeout(hideTimer);
    hideTimer = setTimeout(hide, 2500);
  }

  function hide() {
    if (!host || busy || menu) return;
    button.classList.remove("shown");
    closeButton.classList.remove("shown");
    hideTimer = setTimeout(() => (host.style.display = "none"), 160);
  }

  function label(text) {
    button.querySelector("span").textContent = text;
  }

  /** Hides the button at once, dropping any question it has open. */
  function dismiss() {
    if (!host) return;
    ticket++;
    busy = false;
    label("Download video");
    if (menu) closeMenu();
    clearTimeout(hideTimer);
    button.classList.remove("shown");
    closeButton.classList.remove("shown");
    host.style.display = "none";
  }

  function onClose(event) {
    event.preventDefault();
    closedOn = pageUrl();
    dismiss();
  }

  const PLAYLIST = /(^|\/)[^/?#]*\.(m3u8|mpd)(?:[?#]|$)/i;

  /** The playlist a player names in its own markup, as a <source> or an
   * attribute of the video, its shadow host or a near ancestor (Reddit's
   * player puts its HLS playlist in src). A player feeding the video through
   * MediaSource leaves it with only a blob: URL, so this is what ties the
   * video on screen to one of the playlists the page loaded. */
  function manifestFor(video) {
    const named = (element) => {
      for (const { value } of element.attributes) {
        if (PLAYLIST.test(value) && URL.canParse(value, location.href)) {
          const url = new URL(value, location.href);
          if (/^https?:$/.test(url.protocol)) return url.href;
        }
      }
      return "";
    };
    let node = video;
    for (let depth = 0; node && depth < 8; depth++) {
      const found = named(node) || (node === video ? [...video.querySelectorAll("source")].map(named).find(Boolean) : "");
      if (found) return found;
      node = node.parentElement ?? node.getRootNode()?.host ?? null;
    }
    return "";
  }

  /** The page a video previews: feeds play previews of other pages over
   * their links (YouTube's home page), and those pages are what to look up. */
  function linkFor(video) {
    const box = video.getBoundingClientRect();
    const under = document.elementsFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    const here = location.href.split("#")[0];
    for (const link of [video.closest("a[href]"), ...under.map((node) => node.closest("a[href]"))]) {
      if (link && /^https?:/i.test(link.href) && link.host === location.host && link.href.split("#")[0] !== here) return link.href;
    }
    return "";
  }

  /** Shows a result on the button for a moment. */
  function flash(text, ms) {
    busy = true;
    label(text);
    setTimeout(() => {
      busy = false;
      label("Download video");
      hide();
    }, ms);
  }

  async function onClick(event) {
    event.preventDefault();
    if (menu) return closeMenu();
    if (busy || !current) return;
    busy = true;
    asked = location.href;
    const mine = ++ticket;
    label("Finding qualities…");
    const question = send({ type: "formats", src: current.currentSrc || current.src || "", manifest: manifestFor(current), link: linkFor(current) });
    const late = new Promise((resolve) => setTimeout(() => resolve({ ok: false, error: "OpenDM is taking long. Try again." }), 30_000));
    const answer = await Promise.race([question, late]);
    if (mine !== ticket) return;
    busy = false;
    label("Download video");
    // The page moved on meanwhile (the next short); this answer is for a video that is gone.
    if (asked !== location.href) return;
    if (!answer?.ok) return flash(answer?.error || "OpenDM is not available", 3500);
    openMenu(answer.groups ?? [], answer.note);
  }

  function openMenu(groups, note) {
    menu = element("div", "menu options");
    menu.setAttribute("role", "menu");
    if (!groups.some((group) => group.options.length)) {
      menu.append(element("div", "note", note || "No video found yet. Play it for a moment, then try again."));
    }
    for (const group of groups) {
      if (group.title) menu.append(element("div", "group", group.title));
      for (const option of group.options) {
        const item = element("button", "option");
        item.type = "button";
        item.setAttribute("role", "menuitem");
        item.title = option.detail;
        item.append(element("span", "label", option.label), element("span", "detail", option.detail));
        item.addEventListener("click", () => choose(option.choice));
        menu.append(item);
      }
    }
    button.classList.add("open");
    button.setAttribute("aria-expanded", "true");
    button.after(menu);
    clearTimeout(hideTimer);
    addEventListener("mousedown", onOutside, true);
  }

  function closeMenu() {
    menu?.remove();
    menu = null;
    button?.classList.remove("open");
    button?.setAttribute("aria-expanded", "false");
    removeEventListener("mousedown", onOutside, true);
    hideTimer = setTimeout(hide, 1200);
  }

  function onOutside(event) {
    if (!event.composedPath().includes(host)) closeMenu();
  }

  function onKey(event) {
    if (!menu) return;
    const items = [...menu.querySelectorAll(".option")];
    const at = items.indexOf(menu.getRootNode().activeElement);
    if (event.key === "Escape") {
      closeMenu();
      button.focus();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      items[(at + step + items.length) % items.length]?.focus();
    }
  }

  async function choose(choice) {
    closeMenu();
    const answer = await send({ type: "download-choice", choice });
    flash(answer?.ok ? "Sent to OpenDM" : answer?.error || "OpenDM is not available", answer?.ok ? 1400 : 3500);
  }

  addEventListener(
    "mousemove",
    (event) => {
      const now = performance.now();
      if (blockedHere || now - lastMove < 120) return;
      lastMove = now;
      if (busy && asked !== location.href) {
        ticket++;
        busy = false;
        label("Download video");
        if (menu) closeMenu();
      }
      if (!videoButtonOn || closedOn === pageUrl()) return;
      const video = videoAt(event);
      if (video) show(video);
    },
    { capture: true, passive: true },
  );
  addEventListener("scroll", () => host && current && host.style.display !== "none" && place(), { capture: true, passive: true });
})();
