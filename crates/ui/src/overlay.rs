//! The arithmetic behind drawing hand landmarks over a video and stepping through it by frame.
//!
//! Nothing here touches the browser, so it can be tested directly.

/// Pairs of landmarks joined by a line when the hand is drawn (the MediaPipe numbering: 0 is the
/// wrist, then four joints for each of the thumb, index, middle, ring and little finger).
pub const BONES: [(usize, usize); 21] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (0, 5),
    (5, 6),
    (6, 7),
    (7, 8),
    (5, 9),
    (9, 10),
    (10, 11),
    (11, 12),
    (9, 13),
    (13, 14),
    (14, 15),
    (15, 16),
    (13, 17),
    (17, 18),
    (18, 19),
    (19, 20),
    (0, 17),
];

/// Playback speeds offered by the keyboard.
pub const SPEEDS: [f64; 5] = [0.25, 0.5, 1.0, 1.5, 2.0];

/// What a key press asks of the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Play if paused, pause if playing.
    TogglePlay,
    /// Move this many frames (negative is backwards); the video is paused.
    Step(i32),
    /// Show or hide the drawn hand.
    ToggleOverlay,
    /// Play at the next higher speed.
    Faster,
    /// Play at the next lower speed.
    Slower,
}

/// The action for a key, or `None` when the key means nothing to the player.
///
/// `Shift` makes a step ten frames.
#[must_use]
pub fn action_for(key: &str, shift: bool) -> Option<Action> {
    let jump = if shift { 10 } else { 1 };
    match key {
        " " | "k" => Some(Action::TogglePlay),
        "ArrowRight" | "." | ">" => Some(Action::Step(jump)),
        "ArrowLeft" | "," | "<" => Some(Action::Step(-jump)),
        "o" => Some(Action::ToggleOverlay),
        "]" => Some(Action::Faster),
        "[" => Some(Action::Slower),
        _ => None,
    }
}

/// Index of the frame on screen at `time` seconds, or `None` when there are no frames.
///
/// Frame `n` is shown from `n / fps` up to the next frame.
#[must_use]
pub fn frame_index(time: f64, fps: f64, count: usize) -> Option<usize> {
    if count == 0 || !fps.is_finite() || fps <= 0.0 || !time.is_finite() {
        return None;
    }
    let index = (time * fps + 1e-6).floor().max(0.0) as usize;
    Some(index.min(count - 1))
}

/// The time to seek to so that the frame `frames` away from the current one is shown.
///
/// The time is the middle of the target frame, so a browser rounding it slightly still lands on
/// that frame. The result stays inside the video (`duration` seconds long).
#[must_use]
pub fn step_target(time: f64, fps: f64, frames: i32, duration: f64) -> f64 {
    if !fps.is_finite() || fps <= 0.0 {
        return time;
    }
    let current = (time * fps + 1e-6).floor();
    let last = ((duration * fps).round() - 1.0).max(0.0);
    let target = (current + f64::from(frames)).clamp(0.0, last);
    ((target + 0.5) / fps).min(duration.max(0.0))
}

/// The next speed up or down from `rate`, staying within [`SPEEDS`].
#[must_use]
pub fn speed_after(rate: f64, faster: bool) -> f64 {
    let nearest = SPEEDS
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - rate).abs().total_cmp(&(b.1 - rate).abs()))
        .map_or(2, |(i, _)| i);
    let next = if faster {
        (nearest + 1).min(SPEEDS.len() - 1)
    } else {
        nearest.saturating_sub(1)
    };
    SPEEDS[next]
}

/// Where a picture of `video` size sits inside a box of `area` size when it is shrunk to fit
/// without changing its shape (what the browser does with `object-fit: contain`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// Factor from video pixels to box pixels.
    pub scale: f64,
    /// Gap to the left of the picture, in box pixels.
    pub left: f64,
    /// Gap above the picture, in box pixels.
    pub top: f64,
}

impl Fit {
    /// Fit a `video_width` x `video_height` picture into `box_width` x `box_height`; `None` when
    /// any side is not positive.
    #[must_use]
    pub fn contain(
        video_width: f64,
        video_height: f64,
        box_width: f64,
        box_height: f64,
    ) -> Option<Self> {
        let sides = [video_width, video_height, box_width, box_height];
        if sides.iter().any(|s| !s.is_finite() || *s <= 0.0) {
            return None;
        }
        let scale = (box_width / video_width).min(box_height / video_height);
        Some(Self {
            scale,
            left: (box_width - video_width * scale) / 2.0,
            top: (box_height - video_height * scale) / 2.0,
        })
    }

