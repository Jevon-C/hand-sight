//! Saved results: one folder per result key, written once and never changed.
//!
//! ```text
//! results/<key>/result.json    the analysis result
//! results/<key>/signals.json   data behind the plots (optional)
//! results/<key>/landmarks.json hand landmarks for drawing over the video (optional)
//! results/<key>/hashes.json    SHA-256 of the other files, checked on every read
//! ```
//!
//! A folder is built under a temporary name and renamed into place, so a reader never sees a
//! half-written result. The files are the source of truth: the SQLite index of results can be
//! rebuilt from them with [`rebuild_index`].

use std::path::{Path, PathBuf};

use axonhand_api::{AnalysisResult, Signals};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::media::{hex, is_video_id};
use crate::store::{Store, StoreError};

/// Why a result could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum ResultError {
    /// The key is not a result key, so it was never used to touch the file system.
    #[error("not a valid result key")]
    BadKey,
    /// A file operation failed.
    #[error("result files: {0}")]
    Io(String),
    /// A stored file is missing, unreadable or does not match its recorded hash.
    #[error("stored result is damaged: {0}")]
    Corrupt(String),
    /// The index could not be updated.
    #[error(transparent)]
    Index(#[from] StoreError),
}

#[derive(Serialize, Deserialize)]
struct Hashes {
    result: String,
    signals: Option<String>,
    #[serde(default)]
    landmarks: Option<String>,
}

/// The folder of saved results.
#[derive(Debug, Clone)]
pub struct ResultFiles {
    dir: PathBuf,
}

fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn io(error: &std::io::Error) -> ResultError {
    ResultError::Io(error.to_string())
}

impl ResultFiles {
    /// Use (creating if needed) `dir` as the results folder.
    ///
    /// # Errors
    ///
    /// [`ResultError::Io`] if the folder cannot be created.
    pub fn open(dir: &Path) -> Result<Self, ResultError> {
        std::fs::create_dir_all(dir).map_err(|e| io(&e))?;
        Ok(Self {
            dir: dir.to_owned(),
        })
    }

    fn folder(&self, key: &str) -> Result<PathBuf, ResultError> {
        if is_video_id(key) {
            Ok(self.dir.join(key))
        } else {
            Err(ResultError::BadKey)
        }
    }

    /// Whether a result with this key is saved.
    ///
    /// # Errors
    ///
    /// [`ResultError::BadKey`] for a malformed key.
    pub fn exists(&self, key: &str) -> Result<bool, ResultError> {
        Ok(self.folder(key)?.join("hashes.json").is_file())
    }

