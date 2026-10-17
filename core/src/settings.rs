//! User settings, stored as JSON next to the history database.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
        }
    }
}

fn rule(name: &str, extensions: &str) -> CategoryRule {
    CategoryRule {
        name: name.into(),
        extensions: extensions.split_whitespace().map(String::from).collect(),
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
        vec![
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
        ]
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
}
