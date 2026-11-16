//! The set-up panel beside the analysis: choose a test, choose or upload a video, then run.

use std::rc::Rc;

use axonhand_api::{GateStatus, Hand, TestStatus, UploadMeta, Video};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, File, HtmlInputElement};

use crate::client::{fetch_test, fetch_tests, fetch_videos, upload_video};
use crate::format::{clip_length, file_size, hand_label};
use crate::pages::run::start_run;
use crate::selection::{toggle, RunSelection, RunState};

/// The button in a result's header that shows or hides the panel.
#[component]
pub fn SetupToggle() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    view! {
        <button
            type="button"
            class="tool tool--icon"
            aria-label="Set-up panel"
            aria-pressed=move || selection.panel_open.get().to_string()
            on:click=move |_| selection.panel_open.update(|open| *open = !*open)
        >
            <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true" focusable="false">
                <path d="M2 5h8M14 5h2M2 13h2M8 13h8" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"></path>
                <circle cx="12" cy="5" r="2" fill="none" stroke="currentColor" stroke-width="1.6"></circle>
                <circle cx="6" cy="13" r="2" fill="none" stroke="currentColor" stroke-width="1.6"></circle>
            </svg>
        </button>
    }
}

/// The panel itself. Always mounted so an upload keeps going while it is hidden.
#[component]
pub fn SetupPanel() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    provide_context(Reload(RwSignal::new(0)));
    view! {
        <aside class="setup" class:is-open=move || selection.panel_open.get() aria-label="Set up analysis" inert=move || !selection.panel_open.get()>
            <div class="setup__head">
                <h2 class="setup__title">"Set up analysis"</h2>
                <button type="button" class="tool" on:click=move |_| selection.panel_open.set(false)>"Close"</button>
            </div>
            <TestStep/>
            <VideoStep/>
            <RunStep/>
        </aside>
    }
}

