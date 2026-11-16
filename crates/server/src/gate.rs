//! Quality gate: decides from a video's facts whether it is good enough to analyse.
//!
//! The gate only reports. It never blocks an upload, so a person can always see why a clip
//! was flagged. Checks that need the hand or board detectors cannot be made yet and are
//! reported as [`GateStatus::NotChecked`], never as a pass.

use axonhand_api::{GateCheck, GateCheckName, GateStatus};

/// Lowest frame rate that resolves finger taps. 59.94 fps cameras must pass.
pub const MIN_FPS: f64 = 50.0;
/// Shortest clip that can hold a full tapping task, in seconds.
pub const MIN_DURATION_SECONDS: f64 = 5.0;

/// Run every check for a video with the given frame rate and length.
#[must_use]
pub fn evaluate(fps: f64, duration_seconds: f64) -> Vec<GateCheck> {
    let frame_rate = if fps >= MIN_FPS {
        (GateStatus::Pass, format!("{fps:.1} frames per second."))
    } else {
        (
            GateStatus::Fail,
            format!(
                "{fps:.1} frames per second; at least {MIN_FPS:.0} are needed to see finger taps."
            ),
        )
    };
    let duration = if duration_seconds >= MIN_DURATION_SECONDS {
        (GateStatus::Pass, format!("{duration_seconds:.1} seconds."))
    } else {
        (
            GateStatus::Fail,
            format!(
                "{duration_seconds:.1} seconds; at least {MIN_DURATION_SECONDS:.0} are needed."
            ),
        )
    };
    let later = "Not checked yet: needs the hand detector.";
    vec![
        GateCheck {
            check: GateCheckName::FrameRate,
            status: frame_rate.0,
            detail: frame_rate.1,
        },
        GateCheck {
            check: GateCheckName::Duration,
            status: duration.0,
            detail: duration.1,
        },
        GateCheck {
            check: GateCheckName::HandVisible,
            status: GateStatus::NotChecked,
            detail: later.to_owned(),
        },
        GateCheck {
            check: GateCheckName::BoardVisible,
            status: GateStatus::NotChecked,
            detail: "Not checked yet: needs the board detector.".to_owned(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(checks: &[GateCheck], name: GateCheckName) -> Option<GateStatus> {
        checks.iter().find(|c| c.check == name).map(|c| c.status)
    }

    #[test]
    fn a_sixty_fps_ten_second_clip_passes_what_can_be_checked() {
        let checks = evaluate(60.0, 10.0);
        assert_eq!(
            status(&checks, GateCheckName::FrameRate),
            Some(GateStatus::Pass)
        );
        assert_eq!(
            status(&checks, GateCheckName::Duration),
            Some(GateStatus::Pass)
        );
    }

    #[test]
    fn ntsc_sixty_passes_and_thirty_fails() {
        assert_eq!(
            status(&evaluate(59.94, 10.0), GateCheckName::FrameRate),
            Some(GateStatus::Pass)
        );
        assert_eq!(
            status(&evaluate(30.0, 10.0), GateCheckName::FrameRate),
            Some(GateStatus::Fail)
        );
    }

    #[test]
    fn short_clips_fail_the_duration_check() {
        assert_eq!(
            status(&evaluate(60.0, 4.9), GateCheckName::Duration),
            Some(GateStatus::Fail)
        );
        assert_eq!(
            status(&evaluate(60.0, 5.0), GateCheckName::Duration),
            Some(GateStatus::Pass)
        );
    }

    #[test]
    fn detector_checks_are_never_reported_as_passes() {
        let checks = evaluate(120.0, 60.0);
        assert_eq!(
            status(&checks, GateCheckName::HandVisible),
            Some(GateStatus::NotChecked)
        );
        assert_eq!(
            status(&checks, GateCheckName::BoardVisible),
            Some(GateStatus::NotChecked)
        );
        assert_eq!(checks.len(), 4);
    }

    #[test]
    fn failing_details_say_what_is_needed() {
        let checks = evaluate(30.0, 2.0);
        assert!(checks[0].detail.contains("at least 50"));
        assert!(checks[1].detail.contains("at least 5"));
    }
}
