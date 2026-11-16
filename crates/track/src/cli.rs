//! Command-line arguments and the whole run.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use axonhand_hand::{Device, HandError, HandTracker, Model, Placement, Runtime};

use crate::npz::write_npz;
use crate::overlay::overlay_json;
use crate::stream::FrameStream;
use crate::track::{track_stream, TrackError};

const PALM_MODEL: &str = "palm_detection_mediapipe_2023feb.onnx";
const LANDMARK_MODEL: &str = "handpose_estimation_mediapipe_2023feb.onnx";

/// The usage text, shown when the arguments are wrong.
pub const USAGE: &str = "usage: axonhand-track OUT.npz [--models DIR] [--device auto|cpu|npu] \
[--library ONNXRUNTIME] [--npu-name NAME --npu-library PATH]\n\
Reads the frames written by `python -m axonhand.tools.decode_frames` on standard input and writes \
OUT.npz and, beside it, OUT.json.";

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    /// Where to write the landmark track (`.npz`); the overlay goes beside it as `.json`.
    pub output: PathBuf,
    /// Folder holding the two ONNX files.
    pub models: PathBuf,
    /// Where the models should run.
    pub device: Device,
    /// The ONNX Runtime shared library; the `ORT_DYLIB_PATH` variable when absent.
    pub library: Option<PathBuf>,
    /// Name and library of the NPU provider to register.
    pub npu: Option<(String, PathBuf)>,
}

impl Args {
    /// Read the arguments (without the program name). `default_models` is used when `--models`
    /// is not given.
    ///
    /// # Errors
    ///
    /// A sentence saying what is wrong.
    pub fn parse<I: IntoIterator<Item = String>>(
        arguments: I,
        default_models: PathBuf,
    ) -> Result<Self, String> {
        let mut output = None;
        let mut models = default_models;
        let mut device = Device::Auto;
        let (mut library, mut name, mut npu_library) = (None, None, None);
        let mut iterator = arguments.into_iter();
        while let Some(argument) = iterator.next() {
            let mut value = || {
                iterator
                    .next()
                    .ok_or_else(|| format!("{argument} needs a value"))
            };
            match argument.as_str() {
                "--models" => models = PathBuf::from(value()?),
                "--device" => {
                    device = match value()?.as_str() {
                        "auto" => Device::Auto,
                        "cpu" => Device::Cpu,
                        "npu" => Device::Npu,
                        other => {
                            return Err(format!("--device must be auto, cpu or npu, not {other}"))
                        }
                    }
                }
                "--library" => library = Some(PathBuf::from(value()?)),
                "--npu-name" => name = Some(value()?),
                "--npu-library" => npu_library = Some(PathBuf::from(value()?)),
                flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
                path => {
                    if output.replace(PathBuf::from(path)).is_some() {
                        return Err("more than one output file was given".into());
                    }
                }
            }
        }
        let npu = match (name, npu_library) {
            (Some(name), Some(library)) => Some((name, library)),
            (None, None) => None,
            _ => return Err("--npu-name and --npu-library go together".into()),
        };
        Ok(Self {
            output: output.ok_or("the output file is missing")?,
            models,
            device,
            library,
            npu,
        })
    }
}

/// What a finished run reports.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Frames tracked.
    pub frames: usize,
    /// Frames in which a hand was found.
    pub found: usize,
    /// Seconds spent, decoding excluded (it happens in another program).
    pub seconds: f64,
    /// Times the palm detector ran.
    pub detections: usize,
    /// Where the palm detector ran.
    pub palm: Placement,
    /// Where the landmark model ran.
    pub landmarks: Placement,
    /// Reasons the CPU was used when the NPU was wanted.
    pub notes: Vec<String>,
}

fn place(placement: Placement) -> &'static str {
    if placement == Placement::Npu {
        "NPU"
    } else {
        "CPU"
    }
}

impl Summary {
    /// The lines printed on success; the server keeps them in its log.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "{} frames in {:.1} s; palm detector on {} ({} runs), landmark model on {}; \
             hand found in {}",
            self.frames,
            self.seconds,
            place(self.palm),
            self.detections,
            place(self.landmarks),
            self.found
        )];
        lines.extend(self.notes.iter().map(|n| format!("note: {n}")));
        lines
    }
}

/// The NPU plug-in to load, if any. It is left out for a CPU-only run: registering the plug-in
/// changed the numbers a CPU run produced (measured on the laptop, 2026-11-13: a CPU run with the
/// plug-in registered differed from Python by up to half a palm length, and without it was
/// identical), so a CPU run must not load it.
fn npu_to_register(args: &Args) -> Option<&(String, PathBuf)> {
    if args.device == Device::Cpu {
        None
    } else {
        args.npu.as_ref()
    }
}

