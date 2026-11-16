# Storage

Everything the server keeps lives under one **data folder** (default `data/`, set with
`data_dir` or `AXONHAND_DATA_DIR`). The folder is git-ignored; it holds research video and must
not be committed or shared.

```
data/
  index.sqlite        facts about each video (SQLite, schema versioned)
  videos/<id>.<ext>   the video bytes, named by content hash
  tmp/                uploads in progress; emptied when the server starts
```

## Video identity

A video's **id is the SHA-256 of its bytes**, in lowercase hex. Uploading the same file twice
gives the same id, so the second upload returns the existing record (`200`, `duplicate: true`)
and stores nothing. The details given with the first upload (participant, date, hand) stand;
they are not overwritten.

## Upload steps

1. The form fields (`participant_code`, `visit_date`, `hand`) must come before `file`. They are
   validated before any video byte is read.
2. The file streams to `tmp/` while being hashed. Above `max_upload_bytes` (default 2 GiB) the
   upload stops with `413` and the partial file is removed.
3. The first bytes decide the type (MP4, MOV, WebM); the file name is ignored. Anything else
   gets `415`.
4. If the id is already stored, the temporary file is discarded.
5. `python -m axonhand.tools.probe` reads size, frame rate, length and codec in a separate
   process. A file it cannot read gets `415` with its message; a probe that cannot run is a
   `500` whose details go to the log only.
6. The file is renamed into `videos/` and the record inserted. If the insert fails the file is
   removed again.

The uploaded file name is kept only as display text (path parts and control characters
removed, 200 characters at most). It is never used to build a path.

## Quality gate

Computed at upload and stored with the record. It reports and never blocks:

| Check | Pass when |
| --- | --- |
| Frame rate | at least 50 fps (so 59.94 passes) |
| Duration | at least 5 s |
| Hand visible | *not checked yet* (needs the hand detector, phase 5) |
| Board visible | *not checked yet* (needs the board detector, phase 5) |

Checks that cannot be made are reported `not_checked`, never as a pass.

## Index schema

Numbered migrations in `crates/server/src/store.rs`, recorded in SQLite `user_version`. A data
folder from an older version is upgraded on start; one from a newer version is refused. A
migration that has shipped is never edited; add a new one.

Version 1: table `videos` (one row per video, gate report stored as JSON), indexes on
participant and upload time.

Version 2: table `jobs` (the queue; a job left `running` by a restart is queued again on the next
start) and table `results` (an index of saved results). Annotations arrive in a later migration.

## Results

A result is saved once, in `data/results/<key>/`, and never changed:

| File | Content |
| --- | --- |
| `result.json` | the measures, quality checks, notes, test version, model and every setting |
| `signals.json` | data for the plots (optional) |
| `landmarks.json` | the hand's landmarks in every frame, in pixels of the upright video, for drawing over it (optional; the real hand model writes it, the development stand-in does not) |
| `hashes.json` | SHA-256 of the files above, checked on every read |

The key is the SHA-256 of everything that decides the numbers: the video id, test id and
version, the model's hash, the sorted metric ids, and every setting (numbers by their exact bit
pattern). The same inputs always give the same key, so asking again returns the saved result
instead of repeating the work. A different test version, model or setting gives a new key, so
older results stay available and are never silently replaced. The folder is written under a
temporary name and renamed into place, so a crash never leaves a half-written result. A file
that no longer matches its hash is reported as damaged, not shown. The `results` table in the
index can be rebuilt from these folders at any start.

Scratch files for a running job (landmarks, plug-in input and output) live in `data/work/` and
are removed when the job succeeds. A failed job's folder is kept so its `plugin/output.log` can be read; delete it by hand when no longer needed.

## Streaming

`GET /api/videos/{id}/stream` serves the stored file with `Accept-Ranges`, so the browser can
seek. Ids that are not 64 lowercase hex digits are `404` before any file or query is touched.

## Clinician scores

A clinician's MDS-UPDRS score (0 to 4) for a result is kept in the index (table `scores`, one row
for each result and rater), not in the result's folder, because a result is never changed and a
score is a separate fact that a rater may correct. Saving again with the same rater name replaces
that rater's row, with a new time. Several raters can score the same result, which is what is
needed to compare raters with each other and with the measures. The score is entered after the
result is shown, so the app does not blind the rater to the measures; a blinded study must have the
scores entered from the video alone, before the result is opened. Scores are not written to the
CSV export.

