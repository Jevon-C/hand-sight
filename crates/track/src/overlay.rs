//! Writes the landmarks for drawing over the video, in the form `axonhand_api::LandmarkOverlay`
//! reads: `{"fps": ..., "points": [...]}` with, for each frame, `null` when no hand was found or
//! 21 entries of `[x, y]` in pixels of the upright video, rounded to a tenth of a pixel.

use std::fmt::Write as _;

use crate::track::Points;

/// The overlay as compact JSON.
#[must_use]
pub fn overlay_json(fps: f64, frames: &[Option<Points>]) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail, so the results below are ignored.
    let _ = write!(out, "{{\"fps\":{fps:?},\"points\":[");
    for (index, frame) in frames.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        match frame {
            None => out.push_str("null"),
            Some(points) => {
                out.push('[');
                for (i, point) in points.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if point[0].is_finite() && point[1].is_finite() {
                        let _ = write!(out, "[{:.1},{:.1}]", point[0], point[1]);
                    } else {
                        out.push_str("null");
                    }
                }
                out.push(']');
            }
        }
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use axonhand_hand::rois::LANDMARK_COUNT;

    #[test]
    fn a_missing_hand_is_null_and_a_found_one_is_rounded_pairs() {
        let mut hand = [[1.25, 2.0, 9.0]; LANDMARK_COUNT];
        hand[1] = [f64::NAN, 3.0, 0.0];
        let text = overlay_json(30.0, &[None, Some(hand)]);
        assert!(text.starts_with("{\"fps\":30.0,\"points\":[null,[[1.2,2.0],null,[1.2,2.0]"));
        assert!(text.ends_with("]]}"));
        let landmarks = text.matches("[1.2,2.0]").count();
        assert_eq!(landmarks, LANDMARK_COUNT - 1);
    }

    #[test]
    fn an_empty_track_is_valid_json() {
        assert_eq!(overlay_json(25.0, &[]), "{\"fps\":25.0,\"points\":[]}");
    }
}
