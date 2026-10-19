//! User settings, stored as JSON next to the history database.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What downloads added by hand send when the settings name no user agent
/// and no browser has handed over a download yet: a current Chrome's. Some
/// servers refuse requests that do not look like a browser.
pub const DEFAULT_USER_AGENT: &str = if cfg!(target_os = "macos") {
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36"
} else if cfg!(target_os = "linux") {
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36"
} else {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36"
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(default)]
pub struct Settings {
    pub download_dir: String,
    /// Save each download into its category's folder under `download_dir`.
    pub category_folders: bool,
    pub max_concurrent_downloads: u32,
    pub max_connections_per_server: u32,
    pub split: u32,
    /// aria2 size syntax, e.g. "1M".
    pub min_split_size: String,
    pub categories: Vec<CategoryRule>,
    /// Hand matching browser downloads to OpenDM.
    pub browser_capture: bool,
    /// Lowercase extensions, without the dot, that the browser hands over.
    pub capture_extensions: Vec<String>,
    /// Sites whose downloads stay in the browser; a name also covers its
    /// subdomains.
    pub capture_excluded_sites: Vec<String>,
    /// Smaller files stay in the browser; 0 captures any size.
    pub capture_min_size: u64,
    /// Ask before each new download (the Download File Info dialog);
    /// otherwise downloads start at once.
    pub confirm_downloads: bool,
    /// Show a progress window for each download while it runs.
    pub show_progress_window: bool,
    /// Bytes per second for all downloads together; 0 for no limit.
    pub max_download_speed: u64,
    /// Bytes per second for each download; 0 for no limit.
    pub max_task_speed: u64,
    /// Attempts per server before a download fails; 0 tries forever.
    pub max_tries: u32,
    /// Seconds between attempts.
    pub retry_wait: u32,
    /// Times a download that failed partway is tried again on its own,
    /// minutes apart, without getting further; 0 never.
    pub auto_retries: u32,
    /// Seconds without data before a connection is retried.
    pub timeout: u32,
    pub connect_timeout: u32,
    /// "http://user:password@host:port", or empty for none.
    pub proxy: String,
    /// Hosts reached without the proxy, comma separated.
    pub no_proxy: String,
    /// For downloads the browser did not hand over; empty for the browser's:
    /// the one its last download sent, or a current Chrome's before any.
    pub user_agent: String,
    /// "Name: value" headers sent with every download.
    pub custom_headers: Vec<String>,
    pub file_exists: FileExists,
    /// Any other aria2 options, one "name=value" per line.
    pub engine_extra: String,
    pub schedule: Schedule,
}

/// What to do when the file a download would write already exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum FileExists {
    /// Save under a new name: "report (1).pdf".
    #[default]
    Rename,
    Overwrite,
    /// Stop with an error.
    Fail,
}

/// Starts waiting downloads at a time of day and pauses them at another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    /// "HH:MM", local time.
    pub start: String,
    /// "HH:MM", or empty to let downloads run on.
    pub stop: String,
    /// Days it runs on, 0 for Sunday to 6 for Saturday.
    pub days: Vec<u8>,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: false,
            start: "02:00".into(),
            stop: "08:00".into(),
            days: (0..7).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct CategoryRule {
    pub name: String,
    /// Lowercase extensions without the dot.
    pub extensions: Vec<String>,
    /// Folder relative to the download directory, or an absolute path.
    pub folder: String,
}

/// Name used for files that match no category rule.
pub const OTHER_CATEGORY: &str = "Other";

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: String::new(),
            category_folders: true,
            max_concurrent_downloads: 5,
            max_connections_per_server: 8,
            split: 8,
            min_split_size: "1M".into(),
            categories: default_categories(),
            browser_capture: true,
            capture_extensions: words(DEFAULT_CAPTURE_EXTENSIONS),
            capture_excluded_sites: Vec::new(),
            capture_min_size: 0,
            confirm_downloads: true,
            show_progress_window: true,
            max_download_speed: 0,
            max_task_speed: 0,
            max_tries: 5,
            retry_wait: 5,
            auto_retries: 10,
            timeout: 60,
            connect_timeout: 30,
            proxy: String::new(),
            no_proxy: String::new(),
            user_agent: String::new(),
            custom_headers: Vec::new(),
            file_exists: FileExists::Rename,
            engine_extra: String::new(),
            schedule: Schedule::default(),
        }
    }
}

