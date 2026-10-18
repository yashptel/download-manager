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
use crate::{GlobalStats, Shared, TaskRow, TaskStatus};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

const VISIBLE_INTERVAL: Duration = Duration::from_millis(500);
const BACKGROUND_INTERVAL: Duration = Duration::from_secs(2);
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
                added_at,
                finished_at: None,
            },
            files: Vec::new(),
            media: false,
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
                    table
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
    let mut new_stats = None;
    {
        let mut table = shared.tasks.lock().unwrap();
        for status in statuses {
            let Some(id) = gid(status) else { continue };
            if table.removing.contains(id) {
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
            if task.row != before || is_new {
                changed.push(task.row.clone());
                if is_new || task.row.status != before.status || task.row.name != before.name {
                    persist.push(task.clone());
                }
                // Finished results leave the engine; history keeps them.
                if task.row.status == TaskStatus::Complete && before.status != TaskStatus::Complete
                {
                    purge.push(id.to_string());
                }
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
                .map(String::from)
                .or_else(|| row.error.clone())
                .unwrap_or_else(|| "Download failed".into()),
        ),
        _ => None,
    };
    if row.status != TaskStatus::Downloading {
        row.speed = 0;
        row.connections = 0;
    }
    row.eta_secs = match row.status {
        TaskStatus::Downloading
            if !task.media && row.speed > 0 && row.total_bytes > row.done_bytes =>
        {
            Some((row.total_bytes - row.done_bytes) / row.speed)
        }
        _ => None,
    };
    row.category = settings.classify(&row.name);
}

// User actions. Each one changes the engine, then marks the tasks dirty so
// the next tick (requested immediately) reports their new state.

pub async fn add_urls(
    shared: &Shared,
    urls: Vec<String>,
    start_paused: bool,
) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let settings = shared.settings.lock().unwrap().clone();
    let now = now_ms();
    let mut new_tasks = Vec::new();
    let mut calls = Vec::new();
    for (index, url) in urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
        .enumerate()
    {
        let id = random_hex(8)?;
        // Offset by the index so a pasted list keeps its order.
        let mut task = Task::new(
            id.clone(),
            url.to_string(),
            String::new(),
            now + index as i64,
        );
        task.row.category = settings.classify(&task.row.name);
        task.row.dir = settings
            .directory_for(&task.row.category)
            .display()
            .to_string();
        if start_paused {
            task.row.status = TaskStatus::Paused;
        }
        calls.push((
            id,
            json!([url]),
            json!({ "gid": task.row.id, "dir": task.row.dir, "pause": start_paused.to_string() }),
        ));
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
    let ids: Vec<String> = failed.iter().map(|(id, _)| id.clone()).collect();
    forget(shared, &ids);
    Err(match failed.len() {
        1 => format!("Could not add the download: {}", failed[0].1),
        count => format!("Could not add {count} downloads: {}", failed[0].1),
    })
}

pub async fn pause(shared: &Shared, ids: Vec<String>) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let targets = select(shared, &ids, |task| {
        matches!(
            task.row.status,
            TaskStatus::Downloading | TaskStatus::Queued
        )
    });
    let calls = targets
        .iter()
        .map(|task| ("aria2.forcePause", vec![json!(task.row.id)]))
        .collect();
    rpc.multicall(calls).await?;
    finish_action(shared, &rpc, &targets).await;
    Ok(())
}

pub async fn resume(shared: &Shared, ids: Vec<String>) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    let targets = select(shared, &ids, |task| {
        matches!(task.row.status, TaskStatus::Paused | TaskStatus::Failed)
    });
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
                if in_engine {
                    let _ = rpc
                        .call("aria2.removeDownloadResult", vec![id.clone()])
                        .await;
                }
                rpc.call(
                    "aria2.addUri",
                    vec![json!([task.row.url]), readd_options(task)],
                )
                .await
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

/// Options that restart a failed or lost download in place: same GID, same
/// file, continuing from the bytes already on disk.
fn readd_options(task: &Task) -> Value {
    let mut options = json!({ "gid": task.row.id, "dir": task.row.dir, "continue": "true" });
    if task.files.len() == 1 {
        options["out"] = json!(task.row.name);
    }
    options
}

pub async fn pause_all(shared: &Shared) -> Result<(), String> {
    let rpc = shared.wait_rpc().await?;
    rpc.call("aria2.forcePauseAll", vec![]).await?;
    let targets = select(shared, &[], |task| {
        matches!(
            task.row.status,
            TaskStatus::Downloading | TaskStatus::Queued
        )
    });
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
    let targets = select(shared, &ids, |_| true);
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
        // A removed task must stop before its result can be dropped and its files deleted.
        for _ in 0..60 {
            let answers = rpc
                .multicall(
                    running
                        .iter()
                        .map(|task| {
                            (
                                "aria2.tellStatus",
                                vec![json!(task.row.id), json!(["status"])],
                            )
                        })
                        .collect(),
                )
                .await
                .unwrap_or_default();
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
    }

    if delete_files {
        for task in &targets {
            for file in &task.files {
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
