//! Site support through yt-dlp. Some sites never show the browser a video
//! file or playlist (YouTube streams through its own protocol); yt-dlp turns
//! such a page into playlists and files, which then go through the same
//! quality menu and engine as anything the browser found.
//!
//! yt-dlp, and Deno for the scripts YouTube's player needs, are downloaded
//! by the engine into the data folder, in the background, once the browser
//! extension first connects. A lookup before they are there answers at once
//! that it should be retried. yt-dlp then updates itself at most once a
//! day, because sites change.

use crate::Shared;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};
use tokio::process::Command;

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(90);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 3600);
/// At most this many videos from a page that has several.
const MAX_VIDEOS: usize = 3;
/// The answer while yt-dlp is not there yet; the extension asks again.
pub const INSTALLING: &str =
    "OpenDM is getting its video site support ready (first time only). Try again in a minute.";

const YT_DLP_RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/";
const DENO_RELEASES: &str = "https://github.com/denoland/deno/releases/latest/download/";

struct Tools {
    yt_dlp: PathBuf,
    deno: PathBuf,
}

fn tools_dir(shared: &Shared) -> PathBuf {
    shared.data_dir.join("tools")
}

/// (yt-dlp file, Deno archive) for this platform.
fn assets() -> (&'static str, &'static str) {
    match (cfg!(windows), cfg!(target_arch = "aarch64")) {
        (true, true) => ("yt-dlp_arm64.exe", "deno-aarch64-pc-windows-msvc.zip"),
        (true, false) => ("yt-dlp.exe", "deno-x86_64-pc-windows-msvc.zip"),
        (false, true) => ("yt-dlp_macos", "deno-aarch64-apple-darwin.zip"),
        (false, false) => ("yt-dlp_macos", "deno-x86_64-apple-darwin.zip"),
    }
}

/// yt-dlp's view of a page: per video its title and the playlists and files
/// it can be downloaded from.
pub async fn lookup(shared: &Arc<Shared>, page_url: &str, cookies: &[Value]) -> Result<Value, String> {
    if !(page_url.starts_with("http://") || page_url.starts_with("https://")) {
        return Err("not a web page".into());
    }
    let tools = installed(shared);
    if !(tools.yt_dlp.exists() && tools.deno.exists()) {
        prepare(shared.clone());
        return Err(INSTALLING.into());
    }
    let dir = tools_dir(shared);
    let cookie_file = (!cookies.is_empty())
        .then(|| write_cookies(&dir, cookies))
        .transpose()?;

    let mut command = Command::new(&tools.yt_dlp);
    command
        .args(["-J", "--no-playlist", "--no-warnings", "--ignore-config"])
        .args(["--playlist-items", "1-3"])
        .arg("--js-runtimes")
        .arg(format!("deno:{}", tools.deno.display()))
        .arg("--cache-dir")
        .arg(dir.join("cache"));
    if let Some(file) = &cookie_file {
        command.arg("--cookies").arg(file);
    }
    command.arg("--").arg(page_url).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let output = tokio::time::timeout(LOOKUP_TIMEOUT, command.output()).await;
    if let Some(file) = &cookie_file {
        let _ = std::fs::remove_file(file);
    }
    let output = output
        .map_err(|_| "The site took too long to answer".to_string())?
        .map_err(|error| format!("yt-dlp did not start: {error}"))?;
    if !output.status.success() {
        let errors = String::from_utf8_lossy(&output.stderr);
        let message = errors
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("ERROR: "))
            .unwrap_or("yt-dlp could not read this page");
        return Err(message.trim().to_string());
    }
    let info: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("yt-dlp answered something unexpected: {error}"))?;
    update_later(shared);
    Ok(videos(&info))
}

/// The parts of yt-dlp's answer the extension needs; the full answer can be
/// several megabytes, more than a browser message may carry.
fn videos(info: &Value) -> Value {
    let videos: Vec<&Value> = match info["entries"].as_array() {
        Some(entries) => entries.iter().take(MAX_VIDEOS).collect(),
        None => vec![info],
    };
    Value::Array(videos.into_iter().map(video).collect())
}

