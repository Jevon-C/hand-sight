//! Geometry of the two-stage hand pipeline (the MediaPipe design), with no model in it.
//!
//! This is a line-for-line counterpart of `axonhand/hand/rois.py`; `tests/parity.rs` checks the
//! two against numbers produced by the Python code. Everything is in continuous image
//! coordinates: the top-left corner of the top-left pixel is (0, 0), x runs right, y runs down.
//! Angles are in radians; a rectangle's rotation turns an upright hand (wrist at the bottom,
//! fingers up) into the hand in the image, positive clockwise on screen.

use std::f64::consts::PI;

use crate::error::{HandError, Result};

/// Side of the palm detector's square input, in pixels.
pub const PALM_INPUT: usize = 192;
/// Side of the landmark model's square input, in pixels.
pub const LANDMARK_INPUT: usize = 224;
/// Number of anchors of the palm detector.
pub const PALM_ANCHORS: usize = 2016;
/// Number of landmarks of one hand.
pub const LANDMARK_COUNT: usize = 21;

const PALM_WRIST: usize = 0;
const PALM_MIDDLE: usize = 2;
const WRIST: usize = 0;
const MIDDLE_MCP: usize = 9;
const PALM_SCALE: f64 = 2.6;
const PALM_SHIFT_Y: f64 = -0.5;
const TRACK_SCALE: f64 = 2.0;
const TRACK_SHIFT_Y: f64 = -0.1;
const MAX_LOGIT: f64 = 100.0;

/// A square, possibly rotated, region of the image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Centre, x.
    pub cx: f64,
    /// Centre, y.
    pub cy: f64,
    /// Side length.
    pub size: f64,
    /// Rotation in radians.
    pub rotation: f64,
}

/// One detected palm, in image coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Palm {
    /// Detector confidence, 0 to 1.
    pub score: f64,
    /// Box centre, x.
    pub cx: f64,
    /// Box centre, y.
    pub cy: f64,
    /// Box width.
    pub width: f64,
    /// Box height.
    pub height: f64,
    /// Wrist centre, index, middle, ring and little finger base, thumb base and joint.
    pub keypoints: [[f64; 2]; 7],
}

/// How an image was fitted into the square detector input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Letterbox {
    /// Factor the image was scaled by.
    pub scale: f64,
    /// Padding to the left of the picture, in detector pixels.
    pub pad_x: f64,
    /// Padding above the picture, in detector pixels.
    pub pad_y: f64,
}

impl Letterbox {
    /// Map a detector-input position (0..192) back to the image.
    #[must_use]
    pub fn to_image(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.pad_x) / self.scale, (y - self.pad_y) / self.scale)
    }
}

/// Fit a `width` x `height` image inside `size` x `size` keeping its shape.
///
/// Returns the mapping plus the resized width and height. The picture is centred to a whole pixel.
///
/// # Errors
///
/// Fails if a side is zero.
pub fn letterbox(width: usize, height: usize, size: usize) -> Result<(Letterbox, usize, usize)> {
    if width == 0 || height == 0 {
        return Err(HandError::Image("image size must be positive".into()));
    }
    let scale = size as f64 / width.max(height) as f64;
    let new_w = ((width as f64 * scale).round_ties_even() as usize).max(1);
    let new_h = ((height as f64 * scale).round_ties_even() as usize).max(1);
    let mapping = Letterbox {
        scale,
        pad_x: (size.saturating_sub(new_w) / 2) as f64,
        pad_y: (size.saturating_sub(new_h) / 2) as f64,
    };
    Ok((mapping, new_w, new_h))
}

