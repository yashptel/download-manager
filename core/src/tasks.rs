//! The task table and the user actions on it.
//!
//! Each tick asks aria2 only for the active downloads (one round trip).
//! Tasks named by an engine notification, tasks that left the active list,
//! and active tasks whose files the engine has not reported yet get a full
//! status in a second round trip. Only rows that actually changed are sent
//! to the UI.

use crate::db::progress;
use crate::engine::random_hex;
use crate::rpc::Rpc;
use crate::settings::{OTHER_CATEGORY, Settings};
use crate::{ConnectionInfo, GlobalStats, NewDownload, Shared, TaskDetails, TaskRow, TaskStatus};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// While a window shows progress, often enough that the numbers count up
/// as the bytes arrive; the engine's speed is already averaged over seconds.
const VISIBLE_INTERVAL: Duration = Duration::from_millis(250);
const BACKGROUND_INTERVAL: Duration = Duration::from_secs(2);
/// Waits before a failed download is tried again on its own; the last one
/// repeats. The engine has already retried each connection a few times, so
/// these are for what passes in minutes: a dropped network, or a server
/// that blocks this address for a while.
const RETRY_WAITS: [Duration; 5] = [
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
    Duration::from_secs(600),
];
/// Tries in a row that get no further before a download stays failed.
const MAX_RETRIES: u32 = 10;
/// Adds are sent in batches so a paste of thousands of links stays one
/// round trip per batch.
const ADD_BATCH: usize = 500;

const ACTIVE_KEYS: &[&str] = &[
    "gid",
    "status",
    "totalLength",
    "completedLength",
    "downloadSpeed",
    "connections",
    "media",
];
const FULL_KEYS: &[&str] = &[
    "gid",
    "status",
    "totalLength",
    "completedLength",
    "downloadSpeed",
    "connections",
    "media",
    "errorCode",
    "errorMessage",
    "dir",
    "files",
    "bittorrent",
];

const INTERRUPTED: &str = "Interrupted. Resume to continue.";

#[derive(Debug, Clone)]
pub struct Task {
    pub row: TaskRow,
    /// Every output file the engine reported, for deleting with the task.
    pub files: Vec<String>,
    /// An HLS/DASH presentation; progress comes from media time, not bytes.
    pub media: bool,
    /// A folder and name chosen after the download started. A running
    /// download cannot move, so it moves there when it completes.
    pub move_dir: Option<String>,
    pub move_name: Option<String>,
    /// The page the download came from (the browser's referer), for
    /// getting a new address when this one expires.
    pub page: Option<String>,
    /// Restarted once with the browser's fresh cookies after a 401 or 403
    /// since the user last started it; not saved.
    pub cookies_refreshed: bool,
    /// Tries made on its own since the download last got further, and its
    /// bytes done then; not saved.
    pub retries: u32,
    pub retried_from: u64,
    /// The rate behind time left, while the download runs; not saved.
    pub rate: Option<crate::eta::RateFilter>,
}

impl Task {
    pub fn new(id: String, url: String, dir: String, added_at: i64) -> Task {
        Task {
            row: TaskRow {
                name: name_from_url(&url),
                id,
                url,
                dir,
                path: None,
                category: OTHER_CATEGORY.into(),
                status: TaskStatus::Queued,
                total_bytes: 0,
                done_bytes: 0,
                speed: 0,
                progress: -1.0,
                eta_secs: None,
                connections: 0,
                error: None,
                retry_at: None,
                added_at,
                finished_at: None,
            },
            files: Vec::new(),
            media: false,
            move_dir: None,
            move_name: None,
            page: None,
            cookies_refreshed: false,
            retries: 0,
            retried_from: 0,
            rate: None,
        }
    }
}

#[derive(Default)]
pub struct Table {
    pub tasks: HashMap<String, Task>,
    /// GIDs in the last `tellActive` answer.
    active: HashSet<String>,
    /// GIDs that need a full status on the next tick.
    dirty: HashSet<String>,
    /// GIDs being removed; engine answers about them are ignored.
    removing: HashSet<String>,
    /// Captured downloads started before the app has taken them; they stay
    /// out of the list until the server's first answer says it can.
    held: HashSet<String>,
    /// Failed with 401 or 403 (the value): the poller asks the browser for
    /// new cookies, then the user for a login.
    refused: HashMap<String, u16>,
    /// Failed partway for a reason that may pass: the poller sets a time to
    /// try again.
    interrupted: HashSet<String>,
    stats: GlobalStats,
}

impl Table {
    pub fn mark_dirty(&mut self, id: String) {
        self.dirty.insert(id);
    }

    pub fn rows(&self) -> Vec<TaskRow> {
        let mut rows: Vec<TaskRow> = self.tasks.values().map(|task| task.row.clone()).collect();
        rows.sort_by(|a, b| a.added_at.cmp(&b.added_at).then_with(|| a.id.cmp(&b.id)));
        rows
    }

    pub fn stats(&self) -> GlobalStats {
        self.stats
    }

    /// When a new batch of downloads starts. Each pasted link takes the next
    /// millisecond to keep its order, so a large batch reaches into the future;
    /// the next batch starts after it and never sorts into its middle.
    pub fn next_added_at(&self, now: i64) -> i64 {
        let newest = self.tasks.values().map(|task| task.row.added_at).max();
        newest.map_or(now, |newest| now.max(newest + 1))
    }
}

fn is_unfinished(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Downloading | TaskStatus::Queued | TaskStatus::Paused
    )
}

pub async fn run_poller(shared: Arc<Shared>) {
    let mut shutdown = shared.shutdown.subscribe();
    loop {
        let interval = if shared.visible.load(Ordering::Relaxed) {
            VISIBLE_INTERVAL
        } else {
            BACKGROUND_INTERVAL
        };
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = shared.wake.notified() => {}
            _ = crate::stopped(&mut shutdown) => return,
        }
        if let Some(rpc) = shared.current_rpc() {
            // A failing tick means the engine is going away; the supervisor handles that.
            let _ = tick(&shared, &rpc).await;
        }
        let refused = std::mem::take(&mut shared.tasks.lock().unwrap().refused);
        for (id, status) in refused {
            tokio::spawn(after_refusal(shared.clone(), id, status));
        }
        let interrupted = std::mem::take(&mut shared.tasks.lock().unwrap().interrupted);
        for id in interrupted {
            if let Some((at, wait)) = schedule_retry(&shared, &id) {
                tokio::spawn(retry_later(shared.clone(), id, at, wait));
            }
        }
    }
}

async fn tick(shared: &Shared, rpc: &Rpc) -> Result<(), String> {
    let mut answers = rpc
        .multicall(vec![
            ("aria2.tellActive", vec![json!(ACTIVE_KEYS)]),
            ("aria2.getGlobalStat", vec![]),
        ])
        .await?
        .into_iter();
    let active = into_list(
        answers
            .next()
            .unwrap_or_else(|| Err("missing answer".into()))?,
    );
    let stats = answers
        .next()
        .and_then(Result::ok)
        .map(|value| parse_stats(&value));

    let active_ids: HashSet<String> = active.iter().filter_map(gid).map(String::from).collect();
    let need: Vec<String> = {
        let mut table = shared.tasks.lock().unwrap();
        let mut need = std::mem::take(&mut table.dirty);
        need.extend(table.active.difference(&active_ids).cloned());
        // The engine names the file only after the server answers, which can
        // be after the notification that started the download.
        need.extend(
            active_ids
                .iter()
                .filter(|id| {
                    !table.held.contains(*id)
                        && table
                            .tasks
                            .get(*id)
                            .is_none_or(|task| task.files.is_empty())
                })
                .cloned(),
        );
        table.active = active_ids;
        need.into_iter().collect()
    };
    let full = rpc
        .multicall(
            need.iter()
                .map(|id| ("aria2.tellStatus", vec![json!(id), json!(FULL_KEYS)]))
                .collect(),
        )
        .await?;

    let mut statuses = active;
    let mut missing = Vec::new();
    for (id, answer) in need.into_iter().zip(full) {
        match answer {
            Ok(status) => statuses.push(status),
            Err(_) => missing.push(id),
        }
    }
    apply(shared, rpc, &statuses, &missing, stats).await;
    Ok(())
}

