//! End-to-end checks against the real aria2-next binary.
//!
//! Run `scripts/fetch-engine.sh` first, or point OPENDM_TEST_ENGINE at an
//! aria2-next executable. Without one the test is skipped.
#![cfg(unix)]

use opendm_core::{
    CoreConfig, CoreListener, DownloadCore, EngineState, GlobalStats, TaskRow, TaskStatus,
};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FILE_SIZE: u64 = 12 * 1024 * 1024;

fn engine_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OPENDM_TEST_ENGINE") {
        return Some(path.into());
    }
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../vendor/aria2-next/bin/aria2-next-macos-{arch}"));
    path.exists().then_some(path)
}

#[derive(Default)]
struct Recorder {
    states: Mutex<Vec<EngineState>>,
    errors: Mutex<Vec<String>>,
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
        if let Some(range) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
            let (from, to) = range.split_once('-').unwrap_or((range, ""));
            start = from.parse().unwrap_or(0);
            if let Ok(to) = to.parse::<u64>() {
                end = to.min(FILE_SIZE - 1);
            }
            ranged = true;
        }
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
    std::process::Command::new("kill")
        .args(["-9", &first_pid.to_string()])
        .status()
        .unwrap();

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
    assert_eq!(path, downloads.join("data.bin"));
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
    assert_eq!(other_path, downloads.join("Compressed").join("other.zip"));
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