/// The 2016 anchor centres of the palm detector, as fractions of its input (0..1).
///
/// Four layers at strides 8, 16, 16 and 16. Layers with the same stride share a grid, and each
/// layer puts two anchors in every cell: 24x24x2 + 12x12x6 = 2016.
#[must_use]
pub fn palm_anchors() -> Vec<[f64; 2]> {
    const STRIDES: [usize; 4] = [8, 16, 16, 16];
    let mut anchors = Vec::with_capacity(PALM_ANCHORS);
    let mut layer = 0;
    while layer < STRIDES.len() {
        let mut last = layer;
        while last < STRIDES.len() && STRIDES[last] == STRIDES[layer] {
            last += 1;
        }
        let cells = PALM_INPUT.div_ceil(STRIDES[layer]);
        let per_cell = 2 * (last - layer);
        for y in 0..cells {
            for x in 0..cells {
                let centre = [
                    (x as f64 + 0.5) / cells as f64,
                    (y as f64 + 0.5) / cells as f64,
                ];
                anchors.extend(std::iter::repeat_n(centre, per_cell));
            }
        }
        layer = last;
    }
    anchors
}

/// Intersection over union of two boxes `[x1, y1, x2, y2]`.
fn iou(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let inter =
        (a[2].min(b[2]) - a[0].max(b[0])).max(0.0) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter;
    if union > 0.0 {
        inter / union
    } else {
        0.0
    }
}

/// Turn the detector's raw output into palms, best first, with overlaps removed.
///
/// `raw` is `2016 x 18` numbers (box centre offset, box size, seven keypoint offsets, in detector
/// input pixels relative to each anchor); `logits` is 2016 numbers.
///
/// # Errors
///
/// Fails if the outputs have the wrong length.
pub fn decode_palms(
    raw: &[f32],
    logits: &[f32],
    mapping: &Letterbox,
    score_threshold: f64,
    iou_threshold: f64,
) -> Result<Vec<Palm>> {
    if raw.len() != PALM_ANCHORS * 18 || logits.len() != PALM_ANCHORS {
        return Err(HandError::Output(
            "the palm detector's output has the wrong size".into(),
        ));
    }
    let anchors = palm_anchors();
    let input = PALM_INPUT as f64;
    // Candidates above the threshold: index, score, box and the 18 raw numbers.
    struct Candidate {
        score: f64,
        bounds: [f64; 4],
        centre: [f64; 2],
        size: [f64; 2],
        keypoints: [[f64; 2]; 7],
    }
    let mut candidates = Vec::new();
    for (i, (anchor, logit)) in anchors.iter().zip(logits).enumerate() {
        let score = 1.0 / (1.0 + (-f64::from(*logit).clamp(-MAX_LOGIT, MAX_LOGIT)).exp());
        if score < score_threshold {
            continue;
        }
        let row: Vec<f64> = raw[i * 18..(i + 1) * 18]
            .iter()
            .map(|&v| f64::from(v))
            .collect();
        let (ax, ay) = (anchor[0] * input, anchor[1] * input);
        let (cx, cy) = (row[0] + ax, row[1] + ay);
        let (w, h) = (row[2], row[3]);
        let mut keypoints = [[0.0; 2]; 7];
        for (k, point) in keypoints.iter_mut().enumerate() {
            let (ix, iy) = mapping.to_image(row[4 + 2 * k] + ax, row[5 + 2 * k] + ay);
            *point = [ix, iy];
        }
        let (bx, by) = mapping.to_image(cx, cy);
        candidates.push(Candidate {
            score,
            bounds: [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0],
            centre: [bx, by],
            size: [w / mapping.scale, h / mapping.scale],
            keypoints,
        });
    }
    // Best first; the sort is stable so equal scores keep anchor order.
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut alive = vec![true; candidates.len()];
    let mut palms = Vec::new();
    for rank in 0..candidates.len() {
        if !alive[rank] {
            continue;
        }
        let best = &candidates[rank];
        for other in rank + 1..candidates.len() {
            if alive[other] && iou(&best.bounds, &candidates[other].bounds) > iou_threshold {
                alive[other] = false;
            }
        }
        palms.push(Palm {
            score: best.score,
            cx: best.centre[0],
            cy: best.centre[1],
            width: best.size[0],
            height: best.size[1],
            keypoints: best.keypoints,
        });
    }
    Ok(palms)
}

