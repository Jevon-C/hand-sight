//! Root component and routes.

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::layout::Shell;
use crate::pages::{Analysis, Compare, Library, NotFound};
use crate::selection::RunSelection;

/// The whole app: a router wrapped in the shared page layout.
#[component]
pub fn App() -> impl IntoView {
    provide_context(RunSelection::new());
    view! {
        <Router>
            <Shell>
                <Routes fallback=NotFound>
                    <Route path=path!("/") view=Analysis/>
                    <Route path=path!("/results") view=Analysis/>
                    <Route path=path!("/library") view=Library/>
                    <Route path=path!("/compare") view=Compare/>
                </Routes>
            </Shell>
        </Router>
    }
}
