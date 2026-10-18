//! Runs aria2-next as a child process.
//!
//! Each launch gets a free loopback port and a fresh random secret, passed in
//! a private config file rather than on the command line. Unfinished tasks
//! survive through the session file. A crashed engine is restarted, up to
//! `CRASH_BUDGET` times per `CRASH_WINDOW`, then the failure is reported.

use crate::rpc::{Connection, Rpc};
use crate::settings::Settings;
use crate::{EngineState, Shared, tasks};
use std::collections::VecDeque;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

const CRASH_BUDGET: usize = 3;
const CRASH_WINDOW: Duration = Duration::from_secs(300);
const START_TIMEOUT: Duration = Duration::from_secs(10);

enum Exit {
    Shutdown,
    Failed(String),
}

pub async fn supervise(shared: Arc<Shared>) {
    let mut shutdown = shared.shutdown.subscribe();
    let mut crashes: VecDeque<Instant> = VecDeque::new();
    shared.set_engine_state(EngineState::Starting);
    loop {
        let reason = match run_once(&shared, &mut shutdown).await {
            Exit::Shutdown => break,
            Exit::Failed(reason) => reason,
        };
        *shared.engine_pid.lock().unwrap() = None;

        let now = Instant::now();
        crashes.push_back(now);
        while crashes
            .front()
            .is_some_and(|at| now.duration_since(*at) > CRASH_WINDOW)
        {
            crashes.pop_front();
        }
        if crashes.len() > CRASH_BUDGET {
            shared.set_engine_state(EngineState::Failed { reason });
            tokio::select! {
                _ = shared.restart.notified() => {
                    crashes.clear();
                    shared.set_engine_state(EngineState::Starting);
                    continue;
                }
                _ = crate::stopped(&mut shutdown) => break,
            }
        }
        shared.set_engine_state(EngineState::Restarting { reason });
        let delay = Duration::from_millis(500 * crashes.len() as u64);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = crate::stopped(&mut shutdown) => break,
        }
    }
    *shared.engine_pid.lock().unwrap() = None;
    shared.set_engine_state(EngineState::Stopped);
}

async fn run_once(shared: &Arc<Shared>, shutdown: &mut tokio::sync::watch::Receiver<bool>) -> Exit {
    let settings = shared.settings.lock().unwrap().clone();
    let (mut child, connection) = match launch(shared, &settings).await {
        Ok(launched) => launched,
        Err(reason) => return Exit::Failed(reason),
    };
    let Connection {
        rpc,
        mut notifications,
        closed,
    } = connection;
    *shared.engine_pid.lock().unwrap() = child.id();

    if let Err(error) = tasks::sync_all(shared, &rpc).await {
        let _ = child.kill().await;
        return Exit::Failed(format!("Could not read the engine's downloads: {error}"));
    }
    shared.rpc.send_replace(Some(rpc.clone()));
    shared.set_engine_state(EngineState::Running);
    let extra = shared.settings.lock().unwrap().extra_engine_options();
    apply_extra_options(shared, &rpc, extra).await;

    let forward = tokio::spawn({
        let shared = shared.clone();
        async move {
            while let Some(notification) = notifications.recv().await {
                shared.tasks.lock().unwrap().mark_dirty(notification.gid);
                shared.wake.notify_one();
            }
        }
    });

    let exit = tokio::select! {
        status = child.wait() => Exit::Failed(describe_exit(status, &engine_dir(shared))),
        _ = closed => {
            let _ = child.kill().await;
            Exit::Failed("Lost the connection to the engine".into())
        }
        _ = crate::stopped(shutdown) => {
            stop_gracefully(&rpc, &mut child).await;
            Exit::Shutdown
        }
    };
    forward.abort();
    shared.rpc.send_replace(None);
    exit
}

fn engine_dir(shared: &Shared) -> std::path::PathBuf {
    shared.data_dir.join("engine")
}

