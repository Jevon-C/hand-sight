//! The app's screens.

use leptos::prelude::*;

mod analysis;
mod compare;
mod library;
mod results;
mod run;
mod score;
mod setup;

pub use analysis::Analysis;
pub use compare::Compare;
pub use library::Library;

/// Shown for any address the app does not know.
#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <h1 class="page-title">"Page not found"</h1>
        <p class="muted">"Use the menu above to go to a screen."</p>
    }
}
