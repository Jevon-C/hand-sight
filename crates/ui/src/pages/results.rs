//! Results screen: one saved analysis with its numbers, plots and the facts needed to repeat it.

use std::collections::HashMap;

use axonhand_api::{
    AnalysisResult, EventData, LandmarkOverlay, QualityStatus, ResultSummary, SeriesData, Signals,
};
use leptos::prelude::*;
use leptos_router::components::A;
use wasm_bindgen::JsCast;

use crate::client::{fetch_landmarks, fetch_result, fetch_results, fetch_signals, fetch_test};
use crate::format::{hand_label, measure_number};
use crate::pages::score::ScoreCard;
use crate::pages::setup::SetupToggle;
use crate::player::{Playhead, VideoPlayer};
use crate::plot::{polyline, tick_label, ticks, value_at, Frame, Range};

/// Everything the screen shows. Only the result itself is required.
#[derive(Clone)]
pub(crate) struct Loaded {
    result: AnalysisResult,
    signals: Option<Signals>,
    overlay: Option<LandmarkOverlay>,
    /// Participant, visit and hand, when the result list still has this result.
    summary: Option<ResultSummary>,
    /// What each measure means, by measure id, when the test can be read.
    descriptions: HashMap<String, String>,
    /// The test's own order of its measures, so they read as the test lists them.
    order: Vec<String>,
}

pub(crate) async fn load(key: String) -> Result<Loaded, String> {
    let result = fetch_result(&key).await?;
    // The numbers are useful without the plots, so a missing signals file is not an error.
    let signals = fetch_signals(&key).await.ok();
    // Results saved before landmarks were kept, and stand-in results, have none.
    let overlay = fetch_landmarks(&key).await.ok();
    // Participant and hand, and the plain-language meaning of each measure, only add context.
    let summary = fetch_results()
        .await
        .ok()
        .and_then(|list| list.results.into_iter().find(|s| s.key == key));
    let (descriptions, order) = fetch_test(&result.test_id).await.map_or_else(
        |_| (HashMap::new(), Vec::new()),
        |t| {
            let order = t.metrics.iter().map(|m| m.id.clone()).collect();
            let described = t
                .metrics
                .into_iter()
                .map(|m| (m.id, m.description))
                .collect();
            (described, order)
        },
    );
    Ok(Loaded {
        result,
        signals,
        overlay,
        summary,
        descriptions,
        order,
    })
}

/// Print the page; the style sheet hides everything but the report.
pub(crate) fn print_page() {
    if let Some(window) = web_sys::window() {
        let _ = window.print();
    }
}

