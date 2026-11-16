//! The real pipe: Python decodes, this program tracks, with the real models.
//!
//! Runs only when `AXONHAND_PYTHON`, `AXONHAND_MODELS` and `ORT_DYLIB_PATH` are set. A synthetic
//! video has no real hand in it, so these tests check the plumbing and the plain-language
//! failure; the laptop script `scripts/try_rust_tracker.ps1` checks the landmarks on a real clip.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn environment() -> Option<(PathBuf, PathBuf)> {
    let python = std::env::var_os("AXONHAND_PYTHON")?;
    let models = std::env::var_os("AXONHAND_MODELS")?;
    std::env::var_os("ORT_DYLIB_PATH")?;
    Some((PathBuf::from(python), PathBuf::from(models)))
}

fn temp(name: &str) -> Result<PathBuf, std::io::Error> {
    let dir = std::env::temp_dir().join(format!("axonhand-pipe-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

struct Piped {
    decoder_status: Option<i32>,
    decoder_stderr: String,
    tracker_status: Option<i32>,
    tracker_stderr: String,
}

fn pipe(python: &Path, models: &Path, video: &Path, output: &Path) -> std::io::Result<Piped> {
    let mut decoder = Command::new(python)
        .args(["-m", "axonhand.tools.decode_frames"])
        .arg(video)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let frames = decoder
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("no decoder output"))?;
    let tracker = Command::new(env!("CARGO_BIN_EXE_axonhand-track"))
        .arg(output)
        .arg("--models")
        .arg(models)
        .args(["--device", "cpu"])
        .stdin(Stdio::from(frames))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    let decoded = decoder.wait_with_output()?;
    Ok(Piped {
        decoder_status: decoded.status.code(),
        decoder_stderr: String::from_utf8_lossy(&decoded.stderr).into_owned(),
        tracker_status: tracker.status.code(),
        tracker_stderr: String::from_utf8_lossy(&tracker.stderr).into_owned(),
    })
}

#[test]
fn a_video_with_no_hand_is_refused_in_plain_words_and_writes_nothing() -> std::io::Result<()> {
    let Some((python, models)) = environment() else {
        eprintln!("skipped: set AXONHAND_PYTHON, AXONHAND_MODELS and ORT_DYLIB_PATH");
        return Ok(());
    };
    let dir = temp("nohand")?;
    let video = dir.join("dots.mp4");
    let made = Command::new(&python)
        .args(["-m", "axonhand.tools.make_test_video"])
        .arg(&video)
        .args(["--taps", "4", "--rate", "2.0"])
        .output()?;
    assert!(made.status.success());
    let output = dir.join("track.npz");
    let result = pipe(&python, &models, &video, &output)?;
    assert_eq!(result.decoder_status, Some(0), "{}", result.decoder_stderr);
    assert_eq!(result.tracker_status, Some(2), "{}", result.tracker_stderr);
    assert!(
        result
            .tracker_stderr
            .contains("axonhand-message: No hand was found in this video."),
        "{}",
        result.tracker_stderr
    );
    assert!(!output.exists());
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn a_file_that_is_not_a_video_is_the_decoders_message_to_the_person() -> std::io::Result<()> {
    let Some((python, models)) = environment() else {
        eprintln!("skipped: set AXONHAND_PYTHON, AXONHAND_MODELS and ORT_DYLIB_PATH");
        return Ok(());
    };
    let dir = temp("junk")?;
    let video = dir.join("junk.mp4");
    std::fs::write(&video, b"not a video")?;
    let result = pipe(&python, &models, &video, &dir.join("track.npz"))?;
    assert_eq!(result.decoder_status, Some(2));
    assert!(result.decoder_stderr.contains("axonhand-message: "));
    // The tracker sees an empty stream, which is a fault of the pipe, not of the video; the
    // server reads the decoder's status first.
    assert_eq!(result.tracker_status, Some(1), "{}", result.tracker_stderr);
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn wrong_arguments_exit_with_the_usage_status() -> std::io::Result<()> {
    let out = Command::new(env!("CARGO_BIN_EXE_axonhand-track"))
        .arg("--device")
        .arg("gpu")
        .stdin(Stdio::null())
        .output()?;
    assert_eq!(out.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"));
    Ok(())
}

#[test]
fn asking_for_an_npu_that_is_not_there_is_refused_in_plain_words() -> std::io::Result<()> {
    let Some((_, models)) = environment() else {
        eprintln!("skipped: set AXONHAND_PYTHON, AXONHAND_MODELS and ORT_DYLIB_PATH");
        return Ok(());
    };
    let dir = temp("nonpu")?;
    let result = Command::new(env!("CARGO_BIN_EXE_axonhand-track"))
        .arg(dir.join("track.npz"))
        .arg("--models")
        .arg(&models)
        .args(["--device", "npu"])
        .stdin(Stdio::null())
        .output()?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    // A machine that does have a working NPU would succeed in opening the models, so the
    // assertion only applies where there is none to find; with no plug-in registered there never
    // is one.
    assert_eq!(result.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("axonhand-message: The NPU could not be used on this computer."),
        "{stderr}"
    );
    assert!(!dir.join("track.npz").exists());
    Ok(())
}