/// Reads every task the engine knows after it (re)starts. Unfinished tasks
/// the engine lost are marked interrupted so the user can resume them.
pub async fn sync_all(shared: &Shared, rpc: &Rpc) -> Result<(), String> {
    let answers = rpc
        .multicall(vec![
            ("aria2.tellActive", vec![json!(FULL_KEYS)]),
            (
                "aria2.tellWaiting",
                vec![json!(0), json!(1_000_000), json!(FULL_KEYS)],
            ),
            (
                "aria2.tellStopped",
                vec![json!(0), json!(1_000_000), json!(FULL_KEYS)],
            ),
        ])
        .await?;
    let mut lists = Vec::new();
    for answer in answers {
        lists.push(into_list(answer?));
    }
    let statuses: Vec<Value> = lists.concat();
    // Probes left behind by a crash.
    let probes: Vec<(&str, Vec<Value>)> = statuses
        .iter()
        .filter_map(gid)
        .filter(|id| crate::media::is_probe(id))
        .flat_map(|id| {
            [
                ("aria2.forceRemove", vec![json!(id)]),
                ("aria2.removeDownloadResult", vec![json!(id)]),
            ]
        })
        .collect();
    if !probes.is_empty() {
        let _ = rpc.multicall(probes).await;
    }
    let known: HashSet<&str> = statuses.iter().filter_map(gid).collect();
    let missing: Vec<String> = {
        let mut table = shared.tasks.lock().unwrap();
        table.active = lists
            .first()
            .into_iter()
            .flatten()
            .filter_map(gid)
            .map(String::from)
            .collect();
        table.dirty.clear();
        table
            .tasks
            .values()
            .filter(|task| is_unfinished(task.row.status) && !known.contains(task.row.id.as_str()))
            .map(|task| task.row.id.clone())
            .collect()
    };
    apply(shared, rpc, &statuses, &missing, None).await;
    Ok(())
}

async fn apply(
    shared: &Shared,
    rpc: &Rpc,
    statuses: &[Value],
    missing: &[String],
    stats: Option<GlobalStats>,
) {
    let settings = shared.settings.lock().unwrap().clone();
    let now = now_ms();
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    let mut persist = Vec::new();
    let mut purge = Vec::new();
    let mut moves = Vec::new();
    let mut new_stats = None;
    {
        let mut table = shared.tasks.lock().unwrap();
        for status in statuses {
            let Some(id) = gid(status) else { continue };
            if table.removing.contains(id) || table.held.contains(id) || crate::media::is_probe(id) {
                continue;
            }
            if status["status"] == "removed" {
                // Removed by another RPC client.
                if table.tasks.remove(id).is_some() {
                    removed.push(id.to_string());
                }
                continue;
            }
            let is_new = !table.tasks.contains_key(id);
            let task = table
                .tasks
                .entry(id.to_string())
                .or_insert_with(|| Task::new(id.to_string(), String::new(), String::new(), now));
            let before = task.row.clone();
            update_from_engine(task, status, &settings, now);
            let refused = match status["errorMessage"].as_str() {
                Some(message) if message.starts_with("HTTP 401") => Some(401),
                Some(message) if message.starts_with("HTTP 403") => Some(403),
                _ => None,
            }
            .filter(|_| {
                task.row.status == TaskStatus::Failed && before.status != TaskStatus::Failed && !task.media
            });
            let interrupted = task.row.status == TaskStatus::Failed
                && before.status != TaskStatus::Failed
                && !task.media
                && task.row.done_bytes > 0
                && is_temporary(status);
            if task.row != before || is_new {
                changed.push(task.row.clone());
                if is_new || task.row.status != before.status || task.row.name != before.name {
                    persist.push(task.clone());
                }
                // Finished results leave the engine; history keeps them.
                if task.row.status == TaskStatus::Complete && before.status != TaskStatus::Complete
                {
                    purge.push(id.to_string());
                    if task.move_dir.is_some() || task.move_name.is_some() {
                        moves.push(id.to_string());
                    }
                }
            }
            if let Some(code) = refused {
                table.refused.insert(id.to_string(), code);
            }
            if interrupted {
                table.interrupted.insert(id.to_string());
            }
        }
        for id in missing {
            if let Some(task) = table.tasks.get_mut(id)
                && is_unfinished(task.row.status)
            {
                task.row.status = TaskStatus::Failed;
                task.row.error = Some(INTERRUPTED.into());
                task.row.speed = 0;
                task.row.eta_secs = None;
                task.row.connections = 0;
                changed.push(task.row.clone());
                persist.push(task.clone());
            }
        }
        if let Some(stats) = stats
            && stats != table.stats
        {
            table.stats = stats;
            new_stats = Some(stats);
        }
    }

    if !persist.is_empty() || !removed.is_empty() {
        let mut db = shared.db.lock().unwrap();
        let saved = db.save(&persist).and_then(|_| db.delete(&removed));
        if let Err(error) = saved {
            shared
                .listener
                .error(format!("Could not save download history: {error}"));
        }
    }
    if !purge.is_empty() {
        let _ = rpc
            .multicall(
                purge
                    .iter()
                    .map(|id| ("aria2.removeDownloadResult", vec![json!(id)]))
                    .collect(),
            )
            .await;
    }
    if !changed.is_empty() {
        shared.listener.rows_changed(changed);
    }
    if !removed.is_empty() {
        shared.listener.rows_removed(removed);
    }
    for id in moves {
        move_finished(shared, &id);
    }
    if let Some(stats) = new_stats {
        shared.listener.stats_changed(stats);
    }
}