/// Archives, programs, disk images, media and large documents. Pages,
/// images and plain text stay in the browser, which shows them.
const DEFAULT_CAPTURE_EXTENSIONS: &str = "zip rar 7z tar gz tgz bz2 xz zst lz lzma cab \
    exe msi msix appx dmg pkg iso img deb rpm apk appimage bin \
    mp4 mkv webm avi mov wmv flv m4v mpg mpeg 3gp m3u8 mpd \
    mp3 m4a aac flac wav ogg opus wma aiff \
    pdf doc docx xls xlsx ppt pptx odt ods odp epub";

fn words(list: &str) -> Vec<String> {
    list.split_whitespace().map(String::from).collect()
}

fn rule(name: &str, extensions: &str) -> CategoryRule {
    CategoryRule {
        name: name.into(),
        extensions: words(extensions),
        folder: name.into(),
    }
}

fn default_categories() -> Vec<CategoryRule> {
    vec![
        rule("Compressed", "zip rar 7z tar gz tgz bz2 xz zst lz lzma cab"),
        rule(
            "Documents",
            "pdf doc docx xls xlsx ppt pptx odt ods odp rtf txt md csv epub pages numbers key",
        ),
        rule("Music", "mp3 m4a aac flac wav ogg opus wma aiff alac"),
        rule(
            "Programs",
            "exe msi msix appx dmg pkg iso deb rpm apk appimage",
        ),
        rule(
            "Video",
            "mp4 mkv webm avi mov wmv flv m4v mpg mpeg ts 3gp m3u8 mpd",
        ),
    ]
}

impl Settings {
    /// Reads settings, falling back to defaults for a missing or unreadable file.
    pub fn load(path: &Path, default_download_dir: &str) -> Settings {
        let mut settings = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default();
        if settings.download_dir.is_empty() {
            settings.download_dir = default_download_dir.to_string();
        }
        settings
    }

