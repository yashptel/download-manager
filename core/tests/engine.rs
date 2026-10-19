//! End-to-end checks against the real aria2-next binary.
//!
//! Run `scripts/fetch-engine.sh` first, or point OPENDM_TEST_ENGINE at an
//! aria2-next executable (on Windows, `scripts/fetch-engine.ps1`). Without
//! one the test is skipped.

use opendm_core::{
    BrowserSource, CoreConfig, CoreListener, DownloadCore, EngineState, GlobalStats, NewDownload,
    TaskRow, TaskStatus,
};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FILE_SIZE: u64 = 12 * 1024 * 1024;
/// While set, /flaky/ responses break off after 256 KB.
static FLAKY: AtomicBool = AtomicBool::new(true);
/// While set, /outage/ cuts every connection partway, like a server that
/// blocks this address for a while.
static OUTAGE: AtomicBool = AtomicBool::new(true);
/// Like OUTAGE, for /blocked/.
static BLOCKED: AtomicBool = AtomicBool::new(true);

fn engine_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OPENDM_TEST_ENGINE") {
        return Some(path.into());
    }
    let name = match (cfg!(windows), cfg!(target_arch = "aarch64")) {
        (true, true) => "aria2-next-windows-arm64.exe",
        (true, false) => "aria2-next-windows-x86_64.exe",
        (false, true) => "aria2-next-macos-arm64",
        (false, false) => "aria2-next-macos-x86_64",
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/aria2-next/bin")
        .join(name);
    path.exists().then_some(path)
}

fn kill_hard(pid: u32) {
    let pid = pid.to_string();
    let status = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid])
            .status()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &pid])
            .status()
    };
    assert!(status.unwrap().success(), "could not kill the engine");
}

/// Compares paths regardless of separator style and symlinked temp folders.
fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok()
}

#[derive(Default)]
struct Recorder {
    states: Mutex<Vec<EngineState>>,
    errors: Mutex<Vec<String>>,
    logins: Mutex<Vec<(String, String)>>,
}

impl CoreListener for Recorder {
    fn rows_changed(&self, _rows: Vec<TaskRow>) {}
    fn rows_removed(&self, _ids: Vec<String>) {}
    fn stats_changed(&self, _stats: GlobalStats) {}
    fn engine_state_changed(&self, state: EngineState) {
        self.states.lock().unwrap().push(state);
    }
    fn error(&self, message: String) {
        self.errors.lock().unwrap().push(message);
    }
    fn browser_downloads(&self, _downloads: Vec<NewDownload>, _source: BrowserSource) {}
    fn login_needed(&self, id: String, url: String) {
        self.logins.lock().unwrap().push((id, url));
    }
    fn new_address(&self, _id: String, _download: NewDownload) {}
}

/// Byte `i` of every served file.
fn byte_at(index: u64) -> u8 {
    (index % 251) as u8
}

/// A slow HTTP server with Range support, so downloads take a few seconds.
fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let _ = respond(stream);
            });
        }
    });
    port
}