/// Copies the fields present in an aria2 status object into the task.
/// `tellActive` answers carry only some keys, so absent keys keep their value.
fn update_from_engine(task: &mut Task, status: &Value, settings: &Settings, now: i64) {
    let row = &mut task.row;
    if let Some(state) = status["status"].as_str() {
        row.status = match state {
            "active" => TaskStatus::Downloading,
            "waiting" => TaskStatus::Queued,
            "paused" => TaskStatus::Paused,
            "complete" => TaskStatus::Complete,
            _ => TaskStatus::Failed,
        };
    }
    if let Some(total) = number(status, "totalLength") {
        row.total_bytes = total;
    }
    if let Some(done) = number(status, "completedLength") {
        row.done_bytes = done;
    }
    if let Some(speed) = number(status, "downloadSpeed") {
        row.speed = speed;
    }
    if let Some(connections) = number(status, "connections") {
        row.connections = connections as u32;
    }
    if let Some(dir) = status["dir"].as_str() {
        row.dir = native_path(dir);
    }
    if let Some(files) = status["files"].as_array() {
        task.files = files
            .iter()
            .filter_map(|file| file["path"].as_str())
            .filter(|path| !path.is_empty())
            .map(native_path)
            .collect();
        if row.url.is_empty()
            && let Some(uri) = files
                .first()
                .and_then(|file| file.pointer("/uris/0/uri"))
                .and_then(Value::as_str)
        {
            row.url = uri.to_string();
            row.name = name_from_url(uri);
        }
    }
    if let Some(name) = status
        .pointer("/bittorrent/info/name")
        .and_then(Value::as_str)
    {
        row.name = name.to_string();
        row.path = Some(
            std::path::Path::new(&row.dir)
                .join(name)
                .display()
                .to_string(),
        );
    } else if let Some(first) = task.files.first() {
        row.name = base_name(first).to_string();
        row.path = Some(first.clone());
    }
    if let Some(name) = &task.move_name {
        row.name = name.clone();
    }

    let media = status.get("media").filter(|media| media.is_object());
    if let Some(media) = media {
        task.media = true;
        // Output size is unknown until remuxing finishes; progress is media time.
        row.progress = media["progress"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .unwrap_or(-1.0);
        row.done_bytes = number(media, "downloadedLength").unwrap_or(row.done_bytes);
    } else if !task.media {
        row.progress = progress(row.status, row.total_bytes, row.done_bytes);
    }
    if row.status == TaskStatus::Complete {
        row.progress = 1.0;
        row.finished_at.get_or_insert(now);
    }

    row.error = match row.status {
        TaskStatus::Failed => Some(
            media
                .and_then(|media| media["error"].as_str())
                .filter(|message| !message.is_empty())
                .or_else(|| {
                    status["errorMessage"]
                        .as_str()
                        .filter(|message| !message.is_empty())
                })
                .map(friendly_error)
                .or_else(|| row.error.clone())
                .unwrap_or_else(|| "Download failed".into()),
        ),
        _ => None,
    };
    if row.status != TaskStatus::Failed {
        row.retry_at = None;
    }
    if row.status != TaskStatus::Downloading {
        row.speed = 0;
        row.connections = 0;
    }
    row.eta_secs = match row.status {
        TaskStatus::Downloading if !task.media && row.total_bytes > row.done_bytes => {
            let rate = task
                .rate
                .get_or_insert_with(|| crate::eta::RateFilter::new(now, row.done_bytes, row.speed));
            rate.update(now, row.done_bytes, row.speed);
            rate.time_left(row.total_bytes - row.done_bytes)
        }
        _ => {
            task.rate = None;
            None
        }
    };
    row.category = settings.classify(&row.name);
}

// User actions. Each one changes the engine, then marks the tasks dirty so
// the next tick (requested immediately) reports their new state.

impl NewDownload {
    pub fn url(url: &str) -> NewDownload {
        NewDownload {
            url: url.to_string(),
            ..NewDownload::default()
        }
    }

    /// Per-download engine options beyond the folder and the GID.
    pub fn engine_options(&self, options: &mut serde_json::Map<String, Value>) {
        self.request_options(options);
        let mut put = |key: &str, value: &Option<String>| {
            if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                options.insert(key.into(), json!(value));
            }
        };
        // A name the user typed is used as it is; a stream's still gets the
        // container's extension from the engine.
        let (hint, source) = match (self.filename_source.as_deref(), &self.filename) {
            (Some("user"), Some(name)) if self.media_kind.is_some() => {
                let format = self.media_format.as_deref().unwrap_or("mp4");
                let stem = name.strip_suffix(&format!(".{format}")).unwrap_or(name);
                (Some(stem.to_string()), Some("title".to_string()))
            }
            (Some("user"), name) => (name.clone(), Some("browser".to_string())),
            (_, name) => (name.clone(), self.filename_source.clone()),
        };
        put("filename-hint", &hint);
        if hint.is_some() {
            put("filename-hint-source", &source);
        }
        put("media", &self.media_kind);
        put("media-video", &self.media_video);
        put("media-audio", &self.media_audio);
        put("media-format", &self.media_format);
        if let Some(content) = self.media_manifest.as_deref().filter(|c| !c.is_empty()) {
            let input = json!({
                "manifests": [{ "url": self.url.trim(), "content": content }],
                "tracks": [],
                "keys": [],
            });
            options.insert("media-input".into(), json!(input.to_string()));
        }
    }

    /// What makes the server answer as it did for the browser.
    fn request_options(&self, options: &mut serde_json::Map<String, Value>) {
        for (key, value) in [("referer", &self.referer), ("user-agent", &self.user_agent)] {
            if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                options.insert(key.into(), json!(value));
            }
        }
        let mut headers: Vec<String> = self
            .headers
            .iter()
            // Header values must not smuggle in a second header.
            .filter(|header| header.contains(':') && !header.contains(['\r', '\n']))
            .cloned()
            .collect();
        if let Some(cookie) = self
            .cookie
            .as_deref()
            .map(str::trim)
            .filter(|cookie| !cookie.is_empty() && !cookie.contains(['\r', '\n']))
        {
            headers.push(format!("Cookie: {cookie}"));
        }
        if !headers.is_empty() {
            options.insert("header".into(), json!(headers));
        }
        if let Some(proxy) = self.proxy.as_deref().filter(|proxy| !proxy.is_empty()) {
            options.insert("all-proxy".into(), json!(proxy));
            // OpenDM's own exceptions belong to its own proxy.
            options.insert("no-proxy".into(), json!(""));
        }
    }

    /// The name this download most likely gets, before any server answers.
    pub fn planned_name(&self) -> String {
        let hint = self.filename.as_deref().map(str::trim).filter(|n| !n.is_empty());
        match hint {
            // The engine adds a stream's container to its title.
            Some(name) if self.media_kind.is_some() && self.filename_source.as_deref() != Some("user") => {
                format!("{name}.{}", self.media_format.as_deref().unwrap_or("mp4"))
            }
            Some(name) => name.to_string(),
            None => name_from_url(self.url.trim()),
        }
    }

    /// Category and folder for this download once it is called `name`.
    pub fn place(&self, settings: &Settings, name: &str) -> (String, String) {
        let category = match self.category.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(category) => category.to_string(),
            None => match settings.classify(name) {
                // A name without a known extension, such as a page title:
                // the URL's type still counts (a playlist is a video).
                other if other == OTHER_CATEGORY => settings.classify(&name_from_url(self.url.trim())),
                category => category,
            },
        };
        let dir = match self.dir.as_deref().filter(|dir| !dir.is_empty()) {
            Some(dir) => dir.to_string(),
            None => settings.directory_for(&category).display().to_string(),
        };
        (category, dir)
    }
}

/// A running download's segments and connections, for its progress window.
/// `cells` is how many parts the segment map is drawn in; each holds the
/// percentage of its pieces that are done.
pub async fn details(shared: &Shared, id: &str, cells: usize) -> TaskDetails {
    let mut details = TaskDetails { segments: Vec::new(), connections: Vec::new() };
    let Some(rpc) = shared.current_rpc() else { return details };
    let Ok(answers) = rpc
        .multicall(vec![
            ("aria2.tellStatus", vec![json!(id), json!(["bitfield", "numPieces"])]),
            ("aria2.getServers", vec![json!(id)]),
        ])
        .await
    else {
        return details;
    };
    let mut answers = answers.into_iter();
    if let Some(Ok(status)) = answers.next() {
        let pieces = number(&status, "numPieces").unwrap_or(0) as usize;
        let bitfield = status["bitfield"].as_str().unwrap_or("");
        details.segments = segment_map(bitfield, pieces, cells);
    }
    if let Some(Ok(files)) = answers.next() {
        for server in into_list(files).iter().flat_map(|file| into_list(file["servers"].clone())) {
            let uri = server["currentUri"].as_str().or(server["uri"].as_str()).unwrap_or("");
            let host = uri.split_once("://").map_or(uri, |(_, rest)| rest);
            details.connections.push(ConnectionInfo {
                host: host.split(['/', '?']).next().unwrap_or("").to_string(),
                speed: number(&server, "downloadSpeed").unwrap_or(0),
            });
        }
    }
    details
}

/// aria2's bitfield (hex, first piece in the high bit) as the share done
/// of each of `cells` equal parts.
fn segment_map(bitfield: &str, pieces: usize, cells: usize) -> Vec<u8> {
    if pieces == 0 || cells == 0 || bitfield.is_empty() {
        return Vec::new();
    }
    let bits: Vec<bool> = bitfield
        .chars()
        .filter_map(|c| c.to_digit(16))
        .flat_map(|digit| (0..4).rev().map(move |shift| digit >> shift & 1 == 1))
        .take(pieces)
        .collect();
    let cells = cells.min(pieces);
    (0..cells)
        .map(|cell| {
            let (start, end) = (cell * pieces / cells, (cell + 1) * pieces / cells);
            let done = bits[start..end.min(bits.len())].iter().filter(|done| **done).count();
            (done * 100 / (end - start).max(1)) as u8
        })
        .collect()
}

pub async fn add_urls(
    shared: &Shared,
    urls: Vec<String>,
    start_paused: bool,
) -> Result<(), String> {
    let downloads = with_ids(urls.iter().map(|url| NewDownload::url(url)).collect())?;
    add(shared, downloads, start_paused).await
}

/// Gives each download its engine ID up front, so callers can follow it
/// before the engine has it. Downloads without an address are dropped.
pub fn with_ids(downloads: Vec<NewDownload>) -> Result<Vec<(String, NewDownload)>, String> {
    downloads
        .into_iter()
        .filter(|download| !download.url.trim().is_empty())
        .map(|download| Ok((random_hex(8)?, download)))
        .collect()
}

/// The row and engine options of a new download.
fn prepare(
    settings: &Settings,
    id: String,
    download: &NewDownload,
    start_paused: bool,
    added_at: i64,
) -> (Task, Value) {
    let mut task = Task::new(id, download.url.trim().to_string(), String::new(), added_at);
    task.row.name = download.planned_name();
    (task.row.category, task.row.dir) = download.place(settings, &task.row.name);
    task.page = download.referer.clone().filter(|page| page.starts_with("http"));
    if start_paused {
        task.row.status = TaskStatus::Paused;
    }
    let mut options = serde_json::Map::new();
    options.insert("gid".into(), json!(task.row.id));
    options.insert("dir".into(), json!(task.row.dir));
    options.insert("pause".into(), json!(start_paused.to_string()));
    download.engine_options(&mut options);
    // The user's own headers come first; the site's may repeat a name.
    let custom = settings.header_lines();
    if !custom.is_empty() {
        let own: Vec<Value> = options.remove("header").map(into_list).unwrap_or_default();
        let all: Vec<Value> = custom.into_iter().map(Value::from).chain(own).collect();
        options.insert("header".into(), Value::Array(all));
    }
    (task, Value::Object(options))
}

