//! Jobs and the index of saved results, kept in the same SQLite file as the videos.
//!
//! A job is one request to run a test on a video. Jobs are claimed one at a time in the order
//! they were made, and a job that was running when the server stopped is put back in the queue
//! on the next start, so nothing is lost or left stuck.

use axonhand_api::{Hand, Job, JobRequest, JobState, ResultSummary};
use rusqlite::{params, OptionalExtension, Row};

use crate::store::{Store, StoreError};

/// A job together with what was asked.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    /// The job as clients see it.
    pub job: Job,
    /// The request, with every setting filled in.
    pub request: JobRequest,
}

fn state_text(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Succeeded => "succeeded",
        JobState::Failed => "failed",
    }
}

const COLUMNS: &str =
    "id, state, result_key, video_id, test_id, request_json, step, progress, error, created_at";

fn read_job(row: &Row<'_>) -> rusqlite::Result<Result<JobRecord, StoreError>> {
    let state_text: String = row.get(1)?;
    let state = match state_text.as_str() {
        "queued" => JobState::Queued,
        "running" => JobState::Running,
        "succeeded" => JobState::Succeeded,
        "failed" => JobState::Failed,
        other => {
            return Ok(Err(StoreError::Damaged(format!(
                "unknown job state {other:?}"
            ))))
        }
    };
    let request_json: String = row.get(5)?;
    let request: JobRequest = match serde_json::from_str(&request_json) {
        Ok(request) => request,
        Err(error) => return Ok(Err(StoreError::Damaged(error.to_string()))),
    };
    Ok(Ok(JobRecord {
        job: Job {
            id: row.get(0)?,
            state,
            result_key: row.get(2)?,
            video_id: row.get(3)?,
            test_id: row.get(4)?,
            step: row.get(6)?,
            progress: row.get(7)?,
            error: row.get(8)?,
            created_at: row.get(9)?,
        },
        request,
    }))
}

