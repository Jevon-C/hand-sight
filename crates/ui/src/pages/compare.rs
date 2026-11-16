//! Compare screen: two saved results of the same test, side by side.

use std::collections::HashMap;

use axonhand_api::{AnalysisResult, ClinicianScore, ResultSummary};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::client::{fetch_landmarks, fetch_result, fetch_results, fetch_scores, fetch_tests};
use crate::compare::{compare, partners, result_label, score_rows, Comparison};
use crate::format::hand_label;
use crate::landmark_diff::{describe, landmark_difference};
use crate::pages::results::print_page;

/// One result with its clinician scores and, when the list still has it, who and when.
#[derive(Clone)]
struct Side1 {
    result: AnalysisResult,
    scores: Vec<ClinicianScore>,
    summary: Option<ResultSummary>,
}

/// The two results being compared.
type Pair = (Side1, Side1);

async fn load_one(key: &str, listed: &[ResultSummary]) -> Result<Side1, String> {
    // A failed request for the scores is an error, not "no scores": an empty list would be
    // read as nobody having scored the result.
    Ok(Side1 {
        result: fetch_result(key).await?,
        scores: fetch_scores(key).await?.scores,
        summary: listed.iter().find(|s| s.key == key).cloned(),
    })
}

async fn load_pair(first: String, second: String) -> Result<Pair, String> {
    // Who and when only add context, so a failed list is not an error.
    let listed = fetch_results().await.map(|l| l.results).unwrap_or_default();
    Ok((
        load_one(&first, &listed).await?,
        load_one(&second, &listed).await?,
    ))
}

/// Two results, chosen by `a` and `b` in the address. With one or none chosen, a picker is shown.
#[component]
pub fn Compare() -> impl IntoView {
    let query = use_query_map();
    let chosen = move || {
        let map = query.read();
        let value = |name: &str| map.get(name).filter(|v| !v.is_empty());
        (value("a"), value("b"))
    };
    view! {
        {move || match chosen() {
            (Some(a), Some(b)) => view! { <Side data=LocalResource::new(move || load_pair(a.clone(), b.clone()))/> }.into_any(),
            (a, _) => view! {
                <h1 class="page-title">"Comparison"</h1>
                <Picker first=a.unwrap_or_default()/>
            }.into_any(),
        }}
    }
}

