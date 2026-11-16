//! Find the hand in one frame at a time: detect once, then follow it from frame to frame.
//!
//! A counterpart of `axonhand/hand/tracker.py`.

use crate::error::{HandError, Result};
use crate::image::RgbImage;
use crate::rois::{self, Rect, LANDMARK_COUNT, LANDMARK_INPUT, PALM_INPUT};
use crate::runtime::Network;

const HAND_PRESENT: f64 = 0.5;
const PALM_SCORE: f64 = 0.5;
const PALM_IOU: f64 = 0.3;

/// What was found in one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HandFrame {
    /// x and y in image pixels, depth in pixels relative to the wrist.
    pub points: [[f64; 3]; LANDMARK_COUNT],
    /// The landmark model's score that a hand is present, 0 to 1.
    pub confidence: f64,
    /// True when the palm detector had to find the hand again in this frame.
    pub redetected: bool,
}

/// Follows one hand through a video. Create one per video; it keeps the last position.
pub struct HandTracker<P: Network, L: Network> {
    palm: P,
    landmarks: L,
    rect: Option<Rect>,
    detections: usize,
}

impl<P: Network, L: Network> HandTracker<P, L> {
    /// Use the given open models.
    pub fn new(palm: P, landmarks: L) -> Self {
        Self {
            palm,
            landmarks,
            rect: None,
            detections: 0,
        }
    }

    /// How many times the palm detector has been run, for reporting.
    #[must_use]
    pub fn detections(&self) -> usize {
        self.detections
    }

    /// Forget the hand, for example after a cut in the video.
    pub fn reset(&mut self) {
        self.rect = None;
    }

    fn detect(&mut self, image: &RgbImage) -> Result<Option<Rect>> {
        let (mapping, width, height) = rois::letterbox(image.width(), image.height(), PALM_INPUT)?;
        let mut canvas = RgbImage::black(PALM_INPUT, PALM_INPUT)?;
        let resized = image.resize_bilinear(width, height)?;
        canvas.paste(&resized, mapping.pad_x as usize, mapping.pad_y as usize);
        let outputs = self
            .palm
            .run(canvas.to_blob(), [1, PALM_INPUT, PALM_INPUT, 3])?;
        self.detections += 1;
        let (raw, logits) = match outputs.as_slice() {
            [raw, logits, ..] => (raw, logits),
            _ => {
                return Err(HandError::Output(
                    "the palm detector gave fewer than two outputs".into(),
                ))
            }
        };
        let palms = rois::decode_palms(raw, logits, &mapping, PALM_SCORE, PALM_IOU)?;
        Ok(palms.first().map(rois::rect_from_palm))
    }

    fn landmark(
        &mut self,
        image: &RgbImage,
        rect: &Rect,
    ) -> Result<Option<([[f64; 3]; LANDMARK_COUNT], f64)>> {
        let crop = image.affine_crop(rois::crop_matrix(rect, LANDMARK_INPUT), LANDMARK_INPUT)?;
        let outputs = self
            .landmarks
            .run(crop.to_blob(), [1, LANDMARK_INPUT, LANDMARK_INPUT, 3])?;
        let (local, score) = match outputs.as_slice() {
            [local, score, ..] => (local, score.first().copied()),
            _ => {
                return Err(HandError::Output(
                    "the landmark model gave fewer than two outputs".into(),
                ))
            }
        };
        let score = f64::from(
            score.ok_or_else(|| HandError::Output("the landmark score is empty".into()))?,
        );
        if score < HAND_PRESENT {
            return Ok(None);
        }
        if local.len() != LANDMARK_COUNT * 3 {
            return Err(HandError::Output(
                "the landmark model gave the wrong number of values".into(),
            ));
        }
        let crop_points: Vec<[f64; 3]> = local
            .chunks_exact(3)
            .map(|c| [f64::from(c[0]), f64::from(c[1]), f64::from(c[2])])
            .collect();
        let mapped = rois::crop_to_image(rect, &crop_points, LANDMARK_INPUT);
        let mut points = [[0.0; 3]; LANDMARK_COUNT];
        points.copy_from_slice(&mapped);
        Ok(Some((points, score)))
    }

