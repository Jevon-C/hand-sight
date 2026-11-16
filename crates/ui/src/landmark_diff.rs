//! How far apart two runs of the hand model put the landmarks of the same video.
//!
//! The error of each landmark is its distance between the two runs, as a fraction of the palm
//! length (wrist to the base of the middle finger) in the first run, so it does not depend on how
//! large the hand is in the picture. This is the same measure as
//! `python -m axonhand.tools.compare_landmarks`, which is how the CPU and the NPU were compared.
//! The saved landmarks are rounded to a tenth of a pixel, so differences below about 0.001 palm
//! lengths are rounding, not disagreement.

use axonhand_api::LandmarkOverlay;

/// The median error the proposed limits allow, in palm lengths (docs/METRICS.md).
pub const MAX_MEDIAN: f64 = 0.02;
/// The 95th-percentile error the proposed limits allow, in palm lengths.
pub const MAX_P95: f64 = 0.05;

const WRIST: usize = 0;
const MIDDLE_MCP: usize = 9;
const LANDMARKS: usize = 21;

/// The difference between two runs over the frames where both found a hand.
#[derive(Debug, Clone, PartialEq)]
pub struct LandmarkDifference {
    /// Frames in the video.
    pub frames: usize,
    /// Frames where both runs found a hand.
    pub both_found: usize,
    /// Frames where one run found a hand and the other did not.
    pub disagree: usize,
    /// Median error over every landmark of every frame both found, in palm lengths.
    pub median: f64,
    /// 95th-percentile error, in palm lengths.
    pub p95: f64,
    /// The largest error, in palm lengths.
    pub largest: f64,
    /// The frame (counting from 0) and landmark (0 is the wrist) of the largest error.
    pub worst: (usize, usize),
    /// In how many frames some landmark is further off than [`MAX_P95`].
    pub frames_over_limit: usize,
}

impl LandmarkDifference {
    /// Whether the two runs agree within the proposed limits.
    #[must_use]
    pub fn within_limits(&self) -> bool {
        self.median <= MAX_MEDIAN && self.p95 <= MAX_P95
    }
}

fn hand(frame: &Option<Vec<Option<[f64; 2]>>>) -> Option<&[Option<[f64; 2]>]> {
    frame.as_deref().filter(|points| points.len() == LANDMARKS)
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The value below which `fraction` (0 to 1) of the sorted values lie, interpolating between
/// neighbours the way NumPy does.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let position = fraction * (sorted.len().saturating_sub(1)) as f64;
    let (below, above) = (position.floor() as usize, position.ceil() as usize);
    let at = |i: usize| sorted.get(i).copied().unwrap_or(0.0);
    at(below) + (at(above) - at(below)) * (position - position.floor())
}

/// Compare two runs of the same video.
///
/// # Errors
///
/// A sentence for the person when the two cover different numbers of frames or no frame has a
/// hand in both.
pub fn landmark_difference(
    first: &LandmarkOverlay,
    second: &LandmarkOverlay,
) -> Result<LandmarkDifference, String> {
    if first.points.len() != second.points.len() {
        return Err(
            "The two results cover different numbers of frames, so their landmarks \
                    cannot be compared."
                .to_owned(),
        );
    }
    let mut errors: Vec<(f64, usize, usize)> = Vec::new();
    let (mut both_found, mut disagree, mut over_limit) = (0, 0, 0);
    for (index, (a, b)) in first.points.iter().zip(&second.points).enumerate() {
        let (a, b) = (hand(a), hand(b));
        if a.is_some() != b.is_some() {
            disagree += 1;
        }
        let (Some(a), Some(b)) = (a, b) else { continue };
        let (Some(wrist), Some(middle)) = (a[WRIST], a[MIDDLE_MCP]) else {
            continue;
        };
        both_found += 1;
        let palm = distance(wrist, middle).max(1e-6);
        let mut worst_here = 0.0_f64;
        for (landmark, (p, q)) in a.iter().zip(b).enumerate() {
            if let (Some(p), Some(q)) = (p, q) {
                let error = distance(*p, *q) / palm;
                worst_here = worst_here.max(error);
                errors.push((error, index, landmark));
            }
        }
        if worst_here > MAX_P95 {
            over_limit += 1;
        }
    }
    let Some(&(largest, frame, landmark)) = errors.iter().max_by(|x, y| x.0.total_cmp(&y.0)) else {
        return Err(
            "No frame has a hand in both results, so their landmarks cannot be \
                    compared."
                .to_owned(),
        );
    };
    let mut sorted: Vec<f64> = errors.iter().map(|e| e.0).collect();
    sorted.sort_by(f64::total_cmp);
    Ok(LandmarkDifference {
        frames: first.points.len(),
        both_found,
        disagree,
        median: percentile(&sorted, 0.5),
        p95: percentile(&sorted, 0.95),
        largest,
        worst: (frame, landmark),
        frames_over_limit: over_limit,
    })
}

