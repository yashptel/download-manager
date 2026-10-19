//! Reading an HLS or DASH presentation's tracks before the user picks one.
//!
//! The engine reads the playlist: a hidden task with
//! `media-pause-after-probe` stops as soon as the presentation is
//! parsed, its `media.tracks` are the answer, and the task is removed again.
//! The download the user then picks is an ordinary new task with
//! `media-video` and `media-audio` set. Track IDs come from the playlist
//! URLs, which some servers (YouTube) sign afresh on every request, so the
//! extension sends the same playlist text (`media-input`) with both.
//!
//! A collection is a video file and an audio file the engine joins into one,
//! for sites whose videos come as separate files (YouTube, when yt-dlp has no
//! playlist for it). Its files also travel as `media-input`, one track each.

use crate::engine::random_hex;
use crate::rpc::Rpc;
use crate::{NewDownload, Shared};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Probes and other hidden tasks have GIDs starting with this, so the task
/// list skips them.
const PROBE_PREFIX: &str = "0bd0be";
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// YouTube serves a whole file, or any piece above 10 MiB, at a crawl, and a
/// piece this size at full speed; yt-dlp downloads in the same pieces.
const PIECE: u64 = 10 * 1024 * 1024;
const MAX_URL: usize = 16384;

/// The files of a collection as the extension sends them, `[{ type, url,
/// size }]`, kept as one file per type with an http(s) URL; None when there
/// is nothing the engine could join.
pub fn collection_files(text: &str) -> Option<String> {
    let files: Vec<Value> = serde_json::from_str(text).ok()?;
    let mut kept: Vec<Value> = Vec::new();
    for file in files {
        let Some(kind) = file["type"].as_str().filter(|kind| ["video", "audio"].contains(kind)) else { continue };
        let Some(url) = file["url"].as_str() else { continue };
        let plain = url.len() <= MAX_URL
            && (url.starts_with("http://") || url.starts_with("https://"))
            && !url.chars().any(|c| c.is_whitespace() || c.is_control());
        if !plain || kept.iter().any(|f| f["type"] == kind) {
            continue;
        }
        kept.push(json!({ "type": kind, "url": url, "size": file["size"].as_u64() }));
    }
    (!kept.is_empty()).then(|| Value::Array(kept).to_string())
}

/// The engine's `media-input` for a collection: each file one track, named
/// after its type, as the quality menu's choices are.
pub fn collection_input(files: &str) -> Value {
    let files: Vec<Value> = serde_json::from_str(files).unwrap_or_default();
    let tracks: Vec<Value> = files
        .iter()
        .map(|file| {
            let url = file["url"].as_str().unwrap_or("");
            json!({ "id": file["type"], "type": file["type"], "urls": pieces(url, file["size"].as_u64().unwrap_or(0)) })
        })
        .collect();
    json!({ "manifests": [], "tracks": tracks, "keys": [] })
}

