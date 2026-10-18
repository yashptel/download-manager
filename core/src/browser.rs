//! The browser extension's connection to the app.
//!
//! The native messaging host (`opendm-host`) relays the extension's messages
//! over a local socket: a Unix socket in the data folder, or a named pipe on
//! Windows. Each message is one JSON object per line. The extension asks with
//! an `id` and gets the same `id` back; the app also pushes capture rules
//! whenever they change.
//!
//! A captured download is "accepted" as soon as the app has it, and the
//! browser then drops its copy; the app's own dialog decides the rest. When
//! the app cannot take it, the answer is "fallback" and the browser keeps
//! downloading, so a download is never lost.
//!
//! "inspect" reads a stream's video and audio tracks for the extension's
//! quality menu, and "site" asks yt-dlp what a page offers. Both take a
//! moment, so they run beside the other messages.

use crate::{BrowserSource, EngineState, NewDownload, Shared};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// More links than any page should hand over at once.
const MAX_ITEMS: usize = 10_000;
/// The extension sends a playlist only when it is smaller than this.
const MAX_MANIFEST: usize = 2 * 1024 * 1024;

/// Request headers the engine sets itself, or that would break a download.
const SKIPPED_HEADERS: &[&str] = &[
    "accept-encoding",
    "connection",
    "content-length",
    "cookie",
    "host",
    "keep-alive",
    "proxy-authorization",
    "range",
    "referer",
    "te",
    "transfer-encoding",
    "upgrade",
    "user-agent",
];

/// Accepts browser connections until the core shuts down.
pub async fn serve(shared: Arc<Shared>) {
    if let Err(error) = listen(shared.clone()).await {
        shared.listener.error(format!(
            "The browser extension cannot reach OpenDM: {error}"
        ));
    }
}

#[cfg(unix)]
async fn listen(shared: Arc<Shared>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::{UnixListener, UnixStream};

    let path = opendm_host::endpoint(&shared.data_dir);
    if UnixStream::connect(&path).await.is_ok() {
        return Err("another OpenDM is already connected to the browser".into());
    }
    // A socket left by a crash.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let mut shutdown = shared.shutdown.subscribe();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                if let Ok((stream, _)) = accepted {
                    tokio::spawn(connection(shared.clone(), stream));
                }
            }
            _ = shutdown.wait_for(|stop| *stop) => break,
        }
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

#[cfg(windows)]
async fn listen(shared: Arc<Shared>) -> Result<(), String> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let name = opendm_host::endpoint(&shared.data_dir);
    let create = |first: bool| {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create(&name)
    };
    // Fails when another OpenDM already owns the pipe.
    let mut server = create(true).map_err(|error| error.to_string())?;
    let mut shutdown = shared.shutdown.subscribe();
    loop {
        tokio::select! {
            connected = server.connect() => {
                if connected.is_ok() {
                    let next = create(false).map_err(|error| error.to_string())?;
                    tokio::spawn(connection(shared.clone(), std::mem::replace(&mut server, next)));
                }
            }
            _ = shutdown.wait_for(|stop| *stop) => break,
        }
    }
    Ok(())
}

async fn connection<S: AsyncRead + AsyncWrite + Send + 'static>(shared: Arc<Shared>, stream: S) {
    let (reader, mut writer) = tokio::io::split(stream);
    let (sender, mut outbox) = mpsc::unbounded_channel::<String>();
    shared.browsers.lock().unwrap().push(sender.clone());
    let write = tokio::spawn(async move {
        while let Some(mut line) = outbox.recv().await {
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message["type"] == "hello" {
            // A browser with the extension: video sites will be asked for.
            crate::sites::prepare(shared.clone());
        }
        if message["type"] == "inspect" || message["type"] == "site" {
            let (shared, sender) = (shared.clone(), sender.clone());
            tokio::spawn(async move {
                let mut answer = if message["type"] == "site" {
                    site(&shared, &message).await
                } else {
                    inspect(&shared, &message).await
                };
                answer["id"] = message["id"].clone();
                let _ = sender.send(answer.to_string());
            });
            continue;
        }
        if let Some(mut answer) = respond(&shared, &message) {
            if let Some(id) = message.get("id") {
                answer["id"] = id.clone();
            }
            if sender.send(answer.to_string()).is_err() {
                break;
            }
        }
    }
    drop(sender);
    write.abort();
}

/// Sends the current capture rules to every connected browser.
pub fn push_rules(shared: &Shared) {
    let message = json!({ "type": "rules", "rules": rules(shared) }).to_string();
    shared
        .browsers
        .lock()
        .unwrap()
        .retain(|browser| browser.send(message.clone()).is_ok());
}

fn rules(shared: &Shared) -> Value {
    let settings = shared.settings.lock().unwrap();
    json!({
        "capture": settings.browser_capture,
        "extensions": settings.capture_extensions,
        "excludedSites": settings.capture_excluded_sites,
        "minSize": settings.capture_min_size,
    })
}

fn respond(shared: &Shared, message: &Value) -> Option<Value> {
    match message["type"].as_str()? {
        "hello" => Some(json!({
            "type": "hello",
            "app": "OpenDM",
            "version": env!("CARGO_PKG_VERSION"),
            "rules": rules(shared),
        })),
        "download" => Some(match accept(shared, message) {
            Ok(()) => json!({ "result": "accepted" }),
            Err(reason) => json!({ "result": "fallback", "reason": reason }),
        }),
        _ => None,
    }
}

/// Hands the downloads in `message` to the app, or says why it cannot take them.
fn accept(shared: &Shared, message: &Value) -> Result<(), String> {
    let source = match message["source"].as_str() {
        Some("download") => BrowserSource::Download,
        Some("link") => BrowserSource::Link,
        Some("links") => BrowserSource::Links,
        Some("media") => BrowserSource::Media,
        other => return Err(format!("unknown source {other:?}")),
    };
    if source == BrowserSource::Download && !shared.settings.lock().unwrap().browser_capture {
        return Err("capture is off".into());
    }
    if matches!(
        *shared.engine_state.lock().unwrap(),
        EngineState::Failed { .. } | EngineState::Stopped
    ) {
        return Err("the download engine is not running".into());
    }
    let downloads: Vec<NewDownload> = message["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .take(MAX_ITEMS)
                .filter_map(parse_item)
                .collect()
        })
        .unwrap_or_default();
    if downloads.is_empty() {
        return Err("no downloadable links".into());
    }
    shared.listener.browser_downloads(downloads, source);
    Ok(())
}

