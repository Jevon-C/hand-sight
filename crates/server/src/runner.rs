//! Running a plug-in as a separate process under limits.
//!
//! The plug-in is started with the job file and the path where it must write its result.
//! The server never loads plug-in code into its own process, so a crash, a hang or an endless
//! output cannot take the server down. The plug-in has a time limit and its files have a size
//! limit; past either it is killed. Its answer is validated before anyone sees it.

use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::protocol::{validate_result, validate_signals, JobFile, PluginResult, SignalsFile};
use crate::registry::Plugin;
use axonhand_api::Signals;

/// Limits for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Longest the plug-in may run.
    pub timeout: Duration,
    /// Largest size, in bytes, of each of its output files (result, log and error file).
    pub max_output_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(600),
            max_output_bytes: 8 * 1024 * 1024,
        }
    }
}

/// What a successful run returns.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutput {
    /// The validated result.
    pub result: PluginResult,
    /// The plot data, when the plug-in wrote any.
    pub signals: Option<Signals>,
}

/// Why a run did not produce a valid result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// The process could not be started. Not for the user's eyes; the text is for the log.
    #[error("cannot start the plug-in: {0}")]
    Launch(String),
    /// The plug-in ran past its time limit and was stopped.
    #[error("The test took too long and was stopped.")]
    TimedOut,
    /// A plug-in output file grew past the limit and the plug-in was stopped.
    #[error("The test produced too much output and was stopped.")]
    TooMuchOutput,
    /// The plug-in reported a failure. The text is the plug-in's own explanation.
    #[error("The test failed: {0}")]
    PluginFailed(String),
    /// The plug-in finished but its result is missing, unreadable or invalid.
    #[error("The test returned an unusable result: {0}")]
    BadOutput(String),
}

const POLL: Duration = Duration::from_millis(20);
const MAX_EXPLANATION_CHARS: usize = 500;

#[derive(Deserialize)]
struct ErrorFile {
    error: String,
}

/// `path` made absolute without touching the file system. The plug-in is started inside its
/// own work folder, so any path given to it, and the program path itself, must not depend on the
/// server's current folder.
pub(crate) fn absolute(path: &Path) -> Result<std::path::PathBuf, std::io::Error> {
    std::path::absolute(path)
}

/// Run `plugin` on `job` inside `work_dir` (created if missing) and return its validated
/// result.
///
/// # Errors
///
/// A [`RunError`] for every way the run can fail; none of them panics or leaves the process
/// running.
pub fn run(
    python: &Path,
    plugin: &Plugin,
    job: &JobFile,
    work_dir: &Path,
    limits: Limits,
) -> Result<RunOutput, RunError> {
    let launch = |what: &str, e: std::io::Error| RunError::Launch(format!("{what}: {e}"));
    let Some(entrypoint) = &plugin.entrypoint else {
        return Err(RunError::Launch("the plug-in has no entrypoint".to_owned()));
    };
    let work_dir = absolute(work_dir).map_err(|e| launch("work folder", e))?;
    let work_dir = work_dir.as_path();
    let script = absolute(&plugin.dir.join(entrypoint)).map_err(|e| launch("plug-in", e))?;
    // A bare name such as `python` is looked up on the path; anything with a folder in it is
    // made absolute so it still works from the work folder.
    let program = if python.components().count() > 1 {
        absolute(python).map_err(|e| launch("python", e))?
    } else {
        python.to_path_buf()
    };
    std::fs::create_dir_all(work_dir).map_err(|e| launch("work folder", e))?;
    let job_path = work_dir.join("job.json");
    let result_path = work_dir.join("result.json");
    let log_path = work_dir.join("output.log");
    let job_text =
        serde_json::to_string_pretty(job).map_err(|e| RunError::Launch(e.to_string()))?;
    std::fs::write(&job_path, job_text).map_err(|e| launch("job file", e))?;
    let log = File::create(&log_path).map_err(|e| launch("log file", e))?;
    let log_for_errors = log.try_clone().map_err(|e| launch("log file", e))?;

    let mut child = Command::new(&program)
        .arg(&script)
        .arg(&job_path)
        .arg(&result_path)
        .current_dir(work_dir)
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_for_errors))
        .spawn()
        .map_err(|e| launch(&program.display().to_string(), e))?;

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                stop(&mut child);
                return Err(launch("waiting", e));
            }
        }
        if started.elapsed() >= limits.timeout {
            stop(&mut child);
            return Err(RunError::TimedOut);
        }
        if size_of(&log_path) > limits.max_output_bytes
            || size_of(&result_path) > limits.max_output_bytes
        {
            stop(&mut child);
            return Err(RunError::TooMuchOutput);
        }
        std::thread::sleep(POLL);
    };

    if !status.success() {
        return Err(RunError::PluginFailed(explanation(
            work_dir,
            status.code(),
            limits,
        )));
    }
    if size_of(&result_path) > limits.max_output_bytes {
        return Err(RunError::TooMuchOutput);
    }
    let text = std::fs::read_to_string(&result_path)
        .map_err(|_| RunError::BadOutput("no result file was written".to_owned()))?;
    let result: PluginResult = serde_json::from_str(&text).map_err(|e| {
        RunError::BadOutput(format!("the result is not in the expected format ({e})"))
    })?;
    validate_result(&job.metrics, &result).map_err(|r| RunError::BadOutput(r.0))?;
    let signals = read_signals(work_dir, limits)?;
    Ok(RunOutput { result, signals })
}

