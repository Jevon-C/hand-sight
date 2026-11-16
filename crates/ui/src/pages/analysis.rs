//! The Analysis screen: a saved result when the address names one, otherwise an empty page that
//! fills in as the test and video are chosen in the set-up panel and runs when asked.

use axonhand_api::Video;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::client::{fetch_test, fetch_videos};
use crate::format::{hand_label, progress_percent};
use crate::pages::results::{load, ResultView};
use crate::pages::setup::{SetupPanel, SetupToggle};
use crate::player::VideoPlayer;
use crate::selection::{RunSelection, RunState};

/// Analysis page with the set-up panel pinned on the right.
#[component]
pub fn Analysis() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let query = use_query_map();
    let key = Memo::new(move |_| query.read().get("key").filter(|k| !k.is_empty()));

    // Opening a saved result puts the panel away. A new analysis does not open it by itself:
    // the person opens it from the button beside the actions.
    Effect::new(move |_| {
        if key.get().is_some() {
            selection.panel_open.set(false);
        }
    });
    // A run that fails needs the panel, where the reason is shown.
    Effect::new(move |_| {
        if matches!(selection.run.get(), RunState::Failed(_)) {
            selection.panel_open.set(true);
        }
    });

    let data = LocalResource::new(move || {
        let key = key.get();
        async move {
            match key {
                Some(key) => Some(load(key).await),
                None => None,
            }
        }
    });

    view! {
        <div class="analysis" class:analysis--open=move || selection.panel_open.get()>
            <div class="analysis__main">
                {move || match key.get() {
                    None => view! { <Draft/> }.into_any(),
                    Some(_) => view! {
                        <Suspense fallback=|| view! { <p class="muted" role="status">"Loading the result…"</p> }>
                            {move || data.get().flatten().map(|loaded| match loaded {
                                Err(message) => view! {
                                    <h1 class="page-title">"Analysis"</h1>
                                    <section class="card empty">
                                        <p class="field-error" role="alert">{message}</p>
                                        <p class="muted">"This result could not be opened. It may have been removed, or the address may be incomplete."</p>
                                        <div class="empty__actions">
                                            <A href="/" attr:class="button">"Start a new analysis"</A>
                                            <A href="/library?tab=results" attr:class="button button--quiet">"Open the library"</A>
                                        </div>
                                    </section>
                                }.into_any(),
                                Ok(loaded) => view! { <ResultView loaded/> }.into_any(),
                            })}
                        </Suspense>
                    }.into_any(),
                }}
            </div>
            <div class="backdrop" class:is-open=move || selection.panel_open.get()
                on:click=move |_| selection.panel_open.set(false)></div>
            <SetupPanel/>
        </div>
    }
}

