//! Follow the hand through a whole stream of frames.

use std::io::Read;

use axonhand_hand::rois::LANDMARK_COUNT;
use axonhand_hand::{HandError, HandTracker, Network};

use crate::stream::{FrameStream, StreamError};

/// The 21 landmarks of one frame: x and y in pixels, then depth.
pub type Points = [[f64; 3]; LANDMARK_COUNT];

/// The hand's position in every frame of a video.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Frames per second of the video.
    pub fps: f64,
    /// One entry for each frame; `None` where no hand was found.
    pub frames: Vec<Option<Points>>,
}

impl Track {
    /// In how many frames a hand was found.
    #[must_use]
    pub fn found(&self) -> usize {
        self.frames.iter().filter(|f| f.is_some()).count()
    }
}

/// Why a video could not be tracked.
#[derive(Debug, thiserror::Error)]
pub enum TrackError {
    /// The video has no frames. The message is for the person.
    #[error("The video has no frames.")]
    NoFrames,
    /// No frame shows a hand. The message is for the person.
    #[error("No hand was found in this video. Check that the hand is in view.")]
    NoHand,
    /// The NPU was asked for and cannot be used. The message is for the person.
    #[error("The NPU could not be used on this computer. Run the analysis on the CPU instead.")]
    NpuUnavailable,
    /// The frame stream is damaged.
    #[error(transparent)]
    Stream(#[from] StreamError),
    /// A model could not run.
    #[error("hand model failed: {0}")]
    Model(#[from] HandError),
}

impl TrackError {
    /// True when the video itself is the problem and the message can be shown to a clinician.
    #[must_use]
    pub fn is_for_the_person(&self) -> bool {
        matches!(self, Self::NoFrames | Self::NoHand | Self::NpuUnavailable)
    }
}

/// Find the hand in every frame of `stream`.
///
/// # Errors
///
/// [`TrackError::NoFrames`] or [`TrackError::NoHand`] when the video is unusable, otherwise the
/// error of the stream or the models.
pub fn track_stream<R: Read, P: Network, L: Network>(
    stream: &mut FrameStream<R>,
    tracker: &mut HandTracker<P, L>,
) -> Result<Track, TrackError> {
    let fps = stream.header().fps;
    let mut frames = Vec::new();
    while let Some(frame) = stream.next_frame()? {
        frames.push(tracker.process(&frame)?.map(|found| found.points));
    }
    if frames.is_empty() {
        return Err(TrackError::NoFrames);
    }
    let track = Track { fps, frames };
    if track.found() == 0 {
        return Err(TrackError::NoHand);
    }
    Ok(track)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axonhand_hand::rois::{LANDMARK_INPUT, PALM_ANCHORS};
    use axonhand_hand::Result as HandResult;

    struct SeesPalm;

    impl Network for SeesPalm {
        fn run(&mut self, _blob: Vec<f32>, _shape: [usize; 4]) -> HandResult<Vec<Vec<f32>>> {
            let mut raw = vec![0.0; PALM_ANCHORS * 18];
            raw[200 * 18 + 2] = 40.0;
            raw[200 * 18 + 3] = 40.0;
            let mut logits = vec![-20.0; PALM_ANCHORS];
            logits[200] = 5.0;
            Ok(vec![raw, logits])
        }
    }

    /// Reports a hand for the first `n` calls, then none.
    struct HandFor(usize);

    impl Network for HandFor {
        fn run(&mut self, _blob: Vec<f32>, _shape: [usize; 4]) -> HandResult<Vec<Vec<f32>>> {
            let present = self.0 > 0;
            self.0 = self.0.saturating_sub(1);
            let points: Vec<f32> = (0..LANDMARK_COUNT * 3)
                .map(|i| (LANDMARK_INPUT as f32) / 4.0 + (i % 7) as f32 * 20.0)
                .collect();
            Ok(vec![points, vec![if present { 0.9 } else { 0.1 }]])
        }
    }

    fn stream(frames: usize) -> Result<FrameStream<std::io::Cursor<Vec<u8>>>, StreamError> {
        let mut data = b"AXONFRAMES 1 64 48 30.0\n".to_vec();
        data.resize(data.len() + frames * 64 * 48 * 3, 0);
        FrameStream::open(std::io::Cursor::new(data))
    }

    #[test]
    fn every_frame_gets_an_entry_and_missing_hands_are_none() -> Result<(), TrackError> {
        let mut tracker = HandTracker::new(SeesPalm, HandFor(2));
        let track = track_stream(&mut stream(4)?, &mut tracker)?;
        assert_eq!(track.frames.len(), 4);
        assert_eq!(track.fps, 30.0);
        assert!(track.frames[0].is_some() && track.frames[1].is_some());
        assert!(track.frames[3].is_none());
        assert_eq!(track.found(), 2);
        Ok(())
    }

    #[test]
    fn a_video_without_a_hand_is_unusable_and_says_so_plainly() -> Result<(), StreamError> {
        let mut tracker = HandTracker::new(SeesPalm, HandFor(0));
        let error = track_stream(&mut stream(3)?, &mut tracker).err();
        assert!(matches!(error, Some(TrackError::NoHand)));
        assert!(error.is_some_and(|e| e.is_for_the_person()
            && e.to_string()
                == "No hand was found in this video. Check that the hand is in view."));
        Ok(())
    }

    #[test]
    fn a_video_without_frames_is_unusable() -> Result<(), StreamError> {
        let mut tracker = HandTracker::new(SeesPalm, HandFor(1));
        assert!(matches!(
            track_stream(&mut stream(0)?, &mut tracker),
            Err(TrackError::NoFrames)
        ));
        Ok(())
    }

    #[test]
    fn a_damaged_stream_is_a_fault_not_a_video_problem() -> Result<(), StreamError> {
        let mut data = b"AXONFRAMES 1 64 48 30.0\n".to_vec();
        data.resize(data.len() + 64 * 48 * 3 + 5, 0);
        let mut stream = FrameStream::open(std::io::Cursor::new(data))?;
        let mut tracker = HandTracker::new(SeesPalm, HandFor(5));
        let error = track_stream(&mut stream, &mut tracker).err();
        assert!(matches!(
            error,
            Some(TrackError::Stream(StreamError::Truncated))
        ));
        assert!(error.is_some_and(|e| !e.is_for_the_person()));
        Ok(())
    }
}
