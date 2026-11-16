//! The hand model run by the Rust tracker.
//!
//! Two programs run as a pipe: Python decodes the video to raw frames
//! (`axonhand.tools.decode_frames`), and `axonhand-track` finds the hand in each frame with the
//! Rust pipeline, on the NPU when there is one, and writes the same landmark files the Python
//! pipeline writes. The rest of the server cannot tell the two apart except by the model's name
//! and hash, so results made by one are never mistaken for results made by the other.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axonhand_api::{ComputeDevice, ModelInfo};

use crate::jobs::{models_fingerprint, user_message, ExtractFailure, ExtractFn, Extractor};
use crate::jobs::{EXTRACT_TIMEOUT, LANDMARK_MODEL, PALM_MODEL, PIPELINE_VERSION};

/// What a person is told when they ask for the NPU and its software is not installed.
const NO_NPU_PACKAGE: &str =
    "The NPU is not available on this computer. Run the analysis on the CPU instead.";

/// The file name of the tracker program on this system.
#[must_use]
pub fn track_binary_name() -> String {
    format!("axonhand-track{}", std::env::consts::EXE_SUFFIX)
}

/// Where the tracker program is: the configured path when it names a folder, otherwise next to
/// the server program, otherwise the bare name, which the system searches for on `PATH`.
#[must_use]
pub fn locate_track_binary(configured: &Path) -> PathBuf {
    if configured.components().count() > 1 {
        return configured.to_path_buf();
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(track_binary_name())));
    match beside {
        Some(path) if path.is_file() => path,
        _ => configured.to_path_buf(),
    }
}

/// Where the Python package keeps ONNX Runtime and the NPU provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct OrtPaths {
    /// The ONNX Runtime shared library.
    pub library: Option<PathBuf>,
    /// The NPU provider's name, when its package is installed.
    pub npu_name: Option<String>,
    /// The NPU provider's library.
    pub npu_library: Option<PathBuf>,
}