    /// Save a result. Returns `false`, changing nothing, when the key already has one.
    ///
    /// # Errors
    ///
    /// [`ResultError`] if the key is malformed or the files cannot be written.
    pub fn write(
        &self,
        key: &str,
        result: &AnalysisResult,
        signals: Option<&Signals>,
        landmarks: Option<&[u8]>,
    ) -> Result<bool, ResultError> {
        let target = self.folder(key)?;
        if target.exists() {
            return Ok(false);
        }
        let result_bytes =
            serde_json::to_vec_pretty(result).map_err(|e| ResultError::Io(e.to_string()))?;
        let signal_bytes = signals
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|e| ResultError::Io(e.to_string()))?;
        let hashes = Hashes {
            result: sha(&result_bytes),
            signals: signal_bytes.as_deref().map(sha),
            landmarks: landmarks.map(sha),
        };
        let temp = self.dir.join(format!(".tmp-{key}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).map_err(|e| io(&e))?;
        let built = (|| -> std::io::Result<()> {
            std::fs::write(temp.join("result.json"), &result_bytes)?;
            if let Some(bytes) = &signal_bytes {
                std::fs::write(temp.join("signals.json"), bytes)?;
            }
            if let Some(bytes) = landmarks {
                std::fs::write(temp.join("landmarks.json"), bytes)?;
            }
            std::fs::write(
                temp.join("hashes.json"),
                serde_json::to_vec(&hashes).map_err(std::io::Error::other)?,
            )
        })();
        if let Err(error) = built {
            let _ = std::fs::remove_dir_all(&temp);
            return Err(io(&error));
        }
        match std::fs::rename(&temp, &target) {
            Ok(()) => Ok(true),
            Err(_) if target.exists() => {
                // Another run saved the same key first; its result stands.
                let _ = std::fs::remove_dir_all(&temp);
                Ok(false)
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(&temp);
                Err(io(&error))
            }
        }
    }

    fn read_checked(&self, key: &str, name: &str, expected: &str) -> Result<Vec<u8>, ResultError> {
        let bytes = std::fs::read(self.folder(key)?.join(name))
            .map_err(|_| ResultError::Corrupt(format!("{name} is missing or unreadable")))?;
        if sha(&bytes) == expected {
            Ok(bytes)
        } else {
            Err(ResultError::Corrupt(format!(
                "{name} does not match its recorded hash"
            )))
        }
    }

    fn hashes(&self, key: &str) -> Result<Option<Hashes>, ResultError> {
        let path = self.folder(key)?.join("hashes.json");
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| io(&e))?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| ResultError::Corrupt("hashes.json is unreadable".to_owned()))
    }

    /// Read a result, checking its hash. `None` when there is no such result.
    ///
    /// # Errors
    ///
    /// [`ResultError::Corrupt`] when a file does not match its hash.
    pub fn read(&self, key: &str) -> Result<Option<AnalysisResult>, ResultError> {
        let Some(hashes) = self.hashes(key)? else {
            return Ok(None);
        };
        let bytes = self.read_checked(key, "result.json", &hashes.result)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| ResultError::Corrupt("result.json is not a result".to_owned()))
    }

    /// Read the plot data of a result, checking its hash. `None` when the result has none.
    ///
    /// # Errors
    ///
    /// [`ResultError::Corrupt`] when the file does not match its hash.
    pub fn read_signals(&self, key: &str) -> Result<Option<Signals>, ResultError> {
        let Some(hashes) = self.hashes(key)? else {
            return Ok(None);
        };
        let Some(expected) = hashes.signals else {
            return Ok(None);
        };
        let bytes = self.read_checked(key, "signals.json", &expected)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| ResultError::Corrupt("signals.json is not valid".to_owned()))
    }

    /// Read the landmark file of a result as bytes, checking its hash. `None` when it has none.
    ///
    /// # Errors
    ///
    /// [`ResultError::Corrupt`] when the file does not match its hash.
    pub fn read_landmarks(&self, key: &str) -> Result<Option<Vec<u8>>, ResultError> {
        let Some(hashes) = self.hashes(key)? else {
            return Ok(None);
        };
        let Some(expected) = hashes.landmarks else {
            return Ok(None);
        };
        self.read_checked(key, "landmarks.json", &expected)
            .map(Some)
    }

    /// Keys of every saved result.
    ///
    /// # Errors
    ///
    /// [`ResultError::Io`] if the folder cannot be listed.
    pub fn keys(&self) -> Result<Vec<String>, ResultError> {
        let mut keys: Vec<String> = std::fs::read_dir(&self.dir)
            .map_err(|e| io(&e))?
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| is_video_id(name))
            .collect();
        keys.sort();
        Ok(keys)
    }
}