/// Starts a captured download before the app takes it, out of the list, so
/// the server is asked only once: its first answer decides whether the app
/// takes the download, and the same request carries on as the download.
pub async fn start_held(shared: &Shared, id: String, download: &NewDownload) -> Result<Task, String> {
    let rpc = shared.wait_rpc().await?;
    let settings = shared.settings.lock().unwrap().clone();
    let added_at = shared.tasks.lock().unwrap().next_added_at(now_ms());
    let (task, options) = prepare(&settings, id.clone(), download, false, added_at);
    shared.tasks.lock().unwrap().held.insert(id.clone());
    let added = rpc
        .call("aria2.addUri", vec![json!([download.url.trim()]), options])
        .await;
    if let Err(error) = added {
        shared.tasks.lock().unwrap().held.remove(&id);
        return Err(error);
    }
    Ok(task)
}

/// Waits for the server's first answer to a held download: the reason when
/// it refused, or Ok once data comes, or once `limit` has passed.
pub async fn first_answer(shared: &Shared, id: &str, limit: Duration) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let started = Instant::now();
    while started.elapsed() < limit {
        let status = rpc
            .call(
                "aria2.tellStatus",
                vec![json!(id), json!(["status", "totalLength", "completedLength", "errorMessage"])],
            )
            .await?;
        let data = number(&status, "totalLength").unwrap_or(0) > 0
            || number(&status, "completedLength").unwrap_or(0) > 0;
        match status["status"].as_str() {
            Some("error") => {
                let message = status["errorMessage"].as_str().unwrap_or("The download failed");
                return Err(friendly_error(message));
            }
            Some("complete" | "removed") => return Ok(()),
            _ if data => return Ok(()),
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    Ok(())
}

/// Puts a held download in the list: the app has taken it.
pub async fn release(shared: &Shared, task: Task) {
    let id = task.row.id.clone();
    insert(shared, std::slice::from_ref(&task));
    {
        let mut table = shared.tasks.lock().unwrap();
        table.held.remove(&id);
        table.mark_dirty(id);
    }
    if let Some(rpc) = shared.current_rpc() {
        let _ = rpc.call("aria2.saveSession", vec![]).await;
    }
    shared.wake.notify_one();
}

/// Drops a held download the server refused. A refused request wrote no file.
pub async fn discard(shared: &Shared, id: &str) {
    if let Some(rpc) = shared.current_rpc() {
        let _ = rpc.call("aria2.forceRemove", vec![json!(id)]).await;
        let _ = rpc.call("aria2.removeDownloadResult", vec![json!(id)]).await;
    }
    shared.tasks.lock().unwrap().held.remove(id);
}

/// Saves a download under another folder or name (None keeps the current
/// one). It moves there when it completes, or now if it already has.
pub fn set_destination(shared: &Shared, id: &str, dir: Option<String>, name: Option<String>) {
    let settings = shared.settings.lock().unwrap().clone();
    let task = {
        let mut table = shared.tasks.lock().unwrap();
        let Some(task) = table.tasks.get_mut(id) else {
            return;
        };
        task.move_dir = dir.map(|dir| dir.trim().to_string()).filter(|dir| !dir.is_empty());
        task.move_name = name.map(|name| name.trim().to_string()).filter(|name| !name.is_empty());
        if let Some(name) = &task.move_name {
            task.row.name = name.clone();
            task.row.category = settings.classify(name);
        }
        task.clone()
    };
    if task.row.status == TaskStatus::Complete {
        move_finished(shared, id);
        return;
    }
    if let Err(error) = shared.db.lock().unwrap().save(std::slice::from_ref(&task)) {
        shared.listener.error(format!("Could not save download history: {error}"));
    }
    shared.listener.rows_changed(vec![task.row]);
}

/// Moves a finished download to the folder and name chosen for it.
fn move_finished(shared: &Shared, id: &str) {
    let Some(mut task) = shared.tasks.lock().unwrap().tasks.get(id).cloned() else {
        return;
    };
    let dir = task.move_dir.take();
    let name = task.move_name.take();
    if let Some(from) = task.row.path.clone().or_else(|| task.files.first().cloned()) {
        let from = PathBuf::from(from);
        let to_dir = dir.map(PathBuf::from).or_else(|| from.parent().map(Path::to_path_buf)).unwrap_or_default();
        let to_name = name.unwrap_or_else(|| base_name(&from.display().to_string()).to_string());
        let to = to_dir.join(&to_name);
        if to != from {
            match move_file(&from, &to) {
                Ok(moved) => {
                    let path = moved.display().to_string();
                    task.row.name = base_name(&path).to_string();
                    task.row.dir = to_dir.display().to_string();
                    task.row.path = Some(path.clone());
                    task.files = vec![path];
                }
                Err(error) => shared.listener.error(format!(
                    "Could not move {} to {}: {error}",
                    from.display(),
                    to.display()
                )),
            }
        }
    }
    shared.tasks.lock().unwrap().tasks.insert(id.to_string(), task.clone());
    if let Err(error) = shared.db.lock().unwrap().save(std::slice::from_ref(&task)) {
        shared.listener.error(format!("Could not save download history: {error}"));
    }
    shared.listener.rows_changed(vec![task.row]);
}

/// Renames, or copies to another disk, never over a file already there.
/// Returns where the file ended up.
fn move_file(from: &Path, to: &Path) -> std::io::Result<PathBuf> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let to = free_path(to);
    match std::fs::rename(from, &to) {
        Ok(()) => Ok(to),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(from, &to)?;
            std::fs::remove_file(from)?;
            Ok(to)
        }
        Err(error) => Err(error),
    }
}

/// `to`, or "name (2).ext" and so on when it is taken.
fn free_path(to: &Path) -> PathBuf {
    if !to.exists() {
        return to.to_path_buf();
    }
    let stem = to.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = to.extension().map(|ext| format!(".{}", ext.to_string_lossy())).unwrap_or_default();
    (2..)
        .map(|n| to.with_file_name(format!("{stem} ({n}){extension}")))
        .find(|candidate| !candidate.exists())
        .unwrap()
}

/// The engine's "HTTP 403" in words.
pub fn friendly_error(message: &str) -> String {
    if message.starts_with("Another task is writing") {
        return "Another download is saving to the same file".into();
    }
    if message.starts_with("Requested media track or language is unavailable") {
        return "That quality is no longer offered. Download it again from the video menu".into();
    }
    // The engine passes on curl's wording, such as "Failed to connect to
    // example.com:443 after 3 ms: Could not connect to server".
    if let Some(rest) = message.strip_prefix("Failed to connect to ") {
        let server = rest.split(" after ").next().unwrap_or(rest).trim();
        return format!("Could not connect to {server}");
    }
    if message.starts_with("Could not resolve") {
        let server = message.rsplit(": ").next().filter(|host| *host != message && !host.contains(' '));
        return match server {
            Some(host) => format!("Could not find the server {host}"),
            None => "Could not find the server".into(),
        };
    }
    if message.starts_with("Operation timed out") {
        return "The server stopped responding".into();
    }
    let status = message
        .strip_prefix("HTTP ")
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse::<u16>().ok());
    match status {
        Some(401) => "The server asks for a login (HTTP 401)".into(),
        Some(403) => "The server refused the download (HTTP 403)".into(),
        Some(code @ (404 | 410)) => format!("The file is not on the server (HTTP {code})"),
        _ => message.to_string(),
    }
}