    /// Find the hand in a frame, or return `None` when there is none.
    ///
    /// # Errors
    ///
    /// Fails if a model cannot run or returns something unexpected.
    pub fn process(&mut self, frame: &RgbImage) -> Result<Option<HandFrame>> {
        let mut redetected = false;
        let mut found = None;
        if let Some(rect) = self.rect {
            found = self.landmark(frame, &rect)?;
        }
        if found.is_none() {
            // Lost, or never found: look for the palm again, once, in this same frame.
            self.rect = self.detect(frame)?;
            redetected = true;
            if let Some(rect) = self.rect {
                found = self.landmark(frame, &rect)?;
            }
        }
        match found {
            None => {
                self.rect = None;
                Ok(None)
            }
            Some((points, confidence)) => {
                self.rect = Some(rois::rect_from_landmarks(&points));
                Ok(Some(HandFrame {
                    points,
                    confidence,
                    redetected,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rois::PALM_ANCHORS;

    /// A palm detector that always sees one palm in the middle of the picture.
    struct SeesPalm;

    impl Network for SeesPalm {
        fn run(&mut self, _blob: Vec<f32>, _shape: [usize; 4]) -> Result<Vec<Vec<f32>>> {
            let mut raw = vec![0.0; PALM_ANCHORS * 18];
            raw[200 * 18 + 2] = 40.0;
            raw[200 * 18 + 3] = 40.0;
            let mut logits = vec![-20.0; PALM_ANCHORS];
            logits[200] = 5.0;
            Ok(vec![raw, logits])
        }
    }

    /// A landmark model that reports a hand for as many frames as it is told to, then none.
    struct HandFor(usize);

    impl Network for HandFor {
        fn run(&mut self, _blob: Vec<f32>, _shape: [usize; 4]) -> Result<Vec<Vec<f32>>> {
            let present = self.0 > 0;
            self.0 = self.0.saturating_sub(1);
            let points: Vec<f32> = (0..LANDMARK_COUNT * 3)
                .map(|i| 60.0 + (i % 7) as f32 * 20.0)
                .collect();
            Ok(vec![points, vec![if present { 0.9 } else { 0.1 }]])
        }
    }

    struct Broken;

    impl Network for Broken {
        fn run(&mut self, _blob: Vec<f32>, _shape: [usize; 4]) -> Result<Vec<Vec<f32>>> {
            Ok(vec![vec![0.0]])
        }
    }

    fn frame() -> RgbImage {
        RgbImage::black(320, 240).unwrap_or_else(|_| unreachable!("a 320 x 240 picture is valid"))
    }

    #[test]
    fn follows_the_hand_without_detecting_again() -> Result<()> {
        let mut tracker = HandTracker::new(SeesPalm, HandFor(5));
        let first = tracker.process(&frame())?;
        let second = tracker.process(&frame())?;
        assert!(first.is_some_and(|f| f.redetected));
        assert!(second.is_some_and(|f| !f.redetected));
        assert_eq!(tracker.detections(), 1);
        Ok(())
    }

    #[test]
    fn a_lost_hand_is_looked_for_once_and_reported_missing() -> Result<()> {
        let mut tracker = HandTracker::new(SeesPalm, HandFor(1));
        assert!(tracker.process(&frame())?.is_some());
        // The tracked box fails (hand gone), the detector finds a palm, the landmarks fail again.
        assert!(tracker.process(&frame())?.is_none());
        assert_eq!(tracker.detections(), 2);
        Ok(())
    }

    #[test]
    fn reset_forgets_the_hand() -> Result<()> {
        let mut tracker = HandTracker::new(SeesPalm, HandFor(9));
        tracker.process(&frame())?;
        tracker.reset();
        let again = tracker.process(&frame())?;
        assert!(again.is_some_and(|f| f.redetected));
        Ok(())
    }

    #[test]
    fn a_model_with_too_few_outputs_is_an_error() {
        let mut tracker = HandTracker::new(Broken, HandFor(1));
        assert!(tracker.process(&frame()).is_err());
    }
}
