//! A video with the tracked hand drawn over it, and keyboard control by frame.
//!
//! The hand is drawn on a canvas laid over the video. A browser has no event for "a new frame was
//! shown" that works everywhere, so while the player is on screen a small loop looks at the
//! video's time on every screen refresh and redraws when it changed. The arithmetic (which frame,
//! where on the canvas, what a key means) is in [`crate::overlay`] and is tested there.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use axonhand_api::LandmarkOverlay;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement, KeyboardEvent};

use crate::overlay::{action_for, frame_index, speed_after, step_target, Action, Fit, BONES};

const OUTLINE: &str = "rgba(0, 0, 0, 0.65)";
const BONE: &str = "#ffffff";
const JOINT: &str = "#ffd43b";

/// The player's clock, shared with the charts beside it so that they can draw a playhead and ask
/// the video to move. `time` is the video's current time in seconds; setting `seek` moves the video
/// there once.
#[derive(Clone, Copy)]
pub struct Playhead {
    /// Current time of the video, in seconds.
    pub time: RwSignal<f64>,
    /// A time to jump to; the player clears it after moving.
    pub seek: RwSignal<Option<f64>>,
}

impl Playhead {
    /// A clock at zero with no jump pending.
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: RwSignal::new(0.0),
            seek: RwSignal::new(None),
        }
    }
}

impl Default for Playhead {
    fn default() -> Self {
        Self::new()
    }
}

/// What the last drawing was made from: time, whether the hand is shown, canvas size, picture size.
type DrawKey = (u64, bool, u32, u32, u32, u32);

/// Holds the loop's closure so that it can schedule itself again.
type LoopSlot = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

fn context(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    canvas
        .get_context("2d")
        .ok()
        .flatten()?
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()
}

/// Draw the hand of the frame on screen, or just clear the canvas.
fn draw(
    video: &HtmlVideoElement,
    canvas: &HtmlCanvasElement,
    overlay: &LandmarkOverlay,
    show: bool,
) {
    let rect = canvas.get_bounding_client_rect();
    let (width, height) = (rect.width(), rect.height());
    if width < 1.0 || height < 1.0 {
        return;
    }
    let (pixels_x, pixels_y) = (width.round() as u32, height.round() as u32);
    if canvas.width() != pixels_x {
        canvas.set_width(pixels_x);
    }
    if canvas.height() != pixels_y {
        canvas.set_height(pixels_y);
    }
    let Some(ctx) = context(canvas) else {
        return;
    };
    ctx.clear_rect(0.0, 0.0, width, height);
    if !show {
        return;
    }
    let Some(fit) = Fit::contain(
        f64::from(video.video_width()),
        f64::from(video.video_height()),
        width,
        height,
    ) else {
        return;
    };
    let Some(index) = frame_index(video.current_time(), overlay.fps, overlay.points.len()) else {
        return;
    };
    let Some(Some(points)) = overlay.points.get(index) else {
        return;
    };
    let at = |i: usize| points.get(i).copied().flatten().map(|p| fit.map(p));
    for (colour, line) in [(OUTLINE, 5.0), (BONE, 2.0)] {
        ctx.set_stroke_style_str(colour);
        ctx.set_line_width(line);
        ctx.begin_path();
        for (a, b) in BONES {
            if let (Some((ax, ay)), Some((bx, by))) = (at(a), at(b)) {
                ctx.move_to(ax, ay);
                ctx.line_to(bx, by);
            }
        }
        ctx.stroke();
    }
    ctx.set_fill_style_str(JOINT);
    for i in 0..points.len() {
        if let Some((x, y)) = at(i) {
            ctx.begin_path();
            if ctx.arc(x, y, 3.5, 0.0, std::f64::consts::TAU).is_ok() {
                ctx.fill();
            }
        }
    }
}

/// Redraw on every screen refresh while `alive` is true. The browser keeps the loop's closure, so
/// when the player goes away the loop stops rescheduling itself and the closure is left behind
/// (a few bytes, once per player shown).
fn start_loop(
    video: HtmlVideoElement,
    canvas: HtmlCanvasElement,
    overlay: Rc<LandmarkOverlay>,
    show: RwSignal<bool>,
    time: RwSignal<f64>,
    alive: RwSignal<bool>,
) {
    let slot: LoopSlot = Rc::new(RefCell::new(None));
    let again = Rc::clone(&slot);
    let last: Cell<Option<DrawKey>> = Cell::new(None);
    *slot.borrow_mut() = Some(Closure::new(move || {
        if !alive.try_get_untracked().unwrap_or(false) {
            return;
        }
        let now = video.current_time();
        let shown = show.try_get_untracked().unwrap_or(false);
        let key = (
            now.to_bits(),
            shown,
            canvas.client_width() as u32,
            canvas.client_height() as u32,
            // The picture's own size is known only once the video's metadata has loaded; the
            // hand must be drawn then even though the time and the canvas have not changed.
            video.video_width(),
            video.video_height(),
        );
        if last.get() != Some(key) {
            last.set(Some(key));
            time.try_set(now);
            draw(&video, &canvas, &overlay, shown);
        }
        if let (Some(window), Some(closure)) = (web_sys::window(), again.borrow().as_ref()) {
            let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
        }
    }));
    let first = slot.borrow();
    if let (Some(window), Some(closure)) = (web_sys::window(), first.as_ref()) {
        let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
    }
}

