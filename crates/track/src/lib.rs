//! The Hand Sight command-line hand tracker.
//!
//! It reads the raw frames that `python -m axonhand.tools.decode_frames` writes, finds the hand
//! in every frame with the Rust pipeline of `axonhand-hand` (on the NPU when there is one), and
//! writes the files the rest of Hand Sight already reads: the landmark track as `.npz` and the
//! landmarks for drawing over the video as `.json`. The server runs the two programs as a pipe.

#![deny(missing_docs)]

pub mod cli;
pub mod npz;
pub mod overlay;
pub mod stream;
pub mod track;

pub use stream::{FrameStream, Header, StreamError};
pub use track::{track_stream, Points, Track, TrackError};
