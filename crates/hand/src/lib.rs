//! Hand tracking for Hand Sight: the two-stage palm and landmark pipeline in Rust.
//!
//! It mirrors the Python pipeline in `axonhand/hand/`, and `tests/parity.rs` checks that the two
//! agree. Frames come in as [`RgbImage`]; decoding video is not this crate's job.

#![deny(missing_docs)]

mod error;
mod image;
pub mod rois;
mod runtime;
mod tracker;

pub use error::{HandError, Result};
pub use image::RgbImage;
pub use runtime::{Device, Model, Network, Placement, Runtime};
pub use tracker::{HandFrame, HandTracker};
