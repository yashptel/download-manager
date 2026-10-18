//! OpenDM core, shared by the macOS and Windows apps through UniFFI.
//!
//! It owns the aria2-next engine process, the task list, download history
//! and settings. UI calls return immediately; results arrive through the
//! `CoreListener` the app passes in, always as changed rows only.

mod browser;
mod db;
mod engine;
mod media;
mod rpc;
mod settings;
mod sites;
mod tasks;

pub use settings::{CategoryRule, Settings};

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, watch};

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskRow {
    /// The aria2 GID; stable across restarts.
    pub id: String,
    pub name: String,
    pub url: String,
    pub dir: String,
    /// Output file (or a torrent's top folder) once the engine has named it.
    pub path: Option<String>,
    pub category: String,
    pub status: TaskStatus,
    /// 0 while unknown.
    pub total_bytes: u64,
    pub done_bytes: u64,
    /// Bytes per second.
    pub speed: u64,
    /// 0 to 1, or negative while unknown.
    pub progress: f64,
    pub eta_secs: Option<u64>,
    pub connections: u32,
    pub error: Option<String>,
    /// Unix time in milliseconds.
    pub added_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TaskStatus {
    Downloading,
    Queued,
    Paused,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum EngineState {
    Starting,
    Running,
    /// Crashed and restarting on its own.
    Restarting {
        reason: String,
    },
    /// Crashed too often; waits for `restart_engine`.
    Failed {
        reason: String,
    },
    Stopped,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct GlobalStats {
    pub download_speed: u64,
    pub upload_speed: u64,
    pub active: u32,
    pub waiting: u32,
}

/// One download to start, with whatever the browser knows about it.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct NewDownload {
    pub url: String,
    /// A name for the file. The engine still prefers Content-Disposition
    /// unless the source is "browser".
    #[uniffi(default = None)]
    pub filename: Option<String>,
    /// "browser" (the browser resolved the name), "title" (from the page
    /// title) or "suggested".
    #[uniffi(default = None)]
    pub filename_source: Option<String>,
    #[uniffi(default = None)]
    pub referer: Option<String>,
    #[uniffi(default = None)]
    pub cookie: Option<String>,
    #[uniffi(default = None)]
    pub user_agent: Option<String>,
    /// Extra request headers as "Name: value".
    #[uniffi(default = [])]
    pub headers: Vec<String>,
    /// A save folder chosen by the user instead of the category folder.
    #[uniffi(default = None)]
    pub dir: Option<String>,
    /// What the browser saw, for showing before the download starts.
    #[uniffi(default = None)]
    pub mime: Option<String>,
    #[uniffi(default = None)]
    pub size: Option<u64>,
    /// "hls" or "dash" when this is a stream, so the engine reads it as one
    /// whatever its URL looks like.
    #[uniffi(default = None)]
    pub media_kind: Option<String>,
    /// The playlist as the page received it, used instead of fetching the
    /// URL again (pages that build or sign their playlists in script).
    #[uniffi(default = None)]
    pub media_manifest: Option<String>,
    /// The stream's video and audio track: "best", "none" or a track ID
    /// from the quality menu.
    #[uniffi(default = None)]
    pub media_video: Option<String>,
    #[uniffi(default = None)]
    pub media_audio: Option<String>,
    /// "mp4" or "mkv".
    #[uniffi(default = None)]
    pub media_format: Option<String>,
    /// The quality chosen in the browser, as the menu showed it
    /// ("1080p · VP9 · ≈ 327 MB"), for the confirmation dialog.
    #[uniffi(default = None)]
    pub media_quality: Option<String>,
    /// A category chosen by the user instead of the one the name implies.
    #[uniffi(default = None)]
    pub category: Option<String>,
}

/// A download as it will be, before it starts.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskDetails {
    /// Share done (0-100) of each equal part of the file; empty when the
    /// engine has no piece map (streams, or not started yet).
    pub segments: Vec<u8>,
    pub connections: Vec<ConnectionInfo>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ConnectionInfo {
    pub host: String,
    /// Bytes per second.
    pub speed: u64,
}

/// A download as it will be, before it starts.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FileInfo {
    pub name: String,
    pub size: Option<u64>,
    pub mime: Option<String>,
    /// Whether the server can continue an interrupted download, when known.
    pub resumable: Option<bool>,
    pub category: String,
    /// The folder it is saved to.
    pub dir: String,
}

/// How a browser handed downloads to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BrowserSource {
    /// A download the browser started, captured by the extension.
    Download,
    /// "Download with OpenDM" on one link, image or video.
    Link,
    /// The selected links or all links on a page.
    Links,
    /// A video or audio stream found on the page.
    Media,
}