/// Starts downloads under their IDs.
pub async fn add(
    shared: &Shared,
    downloads: Vec<(String, NewDownload)>,
    start_paused: bool,
) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let settings = shared.settings.lock().unwrap().clone();
    let start = shared.tasks.lock().unwrap().next_added_at(now_ms());
    let mut new_tasks = Vec::new();
    let mut calls = Vec::new();
    // Links added by hand look like the browser, unless the settings name a user agent.
    let agent = settings.user_agent.trim().is_empty().then(|| crate::browser::user_agent(shared));
    for (index, (id, mut download)) in downloads.into_iter().enumerate() {
        let has_agent = download.headers.iter().any(|header| header.to_ascii_lowercase().starts_with("user-agent:"));
        if download.user_agent.is_none() && !has_agent {
            download.user_agent = agent.clone();
        }
        // Offset by the index so a pasted list keeps its order.
        let (task, options) = prepare(&settings, id.clone(), &download, start_paused, start + index as i64);
        calls.push((id, json!([download.url.trim()]), options));
        new_tasks.push(task);
    }
    if new_tasks.is_empty() {
        return Ok(());
    }
    // Show the rows right away; the engine fills in names and sizes.
    insert(shared, &new_tasks);

    let mut failed: Vec<(String, String)> = Vec::new();
    for batch in calls.chunks(ADD_BATCH) {
        let answers = rpc
            .multicall(
                batch
                    .iter()
                    .map(|(_, uris, options)| ("aria2.addUri", vec![uris.clone(), options.clone()]))
                    .collect(),
            )
            .await;
        match answers {
            Ok(answers) => failed.extend(
                batch
                    .iter()
                    .zip(answers)
                    .filter_map(|((id, _, _), answer)| {
                        answer.err().map(|error| (id.clone(), error))
                    }),
            ),
            Err(error) => failed.extend(batch.iter().map(|(id, _, _)| (id.clone(), error.clone()))),
        }
    }
    let _ = rpc.call("aria2.saveSession", vec![]).await;
    shared.wake.notify_one();

    if failed.is_empty() {
        return Ok(());
    }
    let failed_ids: Vec<String> = failed.iter().map(|(id, _)| id.clone()).collect();
    forget(shared, &failed_ids);
    Err(match failed.len() {
        1 => format!("Could not add the download: {}", failed[0].1),
        count => format!("Could not add {count} downloads: {}", failed[0].1),
    })
}

pub async fn pause(shared: &Shared, ids: Vec<String>) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let targets = select(shared, &ids, |task| {
        matches!(task.row.status, TaskStatus::Downloading | TaskStatus::Queued) || waiting_to_retry(task)
    });
    let calls: Vec<_> = targets
        .iter()
        .filter(|task| !waiting_to_retry(task))
        .map(|task| ("aria2.forcePause", vec![json!(task.row.id)]))
        .collect();
    if !calls.is_empty() {
        rpc.multicall(calls).await?;
    }
    let mut errors = Vec::new();
    for task in targets.iter().filter(|task| waiting_to_retry(task)) {
        if let Err(error) = pause_waiting(shared, &rpc, task).await {
            errors.push(format!("{}: {error}", task.row.name));
        }
    }
    finish_action(shared, &rpc, &targets).await;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("Could not pause {}", errors.join("; ")))
    }
}

fn waiting_to_retry(task: &Task) -> bool {
    task.row.status == TaskStatus::Failed && task.row.retry_at.is_some()
}

/// Stops a download waiting to try again: it goes back to the engine
/// paused, keeping what is on disk, and resumes like any paused download.
async fn pause_waiting(shared: &Shared, rpc: &Rpc, task: &Task) -> Result<(), String> {
    let mut options = readd_options(rpc, task).await;
    options.insert("pause".into(), json!("true"));
    let progress = saved_progress(shared, &task.row.id);
    restart(shared, rpc, task, &task.row.url, options, progress).await?;
    let row = {
        let mut table = shared.tasks.lock().unwrap();
        let Some(task) = table.tasks.get_mut(&task.row.id) else {
            return Ok(());
        };
        task.row.status = TaskStatus::Paused;
        task.row.retry_at = None;
        task.row.error = None;
        task.row.clone()
    };
    shared.listener.rows_changed(vec![row]);
    Ok(())
}

pub async fn resume(shared: &Shared, ids: Vec<String>) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let targets = select(shared, &ids, |task| {
        matches!(task.row.status, TaskStatus::Paused | TaskStatus::Failed)
    });
    {
        // Started again by the user: a refusal may ask the browser once more.
        let mut table = shared.tasks.lock().unwrap();
        for task in &targets {
            if let Some(task) = table.tasks.get_mut(&task.row.id) {
                task.cookies_refreshed = false;
                task.retries = 0;
            }
        }
    }
    let mut errors = Vec::new();
    for task in &targets {
        let id = json!(task.row.id);
        let result = if task.row.status == TaskStatus::Paused {
            rpc.call("aria2.unpause", vec![id]).await
        } else {
            let in_engine = rpc
                .call("aria2.tellStatus", vec![id.clone(), json!(["status"])])
                .await
                .is_ok();
            if task.media && in_engine {
                // Keeps the media recovery data that a remove would discard.
                rpc.call("aria2.retryMedia", vec![id]).await
            } else {
                let options = readd_options(&rpc, task).await;
                let progress = saved_progress(shared, &task.row.id);
                restart(shared, &rpc, task, &task.row.url, options, progress).await
            }
        };
        if let Err(error) = result {
            errors.push(format!("{}: {error}", task.row.name));
        }
    }
    finish_action(shared, &rpc, &targets).await;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("Could not resume {}", errors.join("; ")))
    }
}

/// Options of a download the engine still holds that a restart keeps: what
/// makes the server answer as it did for the browser, and the stream choices.
const KEPT_OPTIONS: &[&str] = &[
    "filename-hint",
    "filename-hint-source",
    "header",
    "all-proxy",
    "no-proxy",
    "http-user",
    "http-passwd",
    "referer",
    "user-agent",
    "media",
    "media-video",
    "media-audio",
    "media-format",
    "media-input",
];

/// Options that restart a failed or lost download in place: same GID, same
/// file, continuing from the bytes already on disk, with the request headers
/// and name hints it had (a download the engine lost after a restart has
/// none left). Before any bytes, the name is still the server's to give.
async fn readd_options(rpc: &Rpc, task: &Task) -> serde_json::Map<String, Value> {
    let mut options = serde_json::Map::new();
    if let Ok(Value::Object(old)) = rpc.call("aria2.getOption", vec![json!(task.row.id)]).await {
        for key in KEPT_OPTIONS {
            if let Some(value) = old.get(*key) {
                options.insert(key.to_string(), value.clone());
            }
        }
    }
    // getOption joins headers with newlines; addUri takes a list.
    if let Some(Value::String(lines)) = options.get("header") {
        let lines: Vec<&str> = lines.lines().filter(|line| !line.trim().is_empty()).collect();
        options.insert("header".into(), json!(lines));
    }
    options.insert("gid".into(), json!(task.row.id));
    options.insert("dir".into(), json!(task.row.dir));
    options.insert("continue".into(), json!("true"));
    if task.files.len() == 1 && task.row.done_bytes > 0 {
        options.insert("out".into(), json!(base_name(&task.files[0])));
    }
    options
}

/// Starts a stopped download again under its own GID, from `url`, keeping
/// every part already on disk.
///
/// The engine sizes the file in full when it starts and keeps which ranges
/// are done in its resume record, which dropping the old task deletes. So
/// the record taken before is put back, and the new task resumes from it.
/// Without a record, a full-length file would pass for done, so it starts
/// over instead.
async fn restart(
    shared: &Shared,
    rpc: &Rpc,
    task: &Task,
    url: &str,
    options: serde_json::Map<String, Value>,
    progress: Option<Progress>,
) -> Result<Value, String> {
    let _ = rpc.call("aria2.removeDownloadResult", vec![json!(task.row.id)]).await;
    let kept = progress.is_some_and(|mut progress| {
        if url != task.row.url {
            // The old address's validators say nothing about the new one; its
            // first answer sets them. The length is still compared.
            progress.etag.clear();
            progress.last_modified.clear();
        }
        progress.put_back(shared, &task.row.id, url).is_ok()
    });
    if !kept {
        for file in &task.files {
            if std::fs::metadata(file).is_ok_and(|meta| meta.len() > 0) {
                let cut = std::fs::OpenOptions::new().write(true).open(file).and_then(|file| file.set_len(0));
                if let Err(error) = cut {
                    return Err(format!("Could not prepare {file} to start over: {error}"));
                }
            }
        }
    }
    rpc.call("aria2.addUri", vec![json!([url]), Value::Object(options)])
        .await
}

/// A download's resume record in the engine's state: its file, the server's
/// validators, and the byte ranges on disk ("0-98304,1585152-1683456").
struct Progress {
    path: String,
    etag: String,
    last_modified: String,
    total_length: i64,
    completed_length: i64,
    completed_ranges: String,
}