fn read_signals(work_dir: &Path, limits: Limits) -> Result<Option<Signals>, RunError> {
    let path = work_dir.join("signals.json");
    if !path.is_file() {
        return Ok(None);
    }
    if size_of(&path) > limits.max_output_bytes {
        return Err(RunError::TooMuchOutput);
    }
    let bad = |why: String| RunError::BadOutput(why);
    let text = std::fs::read_to_string(&path)
        .map_err(|_| bad("the signals file cannot be read".into()))?;
    let file: SignalsFile = serde_json::from_str(&text).map_err(|e| {
        bad(format!(
            "the signals file is not in the expected format ({e})"
        ))
    })?;
    validate_signals(file).map(Some).map_err(|r| bad(r.0))
}

fn stop(child: &mut std::process::Child) {
    // On Windows a virtual environment's python.exe is a small launcher that starts the real
    // interpreter as a child, and killing the launcher alone leaves that child running. Ask
    // Windows to end the whole tree first.
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // Both can fail only if the process has already gone, which is the outcome wanted.
    let _ = child.kill();
    let _ = child.wait();
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

/// The plug-in's own explanation from `error.json`, or the exit status when there is none.
fn explanation(work_dir: &Path, code: Option<i32>, limits: Limits) -> String {
    let path = work_dir.join("error.json");
    if size_of(&path) <= limits.max_output_bytes {
        if let Some(parsed) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<ErrorFile>(&t).ok())
        {
            let cleaned: String = parsed
                .error
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_EXPLANATION_CHARS)
                .collect();
            if !cleaned.trim().is_empty() {
                return cleaned;
            }
        }
    }
    match code {
        Some(code) => format!("the plug-in stopped with exit status {code}."),
        None => "the plug-in was stopped by the system.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A Python interpreter for the tests. CI and the laptop have one; failing loudly beats a
    /// silent skip.
    fn python() -> PathBuf {
        if let Ok(path) = std::env::var("AXONHAND_TEST_PYTHON") {
            return PathBuf::from(path);
        }
        for name in ["python3", "python"] {
            let works = Command::new(name)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if works {
                return PathBuf::from(name);
            }
        }
        PathBuf::from("python3")
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        plugin: Plugin,
        work: PathBuf,
    }

    fn fixture(script: &str) -> Result<Fixture, std::io::Error> {
        fixture_in(tempfile::tempdir()?, script)
    }

    fn fixture_in(dir: tempfile::TempDir, script: &str) -> Result<Fixture, std::io::Error> {
        let plugin_dir = dir.path().join("plugin");
        std::fs::create_dir_all(&plugin_dir)?;
        std::fs::write(plugin_dir.join("p.py"), script)?;
        let manifest = crate::manifest::parse(
            r#"id = "t"
version = "1.0.0"
name = "T"
clinical_item = "X"
status = "available"
summary = "s"
entrypoint = "p.py"
[capture]
min_fps = 50
min_duration_seconds = 5
instructions = "i"
[[metrics]]
id = "a"
name = "A"
description = "d"
"#,
        )
        .map_err(|p| std::io::Error::other(p.join(";")))?;
        Ok(Fixture {
            plugin: Plugin {
                detail: manifest.detail,
                dir: plugin_dir,
                entrypoint: manifest.entrypoint,
            },
            work: dir.path().join("work"),
            _dir: dir,
        })
    }

    fn job() -> JobFile {
        JobFile {
            schema_version: 1,
            job_id: "j1".into(),
            test_id: "t".into(),
            video_path: "/nonexistent.mp4".into(),
            landmarks_path: None,
            metrics: vec!["a".into()],
            parameters: BTreeMap::new(),
        }
    }

    fn quick() -> Limits {
        Limits {
            timeout: Duration::from_secs(10),
            max_output_bytes: 100_000,
        }
    }

    const GOOD: &str = r#"
import json, sys
job = json.load(open(sys.argv[1]))
out = {"schema_version": 1, "measures": [{"metric_id": m, "value": 1.5} for m in job["metrics"]]}
json.dump(out, open(sys.argv[2], "w"))
"#;

    #[test]
    fn a_well_behaved_plugin_returns_its_result() -> Result<(), Box<dyn std::error::Error>> {
        let f = fixture(GOOD)?;
        let result = run(&python(), &f.plugin, &job(), &f.work, quick())?.result;
        assert_eq!(result.measures[0].value, Some(1.5));
        assert!(f.work.join("job.json").is_file());
        Ok(())
    }

    #[test]
    fn the_plugin_sees_the_job_file() -> Result<(), Box<dyn std::error::Error>> {
        let script = r#"
import json, sys
job = json.load(open(sys.argv[1]))
assert job["test_id"] == "t" and job["schema_version"] == 1
json.dump({"schema_version": 1, "measures": [{"metric_id": "a", "value": float(len(job["job_id"]))}]}, open(sys.argv[2], "w"))
"#;
        let f = fixture(script)?;
        let result = run(&python(), &f.plugin, &job(), &f.work, quick())?.result;
        assert_eq!(result.measures[0].value, Some(2.0));
        Ok(())
    }

    #[test]
    fn a_crash_is_reported_with_its_exit_status() -> Result<(), Box<dyn std::error::Error>> {
        let f = fixture("raise RuntimeError('boom with secret path /opt/x')")?;
        let error = run(&python(), &f.plugin, &job(), &f.work, quick()).err();
        match error {
            Some(RunError::PluginFailed(text)) => {
                assert!(text.contains("exit status 1"));
                assert!(!text.contains("secret"), "stderr must not reach the user");
            }
            other => return Err(format!("unexpected: {other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn a_plugin_can_explain_its_failure() -> Result<(), Box<dyn std::error::Error>> {
        let script = r#"
import json, sys
json.dump({"error": "No hand was visible in the video."}, open("error.json", "w"))
sys.exit(3)
"#;
        let f = fixture(script)?;
        let error = run(&python(), &f.plugin, &job(), &f.work, quick()).err();
        assert_eq!(
            error,
            Some(RunError::PluginFailed(
                "No hand was visible in the video.".into()
            ))
        );
        Ok(())
    }

    #[test]
    fn a_hung_plugin_is_killed_at_the_time_limit() -> Result<(), Box<dyn std::error::Error>> {
        let f = fixture("import time\ntime.sleep(60)")?;
        let limits = Limits {
            timeout: Duration::from_millis(300),
            max_output_bytes: 100_000,
        };
        let started = Instant::now();
        let error = run(&python(), &f.plugin, &job(), &f.work, limits).err();
        assert_eq!(error, Some(RunError::TimedOut));
        // The plug-in sleeps for 60 seconds; a generous bound keeps a slow, busy machine from
        // failing the test while still proving the sleep was cut short.
        assert!(started.elapsed() < Duration::from_secs(30));
        Ok(())
    }

    #[test]
    fn relative_paths_work_although_the_plugin_runs_in_its_own_folder(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // A folder made relative to the current one, as `plugins` and `data` are in real use.
        let mut f = fixture_in(tempfile::tempdir_in(".")?, GOOD)?;
        let here = std::env::current_dir()?;
        f.plugin.dir = f.plugin.dir.strip_prefix(&here)?.to_path_buf();
        f.work = f.work.strip_prefix(&here)?.to_path_buf();
        assert!(f.plugin.dir.is_relative() && f.work.is_relative());
        let output = run(&python(), &f.plugin, &job(), &f.work, quick())?;
        assert_eq!(output.result.measures.len(), 1);
        Ok(())
    }

    #[test]
    fn endless_output_is_cut_off() -> Result<(), Box<dyn std::error::Error>> {
        let script =
            "import sys\nwhile True:\n    sys.stdout.write('x' * 100000)\n    sys.stdout.flush()";
        let f = fixture(script)?;
        let limits = Limits {
            timeout: Duration::from_secs(20),
            max_output_bytes: 50_000,
        };
        let error = run(&python(), &f.plugin, &job(), &f.work, limits).err();
        assert_eq!(error, Some(RunError::TooMuchOutput));
        Ok(())
    }

    #[test]
    fn missing_and_malformed_results_are_bad_output() -> Result<(), Box<dyn std::error::Error>> {
        let none = fixture("pass")?;
        assert!(
            matches!(run(&python(), &none.plugin, &job(), &none.work, quick()), Err(RunError::BadOutput(m)) if m.contains("no result"))
        );
        let junk = fixture("import sys\nopen(sys.argv[2], 'w').write('not json')")?;
        assert!(matches!(
            run(&python(), &junk.plugin, &job(), &junk.work, quick()),
            Err(RunError::BadOutput(_))
        ));
        let wrong = fixture("import sys, json\njson.dump({'schema_version': 1, 'measures': []}, open(sys.argv[2], 'w'))")?;
        assert!(
            matches!(run(&python(), &wrong.plugin, &job(), &wrong.work, quick()), Err(RunError::BadOutput(m)) if m.contains("did not return"))
        );
        Ok(())
    }

    #[test]
    fn signals_are_returned_when_valid_and_refused_when_not(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let write_signals = |body: &str| {
            format!(
                "import json, sys\njob = json.load(open(sys.argv[1]))\n\
                 json.dump({{'schema_version': 1, 'measures': [{{'metric_id': 'a', 'value': 1.0}}]}}, open(sys.argv[2], 'w'))\n\
                 open('signals.json', 'w').write({body:?})\n"
            )
        };
        let good = r#"{"schema_version":1,"series":[],"events":[{"id":"e","name":"E","unit":"","times":[1.0],"values":[2.0]}]}"#;
        let f = fixture(&write_signals(good))?;
        let output = run(&python(), &f.plugin, &job(), &f.work, quick())?;
        assert_eq!(output.signals.map(|s| s.events.len()), Some(1));
        let bad = r#"{"schema_version":1,"series":[],"events":[{"id":"e","name":"E","unit":"","times":[1.0,2.0],"values":[2.0]}]}"#;
        let f = fixture(&write_signals(bad))?;
        let error = run(&python(), &f.plugin, &job(), &f.work, quick()).err();
        assert!(matches!(error, Some(RunError::BadOutput(_))));
        Ok(())
    }

    #[test]
    fn a_missing_interpreter_is_a_launch_error() -> Result<(), Box<dyn std::error::Error>> {
        let f = fixture(GOOD)?;
        let error = run(
            Path::new("/definitely/not/python"),
            &f.plugin,
            &job(),
            &f.work,
            quick(),
        )
        .err();
        assert!(matches!(error, Some(RunError::Launch(_))));
        Ok(())
    }

    #[test]
    fn launch_errors_are_the_only_ones_with_internal_detail() {
        for error in [RunError::TimedOut, RunError::TooMuchOutput] {
            assert!(!error.to_string().contains('/'));
        }
    }
}