/// A file's URLs for the engine, which joins them into one byte stream.
/// YouTube's servers take a piece as a `range=` query parameter.
fn pieces(url: &str, size: u64) -> Vec<String> {
    let host = url.split('/').nth(2).unwrap_or("").rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("");
    let youtube = host == "googlevideo.com" || host.ends_with(".googlevideo.com");
    if !youtube || size == 0 {
        return vec![url.to_string()];
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    (0..size)
        .step_by(PIECE as usize)
        .map(|start| format!("{url}{separator}range={start}-{}", (start + PIECE).min(size) - 1))
        .collect()
}

pub fn is_probe(gid: &str) -> bool {
    gid.starts_with(PROBE_PREFIX)
}

/// A GID for an engine task that stays out of the task list.
pub fn hidden_gid() -> Result<String, String> {
    Ok(format!("{PROBE_PREFIX}{}", random_hex(5)?))
}

/// The tracks of the presentation at `download.url`, for the extension's
/// quality menu: `{ live, durationMs, tracks: [{ id, type, codec, ... }] }`.
pub async fn probe(shared: &Shared, download: &NewDownload) -> Result<Value, String> {
    if !matches!(download.media_kind.as_deref(), Some("hls" | "dash")) {
        return Err("only HLS and DASH streams have tracks to choose".into());
    }
    let rpc = shared.wait_rpc().await?;
    let gid = hidden_gid()?;
    let mut options = serde_json::Map::new();
    options.insert("gid".into(), json!(gid));
    options.insert(
        "dir".into(),
        json!(shared.data_dir.join("probes").display().to_string()),
    );
    options.insert("media-pause-after-probe".into(), json!("true"));
    download.engine_options(&mut options);

    // A full queue would hold the probe back; it gets a slot of its own and
    // goes first in line. Once paused it no longer counts as active.
    extra_slot(shared, &rpc, 1).await;
    let added = rpc
        .call(
            "aria2.addUri",
            vec![json!([download.url]), Value::Object(options), json!(0)],
        )
        .await;
    let result = match added {
        Ok(_) => wait(&rpc, &gid).await,
        Err(error) => Err(error),
    };
    extra_slot(shared, &rpc, -1).await;
    let _ = rpc.call("aria2.forceRemove", vec![json!(gid)]).await;
    let _ = rpc.call("aria2.removeDownloadResult", vec![json!(gid)]).await;
    result
}

async fn wait(rpc: &Rpc, gid: &str) -> Result<Value, String> {
    let started = Instant::now();
    loop {
        let status = rpc
            .call(
                "aria2.tellStatus",
                vec![json!(gid), json!(["status", "media", "errorMessage"])],
            )
            .await?;
        let media = &status["media"];
        match status["status"].as_str() {
            Some("paused") if media["state"] == "awaiting-selection" => return Ok(tracks(media)),
            Some("error") => {
                let message = [&media["error"], &status["errorMessage"]]
                    .into_iter()
                    .filter_map(Value::as_str)
                    .find(|message| !message.is_empty())
                    .unwrap_or("The stream could not be read");
                return Err(message.to_string());
            }
            Some("complete" | "removed") => return Err("This is not a stream".into()),
            _ => {}
        }
        if started.elapsed() > PROBE_TIMEOUT {
            return Err("The stream took too long to answer".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The engine's numbers are decimal strings; the extension gets numbers.
fn tracks(media: &Value) -> Value {
    let number = |value: &Value, key: &str| -> f64 {
        value[key]
            .as_str()
            .and_then(|text| text.parse().ok())
            .unwrap_or(0.0)
    };
    let text = |value: &Value, key: &str| value[key].as_str().unwrap_or("").to_string();
    let tracks: Vec<Value> = media["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|track| {
            json!({
                "id": text(track, "id"),
                "type": text(track, "type"),
                "language": text(track, "language"),
                "codec": text(track, "codec"),
                "width": number(track, "width") as u64,
                "height": number(track, "height") as u64,
                "bandwidth": number(track, "bandwidth") as u64,
                "frameRate": number(track, "frameRate"),
            })
        })
        .collect();
    json!({
        "protocol": text(media, "protocol"),
        "live": media["live"] == "true",
        "durationMs": number(media, "duration") as u64,
        "tracks": tracks,
    })
}

/// Hidden tasks each get an engine slot of their own, so a full download
/// queue does not hold them back.
pub async fn extra_slot(shared: &Shared, rpc: &Rpc, change: i32) {
    let extra = {
        let mut slots = shared.probe_slots.lock().unwrap();
        *slots = slots.saturating_add_signed(change);
        *slots
    };
    let base = shared
        .settings
        .lock()
        .unwrap()
        .max_concurrent_downloads
        .max(1);
    let _ = rpc
        .call(
            "aria2.changeGlobalOption",
            vec![json!({ "max-concurrent-downloads": (base + extra).to_string() })],
        )
        .await;
}
