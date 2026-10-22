//! What the app and the native messaging host must agree on: where the app
//! listens for the browser, and the manifests that let browsers start the
//! host. The relay itself is in `main.rs`.

use std::io;
use std::path::{Path, PathBuf};

/// The native messaging host name the extension connects to.
pub const HOST_NAME: &str = "org.opendm.host";

/// Chromium extension IDs allowed to start the host: the development ID
/// fixed by the public key in `extension/manifest.json`, then the Chrome Web
/// Store's.
pub const CHROMIUM_EXTENSION_IDS: &[&str] = &[
    "nkhhoiejpomapjkeomdbflaicnnjclfj",
    "mjebgoilgooegcpfbmjnapamjpcnhcbg",
];

/// The Firefox add-on ID from `browser_specific_settings` in the manifest.
pub const FIREFOX_EXTENSION_ID: &str = "connect@opendm.org";

/// The app's data folder: OPENDM_DATA_DIR, or the platform default the apps use.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OPENDM_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("OpenDM"))
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support/OpenDM"))
    }
}

/// Where the app listens: a Unix socket in the data folder, or on Windows a
/// named pipe whose name is derived from the data folder, so a development
/// copy with its own OPENDM_DATA_DIR gets its own pipe.
pub fn endpoint(data_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        let key = data_dir.to_string_lossy().to_lowercase();
        PathBuf::from(format!(
            r"\\.\pipe\opendm-browser-{:016x}",
            fnv1a(key.as_bytes())
        ))
    } else {
        data_dir.join("browser.sock")
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The manifest that lets a browser start `host_exe`.
pub fn manifest(host_exe: &Path, firefox: bool) -> String {
    let mut manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "OpenDM",
        "path": host_exe.to_string_lossy(),
        "type": "stdio",
    });
    if firefox {
        manifest["allowed_extensions"] = serde_json::json!([FIREFOX_EXTENSION_ID]);
    } else {
        manifest["allowed_origins"] = CHROMIUM_EXTENSION_IDS
            .iter()
            .map(|id| format!("chrome-extension://{id}/"))
            .collect();
    }
    serde_json::to_string_pretty(&manifest).unwrap()
}

/// Registers the host with every installed browser that supports it. Runs
/// at each app launch, so moving the app fixes the paths on the next start.
/// Returns the manifests it wrote.
pub fn install(host_exe: &Path, data_dir: &Path) -> io::Result<Vec<PathBuf>> {
    platform::install(host_exe, data_dir)
}

/// Writes only when the contents changed, to leave timestamps alone.
fn write_if_changed(path: &Path, contents: &str) -> io::Result<bool> {
    if std::fs::read_to_string(path).is_ok_and(|current| current == contents) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(true)
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    /// Browser support folders under ~/Library/Application Support, and
    /// whether the browser is Firefox.
    const BROWSERS: &[(&str, bool)] = &[
        ("Google/Chrome", false),
        ("Google/Chrome Beta", false),
        ("Google/Chrome Dev", false),
        ("Google/Chrome Canary", false),
        ("Chromium", false),
        ("Microsoft Edge", false),
        ("Microsoft Edge Beta", false),
        ("Microsoft Edge Dev", false),
        ("Microsoft Edge Canary", false),
        ("BraveSoftware/Brave-Browser", false),
        ("Vivaldi", false),
        ("Mozilla", true),
    ];

    pub fn install(host_exe: &Path, _data_dir: &Path) -> io::Result<Vec<PathBuf>> {
        let Some(home) = std::env::var_os("HOME") else {
            return Ok(Vec::new());
        };
        let support = PathBuf::from(home).join("Library/Application Support");
        let mut written = Vec::new();
        for (folder, firefox) in BROWSERS {
            let browser = support.join(folder);
            // Only browsers that are installed and have run.
            if !browser.is_dir() {
                continue;
            }
            let path = browser
                .join("NativeMessagingHosts")
                .join(format!("{HOST_NAME}.json"));
            write_if_changed(&path, &manifest(host_exe, *firefox))?;
            written.push(path);
        }
        Ok(written)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// Registry keys under HKCU that point a browser at a host manifest, and
    /// whether the browser is Firefox.
    const BROWSERS: &[(&str, bool)] = &[
        (r"Software\Google\Chrome\NativeMessagingHosts", false),
        (r"Software\Chromium\NativeMessagingHosts", false),
        (r"Software\Microsoft\Edge\NativeMessagingHosts", false),
        (
            r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
            false,
        ),
        (r"Software\Vivaldi\NativeMessagingHosts", false),
        (r"Software\Mozilla\NativeMessagingHosts", true),
    ];

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn install(host_exe: &Path, data_dir: &Path) -> io::Result<Vec<PathBuf>> {
        let folder = data_dir.join("browser");
        let chromium = folder.join(format!("{HOST_NAME}.json"));
        let firefox = folder.join(format!("{HOST_NAME}.firefox.json"));
        write_if_changed(&chromium, &manifest(host_exe, false))?;
        write_if_changed(&firefox, &manifest(host_exe, true))?;
        for (key, is_firefox) in BROWSERS {
            let target = if *is_firefox { &firefox } else { &chromium };
            // reg.exe keeps this free of a registry crate; it runs once per launch.
            let status = Command::new("reg")
                .args([
                    "add",
                    &format!(r"HKCU\{key}\{HOST_NAME}"),
                    "/ve",
                    "/t",
                    "REG_SZ",
                    "/d",
                ])
                .arg(target)
                .arg("/f")
                .creation_flags(CREATE_NO_WINDOW)
                .status()?;
            if !status.success() {
                return Err(io::Error::other(format!("reg add {key} failed: {status}")));
            }
        }
        Ok(vec![chromium, firefox])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_name_the_host_and_its_extensions() {
        let host = Path::new("/Applications/OpenDM.app/Contents/MacOS/opendm-host");
        let chromium: serde_json::Value = serde_json::from_str(&manifest(host, false)).unwrap();
        assert_eq!(chromium["name"], "org.opendm.host");
        assert_eq!(
            chromium["path"],
            "/Applications/OpenDM.app/Contents/MacOS/opendm-host"
        );
        assert_eq!(
            chromium["allowed_origins"],
            serde_json::json!([
                "chrome-extension://nkhhoiejpomapjkeomdbflaicnnjclfj/",
                "chrome-extension://mjebgoilgooegcpfbmjnapamjpcnhcbg/",
                "chrome-extension://kdehjojoelodooodmmpaaalkaoolkfal/"
            ])
        );
        let firefox: serde_json::Value = serde_json::from_str(&manifest(host, true)).unwrap();
        assert_eq!(
            firefox["allowed_extensions"],
            serde_json::json!(["connect@opendm.org"])
        );
        assert!(firefox.get("allowed_origins").is_none());
    }

    #[test]
    fn the_endpoint_follows_the_data_folder() {
        let a = endpoint(Path::new("/tmp/a"));
        let b = endpoint(Path::new("/tmp/b"));
        assert_ne!(a, b);
        if !cfg!(windows) {
            assert_eq!(a, PathBuf::from("/tmp/a/browser.sock"));
        }
    }
}
