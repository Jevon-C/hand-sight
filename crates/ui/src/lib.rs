//! Hand Sight browser app: layout, routes and the four screens.
//!
//! Screens call the server through the types in `axonhand-api`. Visual design lives in
//! `style/tokens.css` and `style/app.css`, which `index.html` loads.

#![deny(missing_docs)]

mod app;
mod client;
mod compare;
mod format;
mod landmark_diff;
mod layout;
mod overlay;
mod pages;
mod player;
mod plot;
mod selection;

pub use app::App;