fn state_db(shared: &Shared) -> std::path::PathBuf {
    crate::engine::engine_dir(shared).join("state/stream/state.db")
}

/// The engine's resume record of a download, when it has one.
fn saved_progress(shared: &Shared, id: &str) -> Option<Progress> {
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = rusqlite::Connection::open_with_flags(state_db(shared), flags).ok()?;
    db.query_row(
        "SELECT path, etag, last_modified, total_length, completed_length, completed_ranges
         FROM downloads WHERE gid = ?1",
        [id],
        |row| {
            Ok(Progress {
                path: row.get(0)?,
                etag: row.get(1)?,
                last_modified: row.get(2)?,
                total_length: row.get(3)?,
                completed_length: row.get(4)?,
                completed_ranges: row.get(5)?,
            })
        },
    )
    .ok()
}

impl Progress {
    fn put_back(&self, shared: &Shared, id: &str, url: &str) -> rusqlite::Result<()> {
        let db = rusqlite::Connection::open_with_flags(state_db(shared), rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        db.busy_timeout(Duration::from_secs(2))?;
        db.execute(
            "INSERT OR REPLACE INTO downloads
             (gid, uri, path, etag, last_modified, total_length, completed_length, completed_ranges)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id,
                url,
                self.path,
                self.etag,
                self.last_modified,
                self.total_length,
                self.completed_length,
                self.completed_ranges
            ],
        )?;
        Ok(())
    }
}

/// Engine failures that may pass on their own: a timeout, a dropped or
/// refused connection, a server error, a name that did not resolve, or a
/// busy server (503 or 429).
fn is_temporary(status: &Value) -> bool {
    matches!(status["errorCode"].as_str(), Some("2" | "5" | "6" | "19" | "29"))
}

/// Sets when a download that failed partway is tried again, or leaves it
/// failed after MAX_RETRIES tries that got no further.
fn schedule_retry(shared: &Shared, id: &str) -> Option<(i64, Duration)> {
    let (row, at, wait) = {
        let mut table = shared.tasks.lock().unwrap();
        let task = table.tasks.get_mut(id)?;
        if task.row.done_bytes > task.retried_from {
            task.retries = 0;
            task.retried_from = task.row.done_bytes;
        }
        if task.retries >= MAX_RETRIES {
            return None;
        }
        let wait = RETRY_WAITS[(task.retries as usize).min(RETRY_WAITS.len() - 1)];
        task.retries += 1;
        let at = now_ms() + wait.as_millis() as i64;
        task.row.retry_at = Some(at);
        (task.row.clone(), at, wait)
    };
    shared.listener.rows_changed(vec![row]);
    Some((at, wait))
}

/// Tries a download again when its time comes, keeping what is on disk,
/// unless the user has started or removed it meanwhile.
async fn retry_later(shared: Arc<Shared>, id: String, at: i64, wait: Duration) {
    let mut shutdown = shared.shutdown.subscribe();
    tokio::select! {
        _ = tokio::time::sleep(wait) => {}
        _ = crate::stopped(&mut shutdown) => return,
    }
    let due = |task: &Task| task.row.status == TaskStatus::Failed && task.row.retry_at == Some(at);
    let Some(task) = select(&shared, std::slice::from_ref(&id), due).pop() else {
        return;
    };
    let Ok(rpc) = shared.wait_rpc().await else {
        return;
    };
    let options = readd_options(&rpc, &task).await;
    let progress = saved_progress(&shared, &id);
    let restarted = restart(&shared, &rpc, &task, &task.row.url, options, progress).await.is_ok();
    let row = {
        let mut table = shared.tasks.lock().unwrap();
        let Some(task) = table.tasks.get_mut(&id).filter(|task| due(task)) else {
            return;
        };
        task.row.retry_at = None;
        // Queued in the engine again, so a quick new failure counts as one.
        if restarted {
            task.row.status = TaskStatus::Queued;
            task.row.error = None;
        }
        task.row.clone()
    };
    shared.listener.rows_changed(vec![row]);
    if restarted {
        finish_action(&shared, &rpc, std::slice::from_ref(&task)).await;
    }
}

/// After a 401 or 403: fresh browser cookies first, once until the user
/// starts the download again; then, for a 401, a user name and password.
async fn after_refusal(shared: Arc<Shared>, id: String, status: u16) {
    let Some(task) = select(&shared, std::slice::from_ref(&id), |task| task.row.status == TaskStatus::Failed).pop()
    else {
        return;
    };
    let first = {
        let mut table = shared.tasks.lock().unwrap();
        table.tasks.get_mut(&id).is_some_and(|task| !std::mem::replace(&mut task.cookies_refreshed, true))
    };
    if first && retry_with_fresh_cookies(&shared, &task).await {
        return;
    }
    if status == 401 {
        shared.listener.login_needed(id, task.row.url);
    }
}

/// Asks the browser which cookies it would send now and, when they differ
/// from the download's, restarts it with them. IDM does the same ("Change
/// cookies, try again"): a session can expire or rotate between the click
/// and the download, or while it waits paused. True when it restarted.
async fn retry_with_fresh_cookies(shared: &Shared, task: &Task) -> bool {
    let Some(cookie) = crate::browser::fresh_cookies(shared, &task.row.url).await else {
        return false;
    };
    let Some(rpc) = shared.current_rpc() else {
        return false;
    };
    let mut options = readd_options(&rpc, task).await;
    let mut headers: Vec<Value> = options.remove("header").map(into_list).unwrap_or_default();
    let is_cookie = |line: &Value| line.as_str().is_some_and(|line| line.to_ascii_lowercase().starts_with("cookie:"));
    let fresh = format!("Cookie: {cookie}");
    if headers.iter().any(|line| line.as_str() == Some(fresh.as_str())) {
        return false; // The browser has nothing newer.
    }
    headers.retain(|line| !is_cookie(line));
    headers.push(json!(fresh));
    options.insert("header".into(), Value::Array(headers));
    let progress = saved_progress(shared, &task.row.id);
    let restarted = restart(shared, &rpc, task, &task.row.url, options, progress).await.is_ok();
    if restarted {
        finish_action(shared, &rpc, std::slice::from_ref(task)).await;
    }
    restarted
}

/// Starts waiting for a new address for download `id`: the next download a
/// browser hands over goes to the app as that address (`new_address`).
/// Returns the page the download came from, to open in the browser.
pub fn wait_for_address(shared: &Shared, id: &str) -> Result<Option<String>, String> {
    let page = {
        let table = shared.tasks.lock().unwrap();
        let task = table.tasks.get(id).ok_or("The download is gone")?;
        if task.media {
            return Err("A stream gets a new address from its page's video menu".into());
        }
        if task.row.status == TaskStatus::Complete {
            return Err("The download is already complete".into());
        }
        task.page.clone()
    };
    *shared.waiting_address.lock().unwrap() = Some(id.to_string());
    Ok(page)
}

