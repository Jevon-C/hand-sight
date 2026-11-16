//! The Library: stored videos, installed tests and saved results.

use std::collections::{BTreeSet, HashMap};

use axonhand_api::{GateStatus, ResultSummary, TestStatus, TestSummary, Video};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_query_map};
use leptos_router::NavigateOptions;

use crate::client::{fetch_results, fetch_test, fetch_tests, fetch_videos};
use crate::format::{clip_length, hand_label};
use crate::selection::RunSelection;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Videos,
    Tests,
    Results,
}

impl Tab {
    const ALL: [(Tab, &'static str, &'static str); 3] = [
        (Tab::Videos, "videos", "Videos"),
        (Tab::Tests, "tests", "Tests"),
        (Tab::Results, "results", "Results"),
    ];

    fn from_query(text: Option<&str>) -> Self {
        match text {
            Some("tests") => Tab::Tests,
            Some("results") => Tab::Results,
            _ => Tab::Videos,
        }
    }
}

/// Library with its three tabs.
#[component]
pub fn Library() -> impl IntoView {
    let query = use_query_map();
    let tab = RwSignal::new(Tab::from_query(
        query.read_untracked().get("tab").as_deref(),
    ));
    view! {
        <h1 class="page-title">"Library"</h1>
        <div class="tabs" role="tablist" aria-label="Library sections">
            {Tab::ALL.iter().map(|&(which, id, label)| view! {
                <button type="button" role="tab" id=format!("tab-{id}")
                    aria-selected=move || (tab.get() == which).to_string()
                    aria-controls="library-panel"
                    on:click=move |_| tab.set(which)>
                    {label}
                </button>
            }).collect_view()}
        </div>
        <section class="card" id="library-panel" role="tabpanel"
            aria-labelledby=move || format!("tab-{}", Tab::ALL.iter().find(|t| t.0 == tab.get()).map_or("videos", |t| t.1))>
            {move || match tab.get() {
                Tab::Videos => view! { <Videos/> }.into_any(),
                Tab::Tests => view! { <Tests/> }.into_any(),
                Tab::Results => view! { <Results/> }.into_any(),
            }}
        </section>
    }
}

#[component]
fn Videos() -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let navigate = use_navigate();
    let search = RwSignal::new(String::new());
    let videos = LocalResource::new(move || {
        let code = search.get();
        async move { fetch_videos(&code, None).await }
    });
    let upload = {
        let navigate = navigate.clone();
        move |_| {
            selection.uploading.set(true);
            selection.panel_open.set(true);
            navigate("/", NavigateOptions::default());
        }
    };
    view! {
        <div class="library__bar">
            <div class="field">
                <label for="lib-code">"Participant"</label>
                <input id="lib-code" type="search" autocomplete="off" placeholder="Search by participant"
                    on:input=move |e| search.set(event_target_value(&e))/>
            </div>
            <button type="button" class="button" on:click=upload>"Upload a video"</button>
        </div>
        <Suspense fallback=|| view! { <p class="muted" role="status">"Loading videos…"</p> }>
            {move || videos.get().map(|result| match result {
                Err(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                Ok(list) if list.videos.is_empty() => view! {
                    <p class="muted">"No videos yet. Choose “Upload a video” to add one."</p>
                }.into_any(),
                Ok(list) => {
                    let navigate = navigate.clone();
                    view! {
                        <table class="data">
                            <caption class="visually-hidden">"Stored videos"</caption>
                            <thead><tr>
                                <th scope="col">"Participant"</th><th scope="col">"Visit"</th>
                                <th scope="col">"Hand"</th><th scope="col">"Length"</th>
                                <th scope="col">"Quality"</th><th scope="col"><span class="visually-hidden">"Action"</span></th>
                            </tr></thead>
                            <tbody>
                                {list.videos.into_iter().map(|video| {
                                    let navigate = navigate.clone();
                                    view! { <VideoRow video navigate/> }
                                }).collect_view()}
                            </tbody>
                        </table>
                    }.into_any()
                }
            })}
        </Suspense>
    }
}

#[component]
fn VideoRow(
    video: Video,
    navigate: impl Fn(&str, NavigateOptions) + Clone + 'static,
) -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let failing = video.gate.iter().any(|c| c.status == GateStatus::Fail);
    let (label, class) = if failing {
        ("Check", "badge badge--bad")
    } else {
        ("Pass", "badge badge--good")
    };
    let id = video.id.clone();
    let code = video.participant_code.clone();
    view! {
        <tr>
            <th scope="row">{video.participant_code.clone()}</th>
            <td>{video.visit_date.clone()}</td>
            <td>{hand_label(video.hand)}</td>
            <td>{clip_length(video.duration_seconds)}</td>
            <td><span class=class>{label}</span></td>
            <td>
                <button type="button" class="tool" aria-label=format!("Analyse {code}")
                    on:click=move |_| {
                        selection.video_id.set(Some(id.clone()));
                        selection.uploading.set(false);
                        selection.panel_open.set(true);
                        navigate("/", NavigateOptions::default());
                    }>"Analyse"</button>
            </td>
        </tr>
    }
}

