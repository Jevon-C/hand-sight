//! Clinician scores kept in the index. A result's own files never change; scores are separate
//! facts about a result, so they live in the database and can be corrected.

use axonhand_api::{ClinicianScore, ScoreRequest};
use rusqlite::params;

use crate::store::{Store, StoreError};

impl Store {
    /// Save a score, replacing the same rater's earlier score for the result.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn put_score(
        &self,
        result_key: &str,
        request: &ScoreRequest,
        recorded_at: &str,
    ) -> Result<ClinicianScore, StoreError> {
        self.lock()?.execute(
            "INSERT INTO scores (result_key, rater, score, note, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (result_key, rater) DO UPDATE
             SET score = excluded.score, note = excluded.note, recorded_at = excluded.recorded_at",
            params![
                result_key,
                request.rater,
                request.score,
                request.note,
                recorded_at
            ],
        )?;
        Ok(ClinicianScore {
            result_key: result_key.to_owned(),
            rater: request.rater.clone(),
            score: request.score,
            note: request.note.clone(),
            recorded_at: recorded_at.to_owned(),
        })
    }

    /// The scores of one result, ordered by rater name.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on a database failure.
    pub fn scores(&self, result_key: &str) -> Result<Vec<ClinicianScore>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT result_key, rater, score, note, recorded_at FROM scores
             WHERE result_key = ?1 ORDER BY rater",
        )?;
        let rows = statement.query_map(params![result_key], |row| {
            Ok(ClinicianScore {
                result_key: row.get(0)?,
                rater: row.get(1)?,
                score: row.get(2)?,
                note: row.get(3)?,
                recorded_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(rater: &str, score: u8, note: &str) -> ScoreRequest {
        ScoreRequest {
            rater: rater.to_owned(),
            score,
            note: note.to_owned(),
        }
    }

    #[test]
    fn a_raters_score_is_replaced_and_others_are_kept() -> Result<(), StoreError> {
        let store = Store::open_in_memory()?;
        store.put_score("k1", &request("B", 2, ""), "2026-01-01T00:00:00Z")?;
        store.put_score("k1", &request("A", 1, "first"), "2026-01-01T00:00:01Z")?;
        store.put_score("k2", &request("A", 4, ""), "2026-01-01T00:00:02Z")?;
        let changed = store.put_score("k1", &request("A", 3, "second"), "2026-01-02T00:00:00Z")?;
        assert_eq!(changed.score, 3);
        let scores = store.scores("k1")?;
        let summary: Vec<(&str, u8, &str)> = scores
            .iter()
            .map(|s| (s.rater.as_str(), s.score, s.note.as_str()))
            .collect();
        assert_eq!(summary, [("A", 3, "second"), ("B", 2, "")]);
        assert_eq!(scores[0].recorded_at, "2026-01-02T00:00:00Z");
        assert_eq!(store.scores("k2")?.len(), 1);
        assert!(store.scores("none")?.is_empty());
        Ok(())
    }
}