fn respond(mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut start = 0u64;
    let mut end = FILE_SIZE - 1;
    let mut ranged = false;
    let mut is_head = false;
    let mut private = false;
    let mut flaky = false;
    let mut authorized = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if line.starts_with("HEAD ") {
            is_head = true;
        }
        if line.contains(" /private/") {
            private = true;
        }
        if line.contains(" /flaky/") {
            flaky = FLAKY.load(Ordering::SeqCst);
        }
        if line.contains(" /outage/") {
            flaky = OUTAGE.load(Ordering::SeqCst);
        }
        if line.contains(" /blocked/") {
            flaky = BLOCKED.load(Ordering::SeqCst);
        }
        // An address that has expired partway through.
        if line.contains(" /expires/") {
            flaky = true;
        }
        // u:p
        if line.eq_ignore_ascii_case("authorization: basic dTpw") {
            authorized = true;
        }
        if let Some(range) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
            let (from, to) = range.split_once('-').unwrap_or((range, ""));
            start = from.parse().unwrap_or(0);
            if let Ok(to) = to.parse::<u64>() {
                end = to.min(FILE_SIZE - 1);
            }
            ranged = true;
        }
    }
    if private && !authorized {
        return stream.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    }
    let length = end - start + 1;
    let head = if ranged {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{FILE_SIZE}\r\nContent-Length: {length}\r\n"
        )
    } else {
        format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n")
    };
    stream.write_all(
        format!("{head}Accept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )?;
    if is_head {
        return Ok(());
    }
    let mut position = start;
    if flaky {
        end = end.min(start + 256 * 1024);
    }
    while position <= end {
        let chunk_end = (position + 16 * 1024).min(end + 1);
        let chunk: Vec<u8> = (position..chunk_end).map(byte_at).collect();
        stream.write_all(&chunk)?;
        position = chunk_end;
        std::thread::sleep(Duration::from_millis(15));
    }
    Ok(())
}

fn wait_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn row(core: &DownloadCore, id: &str) -> Option<TaskRow> {
    core.rows().into_iter().find(|row| row.id == id)
}

/// Bytes the engine has recorded as on disk for a download it can resume.
/// Writes still in flight when it fails are not among them.
fn recorded(data: &Path, id: &str) -> u64 {
    let db = rusqlite::Connection::open(data.join("engine/state/stream/state.db")).unwrap();
    let bytes: i64 =
        db.query_row("SELECT completed_length FROM downloads WHERE gid = ?1", [id], |row| row.get(0)).unwrap();
    bytes as u64
}

fn open(
    engine: &Path,
    data: &Path,
    downloads: &Path,
    listener: Arc<Recorder>,
) -> Arc<DownloadCore> {
    DownloadCore::new(
        CoreConfig {
            engine_path: engine.display().to_string(),
            data_dir: data.display().to_string(),
            default_download_dir: downloads.display().to_string(),
        },
        listener,
    )
    .unwrap()
}

#[test]
fn survives_an_engine_crash_and_an_app_restart() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary (run scripts/fetch-engine.sh)");
        return;
    };
    let port = serve();
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let downloads = temp.path().join("downloads");
    let recorder = Arc::new(Recorder::default());
    let core = open(&engine, &data, &downloads, recorder.clone());
    core.start();
    wait_until("engine start", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
    });

    // A download is cut off by an engine crash and finishes after the restart.
    core.add_urls(
        vec![format!("http://127.0.0.1:{port}/files/data.bin")],
        false,
    );
    wait_until("the download row", Duration::from_secs(5), || {
        core.rows().len() == 1
    });
    let id = core.rows()[0].id.clone();
    wait_until("download progress", Duration::from_secs(15), || {
        row(&core, &id)
            .is_some_and(|row| row.status == TaskStatus::Downloading && row.done_bytes > 0)
    });
    let first_pid = core.engine_pid().unwrap();
    kill_hard(first_pid);

    wait_until("engine restart", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
            && core.engine_pid().is_some_and(|pid| pid != first_pid)
    });
    assert!(
        recorder
            .states
            .lock()
            .unwrap()
            .iter()
            .any(|state| matches!(state, EngineState::Restarting { .. })),
        "the crash should be reported"
    );
    wait_until(
        "completion after the crash",
        Duration::from_secs(60),
        || row(&core, &id).is_some_and(|row| row.status == TaskStatus::Complete),
    );
    let done = row(&core, &id).unwrap();
    assert_eq!(done.name, "data.bin");
    assert_eq!(done.category, "Other");
    assert_eq!(done.total_bytes, FILE_SIZE);
    let path = PathBuf::from(done.path.clone().unwrap());
    assert!(
        !cfg!(windows) || !path.to_string_lossy().contains('/'),
        "paths should use backslashes on Windows: {path:?}"
    );
    assert!(
        same_file(&path, &downloads.join("data.bin")),
        "unexpected path {path:?}"
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len() as u64, FILE_SIZE);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte == byte_at(index as u64)),
        "content mismatch"
    );

    // Removing with files deletes the partial file.
    core.add_urls(
        vec![format!("http://127.0.0.1:{port}/files/other.zip")],
        false,
    );
    wait_until("second download progress", Duration::from_secs(15), || {
        core.rows()
            .iter()
            .any(|row| row.name == "other.zip" && row.done_bytes > 0)
    });
    let other = core
        .rows()
        .into_iter()
        .find(|row| row.name == "other.zip")
        .unwrap();
    let other_path = PathBuf::from(other.path.clone().unwrap());
    assert!(
        same_file(&other_path, &downloads.join("Compressed").join("other.zip")),
        "unexpected path {other_path:?}"
    );
    core.remove(vec![other.id.clone()], true);
    wait_until("removal", Duration::from_secs(10), || {
        row(&core, &other.id).is_none()
    });
    assert!(!other_path.exists(), "the partial file should be deleted");

    // History survives an app restart, and the finished task stays finished.
    core.shutdown();
    assert_eq!(core.engine_state(), EngineState::Stopped);
    drop(core);
    let core = open(&engine, &data, &downloads, Arc::new(Recorder::default()));
    assert_eq!(
        row(&core, &id).map(|row| row.status),
        Some(TaskStatus::Complete)
    );
    assert_eq!(core.rows().len(), 1);
    core.start();
    wait_until(
        "engine start after reopening",
        Duration::from_secs(15),
        || core.engine_state() == EngineState::Running,
    );
    assert_eq!(
        row(&core, &id).map(|row| row.status),
        Some(TaskStatus::Complete)
    );
    core.shutdown();

    let errors = recorder.errors.lock().unwrap();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn a_refused_download_asks_for_a_login_and_finishes_with_it() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary (run scripts/fetch-engine.sh)");
        return;
    };
    let port = serve();
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let recorder = Arc::new(Recorder::default());
    let core = open(&engine, &temp.path().join("data"), &downloads, recorder.clone());
    core.start();
    wait_until("engine start", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
    });

    let url = format!("http://127.0.0.1:{port}/private/secret.bin");
    core.add_urls(vec![url.clone()], false);
    wait_until("the login request", Duration::from_secs(15), || {
        !recorder.logins.lock().unwrap().is_empty()
    });
    let (id, asked_for) = recorder.logins.lock().unwrap()[0].clone();
    assert_eq!(asked_for, url);
    let failed = row(&core, &id).unwrap();
    assert_eq!(failed.status, TaskStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("The server asks for a login (HTTP 401)"));

    core.set_login(id.clone(), "u".into(), "p".into());
    wait_until("completion with the login", Duration::from_secs(60), || {
        row(&core, &id).is_some_and(|row| row.status == TaskStatus::Complete)
    });
    let path = PathBuf::from(row(&core, &id).unwrap().path.unwrap());
    assert_eq!(std::fs::read(&path).unwrap().len() as u64, FILE_SIZE);
    assert_eq!(recorder.logins.lock().unwrap().len(), 1, "asked once");
    core.shutdown();
}


