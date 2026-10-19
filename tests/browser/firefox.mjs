// Firefox check. On macOS Firefox is launched through LaunchServices (helpers
// started from a sandboxed shell cannot sandbox themselves); on Windows it is
// started directly. The add-on is installed as a temporary add-on over
// Firefox's remote debugging protocol.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const WINDOWS = process.platform === "win32";
const FIREFOX = process.env.FIREFOX ?? "C:/Program Files/Mozilla Firefox/firefox.exe";
const REPO = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
const ROOT = process.env.OPENDM_E2E_DIR ?? path.join(os.tmpdir(), "opendm-e2e"), RUN = ROOT + "/ff-" + Date.now();
const DATA = RUN + "/data", APP_DL = RUN + "/dl", BROWSER_DL = RUN + "/browser-dl", PROFILE = RUN + "/profile";
const PORT = 6125;
for (const dir of [DATA, APP_DL, BROWSER_DL, PROFILE]) fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(DATA + "/settings.json", JSON.stringify({ download_dir: APP_DL, confirm_downloads: false, show_progress_window: false }));
fs.rmSync(ROOT + "/site/go.json", { force: true });
const prefs = {
  "devtools.debugger.remote-enabled": true, "devtools.chrome.enabled": true, "devtools.debugger.prompt-connection": false,
  "browser.download.folderList": 2, "browser.download.dir": path.resolve(BROWSER_DL), "browser.download.useDownloadDir": true,
  "browser.download.alwaysOpenPanel": false, "browser.shell.checkDefaultBrowser": false,
  "browser.aboutwelcome.enabled": false, "datareporting.policy.dataSubmissionPolicyBypassNotification": true,
  "browser.startup.homepage_override.mstone": "ignore",
};
fs.writeFileSync(PROFILE + "/user.js", Object.entries(prefs).map(([k, v]) => "user_pref(" + JSON.stringify(k) + ", " + JSON.stringify(v) + ");").join("\n"));
const started = Date.now() / 1000;
const firefoxArgs = ["-no-remote", "-profile", path.resolve(PROFILE), "--start-debugger-server", String(PORT), "http://127.0.0.1:8770/wait.html"];
let firefox;
if (WINDOWS) {
  // Firefox on Windows finds native hosts in the registry only; the app
  // writes the same key at every launch.
  const manifest = path.resolve(RUN, "org.opendm.host.firefox.json");
  fs.writeFileSync(manifest, JSON.stringify({
    name: "org.opendm.host", description: "OpenDM", type: "stdio",
    path: path.join(REPO, "build/windows/OpenDM-x64/opendm-host.exe"),
    allowed_extensions: ["connect@opendm.org"],
  }));
  execFileSync("reg", ["add", "HKCU\\Software\\Mozilla\\NativeMessagingHosts\\org.opendm.host", "/ve", "/d", manifest, "/f"], { stdio: "ignore" });
  // Without -wait-for-browser the launcher exits at once and the browser it
  // starts outlives taskkill, keeping the debugger port for the next run.
  firefox = spawn(FIREFOX, ["-wait-for-browser", ...firefoxArgs], { env: { ...process.env, OPENDM_DATA_DIR: DATA }, stdio: "ignore" });
} else {
  execFileSync("/usr/bin/open", ["-n", "-a", "/Applications/Firefox.app", "--env", "OPENDM_DATA_DIR=" + DATA, "--args", ...firefoxArgs]);
}

/** Minimal remote debugging protocol client: messages are "length:json". */
async function rdp() {
  let socket;
  for (let i = 0; i < 60 && !socket; i++) {
    await new Promise((r) => setTimeout(r, 500));
    socket = await new Promise((resolve) => {
      const s = net.connect(PORT, "127.0.0.1", () => resolve(s));
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
      if (colon < 0) break;
      const length = Number(buffer.subarray(0, colon).toString());
      if (buffer.length < colon + 1 + length) break;
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
  const ask = async (message) => {
    const json = JSON.stringify(message);
    socket.write(Buffer.byteLength(json) + ":" + json);
    return next(message.to);
  };
  await next("root");
  return { ask, close: () => socket.destroy() };
}

const results = [];
const check = (name, ok, detail) => { results.push(ok); console.log((ok ? "PASS " : "FAIL ") + name + (detail ? " — " + detail : "")); };
const files = (dir) => fs.readdirSync(dir, { recursive: true }).map((f) => String(f).replaceAll("\\", "/")).filter((f) => f.includes("."));
// Newer Firefox removes a temporary add-on when the connection that installed it closes.
let client;
try {
  client = await rdp();
  const root = await client.ask({ to: "root", type: "getRoot" });
  const installed = await client.ask({ to: root.addonsActor, type: "installTemporaryAddon", addonPath: path.join(REPO, "extension") });
  check("Firefox installs the add-on", installed.addon?.id === "connect@opendm.org", installed.addon?.id ?? JSON.stringify(installed));
  await new Promise((r) => setTimeout(r, 1500));
  fs.writeFileSync(ROOT + "/site/go.json", JSON.stringify({ url: "http://127.0.0.1:8770/index.html?auto=zip&firefox=1" }));

  let saved;
  for (let i = 0; i < 200 && !saved; i++) {
    await new Promise((r) => setTimeout(r, 200));
    saved = files(APP_DL).find((f) => f.endsWith("Quarterly Report.zip"));
  }
  await new Promise((r) => setTimeout(r, 1500));
  const requests = fs.readFileSync(ROOT + "/requests.log", "utf8").trim().split("\n").map(JSON.parse).filter((r) => r.at > started && r.path === "/files/report.zip");
  const engine = requests.find((r) => !r.headers["Sec-Fetch-Dest"]);
  check("OpenDM saved the Firefox download", Boolean(saved), saved);
  check("the file is complete", Boolean(saved) && fs.statSync(APP_DL + "/" + saved).size === 6291456);
  check("the engine sent Firefox's user agent", /Firefox\//.test(engine?.headers["User-Agent"] ?? ""), engine?.headers["User-Agent"]);
  check("the engine sent the cookie", Boolean(engine?.headers.Cookie?.includes("session=abc123")), engine?.headers.Cookie);
  check("the engine sent the referer", Boolean(engine?.headers.Referer?.includes("auto=zip")), engine?.headers.Referer);
  check("Firefox kept no copy", files(BROWSER_DL).length === 0, files(BROWSER_DL).join(", "));
  const pageScript = fs.readFileSync(ROOT + "/requests.log", "utf8").trim().split("\n").map(JSON.parse)
    .filter((r) => r.at > started && r.path.startsWith("/log?page-script=")).at(-1);
  check("the page script runs in the page", pageScript?.path === "/log?page-script=true", pageScript?.path);
} catch (error) {
  check("test ran to the end", false, error.message);
} finally {
  client?.close();
  fs.rmSync(ROOT + "/site/go.json", { force: true });
  try {
    if (WINDOWS) execFileSync("taskkill", ["/F", "/T", "/PID", String(firefox.pid)], { stdio: "ignore" });
    else execFileSync("/usr/bin/pkill", ["-f", PROFILE]);
  } catch {}
}
process.exit(results.every(Boolean) ? 0 : 1);