/// Continues download `id` from `download`'s address, with the cookies and
/// headers the browser sent for it, from the part already on disk.
pub async fn refresh_address(shared: &Shared, id: &str, download: &NewDownload) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let settings = shared.settings.lock().unwrap().clone();
    let Some(task) = select(shared, &[id.to_string()], |task| task.row.status != TaskStatus::Complete).pop() else {
        return Ok(());
    };
    // Out of the poller's sight while it stops and comes back: a removed task
    // would otherwise leave the list.
    shared.tasks.lock().unwrap().removing.insert(id.to_string());
    let result = async {
        // Read before a running download is removed: removing deletes it.
        let progress = saved_progress(shared, id);
        if matches!(task.row.status, TaskStatus::Downloading | TaskStatus::Queued | TaskStatus::Paused) {
            let _ = rpc.call("aria2.forceRemove", vec![json!(id)]).await;
            for _ in 0..60 {
                let status = rpc.call("aria2.tellStatus", vec![json!(id), json!(["status"])]).await;
                if !matches!(status.as_ref().ok().and_then(|status| status["status"].as_str()), Some("active" | "waiting" | "paused")) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        let mut options = readd_options(&rpc, &task).await;
        for key in ["header", "referer", "user-agent", "all-proxy", "no-proxy"] {
            options.remove(key);
        }
        let mut fresh = serde_json::Map::new();
        download.request_options(&mut fresh);
        let custom: Vec<Value> = settings.header_lines().into_iter().map(Value::from).collect();
        let own: Vec<Value> = fresh.remove("header").map(into_list).unwrap_or_default();
        if !custom.is_empty() || !own.is_empty() {
            fresh.insert("header".into(), Value::Array(custom.into_iter().chain(own).collect()));
        }
        options.extend(fresh);
        restart(shared, &rpc, &task, download.url.trim(), options, progress).await
    }
    .await;
    let updated = {
        let mut table = shared.tasks.lock().unwrap();
        table.removing.remove(id);
        table.tasks.get_mut(id).map(|task| {
            if result.is_ok() {
                task.row.url = download.url.trim().to_string();
                task.page = download.referer.clone().filter(|page| page.starts_with("http")).or(task.page.take());
                task.row.error = None;
                task.cookies_refreshed = false;
            }
            task.clone()
        })
    };
    if let Some(task) = updated {
        if let Err(error) = shared.db.lock().unwrap().save(std::slice::from_ref(&task)) {
            shared.listener.error(format!("Could not save download history: {error}"));
        }
        shared.listener.rows_changed(vec![task.row.clone()]);
        finish_action(shared, &rpc, std::slice::from_ref(&task)).await;
    }
    result.map(|_| ())
}

/// Restarts a refused download with a login. The engine sends it as HTTP
/// Basic, or Digest when the server asks for that.
pub async fn set_login(shared: &Shared, id: &str, user: &str, password: &str) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let Some(task) = select(shared, &[id.to_string()], |task| task.row.status == TaskStatus::Failed).pop() else {
        return Ok(());
    };
    let mut options = readd_options(&rpc, &task).await;
    options.insert("http-user".into(), json!(user));
    options.insert("http-passwd".into(), json!(password));
    let progress = saved_progress(shared, &task.row.id);
    restart(shared, &rpc, &task, &task.row.url, options, progress).await?;
    finish_action(shared, &rpc, std::slice::from_ref(&task)).await;
    Ok(())
}

pub async fn pause_all(shared: &Shared) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    rpc.call("aria2.forcePauseAll", vec![]).await?;
    let targets = select(shared, &[], |task| {
        matches!(task.row.status, TaskStatus::Downloading | TaskStatus::Queued) || waiting_to_retry(task)
    });
    for task in targets.iter().filter(|task| waiting_to_retry(task)) {
        let _ = pause_waiting(shared, &rpc, task).await;
    }
    finish_action(shared, &rpc, &targets).await;
    Ok(())
}

pub async fn resume_all(shared: &Shared) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    rpc.call("aria2.unpauseAll", vec![]).await?;
    let targets = select(shared, &[], |task| task.row.status == TaskStatus::Paused);
    finish_action(shared, &rpc, &targets).await;
    Ok(())
}

pub async fn remove(shared: &Shared, ids: Vec<String>, delete_files: bool) -> Result<(), String> {
    let mut targets = select(shared, &ids, |_| true);
    if targets.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = targets.iter().map(|task| task.row.id.clone()).collect();
    shared
        .tasks
        .lock()
        .unwrap()
        .removing
        .extend(ids.iter().cloned());

    if let Some(rpc) = shared.current_rpc() {
        let running: Vec<&Task> = targets
            .iter()
            .filter(|task| is_unfinished(task.row.status))
            .collect();
        let _ = rpc
            .multicall(
                running
                    .iter()
                    .map(|task| ("aria2.forceRemove", vec![json!(task.row.id)]))
                    .collect(),
            )
            .await;
        // A removed task must stop before its result can be dropped and its
        // files deleted. Its file paths come along: a download removed just
        // after it started may not have reported them yet.
        let mut files: HashMap<String, Vec<String>> = HashMap::new();
        for _ in 0..60 {
            let answers = rpc
                .multicall(
                    running
                        .iter()
                        .map(|task| {
                            (
                                "aria2.tellStatus",
                                vec![json!(task.row.id), json!(["status", "files"])],
                            )
                        })
                        .collect(),
                )
                .await
                .unwrap_or_default();
            for (task, answer) in running.iter().zip(&answers) {
                if let Ok(status) = answer {
                    let paths: Vec<String> = into_list(status["files"].clone())
                        .iter()
                        .filter_map(|file| file["path"].as_str())
                        .filter(|path| !path.is_empty())
                        .map(native_path)
                        .collect();
                    if !paths.is_empty() {
                        files.insert(task.row.id.clone(), paths);
                    }
                }
            }
            let stopped = answers.iter().all(|answer| match answer {
                Ok(status) => matches!(
                    status["status"].as_str(),
                    Some("removed" | "complete" | "error")
                ),
                Err(_) => true,
            });
            if stopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let _ = rpc
            .multicall(
                ids.iter()
                    .map(|id| ("aria2.removeDownloadResult", vec![json!(id)]))
                    .collect(),
            )
            .await;
        let _ = rpc.call("aria2.saveSession", vec![]).await;
        for task in &mut targets {
            if task.files.is_empty()
                && let Some(paths) = files.remove(&task.row.id)
            {
                task.files = paths;
            }
        }
    }

    if delete_files {
        for task in &targets {
            let controls: Vec<String> = task.files.iter().map(|file| format!("{file}.aria2")).collect();
            for file in task.files.iter().chain(&controls) {
                match std::fs::remove_file(file) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => shared
                        .listener
                        .error(format!("Could not delete {file}: {error}")),
                }
            }
        }
    }
    forget(shared, &ids);
    let mut table = shared.tasks.lock().unwrap();
    for id in &ids {
        table.removing.remove(id);
    }
    Ok(())
}

/// Writes current progress of every task, so the list is right before the
/// engine is back after a restart.
pub fn save_all(shared: &Shared) {
    let tasks: Vec<Task> = shared
        .tasks
        .lock()
        .unwrap()
        .tasks
        .values()
        .cloned()
        .collect();
    if let Err(error) = shared.db.lock().unwrap().save(&tasks) {
        shared
            .listener
            .error(format!("Could not save download history: {error}"));
    }
}

fn insert(shared: &Shared, tasks: &[Task]) {
    {
        let mut table = shared.tasks.lock().unwrap();
        for task in tasks {
            table.tasks.insert(task.row.id.clone(), task.clone());
        }
    }
    if let Err(error) = shared.db.lock().unwrap().save(tasks) {
        shared
            .listener
            .error(format!("Could not save download history: {error}"));
    }
    shared
        .listener
        .rows_changed(tasks.iter().map(|task| task.row.clone()).collect());
}

fn forget(shared: &Shared, ids: &[String]) {
    {
        let mut table = shared.tasks.lock().unwrap();
        for id in ids {
            table.tasks.remove(id);
        }
    }
    if let Err(error) = shared.db.lock().unwrap().delete(ids) {
        shared
            .listener
            .error(format!("Could not save download history: {error}"));
    }
    shared.listener.rows_removed(ids.to_vec());
}

/// Tasks with the given ids (or all tasks when `ids` is empty) that match.
fn select(shared: &Shared, ids: &[String], keep: impl Fn(&Task) -> bool) -> Vec<Task> {
    let table = shared.tasks.lock().unwrap();
    let chosen: Box<dyn Iterator<Item = &Task>> = if ids.is_empty() {
        Box::new(table.tasks.values())
    } else {
        Box::new(ids.iter().filter_map(|id| table.tasks.get(id)))
    };
    chosen.filter(|task| keep(task)).cloned().collect()
}

async fn finish_action(shared: &Shared, rpc: &Rpc, targets: &[Task]) {
    {
        let mut table = shared.tasks.lock().unwrap();
        for task in targets {
            table.mark_dirty(task.row.id.clone());
        }
    }
    let _ = rpc.call("aria2.saveSession", vec![]).await;
    shared.wake.notify_one();
}

fn gid(status: &Value) -> Option<&str> {
    status["gid"].as_str()
}

fn into_list(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        _ => Vec::new(),
    }
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value[key].as_str().and_then(|text| text.parse().ok())
}