/// Choose the two results to compare.
#[component]
fn Picker(first: String) -> impl IntoView {
    let list = LocalResource::new(|| async {
        let results = fetch_results().await?;
        // Test names make the list readable; the list is still usable without them.
        let names: HashMap<String, String> = fetch_tests()
            .await
            .map(|t| t.tests.into_iter().map(|t| (t.id, t.name)).collect())
            .unwrap_or_default();
        Ok::<_, String>((results.results, names))
    });
    let a = RwSignal::new(first);
    let b = RwSignal::new(String::new());
    view! {
        <p class="lede">"Choose two results of the same test, for example the same hand at two visits."</p>
        <Suspense fallback=|| view! { <p class="muted" role="status">"Loading the saved results…"</p> }>
            {move || list.get().map(|loaded| match loaded {
                Err(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                Ok((results, _)) if results.len() < 2 => view! {
                    <p class="muted">
                        "At least two saved results are needed. Analyse videos in the "
                        <A href="/">"Analysis page"</A>
                        " first."
                    </p>
                }.into_any(),
                Ok((results, names)) => view! { <PickerForm results names a b/> }.into_any(),
            })}
        </Suspense>
    }
}

fn option_view(summary: &ResultSummary, names: &HashMap<String, String>) -> (String, String) {
    let name = names
        .get(&summary.test_id)
        .map_or(summary.test_id.as_str(), String::as_str);
    (summary.key.clone(), result_label(summary, name))
}

#[component]
fn PickerForm(
    results: Vec<ResultSummary>,
    names: HashMap<String, String>,
    a: RwSignal<String>,
    b: RwSignal<String>,
) -> impl IntoView {
    let all = StoredValue::new(results);
    let names = StoredValue::new(names);
    let first_options = move || {
        all.with_value(|all| {
            names.with_value(|names| {
                all.iter()
                    .map(|r| option_view(r, names))
                    .collect::<Vec<_>>()
            })
        })
    };
    let second_options = move || {
        all.with_value(|all| {
            names.with_value(|names| {
                partners(all, &a.get())
                    .into_iter()
                    .map(|r| option_view(r, names))
                    .collect::<Vec<_>>()
            })
        })
    };
    let ready = move || !a.get().is_empty() && !b.get().is_empty();
    view! {
        <section class="card" aria-labelledby="pick-h">
            <h2 class="eyebrow" id="pick-h">"Results to compare"</h2>
            <div class="fields">
                <div class="field">
                    <label for="pick-a">"First result"</label>
                    <select id="pick-a" on:change=move |ev| { a.set(event_target_value(&ev)); b.set(String::new()); }>
                        <option value="" selected=move || a.get().is_empty()>"Choose a result"</option>
                        {move || first_options().into_iter().map(|(key, label)| {
                            let k = key.clone();
                            view! { <option value=key selected=move || a.get() == k>{label}</option> }
                        }).collect_view()}
                    </select>
                </div>
                <div class="field">
                    <label for="pick-b">"Second result"</label>
                    <select id="pick-b" disabled=move || a.get().is_empty()
                        on:change=move |ev| b.set(event_target_value(&ev))>
                        <option value="" selected=move || b.get().is_empty()>"Choose a result"</option>
                        {move || second_options().into_iter().map(|(key, label)| {
                            let k = key.clone();
                            view! { <option value=key selected=move || b.get() == k>{label}</option> }
                        }).collect_view()}
                    </select>
                </div>
            </div>
            {move || (!a.get().is_empty() && second_options().is_empty()).then(|| view! {
                <p class="muted">"No other saved result of this test yet."</p>
            })}
            <p>
                {move || if ready() {
                    view! { <A href=format!("/compare?a={}&b={}", a.get(), b.get()) attr:class="button">"Compare"</A> }.into_any()
                } else {
                    view! { <button class="button" type="button" disabled>"Compare"</button> }.into_any()
                }}
            </p>
        </section>
    }
}

/// The two results with their differences.
#[component]
fn Side(data: LocalResource<Result<Pair, String>>) -> impl IntoView {
    view! {
        <Suspense fallback=|| view! { <h1 class="page-title">"Comparison"</h1><p class="muted" role="status">"Loading the results…"</p> }>
            {move || data.get().map(|loaded| match loaded {
                Err(message) => view! {
                    <h1 class="page-title">"Comparison"</h1>
                    <p class="field-error" role="alert">{message}</p>
                    <p><A href="/compare" attr:class="button button--quiet">"Choose different results"</A></p>
                }.into_any(),
                Ok((first, second)) => match compare(&first.result, &second.result) {
                    Err(message) => view! {
                        <h1 class="page-title">"Comparison"</h1>
                        <p class="field-error" role="alert">{message}</p>
                        <p><A href="/compare" attr:class="button button--quiet">"Choose different results"</A></p>
                    }.into_any(),
                    Ok(comparison) => view! { <ComparisonView first second comparison/> }.into_any(),
                },
            })}
        </Suspense>
    }
}

/// Which way a change goes, from the text of the change: arrows show direction only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Direction {
    Up,
    Down,
    Same,
}

fn direction(change: &str) -> Direction {
    let text = change.trim();
    if text.starts_with('+') && text.chars().any(|c| c.is_ascii_digit() && c != '0') {
        Direction::Up
    } else if (text.starts_with('-') || text.starts_with('\u{2212}'))
        && text.chars().any(|c| c.is_ascii_digit() && c != '0')
    {
        Direction::Down
    } else {
        Direction::Same
    }
}