    /// Writes atomically so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, path)
    }

    /// Global engine options derived from these settings.
    pub fn engine_options(&self) -> Vec<(&'static str, String)> {
        let mut options = vec![
            ("dir", self.download_dir.clone()),
            (
                "max-concurrent-downloads",
                self.max_concurrent_downloads.max(1).to_string(),
            ),
            (
                "max-connection-per-server",
                self.max_connections_per_server.clamp(1, 16).to_string(),
            ),
            ("split", self.split.max(1).to_string()),
            ("min-split-size", self.min_split_size.clone()),
            ("max-overall-download-limit", self.max_download_speed.to_string()),
            ("max-download-limit", self.max_task_speed.to_string()),
            ("max-tries", self.max_tries.to_string()),
            ("retry-wait", self.retry_wait.to_string()),
            ("timeout", self.timeout.max(1).to_string()),
            ("connect-timeout", self.connect_timeout.max(1).to_string()),
            ("all-proxy", self.proxy.trim().to_string()),
            ("no-proxy", self.no_proxy.trim().to_string()),
            ("allow-overwrite", (self.file_exists == FileExists::Overwrite).to_string()),
            ("auto-file-renaming", (self.file_exists == FileExists::Rename).to_string()),
        ];
        if !self.user_agent.trim().is_empty() {
            options.push(("user-agent", self.user_agent.trim().to_string()));
        } else {
            options.push(("user-agent", DEFAULT_USER_AGENT.to_string()));
        }
        // Values go into a config file, one per line.
        for (_, value) in &mut options {
            value.retain(|c| c != '\r' && c != '\n');
        }
        options
    }

    /// The "name=value" lines of `engine_extra`; malformed lines are skipped.
    pub fn extra_engine_options(&self) -> Vec<(String, String)> {
        self.engine_extra
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(name, value)| (name.trim().trim_start_matches("--").to_string(), value.trim().to_string()))
            .filter(|(name, _)| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            })
            .collect()
    }

    /// Valid "Name: value" lines of `custom_headers`.
    pub fn header_lines(&self) -> Vec<String> {
        self.custom_headers
            .iter()
            .map(|header| header.trim().to_string())
            .filter(|header| header.contains(':') && !header.contains(['\r', '\n']))
            .collect()
    }

    /// Category whose extension list contains the file name's extension.
    pub fn classify(&self, file_name: &str) -> String {
        let extension = match file_name.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => extension.to_ascii_lowercase(),
            _ => return OTHER_CATEGORY.into(),
        };
        self.categories
            .iter()
            .find(|rule| rule.extensions.contains(&extension))
            .map(|rule| rule.name.clone())
            .unwrap_or_else(|| OTHER_CATEGORY.into())
    }

    /// Directory a new download of `category` is saved to.
    pub fn directory_for(&self, category: &str) -> PathBuf {
        let base = PathBuf::from(&self.download_dir);
        if !self.category_folders {
            return base;
        }
        match self.categories.iter().find(|rule| rule.name == category) {
            Some(rule) if !rule.folder.is_empty() => base.join(&rule.folder),
            _ => base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension_case_insensitively() {
        let settings = Settings::default();
        assert_eq!(settings.classify("Setup.EXE"), "Programs");
        assert_eq!(settings.classify("archive.tar.gz"), "Compressed");
        assert_eq!(settings.classify("master.m3u8"), "Video");
        assert_eq!(settings.classify(".bashrc"), "Other");
        assert_eq!(settings.classify("README"), "Other");
    }

    #[test]
    fn category_folders_follow_the_setting() {
        let mut settings = Settings {
            download_dir: "/dl".into(),
            ..Settings::default()
        };
        assert_eq!(settings.directory_for("Video"), PathBuf::from("/dl/Video"));
        assert_eq!(settings.directory_for("Other"), PathBuf::from("/dl"));
        settings.category_folders = false;
        assert_eq!(settings.directory_for("Video"), PathBuf::from("/dl"));
    }

    #[test]
    fn missing_fields_take_defaults_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{ "split": 3 }"#).unwrap();
        let mut settings = Settings::load(&path, "/Users/me/Downloads");
        assert_eq!(settings.split, 3);
        assert_eq!(settings.download_dir, "/Users/me/Downloads");
        assert_eq!(settings.categories.len(), 5);

        settings.max_concurrent_downloads = 9;
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path, "/elsewhere"), settings);
    }

    #[test]
    fn advanced_settings_become_engine_options() {
        let settings = Settings {
            max_download_speed: 1_048_576,
            proxy: "http://user:pw@proxy:8080\n".into(),
            file_exists: FileExists::Overwrite,
            engine_extra: "# comment\n--lowest-speed-limit=10K\nbad line\nUPPER=1\nsplit = 4\n".into(),
            custom_headers: vec!["X-Token: 1".into(), "no colon".into()],
            ..Settings::default()
        };
        let options: std::collections::HashMap<_, _> = settings.engine_options().into_iter().collect();
        assert_eq!(options["max-overall-download-limit"], "1048576");
        assert_eq!(options["all-proxy"], "http://user:pw@proxy:8080");
        assert_eq!(options["allow-overwrite"], "true");
        assert_eq!(options["auto-file-renaming"], "false");
        assert_eq!(options["user-agent"], DEFAULT_USER_AGENT);
        assert_eq!(
            settings.extra_engine_options(),
            vec![("lowest-speed-limit".into(), "10K".into()), ("split".into(), "4".into())]
        );
        assert_eq!(settings.header_lines(), vec!["X-Token: 1".to_string()]);
    }
}
