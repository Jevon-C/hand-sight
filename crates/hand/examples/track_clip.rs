//! Track the hand through frames exported by `python -m axonhand.tools.parity_fixture --clip`,
//! on the CPU or the NPU, and report the speed and the difference from Python's landmarks.
//!
//! ```text
//! cargo run --release -p axonhand-hand --example track_clip -- \
//!     --library <onnxruntime.dll> --models <folder> --clip <folder> --device npu \
//!     --npu-name <name> --npu-library <path> [--out landmarks.json]
//! ```
//!
//! For Qualcomm, `--npu-name` and `--npu-library` are the values of
//! `onnxruntime_qnn.get_ep_name()` and `onnxruntime_qnn.get_library_path()`.

use std::path::PathBuf;
use std::time::Instant;

use axonhand_hand::{Device, HandTracker, Model, Placement, RgbImage, Runtime};

type Landmarks = Vec<Option<Vec<[f64; 3]>>>;

struct Args {
    library: PathBuf,
    models: PathBuf,
    clip: PathBuf,
    device: Device,
    npu: Option<(String, PathBuf)>,
    out: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut values = std::collections::BTreeMap::new();
    let mut args = std::env::args().skip(1);
    while let Some(key) = args.next() {
        let value = args.next().ok_or(format!("{key} needs a value"))?;
        values.insert(key, value);
    }
    let get = |key: &str| values.get(key).cloned().ok_or(format!("missing {key}"));
    let device = match values.get("--device").map(String::as_str) {
        None | Some("auto") => Device::Auto,
        Some("cpu") => Device::Cpu,
        Some("npu") => Device::Npu,
        Some(other) => return Err(format!("--device must be auto, cpu or npu, not {other}")),
    };
    let npu = match (values.get("--npu-name"), values.get("--npu-library")) {
        (Some(name), Some(library)) => Some((name.clone(), PathBuf::from(library))),
        (None, None) => None,
        _ => return Err("--npu-name and --npu-library go together".into()),
    };
    Ok(Args {
        library: PathBuf::from(get("--library")?),
        models: PathBuf::from(get("--models")?),
        clip: PathBuf::from(get("--clip")?),
        device,
        npu,
        out: values.get("--out").map(PathBuf::from),
    })
}

fn run(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::load(Some(&args.library))?;
    if let Some((name, library)) = &args.npu {
        runtime.register_npu_library(name, library)?;
    }
    let palm = Model::open_on(
        &runtime,
        &args.models.join("palm_detection_mediapipe_2023feb.onnx"),
        args.device,
    )?;
    let landmarks = Model::open_on(
        &runtime,
        &args
            .models
            .join("handpose_estimation_mediapipe_2023feb.onnx"),
        args.device,
    )?;
    let name = |p: Placement| if p == Placement::Npu { "NPU" } else { "CPU" };
    println!(
        "palm detector on {}, landmark model on {}",
        name(palm.placement()),
        name(landmarks.placement())
    );
    for model in [&palm, &landmarks] {
        if !model.note().is_empty() {
            println!("note: {}", model.note());
        }
    }
    let expected: Option<Landmarks> = std::fs::read(args.clip.join("landmarks.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let count = std::fs::read_dir(&args.clip)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "ppm"))
        .count();
    let frames = (0..count)
        .map(|i| RgbImage::from_ppm(&args.clip.join(format!("frame_{i:04}.ppm"))))
        .collect::<Result<Vec<_>, _>>()?;
    let mut tracker = HandTracker::new(palm, landmarks);
    let (mut worst, mut found, mut disagree) = (0.0_f64, 0, 0);
    let mut written: Landmarks = Vec::with_capacity(frames.len());
    let started = Instant::now();
    for (index, frame) in frames.iter().enumerate() {
        let got = tracker.process(frame)?;
        found += usize::from(got.is_some());
        written.push(got.as_ref().map(|g| g.points.to_vec()));
        if let Some(Some(want)) = expected.as_ref().map(|e| e.get(index)) {
            match (&got, want) {
                (Some(g), Some(w)) => {
                    for (a, b) in g.points.iter().zip(w) {
                        worst = worst.max((a[0] - b[0]).abs()).max((a[1] - b[1]).abs());
                    }
                }
                (None, None) => {}
                _ => disagree += 1,
            }
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    let n = frames.len().max(1) as f64;
    println!(
        "{} frames, hand found in {found}, {:.2} ms per frame ({:.0} fps), palm detector run {} times",
        frames.len(),
        1000.0 * seconds / n,
        n / seconds,
        tracker.detections()
    );
    if let Some(out) = &args.out {
        std::fs::write(out, serde_json::to_vec(&written)?)?;
        println!("landmarks written to {}", out.display());
    }
    if expected.is_some() {
        println!("against Python: {disagree} frames disagree on a hand being present; largest landmark difference {worst:.4} pixels");
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let args = match parse() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return std::process::ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
