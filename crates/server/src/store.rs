//! SQLite index of stored videos.
//!
//! The index holds the facts about each video; the video bytes live in files named after
//! their content hash (see [`crate::media`]). Schema changes are numbered migrations applied
//! in order and recorded in SQLite's `user_version`, so a data folder from an older version is
//! upgraded in place and one from a newer version is refused instead of corrupted.

use std::path::Path;
use std::sync::Mutex;

use axonhand_api::{GateCheck, Hand, Video};
use rusqlite::{params, Connection, OptionalExtension, Row};

/// Each entry upgrades the schema by one version. Never edit an entry that has shipped.
const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE videos (
        id                TEXT PRIMARY KEY,
        filename          TEXT NOT NULL,
        participant_code  TEXT NOT NULL,
        visit_date        TEXT NOT NULL,
        hand              TEXT NOT NULL CHECK (hand IN ('left', 'right')),
        uploaded_at       TEXT NOT NULL,
        size_bytes        INTEGER NOT NULL,
        width             INTEGER NOT NULL,
        height            INTEGER NOT NULL,
        fps               REAL NOT NULL,
        duration_seconds  REAL NOT NULL,
        frame_count       INTEGER NOT NULL,
        codec             TEXT NOT NULL,
        extension         TEXT NOT NULL,
        gate_json         TEXT NOT NULL
    );
    CREATE INDEX videos_by_participant ON videos (participant_code);
    CREATE INDEX videos_by_upload ON videos (uploaded_at);
",
    "
    CREATE TABLE jobs (
        id           TEXT PRIMARY KEY,
        state        TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed')),
        result_key   TEXT NOT NULL,
        video_id     TEXT NOT NULL,
        test_id      TEXT NOT NULL,
        request_json TEXT NOT NULL,
        step         TEXT NOT NULL DEFAULT '',
        progress     REAL NOT NULL DEFAULT 0,
        error        TEXT,
        created_at   TEXT NOT NULL,
        seq          INTEGER NOT NULL
    );
    CREATE INDEX jobs_by_state ON jobs (state, seq);
    CREATE INDEX jobs_by_key ON jobs (result_key);
    CREATE TABLE results (
        key          TEXT PRIMARY KEY,
        video_id     TEXT NOT NULL,
        test_id      TEXT NOT NULL,
        test_version TEXT NOT NULL,
        created_at   TEXT NOT NULL
    );
    CREATE INDEX results_by_video ON results (video_id);
",
    "
    CREATE TABLE scores (
        result_key  TEXT NOT NULL,
        rater       TEXT NOT NULL,
        score       INTEGER NOT NULL CHECK (score BETWEEN 0 AND 4),
        note        TEXT NOT NULL,
        recorded_at TEXT NOT NULL,
        PRIMARY KEY (result_key, rater)
    );
",
];

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite reported an error.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    /// The data folder was written by a newer version of the program.
    #[error("the index is version {found}, newer than this program understands ({supported})")]
    TooNew {
        /// Version found in the file.
        found: usize,
        /// Newest version this program can read.
        supported: usize,
    },
    /// A stored row could not be read back as a video.
    #[error("stored record is damaged: {0}")]
    Damaged(String),
    /// The lock was poisoned by a panic in another thread.
    #[error("the index lock is poisoned")]
    Poisoned,
}

/// Filters for [`Store::list`]. Empty means no filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoFilter {
    /// Only videos of this participant.
    pub participant_code: Option<String>,
    /// Only videos of this hand.
    pub hand: Option<Hand>,
}

/// A stored video together with the file extension its bytes are saved under.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredVideo {
    /// The record shown to clients.
    pub video: Video,
    /// File extension without the dot, for example `mp4`.
    pub extension: String,
}

/// Handle to the index. Cheap to share behind an `Arc`; calls block, so async code runs them
/// with `spawn_blocking`.
pub struct Store {
    connection: Mutex<Connection>,
}

fn hand_text(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "left",
        Hand::Right => "right",
    }
}