#[component]
fn Tests() -> impl IntoView {
    let tests = LocalResource::new(fetch_tests);
    view! {
        <p class="muted">"Tests are plug-in folders on this computer. Adding one from this screen is planned."</p>
        <Suspense fallback=|| view! { <p class="muted" role="status">"Loading tests…"</p> }>
            {move || tests.get().map(|result| match result {
                Err(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                Ok(list) if list.tests.is_empty() => view! {
                    <p class="muted">"No tests are installed. Add a plug-in folder and restart the server."</p>
                }.into_any(),
                Ok(list) => view! {
                    <ul class="test-cards">
                        {list.tests.into_iter().map(|test| view! { <TestCard test/> }).collect_view()}
                    </ul>
                }.into_any(),
            })}
        </Suspense>
    }
}

#[component]
fn TestCard(test: TestSummary) -> impl IntoView {
    let selection = expect_context::<RunSelection>();
    let navigate = use_navigate();
    let runnable = test.status == TestStatus::Available;
    let (label, class) = match test.status {
        TestStatus::Available => ("Available", "badge badge--good"),
        TestStatus::Planned => ("Planned", "badge badge--neutral"),
        TestStatus::Broken => ("Error", "badge badge--bad"),
    };
    let id = test.id.clone();
    let detail_id = test.id.clone();
    let detail = LocalResource::new(move || {
        let id = detail_id.clone();
        async move { fetch_test(&id).await.ok() }
    });
    view! {
        <li class="card test-card">
            <h2 class="panel-title">{test.name.clone()}</h2>
            <p><span class=class>{label}</span>" "<span class="muted">{format!("{} · version {}", test.clinical_item, test.version)}</span></p>
            <p>{test.summary.clone()}</p>
            {test.error.clone().map(|e| view! { <p class="field-error" role="alert">{format!("Could not load: {e}")}</p> })}
            <details>
                <summary>"Measures and settings"</summary>
                {move || detail.get().flatten().map(|d| view! {
                    <ul class="metric-plain">
                        {d.metrics.iter().map(|m| view! {
                            <li><strong>{m.name.clone()}</strong>" "<span class="muted">{m.description.clone()}</span></li>
                        }).collect_view()}
                    </ul>
                    {(!d.parameters.is_empty()).then(|| view! {
                        <ul class="metric-plain">
                            {d.parameters.iter().map(|p| view! {
                                <li><strong>{p.name.clone()}</strong>" "<span class="muted">{format!("default {}. {}", p.default, p.description)}</span></li>
                            }).collect_view()}
                        </ul>
                    })}
                })}
            </details>
            {runnable.then(|| view! {
                <button type="button" class="tool" on:click=move |_| {
                    selection.test_id.set(Some(id.clone()));
                    selection.metrics_for.set(None);
                    selection.panel_open.set(true);
                    navigate("/", NavigateOptions::default());
                }>"Open in analysis"</button>
            })}
        </li>
    }
}

#[component]
fn Results() -> impl IntoView {
    let results = LocalResource::new(|| async {
        let list = fetch_results().await?;
        // Test names read better than ids; the list works without them.
        let names: HashMap<String, String> = fetch_tests()
            .await
            .map(|t| t.tests.into_iter().map(|t| (t.id, t.name)).collect())
            .unwrap_or_default();
        Ok::<_, String>((list, names))
    });
    let ticked = RwSignal::new(BTreeSet::<String>::new());
    view! {
        <Suspense fallback=|| view! { <p class="muted" role="status">"Loading results…"</p> }>
            {move || results.get().map(|result| match result {
                Err(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                Ok((list, _)) if list.results.is_empty() => view! {
                    <p class="muted">"No results yet. Analyse a video and its result is saved here."</p>
                }.into_any(),
                Ok((list, names)) => view! { <ResultTable rows=list.results names ticked/> }.into_any(),
            })}
        </Suspense>
    }
}

#[component]
fn ResultTable(
    rows: Vec<ResultSummary>,
    names: HashMap<String, String>,
    ticked: RwSignal<BTreeSet<String>>,
) -> impl IntoView {
    let compare_href = move || {
        let chosen = ticked.get();
        let mut keys = chosen.iter();
        match (keys.next(), keys.next(), keys.next()) {
            (Some(a), Some(b), None) => Some(format!("/compare?a={a}&b={b}")),
            _ => None,
        }
    };
    view! {
        <div class="library__bar">
            <p class="muted">"Tick two results of the same test to compare them."</p>
            {move || match compare_href() {
                Some(href) => view! { <A href attr:class="button">"Compare 2 selected"</A> }.into_any(),
                None => view! { <button type="button" class="button" disabled>"Compare 2 selected"</button> }.into_any(),
            }}
        </div>
        <table class="data">
            <caption class="visually-hidden">"Saved results"</caption>
            <thead><tr>
                <th scope="col"><span class="visually-hidden">"Select"</span></th>
                <th scope="col">"Participant"</th><th scope="col">"Visit"</th><th scope="col">"Hand"</th>
                <th scope="col">"Test"</th><th scope="col">"Created"</th>
                <th scope="col"><span class="visually-hidden">"Action"</span></th>
            </tr></thead>
            <tbody>
                {rows.into_iter().map(|row| {
                    let key = row.key.clone();
                    let checked_key = key.clone();
                    let open = format!("/?key={}", row.key);
                    let label = format!("Select {} {}", row.participant_code, row.visit_date);
                    view! {
                        <tr>
                            <td><input type="checkbox" aria-label=label
                                prop:checked=move || ticked.get().contains(&checked_key)
                                on:change=move |e| ticked.update(|set| crate::selection::toggle(set, &key, event_target_checked(&e)))/></td>
                            <th scope="row">{row.participant_code.clone()}</th>
                            <td>{row.visit_date.clone()}</td>
                            <td>{hand_label(row.hand)}</td>
                            <td>{names.get(&row.test_id).cloned().unwrap_or_else(|| row.test_id.replace('_', " "))}</td>
                            <td>{row.created_at.clone()}</td>
                            <td><A href=open attr:class="tool">"Open"</A></td>
                        </tr>
                    }
                }).collect_view()}
            </tbody>
        </table>
    }
}