/// The video of a result. With `overlay`, the hand is drawn over it, and the keyboard steps through
/// it frame by frame once the player has focus.
#[component]
pub fn VideoPlayer(
    video_url: String,
    overlay: Option<LandmarkOverlay>,
    /// Drawn between the picture and the controls, such as a chart that follows the playhead.
    #[prop(optional, into)]
    timeline: Option<ViewFn>,
) -> impl IntoView {
    let playhead = use_context::<Playhead>();
    let playing = RwSignal::new(false);
    let video: NodeRef<html::Video> = NodeRef::new();
    let canvas: NodeRef<html::Canvas> = NodeRef::new();
    let show = RwSignal::new(true);
    let time = RwSignal::new(0.0_f64);
    let rate = RwSignal::new(1.0_f64);
    let alive = RwSignal::new(true);
    on_cleanup(move || alive.set(false));

    if let Some(playhead) = playhead {
        Effect::new(move |_| playhead.time.set(time.get()));
        Effect::new(move |_| {
            if let Some(target) = playhead.seek.get() {
                if let Some(player) = video.get_untracked() {
                    player.set_current_time(target);
                }
                playhead.seek.set(None);
            }
        });
    }

    let fps = overlay.as_ref().map_or(0.0, |o| o.fps);
    let frames = overlay.as_ref().map_or(0, |o| o.points.len());
    let shared = overlay.map(Rc::new);

    if let Some(shared) = shared.clone() {
        let started = Cell::new(false);
        Effect::new(move |_| {
            if let (Some(video), Some(canvas)) = (video.get(), canvas.get()) {
                if !started.replace(true) {
                    start_loop(video, canvas, Rc::clone(&shared), show, time, alive);
                }
            }
        });
    }

    let apply = Rc::new(move |action: Action| {
        let Some(player) = video.get_untracked() else {
            return;
        };
        match action {
            Action::TogglePlay => {
                if player.paused() {
                    let _ = player.play();
                } else {
                    let _ = player.pause();
                }
            }
            Action::Step(count) => {
                let duration = player.duration();
                if fps > 0.0 && duration.is_finite() {
                    let _ = player.pause();
                    player.set_current_time(step_target(
                        player.current_time(),
                        fps,
                        count,
                        duration,
                    ));
                }
            }
            Action::ToggleOverlay => show.update(|s| *s = !*s),
            Action::Faster | Action::Slower => {
                let next = speed_after(player.playback_rate(), action == Action::Faster);
                player.set_playback_rate(next);
                rate.set(next);
            }
        }
    });
    let on_key = {
        let apply = Rc::clone(&apply);
        move |event: KeyboardEvent| {
            if event.ctrl_key() || event.meta_key() || event.alt_key() {
                return;
            }
            if let Some(action) = action_for(&event.key(), event.shift_key()) {
                // Without a landmark file there is no frame rate to step by.
                if fps > 0.0
                    || matches!(action, Action::TogglePlay | Action::Faster | Action::Slower)
                {
                    event.prevent_default();
                    apply(action);
                }
            }
        }
    };
    let (back, forward) = (Rc::clone(&apply), Rc::clone(&apply));
    let has_overlay = shared.is_some();
    let toggle = Rc::clone(&apply);
    view! {
        <div class="player" tabindex="0" role="group" aria-label="Video with the tracked hand" on:keydown=on_key>
            <div class="player__stage">
                <video
                    node_ref=video
                    class="preview"
                    controls=!has_overlay
                    preload="metadata"
                    src=video_url
                    on:play=move |_| playing.set(true)
                    on:pause=move |_| playing.set(false)
                    on:ended=move |_| playing.set(false)
                ></video>
                <canvas node_ref=canvas class="player__overlay" aria-hidden="true"></canvas>
                {has_overlay.then(move || view! {
                    <span class="player__tag player__tag--frame" aria-hidden="true">
                        {move || match frame_index(time.get(), fps, frames) {
                            Some(index) => format!("{} / {}", index + 1, frames),
                            None => String::new(),
                        }}
                    </span>
                    <span class="player__tag player__tag--time" aria-hidden="true">
                        {move || format!("{:.2} s", time.get())}
                    </span>
                })}
            </div>
            {timeline.map(|timeline| timeline.run())}
            {has_overlay.then(move || view! {
                <div class="player__bar">
                    <button type="button" class="button button--quiet" on:click=move |_| back(Action::Step(-1))>
                        "Previous frame"
                    </button>
                    <button type="button" class="button" on:click=move |_| toggle(Action::TogglePlay)>
                        {move || if playing.get() { "Pause" } else { "Play" }}
                    </button>
                    <button type="button" class="button button--quiet" on:click=move |_| forward(Action::Step(1))>
                        "Next frame"
                    </button>
                    <label class="player__toggle">
                        <input
                            type="checkbox"
                            prop:checked=move || show.get()
                            on:change=move |_| show.update(|s| *s = !*s)
                        />
                        " Show the tracked hand"
                    </label>
                    <span class="muted" role="status">
                        {move || match frame_index(time.get(), fps, frames) {
                            Some(index) => format!(
                                "Frame {} of {} · {:.2} s · speed {}×",
                                index + 1,
                                frames,
                                time.get(),
                                rate.get()
                            ),
                            None => String::new(),
                        }}
                    </span>
                </div>
                <p class="player__help muted">
                    "Click the video, then press "
                    <kbd>"Space"</kbd>" to play or pause, "
                    <kbd>"←"</kbd><kbd>"→"</kbd>" to step a frame, "
                    <kbd>"Shift"</kbd>" with an arrow for ten, "
                    <kbd>"O"</kbd>" to hide the hand, "
                    <kbd>"["</kbd><kbd>"]"</kbd>" for speed."
                </p>
            })}
        </div>
    }
}
