//! Native messaging host. A browser starts it when the OpenDM extension
//! connects and stops it when the extension disconnects. It relays messages
//! both ways: length-prefixed JSON with the browser on stdin and stdout, one
//! JSON message per line with the app on its local socket.
//!
//! The extension's first message decides what happens when the app is not
//! running: with `"start": true` (it has a download to deliver) the host
//! starts the app in the background and waits for its socket; otherwise it
//! answers that the app is not running and exits.

use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// Largest message accepted from the browser.
const MAX_FROM_BROWSER: usize = 64 << 20;
/// Chromium rejects host messages over 1 MB.
const MAX_TO_BROWSER: usize = 1 << 20;
/// How long a starting app has to open its socket.
const START_TIMEOUT: Duration = Duration::from_secs(20);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let code = match run().await {
        Ok(()) => 0,
        Err((code, error)) => {
            let message = json!({ "type": "error", "code": code, "error": error });
            let _ = write_message(&mut tokio::io::stdout(), &message.to_string()).await;
            1
        }
    };
    // A blocked stdin read would otherwise hold the runtime open.
    std::process::exit(code);
}

/// An error code the extension can act on, and a message for people.
type Failure = (&'static str, String);

async fn run() -> Result<(), Failure> {
    let failed = |error: String| ("failed", error);
    let mut stdin = tokio::io::stdin();
    let Some(first) = read_message(&mut stdin).await.map_err(failed)? else {
        return Ok(());
    };
    let first = to_line(&first).map_err(failed)?;
    let may_start = serde_json::from_str::<Value>(&first)
        .ok()
        .and_then(|message| message["start"].as_bool())
        == Some(true);

    let data_dir = opendm_host::data_dir().ok_or(("failed", "No home folder".to_string()))?;
    let endpoint = opendm_host::endpoint(&data_dir);
    let stream = match platform::connect(&endpoint).await {
        Ok(stream) => stream,
        Err(_) if !may_start => return Err(("not-running", "OpenDM is not running".into())),
        Err(_) => start_and_connect(&endpoint).await.map_err(failed)?,
    };
    let (from_app, mut to_app) = tokio::io::split(stream);

    let browser_to_app = async {
        let mut line = first;
        loop {
            to_app
                .write_all(line.as_bytes())
                .await
                .map_err(|error| format!("OpenDM stopped listening: {error}"))?;
            match read_message(&mut stdin).await? {
                Some(message) => line = to_line(&message)?,
                None => return Ok(()), // The browser closed the port.
            }
        }
    };
    let app_to_browser = async {
        let mut lines = BufReader::new(from_app).lines();
        let mut stdout = tokio::io::stdout();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.len() <= MAX_TO_BROWSER {
                write_message(&mut stdout, &line)
                    .await
                    .map_err(|error| format!("The browser stopped listening: {error}"))?;
            }
        }
        Err("OpenDM closed the connection".to_string())
    };
    tokio::select! {
        result = browser_to_app => result,
        result = app_to_browser => result,
    }
    .map_err(failed)
}

/// One message per line for the app; re-encoding drops any line breaks.
fn to_line(message: &[u8]) -> Result<String, String> {
    let value: Value = serde_json::from_slice(message)
        .map_err(|error| format!("Not JSON from the browser: {error}"))?;
    let mut line = value.to_string();
    line.push('\n');
    Ok(line)
}

/// Reads one native message: a 32-bit length in native byte order, then JSON.
async fn read_message(input: &mut (impl AsyncRead + Unpin)) -> Result<Option<Vec<u8>>, String> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("Could not read from the browser: {error}")),
    }
    let length = u32::from_ne_bytes(length) as usize;
    if length > MAX_FROM_BROWSER {
        return Err(format!("A {length}-byte message is too large"));
    }
    let mut message = vec![0; length];
    input
        .read_exact(&mut message)
        .await
        .map_err(|error| format!("Could not read from the browser: {error}"))?;
    Ok(Some(message))
}

