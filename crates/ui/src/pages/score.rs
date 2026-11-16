//! Clinician scores for one result: enter your own score and see everyone's.

use axonhand_api::{ScoreRequest, MAX_NOTE_LEN, MAX_RATER_LEN, MAX_SCORE};
use leptos::prelude::*;

use crate::client::{fetch_scores, save_score};
use crate::format::score_label;

/// Where saving stands.
#[derive(Clone, Debug, PartialEq)]
enum Progress {
    Idle,
    Working,
    Saved,
    Failed(String),
}

/// The scores of the result `key`, and a form to add or replace one.
#[component]
pub fn ScoreCard(key: String) -> impl IntoView {
    let rater = RwSignal::new(String::new());
    let score = RwSignal::new(None::<u8>);
    let note = RwSignal::new(String::new());
    let state = RwSignal::new(Progress::Idle);
    let attempted = RwSignal::new(false);
    let reload = RwSignal::new(0_u32);
    let result_key = key.clone();
    let scores = LocalResource::new(move || {
        reload.track();
        let key = key.clone();
        async move { fetch_scores(&key).await }
    });

    // The request as the server will check it, so the messages are the same. With no score
    // chosen yet, the other fields are still checked (as if for 0), so every problem shows at once.
    let request = move || ScoreRequest {
        rater: rater.get(),
        score: score.get().unwrap_or(0),
        note: note.get(),
    };
    let problem = move |field: &'static str| -> Option<String> {
        if !attempted.get() {
            return None;
        }
        if field == "score" {
            return score.get().is_none().then(|| "Choose a score.".to_owned());
        }
        request()
            .validated()
            .err()
            .filter(|e| e.field.as_deref() == Some(field))
            .map(|e| e.message)
    };

    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        attempted.set(true);
        if score.get_untracked().is_none() {
            return;
        }
        let Ok(valid) = request().validated() else {
            return;
        };
        if state.get_untracked() == Progress::Working {
            return;
        }
        state.set(Progress::Working);
        let key = result_key.clone();
        leptos::task::spawn_local(async move {
            match save_score(&key, &valid).await {
                Ok(_) => {
                    state.set(Progress::Saved);
                    attempted.set(false);
                    reload.update(|n| *n += 1);
                }
                Err(message) => state.set(Progress::Failed(message)),
            }
        });
    };

    view! {
        <section class="card" aria-labelledby="score-h">
            <h2 class="eyebrow" id="score-h">"Clinician scores"</h2>
            <p class="muted">
                "Record your own MDS-UPDRS score (0 normal to 4 severe) for what you see in this video, "
                "so the measures can be checked against clinical practice. Saving again with the same name replaces your score."
            </p>
            <Suspense fallback=|| view! { <p class="muted" role="status">"Loading the scores…"</p> }>
                {move || scores.get().map(|loaded| match loaded {
                    Err(message) => view! { <p class="field-error" role="alert">{message}</p> }.into_any(),
                    Ok(list) if list.scores.is_empty() => view! { <p class="muted">"No scores yet."</p> }.into_any(),
                    Ok(list) => view! {
                        <table class="measures" aria-label="Scores given">
                            <thead><tr>
                                <th scope="col">"Rater"</th><th scope="col">"Score"</th>
                                <th scope="col">"Note"</th><th scope="col">"Saved"</th>
                            </tr></thead>
                            <tbody>
                                {list.scores.into_iter().map(|s| view! {
                                    <tr>
                                        <th scope="row">{s.rater}</th>
                                        <td>{score_label(s.score)}</td>
                                        <td>{s.note}</td>
                                        <td class="muted">{s.recorded_at}</td>
                                    </tr>
                                }).collect_view()}
                            </tbody>
                        </table>
                    }.into_any(),
                })}
            </Suspense>
            <form on:submit=submit novalidate>
                <div class="fields">
                    <div class="field">
                        <label for="score-rater">"Your name or initials"</label>
                        <input id="score-rater" type="text" autocomplete="off" maxlength=MAX_RATER_LEN
                            prop:value=move || rater.get()
                            on:input=move |e| rater.set(event_target_value(&e))/>
                        <p class="field-error" role="alert">{move || problem("rater")}</p>
                    </div>
                    <div class="field">
                        <label for="score-value">"Score"</label>
                        <select id="score-value" on:change=move |e| score.set(event_target_value(&e).parse().ok())>
                            <option value="" selected=move || score.get().is_none()>"Choose a score"</option>
                            {(0..=MAX_SCORE).map(|n| view! {
                                <option value=n.to_string() selected=move || score.get() == Some(n)>{score_label(n)}</option>
                            }).collect_view()}
                        </select>
                        <p class="field-error" role="alert">{move || problem("score")}</p>
                    </div>
                    <div class="field">
                        <label for="score-note">"Note (optional)"</label>
                        <input id="score-note" type="text" autocomplete="off" maxlength=MAX_NOTE_LEN
                            prop:value=move || note.get()
                            on:input=move |e| note.set(event_target_value(&e))/>
                        <p class="field-error" role="alert">{move || problem("note")}</p>
                    </div>
                </div>
                <div class="actions">
                    <button type="submit" class="button" disabled=move || state.get() == Progress::Working>
                        "Save score"
                    </button>
                    <p class="muted" role="status">
                        {move || match state.get() {
                            Progress::Idle | Progress::Working => String::new(),
                            Progress::Saved => "Score saved.".to_owned(),
                            Progress::Failed(message) => message,
                        }}
                    </p>
                </div>
            </form>
        </section>
    }
}