#[component]
pub(crate) fn ResultView(loaded: Loaded) -> impl IntoView {
    let Loaded {
        result,
        signals,
        overlay,
        summary,
        descriptions,
        order,
    } = loaded;
    let mut measures = result.measures.clone();
    // Stable, so measures the test does not list keep their order, after the listed ones.
    measures.sort_by_key(|m| {
        order
            .iter()
            .position(|id| *id == m.metric_id)
            .unwrap_or(usize::MAX)
    });
    provide_context(Playhead::new());
    let title = summary
        .as_ref()
        .map_or_else(|| "Analysis".to_owned(), |s| s.participant_code.clone());
    let hand = summary.as_ref().map(|s| hand_label(s.hand));
    let video_url = format!("/api/videos/{}/stream", result.video_id);
    let csv_url = format!("/api/results/{}/export.csv", result.key);
    let compare_url = format!("/compare?a={}", result.key);
    let illustrative = result.illustrative;
    let model = format!(
        "{}{}{}",
        result.model.name,
        if result.model.device.is_empty() {
            String::new()
        } else {
            format!(" on the {}", result.model.device)
        },
        if result.model.stand_in {
            " (stand-in for development)"
        } else {
            ""
        }
    );
    let settings = result
        .parameters
        .iter()
        .map(|(name, value)| format!("{name} = {value}"))
        .collect::<Vec<_>>()
        .join(", ");
    let timeline_signals = signals.clone();
    let timeline = move || match timeline_signals.clone() {
        Some(signals) => view! { <Timeline signals/> }.into_any(),
        None => ().into_any(),
    };
    view! {
        <header class="result-head">
            <h1 class="result-title">{title}</h1>
            <div class="result-bar">
                <p class="pill-group">
                    <span>{hand.unwrap_or("Hand")}</span>
                    <span class="pill-group__rule" aria-hidden="true"></span>
                    <span>{result.test_name.clone()}</span>
                </p>
                <div class="result-actions">
                    <a class="tool tool--filled" href=csv_url download>"Download as CSV"</a>
                    <button type="button" class="tool" on:click=move |_| print_page()>"Print this report"</button>
                    <A href=compare_url attr:class="tool">"Compare results"</A>
                    <SetupToggle/>
                </div>
            </div>
        </header>
        {illustrative.then(|| view! {
            <p class="banner banner--warn" role="note">
                <strong>"Illustrative. "</strong>
                "These numbers do not come from a validated pipeline. Do not interpret them."
            </p>
        })}

        <section class="card stage" aria-label="Video and signal">
            <VideoPlayer video_url overlay timeline/>
        </section>

        <div class="pair">
            <section class="card" aria-labelledby="measures-h">
                <h2 class="eyebrow" id="measures-h">"Measures"</h2>
                <div class="measure-frame">
                <ul class="measure-grid" aria-label="Measures">
                    {measures.iter().map(|m| {
                        let description = descriptions.get(&m.metric_id).cloned();
                        let number = measure_number(m.value).replace('-', "\u{2212}");
                        let measured = m.value.is_some_and(f64::is_finite);
                        view! {
                            <li class="measure">
                                <div class="measure__text">
                                    <p class="measure__name">{m.name.clone()}</p>
                                    {description.map(|d| view! { <p class="measure__desc">{d}</p> })}
                                    {m.reason.clone().map(|r| view! { <p class="measure__desc">{r}</p> })}
                                </div>
                                <p class="measure__value">
                                    <span class="measure__number" class:measure__number--none=!measured>{number}</span>
                                    <span class="measure__unit">{if measured { m.unit.clone() } else { String::new() }}</span>
                                </p>
                            </li>
                        }
                    }).collect_view()}
                </ul>
                </div>
            </section>
            <ScoreCard key=result.key.clone()/>
        </div>

        <section class="card" aria-labelledby="repeat-h">
            <h2 class="eyebrow" id="repeat-h">"How to repeat this result"</h2>
            <dl class="facts">
                <dt>"Test and version"</dt><dd>{format!("{} {}", result.test_name, result.test_version)}</dd>
                <dt>"Hand model"</dt><dd>{model}</dd>
                <dt>"Settings"</dt><dd>{settings}</dd>
                <dt>"Video"</dt><dd class="mono">{result.video_id.clone()}</dd>
                <dt>"Result key"</dt><dd class="mono">{result.key.clone()}</dd>
                <dt>"Created"</dt><dd>{result.created_at.clone()}</dd>
            </dl>
            {(!result.notes.is_empty()).then(|| view! {
                <ul class="notes">{result.notes.iter().map(|n| view! { <li>{n.clone()}</li> }).collect_view()}</ul>
            })}
            <h3 class="eyebrow eyebrow--sub" id="quality-h">"Quality checks"</h3>
            <ul class="gate" aria-labelledby="quality-h">
                {result.quality.iter().map(|q| {
                    let (label, class) = match q.status {
                        QualityStatus::Pass => ("Pass", "badge badge--good"),
                        QualityStatus::Flag => ("Take care", "badge badge--warn"),
                        QualityStatus::Fail => ("Do not rely", "badge badge--bad"),
                    };
                    view! { <li><span class=class>{label}</span>" "<strong>{q.name.clone()}</strong>" — "{q.detail.clone()}</li> }
                }).collect_view()}
            </ul>
        </section>

        {signals.map(|signals| view! {
            <details class="card charts">
                <summary>"All charts"</summary>
                <Plots signals/>
            </details>
        })}
    }
}