impl Store {
    /// Open (creating if needed) the index at `path` and bring it up to date.
    ///
    /// # Errors
    ///
    /// [`StoreError`] if the file cannot be opened or is from a newer version.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// An index held in memory, for tests.
    ///
    /// # Errors
    ///
    /// [`StoreError`] if SQLite cannot start.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// The connection, for the sibling modules that add their own queries.
    pub(crate) fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::Poisoned)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, StoreError> {
        let stored: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let version = usize::try_from(stored).unwrap_or(usize::MAX);
        if version > MIGRATIONS.len() {
            return Err(StoreError::TooNew {
                found: version,
                supported: MIGRATIONS.len(),
            });
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let transaction = connection.transaction()?;
            transaction.execute_batch(sql)?;
            transaction.pragma_update(
                None,
                "user_version",
                i64::try_from(index + 1).unwrap_or(i64::MAX),
            )?;
            transaction.commit()?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// Add a video. Returns `false` if a video with the same id is already stored, leaving the
    /// existing record untouched.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn insert(&self, video: &Video, extension: &str) -> Result<bool, StoreError> {
        let gate =
            serde_json::to_string(&video.gate).map_err(|e| StoreError::Damaged(e.to_string()))?;
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let rows = connection.execute(
            "INSERT OR IGNORE INTO videos (id, filename, participant_code, visit_date, hand,
                uploaded_at, size_bytes, width, height, fps, duration_seconds, frame_count,
                codec, extension, gate_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                video.id,
                video.filename,
                video.participant_code,
                video.visit_date,
                hand_text(video.hand),
                video.uploaded_at,
                i64::try_from(video.size_bytes).unwrap_or(i64::MAX),
                video.width,
                video.height,
                video.fps,
                video.duration_seconds,
                video.frame_count,
                video.codec,
                extension,
                gate,
            ],
        )?;
        Ok(rows == 1)
    }

    /// Look up one video by id.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure or a damaged row.
    pub fn get(&self, id: &str) -> Result<Option<StoredVideo>, StoreError> {
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let row = connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM videos WHERE id = ?1"),
                params![id],
                read_row,
            )
            .optional()?;
        row.transpose()
    }

    /// List videos, newest first, applying `filter`.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure or a damaged row.
    pub fn list(&self, filter: &VideoFilter) -> Result<Vec<Video>, StoreError> {
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM videos
             WHERE (?1 IS NULL OR participant_code = ?1) AND (?2 IS NULL OR hand = ?2)
             ORDER BY uploaded_at DESC, id"
        ))?;
        let rows = statement.query_map(
            params![filter.participant_code, filter.hand.map(hand_text)],
            read_row,
        )?;
        let mut videos = Vec::new();
        for row in rows {
            videos.push(row??.video);
        }
        Ok(videos)
    }
}

const COLUMNS: &str = "id, filename, participant_code, visit_date, hand, uploaded_at, size_bytes,
    width, height, fps, duration_seconds, frame_count, codec, extension, gate_json";