async fn inspect(shared: &Shared, message: &Value) -> Value {
    let Some(download) = parse_item(&message["item"]) else {
        return json!({ "result": "failed", "reason": "not a stream URL" });
    };
    match crate::media::probe(shared, &download).await {
        Ok(presentation) => json!({ "result": "ready", "presentation": presentation }),
        Err(reason) => json!({ "result": "failed", "reason": reason }),
    }
}

async fn site(shared: &Arc<Shared>, message: &Value) -> Value {
    let url = message["url"].as_str().unwrap_or("");
    let cookies = message["cookies"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    match crate::sites::lookup(shared, url, cookies).await {
        Ok(videos) => json!({ "result": "ready", "videos": videos }),
        Err(reason) => {
            let retry = reason == crate::sites::INSTALLING;
            json!({ "result": "failed", "reason": reason, "retry": retry })
        }
    }
}

fn parse_item(item: &Value) -> Option<NewDownload> {
    let text = |key: &str| {
        item[key]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(String::from)
    };
    let url = text("url")?;
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ftp://")) {
        return None;
    }
    let headers = item["headers"]
        .as_array()
        .map(|headers| {
            headers
                .iter()
                .filter_map(|header| Some((header["name"].as_str()?, header["value"].as_str()?)))
                .filter(|(name, _)| {
                    !name.is_empty()
                        && !SKIPPED_HEADERS.contains(&name.to_ascii_lowercase().as_str())
                })
                .map(|(name, value)| format!("{name}: {value}"))
                .collect()
        })
        .unwrap_or_default();
    // A name the browser resolved can carry a folder; only its last part counts.
    let filename =
        text("filename").map(|name| name.rsplit(['/', '\\']).next().unwrap_or(&name).to_string());
    let filename_source = text("filenameSource")
        .filter(|source| ["browser", "title", "suggested"].contains(&source.as_str()));
    // Track choices go to the engine as options, so only plain values pass.
    let track = |key: &str| {
        text(key).filter(|value| {
            value.len() <= 128
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    };
    let media_kind = text("mediaKind").filter(|kind| kind == "hls" || kind == "dash");
    Some(NewDownload {
        url,
        filename_source: filename.as_ref().and(filename_source),
        filename,
        referer: text("referrer"),
        cookie: text("cookie"),
        user_agent: text("userAgent"),
        headers,
        dir: None,
        mime: text("mime"),
        size: item["size"].as_u64().filter(|size| *size > 0),
        media_manifest: media_kind
            .as_ref()
            .and(item["mediaManifest"].as_str())
            .filter(|content| !content.is_empty() && content.len() <= MAX_MANIFEST)
            .map(String::from),
        media_kind,
        media_video: track("mediaVideo"),
        media_audio: track("mediaAudio"),
        media_format: text("mediaFormat").filter(|format| format == "mp4" || format == "mkv"),
        media_quality: text("mediaQuality").filter(|quality| quality.len() <= 200),
        category: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_keep_what_the_engine_can_use() {
        let item = json!({
            "url": "https://cdn.example.com/get?id=7",
            "referrer": "https://example.com/files",
            "cookie": "session=abc",
            "userAgent": "Mozilla/5.0",
            "filename": "Downloads/report final.pdf",
            "filenameSource": "browser",
            "mime": "application/pdf",
            "size": 1234,
            "headers": [
                { "name": "Authorization", "value": "Bearer t" },
                { "name": "Range", "value": "bytes=0-" },
                { "name": "cookie", "value": "duplicate" },
            ],
        });
        let download = parse_item(&item).unwrap();
        assert_eq!(download.url, "https://cdn.example.com/get?id=7");
        assert_eq!(download.filename.as_deref(), Some("report final.pdf"));
        assert_eq!(download.filename_source.as_deref(), Some("browser"));
        assert_eq!(
            download.referer.as_deref(),
            Some("https://example.com/files")
        );
        assert_eq!(download.cookie.as_deref(), Some("session=abc"));
        assert_eq!(
            download.headers,
            vec!["Authorization: Bearer t".to_string()]
        );
        assert_eq!(download.size, Some(1234));
    }

    #[test]
    fn page_and_script_urls_are_not_downloads() {
        for url in [
            "blob:https://example.com/1",
            "data:text/plain,hi",
            "javascript:void(0)",
            "",
        ] {
            assert!(parse_item(&json!({ "url": url })).is_none(), "{url}");
        }
        let unknown_source =
            parse_item(&json!({ "url": "https://a.b/c", "filenameSource": "guess" }));
        assert_eq!(unknown_source.unwrap().filename_source, None);
    }
}