/// Implemented by the app. Called on a background thread.
#[uniffi::export(with_foreign)]
pub trait CoreListener: Send + Sync {
    fn rows_changed(&self, rows: Vec<TaskRow>);
    fn rows_removed(&self, ids: Vec<String>);
    fn stats_changed(&self, stats: GlobalStats);
    fn engine_state_changed(&self, state: EngineState);
    fn error(&self, message: String);
    /// The browser handed over downloads. The app confirms them with the
    /// user, or starts them with `add_downloads`. The browser has already
    /// dropped its own copy.
    fn browser_downloads(&self, downloads: Vec<NewDownload>, source: BrowserSource);
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreConfig {
    /// The bundled aria2-next executable.
    pub engine_path: String,
    /// Where history, settings and engine state live.
    pub data_dir: String,
    /// Used until the user picks another download folder.
    pub default_download_dir: String,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{message}")]
    Failed { message: String },
}

impl From<String> for CoreError {
    fn from(message: String) -> Self {
        CoreError::Failed { message }
    }
}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        CoreError::Failed {
            message: error.to_string(),
        }
    }
}

/// State shared by the supervisor, the poller and the user actions.
pub(crate) struct Shared {
    pub engine_path: PathBuf,
    pub data_dir: PathBuf,
    pub listener: Arc<dyn CoreListener>,
    pub db: Mutex<db::Db>,
    pub settings: Mutex<Settings>,
    pub tasks: Mutex<tasks::Table>,
    /// The connection to the running engine, if any.
    pub rpc: watch::Sender<Option<rpc::Rpc>>,
    pub engine_pid: Mutex<Option<u32>>,
    pub engine_state: Mutex<EngineState>,
    pub shutdown: watch::Sender<bool>,
    pub restart: Notify,
    /// Requests an immediate poll.
    pub wake: Notify,
    pub visible: AtomicBool,
    /// Connected browser extensions, for pushing capture rules.
    pub browsers: Mutex<Vec<tokio::sync::mpsc::UnboundedSender<String>>>,
    /// Stream probes running now; each gets an extra engine slot.
    pub probe_slots: Mutex<u32>,
    /// Set while yt-dlp and Deno are being downloaded.
    pub tools_busy: AtomicBool,
}

impl Shared {
    pub fn current_rpc(&self) -> Option<rpc::Rpc> {
        self.rpc.borrow().clone()
    }

    /// The engine connection, waiting briefly while the engine (re)starts.
    pub async fn wait_rpc(&self) -> Result<rpc::Rpc, String> {
        let mut receiver = self.rpc.subscribe();
        match tokio::time::timeout(Duration::from_secs(15), receiver.wait_for(Option::is_some))
            .await
        {
            Ok(Ok(rpc)) => Ok(rpc.clone().expect("checked by wait_for")),
            _ => Err("The download engine is not running.".into()),
        }
    }

    pub fn set_engine_state(&self, state: EngineState) {
        *self.engine_state.lock().unwrap() = state.clone();
        self.listener.engine_state_changed(state);
    }
}

/// Resolves once shutdown has been requested.
pub(crate) async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|stop| *stop).await;
}

