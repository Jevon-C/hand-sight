//! Starting a test on a video and following it until its result is ready.

use std::rc::Rc;
use std::time::Duration;

use axonhand_api::{ComputeDevice, JobRequest};
use leptos::prelude::*;

use crate::client::{fetch_job, start_job};
use crate::selection::{RunSelection, RunState};

const POLL_EVERY: Duration = Duration::from_millis(400);

/// Check a job, update the screen, and ask again shortly unless it has finished. When it
/// succeeds the app moves to the result.
fn follow(job_id: String, state: RwSignal<RunState>, go_to_result: Rc<dyn Fn(&str)>) {
    leptos::task::spawn_local(async move {
        match fetch_job(&job_id).await {
            Err(message) => state.set(RunState::Failed(message)),
            Ok(job) => match job.state {
                axonhand_api::JobState::Succeeded => {
                    state.set(RunState::Idle);
                    go_to_result(&job.result_key);
                }
                axonhand_api::JobState::Failed => state
                    .set(RunState::Failed(job.error.unwrap_or_else(|| {
                        "The analysis stopped without a reason.".to_owned()
                    }))),
                axonhand_api::JobState::Queued | axonhand_api::JobState::Running => {
                    let step = if job.step.is_empty() {
                        "Waiting for its turn".to_owned()
                    } else {
                        job.step
                    };
                    state.set(RunState::Working {
                        step,
                        progress: job.progress,
                    });
                    set_timeout(move || follow(job_id, state, go_to_result), POLL_EVERY);
                }
            },
        }
    });
}

/// Start the chosen test on the chosen video. `use_npu` picks the compute device; `go_to_result`
/// is called with the result key when the run succeeds. Does nothing unless a test, its numbers
/// and a video are chosen, or when a run is already going.
pub fn start_run(selection: RunSelection, use_npu: bool, go_to_result: Rc<dyn Fn(&str)>) {
    if matches!(selection.run.get_untracked(), RunState::Working { .. }) {
        return;
    }
    let (Some(test_id), Some(video_id)) = (
        selection.test_id.get_untracked(),
        selection.video_id.get_untracked(),
    ) else {
        return;
    };
    let metrics = selection.metrics.get_untracked();
    if metrics.is_empty() {
        return;
    }
    let state = selection.run;
    state.set(RunState::Working {
        step: "Starting".to_owned(),
        progress: 0.0,
    });
    let request = JobRequest {
        video_id,
        test_id,
        metrics: metrics.into_iter().collect(),
        parameters: Default::default(),
        device: if use_npu {
            ComputeDevice::Npu
        } else {
            ComputeDevice::Cpu
        },
    };
    leptos::task::spawn_local(async move {
        match start_job(&request).await {
            Ok(job) => follow(job.id, state, go_to_result),
            Err(message) => state.set(RunState::Failed(message)),
        }
    });
}
