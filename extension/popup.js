"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const $ = (id) => document.getElementById(id);
let tabId = -1;

api.storage.local.get("videoButton").then((stored) => {
  $("video-button").checked = stored.videoButton !== false;
});

$("video-button").addEventListener("change", (event) => {
  api.storage.local.set({ videoButton: event.target.checked });
});

const STATUS = {
  connected: (state) => "Connected · OpenDM " + state.version + " · Connect " + api.runtime.getManifest().version,
  "not-running": () => "OpenDM is not running",
  error: (state) => (/not found|not registered/i.test(state.error) ? "OpenDM is not installed" : state.error || "Cannot reach OpenDM"),
  idle: () => "Connecting…",
};

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
  const something = state.mediaCount > 0 || state.hasVideo;
  $("empty").hidden = something;
  if (something && !formatsAsked) {
    formatsAsked = true;
    loadFormats();
  }
}

let formatsAsked = false;

/** The same choices as the video button's menu: qualities, audio, files. */
async function loadFormats() {
  $("loading").hidden = false;
  const answer = await api.runtime.sendMessage({ type: "formats", tabId });
  $("loading").hidden = true;
  if (!answer?.ok) return showError(answer?.error);
  const list = $("media");
  list.replaceChildren();
  for (const group of answer.groups) {
    const heading = document.createElement("li");
    heading.className = "group";
    heading.textContent = group.title;
    list.append(heading);
    for (const option of group.options) list.append(optionRow(option));
  }
  $("empty").hidden = answer.groups.length > 0;
  if (answer.note) $("empty").textContent = answer.note;
}

function optionRow(option) {
  const item = document.createElement("li");
  const kind = document.createElement("span");
  kind.className = "kind";
  kind.textContent = option.label;
  const name = document.createElement("div");
  name.className = "name";
  name.textContent = option.detail;
  name.title = option.detail;
  const button = document.createElement("button");
  button.title = "Download with OpenDM";
  button.append(downloadIcon());
  button.addEventListener("click", async () => {
    button.disabled = true;
    const answer = await api.runtime.sendMessage({ type: "download-choice", tabId, choice: option.choice });
    showError(answer?.ok ? "" : answer?.error);
    if (answer?.ok) window.close();
    button.disabled = false;
  });
  item.append(kind, name, button);
  return item;
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
