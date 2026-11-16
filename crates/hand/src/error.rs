//! The one error type of the hand crate.

use std::path::PathBuf;

/// Everything that can go wrong while loading a model or finding a hand.
///
/// Messages are for developers; the server turns them into plain wording for people.
#[derive(Debug, thiserror::Error)]
pub enum HandError {
    /// The ONNX Runtime library could not be loaded.
    #[error("the ONNX Runtime library could not be loaded: {0}")]
    Library(String),
    /// A model file does not exist.
    #[error("model file not found: {}", .0.display())]
    ModelMissing(PathBuf),
    /// A model file exists but could not be opened.
    #[error("cannot load {name}: {message}")]
    Load {
        /// File name of the model.
        name: String,
        /// What the runtime said.
        message: String,
    },
    /// Running a model failed.
    #[error("model run failed: {0}")]
    Run(String),
    /// A model returned something other than what the pipeline expects.
    #[error("unexpected model output: {0}")]
    Output(String),
    /// An image has an impossible size or the wrong number of bytes.
    #[error("invalid image: {0}")]
    Image(String),
}

/// Shorthand for results in this crate.
pub type Result<T> = std::result::Result<T, HandError>;