/// Add every saved result to the index. Safe to run at any time; results already indexed are
/// left alone. Results whose files are damaged are skipped and logged. Returns how many were
/// newly added.
///
/// # Errors
///
/// [`ResultError`] if the results folder cannot be listed or the index fails.
pub fn rebuild_index(files: &ResultFiles, store: &Store) -> Result<usize, ResultError> {
    let mut added = 0;
    for key in files.keys()? {
        match files.read(&key) {
            Ok(Some(result)) => {
                let new = store.index_result(
                    &key,
                    &result.video_id,
                    &result.test_id,
                    &result.test_version,
                    &result.created_at,
                )?;
                added += usize::from(new);
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%key, %error, "skipping a damaged result"),
        }
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axonhand_api::{ModelInfo, QualityCheck, QualityStatus, SeriesData};
    use std::collections::BTreeMap;

    fn key(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    fn sample(video: &str) -> AnalysisResult {
        AnalysisResult {
            key: key(1),
            video_id: video.to_owned(),
            test_id: "t".to_owned(),
            test_name: "T".to_owned(),
            test_version: "1.0.0".to_owned(),
            model: ModelInfo {
                name: "m".into(),
                hash: "h".into(),
                stand_in: true,
                device: String::new(),
            },
            parameters: BTreeMap::from([("p".to_owned(), 1.0)]),
            measures: vec![],
            quality: vec![QualityCheck {
                name: "q".into(),
                status: QualityStatus::Pass,
                detail: "d".into(),
            }],
            notes: vec!["n".into()],
            illustrative: true,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn signals() -> Signals {
        Signals {
            series: vec![SeriesData {
                id: "s".into(),
                name: "S".into(),
                unit: "u".into(),
                start_seconds: 0.0,
                step_seconds: 0.5,
                values: vec![1.0, 2.0],
            }],
            events: vec![],
        }
    }

    #[test]
    fn a_result_and_its_signals_read_back_identically() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let files = ResultFiles::open(dir.path())?;
        assert!(!files.exists(&key(1))?);
        assert!(files.write(&key(1), &sample("v"), Some(&signals()), None)?);
        assert!(files.exists(&key(1))?);
        assert_eq!(files.read(&key(1))?, Some(sample("v")));
        assert_eq!(files.read_signals(&key(1))?, Some(signals()));
        assert_eq!(files.read(&key(2))?, None);
        Ok(())
    }

    #[test]
    fn landmarks_are_stored_checked_and_optional() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let files = ResultFiles::open(dir.path())?;
        let bytes = br#"{"fps":30.0,"points":[null]}"#;
        assert!(files.write(&key(1), &sample("v"), None, Some(bytes))?);
        files.write(&key(2), &sample("v"), None, None)?;
        assert_eq!(files.read_landmarks(&key(1))?, Some(bytes.to_vec()));
        assert_eq!(files.read_landmarks(&key(2))?, None);
        assert_eq!(files.read_landmarks(&key(3))?, None);
        std::fs::write(
            dir.path().join(key(1)).join("landmarks.json"),
            br#"{"fps":1.0,"points":[]}"#,
        )?;
        assert!(matches!(
            files.read_landmarks(&key(1)),
            Err(ResultError::Corrupt(_))
        ));
        Ok(())
    }

    #[test]
    fn a_result_without_signals_reads_back_without_them() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let files = ResultFiles::open(dir.path())?;
        files.write(&key(1), &sample("v"), None, None)?;
        assert_eq!(files.read_signals(&key(1))?, None);
        Ok(())
    }

    #[test]
    fn results_are_written_once_and_never_changed() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let files = ResultFiles::open(dir.path())?;
        assert!(files.write(&key(1), &sample("first"), None, None)?);
        assert!(!files.write(&key(1), &sample("second"), Some(&signals()), None)?);
        assert_eq!(
            files.read(&key(1))?.map(|r| r.video_id),
            Some("first".to_owned())
        );
        assert_eq!(files.read_signals(&key(1))?, None);
        let leftovers = std::fs::read_dir(dir.path())?
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .count();
        assert_eq!(leftovers, 0, "temporary folders are cleaned up");
        Ok(())
    }

    #[test]
    fn tampering_with_a_file_is_detected_on_read() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let files = ResultFiles::open(dir.path())?;
        files.write(&key(1), &sample("v"), Some(&signals()), None)?;
        let folder = dir.path().join(key(1));
        std::fs::write(folder.join("result.json"), b"{}")?;
        assert!(matches!(files.read(&key(1)), Err(ResultError::Corrupt(_))));
        std::fs::write(folder.join("signals.json"), b"{}")?;
        assert!(matches!(
            files.read_signals(&key(1)),
            Err(ResultError::Corrupt(_))
        ));
        std::fs::remove_file(folder.join("signals.json"))?;
        assert!(matches!(
            files.read_signals(&key(1)),
            Err(ResultError::Corrupt(_))
        ));
        std::fs::write(folder.join("hashes.json"), b"garbage")?;
        assert!(matches!(files.read(&key(1)), Err(ResultError::Corrupt(_))));
        Ok(())
    }

    #[test]
    fn keys_that_are_not_result_keys_never_touch_the_file_system(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let files = ResultFiles::open(dir.path())?;
        for bad in ["", "../x", "..", "a/b", &"A".repeat(64)] {
            assert!(
                matches!(files.read(bad), Err(ResultError::BadKey)),
                "{bad:?}"
            );
            assert!(matches!(
                files.write(bad, &sample("v"), None, None),
                Err(ResultError::BadKey)
            ));
            assert!(matches!(files.exists(bad), Err(ResultError::BadKey)));
        }
        Ok(())
    }

    #[test]
    fn the_index_can_be_rebuilt_from_the_files() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let files = ResultFiles::open(dir.path())?;
        files.write(&key(1), &sample("v1"), None, None)?;
        files.write(&key(2), &sample("v2"), None, None)?;
        files.write(&key(3), &sample("v3"), None, None)?;
        std::fs::write(dir.path().join(key(3)).join("result.json"), b"damaged")?;
        let store = Store::open_in_memory()?;
        assert_eq!(rebuild_index(&files, &store)?, 2);
        assert!(store.result_indexed(&key(1))?);
        assert!(store.result_indexed(&key(2))?);
        assert!(
            !store.result_indexed(&key(3))?,
            "a damaged result is not indexed"
        );
        assert_eq!(
            rebuild_index(&files, &store)?,
            0,
            "running again adds nothing"
        );
        Ok(())
    }
}