async fn write_message(
    output: &mut (impl AsyncWrite + Unpin),
    message: &str,
) -> std::io::Result<()> {
    output
        .write_all(&(message.len() as u32).to_ne_bytes())
        .await?;
    output.write_all(message.as_bytes()).await?;
    output.flush().await
}

async fn start_and_connect(endpoint: &Path) -> Result<platform::Stream, String> {
    platform::start_app().map_err(|error| format!("Could not start OpenDM: {error}"))?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        match platform::connect(endpoint).await {
            Ok(stream) => return Ok(stream),
            Err(_) if Instant::now() < deadline => continue,
            Err(error) => return Err(format!("OpenDM did not start in time: {error}")),
        }
    }
}

/// The app to start: OPENDM_APP during development, otherwise the app this
/// host ships inside (macOS) or next to (Windows).
fn app_path() -> std::io::Result<std::path::PathBuf> {
    if let Some(app) = std::env::var_os("OPENDM_APP") {
        return Ok(app.into());
    }
    let exe = std::env::current_exe()?;
    platform::bundled_app(&exe)
        .ok_or_else(|| std::io::Error::other(format!("No OpenDM app next to {}", exe.display())))
}

#[cfg(unix)]
mod platform {
    use std::path::{Path, PathBuf};

    pub type Stream = tokio::net::UnixStream;

    pub async fn connect(endpoint: &Path) -> std::io::Result<Stream> {
        tokio::net::UnixStream::connect(endpoint).await
    }

    /// .../OpenDM.app/Contents/MacOS/opendm-host -> .../OpenDM.app
    pub fn bundled_app(exe: &Path) -> Option<PathBuf> {
        exe.ancestors()
            .find(|path| path.extension().is_some_and(|extension| extension == "app"))
            .map(Path::to_path_buf)
    }

    pub fn start_app() -> std::io::Result<()> {
        let app = super::app_path()?;
        // -g keeps the browser in front; the app starts in the menu bar only.
        let mut open = std::process::Command::new("/usr/bin/open");
        open.arg("-g");
        // open drops the environment; a development copy keeps its data folder.
        if let Some(dir) = std::env::var_os("OPENDM_DATA_DIR") {
            open.arg("--env")
                .arg(format!("OPENDM_DATA_DIR={}", dir.to_string_lossy()));
        }
        let status = open.arg(&app).args(["--args", "--background"]).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "open {} failed: {status}",
                app.display()
            )))
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

    pub type Stream = NamedPipeClient;

    const ERROR_PIPE_BUSY: i32 = 231;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    pub async fn connect(endpoint: &Path) -> std::io::Result<Stream> {
        loop {
            match ClientOptions::new().open(endpoint) {
                // Every instance is taken while the app accepts another one.
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                result => return result,
            }
        }
    }

    pub fn bundled_app(exe: &Path) -> Option<PathBuf> {
        Some(exe.parent()?.join("OpenDM.exe"))
    }

    pub fn start_app() -> std::io::Result<()> {
        let app = super::app_path()?;
        let start = |flags: u32| {
            std::process::Command::new(&app)
                .arg("--background")
                .creation_flags(flags)
                .spawn()
                .map(drop)
        };
        // Leave the browser's job object, if it allows that, so closing the
        // browser does not close OpenDM.
        start(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB)
            .or_else(|_| start(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_messages_round_trip() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, r#"{"type":"hello"}"#)
            .await
            .unwrap();
        write_message(&mut buffer, "{}").await.unwrap();
        let mut input = buffer.as_slice();
        assert_eq!(
            read_message(&mut input).await.unwrap().unwrap(),
            br#"{"type":"hello"}"#
        );
        assert_eq!(read_message(&mut input).await.unwrap().unwrap(), b"{}");
        assert_eq!(read_message(&mut input).await.unwrap(), None);
    }
}