/// Rotation that makes the wrist-to-middle-finger direction point up, in (-pi, pi].
fn hand_rotation(wrist: [f64; 2], middle: [f64; 2]) -> f64 {
    let rotation = PI / 2.0 - (-(middle[1] - wrist[1])).atan2(middle[0] - wrist[0]);
    rotation - 2.0 * PI * ((rotation + PI) / (2.0 * PI)).floor()
}

/// Move a centre along the hand's own up direction by `shift_y` times the height.
fn shifted(cx: f64, cy: f64, height: f64, rotation: f64, shift_y: f64) -> (f64, f64) {
    (
        cx - height * shift_y * rotation.sin(),
        cy + height * shift_y * rotation.cos(),
    )
}

/// The square region to give the landmark model, from a detected palm.
#[must_use]
pub fn rect_from_palm(palm: &Palm) -> Rect {
    let rotation = hand_rotation(palm.keypoints[PALM_WRIST], palm.keypoints[PALM_MIDDLE]);
    let (cx, cy) = shifted(palm.cx, palm.cy, palm.height, rotation, PALM_SHIFT_Y);
    Rect {
        cx,
        cy,
        size: palm.width.max(palm.height) * PALM_SCALE,
        rotation,
    }
}

/// The region for the next frame, from this frame's landmarks (x and y in image pixels).
///
/// The box is measured in the hand's own turned frame, so it does not grow when the hand tilts.
#[must_use]
pub fn rect_from_landmarks(points: &[[f64; 3]; LANDMARK_COUNT]) -> Rect {
    let rotation = hand_rotation(
        [points[WRIST][0], points[WRIST][1]],
        [points[MIDDLE_MCP][0], points[MIDDLE_MCP][1]],
    );
    let (cos, sin) = (rotation.cos(), rotation.sin());
    let (mut lo_x, mut hi_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut lo_y, mut hi_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in points {
        let local_x = p[0] * cos + p[1] * sin;
        let local_y = -p[0] * sin + p[1] * cos;
        lo_x = lo_x.min(local_x);
        hi_x = hi_x.max(local_x);
        lo_y = lo_y.min(local_y);
        hi_y = hi_y.max(local_y);
    }
    let (mid_x, mid_y) = ((lo_x + hi_x) / 2.0, (lo_y + hi_y) / 2.0);
    let (width, height) = (hi_x - lo_x, hi_y - lo_y);
    let cx = mid_x * cos - mid_y * sin;
    let cy = mid_x * sin + mid_y * cos;
    let (cx, cy) = shifted(cx, cy, height, rotation, TRACK_SHIFT_Y);
    Rect {
        cx,
        cy,
        size: width.max(height) * TRACK_SCALE,
        rotation,
    }
}

/// Affine coefficients `(a, b, c, d, e, f)` taking a crop position (u, v) to an image position:
/// `x = a*u + b*v + c`, `y = d*u + e*v + f`, with (u, v) continuous in 0..`size`.
#[must_use]
pub fn crop_matrix(rect: &Rect, size: usize) -> [f64; 6] {
    let scale = rect.size / size as f64;
    let (cos, sin) = (rect.rotation.cos(), rect.rotation.sin());
    let half = size as f64 / 2.0;
    let (a, b) = (scale * cos, -scale * sin);
    let (d, e) = (scale * sin, scale * cos);
    let c = rect.cx - (a * half + b * half);
    let f = rect.cy - (d * half + e * half);
    [a, b, c, d, e, f]
}

/// Map landmark positions in crop pixels back to the image.
///
/// Each point is x and y in crop pixels and a depth in the same units. x and y are placed in the
/// image; depth is only rescaled, because it is relative to the wrist.
#[must_use]
pub fn crop_to_image(rect: &Rect, points: &[[f64; 3]], size: usize) -> Vec<[f64; 3]> {
    let [a, b, c, d, e, f] = crop_matrix(rect, size);
    let depth_scale = rect.size / size as f64;
    points
        .iter()
        .map(|p| {
            [
                a * p[0] + b * p[1] + c,
                d * p[0] + e * p[1] + f,
                p[2] * depth_scale,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_2016_anchors_inside_the_unit_square() {
        let anchors = palm_anchors();
        assert_eq!(anchors.len(), PALM_ANCHORS);
        assert!(anchors
            .iter()
            .all(|a| a.iter().all(|v| *v > 0.0 && *v < 1.0)));
    }

    #[test]
    fn letterbox_centres_a_wide_picture() -> Result<()> {
        let (mapping, w, h) = letterbox(1920, 1080, 192)?;
        assert_eq!((w, h), (192, 108));
        assert_eq!((mapping.pad_x, mapping.pad_y), (0.0, 42.0));
        assert_eq!(mapping.to_image(96.0, 96.0), (960.0, 540.0));
        Ok(())
    }

    #[test]
    fn letterbox_rejects_an_empty_picture() {
        assert!(letterbox(0, 10, 192).is_err());
    }

    #[test]
    fn upright_hand_has_zero_rotation() {
        assert!(hand_rotation([100.0, 200.0], [100.0, 100.0]).abs() < 1e-12);
    }

    #[test]
    fn hand_pointing_right_turns_a_quarter_clockwise() {
        let turn = hand_rotation([0.0, 0.0], [10.0, 0.0]);
        assert!((turn - PI / 2.0).abs() < 1e-12);
    }

    #[test]
    fn crop_matrix_maps_the_crop_centre_to_the_rect_centre() {
        let rect = Rect {
            cx: 300.0,
            cy: 200.0,
            size: 448.0,
            rotation: 0.7,
        };
        let [a, b, c, d, e, f] = crop_matrix(&rect, 224);
        assert!((a * 112.0 + b * 112.0 + c - 300.0).abs() < 1e-9);
        assert!((d * 112.0 + e * 112.0 + f - 200.0).abs() < 1e-9);
    }

    #[test]
    fn crop_to_image_inverts_the_crop_matrix_for_a_plain_square() {
        let rect = Rect {
            cx: 50.0,
            cy: 60.0,
            size: 224.0,
            rotation: 0.0,
        };
        let mapped = crop_to_image(&rect, &[[0.0, 0.0, 4.0], [224.0, 224.0, 0.0]], 224);
        assert_eq!(mapped[0], [-62.0, -52.0, 4.0]);
        assert_eq!(mapped[1], [162.0, 172.0, 0.0]);
    }

    #[test]
    fn decoding_a_wrong_sized_output_is_an_error() {
        let mapping = Letterbox {
            scale: 1.0,
            pad_x: 0.0,
            pad_y: 0.0,
        };
        assert!(decode_palms(&[0.0; 10], &[0.0; 10], &mapping, 0.5, 0.3).is_err());
    }

    #[test]
    fn decoding_keeps_the_best_of_two_overlapping_palms() -> Result<()> {
        let mapping = Letterbox {
            scale: 1.0,
            pad_x: 0.0,
            pad_y: 0.0,
        };
        let mut raw = vec![0.0_f32; PALM_ANCHORS * 18];
        let mut logits = vec![-20.0_f32; PALM_ANCHORS];
        for (i, logit) in [(100, 3.0_f32), (101, 2.0_f32)] {
            raw[i * 18 + 2] = 40.0;
            raw[i * 18 + 3] = 40.0;
            logits[i] = logit;
        }
        let palms = decode_palms(&raw, &logits, &mapping, 0.5, 0.3)?;
        assert_eq!(palms.len(), 1);
        assert!(palms[0].score > 0.95);
        Ok(())
    }
}
