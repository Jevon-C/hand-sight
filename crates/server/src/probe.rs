//! Reading a video's facts by running the Python probe tool as a separate process.
//!
//! A separate process keeps a crash or hang in the video library from taking the server
//! down, and lets the same tool be used from the command line.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::Deserialize;

/// Facts about a video, as printed by `python -m axonhand.tools.probe`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProbeResult {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Length in seconds.
    pub duration_seconds: f64,
    /// Number of frames.
    pub frame_count: u32,
    /// Codec name.
    pub codec: String,
}

/// Why a video could not be probed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeFailure {
    /// The file is not a usable video. The text is safe to show to the user.
    #[error("{0}")]
    NotAVideo(String),
    /// The probe could not run or printed something unexpected. For the log only.
    #[error("probe failed: {0}")]
    Broken(String),
}

/// Something that can probe a video file. Blocking; call it from `spawn_blocking`.
pub type Prober = Arc<dyn Fn(&Path) -> Result<ProbeResult, ProbeFailure> + Send + Sync>;

/// A [`Prober`] that runs `python -m axonhand.tools.probe` with the given interpreter.
#[must_use]
pub fn python_prober(python: PathBuf) -> Prober {
    Arc::new(move |path| run_python(&python, path))
}

fn run_python(python: &Path, video: &Path) -> Result<ProbeResult, ProbeFailure> {
    let output = Command::new(python)
        .args(["-m", "axonhand.tools.probe"])
        .arg(video)
        .output()
        .map_err(|e| ProbeFailure::Broken(format!("cannot run {}: {e}", python.display())))?;
    parse_output(output.status.code(), &output.stdout, &output.stderr)
}

/// Interpret the probe's exit code and output.
fn parse_output(
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<ProbeResult, ProbeFailure> {
    match code {
        Some(0) => serde_json::from_slice(stdout)
            .map_err(|e| ProbeFailure::Broken(format!("unreadable probe output: {e}"))),
        Some(2) => {
            #[derive(Deserialize)]
            struct Failure {
                error: String,
            }
            serde_json::from_slice::<Failure>(stdout)
                .map(|f| ProbeFailure::NotAVideo(f.error))
                .map_or_else(
                    |e| {
                        Err(ProbeFailure::Broken(format!(
                            "unreadable probe output: {e}"
                        )))
                    },
                    Err,
                )
        }
        other => Err(ProbeFailure::Broken(format!(
            "exit status {other:?}: {}",
            String::from_utf8_lossy(stderr).trim()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &[u8] =
        br#"{"width":64,"height":48,"fps":60.0,"duration_seconds":1.0,"frame_count":60,"codec":"h264"}"#;

    #[test]
    fn success_output_is_parsed() {
        let result = parse_output(Some(0), GOOD, b"");
        assert_eq!(result.map(|r| (r.width, r.frame_count)), Ok((64, 60)));
    }

    #[test]
    fn exit_two_means_not_a_video_with_the_tools_message() {
        let result = parse_output(
            Some(2),
            br#"{"error":"the file is not a readable video"}"#,
            b"",
        );
        assert_eq!(
            result,
            Err(ProbeFailure::NotAVideo(
                "the file is not a readable video".to_owned()
            ))
        );
    }

    #[test]
    fn crashes_and_garbage_are_broken_not_user_errors() {
        assert!(
            matches!(parse_output(Some(1), b"", b"Traceback"), Err(ProbeFailure::Broken(m)) if m.contains("Traceback"))
        );
        assert!(matches!(
            parse_output(None, b"", b""),
            Err(ProbeFailure::Broken(_))
        ));
        assert!(matches!(
            parse_output(Some(0), b"not json", b""),
            Err(ProbeFailure::Broken(_))
        ));
        assert!(matches!(
            parse_output(Some(2), b"???", b""),
            Err(ProbeFailure::Broken(_))
        ));
    }

    #[test]
    fn a_missing_interpreter_is_broken() {
        let prober = python_prober(PathBuf::from("/definitely/not/python"));
        assert!(matches!(
            prober(Path::new("x.mp4")),
            Err(ProbeFailure::Broken(_))
        ));
    }
}
