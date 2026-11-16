//! What the person has chosen to run next, shared by every screen.

use std::collections::BTreeSet;

use leptos::prelude::*;

/// Where a run stands.
#[derive(Clone, Debug, PartialEq)]
pub enum RunState {
    /// Nothing is running.
    Idle,
    /// The server is working; `progress` is a fraction between 0 and 1.
    Working {
        /// What the server says it is doing.
        step: String,
        /// How far along it is.
        progress: f64,
    },
    /// The run stopped; this is a sentence for the person.
    Failed(String),
}

/// What the person has chosen to run next, shared by every screen: the test and its numbers, the
/// video, whether the set-up panel is open, and where the run stands.
#[derive(Clone, Copy)]
pub struct RunSelection {
    /// Identifier of the chosen test, if any.
    pub test_id: RwSignal<Option<String>>,
    /// Metric ids ticked for the next run.
    pub metrics: RwSignal<BTreeSet<String>>,
    /// The test whose default numbers `metrics` was last filled from.
    pub metrics_for: RwSignal<Option<String>>,
    /// Identifier of the chosen video, if any.
    pub video_id: RwSignal<Option<String>>,
    /// Whether the set-up panel is showing.
    pub panel_open: RwSignal<bool>,
    /// Whether the video step shows the upload form rather than the library.
    pub uploading: RwSignal<bool>,
    /// Whether a file is being sent, so the video to analyse is not settled yet.
    pub upload_busy: RwSignal<bool>,
    /// The run in progress, if any.
    pub run: RwSignal<RunState>,
}

impl RunSelection {
    /// Nothing chosen yet, the panel closed: it opens when the person asks for it.
    #[must_use]
    pub fn new() -> Self {
        Self {
            test_id: RwSignal::new(None),
            metrics: RwSignal::new(BTreeSet::new()),
            metrics_for: RwSignal::new(None),
            video_id: RwSignal::new(None),
            panel_open: RwSignal::new(false),
            uploading: RwSignal::new(false),
            upload_busy: RwSignal::new(false),
            run: RwSignal::new(RunState::Idle),
        }
    }

    /// Whether a test with at least one number and a video are chosen.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.test_id.get().is_some()
            && !self.metrics.get().is_empty()
            && self.video_id.get().is_some()
    }
}

impl Default for RunSelection {
    fn default() -> Self {
        Self::new()
    }
}

/// Add `id` to the set when `on`, remove it otherwise.
pub fn toggle(set: &mut BTreeSet<String>, id: &str, on: bool) {
    if on {
        set.insert(id.to_owned());
    } else {
        set.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_adds_and_removes_without_duplicates() {
        let mut set = BTreeSet::new();
        toggle(&mut set, "a", true);
        toggle(&mut set, "a", true);
        toggle(&mut set, "b", true);
        assert_eq!(set.len(), 2);
        toggle(&mut set, "a", false);
        toggle(&mut set, "zzz", false);
        assert_eq!(set.into_iter().collect::<Vec<_>>(), ["b"]);
    }
}