fn video(info: &Value) -> Value {
    let single = [info.clone()];
    let formats = info["formats"].as_array().map(Vec::as_slice).unwrap_or(&single);
    let mut streams: Vec<Value> = Vec::new();
    let mut files = Vec::new();
    for format in formats {
        if format["has_drm"] == true {
            continue;
        }
        let headers: Vec<Value> = format["http_headers"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| Some(json!({ "name": name, "value": value.as_str()? })))
            .collect();
        let protocol = format["protocol"].as_str().unwrap_or("");
        let kind = match protocol {
            "m3u8" | "m3u8_native" => Some("hls"),
            "http_dash_segments" | "http_dash_segments_generator" => Some("dash"),
            _ => None,
        };
        if let Some(kind) = kind {
            let Some(url) = format["manifest_url"].as_str() else { continue };
            if !streams.iter().any(|stream| stream["url"] == url) {
                streams.push(json!({ "url": url, "kind": kind, "headers": headers }));
            }
        } else if matches!(protocol, "http" | "https") && format["url"].is_string() {
            files.push(json!({
                "id": format["format_id"],
                "url": format["url"],
                "ext": format["ext"],
                "vcodec": format["vcodec"],
                "acodec": format["acodec"],
                "width": format["width"],
                "height": format["height"],
                "fps": format["fps"],
                "tbr": format["tbr"],
                "size": format["filesize"].as_u64().or(format["filesize_approx"].as_u64()),
                "language": format["language"],
                // The video's own language ranks highest; dubbed ones rank below.
                "preference": format["language_preference"],
                "headers": headers,
            }));
        }
    }
    json!({
        "title": info["title"],
        "duration": info["duration"],
        "site": info["extractor_key"],
        "language": info["language"],
        "streams": streams,
        "files": files,
    })
}

/// The browser's cookies for the site, in the cookies.txt format yt-dlp reads.
fn write_cookies(dir: &Path, cookies: &[Value]) -> Result<PathBuf, String> {
    let clean = |value: &Value| {
        value
            .as_str()
            .filter(|text| !text.contains(['\t', '\r', '\n']))
            .map(String::from)
    };
    let mut text = String::from("# Netscape HTTP Cookie File\n");
    for cookie in cookies.iter().take(500) {
        let (Some(domain), Some(name), Some(value)) =
            (clean(&cookie["domain"]), clean(&cookie["name"]), clean(&cookie["value"]))
        else {
            continue;
        };
        let path = clean(&cookie["path"]).unwrap_or_else(|| "/".into());
        let host_only = cookie["hostOnly"] == true;
        let domain = match (host_only, domain.starts_with('.')) {
            (false, false) => format!(".{domain}"),
            _ => domain,
        };
        let flag = |on: bool| if on { "TRUE" } else { "FALSE" };
        let expires = cookie["expirationDate"].as_f64().unwrap_or(0.0) as u64;
        text.push_str(&format!(
            "{domain}\t{}\t{path}\t{}\t{expires}\t{name}\t{value}\n",
            flag(!host_only),
            flag(cookie["secure"] == true),
        ));
    }
    let path = dir.join(format!("cookies-{}.txt", crate::engine::random_hex(6)?));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut file, text.as_bytes()).map_err(|error| error.to_string())?;
    Ok(path)
}

fn installed(shared: &Shared) -> Tools {
    let dir = tools_dir(shared);
    let exe = if cfg!(windows) { ".exe" } else { "" };
    Tools {
        yt_dlp: dir.join(format!("yt-dlp{exe}")),
        deno: dir.join(format!("deno{exe}")),
    }
}

/// Downloads yt-dlp and Deno in the background unless they are there or
/// already on their way.
pub fn prepare(shared: Arc<Shared>) {
    let tools = installed(&shared);
    if tools.yt_dlp.exists() && tools.deno.exists() {
        return;
    }
    if shared.tools_busy.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        if let Err(error) = install(&shared, &tools).await {
            shared.listener.error(format!("Could not set up video site support: {error}"));
        }
        shared.tools_busy.store(false, Ordering::SeqCst);
    });
}

async fn install(shared: &Shared, tools: &Tools) -> Result<(), String> {
    let dir = tools_dir(shared);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let (yt_dlp, deno) = assets();
    if !tools.yt_dlp.exists() {
        // Under another name until complete: the engine sizes the file first.
        fetch(shared, &format!("{YT_DLP_RELEASES}{yt_dlp}"), &dir, "yt-dlp.download").await?;
        std::fs::rename(dir.join("yt-dlp.download"), &tools.yt_dlp).map_err(|error| error.to_string())?;
        make_executable(&tools.yt_dlp)?;
        touch(&dir.join("updated"));
    }
    if !tools.deno.exists() {
        fetch(shared, &format!("{DENO_RELEASES}{deno}"), &dir, "deno.zip").await?;
        // bsdtar reads zip archives, and ships with macOS and Windows 10+.
        // The archive holds the one executable, so a cut-off unpack leaves
        // it missing or short; the next attempt overwrites it.
        let unpacked = Command::new("tar")
            .arg("-xf")
            .arg(dir.join("deno.zip"))
            .arg("-C")
            .arg(&dir)
            .output()
            .await
            .map_err(|error| format!("Could not unpack Deno: {error}"))?;
        let _ = std::fs::remove_file(dir.join("deno.zip"));
        if !unpacked.status.success() || !tools.deno.exists() {
            return Err("Could not unpack Deno".into());
        }
        make_executable(&tools.deno)?;
    }
    Ok(())
}