/// The page before there is a result: placeholders that fill in as choices are made.
#[component]
fn Draft() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let video = LocalResource::new(move || {
        let id = selection.video_id.get();
        async move {
            let id = id?;
            let list = fetch_videos("", None).await.ok()?;
            list.videos.into_iter().find(|v| v.id == id)
        }
    });
    let test = LocalResource::new(move || {
        let id = selection.test_id.get();
        async move {
            match id {
                Some(id) => fetch_test(&id).await.ok(),
                None => None,
            }
        }
    });
    let video_now = move || video.get().flatten();
    let running = move || matches!(selection.run.get(), RunState::Working { .. });

    view! {
        <header class="result-head">
            <h1 class="result-title">
                {move || video_now().map_or_else(|| "New analysis".to_owned(), |v| v.participant_code)}
            </h1>
            <div class="result-bar">
                <p class="pill-group">
                    <span class:pill-group--none=move || video_now().is_none()>
                        {move || video_now().map_or("Hand", |v| hand_label(v.hand))}
                    </span>
                    <span class="pill-group__rule" aria-hidden="true"></span>
                    <span class:pill-group--none=move || test.get().flatten().is_none()>
                        {move || test.get().flatten().map_or_else(|| "Test".to_owned(), |t| t.summary.name)}
                    </span>
                </p>
                <div class="result-actions">
                    <button type="button" class="tool tool--filled" disabled>"Download as CSV"</button>
                    <button type="button" class="tool" disabled>"Print this report"</button>
                    <button type="button" class="tool" disabled>"Compare results"</button>
                    <SetupToggle/>
                </div>
            </div>
        </header>

        <div class="analysis__body" class:is-running=running>
        <section class="card stage" aria-label="Video and signal">
            {move || match video_now() {
                Some(v) => view! { <ChosenVideo video=v/> }.into_any(),
                None => view! {
                    <div class="stage__empty">
                        <svg width="44" height="44" viewBox="0 0 44 44" aria-hidden="true" focusable="false">
                            <rect x="5" y="9" width="34" height="26" rx="5" fill="none" stroke="currentColor" stroke-width="2"></rect>
                            <path d="M19 17l9 5-9 5z" fill="currentColor"></path>
                        </svg>
                        <p>"No video yet"</p>
                    </div>
                    <div class="timeline timeline--empty">"The timeline appears when the analysis is done."</div>
                }.into_any(),
            }}
            <RunCard/>
        </section>

        <div class="pair">
            <section class="card" aria-labelledby="measures-h">
                <h2 class="eyebrow" id="measures-h">"Measures"</h2>
                {move || match test.get().flatten() {
                    None => view! { <p class="placeholder-box">"No test chosen yet"</p> }.into_any(),
                    Some(t) => {
                        let chosen = selection.metrics.get();
                        view! {
                            <ul class="measure-grid" aria-label="Measures">
                                {t.metrics.iter().filter(|m| chosen.contains(&m.id)).map(|m| view! {
                                    <li class="measure measure--ghost">
                                        <div class="measure__text">
                                            <p class="measure__name">{m.name.clone()}</p>
                                            <p class="measure__desc">{m.description.clone()}</p>
                                        </div>
                                        <p class="measure__value"><span class="measure__number measure__number--none">"–"</span></p>
                                    </li>
                                }).collect_view()}
                            </ul>
                        }.into_any()
                    }
                }}
            </section>
            <section class="card is-dimmed" aria-labelledby="scores-h">
                <h2 class="eyebrow" id="scores-h">"Clinician scores"</h2>
                <p class="muted">"Opens when the analysis is done."</p>
            </section>
        </div>

        <section class="card" aria-labelledby="repeat-h">
            <h2 class="eyebrow" id="repeat-h">"How to repeat this result"</h2>
            <dl class="facts">
                <dt>"Test and version"</dt>
                <dd>{move || test.get().flatten().map_or_else(|| "–".to_owned(), |t| format!("{} {}", t.summary.name, t.summary.version))}</dd>
                <dt>"Settings"</dt>
                <dd>{move || test.get().flatten().map_or_else(|| "–".to_owned(), |t| {
                    if t.parameters.is_empty() {
                        "Standard settings".to_owned()
                    } else {
                        t.parameters.iter().map(|p| format!("{} = {}", p.name, p.default)).collect::<Vec<_>>().join(", ")
                    }
                })}</dd>
                <dt>"Hand model"</dt><dd>"Shown once analysed"</dd>
                <dt>"Video"</dt>
                <dd class="mono">{move || selection.video_id.get().unwrap_or_else(|| "–".to_owned())}</dd>
                <dt>"Result key"</dt><dd>"–"</dd>
                <dt>"Created"</dt><dd>"–"</dd>
            </dl>
        </section>
        </div>
    }
}

/// The chosen video, not yet analysed: its first frame and the play and frame-step controls.
#[component]
fn ChosenVideo(video: Video) -> impl IntoView {
    let url = format!("/api/videos/{}/stream", video.id);
    view! {
        <p class="player__tag player__tag--note">"Not analysed yet"</p>
        <VideoPlayer video_url=url overlay=None/>
        <div class="timeline timeline--empty">"The timeline appears when the analysis is done."</div>
    }
}

/// Shown over the video while the server works.
#[component]
fn RunCard() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    view! {
        <Show when=move || matches!(selection.run.get(), RunState::Working { .. })>
            <div class="run-card" role="status">
                <p class="run-card__title">"Analysing"</p>
                <p class="muted">
                    {move || match selection.run.get() {
                        RunState::Working { step, .. } => step,
                        _ => String::new(),
                    }}
                </p>
                <div class="run-card__bar" aria-hidden="true">
                    <span style=move || match selection.run.get() {
                        RunState::Working { progress, .. } => format!("width:{}%", progress_percent(progress)),
                        _ => "width:0%".to_owned(),
                    }></span>
                </div>
                <p class="run-card__percent">
                    {move || match selection.run.get() {
                        RunState::Working { progress, .. } => format!("{}%", progress_percent(progress)),
                        _ => String::new(),
                    }}
                </p>
            </div>
        </Show>
    }
}