/// Ask Python where ONNX Runtime is, so the Rust program loads the same one.
///
/// # Errors
///
/// A sentence saying why the paths could not be found.
pub fn find_ort_paths(python: &Path) -> Result<OrtPaths, String> {
    let output = Command::new(python)
        .args(["-m", "axonhand.tools.ort_paths"])
        .env("PYTHONUTF8", "1")
        .output()
        .map_err(|e| format!("cannot run {} to find ONNX Runtime: {e}", python.display()))?;
    if !output.status.success() {
        return Err(format!(
            "cannot find ONNX Runtime: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let paths: OrtPaths = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("cannot read where ONNX Runtime is: {e}"))?;
    if paths.library.is_none() {
        return Err("the onnxruntime package has no shared library to load".to_owned());
    }
    Ok(paths)
}

/// The command that decodes `video` to raw frames on its standard output.
fn decoder_command(python: &Path, video: &Path) -> Command {
    let mut command = Command::new(python);
    command
        .args(["-m", "axonhand.tools.decode_frames"])
        .arg(video)
        .env("PYTHONUTF8", "1");
    command
}

/// The command that tracks the hand in the frames on its standard input.
fn tracker_command(
    binary: &Path,
    models_dir: &Path,
    paths: &OrtPaths,
    output: &Path,
    device: ComputeDevice,
) -> Command {
    let mut command = Command::new(binary);
    command
        .arg(output)
        .arg("--models")
        .arg(models_dir)
        .arg("--device")
        .arg(device.flag());
    if let Some(library) = &paths.library {
        command.arg("--library").arg(library);
    }
    // The NPU's plug-in is only given when the NPU is wanted: loading it changes what a CPU run
    // produces (see docs/RUST-PORT.md).
    if device != ComputeDevice::Npu {
        return command;
    }
    if let (Some(name), Some(library)) = (&paths.npu_name, &paths.npu_library) {
        command
            .arg("--npu-name")
            .arg(name)
            .arg("--npu-library")
            .arg(library);
    }
    command
}

/// Decide the outcome of the pipe from the two programs' exit statuses.
///
/// A video the decoder cannot read is the person's problem (status 2); so is a video with no hand
/// in it. If the tracker failed, the decoder's own failure is only its pipe closing, so the
/// tracker's explanation wins.
fn verdict(
    decoder: Option<i32>,
    tracker: Option<i32>,
    decoder_error: &str,
    tracker_error: &str,
) -> Result<(), ExtractFailure> {
    if decoder == Some(2) {
        return Err(ExtractFailure::NotUsable(user_message(decoder_error)));
    }
    match tracker {
        Some(0) => {}
        Some(2) => return Err(ExtractFailure::NotUsable(user_message(tracker_error))),
        other => {
            return Err(ExtractFailure::Broken(format!(
                "the tracker exited with status {other:?}: {}",
                tracker_error.trim()
            )))
        }
    }
    match decoder {
        Some(0) => Ok(()),
        other => Err(ExtractFailure::Broken(format!(
            "the decoder exited with status {other:?}: {}",
            decoder_error.trim()
        ))),
    }
}

fn broken(what: &str, error: &dyn std::fmt::Display) -> ExtractFailure {
    ExtractFailure::Broken(format!("{what}: {error}"))
}

/// Note the exit status of `child` once it has finished.
fn poll(child: &mut Child, status: &mut Option<Option<i32>>) -> std::io::Result<()> {
    if status.is_none() {
        if let Some(done) = child.try_wait()? {
            *status = Some(done.code());
        }
    }
    Ok(())
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Run the pipe. The programs' output is kept next to the landmarks (`*.decode.err.log`,
/// `*.out.log`, `*.err.log`) so a failure can be examined afterwards.
fn run_pipe(
    mut decoder: Command,
    mut tracker: Command,
    output: &Path,
) -> Result<(), ExtractFailure> {
    let log = |name: &str| {
        std::fs::File::create(output.with_extension(name)).map_err(|e| broken("log file", &e))
    };
    let mut decode_child = decoder
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log("decode.err.log")?))
        .spawn()
        .map_err(|e| broken("cannot start the video decoder", &e))?;
    let frames = decode_child
        .stdout
        .take()
        .ok_or_else(|| ExtractFailure::Broken("the decoder has no output".to_owned()))?;
    let mut track_child = match tracker
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::from(frames))
        .stdout(Stdio::from(log("out.log")?))
        .stderr(Stdio::from(log("err.log")?))
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            kill(&mut decode_child);
            return Err(broken("cannot start the hand tracker", &e));
        }
    };
    let started = Instant::now();
    let (mut decoded, mut tracked) = (None, None);
    while decoded.is_none() || tracked.is_none() {
        let polled = poll(&mut decode_child, &mut decoded)
            .and_then(|()| poll(&mut track_child, &mut tracked));
        if let Err(e) = polled {
            kill(&mut decode_child);
            kill(&mut track_child);
            return Err(broken("waiting for the hand tracker", &e));
        }
        if started.elapsed() >= EXTRACT_TIMEOUT {
            kill(&mut decode_child);
            kill(&mut track_child);
            return Err(ExtractFailure::Broken(
                "the hand model took too long".to_owned(),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let text = |name: &str| {
        std::fs::read_to_string(output.with_extension(name))
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let outcome = verdict(
        decoded.flatten(),
        tracked.flatten(),
        &text("decode.err.log"),
        &text("err.log"),
    );
    if outcome.is_ok() {
        tracing::info!(summary = %text("out.log"), "hand model finished");
    }
    outcome
}

/// The real hand model, run by the Rust tracker.
///
/// # Errors
///
/// A sentence naming what is missing: a model file, ONNX Runtime, or the tracker program.
pub fn rust_extractor(
    python: PathBuf,
    models_dir: &Path,
    track_binary: &Path,
) -> Result<Extractor, String> {
    let models_dir = std::path::absolute(models_dir)
        .map_err(|e| format!("cannot use the models folder {}: {e}", models_dir.display()))?;
    for name in [PALM_MODEL, LANDMARK_MODEL] {
        let path = models_dir.join(name);
        if !path.is_file() {
            return Err(format!("cannot read {}: no such file", path.display()));
        }
    }
    let binary = locate_track_binary(track_binary);
    // Starting it with no arguments exits at once with its usage text; that proves it can run.
    Command::new(&binary)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run the hand tracker {}: {e}", binary.display()))?;
    let paths = find_ort_paths(&python)?;
    let hash = models_fingerprint(&models_dir, &format!("{PIPELINE_VERSION}-rust"))?;
    let run: ExtractFn = Arc::new(move |video, output, device| {
        if device == ComputeDevice::Npu && (paths.npu_name.is_none() || paths.npu_library.is_none())
        {
            return Err(ExtractFailure::NotUsable(NO_NPU_PACKAGE.to_owned()));
        }
        run_pipe(
            decoder_command(&python, video),
            tracker_command(&binary, &models_dir, &paths, output, device),
            output,
        )
    });
    Ok(Extractor {
        model: ModelInfo {
            name: "MediaPipe hand landmarks (ONNX, Rust tracker)".to_owned(),
            hash,
            stand_in: false,
            device: String::new(),
        },
        run,
        validated: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECODER_MESSAGE: &str =
        "warning from a library\naxonhand-message: The video could not be read.";
    const TRACKER_MESSAGE: &str =
        "axonhand-message: No hand was found in this video. Check that the hand is in view.";

    #[test]
    fn a_clean_run_is_ok() {
        assert_eq!(verdict(Some(0), Some(0), "", ""), Ok(()));
    }

    #[test]
    fn an_unreadable_video_is_the_decoders_message_even_though_the_tracker_then_failed() {
        assert_eq!(
            verdict(
                Some(2),
                Some(1),
                DECODER_MESSAGE,
                "the frame stream is empty"
            ),
            Err(ExtractFailure::NotUsable(
                "The video could not be read.".to_owned()
            ))
        );
    }

    #[test]
    fn a_video_with_no_hand_is_the_trackers_message() {
        assert_eq!(
            verdict(Some(0), Some(2), "", TRACKER_MESSAGE),
            Err(ExtractFailure::NotUsable(
                "No hand was found in this video. Check that the hand is in view.".to_owned()
            ))
        );
    }

    #[test]
    fn a_tracker_fault_wins_over_the_decoder_noticing_the_closed_pipe() {
        let outcome = verdict(Some(1), Some(1), "broken pipe", "hand model failed: boom");
        assert!(matches!(
            outcome,
            Err(ExtractFailure::Broken(text)) if text.contains("hand model failed: boom")
        ));
    }

    #[test]
    fn a_decoder_fault_after_a_good_tracker_run_is_still_a_fault() {
        assert!(matches!(
            verdict(Some(1), Some(0), "decoder exploded", ""),
            Err(ExtractFailure::Broken(text)) if text.contains("decoder exploded")
        ));
        assert!(matches!(
            verdict(None, Some(0), "", ""),
            Err(ExtractFailure::Broken(_))
        ));
        assert!(matches!(
            verdict(Some(0), None, "", ""),
            Err(ExtractFailure::Broken(_))
        ));
    }

    #[test]
    fn a_path_with_a_folder_is_used_as_given_and_a_bare_name_is_searched_for() {
        let given = Path::new("tools").join("axonhand-track");
        assert_eq!(locate_track_binary(&given), given);
        let bare = locate_track_binary(Path::new("definitely-not-installed-anywhere"));
        assert_eq!(bare, PathBuf::from("definitely-not-installed-anywhere"));
    }

    fn paths() -> OrtPaths {
        OrtPaths {
            library: Some(PathBuf::from("ort.dll")),
            npu_name: Some("QNN".to_owned()),
            npu_library: Some(PathBuf::from("qnn.dll")),
        }
    }

    fn arguments(device: ComputeDevice) -> Vec<String> {
        tracker_command(
            Path::new("t"),
            Path::new("m"),
            &paths(),
            Path::new("o.npz"),
            device,
        )
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn the_tracker_is_told_about_the_npu_only_when_the_npu_is_wanted() {
        assert_eq!(
            arguments(ComputeDevice::Npu),
            [
                "o.npz",
                "--models",
                "m",
                "--device",
                "npu",
                "--library",
                "ort.dll",
                "--npu-name",
                "QNN",
                "--npu-library",
                "qnn.dll"
            ]
        );
        // Loading the NPU's plug-in changes what a CPU run produces, so a CPU run never loads it.
        assert_eq!(
            arguments(ComputeDevice::Cpu),
            [
                "o.npz",
                "--models",
                "m",
                "--device",
                "cpu",
                "--library",
                "ort.dll"
            ]
        );
    }
}