fn chip(change: &str) -> impl IntoView {
    let (arrow, class) = match direction(change) {
        Direction::Up => ("↑", "chip chip--up"),
        Direction::Down => ("↓", "chip chip--down"),
        Direction::Same => ("=", "chip"),
    };
    view! { <span class=class><span aria-hidden="true">{arrow}</span>{change.replace('-', "\u{2212}")}</span> }
}

fn result_card(tag: &'static str, side: &Side1) -> impl IntoView {
    let key = side.result.key.clone();
    let video_url = format!("/api/videos/{}/stream", side.result.video_id);
    let open = format!("/?key={key}");
    let who = side
        .summary
        .as_ref()
        .map_or_else(|| "Result".to_owned(), |s| s.participant_code.clone());
    let visit = side
        .summary
        .as_ref()
        .map(|s| format!("visit {}", s.visit_date));
    let hand = side.summary.as_ref().map(|s| hand_label(s.hand));
    let meta = format!(
        "{}{} {} · created {}",
        hand.map(|h| format!("{h} · ")).unwrap_or_default(),
        side.result.test_name,
        side.result.test_version,
        side.result.created_at
    );
    view! {
        <section class="card cmp-card" aria-label=tag>
            <div class="cmp-card__head">
                <h2 class="eyebrow">{tag}</h2>
                <A href=open>"Open this result"</A>
            </div>
            <video class="cmp-video" controls preload="metadata" muted src=video_url
                aria-label=format!("Video of the {} result", tag.to_lowercase())></video>
            <p class="cmp-card__who"><strong>{who}</strong>{visit.map(|v| view! { <span class="muted">{format!(" {v}")}</span> })}</p>
            <p class="muted">{meta}</p>
            <p class="mono muted cmp-card__key">{key}</p>
        </section>
    }
}

#[component]
fn ComparisonView(first: Side1, second: Side1, comparison: Comparison) -> impl IntoView {
    let scores = score_rows(&first.scores, &second.scores);
    let illustrative = first.result.illustrative || second.result.illustrative;
    let heading = format!(
        "{} · version {}",
        first.result.test_name, first.result.test_version
    );
    let open_first = format!("/?key={}", first.result.key);
    let open_second = format!("/?key={}", second.result.key);
    let swap = format!("/compare?a={}&b={}", second.result.key, first.result.key);
    let people = {
        let mut names: Vec<String> = [&first, &second]
            .iter()
            .filter_map(|s| s.summary.as_ref().map(|x| x.participant_code.clone()))
            .collect();
        names.dedup();
        if names.is_empty() {
            "Results".to_owned()
        } else {
            names.join(" · ")
        }
    };
    let hand = first.summary.as_ref().map(|s| hand_label(s.hand));
    let cards = view! {
        <div class="cmp-pair">
            {result_card("First result", &first)}
            {result_card("Second result", &second)}
        </div>
    };
    let same_video = first.result.video_id == second.result.video_id;
    let (first_key, second_key) = (first.result.key.clone(), second.result.key.clone());
    view! {
        <header class="result-head">
            <h1 class="result-title">"Comparison"</h1>
            <div class="result-bar">
                <p class="pill-group">
                    <span>{people}</span>
                    {hand.map(|h| view! { <span class="pill-group__rule" aria-hidden="true"></span><span>{h}</span> })}
                    <span class="pill-group__rule" aria-hidden="true"></span>
                    <span>{first.result.test_name.clone()}</span>
                </p>
                <div class="result-actions">
                    <A href="/compare" attr:class="tool">"Choose different results"</A>
                    <A href=swap attr:class="tool">"Swap order"</A>
                    <button type="button" class="tool tool--filled" on:click=move |_| print_page()>"Print this report"</button>
                </div>
            </div>
        </header>
        {illustrative.then(|| view! {
            <p class="banner banner--warn" role="note">
                <strong>"Illustrative. "</strong>
                "At least one of these results does not come from a validated pipeline. Do not interpret the differences."
            </p>
        })}
        {(!comparison.cautions.is_empty()).then(|| view! {
            <ul class="notes" aria-label="Read with care">
                {comparison.cautions.iter().map(|c| view! { <li>{c.clone()}</li> }).collect_view()}
            </ul>
        })}
        {cards}
        <section class="card cmp-section" aria-labelledby="diff-h">
            <h2 class="eyebrow" id="diff-h">{heading}</h2>
            <table class="measures" aria-label="Measures compared">
                <thead>
                    <tr>
                        <th scope="col">"Measure"</th>
                        <th scope="col"><A href=open_first>"First result"</A></th>
                        <th scope="col"><A href=open_second>"Second result"</A></th>
                        <th scope="col">"Change"</th>
                    </tr>
                </thead>
                <tbody>
                    {comparison.rows.iter().map(|row| view! {
                        <tr>
                            <th scope="row">{row.name.clone()}</th>
                            <td>{row.first.clone()}</td>
                            <td>{row.second.clone()}</td>
                            <td>{chip(&row.change)}</td>
                        </tr>
                    }).collect_view()}
                </tbody>
            </table>
            <p class="muted">"The change is the second result minus the first. Arrows show direction only; they do not say better or worse. These are research measures, not clinical scores."</p>
        </section>
        <div class="cmp-lower">
            <section class="card" aria-labelledby="scores-h">
                <h2 class="eyebrow" id="scores-h">"Clinician scores"</h2>
                {if scores.is_empty() {
                    view! { <p class="muted">"No clinician has scored either result yet. Scores are entered on each result's page."</p> }.into_any()
                } else {
                    view! {
                        <table class="measures" aria-label="Clinician scores compared">
                            <thead>
                                <tr>
                                    <th scope="col">"Rater"</th>
                                    <th scope="col">"First result"</th>
                                    <th scope="col">"Second result"</th>
                                    <th scope="col">"Change"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {scores.iter().map(|row| view! {
                                    <tr>
                                        <th scope="row">{row.rater.clone()}</th>
                                        <td>{row.first.clone()}</td>
                                        <td>{row.second.clone()}</td>
                                        <td>{chip(&row.change)}</td>
                                    </tr>
                                }).collect_view()}
                            </tbody>
                        </table>
                        <p class="muted">"A dash means that rater did not score that result. A change is shown only when the same rater scored both."</p>
                    }.into_any()
                }}
            </section>
            {same_video.then(|| view! { <LandmarkSection first_key second_key/> })}
        </div>
    }
}