#[derive(uniffi::Object)]
pub struct DownloadCore {
    runtime: tokio::runtime::Runtime,
    shared: Arc<Shared>,
    supervisor: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[uniffi::export]
impl DownloadCore {
    /// Opens history and settings. The engine starts with `start`.
    #[uniffi::constructor]
    pub fn new(
        config: CoreConfig,
        listener: Arc<dyn CoreListener>,
    ) -> Result<Arc<Self>, CoreError> {
        let data_dir = PathBuf::from(&config.data_dir);
        std::fs::create_dir_all(&data_dir)?;
        let db = db::Db::open(&data_dir.join("history.db"))?;
        let mut table = tasks::Table::default();
        for task in db.load()? {
            table.tasks.insert(task.row.id.clone(), task);
        }
        let settings = Settings::load(
            &data_dir.join("settings.json"),
            &config.default_download_dir,
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("opendm-core")
            .enable_all()
            .build()?;
        let shared = Arc::new(Shared {
            engine_path: PathBuf::from(&config.engine_path),
            data_dir,
            listener,
            db: Mutex::new(db),
            settings: Mutex::new(settings),
            tasks: Mutex::new(table),
            rpc: watch::Sender::new(None),
            engine_pid: Mutex::new(None),
            engine_state: Mutex::new(EngineState::Stopped),
            shutdown: watch::Sender::new(false),
            restart: Notify::new(),
            wake: Notify::new(),
            visible: AtomicBool::new(true),
            browsers: Mutex::new(Vec::new()),
            probe_slots: Mutex::new(0),
            tools_busy: AtomicBool::new(false),
        });
        Ok(Arc::new(DownloadCore {
            runtime,
            shared,
            supervisor: Mutex::new(None),
        }))
    }

    /// Starts the engine and the progress updates.
    pub fn start(&self) {
        let mut supervisor = self.supervisor.lock().unwrap();
        if supervisor.is_some() {
            return;
        }
        *supervisor = Some(self.runtime.spawn(engine::supervise(self.shared.clone())));
        self.runtime.spawn(tasks::run_poller(self.shared.clone()));
        self.runtime.spawn(browser::serve(self.shared.clone()));
    }

    /// Saves progress and stops the engine. Blocks for at most a few seconds.
    pub fn shutdown(&self) {
        tasks::save_all(&self.shared);
        self.shared.shutdown.send_replace(true);
        if let Some(handle) = self.supervisor.lock().unwrap().take() {
            let _ = self
                .runtime
                .block_on(async { tokio::time::timeout(Duration::from_secs(6), handle).await });
        }
    }

    /// Every task, oldest first.
    pub fn rows(&self) -> Vec<TaskRow> {
        self.shared.tasks.lock().unwrap().rows()
    }

    pub fn stats(&self) -> GlobalStats {
        self.shared.tasks.lock().unwrap().stats()
    }

    pub fn engine_state(&self) -> EngineState {
        self.shared.engine_state.lock().unwrap().clone()
    }

    /// Progress is polled faster while a window shows it.
    pub fn set_window_visible(&self, visible: bool) {
        self.shared.visible.store(visible, Ordering::Relaxed);
        if visible {
            self.shared.wake.notify_one();
        }
    }

    pub fn add_urls(&self, urls: Vec<String>, start_paused: bool) {
        self.run(move |shared| async move { tasks::add_urls(&shared, urls, start_paused).await });
    }

    /// Starts downloads with their names, folders and request headers, and
    /// returns their IDs at once; their rows follow through the listener.
    pub fn add_downloads(&self, downloads: Vec<NewDownload>, start_paused: bool) -> Vec<String> {
        let downloads = match tasks::with_ids(downloads) {
            Ok(downloads) => downloads,
            Err(message) => {
                self.shared.listener.error(message);
                return Vec::new();
            }
        };
        let ids = downloads.iter().map(|(id, _)| id.clone()).collect();
        self.run(move |shared| async move { tasks::add(&shared, downloads, start_paused).await });
        ids
    }

    /// Segment map and connections of a download, for its progress window.
    /// Asks the engine, so call it off the main thread.
    pub fn task_details(&self, id: String, cells: u32) -> TaskDetails {
        let shared = self.shared.clone();
        self.runtime
            .block_on(async move { tasks::details(&shared, &id, cells as usize).await })
    }

    /// Name, category and folder of a download from what is already known,
    /// without asking the server. For showing a confirmation dialog at once.
    pub fn plan_download(&self, download: NewDownload) -> FileInfo {
        let settings = self.shared.settings.lock().unwrap().clone();
        let name = download.planned_name();
        let (category, dir) = download.place(&settings, &name);
        FileInfo {
            name,
            size: download.size,
            mime: download.mime,
            resumable: None,
            category,
            dir,
        }
    }

    /// Like `plan_download`, with the name and size the server gives. Asks
    /// the server with the browser's cookies and headers, so it can take a
    /// few seconds: call it off the main thread.
    pub fn inspect_download(&self, download: NewDownload) -> Result<FileInfo, CoreError> {
        let shared = self.shared.clone();
        Ok(self
            .runtime
            .block_on(async move { tasks::inspect(&shared, &download).await })?)
    }

    /// The folder downloads of `category` are saved to.
    pub fn category_dir(&self, category: String) -> String {
        let settings = self.shared.settings.lock().unwrap();
        settings.directory_for(&category).display().to_string()
    }

    /// Registers `host_path`, the bundled `opendm-host`, with the installed
    /// browsers so the extension can reach the app. Returns the manifests
    /// written. Call it at every launch; it rewrites only what changed.
    pub fn install_browser_host(&self, host_path: String) -> Result<Vec<String>, CoreError> {
        let written = opendm_host::install(&PathBuf::from(host_path), &self.shared.data_dir)?;
        Ok(written
            .iter()
            .map(|path| path.display().to_string())
            .collect())
    }

    pub fn pause(&self, ids: Vec<String>) {
        self.run(move |shared| async move { tasks::pause(&shared, ids).await });
    }

    /// Resumes paused downloads and restarts failed or interrupted ones.
    pub fn resume(&self, ids: Vec<String>) {
        self.run(move |shared| async move { tasks::resume(&shared, ids).await });
    }

    pub fn pause_all(&self) {
        self.run(|shared| async move { tasks::pause_all(&shared).await });
    }

    pub fn resume_all(&self) {
        self.run(|shared| async move { tasks::resume_all(&shared).await });
    }

    pub fn remove(&self, ids: Vec<String>, delete_files: bool) {
        self.run(move |shared| async move { tasks::remove(&shared, ids, delete_files).await });
    }

    pub fn settings(&self) -> Settings {
        self.shared.settings.lock().unwrap().clone()
    }

    /// Saves the settings and applies engine options to the running engine.
    pub fn set_settings(&self, settings: Settings) -> Result<(), CoreError> {
        settings.save(&self.shared.data_dir.join("settings.json"))?;
        let options: serde_json::Map<String, serde_json::Value> = settings
            .engine_options()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.into()))
            .collect();
        *self.shared.settings.lock().unwrap() = settings;
        browser::push_rules(&self.shared);
        self.run(move |shared| async move {
            match shared.current_rpc() {
                Some(rpc) => rpc
                    .call("aria2.changeGlobalOption", vec![options.into()])
                    .await
                    .map(|_| ()),
                None => Ok(()), // Applied when the engine next starts.
            }
        });
        Ok(())
    }

    /// Starts the engine again after it failed too often.
    pub fn restart_engine(&self) {
        self.shared.restart.notify_one();
    }

    pub fn engine_pid(&self) -> Option<u32> {
        *self.shared.engine_pid.lock().unwrap()
    }
}

impl DownloadCore {
    /// Runs an action in the background and reports its error to the listener.
    fn run<F, Fut>(&self, action: F)
    where
        F: FnOnce(Arc<Shared>) -> Fut,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let shared = self.shared.clone();
        let future = action(shared.clone());
        self.runtime.spawn(async move {
            if let Err(message) = future.await {
                shared.listener.error(message);
            }
        });
    }
}