fn read_row(row: &Row<'_>) -> rusqlite::Result<Result<StoredVideo, StoreError>> {
    let hand_text: String = row.get(4)?;
    let gate_json: String = row.get(14)?;
    let size: i64 = row.get(6)?;
    let hand = match hand_text.as_str() {
        "left" => Hand::Left,
        "right" => Hand::Right,
        other => return Ok(Err(StoreError::Damaged(format!("unknown hand {other:?}")))),
    };
    let gate: Vec<GateCheck> = match serde_json::from_str(&gate_json) {
        Ok(gate) => gate,
        Err(error) => return Ok(Err(StoreError::Damaged(error.to_string()))),
    };
    Ok(Ok(StoredVideo {
        video: Video {
            id: row.get(0)?,
            filename: row.get(1)?,
            participant_code: row.get(2)?,
            visit_date: row.get(3)?,
            hand,
            uploaded_at: row.get(5)?,
            size_bytes: u64::try_from(size).unwrap_or(0),
            width: row.get(7)?,
            height: row.get(8)?,
            fps: row.get(9)?,
            duration_seconds: row.get(10)?,
            frame_count: row.get(11)?,
            codec: row.get(12)?,
            gate,
        },
        extension: row.get(13)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, code: &str, hand: Hand, at: &str) -> Video {
        Video {
            id: id.to_owned(),
            filename: "clip.mp4".to_owned(),
            participant_code: code.to_owned(),
            visit_date: "2026-03-01".to_owned(),
            hand,
            uploaded_at: at.to_owned(),
            size_bytes: 1234,
            width: 1920,
            height: 1080,
            fps: 60.0,
            duration_seconds: 12.5,
            frame_count: 750,
            codec: "h264".to_owned(),
            gate: crate::gate::evaluate(60.0, 12.5),
        }
    }

    #[test]
    fn a_stored_video_reads_back_identically() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        let video = sample("a1", "P1", Hand::Left, "2026-03-01T10:00:00Z");
        assert!(store.insert(&video, "mp4")?);
        let back = store.get("a1")?;
        assert_eq!(
            back,
            Some(StoredVideo {
                video,
                extension: "mp4".to_owned()
            })
        );
        assert_eq!(store.get("missing")?, None);
        Ok(())
    }

    #[test]
    fn inserting_the_same_id_twice_keeps_the_first_record() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        let first = sample("a1", "P1", Hand::Left, "2026-03-01T10:00:00Z");
        let second = sample("a1", "P2", Hand::Right, "2026-03-02T10:00:00Z");
        assert!(store.insert(&first, "mp4")?);
        assert!(!store.insert(&second, "mp4")?);
        assert_eq!(
            store.get("a1")?.map(|s| s.video.participant_code),
            Some("P1".to_owned())
        );
        Ok(())
    }

    #[test]
    fn list_is_newest_first_and_filters_combine() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.insert(
            &sample("a", "P1", Hand::Left, "2026-03-01T10:00:00Z"),
            "mp4",
        )?;
        store.insert(
            &sample("b", "P1", Hand::Right, "2026-03-02T10:00:00Z"),
            "mp4",
        )?;
        store.insert(
            &sample("c", "P2", Hand::Left, "2026-03-03T10:00:00Z"),
            "webm",
        )?;
        let ids = |filter: &VideoFilter| -> Result<Vec<String>, StoreError> {
            Ok(store.list(filter)?.into_iter().map(|v| v.id).collect())
        };
        assert_eq!(ids(&VideoFilter::default())?, ["c", "b", "a"]);
        let p1 = VideoFilter {
            participant_code: Some("P1".into()),
            hand: None,
        };
        assert_eq!(ids(&p1)?, ["b", "a"]);
        let left = VideoFilter {
            participant_code: None,
            hand: Some(Hand::Left),
        };
        assert_eq!(ids(&left)?, ["c", "a"]);
        let both = VideoFilter {
            participant_code: Some("P1".into()),
            hand: Some(Hand::Left),
        };
        assert_eq!(ids(&both)?, ["a"]);
        Ok(())
    }

    #[test]
    fn filter_values_are_data_not_sql() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.insert(
            &sample("a", "P1", Hand::Left, "2026-03-01T10:00:00Z"),
            "mp4",
        )?;
        let hostile = VideoFilter {
            participant_code: Some("x' OR '1'='1".into()),
            hand: None,
        };
        assert!(store.list(&hostile)?.is_empty());
        Ok(())
    }

    #[test]
    fn reopening_a_file_keeps_data_and_does_not_rerun_migrations(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("index.sqlite");
        Store::open(&path)?.insert(
            &sample("a", "P1", Hand::Left, "2026-03-01T10:00:00Z"),
            "mp4",
        )?;
        let reopened = Store::open(&path)?;
        assert!(reopened.get("a")?.is_some());
        Ok(())
    }

    #[test]
    fn an_index_from_a_newer_version_is_refused() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("index.sqlite");
        Connection::open(&path)?.pragma_update(None, "user_version", 99)?;
        let error = Store::open(&path).err();
        assert!(matches!(error, Some(StoreError::TooNew { found: 99, .. })));
        Ok(())
    }

    #[test]
    fn the_database_rejects_an_unknown_hand() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        let connection = store.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let result = connection.execute(
            "INSERT INTO videos VALUES ('x','f','p','2026-01-01','middle','t',1,1,1,1.0,1.0,1,'c','mp4','[]')",
            [],
        );
        assert!(result.is_err());
        Ok(())
    }
}
