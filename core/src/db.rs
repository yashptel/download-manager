//! SQLite history: one row per download, keyed by the aria2 GID.
//!
//! Rows are written on status changes and at shutdown, never per progress
//! tick. A database from a newer release is refused rather than modified.

use crate::tasks::Task;
use crate::{TaskRow, TaskStatus};
use rusqlite::{Connection, params};
use std::path::Path;

const SCHEMA_VERSION: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE downloads (
    id          TEXT PRIMARY KEY,
    url         TEXT NOT NULL,
    name        TEXT NOT NULL,
    dir         TEXT NOT NULL,
    path        TEXT,
    category    TEXT NOT NULL,
    status      TEXT NOT NULL,
    total_bytes INTEGER NOT NULL,
    done_bytes  INTEGER NOT NULL,
    error       TEXT,
    added_at    INTEGER NOT NULL,
    finished_at INTEGER,
    files       TEXT NOT NULL,
    media       INTEGER NOT NULL,
    move_dir    TEXT,
    move_name   TEXT,
    page        TEXT
);
CREATE INDEX downloads_added_at ON downloads(added_at);
";

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db, String> {
        let conn = Connection::open(path)
            .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
            .map_err(|error| error.to_string())?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        match version {
            0 => conn
                .execute_batch(&format!(
                    "BEGIN; {SCHEMA} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
                ))
                .map_err(|error| error.to_string())?,
            // 2 adds the folder and name a running download moves to when
            // done; 3 the page it came from.
            1 | 2 => conn
                .execute_batch(&format!(
                    "BEGIN; {} ALTER TABLE downloads ADD COLUMN page TEXT; PRAGMA user_version = 3; COMMIT;",
                    if version == 1 {
                        "ALTER TABLE downloads ADD COLUMN move_dir TEXT; ALTER TABLE downloads ADD COLUMN move_name TEXT;"
                    } else {
                        ""
                    }
                ))
                .map_err(|error| error.to_string())?,
            SCHEMA_VERSION => {}
            newer => {
                return Err(format!(
                    "The download history was written by a newer version (schema {newer}). Update the app to open it."
                ));
            }
        }
        Ok(Db { conn })
    }

    pub fn load(&self) -> Result<Vec<Task>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, url, name, dir, path, category, status, total_bytes, done_bytes, error,
                        added_at, finished_at, files, media, move_dir, move_name, page
                 FROM downloads ORDER BY added_at, id",
            )
            .map_err(|error| error.to_string())?;
        let tasks = statement
            .query_map([], |row| {
                let status: String = row.get(6)?;
                let total: i64 = row.get(7)?;
                let done: i64 = row.get(8)?;
                let status = status_from_str(&status);
                let files: String = row.get(12)?;
                let row_data = TaskRow {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    name: row.get(2)?,
                    dir: row.get(3)?,
                    path: row.get(4)?,
                    category: row.get(5)?,
                    status,
                    total_bytes: total as u64,
                    done_bytes: done as u64,
                    speed: 0,
                    progress: progress(status, total as u64, done as u64),
                    eta_secs: None,
                    connections: 0,
                    // Older rows hold the engine's own wording.
                    error: row.get::<_, Option<String>>(9)?.map(|error| crate::tasks::friendly_error(&error)),
                    added_at: row.get(10)?,
                    finished_at: row.get(11)?,
                };
                Ok(Task {
                    row: row_data,
                    files: serde_json::from_str(&files).unwrap_or_default(),
                    media: row.get::<_, i64>(13)? != 0,
                    move_dir: row.get(14)?,
                    move_name: row.get(15)?,
                    page: row.get(16)?,
                    cookies_refreshed: false,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(tasks)
    }

    /// Inserts or replaces the given tasks in one transaction.
    pub fn save<'a>(&mut self, tasks: impl IntoIterator<Item = &'a Task>) -> Result<(), String> {
        let transaction = self.conn.transaction().map_err(|error| error.to_string())?;
        {
            let mut statement = transaction
                .prepare_cached(
                    "INSERT OR REPLACE INTO downloads
                     (id, url, name, dir, path, category, status, total_bytes, done_bytes, error,
                      added_at, finished_at, files, media, move_dir, move_name, page)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                )
                .map_err(|error| error.to_string())?;
            for task in tasks {
                let row = &task.row;
                statement
                    .execute(params![
                        row.id,
                        row.url,
                        row.name,
                        row.dir,
                        row.path,
                        row.category,
                        status_to_str(row.status),
                        row.total_bytes as i64,
                        row.done_bytes as i64,
                        row.error,
                        row.added_at,
                        row.finished_at,
                        serde_json::to_string(&task.files).unwrap_or_else(|_| "[]".into()),
                        task.media as i64,
                        task.move_dir,
                        task.move_name,
                        task.page,
                    ])
                    .map_err(|error| error.to_string())?;
            }
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn delete(&mut self, ids: &[String]) -> Result<(), String> {
        let transaction = self.conn.transaction().map_err(|error| error.to_string())?;
        for id in ids {
            transaction
                .execute("DELETE FROM downloads WHERE id = ?1", [id])
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }
}

pub fn progress(status: TaskStatus, total: u64, done: u64) -> f64 {
    match status {
        TaskStatus::Complete => 1.0,
        _ if total > 0 => done as f64 / total as f64,
        _ => -1.0,
    }
}

fn status_to_str(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Downloading => "downloading",
        TaskStatus::Queued => "queued",
        TaskStatus::Paused => "paused",
        TaskStatus::Complete => "complete",
        TaskStatus::Failed => "failed",
    }
}

fn status_from_str(status: &str) -> TaskStatus {
    match status {
        "downloading" => TaskStatus::Downloading,
        "queued" => TaskStatus::Queued,
        "paused" => TaskStatus::Paused,
        "complete" => TaskStatus::Complete,
        _ => TaskStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, added_at: i64, status: TaskStatus) -> Task {
        let mut task = Task::new(
            id.into(),
            "https://example.com/a%20b.zip".into(),
            "/dl".into(),
            added_at,
        );
        task.row.status = status;
        task.row.total_bytes = 100;
        task.row.done_bytes = 40;
        task.row.progress = progress(status, 100, 40);
        task.files = vec!["/dl/a b.zip".into()];
        task
    }

    #[test]
    fn saves_loads_and_deletes_in_added_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        let mut db = Db::open(&path).unwrap();
        let first = task("00000000000000a1", 10, TaskStatus::Paused);
        let second = task("00000000000000a2", 20, TaskStatus::Complete);
        db.save([&second, &first]).unwrap();
        drop(db);

        let mut db = Db::open(&path).unwrap();
        let loaded = db.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].row, first.row);
        assert_eq!(loaded[0].files, vec!["/dl/a b.zip".to_string()]);
        assert_eq!(loaded[0].row.progress, 0.4);
        assert_eq!(loaded[1].row.progress, 1.0);

        db.delete(&["00000000000000a1".into()]).unwrap();
        assert_eq!(db.load().unwrap().len(), 1);
    }

    #[test]
    fn opens_a_first_release_history_and_keeps_its_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        let v1 = SCHEMA.replace(",\n    move_dir    TEXT,\n    move_name   TEXT,\n    page        TEXT", "");
        assert!(!v1.contains("move_dir"));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&format!("{v1} PRAGMA user_version = 1;")).unwrap();
        conn.execute(
            "INSERT INTO downloads VALUES ('00000000000000b1', 'https://example.com/x.zip', 'x.zip',
             '/dl', NULL, 'Compressed', 'paused', 100, 40, NULL, 5, NULL, '[]', 0)",
            [],
        )
        .unwrap();
        drop(conn);

        let mut db = Db::open(&path).unwrap();
        let mut loaded = db.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].row.name, "x.zip");
        assert_eq!(loaded[0].move_dir, None);
        loaded[0].move_name = Some("renamed.zip".into());
        db.save(&loaded).unwrap();
        assert_eq!(db.load().unwrap()[0].move_name.as_deref(), Some("renamed.zip"));
    }

    #[test]
    fn refuses_a_database_from_a_newer_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("PRAGMA user_version = 99")
            .unwrap();
        let error = Db::open(&path).err().unwrap();
        assert!(error.contains("newer version"), "{error}");
    }
}