/// Downloads one file with the engine, as a hidden task.
async fn fetch(shared: &Shared, url: &str, dir: &Path, name: &str) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let gid = crate::media::hidden_gid()?;
    let options = json!({
        "gid": gid,
        "dir": dir.display().to_string(),
        "out": name,
        "allow-overwrite": "true",
        "auto-file-renaming": "false",
        "split": "4",
        "max-connection-per-server": "4",
    });
    crate::media::extra_slot(shared, &rpc, 1).await;
    let added = rpc
        .call("aria2.addUri", vec![json!([url]), options, json!(0)])
        .await;
    let mut result = added.map(|_| ());
    let started = std::time::Instant::now();
    while result.is_ok() {
        let status = rpc
            .call("aria2.tellStatus", vec![json!(gid), json!(["status", "errorMessage"])])
            .await?;
        match status["status"].as_str() {
            Some("complete") => break,
            Some("error" | "removed") => {
                let reason = status["errorMessage"].as_str().unwrap_or("download failed");
                result = Err(format!("Could not download {name}: {reason}"));
            }
            _ if started.elapsed() > INSTALL_TIMEOUT => {
                result = Err(format!("Downloading {name} took too long"));
            }
            _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    crate::media::extra_slot(shared, &rpc, -1).await;
    let _ = rpc.call("aria2.forceRemove", vec![json!(gid)]).await;
    let _ = rpc.call("aria2.removeDownloadResult", vec![json!(gid)]).await;
    result
}

fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    let _ = path;
    Ok(())
}

fn touch(path: &Path) {
    let _ = std::fs::write(path, b"");
}

/// Lets yt-dlp replace itself with its newest release, once a day.
fn update_later(shared: &Shared) {
    let stamp = tools_dir(shared).join("updated");
    let fresh = std::fs::metadata(&stamp)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_some_and(|age| age < UPDATE_EVERY);
    if fresh {
        return;
    }
    touch(&stamp);
    let yt_dlp = installed(shared).yt_dlp;
    tokio::spawn(async move {
        let mut command = Command::new(yt_dlp);
        command.args(["--update", "--ignore-config"]).kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let _ = tokio::time::timeout(Duration::from_secs(300), command.output()).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_become_streams_and_files() {
        let info = json!({
            "title": "A talk",
            "extractor_key": "Example",
            "formats": [
                { "format_id": "sb0", "protocol": "mhtml", "url": "https://i.example/sb" },
                { "format_id": "hls-1", "protocol": "m3u8_native", "url": "https://x/1.m3u8", "manifest_url": "https://x/master.m3u8" },
                { "format_id": "hls-2", "protocol": "m3u8_native", "url": "https://x/2.m3u8", "manifest_url": "https://x/master.m3u8" },
                { "format_id": "140", "protocol": "https", "url": "https://x/a.m4a", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "filesize": 100,
                  "language": "en-US", "language_preference": 10, "http_headers": { "User-Agent": "UA" } },
                { "format_id": "drm", "protocol": "https", "url": "https://x/d.mp4", "has_drm": true },
            ],
        });
        let videos = videos(&info);
        let video = &videos[0];
        assert_eq!(video["title"], "A talk");
        assert_eq!(video["streams"], json!([{ "url": "https://x/master.m3u8", "kind": "hls", "headers": [] }]));
        let files = video["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["id"], "140");
        assert_eq!(files[0]["size"], 100);
        assert_eq!(files[0]["language"], "en-US");
        // The video's own language ranks highest; dubbed ones rank below.
        assert_eq!(files[0]["preference"], 10);
        assert_eq!(files[0]["headers"], json!([{ "name": "User-Agent", "value": "UA" }]));
    }

    #[test]
    fn cookies_are_written_for_yt_dlp() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_cookies(
            dir.path(),
            &[
                json!({ "domain": "example.com", "hostOnly": false, "path": "/", "secure": true, "expirationDate": 1900000000.5, "name": "sid", "value": "abc" }),
                json!({ "domain": "www.example.com", "hostOnly": true, "name": "pref", "value": "x" }),
                json!({ "domain": "example.com", "name": "bad", "value": "a\nb" }),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(
            text,
            "# Netscape HTTP Cookie File\n\
             .example.com\tTRUE\t/\tTRUE\t1900000000\tsid\tabc\n\
             www.example.com\tFALSE\t/\tFALSE\t0\tpref\tx\n"
        );
    }
}