async fn launch(shared: &Shared, settings: &Settings) -> Result<(Child, Connection), String> {
    let dir = engine_dir(shared);
    let state_dir = dir.join("state");
    let session = dir.join("session.txt");
    std::fs::create_dir_all(&state_dir).map_err(|error| error.to_string())?;
    // aria2-next refuses to start when the input file does not exist yet.
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&session)
        .map_err(|error| error.to_string())?;

    let port = free_port()?;
    let secret = random_hex(16)?;
    let conf = dir.join("rpc.conf");
    // The settings go in the private file as well: a proxy password would
    // show in the process list on the command line.
    let mut contents = format!("rpc-secret={secret}\nrpc-listen-port={port}\n");
    for (key, value) in settings.engine_options() {
        contents.push_str(&format!("{key}={value}\n"));
    }
    write_private(&conf, &contents)
        .map_err(|error| format!("Cannot write {}: {error}", conf.display()))?;
    let stderr =
        std::fs::File::create(dir.join("stderr.log")).map_err(|error| error.to_string())?;

    let mut command = Command::new(&shared.engine_path);
    command
        .arg(format!("--conf-path={}", conf.display()))
        .args([
            "--enable-rpc=true",
            "--rpc-listen-all=false",
            "--quiet=true",
            "--continue=true",
            "--auto-file-renaming-style=parentheses",
            "--save-session-interval=30",
            "--log-level=warn",
        ])
        .arg(format!("--stop-with-process={}", std::process::id()))
        .arg(format!("--state-dir={}", state_dir.display()))
        .arg(format!("--input-file={}", session.display()))
        .arg(format!("--save-session={}", session.display()))
        .arg(format!("--log={}", dir.join("engine.log").display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command
        .spawn()
        .map_err(|error| format!("Cannot start {}: {error}", shared.engine_path.display()))?;

    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = std::fs::remove_file(&conf);
            return Err(describe_exit(Ok(status), &dir));
        }
        let attempt = match Rpc::connect(port, &secret).await {
            Ok(connection) => connection
                .rpc
                .call("aria2.getVersion", vec![])
                .await
                .map(|_| connection),
            Err(error) => Err(error),
        };
        match attempt {
            Ok(connection) => {
                let _ = std::fs::remove_file(&conf);
                return Ok((child, connection));
            }
            Err(error) if Instant::now() > deadline => {
                let _ = child.kill().await;
                let _ = std::fs::remove_file(&conf);
                return Err(format!("The engine did not answer: {error}"));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

async fn stop_gracefully(rpc: &Rpc, child: &mut Child) {
    let quick = Duration::from_secs(2);
    let _ = tokio::time::timeout(quick, rpc.call("aria2.saveSession", vec![])).await;
    let _ = tokio::time::timeout(quick, rpc.call("aria2.shutdown", vec![])).await;
    if tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}

fn describe_exit(status: std::io::Result<ExitStatus>, dir: &Path) -> String {
    let status = match status {
        Ok(status) => format!("The engine exited ({status})"),
        Err(error) => format!("The engine stopped: {error}"),
    };
    let last_line = std::fs::read_to_string(dir.join("stderr.log"))
        .ok()
        .and_then(|log| {
            log.lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(str::to_owned)
        });
    match last_line {
        Some(line) => format!("{status}: {line}"),
        None => status,
    }
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| format!("No free local port: {error}"))
}

pub fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|error| error.to_string())?;
    Ok(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The user's own aria2 options, applied to the running engine so that a
/// mistake in them is reported rather than keeping the engine from starting.
pub async fn apply_extra_options(shared: &Shared, rpc: &Rpc, options: Vec<(String, String)>) {
    for (name, value) in options {
        let option = serde_json::json!({ name.clone(): value });
        if let Err(error) = rpc.call("aria2.changeGlobalOption", vec![option]).await {
            shared
                .listener
                .error(format!("The engine did not accept the option {name}: {error}"));
        }
    }
}

fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(contents.as_bytes())
}