/// How far apart the two results put the hand landmarks of the same video.
#[component]
fn LandmarkSection(first_key: String, second_key: String) -> impl IntoView {
    let data = LocalResource::new(move || {
        let (a, b) = (first_key.clone(), second_key.clone());
        async move {
            let first = fetch_landmarks(&a).await?;
            let second = fetch_landmarks(&b).await?;
            landmark_difference(&first, &second)
        }
    });
    view! {
        <section class="card" aria-labelledby="landmarks-h">
            <h2 class="eyebrow" id="landmarks-h">"Hand positions compared"</h2>
            <Suspense fallback=|| view! { <p class="muted" role="status">"Comparing the hand positions…"</p> }>
                {move || data.get().map(|loaded| match loaded {
                    Err(message) => view! { <p class="muted">{message}</p> }.into_any(),
                    Ok(difference) => view! {
                        <ul class="notes" aria-label="Hand positions compared">
                            {describe(&difference).into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}
                        </ul>
                        <p class="muted">"Distances are in palm lengths (wrist to the base of the middle finger), so they do not depend on the size of the hand in the picture. Saved positions are rounded to a tenth of a pixel."</p>
                    }.into_any(),
                })}
            </Suspense>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::{direction, Direction};

    #[test]
    fn arrows_follow_the_sign_and_ignore_zero() {
        assert_eq!(direction("+2"), Direction::Up);
        assert_eq!(direction("-1 taps/s (-33%)"), Direction::Down);
        assert_eq!(direction("\u{2212}1"), Direction::Down);
        assert_eq!(direction("0 (+0%)"), Direction::Same);
        assert_eq!(direction("+0 (+0%)"), Direction::Same);
        assert_eq!(direction("-"), Direction::Same);
    }
}
