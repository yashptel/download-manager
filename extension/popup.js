"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const $ = (id) => document.getElementById(id);
let tabId = -1;

const STATUS = {
  connected: (state) => "Connected" + (state.version ? " · " + state.version : ""),
  "not-running": () => "OpenDM is not running",
  error: (state) => (/not found|not registered/i.test(state.error) ? "OpenDM is not installed" : state.error || "Cannot reach OpenDM"),
  idle: () => "Connecting…",
};

function formatSize(bytes) {
  if (!bytes) return "";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return value.toFixed(value < 10 && unit > 0 ? 1 : 0) + " " + units[unit];
}

function streamName(url) {
  try {
    const parts = new URL(url).pathname.split("/").filter(Boolean);
    return decodeURIComponent(parts.pop() || new URL(url).hostname);
  } catch {
    return url;
  }
}

function downloadIcon() {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", "M8 2.5v8M4.5 7.3 8 10.8l3.5-3.5M3 13.5h10");
  svg.append(path);
  return svg;
}

function showError(message) {
  $("error").textContent = message || "";
  $("error").hidden = !message;
}

function render(state) {
  const status = STATUS[state.state] ? state.state : "idle";
  $("status").className = status;
  $("status-text").textContent = STATUS[status](state);
  $("start").hidden = status !== "not-running";
  $("access").hidden = state.hostAccess !== false;
  $("capture").checked = state.captureOn;
  $("capture").disabled = !state.appCapture;
  $("capture-note").textContent = state.appCapture
    ? "Alt-click a link to download it in the browser instead."
    : "Turned off in OpenDM's settings.";

  const list = $("media");
  list.replaceChildren(
    ...state.media.map((entry) => {
      const item = document.createElement("li");
      const kind = document.createElement("span");
      kind.className = "kind";
      kind.textContent = entry.kind;
      const name = document.createElement("div");
      name.className = "name";
      const title = document.createElement("div");
      title.textContent = streamName(entry.url);
      title.title = entry.url;
      const detail = document.createElement("small");
      detail.textContent = entry.kind === "HLS" || entry.kind === "DASH" ? "Stream, saved as MP4" : formatSize(entry.size);
      name.append(title, detail);
      const button = document.createElement("button");
      button.title = "Download with OpenDM";
      button.append(downloadIcon());
      button.addEventListener("click", async () => {
        button.disabled = true;
        const answer = await api.runtime.sendMessage({ type: "download-media", tabId, url: entry.url });
        showError(answer?.ok ? "" : answer?.error);
        if (answer?.ok) window.close();
        button.disabled = false;
      });
      item.append(kind, name, button);
      return item;
    }),
  );
  $("empty").hidden = state.media.length > 0;
}

async function refresh() {
  const state = await api.runtime.sendMessage({ type: "state", tabId });
  if (state?.ok) render(state);
}

$("start").addEventListener("click", async () => {
  $("start").disabled = true;
  $("status-text").textContent = "Starting OpenDM…";
  const answer = await api.runtime.sendMessage({ type: "start" });
  showError(answer?.ok ? "" : answer?.error);
  $("start").disabled = false;
  refresh();
});

$("grant").addEventListener("click", async () => {
  // Must run in the click itself to count as a user gesture.
  await api.permissions.request({ origins: ["<all_urls>"] });
  refresh();
});

$("capture").addEventListener("change", (event) => {
  api.runtime.sendMessage({ type: "set-capture", on: event.target.checked });
});

// ?tab= opens the popup for another tab, for testing it in a tab of its own.
const forcedTab = Number(new URLSearchParams(location.search).get("tab"));
api.tabs.query({ active: true, currentWindow: true }).then(([tab]) => {
  tabId = forcedTab || (tab?.id ?? -1);
  refresh();
});