#[component]
fn TestStep() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let tests = LocalResource::new(fetch_tests);
    let query = RwSignal::new(String::new());
    let detail = LocalResource::new(move || {
        let id = selection.test_id.get();
        async move {
            match id {
                Some(id) => Some(fetch_test(&id).await),
                None => None,
            }
        }
    });

    let open = RwSignal::new(false);
    let typing = RwSignal::new(false);
    let active = RwSignal::new(0_usize);

    // Tests that match what was typed; everything when the box is only being opened.
    let choices = move || -> Vec<(String, String, String, bool)> {
        let wanted = if typing.get() {
            query.get().trim().to_lowercase()
        } else {
            String::new()
        };
        tests
            .get()
            .and_then(Result::ok)
            .map(|list| {
                list.tests
                    .into_iter()
                    .filter(|t| t.name.to_lowercase().contains(&wanted))
                    .map(|t| {
                        (
                            t.id,
                            t.name,
                            t.clinical_item,
                            t.status == TestStatus::Available,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let choose = move |id: String, name: String| {
        selection.test_id.set(Some(id));
        query.set(name);
        typing.set(false);
        open.set(false);
    };
    let on_input = move |event: leptos::ev::Event| {
        let text = event_target_value(&event);
        query.set(text.clone());
        typing.set(true);
        open.set(true);
        active.set(0);
        // A full, exact name chooses that test at once.
        let wanted = text.trim().to_lowercase();
        if let Some(Ok(list)) = tests.get_untracked() {
            if let Some(test) = list
                .tests
                .iter()
                .find(|t| t.status == TestStatus::Available && t.name.to_lowercase() == wanted)
            {
                if selection.test_id.get_untracked().as_deref() != Some(test.id.as_str()) {
                    selection.test_id.set(Some(test.id.clone()));
                }
                open.set(false);
            }
        }
    };
    let on_key = move |event: leptos::ev::KeyboardEvent| {
        let count = choices().len();
        match event.key().as_str() {
            "ArrowDown" => {
                event.prevent_default();
                if open.get_untracked() {
                    active.update(|i| *i = (*i + 1).min(count.saturating_sub(1)));
                } else {
                    open.set(true);
                }
            }
            "ArrowUp" => {
                event.prevent_default();
                active.update(|i| *i = i.saturating_sub(1));
            }
            "Enter" if open.get_untracked() => {
                event.prevent_default();
                if let Some((id, name, _, true)) = choices().get(active.get_untracked()).cloned() {
                    choose(id, name);
                }
            }
            "Escape" if open.get_untracked() => {
                event.prevent_default();
                open.set(false);
            }
            _ => {}
        }
    };

    // Keep the box and the numbers in step with whichever test is chosen, wherever it was chosen.
    Effect::new(move |_| {
        if let Some(Some(Ok(test))) = detail.get() {
            let id = test.summary.id.clone();
            if selection.metrics_for.get_untracked().as_deref() != Some(id.as_str()) {
                selection.metrics.set(
                    test.metrics
                        .iter()
                        .filter(|m| m.default_selected)
                        .map(|m| m.id.clone())
                        .collect(),
                );
                selection.metrics_for.set(Some(id));
            }
            if query.get_untracked().trim().to_lowercase() != test.summary.name.to_lowercase() {
                query.set(test.summary.name.clone());
            }
        }
    });

    view! {
        <section class="setup__step" aria-labelledby="step-test">
            <h3 class="eyebrow" id="step-test">"Test"</h3>
            <p class="setup__ask">"What do you want to measure?"</p>
            <div class="combo">
                <label class="visually-hidden" for="test-search">"Search or choose a test"</label>
                <input
                    id="test-search"
                    type="text"
                    role="combobox"
                    aria-autocomplete="list"
                    aria-expanded=move || open.get().to_string()
                    aria-controls="test-options"
                    aria-activedescendant=move || open.get().then(|| format!("test-opt-{}", active.get()))
                    autocomplete="off"
                    placeholder="Search or choose a test"
                    prop:value=move || query.get()
                    on:input=on_input
                    on:keydown=on_key
                    on:focus=move |_| {
                        typing.set(false);
                        open.set(true);
                    }
                    on:blur=move |_| open.set(false)
                />
                <button type="button" class="combo__toggle" tabindex="-1" aria-hidden="true"
                    on:mousedown=move |e| {
                        e.prevent_default();
                        typing.set(false);
                        open.update(|o| *o = !*o);
                    }>
                    <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true" focusable="false">
                        <path d="M3 5l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path>
                    </svg>
                </button>
                <ul id="test-options" class="combo__list" role="listbox" aria-label="Tests" hidden=move || !open.get()>
                    {move || {
                        let items = choices();
                        if items.is_empty() {
                            view! { <li class="combo__none" role="presentation">"No test matches."</li> }.into_any()
                        } else {
                            items.into_iter().enumerate().map(|(i, (id, name, item, available))| {
                                let picked = {
                                    let id = id.clone();
                                    move || selection.test_id.get().as_deref() == Some(id.as_str())
                                };
                                let pick_name = name.clone();
                                view! {
                                    <li
                                        id=format!("test-opt-{i}")
                                        role="option"
                                        class="combo__option"
                                        class:is-active=move || active.get() == i
                                        class:is-disabled=!available
                                        aria-selected=move || picked().to_string()
                                        aria-disabled=(!available).then_some("true")
                                        on:mousedown=move |e| {
                                            e.prevent_default();
                                            if available {
                                                choose(id.clone(), pick_name.clone());
                                            }
                                        }
                                        on:mousemove=move |_| active.set(i)
                                    >
                                        <span class="combo__name">{name}</span>
                                        <span class="combo__item">{if available { item } else { format!("{item} · planned") }}</span>
                                    </li>
                                }
                            }).collect_view().into_any()
                        }
                    }}
                </ul>
            </div>
            {move || tests.get().and_then(Result::err).map(|message| view! {
                <p class="field-error" role="alert">{message}</p>
            })}
            <Suspense fallback=|| ()>
                {move || detail.get().flatten().map(|result| match result {
                    Err(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                    Ok(test) => {
                        let capture = test.capture.clone();
                        view! {
                            <div class="setup__card">
                                <p class="setup__card-title">
                                    <strong>{test.summary.name.clone()}</strong>
                                    <span class="muted">{format!(" · {}", test.summary.clinical_item)}</span>
                                </p>
                                <p class="muted">{test.summary.summary.clone()}</p>
                                <h4 class="eyebrow eyebrow--sub">"Numbers to report"</h4>
                                <ul class="metric-list">
                                    {test.metrics.iter().map(|m| {
                                        let mid = m.id.clone();
                                        let checked = { let mid = mid.clone(); move || selection.metrics.get().contains(&mid) };
                                        let field = format!("metric-{}", m.id);
                                        view! {
                                            <li>
                                                <input type="checkbox" id=field.clone()
                                                    prop:checked=checked
                                                    on:change=move |e| selection.metrics.update(|set| toggle(set, &mid, event_target_checked(&e)))/>
                                                <label for=field>{m.name.clone()}</label>
                                            </li>
                                        }
                                    }).collect_view()}
                                </ul>
                                <h4 class="eyebrow eyebrow--sub">"How to film this test"</h4>
                                <p class="muted">{capture.instructions}</p>
                            </div>
                        }.into_any()
                    }
                })}
            </Suspense>
        </section>
    }
}

fn first_file(list: Option<web_sys::FileList>) -> Option<File> {
    list.and_then(|files| files.get(0))
}

#[component]
fn VideoStep() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    view! {
        <section class="setup__step" aria-labelledby="step-video">
            <h3 class="eyebrow" id="step-video">"Video"</h3>
            <p class="setup__ask">"Which recording?"</p>
            <div class="seg" role="group" aria-label="Where the video comes from">
                <button type="button" aria-pressed=move || (!selection.uploading.get()).to_string()
                    on:click=move |_| selection.uploading.set(false)>"From the library"</button>
                <button type="button" aria-pressed=move || selection.uploading.get().to_string()
                    on:click=move |_| selection.uploading.set(true)>"Upload new"</button>
            </div>
            <div hidden=move || selection.uploading.get()><LibraryPick/></div>
            <div hidden=move || !selection.uploading.get()><UploadForm/></div>
        </section>
    }
}

/// Reload counter bumped after an upload so the library list refreshes.
#[derive(Clone, Copy)]
struct Reload(RwSignal<u32>);

#[component]
fn LibraryPick() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let reload = use_context::<Reload>().map_or_else(|| RwSignal::new(0), |r| r.0);
    let search = RwSignal::new(String::new());
    let videos = LocalResource::new(move || {
        reload.track();
        let code = search.get();
        async move { fetch_videos(&code, None).await }
    });
    view! {
        <div class="field">
            <label for="filter-code">"Participant"</label>
            <input id="filter-code" type="search" autocomplete="off" placeholder="Search by participant"
                on:input=move |e| search.set(event_target_value(&e))/>
        </div>
        <Suspense fallback=|| view! { <p class="muted" role="status">"Loading videos…"</p> }>
            {move || videos.get().map(|result| match result {
                Err(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                Ok(list) if list.videos.is_empty() => view! {
                    <p class="muted">"No videos yet. Choose “Upload new” to add one."</p>
                }.into_any(),
                Ok(list) => view! {
                    <ul class="pick-list">
                        {list.videos.into_iter().map(|video| view! { <PickCard video/> }).collect_view()}
                    </ul>
                }.into_any(),
            })}
        </Suspense>
        <button type="button" class="pick-add" on:click=move |_| selection.uploading.set(true)>"+ Upload a new video"</button>
    }
}

#[component]
fn PickCard(video: Video) -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let id = video.id.clone();
    let chosen = {
        let id = id.clone();
        Memo::new(move |_| selection.video_id.get().as_deref() == Some(id.as_str()))
    };
    let failing = video.gate.iter().any(|c| c.status == GateStatus::Fail);
    let (label, class) = if failing {
        ("Check", "badge badge--bad")
    } else {
        ("Pass", "badge badge--good")
    };
    let note = video
        .gate
        .iter()
        .find(|c| c.status == GateStatus::Fail)
        .map(|c| c.detail.clone());
    view! {
        <li class="pick" class:is-selected=move || chosen.get()>
            <button type="button" class="pick__button" aria-pressed=move || chosen.get().to_string()
                on:click=move |_| selection.video_id.set(Some(id.clone()))>
                <span class="pick__title">
                    <strong>{video.participant_code.clone()}</strong>
                    <span class=class>{label}</span>
                </span>
                <span class="muted">{format!(
                    "{} · visit {} · {}",
                    hand_label(video.hand), video.visit_date, clip_length(video.duration_seconds)
                )}</span>
            </button>
            {note.map(|n| view! { <p class="pick__warn">{format!("{n} You can still analyse it.")}</p> })}
        </li>
    }
}

/// Where an upload stands.
#[derive(Clone, Debug, PartialEq)]
enum Upload {
    Idle,
    Sending(f64),
    Failed(String),
}

#[component]
fn UploadForm() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let reload = use_context::<Reload>().map_or_else(|| RwSignal::new(0), |r| r.0);
    let code = RwSignal::new(String::new());
    let date = RwSignal::new(String::new());
    let hand = RwSignal::new(None::<Hand>);
    let file = RwSignal::new_local(None::<File>);
    let upload = RwSignal::new(Upload::Idle);
    let attempted = RwSignal::new(false);

    let problem = move |field: &'static str| -> Option<String> {
        if !attempted.get() {
            return None;
        }
        let Some(chosen) = hand.get() else {
            return (field == "hand").then(|| "Choose left or right.".to_owned());
        };
        let meta = UploadMeta {
            participant_code: code.get(),
            visit_date: date.get(),
            hand: chosen,
        };
        meta.validated()
            .err()
            .filter(|e| e.field.as_deref() == Some(field))
            .map(|e| e.message)
    };
    let file_problem = move || {
        (attempted.get() && file.get().is_none()).then(|| "Choose a video file.".to_owned())
    };

    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        attempted.set(true);
        let (Some(chosen_hand), Some(chosen_file)) = (hand.get_untracked(), file.get_untracked())
        else {
            return;
        };
        let Ok(meta) = (UploadMeta {
            participant_code: code.get_untracked(),
            visit_date: date.get_untracked(),
            hand: chosen_hand,
        })
        .validated() else {
            return;
        };
        if matches!(upload.get_untracked(), Upload::Sending(_)) {
            return;
        }
        upload.set(Upload::Sending(0.0));
        selection.upload_busy.set(true);
        upload_video(
            &meta,
            &chosen_file,
            move |fraction| upload.set(Upload::Sending(fraction)),
            move |result| match result {
                Ok(response) => {
                    selection.upload_busy.set(false);
                    selection.video_id.set(Some(response.video.id));
                    upload.set(Upload::Idle);
                    file.set(None);
                    attempted.set(false);
                    reload.update(|n| *n += 1);
                    selection.uploading.set(false);
                }
                Err(message) => {
                    selection.upload_busy.set(false);
                    upload.set(Upload::Failed(message));
                }
            },
        );
    };

    let on_pick = move |event: leptos::ev::Event| {
        let input = event
            .target()
            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
        file.set(input.and_then(|i| first_file(i.files())));
    };
    let on_drop = move |event: DragEvent| {
        event.prevent_default();
        let dropped = event.data_transfer().and_then(|t| first_file(t.files()));
        if dropped.is_some() {
            file.set(dropped);
        }
    };
    let percent = move || match upload.get() {
        Upload::Sending(f) => (f * 100.0).round().clamp(0.0, 100.0),
        _ => 0.0,
    };

    view! {
        <form class="upload-form" on:submit=submit novalidate>
            <div class="dropzone" on:dragover=move |e: DragEvent| e.prevent_default() on:drop=on_drop>
                <label for="video-file">"Video file"</label>
                <input id="video-file" type="file" accept="video/mp4,video/quicktime,video/webm,.mp4,.mov,.webm" on:change=on_pick/>
                <p class="muted">
                    {move || file.get().map_or_else(
                        || "Choose a file or drop one here. MP4, MOV or WebM.".to_owned(),
                        |f| format!("{} ({})", f.name(), file_size(f.size() as u64)),
                    )}
                </p>
                <p class="field-error" role="alert">{file_problem}</p>
            </div>
            <div class="field">
                <label for="participant">"Participant code"</label>
                <input id="participant" type="text" autocomplete="off" maxlength="32"
                    prop:value=move || code.get() on:input=move |e| code.set(event_target_value(&e))/>
                <p class="field-error" role="alert">{move || problem("participant_code")}</p>
            </div>
            <div class="field">
                <label for="visit-date">"Visit date"</label>
                <input id="visit-date" type="date"
                    prop:value=move || date.get() on:input=move |e| date.set(event_target_value(&e))/>
                <p class="field-error" role="alert">{move || problem("visit_date")}</p>
            </div>
            <fieldset class="field">
                <legend>"Hand filmed"</legend>
                <label class="choice"><input type="radio" name="hand" on:change=move |_| hand.set(Some(Hand::Left))/>" Left"</label>
                <label class="choice"><input type="radio" name="hand" on:change=move |_| hand.set(Some(Hand::Right))/>" Right"</label>
                <p class="field-error" role="alert">{move || problem("hand")}</p>
            </fieldset>
            <button type="submit" class="progress-btn" style=move || format!("--p:{}%", percent())
                aria-disabled=move || matches!(upload.get(), Upload::Sending(_)).to_string()>
                <span>{move || match upload.get() {
                    Upload::Sending(_) => format!("Uploading {:.0}%", percent()),
                    _ => "Upload video".to_owned(),
                }}</span>
            </button>
            {move || match upload.get() {
                Upload::Failed(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                _ => ().into_any(),
            }}
        </form>
    }
}

#[component]
fn RunStep() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let navigate = use_navigate();
    let use_npu = RwSignal::new(false);
    let test_done =
        move || selection.test_id.get().is_some() && !selection.metrics.get().is_empty();
    let video_done = move || selection.video_id.get().is_some();
    let busy = move || matches!(selection.run.get(), RunState::Working { .. });
    let go = {
        let navigate = navigate.clone();
        move |_| {
            if !selection.ready()
                || matches!(selection.run.get_untracked(), RunState::Working { .. })
            {
                return;
            }
            let to_result: Rc<dyn Fn(&str)> = {
                let navigate = navigate.clone();
                Rc::new(move |key: &str| {
                    selection.panel_open.set(false);
                    navigate(&format!("/?key={key}"), NavigateOptions::default());
                })
            };
            start_run(selection, use_npu.get_untracked(), to_result);
            selection.panel_open.set(false);
            navigate("/", NavigateOptions::default());
        }
    };
    view! {
        <section class="setup__step" aria-labelledby="step-run">
            <h3 class="eyebrow" id="step-run">"Run"</h3>
            <p class="setup__ask">"Ready when you are."</p>
            <ul class="checks">
                <li class:is-done=test_done>{move || if test_done() { "Test chosen" } else { "Choose a test" }}</li>
                <li class:is-done=video_done>{move || if video_done() { "Video chosen" } else { "Choose a video" }}</li>
            </ul>
            <label class="choice">
                <input type="checkbox" prop:checked=move || use_npu.get()
                    on:change=move |e| use_npu.set(event_target_checked(&e))/>
                " Run on the NPU if this computer has one"
            </label>
            {move || match selection.run.get() {
                RunState::Failed(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                _ => ().into_any(),
            }}
            <button type="button" class="button button--wide"
                disabled=move || !selection.ready() || busy() || selection.upload_busy.get()
                on:click=go>
                "Analyse this video"
            </button>
        </section>
    }
}
