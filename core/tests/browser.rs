//! The extension's path into the app, without a browser: connect to the
//! app's socket the way `opendm-host` does, hand over a download, and check
//! that the request the engine makes carries the browser's cookie, referer,
//! user agent and file name.
//!
//! Needs the aria2-next binary, like the engine test; skipped without it.

use opendm_core::{
    BrowserSource, CoreConfig, CoreListener, DownloadCore, EngineState, GlobalStats, NewDownload,
    TaskRow, TaskStatus,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

#[derive(Default)]
struct Recorder {
    rows: Mutex<Vec<TaskRow>>,
    handed: Mutex<Vec<(Vec<NewDownload>, BrowserSource)>>,
    running: Mutex<bool>,
}

impl CoreListener for Recorder {
    fn rows_changed(&self, rows: Vec<TaskRow>) {
        let mut all = self.rows.lock().unwrap();
        for row in rows {
            all.retain(|old| old.id != row.id);
            all.push(row);
        }
    }
    fn rows_removed(&self, _ids: Vec<String>) {}
    fn stats_changed(&self, _stats: GlobalStats) {}
    fn engine_state_changed(&self, state: EngineState) {
        *self.running.lock().unwrap() = state == EngineState::Running;
    }
    fn error(&self, _message: String) {}
    fn browser_downloads(&self, downloads: Vec<NewDownload>, source: BrowserSource) {
        self.handed.lock().unwrap().push((downloads, source));
    }
}

/// Serves a small file and keeps every request's header lines.
fn serve(requests: Arc<Mutex<Vec<String>>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            requests.lock().unwrap().push(head);
            let body = b"hello from the server";
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = stream.write_all(body);
        }
    });
    port
}

#[cfg(unix)]
fn connect(data_dir: &Path) -> std::io::Result<impl Read + Write> {
    std::os::unix::net::UnixStream::connect(opendm_host::endpoint(data_dir))
}

#[cfg(windows)]
fn connect(data_dir: &Path) -> std::io::Result<impl Read + Write> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(opendm_host::endpoint(data_dir))
}

/// Sends one message and reads one line back.
fn ask(stream: &mut (impl Read + Write), message: serde_json::Value) -> serde_json::Value {
    stream.write_all(format!("{message}\n").as_bytes()).unwrap();
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
        line.push(byte[0]);
    }
    serde_json::from_slice(&line).unwrap()
}

fn wait_for(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_extension_hands_a_download_to_the_app() {
    let Some(engine) = engine_path() else {
        eprintln!("skipped: no aria2-next binary");
        return;
    };
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let port = serve(requests.clone());
    let recorder = Arc::new(Recorder::default());
    let core = DownloadCore::new(
        CoreConfig {
            engine_path: engine.display().to_string(),
            data_dir: data.path().display().to_string(),
            default_download_dir: downloads.path().display().to_string(),
        },
        recorder.clone(),
    )
    .unwrap();
    core.start();
    wait_for("the engine", Duration::from_secs(20), || {
        *recorder.running.lock().unwrap()
    });
    let mut socket = None;
    wait_for("the browser socket", Duration::from_secs(5), || {
        socket = connect(data.path()).ok();
        socket.is_some()
    });
    let mut socket = socket.unwrap();

    let hello = ask(&mut socket, serde_json::json!({ "id": 1, "type": "hello" }));
    assert_eq!(hello["id"], 1);
    assert_eq!(hello["app"], "OpenDM");
    assert_eq!(hello["rules"]["capture"], true);
    assert!(
        hello["rules"]["extensions"]
            .as_array()
            .unwrap()
            .contains(&"zip".into())
    );

    let url = format!("http://127.0.0.1:{port}/get?id=7");
    let answer = ask(
        &mut socket,
        serde_json::json!({
            "id": 2,
            "type": "download",
            "source": "download",
            "items": [{
                "url": url,
                "referrer": "https://example.com/files",
                "cookie": "session=abc",
                "userAgent": "TestBrowser/1.0",
                "filename": "report.pdf",
                "filenameSource": "browser",
            }],
        }),
    );
    assert_eq!(answer, serde_json::json!({ "id": 2, "result": "accepted" }));

    // The app decides; here it starts what it was given.
    wait_for("the handoff", Duration::from_secs(5), || {
        !recorder.handed.lock().unwrap().is_empty()
    });
    let (handed, source) = recorder.handed.lock().unwrap().remove(0);
    assert_eq!(source, BrowserSource::Download);
    core.add_downloads(handed, false);

    wait_for("the download", Duration::from_secs(20), || {
        recorder
            .rows
            .lock()
            .unwrap()
            .iter()
            .any(|row| row.status == TaskStatus::Complete)
    });
    let row = recorder
        .rows
        .lock()
        .unwrap()
        .iter()
        .find(|row| row.status == TaskStatus::Complete)
        .cloned()
        .unwrap();
    assert_eq!(row.name, "report.pdf");
    let saved = std::fs::read(row.path.unwrap()).unwrap();
    assert_eq!(saved, b"hello from the server");
    let head = requests.lock().unwrap().join("\n").to_ascii_lowercase();
    for expected in [
        "cookie: session=abc",
        "referer: https://example.com/files",
        "user-agent: testbrowser/1.0",
    ] {
        assert!(head.contains(expected), "missing {expected:?} in:\n{head}");
    }

    // With capture off, the browser keeps its download.
    let mut settings = core.settings();
    settings.browser_capture = false;
    core.set_settings(settings).unwrap();
    let rules = {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while socket.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
            line.push(byte[0]);
        }
        serde_json::from_slice::<serde_json::Value>(&line).unwrap()
    };
    assert_eq!(rules["type"], "rules");
    assert_eq!(rules["rules"]["capture"], false);
    let declined = ask(
        &mut socket,
        serde_json::json!({ "id": 3, "type": "download", "source": "download", "items": [{ "url": url }] }),
    );
    assert_eq!(declined["result"], "fallback");
    // A link the user picked still goes to the app.
    let picked = ask(
        &mut socket,
        serde_json::json!({ "id": 4, "type": "download", "source": "link", "items": [{ "url": url }] }),
    );
    assert_eq!(picked["result"], "accepted");

    core.shutdown();
}
