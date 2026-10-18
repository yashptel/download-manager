// OpenDM Connect page script: Alt-click bypass and the video button.
"use strict";

(() => {
  if (globalThis.__opendmConnect) return;
  globalThis.__opendmConnect = true;
  const api = globalThis.browser ?? globalThis.chrome;
  const send = (message) => api.runtime.sendMessage(message).catch(() => ({ ok: false }));

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

  // ─── Video button ───────────────────────────────────────────────────────

  const videos = document.getElementsByTagName("video"); // Live, so cheap to re-read.
  let host = null;
  let button = null;
  let current = null;
  let hideTimer = 0;
  let lastMove = 0;
  let busy = false;

  function videoAt(x, y) {
    for (const video of videos) {
      const box = video.getBoundingClientRect();
      if (box.width >= 200 && box.height >= 120 && x >= box.left && x <= box.right && y >= box.top && y <= box.bottom) {
        return video;
      }
    }
    return null;
  }

  const STYLE = `
    button {
      display: flex; align-items: center; gap: 6px;
      height: 30px; padding: 0 12px 0 10px; border: 0; border-radius: 15px;
      font: 500 12.5px/1 -apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
      color: #fff; background: rgba(28, 28, 30, 0.62);
      box-shadow: 0 0 0 0.5px rgba(255, 255, 255, 0.28) inset, 0 4px 14px rgba(0, 0, 0, 0.28);
      -webkit-backdrop-filter: blur(18px) saturate(1.6); backdrop-filter: blur(18px) saturate(1.6);
      cursor: pointer; opacity: 0; transform: translateY(-4px);
      transition: opacity 0.16s ease-out, transform 0.16s ease-out, background 0.12s;
    }
    button.shown { opacity: 1; transform: none; }
    button:hover { background: rgba(10, 132, 255, 0.85); }
    button:active { transform: scale(0.97); }
    svg { width: 14px; height: 14px; flex: none; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
    @media (prefers-reduced-motion: reduce) { button { transition: none; } }
  `;

  function build() {
    host = document.createElement("opendm-connect");
    host.style.cssText = "all: initial; position: fixed; z-index: 2147483647; display: none;";
    const shadow = host.attachShadow({ mode: "closed" });
    // Built with DOM calls: pages with Trusted Types reject innerHTML.
    const style = document.createElement("style");
    style.textContent = STYLE;
    button = document.createElement("button");
    button.type = "button";
    button.title = "Download with OpenDM";
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 16 16");
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", "M8 2v8M4.5 6.8 8 10.3l3.5-3.5M3 13.5h10");
    svg.append(path);
    const text = document.createElement("span");
    text.textContent = "Download video";
    button.append(svg, text);
    shadow.append(style, button);
    button.addEventListener("click", onClick);
    // Keep the page's player from treating the click as its own.
    for (const type of ["mousedown", "mouseup", "pointerdown", "pointerup", "dblclick"]) {
      button.addEventListener(type, (event) => event.stopPropagation());
    }
    button.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    (document.body ?? document.documentElement).appendChild(host);
  }

  function place() {
    const box = current.getBoundingClientRect();
    host.style.top = Math.max(box.top + 10, 6) + "px";
    host.style.right = Math.max(innerWidth - box.right + 10, 6) + "px";
  }

  function show(video) {
    if (!host || !host.isConnected) build();
    current = video;
    place();
    host.style.display = "block";
    requestAnimationFrame(() => button.classList.add("shown"));
    clearTimeout(hideTimer);
    hideTimer = setTimeout(hide, 2500);
  }

  function hide() {
    if (!host || busy) return;
    button.classList.remove("shown");
    hideTimer = setTimeout(() => (host.style.display = "none"), 160);
  }

  function label(text) {
    button.querySelector("span").textContent = text;
  }

  async function onClick(event) {
    event.preventDefault();
    event.stopPropagation();
    if (busy || !current) return;
    busy = true;
    label("Sending…");
    const answer = await send({ type: "video", src: current.currentSrc || current.src || "" });
    label(answer?.ok ? "Sent to OpenDM" : answer?.error || "OpenDM is not available");
    button.title = answer?.ok ? "Download with OpenDM" : answer?.error || "";
    setTimeout(() => {
      busy = false;
      label("Download video");
      hide();
    }, answer?.ok ? 1400 : 3500);
  }

  addEventListener(
    "mousemove",
    (event) => {
      const now = performance.now();
      if (now - lastMove < 120 || videos.length === 0) return;
      lastMove = now;
      const video = videoAt(event.clientX, event.clientY);
      if (video) show(video);
    },
    { capture: true, passive: true },
  );
  addEventListener("scroll", () => host && current && host.style.display !== "none" && place(), { capture: true, passive: true });
})();

