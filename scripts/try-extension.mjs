#!/usr/bin/env node
// Opens Google Chrome or Firefox with a separate development profile and the
// OpenDM Connect extension from extension/ already loaded, for trying it by
// hand. Your everyday browser profiles are not touched.
//
//   node scripts/try-extension.mjs chrome [url...]
//   node scripts/try-extension.mjs firefox [url...]
//
// Start build/OpenDM.app first (or let the extension start it). Chrome 137+
// ignores --load-extension, so the extension is loaded over a DevTools pipe;
// Chrome quits when that pipe closes, so this script keeps running until you
// close that Chrome window. Firefox gets the extension as a temporary add-on,
// which lasts until Firefox quits. Profiles live in ~/.opendm-dev.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import { fileURLToPath } from "node:url";

const REPO = fileURLToPath(new URL("..", import.meta.url)).replace(/\/$/, "");
const EXTENSION = REPO + "/extension";
const HOST = REPO + "/build/OpenDM.app/Contents/MacOS/opendm-host";
const DEV = process.env.HOME + "/.opendm-dev";
const [browser, ...urls] = process.argv.slice(2);

if (!fs.existsSync(HOST)) {
  console.error("Build the app first: scripts/build-macos.sh");
  process.exit(1);
}

if (browser === "chrome") await chrome();
else if (browser === "firefox") await firefox();
else {
  console.error("usage: node scripts/try-extension.mjs chrome|firefox [url...]");
  process.exit(1);
}

async function chrome() {
  const profile = DEV + "/chrome-profile";
  // Chrome keeps running the service worker it cached for an unpacked
  // extension even after its files change; without the cache it reads them.
  fs.rmSync(profile + "/Default/Service Worker", { recursive: true, force: true });
  // With its own profile, Chrome reads native messaging hosts from the profile.
  fs.mkdirSync(profile + "/NativeMessagingHosts", { recursive: true });
  fs.writeFileSync(
    profile + "/NativeMessagingHosts/org.opendm.host.json",
    // Same as manifest() in host/src/lib.rs.
    JSON.stringify({
      name: "org.opendm.host", description: "OpenDM", path: HOST, type: "stdio",
      allowed_origins: ["chrome-extension://nkhhoiejpomapjkeomdbflaicnnjclfj/"],
    }, null, 2),
  );
  const child = spawn(
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ["--user-data-dir=" + profile, "--no-first-run", "--no-default-browser-check",
      "--remote-debugging-pipe", "--enable-unsafe-extension-debugging", ...urls],
    { detached: true, stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
  );
  // DevTools pipe: NUL-terminated JSON messages, answers matched by id.
  const waiting = new Map();
  let buffer = "";
  child.stdio[4].on("data", (chunk) => {
    buffer += chunk;
    for (let end; (end = buffer.indexOf("\0")) >= 0; buffer = buffer.slice(end + 1)) {
      const answer = JSON.parse(buffer.slice(0, end));
      waiting.get(answer.id)?.(answer);
    }
  });
  let next = 1;
  const call = (method, params) =>
    new Promise((resolve) => {
      const id = next++;
      waiting.set(id, resolve);
      child.stdio[3].write(JSON.stringify({ id, method, params }) + "\0");
    });
  const loaded = await call("Extensions.loadUnpacked", { path: EXTENSION });
  if (loaded.error) throw new Error(loaded.error.message);
  const id = loaded.result.id;
  console.log("Chrome is open with OpenDM Connect (" + id + "). Close that Chrome to end this script.");
  child.on("exit", () => process.exit(0));
}

async function firefox() {
  const profile = DEV + "/firefox-profile";
  const port = 6125;
  fs.mkdirSync(profile, { recursive: true });
  const prefs = {
    "devtools.debugger.remote-enabled": true, "devtools.chrome.enabled": true, "devtools.debugger.prompt-connection": false,
    "browser.shell.checkDefaultBrowser": false, "browser.aboutwelcome.enabled": false,
    "datareporting.policy.dataSubmissionPolicyBypassNotification": true,
  };
  fs.writeFileSync(profile + "/user.js", Object.entries(prefs).map(([k, v]) => "user_pref(" + JSON.stringify(k) + ", " + JSON.stringify(v) + ");").join("\n"));
  // Through LaunchServices, so Firefox does not inherit a sandboxed shell.
  execFileSync("/usr/bin/open", ["-n", "-a", "Firefox", "--args", "-no-remote", "-profile", profile, "--start-debugger-server", String(port), ...urls]);
  const id = await installTemporaryAddon(port);
  console.log("Firefox is open with OpenDM Connect (" + id + "). It stays installed until Firefox quits.");
}

/** Firefox's remote debugging protocol: messages are "length:json". */
async function installTemporaryAddon(port) {
  let socket;
  for (let i = 0; i < 60 && !socket; i++) {
    await new Promise((r) => setTimeout(r, 500));
    socket = await new Promise((resolve) => {
      const s = net.connect(port, "127.0.0.1", () => resolve(s));
      s.on("error", () => resolve(null));
    });
  }
  if (!socket) throw new Error("Firefox did not open its debugger port");
  let buffer = Buffer.alloc(0);
  const inbox = [];
  let wake = () => {};
  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      const colon = buffer.indexOf(":");
      const length = Number(buffer.subarray(0, colon).toString());
      if (colon < 0 || buffer.length < colon + 1 + length) break;
      inbox.push(JSON.parse(buffer.subarray(colon + 1, colon + 1 + length).toString()));
      buffer = buffer.subarray(colon + 1 + length);
      wake();
    }
  });
  const next = async (from) => {
    for (;;) {
      const index = inbox.findIndex((m) => m.from === from);
      if (index >= 0) return inbox.splice(index, 1)[0];
      await new Promise((r) => (wake = r));
    }
  };
  const ask = (message) => {
    const json = JSON.stringify(message);
    socket.write(Buffer.byteLength(json) + ":" + json);
    return next(message.to);
  };
  await next("root");
  const root = await ask({ to: "root", type: "getRoot" });
  const installed = await ask({ to: root.addonsActor, type: "installTemporaryAddon", addonPath: EXTENSION });
  socket.destroy();
  if (!installed.addon) throw new Error(JSON.stringify(installed));
  return installed.addon.id;
}