impl Store {
    /// Add a job in the given state.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn insert_job(&self, job: &Job, request: &JobRequest) -> Result<(), StoreError> {
        let json =
            serde_json::to_string(request).map_err(|e| StoreError::Damaged(e.to_string()))?;
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO jobs (id, state, result_key, video_id, test_id, request_json, step,
                progress, error, created_at, seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                (SELECT COALESCE(MAX(seq), 0) + 1 FROM jobs))",
            params![
                job.id,
                state_text(job.state),
                job.result_key,
                job.video_id,
                job.test_id,
                json,
                job.step,
                job.progress,
                job.error,
                job.created_at,
            ],
        )?;
        Ok(())
    }

    /// Look up a job.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure or a damaged row.
    pub fn get_job(&self, id: &str) -> Result<Option<JobRecord>, StoreError> {
        let connection = self.lock()?;
        connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1"),
                params![id],
                read_job,
            )
            .optional()?
            .transpose()
    }

    /// The newest queued or running job that will produce `result_key`, if any.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure or a damaged row.
    pub fn find_active_job(&self, result_key: &str) -> Result<Option<JobRecord>, StoreError> {
        let connection = self.lock()?;
        connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM jobs
                     WHERE result_key = ?1 AND state IN ('queued', 'running')
                     ORDER BY seq DESC LIMIT 1"
                ),
                params![result_key],
                read_job,
            )
            .optional()?
            .transpose()
    }

    /// Take the oldest queued job and mark it running. `None` when the queue is empty.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure or a damaged row.
    pub fn claim_next_job(&self) -> Result<Option<JobRecord>, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let found = transaction
            .query_row(
                &format!("SELECT {COLUMNS} FROM jobs WHERE state = 'queued' ORDER BY seq LIMIT 1"),
                [],
                read_job,
            )
            .optional()?
            .transpose()?;
        let Some(mut record) = found else {
            return Ok(None);
        };
        transaction.execute(
            "UPDATE jobs SET state = 'running', step = 'Starting', progress = 0 WHERE id = ?1",
            params![record.job.id],
        )?;
        transaction.commit()?;
        record.job.state = JobState::Running;
        record.job.step = "Starting".to_owned();
        record.job.progress = 0.0;
        Ok(Some(record))
    }

    /// Record what a running job is doing.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn set_job_progress(&self, id: &str, step: &str, progress: f64) -> Result<(), StoreError> {
        self.lock()?.execute(
            "UPDATE jobs SET step = ?2, progress = ?3 WHERE id = ?1 AND state = 'running'",
            params![id, step, progress.clamp(0.0, 1.0)],
        )?;
        Ok(())
    }

    /// Mark a job finished, successfully (`error` is `None`) or not.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn finish_job(&self, id: &str, error: Option<&str>) -> Result<(), StoreError> {
        let (state, progress) = if error.is_some() {
            ("failed", 0.0)
        } else {
            ("succeeded", 1.0)
        };
        self.lock()?.execute(
            "UPDATE jobs SET state = ?2, step = '', progress = CASE WHEN ?2 = 'succeeded' THEN ?3 ELSE progress END,
                error = ?4 WHERE id = ?1",
            params![id, state, progress, error],
        )?;
        Ok(())
    }

    /// Put jobs that were running when the server stopped back in the queue. Returns how many.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn requeue_running_jobs(&self) -> Result<usize, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE jobs SET state = 'queued', step = '', progress = 0 WHERE state = 'running'",
            [],
        )?)
    }

    /// Note in the index that a result exists. Returns `false` if it was already noted.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn index_result(
        &self,
        key: &str,
        video_id: &str,
        test_id: &str,
        test_version: &str,
        created_at: &str,
    ) -> Result<bool, StoreError> {
        let rows = self.lock()?.execute(
            "INSERT OR IGNORE INTO results (key, video_id, test_id, test_version, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![key, video_id, test_id, test_version, created_at],
        )?;
        Ok(rows == 1)
    }

    /// Saved results with the video each came from, newest first. Empty filters match everything.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn list_results(
        &self,
        test_id: Option<&str>,
        participant_code: Option<&str>,
    ) -> Result<Vec<ResultSummary>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT r.key, r.video_id, r.test_id, r.test_version, r.created_at,
                    v.participant_code, v.visit_date, v.hand
             FROM results r JOIN videos v ON v.id = r.video_id
             WHERE (?1 IS NULL OR r.test_id = ?1) AND (?2 IS NULL OR v.participant_code = ?2)
             ORDER BY r.created_at DESC, r.key",
        )?;
        let rows = statement.query_map(params![test_id, participant_code], |row| {
            let hand: String = row.get(7)?;
            Ok((
                ResultSummary {
                    key: row.get(0)?,
                    video_id: row.get(1)?,
                    test_id: row.get(2)?,
                    test_version: row.get(3)?,
                    created_at: row.get(4)?,
                    participant_code: row.get(5)?,
                    visit_date: row.get(6)?,
                    hand: Hand::Left,
                },
                hand,
            ))
        })?;
        let mut summaries = Vec::new();
        for row in rows {
            let (mut summary, hand) = row?;
            summary.hand = match hand.as_str() {
                "left" => Hand::Left,
                "right" => Hand::Right,
                other => return Err(StoreError::Damaged(format!("unknown hand {other:?}"))),
            };
            summaries.push(summary);
        }
        Ok(summaries)
    }

    /// Whether a result with this key is in the index.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn result_indexed(&self, key: &str) -> Result<bool, StoreError> {
        Ok(self
            .lock()?
            .query_row("SELECT 1 FROM results WHERE key = ?1", params![key], |_| {
                Ok(())
            })
            .optional()?
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn request(video: &str) -> JobRequest {
        JobRequest {
            video_id: video.to_owned(),
            test_id: "t".to_owned(),
            metrics: vec!["a".to_owned()],
            parameters: BTreeMap::new(),
            device: axonhand_api::ComputeDevice::Cpu,
        }
    }

    fn job(id: &str, key: &str) -> Job {
        Job {
            id: id.to_owned(),
            state: JobState::Queued,
            result_key: key.to_owned(),
            video_id: "v".to_owned(),
            test_id: "t".to_owned(),
            step: String::new(),
            progress: 0.0,
            error: None,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn a_job_reads_back_with_its_request() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.insert_job(&job("j1", "k1"), &request("v"))?;
        let back = store.get_job("j1")?;
        assert_eq!(
            back,
            Some(JobRecord {
                job: job("j1", "k1"),
                request: request("v")
            })
        );
        assert_eq!(store.get_job("nope")?, None);
        Ok(())
    }

    #[test]
    fn jobs_are_claimed_one_at_a_time_in_order() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        for (id, key) in [("a", "ka"), ("b", "kb"), ("c", "kc")] {
            store.insert_job(&job(id, key), &request("v"))?;
        }
        let first = store.claim_next_job()?.map(|r| r.job);
        assert_eq!(first.as_ref().map(|j| j.id.as_str()), Some("a"));
        assert_eq!(first.map(|j| j.state), Some(JobState::Running));
        assert_eq!(
            store.claim_next_job()?.map(|r| r.job.id),
            Some("b".to_owned())
        );
        assert_eq!(
            store.get_job("a")?.map(|r| r.job.state),
            Some(JobState::Running)
        );
        assert_eq!(
            store.get_job("c")?.map(|r| r.job.state),
            Some(JobState::Queued)
        );
        store.claim_next_job()?;
        assert_eq!(store.claim_next_job()?, None);
        Ok(())
    }

    #[test]
    fn progress_and_finishing_are_recorded() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.insert_job(&job("a", "ka"), &request("v"))?;
        store.claim_next_job()?;
        store.set_job_progress("a", "Measuring", 0.6)?;
        let mid = store.get_job("a")?.map(|r| r.job);
        assert_eq!(mid.as_ref().map(|j| j.step.as_str()), Some("Measuring"));
        assert_eq!(mid.map(|j| j.progress), Some(0.6));
        store.finish_job("a", None)?;
        let done = store.get_job("a")?.map(|r| r.job);
        assert_eq!(done.as_ref().map(|j| j.state), Some(JobState::Succeeded));
        assert_eq!(done.map(|j| j.progress), Some(1.0));
        store.insert_job(&job("b", "kb"), &request("v"))?;
        store.claim_next_job()?;
        store.finish_job("b", Some("It broke."))?;
        let failed = store.get_job("b")?.map(|r| r.job);
        assert_eq!(failed.as_ref().map(|j| j.state), Some(JobState::Failed));
        assert_eq!(failed.and_then(|j| j.error), Some("It broke.".to_owned()));
        Ok(())
    }

    #[test]
    fn progress_cannot_be_set_on_a_finished_job_and_is_clamped() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.insert_job(&job("a", "ka"), &request("v"))?;
        store.claim_next_job()?;
        store.set_job_progress("a", "x", 7.0)?;
        assert_eq!(store.get_job("a")?.map(|r| r.job.progress), Some(1.0));
        store.finish_job("a", Some("e"))?;
        store.set_job_progress("a", "late", 0.5)?;
        assert_eq!(store.get_job("a")?.map(|r| r.job.step), Some(String::new()));
        Ok(())
    }

    #[test]
    fn running_jobs_return_to_the_queue_after_a_restart() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("i.sqlite");
        {
            let store = Store::open(&path)?;
            store.insert_job(&job("a", "ka"), &request("v"))?;
            store.insert_job(&job("b", "kb"), &request("v"))?;
            store.claim_next_job()?;
            store.finish_job("b", None)?;
        }
        let store = Store::open(&path)?;
        assert_eq!(store.requeue_running_jobs()?, 1);
        assert_eq!(
            store.get_job("a")?.map(|r| r.job.state),
            Some(JobState::Queued)
        );
        assert_eq!(
            store.get_job("b")?.map(|r| r.job.state),
            Some(JobState::Succeeded)
        );
        assert_eq!(
            store.claim_next_job()?.map(|r| r.job.id),
            Some("a".to_owned())
        );
        Ok(())
    }

    #[test]
    fn an_active_job_is_found_by_its_result_key() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.insert_job(&job("a", "same"), &request("v"))?;
        assert_eq!(
            store.find_active_job("same")?.map(|r| r.job.id),
            Some("a".to_owned())
        );
        assert_eq!(store.find_active_job("other")?, None);
        store.claim_next_job()?;
        store.finish_job("a", None)?;
        assert_eq!(
            store.find_active_job("same")?,
            None,
            "finished jobs are not active"
        );
        Ok(())
    }

    #[test]
    fn the_result_index_ignores_repeats() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        assert!(!store.result_indexed("k")?);
        assert!(store.index_result("k", "v", "t", "1.0.0", "2026-01-01T00:00:00Z")?);
        assert!(!store.index_result("k", "v2", "t", "2.0.0", "2026-02-01T00:00:00Z")?);
        assert!(store.result_indexed("k")?);
        Ok(())
    }

    fn video(id: &str, code: &str, hand: Hand) -> axonhand_api::Video {
        axonhand_api::Video {
            id: id.to_owned(),
            filename: "clip.mp4".to_owned(),
            participant_code: code.to_owned(),
            visit_date: "2026-03-01".to_owned(),
            hand,
            uploaded_at: "2026-03-01T10:00:00Z".to_owned(),
            size_bytes: 1,
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
    fn results_are_listed_newest_first_with_their_video_and_filters() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.insert(&video("v1", "P1", Hand::Left), "mp4")?;
        store.insert(&video("v2", "P2", Hand::Right), "mp4")?;
        store.index_result(
            "k1",
            "v1",
            "finger_tapping",
            "0.2.0",
            "2026-03-01T10:00:00Z",
        )?;
        store.index_result(
            "k2",
            "v2",
            "finger_tapping",
            "0.2.0",
            "2026-03-02T10:00:00Z",
        )?;
        store.index_result(
            "k3",
            "v1",
            "hand_open_close",
            "0.1.0",
            "2026-03-03T10:00:00Z",
        )?;
        let keys = |test: Option<&str>, who: Option<&str>| -> Result<Vec<String>, StoreError> {
            Ok(store
                .list_results(test, who)?
                .into_iter()
                .map(|r| r.key)
                .collect())
        };
        assert_eq!(keys(None, None)?, ["k3", "k2", "k1"]);
        assert_eq!(keys(Some("finger_tapping"), None)?, ["k2", "k1"]);
        assert_eq!(keys(None, Some("P1"))?, ["k3", "k1"]);
        assert_eq!(keys(Some("finger_tapping"), Some("P1"))?, ["k1"]);
        let first = store.list_results(Some("finger_tapping"), Some("P2"))?;
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].hand, Hand::Right);
        assert_eq!(first[0].participant_code, "P2");
        assert_eq!(first[0].visit_date, "2026-03-01");
        Ok(())
    }
}