fn open_models(args: &Args) -> Result<(Model, Model), TrackError> {
    let runtime = Runtime::load(args.library.as_deref())?;
    // When the NPU is demanded, anything that stops it being used is the person's to know about,
    // not a fault: they can choose the CPU.
    let npu_unusable = |error: HandError| {
        if args.device == Device::Npu && !matches!(error, HandError::ModelMissing(_)) {
            eprintln!("NPU: {error}");
            TrackError::NpuUnavailable
        } else {
            TrackError::from(error)
        }
    };
    if let Some((name, library)) = npu_to_register(args) {
        runtime
            .register_npu_library(name, library)
            .map_err(npu_unusable)?;
    }
    let open = |file: &str| Model::open_on(&runtime, &args.models.join(file), args.device);
    let palm = open(PALM_MODEL).map_err(npu_unusable)?;
    let landmarks = open(LANDMARK_MODEL).map_err(npu_unusable)?;
    Ok((palm, landmarks))
}

fn write_files(output: &Path, fps: f64, frames: &[Option<crate::Points>]) -> std::io::Result<()> {
    write_npz(output, fps, frames)?;
    std::fs::write(output.with_extension("json"), overlay_json(fps, frames))
}

/// Track the hand in the frames read from `input` and write the files.
///
/// # Errors
///
/// A [`TrackError`]; [`TrackError::is_for_the_person`] says whether the video or the software is
/// at fault.
pub fn run<R: Read>(args: &Args, input: R) -> Result<Summary, TrackError> {
    let (palm, landmarks) = open_models(args)?;
    let mut notes = Vec::new();
    for (name, model) in [("palm detector", &palm), ("landmark model", &landmarks)] {
        if !model.note().is_empty() {
            notes.push(format!("{name}: {}", model.note()));
        }
    }
    let placements = (palm.placement(), landmarks.placement());
    let mut stream = FrameStream::open(input)?;
    let mut tracker = HandTracker::new(palm, landmarks);
    let started = Instant::now();
    let track = track_stream(&mut stream, &mut tracker)?;
    let seconds = started.elapsed().as_secs_f64();
    write_files(&args.output, track.fps, &track.frames)
        .map_err(|e| TrackError::Stream(crate::StreamError::Io(e)))?;
    Ok(Summary {
        frames: track.frames.len(),
        found: track.found(),
        seconds,
        detections: tracker.detections(),
        palm: placements.0,
        landmarks: placements.1,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Args, String> {
        Args::parse(
            arguments.iter().map(|a| (*a).to_owned()),
            PathBuf::from("models"),
        )
    }

    #[test]
    fn the_output_alone_uses_the_defaults() -> Result<(), String> {
        let args = parse(&["track.npz"])?;
        assert_eq!(args.output, PathBuf::from("track.npz"));
        assert_eq!(args.models, PathBuf::from("models"));
        assert_eq!(args.device, Device::Auto);
        assert!(args.library.is_none() && args.npu.is_none());
        Ok(())
    }

    #[test]
    fn a_cpu_run_does_not_load_the_npu_plug_in() -> Result<(), String> {
        let with = |device: &str| {
            parse(&[
                "track.npz",
                "--device",
                device,
                "--npu-name",
                "QNN",
                "--npu-library",
                "qnn.dll",
            ])
        };
        assert!(npu_to_register(&with("cpu")?).is_none());
        assert!(npu_to_register(&with("auto")?).is_some());
        assert!(npu_to_register(&with("npu")?).is_some());
        Ok(())
    }

    #[test]
    fn every_option_is_read() -> Result<(), String> {
        let args = parse(&[
            "--device",
            "npu",
            "out.npz",
            "--models",
            "m",
            "--library",
            "ort.dll",
            "--npu-name",
            "QNN",
            "--npu-library",
            "qnn.dll",
        ])?;
        assert_eq!(args.device, Device::Npu);
        assert_eq!(args.models, PathBuf::from("m"));
        assert_eq!(args.library, Some(PathBuf::from("ort.dll")));
        assert_eq!(args.npu, Some(("QNN".to_owned(), PathBuf::from("qnn.dll"))));
        Ok(())
    }

    #[test]
    fn mistakes_are_explained() {
        for (arguments, expect) in [
            (vec![], "output file is missing"),
            (vec!["a", "b"], "more than one output"),
            (vec!["a", "--device", "gpu"], "auto, cpu or npu"),
            (vec!["a", "--models"], "needs a value"),
            (vec!["a", "--what", "x"], "unknown option"),
            (vec!["a", "--npu-name", "n"], "go together"),
        ] {
            let message = parse(&arguments).err().unwrap_or_default();
            assert!(message.contains(expect), "{arguments:?}: {message}");
        }
    }

    #[test]
    fn the_summary_says_where_each_model_ran_and_why_the_cpu_was_used() {
        let summary = Summary {
            frames: 10,
            found: 9,
            seconds: 0.5,
            detections: 2,
            palm: Placement::Cpu,
            landmarks: Placement::Npu,
            notes: vec!["palm detector: no NPU was found".into()],
        };
        let lines = summary.lines();
        assert!(lines[0].contains("palm detector on CPU (2 runs), landmark model on NPU"));
        assert!(lines[0].contains("hand found in 9"));
        assert_eq!(lines[1], "note: palm detector: no NPU was found");
    }
}