    /// The position in the box of a point given in video pixels.
    #[must_use]
    pub fn map(&self, point: [f64; 2]) -> (f64, f64) {
        (
            self.left + point[0] * self.scale,
            self.top + point[1] * self.scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_actions() {
        assert_eq!(action_for(" ", false), Some(Action::TogglePlay));
        assert_eq!(action_for("ArrowRight", false), Some(Action::Step(1)));
        assert_eq!(action_for("ArrowRight", true), Some(Action::Step(10)));
        assert_eq!(action_for(",", false), Some(Action::Step(-1)));
        assert_eq!(action_for("<", true), Some(Action::Step(-10)));
        assert_eq!(action_for("o", false), Some(Action::ToggleOverlay));
        assert_eq!(action_for("]", false), Some(Action::Faster));
        assert_eq!(action_for("x", false), None);
        assert_eq!(action_for("Tab", false), None);
    }

    #[test]
    fn every_bone_joins_two_different_landmarks_of_a_hand() {
        assert!(BONES.iter().all(|&(a, b)| a != b && a < 21 && b < 21));
        // Every landmark is reached by at least one bone.
        assert!((0..21).all(|i| BONES.iter().any(|&(a, b)| a == i || b == i)));
    }

    #[test]
    fn the_frame_on_screen_follows_the_time() {
        assert_eq!(frame_index(0.0, 30.0, 100), Some(0));
        assert_eq!(frame_index(1.0 / 30.0, 30.0, 100), Some(1));
        assert_eq!(frame_index(0.05, 30.0, 100), Some(1));
        assert_eq!(frame_index(1000.0, 30.0, 100), Some(99));
        assert_eq!(frame_index(-1.0, 30.0, 100), Some(0));
        assert_eq!(frame_index(1.0, 30.0, 0), None);
        assert_eq!(frame_index(1.0, 0.0, 10), None);
        assert_eq!(frame_index(f64::NAN, 30.0, 10), None);
    }

    #[test]
    fn stepping_lands_in_the_middle_of_the_target_frame() {
        let t = step_target(0.0, 30.0, 1, 10.0);
        assert_eq!(frame_index(t, 30.0, 300), Some(1));
        let back = step_target(t, 30.0, -1, 10.0);
        assert_eq!(frame_index(back, 30.0, 300), Some(0));
        let ten = step_target(1.0, 30.0, 10, 10.0);
        assert_eq!(frame_index(ten, 30.0, 300), Some(40));
    }

    #[test]
    fn stepping_stays_inside_the_video() {
        assert_eq!(
            frame_index(step_target(0.0, 30.0, -5, 10.0), 30.0, 300),
            Some(0)
        );
        let end = step_target(9.9, 30.0, 50, 10.0);
        assert!(end <= 10.0);
        assert_eq!(frame_index(end, 30.0, 300), Some(299));
        assert!((step_target(2.0, 0.0, 1, 10.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn speeds_move_one_step_and_stop_at_the_ends() {
        assert_eq!(speed_after(1.0, true), 1.5);
        assert_eq!(speed_after(1.0, false), 0.5);
        assert_eq!(speed_after(2.0, true), 2.0);
        assert_eq!(speed_after(0.25, false), 0.25);
        assert_eq!(speed_after(1.2, true), 1.5);
    }

    #[test]
    fn a_wide_picture_in_a_square_box_is_centred_with_bars_above_and_below() {
        let Some(fit) = Fit::contain(1920.0, 1080.0, 400.0, 400.0) else {
            return;
        };
        assert!((fit.scale - 400.0 / 1920.0).abs() < 1e-12);
        assert!((fit.left).abs() < 1e-9);
        assert!((fit.top - (400.0 - 225.0) / 2.0).abs() < 1e-9);
        let (x, y) = fit.map([960.0, 540.0]);
        assert!((x - 200.0).abs() < 1e-9 && (y - 200.0).abs() < 1e-9);
    }

    #[test]
    fn a_tall_picture_gets_bars_at_the_sides() {
        let fit = Fit::contain(1080.0, 1920.0, 800.0, 400.0);
        assert!(fit.is_some_and(|f| f.top.abs() < 1e-9 && f.left > 0.0));
    }

    #[test]
    fn an_empty_picture_or_box_cannot_be_fitted() {
        assert_eq!(Fit::contain(0.0, 100.0, 10.0, 10.0), None);
        assert_eq!(Fit::contain(100.0, 100.0, 0.0, 10.0), None);
        assert_eq!(Fit::contain(f64::NAN, 100.0, 10.0, 10.0), None);
    }
}