/// The comparison in plain words, for the Compare screen.
#[must_use]
pub fn describe(d: &LandmarkDifference) -> Vec<String> {
    let mut lines = vec![format!(
        "Both found the hand in {} of {} frames{}.",
        d.both_found,
        d.frames,
        if d.disagree == 0 {
            String::new()
        } else {
            format!("; in {} more frames only one of them did", d.disagree)
        }
    )];
    lines.push(format!(
        "Landmark positions differ by a median of {:.4} and at the 95th percentile {:.4} palm \
         lengths; the largest difference is {:.2} (landmark {} in frame {} of {}).",
        d.median,
        d.p95,
        d.largest,
        d.worst.1,
        d.worst.0 + 1,
        d.frames
    ));
    lines.push(format!(
        "{} of {} frames have some landmark off by more than {MAX_P95} palm lengths.",
        d.frames_over_limit, d.both_found
    ));
    lines.push(if d.within_limits() {
        format!(
            "This is within the proposed limits (median up to {MAX_MEDIAN}, 95th percentile up \
             to {MAX_P95})."
        )
    } else {
        format!(
            "This is outside the proposed limits (median up to {MAX_MEDIAN}, 95th percentile up \
             to {MAX_P95})."
        )
    });
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand whose wrist is at `(shift, 0)` and whose middle-finger base is 100 pixels below it
    /// (so a palm length is 100 pixels); the other landmarks lie along the top.
    fn hand_at(shift: f64) -> Option<Vec<Option<[f64; 2]>>> {
        Some(
            (0..LANDMARKS)
                .map(|i| {
                    Some(match i {
                        WRIST => [shift, 0.0],
                        MIDDLE_MCP => [shift, 100.0],
                        _ => [shift + 10.0 * i as f64, 0.0],
                    })
                })
                .collect(),
        )
    }

    fn overlay(frames: Vec<Option<Vec<Option<[f64; 2]>>>>) -> LandmarkOverlay {
        LandmarkOverlay {
            fps: 30.0,
            points: frames,
        }
    }

    #[test]
    fn identical_runs_agree_exactly() -> Result<(), String> {
        let a = overlay(vec![hand_at(0.0), hand_at(5.0)]);
        let d = landmark_difference(&a, &a.clone())?;
        assert_eq!((d.median, d.p95, d.largest), (0.0, 0.0, 0.0));
        assert_eq!((d.both_found, d.disagree, d.frames_over_limit), (2, 0, 0));
        assert!(d.within_limits());
        Ok(())
    }

    #[test]
    fn one_landmark_moved_half_a_palm_is_the_largest_and_is_located() -> Result<(), String> {
        let a = overlay(vec![hand_at(0.0), hand_at(0.0)]);
        let mut moved = hand_at(0.0).ok_or("no hand")?;
        moved[20] = moved[20].map(|[x, y]| [x + 50.0, y]);
        let b = overlay(vec![hand_at(0.0), Some(moved)]);
        let d = landmark_difference(&a, &b)?;
        assert!((d.largest - 0.5).abs() < 1e-9, "{}", d.largest);
        assert_eq!(d.worst, (1, 20));
        assert_eq!(d.frames_over_limit, 1);
        // One landmark of 42 is far off; the median is untouched.
        assert_eq!(d.median, 0.0);
        Ok(())
    }

    #[test]
    fn a_frame_one_run_lacks_counts_as_disagreement_and_is_not_compared() -> Result<(), String> {
        let a = overlay(vec![hand_at(0.0), hand_at(0.0), None]);
        let b = overlay(vec![hand_at(0.0), None, None]);
        let d = landmark_difference(&a, &b)?;
        assert_eq!((d.frames, d.both_found, d.disagree), (3, 1, 1));
        Ok(())
    }

    #[test]
    fn different_lengths_or_no_shared_hand_are_explained() {
        let one = overlay(vec![hand_at(0.0)]);
        let two = overlay(vec![hand_at(0.0), hand_at(0.0)]);
        assert!(landmark_difference(&one, &two)
            .is_err_and(|m| m.contains("different numbers of frames")));
        let none = overlay(vec![None]);
        assert!(landmark_difference(&one, &none).is_err_and(|m| m.contains("No frame")));
    }

    #[test]
    fn percentiles_interpolate_like_numpy() {
        let values = [0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&values, 0.5), 2.0);
        assert!((percentile(&values, 0.95) - 3.8).abs() < 1e-12);
        assert_eq!(percentile(&[7.0], 0.95), 7.0);
    }

    #[test]
    fn the_words_say_whether_the_limits_are_met() -> Result<(), String> {
        let a = overlay(vec![hand_at(0.0)]);
        let same = describe(&landmark_difference(&a, &a.clone())?);
        assert!(same
            .last()
            .is_some_and(|l| l.contains("within the proposed limits")));
        let mut moved = hand_at(0.0).ok_or("no hand")?;
        for point in moved.iter_mut().flatten() {
            point.iter_mut().for_each(|c| *c += 10.0);
        }
        // A whole-hand shift of 10 px is 0.14 palm lengths in x and y together: outside.
        let far = describe(&landmark_difference(&a, &overlay(vec![Some(moved)]))?);
        assert!(far
            .last()
            .is_some_and(|l| l.contains("outside the proposed limits")));
        Ok(())
    }
}