#[test]
fn a_download_that_failed_partway_continues_into_a_correct_file() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary (run scripts/fetch-engine.sh)");
        return;
    };
    let port = serve();
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let core = open(&engine, &temp.path().join("data"), &downloads, Arc::new(Recorder::default()));
    let mut settings = core.settings();
    settings.max_tries = 1;
    settings.retry_wait = 0;
    core.set_settings(settings).unwrap();
    core.start();
    wait_until("engine start", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
    });

    core.add_urls(vec![format!("http://127.0.0.1:{port}/flaky/big.bin")], false);
    wait_until("the row", Duration::from_secs(5), || core.rows().len() == 1);
    let id = core.rows()[0].id.clone();
    wait_until("the failure", Duration::from_secs(30), || {
        row(&core, &id).is_some_and(|row| row.status == TaskStatus::Failed)
    });
    let failed = row(&core, &id).unwrap();
    assert!(failed.done_bytes > 0 && failed.done_bytes < FILE_SIZE, "{failed:?}");
    let kept = recorded(&temp.path().join("data"), &id);
    assert!(kept > 0);

    // The server recovers; Retry must not take the untouched parts as done.
    FLAKY.store(false, Ordering::SeqCst);
    core.resume(vec![id.clone()]);
    // Nor drop the parts that were done, gaps and all.
    let mut least = u64::MAX;
    wait_until("completion after the retry", Duration::from_secs(60), || {
        let row = row(&core, &id).unwrap();
        least = least.min(row.done_bytes);
        row.status == TaskStatus::Complete
    });
    assert!(least >= kept, "went back from {kept} to {least} bytes");
    let path = PathBuf::from(row(&core, &id).unwrap().path.unwrap());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len() as u64, FILE_SIZE);
    let first_wrong = bytes.iter().enumerate().position(|(index, byte)| *byte != byte_at(index as u64));
    assert_eq!(first_wrong, None, "the file should be correct throughout");
    core.shutdown();
}