/// The main signal under the video, with each tap numbered and a line at the video's time.
/// Clicking it moves the video there; the controls below do the same from the keyboard.
#[component]
fn Timeline(signals: Signals) -> impl IntoView {
    let Some(series) = signals.series.first().cloned() else {
        return ().into_any();
    };
    let playhead = expect_context::<Playhead>();
    let events: Vec<EventData> = signals
        .events
        .iter()
        .filter(|e| e.unit == series.unit)
        .cloned()
        .collect();
    let frame = Frame {
        width: 1800.0,
        height: 200.0,
        left: 68.0,
        right: 24.0,
        top: 30.0,
        bottom: 50.0,
    };
    let end = series.start_seconds + series.step_seconds * series.values.len() as f64;
    let latest = events
        .iter()
        .flat_map(|e| e.times.iter().copied())
        .fold(0.0_f64, f64::max);
    let time = Range {
        low: series.start_seconds.min(0.0),
        high: end.max(latest).max(series.start_seconds + 1.0),
    };
    let value = Range::covering(
        series
            .values
            .iter()
            .chain(events.iter().flat_map(|e| e.values.iter())),
    );
    let points = polyline(
        &series.values,
        series.start_seconds,
        series.step_seconds,
        time,
        value,
        frame,
    );
    let taps: Vec<(usize, f64, f64, f64, f64)> = events
        .iter()
        .flat_map(|e| e.times.iter().zip(e.values.iter()))
        .enumerate()
        .map(|(i, (&t, &v))| {
            (
                i + 1,
                t,
                v,
                time.map(t, frame.x0(), frame.x1()),
                value.map(v, frame.y0(), frame.y1()),
            )
        })
        .collect();
    let description = format!(
        "{} over time, in {}, with {} numbered taps. Clicking moves the video to that time.",
        series.name,
        series.unit,
        taps.len()
    );
    let seek = move |event: leptos::ev::MouseEvent| {
        let Some(target) = event
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let rect = target.get_bounding_client_rect();
        if rect.width() < 1.0 {
            return;
        }
        let x = (f64::from(event.client_x()) - rect.left()) / rect.width() * frame.width;
        let fraction = (x - frame.x0()) / (frame.x1() - frame.x0());
        playhead.seek.set(Some(value_at(fraction, time).max(0.0)));
    };
    let table = RwSignal::new(false);
    let heading = format!("{} ({})", series.name, series.unit);
    let rows = taps.clone();
    view! {
        <div class="timeline">
            <div class="timeline__head">
                <h2 class="eyebrow" id="timeline-h">{heading}</h2>
                <button
                    type="button"
                    class="tool tool--small"
                    aria-pressed=move || table.get().to_string()
                    on:click=move |_| table.update(|t| *t = !*t)
                >
                    {move || if table.get() { "Show as chart" } else { "Show as table" }}
                </button>
            </div>
            <div class="plot timeline__scroll" hidden=move || table.get() tabindex="0" role="group" aria-label="Signal chart">
                <svg
                    viewBox=format!("0 0 {} {}", frame.width, frame.height)
                    role="img"
                    aria-labelledby="timeline-h"
                    aria-description=description
                    class="timeline__svg"
                    on:click=seek
                >
                    {axis_labels(frame, time, value)}
                    <polyline points=points class="plot__line" fill="none"></polyline>
                    {taps.into_iter().map(|(n, _, _, x, y)| view! {
                        <g>
                            <circle cx=x cy=y r="5.5" class="plot__dot"></circle>
                            <text x=x y=y - 12.0 text-anchor="middle" class="plot__tapno">{n}</text>
                        </g>
                    }).collect_view()}
                    <line
                        class="plot__playhead"
                        y1=frame.y1()
                        y2=frame.y0()
                        x1=move || time.map(playhead.time.get(), frame.x0(), frame.x1()).clamp(frame.x0(), frame.x1())
                        x2=move || time.map(playhead.time.get(), frame.x0(), frame.x1()).clamp(frame.x0(), frame.x1())
                    ></line>
                </svg>
            </div>
            <table class="measures" hidden=move || !table.get() aria-label="Marked taps">
                <thead><tr><th scope="col">"Tap"</th><th scope="col">"Time (s)"</th><th scope="col">{format!("Value ({})", series.unit)}</th></tr></thead>
                <tbody>
                    {rows.into_iter().map(|(n, t, v, _, _)| view! {
                        <tr><th scope="row">{n}</th><td>{tick_label(t)}</td><td>{tick_label(v)}</td></tr>
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
    .into_any()
}

#[component]
fn Plots(signals: Signals) -> impl IntoView {
    let events = signals.events.clone();
    view! {
        {signals.series.into_iter().map(|series| {
            let marks: Vec<EventData> = events.iter().filter(|e| e.unit == series.unit).cloned().collect();
            view! { <SeriesPlot series events=marks/> }
        }).collect_view()}
        {events.into_iter().map(|event| view! { <EventBars event/> }).collect_view()}
    }
}

fn axis_labels(frame: Frame, time: Range, value: Range) -> impl IntoView {
    let value_ticks = ticks(value, 4);
    let time_ticks = ticks(time, 6);
    // Tick numbers sit just under the axis; the axis title sits at the very bottom, inside the frame.
    let number_y = frame.y0() + frame.bottom * 0.5;
    let title_y = frame.height - 6.0;
    let middle = (frame.x0() + frame.x1()) / 2.0;
    view! {
        <g class="plot__axis">
            {value_ticks.into_iter().map(|v| {
                let y = value.map(v, frame.y0(), frame.y1());
                view! {
                    <line x1=frame.x0() x2=frame.x1() y1=y y2=y class="plot__grid"></line>
                    <text x=frame.x0() - 12.0 y=y dominant-baseline="middle" text-anchor="end">{tick_label(v)}</text>
                }
            }).collect_view()}
            {time_ticks.into_iter().map(|t| {
                let x = time.map(t, frame.x0(), frame.x1());
                view! {
                    <line x1=x x2=x y1=frame.y0() y2=frame.y0() + 6.0 class="plot__tick"></line>
                    <text x=x y=number_y text-anchor="middle">{tick_label(t)}</text>
                }
            }).collect_view()}
            <text x=middle y=title_y text-anchor="middle" class="plot__axis-title">"Time (seconds)"</text>
        </g>
    }
}

/// A signal over time, with a dot where each marked event happened.
#[component]
fn SeriesPlot(series: SeriesData, events: Vec<EventData>) -> impl IntoView {
    let frame = Frame::STANDARD;
    let end = series.start_seconds + series.step_seconds * series.values.len() as f64;
    let latest_event = events
        .iter()
        .flat_map(|e| e.times.iter().copied())
        .fold(0.0_f64, f64::max);
    let time = Range {
        low: series.start_seconds.min(0.0),
        high: end.max(latest_event).max(series.start_seconds + 1.0),
    };
    let value = Range::covering(
        series
            .values
            .iter()
            .chain(events.iter().flat_map(|e| e.values.iter())),
    );
    let points = polyline(
        &series.values,
        series.start_seconds,
        series.step_seconds,
        time,
        value,
        frame,
    );
    let dots: Vec<(f64, f64)> = events
        .iter()
        .flat_map(|e| e.times.iter().zip(e.values.iter()))
        .map(|(&t, &v)| {
            (
                time.map(t, frame.x0(), frame.x1()),
                value.map(v, frame.y0(), frame.y1()),
            )
        })
        .collect();
    let description = format!(
        "{} over time, in {}. {} marked taps.",
        series.name,
        series.unit,
        dots.len()
    );
    view! {
        <section class="card plot" aria-labelledby=format!("plot-{}", series.id)>
            <h2 class="section-title" id=format!("plot-{}", series.id)>{format!("{} ({})", series.name, series.unit)}</h2>
            <svg viewBox=format!("0 0 {} {}", frame.width, frame.height) role="img" aria-label=description>
                {axis_labels(frame, time, value)}
                <polyline points=points class="plot__line" fill="none"></polyline>
                {dots.into_iter().map(|(x, y)| view! { <circle cx=x cy=y r="5.5" class="plot__dot"></circle> }).collect_view()}
            </svg>
        </section>
    }
}

/// One bar for each marked event, in order, so a shrinking or uneven series is easy to see.
#[component]
fn EventBars(event: EventData) -> impl IntoView {
    let frame = Frame::STANDARD;
    let count = event.values.len();
    let value = Range::covering(&event.values);
    let slot = (frame.x1() - frame.x0()) / count.max(1) as f64;
    let bars: Vec<(f64, f64, f64)> = event
        .values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let top = value.map(v, frame.y0(), frame.y1());
            (frame.x0() + slot * i as f64 + slot * 0.15, slot * 0.7, top)
        })
        .collect();
    let value_ticks = ticks(value, 4);
    let description = format!("{}: one bar per event, {count} in all.", event.name);
    view! {
        <section class="card plot" aria-labelledby=format!("bars-{}", event.id)>
            <h2 class="section-title" id=format!("bars-{}", event.id)>{format!("{} (one bar each, {})", event.name, event.unit)}</h2>
            <svg viewBox=format!("0 0 {} {}", frame.width, frame.height) role="img" aria-label=description>
                <g class="plot__axis">
                    {value_ticks.into_iter().map(|v| {
                        let y = value.map(v, frame.y0(), frame.y1());
                        view! {
                            <line x1=frame.x0() x2=frame.x1() y1=y y2=y class="plot__grid"></line>
                            <text x=frame.x0() - 6.0 y=y + 4.0 text-anchor="end">{tick_label(v)}</text>
                        }
                    }).collect_view()}
                </g>
                {bars.into_iter().map(|(x, w, top)| view! {
                    <rect x=x y=top width=w height=(frame.y0() - top).max(0.0) class="plot__bar"></rect>
                }).collect_view()}
            </svg>
        </section>
    }
}
