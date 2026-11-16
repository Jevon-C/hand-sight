//! Hand Sight web server library: settings, error mapping and the HTTP application.
//!
//! The binary in `main.rs` only wires these together, so everything that matters is testable
//! without starting a real server.

#![deny(missing_docs)]

pub mod analysis_api;
pub mod app;
pub mod clock;
pub mod config;
pub mod csv_export;
pub mod error;
pub mod gate;
pub mod jobs;
pub mod jobstore;
pub mod manifest;
pub mod media;
pub mod probe;
pub mod protocol;
pub mod registry;
pub mod result_key;
pub mod results;
pub mod runner;
pub mod rust_hand;
pub mod scorestore;
pub mod store;
pub mod tests_api;
pub mod videos;