#[test]
fn a_download_cut_off_partway_tries_again_on_its_own() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary (run scripts/fetch-engine.sh)");
        return;
    };
    let port = serve();
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let core = open(&engine, &temp.path().join("data"), &downloads, Arc::new(Recorder::default()));
    let mut settings = core.settings();
    settings.max_tries = 1;
    settings.retry_wait = 0;
    core.set_settings(settings).unwrap();
    core.start();
    wait_until("engine start", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
    });

    let id = core.add_downloads(
        vec![NewDownload { url: format!("http://127.0.0.1:{port}/outage/big.bin"), ..NewDownload::default() }],
        false,
    )[0]
    .clone();
    wait_until("the failure", Duration::from_secs(30), || {
        row(&core, &id).is_some_and(|row| row.status == TaskStatus::Failed && row.retry_at.is_some())
    });
    let failed = row(&core, &id).unwrap();
    let wait = failed.retry_at.unwrap() - now_ms();
    assert!((25_000..=30_000).contains(&wait), "retries in {wait} ms");
    let kept = recorded(&temp.path().join("data"), &id);

    // The server comes back; the download continues without the user.
    OUTAGE.store(false, Ordering::SeqCst);
    let mut least = u64::MAX;
    wait_until("completion after the retry", Duration::from_secs(90), || {
        let row = row(&core, &id).unwrap();
        least = least.min(row.done_bytes);
        row.status == TaskStatus::Complete
    });
    assert!(least >= kept, "went back from {kept} to {least} bytes");
    let bytes = std::fs::read(row(&core, &id).unwrap().path.unwrap()).unwrap();
    assert_eq!(bytes.len() as u64, FILE_SIZE);
    let first_wrong = bytes.iter().enumerate().position(|(index, byte)| *byte != byte_at(index as u64));
    assert_eq!(first_wrong, None, "the file should be correct throughout");
    core.shutdown();
}

#[test]
fn a_download_waiting_to_retry_can_be_paused_and_resumed() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary (run scripts/fetch-engine.sh)");
        return;
    };
    let port = serve();
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let core = open(&engine, &temp.path().join("data"), &downloads, Arc::new(Recorder::default()));
    let mut settings = core.settings();
    settings.max_tries = 1;
    settings.retry_wait = 0;
    core.set_settings(settings).unwrap();
    core.start();
    wait_until("engine start", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
    });

    let id = core.add_downloads(
        vec![NewDownload { url: format!("http://127.0.0.1:{port}/blocked/big.bin"), ..NewDownload::default() }],
        false,
    )[0]
    .clone();
    wait_until("the failure", Duration::from_secs(30), || {
        row(&core, &id).is_some_and(|row| row.status == TaskStatus::Failed && row.retry_at.is_some())
    });
    let kept = recorded(&temp.path().join("data"), &id);

    // Pause stops the waiting: the download is paused, with nothing to retry.
    core.pause(vec![id.clone()]);
    wait_until("the pause", Duration::from_secs(10), || {
        row(&core, &id).is_some_and(|row| row.status == TaskStatus::Paused && row.retry_at.is_none())
    });

    BLOCKED.store(false, Ordering::SeqCst);
    core.resume(vec![id.clone()]);
    let mut least = u64::MAX;
    wait_until("completion after resuming", Duration::from_secs(60), || {
        let row = row(&core, &id).unwrap();
        least = least.min(row.done_bytes);
        row.status == TaskStatus::Complete
    });
    assert!(least >= kept, "went back from {kept} to {least} bytes");
    let bytes = std::fs::read(row(&core, &id).unwrap().path.unwrap()).unwrap();
    assert_eq!(bytes.len() as u64, FILE_SIZE);
    let first_wrong = bytes.iter().enumerate().position(|(index, byte)| *byte != byte_at(index as u64));
    assert_eq!(first_wrong, None, "the file should be correct throughout");
    core.shutdown();
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}