fn parse_stats(value: &Value) -> GlobalStats {
    GlobalStats {
        download_speed: number(value, "downloadSpeed").unwrap_or(0),
        upload_speed: number(value, "uploadSpeed").unwrap_or(0),
        active: number(value, "numActive").unwrap_or(0) as u32,
        waiting: number(value, "numWaiting").unwrap_or(0) as u32,
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

/// aria2 reports Windows paths with forward slashes.
fn native_path(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// A display name from the last URL path segment, percent-decoded once.
pub fn name_from_url(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let path = without_query
        .split_once("://")
        .map_or(without_query, |(_, rest)| rest);
    let (host, path) = path.split_once('/').unwrap_or((path, ""));
    let segment = path.rsplit('/').find(|part| !part.is_empty()).unwrap_or("");
    let decoded = percent_decode(segment);
    if !decoded.trim().is_empty() {
        decoded
    } else if !host.is_empty() {
        host.to_string()
    } else {
        "download".into()
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |offset: usize| {
            bytes
                .get(index + offset)
                .and_then(|byte| (*byte as char).to_digit(16))
        };
        match (bytes[index], hex(1), hex(2)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                index += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_errors_read_as_sentences() {
        assert_eq!(
            friendly_error("Failed to connect to 127.0.0.1:8783 after 0 ms: Could not connect to server"),
            "Could not connect to 127.0.0.1:8783"
        );
        assert_eq!(friendly_error("Could not resolve host: files.example"), "Could not find the server files.example");
        assert_eq!(
            friendly_error("Operation timed out after 30001 milliseconds with 0 bytes received"),
            "The server stopped responding"
        );
        assert_eq!(friendly_error("HTTP 404 Not Found"), "The file is not on the server (HTTP 404)");
        // Already friendly, as when read back from the history.
        assert_eq!(friendly_error("Could not connect to 127.0.0.1:8783"), "Could not connect to 127.0.0.1:8783");
    }

    #[test]
    fn names_come_from_the_last_path_segment() {
        assert_eq!(
            name_from_url("https://example.com/files/My%20Report.pdf?x=1#top"),
            "My Report.pdf"
        );
        assert_eq!(name_from_url("https://example.com/dir/"), "dir");
        assert_eq!(name_from_url("https://example.com"), "example.com");
        assert_eq!(name_from_url("https://example.com/100%25.txt"), "100%.txt");
        assert_eq!(
            name_from_url("https://example.com/caf%C3%A9.zip"),
            "café.zip"
        );
    }

    fn settings() -> Settings {
        Settings {
            download_dir: "/dl".into(),
            ..Settings::default()
        }
    }

    #[test]
    fn a_typed_name_and_category_are_kept() {
        let settings = settings();
        let typed = NewDownload {
            filename: Some("notes.txt".into()),
            filename_source: Some("user".into()),
            category: Some("Documents".into()),
            ..NewDownload::url("https://example.com/get?id=1")
        };
        assert_eq!(typed.planned_name(), "notes.txt");
        let documents = std::path::Path::new("/dl").join("Documents").display().to_string();
        assert_eq!(typed.place(&settings, "notes.txt"), ("Documents".into(), documents));
        let mut options = serde_json::Map::new();
        typed.engine_options(&mut options);
        assert_eq!(options["filename-hint"], "notes.txt");
        assert_eq!(options["filename-hint-source"], "browser");
    }

    #[test]
    fn the_segment_map_reads_the_first_piece_from_the_high_bit() {
        // 10 pieces: the first four and the ninth are done.
        assert_eq!(segment_map("f080", 10, 5), vec![100, 100, 0, 0, 50]);
        assert_eq!(segment_map("f080", 10, 100).len(), 10);
        assert!(segment_map("", 10, 5).is_empty());
    }

    #[test]
    fn a_stream_is_named_after_its_title_and_container() {
        let settings = settings();
        let stream = NewDownload {
            filename: Some("A Talk".into()),
            filename_source: Some("title".into()),
            media_kind: Some("hls".into()),
            media_format: Some("mkv".into()),
            ..NewDownload::url("https://cdn.example.com/master.m3u8")
        };
        assert_eq!(stream.planned_name(), "A Talk.mkv");
        assert_eq!(stream.place(&settings, "A Talk.mkv").0, "Video");
        // Renamed by the user: the engine still adds the extension itself.
        let renamed = NewDownload {
            filename: Some("Talk 2.mkv".into()),
            filename_source: Some("user".into()),
            ..stream
        };
        assert_eq!(renamed.planned_name(), "Talk 2.mkv");
        let mut options = serde_json::Map::new();
        renamed.engine_options(&mut options);
        assert_eq!(options["filename-hint"], "Talk 2");
        assert_eq!(options["filename-hint-source"], "title");
    }

    #[test]
    fn a_new_batch_starts_after_the_previous_one() {
        let mut table = Table::default();
        assert_eq!(table.next_added_at(1_000), 1_000);
        // A pasted batch of 5,000 links took 1,000 .. 5,999.
        let last = Task::new("b".into(), String::new(), String::new(), 5_999);
        table.tasks.insert("b".into(), last);
        assert_eq!(table.next_added_at(3_000), 6_000);
        assert_eq!(table.next_added_at(9_000), 9_000);
    }

    #[test]
    fn http_status_fills_the_row() {
        let mut task = Task::new("a1".into(), String::new(), String::new(), 5);
        let status = json!({
            "gid": "a1", "status": "active", "totalLength": "1000", "completedLength": "250",
            "downloadSpeed": "50", "connections": "4", "dir": "/dl/Compressed",
            "files": [{ "path": "/dl/Compressed/tool.zip", "uris": [{ "uri": "https://example.com/get?id=7" }] }]
        });
        update_from_engine(&mut task, &status, &settings(), 9);
        let row = &task.row;
        assert_eq!(row.status, TaskStatus::Downloading);
        assert_eq!(row.name, "tool.zip");
        assert_eq!(row.url, "https://example.com/get?id=7");
        let expected_path = if cfg!(windows) {
            "\\dl\\Compressed\\tool.zip"
        } else {
            "/dl/Compressed/tool.zip"
        };
        assert_eq!(row.path.as_deref(), Some(expected_path));
        assert_eq!(row.category, "Compressed");
        assert_eq!(row.progress, 0.25);
        assert_eq!(row.eta_secs, Some(15));
        assert_eq!(row.connections, 4);

        // A tellActive answer has fewer keys; the others keep their values.
        update_from_engine(
            &mut task,
            &json!({ "gid": "a1", "status": "complete", "completedLength": "1000" }),
            &settings(),
            9,
        );
        assert_eq!(task.row.name, "tool.zip");
        assert_eq!(task.row.progress, 1.0);
        assert_eq!(task.row.speed, 0);
        assert_eq!(task.row.eta_secs, None);
        assert_eq!(task.row.finished_at, Some(9));
    }

    #[test]
    fn media_progress_uses_media_time_and_leaves_size_unknown() {
        let mut task = Task::new(
            "m1".into(),
            "https://cdn.example/master.m3u8".into(),
            "/dl".into(),
            1,
        );
        let status = json!({
            "gid": "m1", "status": "active", "totalLength": "0", "completedLength": "0", "downloadSpeed": "900",
            "media": { "state": "downloading", "progress": "0.333333", "downloadedLength": "4194304", "error": "" }
        });
        update_from_engine(&mut task, &status, &settings(), 2);
        assert!(task.media);
        assert_eq!(task.row.progress, 0.333333);
        assert_eq!(task.row.done_bytes, 4_194_304);
        assert_eq!(task.row.total_bytes, 0);
        assert_eq!(task.row.eta_secs, None);
        assert_eq!(task.row.category, "Video");
    }

    #[test]
    fn a_finished_file_moves_without_replacing_one_already_there() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("download.bin");
        std::fs::write(&from, b"new").unwrap();
        let wanted = dir.path().join("sub").join("report.pdf");
        std::fs::create_dir_all(wanted.parent().unwrap()).unwrap();
        std::fs::write(&wanted, b"old").unwrap();

        let moved = move_file(&from, &wanted).unwrap();
        assert_eq!(moved, dir.path().join("sub").join("report (2).pdf"));
        assert_eq!(std::fs::read(&moved).unwrap(), b"new");
        assert_eq!(std::fs::read(&wanted).unwrap(), b"old");
        assert!(!from.exists());
    }

    #[test]
    fn failures_carry_the_engine_message() {
        let mut task = Task::new(
            "e1".into(),
            "https://example.com/a.bin".into(),
            "/dl".into(),
            1,
        );
        update_from_engine(
            &mut task,
            &json!({ "gid": "e1", "status": "error", "errorMessage": "404 Not Found" }),
            &settings(),
            2,
        );
        assert_eq!(task.row.status, TaskStatus::Failed);
        assert_eq!(task.row.error.as_deref(), Some("404 Not Found"));
    }
}
