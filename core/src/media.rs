//! Reading an HLS or DASH presentation's tracks before the user picks one.
//!
//! The engine reads the playlist: a hidden task with
//! `media-pause-after-probe` stops as soon as the presentation is
//! parsed, its `media.tracks` are the answer, and the task is removed again.
//! Track IDs are stable, so the download the user then picks is an ordinary
//! new task with `media-video` and `media-audio` set.

use crate::engine::random_hex;
use crate::rpc::Rpc;
use crate::{NewDownload, Shared};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Probe tasks have GIDs starting with this, so the task list skips them.
const PROBE_PREFIX: &str = "0bd0be";
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

pub fn is_probe(gid: &str) -> bool {
    gid.starts_with(PROBE_PREFIX)
}

/// The tracks of the presentation at `download.url`, for the extension's
/// quality menu: `{ live, durationMs, tracks: [{ id, type, codec, ... }] }`.
pub async fn probe(shared: &Shared, download: &NewDownload) -> Result<Value, String> {
    if !matches!(download.media_kind.as_deref(), Some("hls" | "dash")) {
        return Err("only HLS and DASH streams have tracks to choose".into());
    }
    let rpc = shared.wait_rpc().await?;
    let gid = format!("{PROBE_PREFIX}{}", random_hex(5)?);
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
    set_probe_slots(shared, &rpc, 1).await;
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
    set_probe_slots(shared, &rpc, -1).await;
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

async fn set_probe_slots(shared: &Shared, rpc: &Rpc, change: i32) {
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