#[test]
fn an_expired_download_continues_from_a_new_address() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary (run scripts/fetch-engine.sh)");
        return;
    };
    let port = serve();
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let core = open(&engine, &temp.path().join("data"), &downloads, Arc::new(Recorder::default()));
    let mut settings = core.settings();
    settings.max_tries = 1;
    settings.retry_wait = 0;
    core.set_settings(settings).unwrap();
    core.start();
    wait_until("engine start", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
    });

    let page = format!("http://127.0.0.1:{port}/downloads.html");
    let id = core.add_downloads(
        vec![NewDownload {
            url: format!("http://127.0.0.1:{port}/expires/movie.bin"),
            referer: Some(page.clone()),
            ..NewDownload::default()
        }],
        false,
    )[0]
    .clone();
    wait_until("the failure", Duration::from_secs(30), || {
        row(&core, &id).is_some_and(|row| row.status == TaskStatus::Failed)
    });
    assert!(row(&core, &id).unwrap().done_bytes > 0);
    let kept = recorded(&temp.path().join("data"), &id);
    assert!(kept > 0);

    assert_eq!(core.wait_for_address(id.clone()).unwrap(), Some(page));
    core.stop_waiting_for_address();
    let fresh = format!("http://127.0.0.1:{port}/files/movie.bin?token=new");
    core.refresh_address(id.clone(), NewDownload { url: fresh.clone(), ..NewDownload::default() });
    let mut least = u64::MAX;
    wait_until("completion at the new address", Duration::from_secs(60), || {
        let Some(row) = row(&core, &id) else { return false };
        least = least.min(row.done_bytes);
        row.status == TaskStatus::Complete
    });
    assert!(least >= kept, "went back from {kept} to {least} bytes");
    let done = row(&core, &id).unwrap();
    assert_eq!(done.url, fresh);
    assert_eq!(done.name, "movie.bin");
    let bytes = std::fs::read(done.path.unwrap()).unwrap();
    assert_eq!(bytes.len() as u64, FILE_SIZE);
    let first_wrong = bytes.iter().enumerate().position(|(index, byte)| *byte != byte_at(index as u64));
    assert_eq!(first_wrong, None, "the file should be correct throughout");
    assert_eq!(core.rows().len(), 1, "the same download, not a new one");
    core.shutdown();
}


#[test]
fn a_download_goes_through_the_proxy_the_browser_used() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary (run scripts/fetch-engine.sh)");
        return;
    };
    // The test server answers any request line, so it also serves as a proxy
    // for a host that does not resolve: only the proxy can reach it.
    let proxy_port = serve();
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let core = open(&engine, &temp.path().join("data"), &downloads, Arc::new(Recorder::default()));
    core.start();
    wait_until("engine start", Duration::from_secs(15), || {
        core.engine_state() == EngineState::Running
    });
    let id = core.add_downloads(
        vec![NewDownload {
            url: "http://files.opendm.invalid/proxied.bin".into(),
            proxy: Some(format!("http://127.0.0.1:{proxy_port}")),
            ..NewDownload::default()
        }],
        false,
    )[0]
    .clone();
    wait_until("completion through the proxy", Duration::from_secs(60), || {
        row(&core, &id).is_some_and(|row| matches!(row.status, TaskStatus::Complete | TaskStatus::Failed))
    });
    let done = row(&core, &id).unwrap();
    assert_eq!(done.status, TaskStatus::Complete, "{:?}", done.error);
    assert_eq!(std::fs::read(done.path.unwrap()).unwrap().len() as u64, FILE_SIZE);
    core.shutdown();
}
