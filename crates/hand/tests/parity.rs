//! The Rust pipeline against numbers produced by the Python pipeline.
//!
//! `tests/fixtures/parity.json` is written by `python -m axonhand.tools.parity_fixture`. The
//! geometry and image tests always run. The model test needs the ONNX Runtime library and the two
//! model files, and is skipped (with a note) unless `ORT_DYLIB_PATH` and `AXONHAND_MODELS` are set.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;

use axonhand_hand::rois::{self, Letterbox, Palm, Rect};
use axonhand_hand::{Model, Network, RgbImage, Runtime};
use serde::Deserialize;

/// The one runtime of this test process. ONNX Runtime allows a single environment per process and
/// tests run on several threads, so every test that needs it takes it from here.
fn shared_runtime(library: &OsString) -> Result<Runtime, Box<dyn std::error::Error>> {
    static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Runtime::load(Some(PathBuf::from(library).as_path())).map_err(|e| e.to_string())
        })
        .clone()
        .map_err(Into::into)
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct Fixture {
    anchors: Anchors,
    geometry: Geometry,
    decode: Decode,
    image: Image,
    models: std::collections::BTreeMap<String, ModelCase>,
}

#[derive(Deserialize)]
struct Anchors {
    count: usize,
    sample: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
struct Geometry {
    letterboxes: Vec<LetterboxCase>,
    hands: Vec<HandCase>,
}

#[derive(Deserialize)]
struct LetterboxCase {
    size: [usize; 2],
    scale: f64,
    pad: [f64; 2],
    resized: [usize; 2],
}

#[derive(Deserialize)]
struct RectData {
    cx: f64,
    cy: f64,
    size: f64,
    rotation: f64,
}

#[derive(Deserialize)]
struct HandCase {
    points: Vec<[f64; 3]>,
    rect: RectData,
    matrix: [f64; 6],
    crop_points: Vec<[f64; 3]>,
    mapped: Vec<[f64; 3]>,
}

#[derive(Deserialize)]
struct Decode {
    raw: Vec<f32>,
    logits: Vec<f32>,
    frame: [usize; 2],
    palms: Vec<PalmCase>,
}

#[derive(Deserialize)]
struct PalmCase {
    score: f64,
    cx: f64,
    cy: f64,
    width: f64,
    height: f64,
    keypoints: [[f64; 2]; 7],
    rect: RectData,
}

#[derive(Deserialize)]
struct Image {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    resize: Vec<ResizeCase>,
    affine: Vec<AffineCase>,
}

#[derive(Deserialize)]
struct ResizeCase {
    to: [usize; 2],
    out: Vec<u8>,
}

#[derive(Deserialize)]
struct AffineCase {
    matrix: [f64; 6],
    size: usize,
    out: Vec<u8>,
}

#[derive(Deserialize)]
struct ModelCase {
    size: usize,
    outputs: Vec<Sampled>,
}

#[derive(Deserialize)]
struct Sampled {
    len: usize,
    sample: Vec<f32>,
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity.json");
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

fn rect_close(rust: &Rect, python: &RectData) -> bool {
    close(rust.cx, python.cx, 1e-9)
        && close(rust.cy, python.cy, 1e-9)
        && close(rust.size, python.size, 1e-9)
        && close(rust.rotation, python.rotation, 1e-12)
}

fn largest_byte_difference(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

#[test]
fn anchors_match() -> TestResult {
    let fixture = fixture()?;
    let anchors = rois::palm_anchors();
    assert_eq!(anchors.len(), fixture.anchors.count);
    for (k, expected) in fixture.anchors.sample.iter().enumerate() {
        let got = anchors[k * 97];
        assert!(close(got[0], expected[0], 1e-12) && close(got[1], expected[1], 1e-12));
    }
    Ok(())
}

#[test]
fn letterboxes_match() -> TestResult {
    for case in fixture()?.geometry.letterboxes {
        let (mapping, w, h) = rois::letterbox(case.size[0], case.size[1], rois::PALM_INPUT)?;
        assert!(
            close(mapping.scale, case.scale, 1e-12),
            "scale for {:?}",
            case.size
        );
        assert_eq!(
            [mapping.pad_x, mapping.pad_y],
            case.pad,
            "padding for {:?}",
            case.size
        );
        assert_eq!([w, h], case.resized, "size for {:?}", case.size);
    }
    Ok(())
}

#[test]
fn rectangles_matrices_and_back_projection_match() -> TestResult {
    for case in fixture()?.geometry.hands {
        let mut points = [[0.0; 3]; rois::LANDMARK_COUNT];
        points.copy_from_slice(&case.points);
        let rect = rois::rect_from_landmarks(&points);
        assert!(rect_close(&rect, &case.rect));
        for (a, b) in rois::crop_matrix(&rect, rois::LANDMARK_INPUT)
            .iter()
            .zip(case.matrix)
        {
            assert!(close(*a, b, 1e-9));
        }
        let mapped = rois::crop_to_image(&rect, &case.crop_points, rois::LANDMARK_INPUT);
        for (got, want) in mapped.iter().zip(&case.mapped) {
            assert!((0..3).all(|k| close(got[k], want[k], 1e-9)));
        }
    }
    Ok(())
}

#[test]
fn palm_decoding_matches() -> TestResult {
    let case = fixture()?.decode;
    let (mapping, _, _): (Letterbox, _, _) =
        rois::letterbox(case.frame[0], case.frame[1], rois::PALM_INPUT)?;
    let palms: Vec<Palm> = rois::decode_palms(&case.raw, &case.logits, &mapping, 0.5, 0.3)?;
    assert_eq!(palms.len(), case.palms.len());
    assert!(!palms.is_empty(), "the fixture should contain palms");
    for (got, want) in palms.iter().zip(&case.palms) {
        assert!(close(got.score, want.score, 1e-9));
        assert!(close(got.cx, want.cx, 1e-6) && close(got.cy, want.cy, 1e-6));
        assert!(close(got.width, want.width, 1e-6) && close(got.height, want.height, 1e-6));
        for (g, w) in got.keypoints.iter().zip(&want.keypoints) {
            assert!(close(g[0], w[0], 1e-6) && close(g[1], w[1], 1e-6));
        }
        assert!(
            rect_close(&rois::rect_from_palm(got), &want.rect) || {
                let r = rois::rect_from_palm(got);
                close(r.cx, want.rect.cx, 1e-6)
                    && close(r.cy, want.rect.cy, 1e-6)
                    && close(r.size, want.rect.size, 1e-6)
            }
        );
    }
    Ok(())
}

#[test]
fn resizing_matches_pillow() -> TestResult {
    let case = fixture()?.image;
    let image = RgbImage::new(case.width, case.height, case.pixels)?;
    for want in case.resize {
        let got = image.resize_bilinear(want.to[0], want.to[1])?;
        let worst = largest_byte_difference(got.as_bytes(), &want.out);
        assert!(worst == 0, "resize to {:?} differs by {worst}", want.to);
    }
    Ok(())
}

#[test]
fn affine_crops_match_pillow() -> TestResult {
    let case = fixture()?.image;
    let image = RgbImage::new(case.width, case.height, case.pixels)?;
    for want in case.affine {
        let got = image.affine_crop(want.matrix, want.size)?;
        let worst = largest_byte_difference(got.as_bytes(), &want.out);
        assert!(worst == 0, "crop differs by {worst}");
    }
    Ok(())
}

#[test]
fn models_match_when_available() -> TestResult {
    let (Some(library), Some(folder)) = (
        std::env::var_os("ORT_DYLIB_PATH"),
        std::env::var_os("AXONHAND_MODELS"),
    ) else {
        eprintln!("skipped: set ORT_DYLIB_PATH and AXONHAND_MODELS to compare the models");
        return Ok(());
    };
    let runtime = shared_runtime(&library)?;
    for (name, want) in fixture()?.models {
        let mut model = Model::open(&runtime, &PathBuf::from(&folder).join(&name))?;
        let size = want.size;
        let input: Vec<f32> = (0..size * size * 3)
            .map(|i| ((i as u64 * 37) % 255) as f32 / 255.0)
            .collect();
        let outputs = model.run(input, [1, size, size, 3])?;
        assert_eq!(
            outputs.len(),
            want.outputs.len(),
            "{name}: number of outputs"
        );
        for (got, expected) in outputs.iter().zip(&want.outputs) {
            assert_eq!(got.len(), expected.len, "{name}: output length");
            for (k, value) in expected.sample.iter().enumerate() {
                let g = got[k * 41];
                assert!(
                    (g - value).abs() <= 1e-3 + 1e-4 * value.abs(),
                    "{name}: value {} is {g}, Python {value}",
                    k * 41
                );
            }
        }
    }
    Ok(())
}

/// The whole tracker, on real models and real frames, against Python's landmarks.
///
/// Needs `ORT_DYLIB_PATH`, `AXONHAND_MODELS` and `AXONHAND_CLIP_DIR` (written by
/// `python -m axonhand.tools.parity_fixture OUT --clip VIDEO --models MODELS`).
#[test]
fn clip_matches_python() -> TestResult {
    let (Some(library), Some(models), Some(clip)) = (
        std::env::var_os("ORT_DYLIB_PATH"),
        std::env::var_os("AXONHAND_MODELS"),
        std::env::var_os("AXONHAND_CLIP_DIR"),
    ) else {
        eprintln!("skipped: set ORT_DYLIB_PATH, AXONHAND_MODELS and AXONHAND_CLIP_DIR");
        return Ok(());
    };
    let runtime = shared_runtime(&library)?;
    let models = PathBuf::from(models);
    let mut tracker = axonhand_hand::HandTracker::new(
        Model::open(
            &runtime,
            &models.join("palm_detection_mediapipe_2023feb.onnx"),
        )?,
        Model::open(
            &runtime,
            &models.join("handpose_estimation_mediapipe_2023feb.onnx"),
        )?,
    );
    let clip = PathBuf::from(clip);
    let expected: Vec<Option<Vec<[f64; 3]>>> =
        serde_json::from_slice(&std::fs::read(clip.join("landmarks.json"))?)?;
    assert!(!expected.is_empty(), "the clip folder has no frames");
    let mut worst = 0.0_f64;
    for (index, want) in expected.iter().enumerate() {
        let frame = RgbImage::from_ppm(&clip.join(format!("frame_{index:04}.ppm")))?;
        let got = tracker.process(&frame)?;
        match (got, want) {
            (None, None) => {}
            (Some(found), Some(points)) => {
                for (g, w) in found.points.iter().zip(points) {
                    worst = worst.max((g[0] - w[0]).abs()).max((g[1] - w[1]).abs());
                }
            }
            (got, want) => {
                return Err(format!(
                    "frame {index}: Rust found a hand: {}, Python: {}",
                    got.is_some(),
                    want.is_some()
                )
                .into())
            }
        }
    }
    eprintln!(
        "largest landmark difference over {} frames: {worst:.6} pixels",
        expected.len()
    );
    assert!(worst < 0.05, "landmarks differ by up to {worst} pixels");
    Ok(())
}

#[test]
fn auto_placement_always_says_why_it_fell_back() -> TestResult {
    let (Some(library), Some(folder)) = (
        std::env::var_os("ORT_DYLIB_PATH"),
        std::env::var_os("AXONHAND_MODELS"),
    ) else {
        eprintln!("skipped: set ORT_DYLIB_PATH and AXONHAND_MODELS");
        return Ok(());
    };
    let runtime = shared_runtime(&library)?;
    let path = PathBuf::from(folder).join("handpose_estimation_mediapipe_2023feb.onnx");
    let model = Model::open_on(&runtime, &path, axonhand_hand::Device::Auto)?;
    // Without the NPU library registered there is no NPU, so this falls back and says so.
    assert_eq!(model.placement(), axonhand_hand::Placement::Cpu);
    assert!(
        model.note().contains("no NPU"),
        "note was {:?}",
        model.note()
    );
    assert!(Model::open_on(&runtime, &path, axonhand_hand::Device::Npu).is_err());
    Ok(())
}
