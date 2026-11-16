//! Error type shared by the geometry core.

/// Errors from geometric operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeomError {
    /// Point is at or behind the camera plane (z <= 0) or not finite.
    BehindCamera,
    /// Depth must be strictly positive and finite.
    InvalidDepth,
    /// An iterative solver did not converge.
    NoConvergence,
    /// An operation needed at least one input item.
    EmptyInput,
    /// Paired inputs had different lengths.
    LengthMismatch,
    /// Fewer points were given than the operation needs.
    TooFewPoints,
    /// The points do not determine a unique answer (for example, all on one line).
    Degenerate,
    /// A parameter was out of its valid range (for example, a negative threshold).
    InvalidParameter,
}

impl core::fmt::Display for GeomError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Self::BehindCamera => "point is behind the camera or not finite",
            Self::InvalidDepth => "depth must be positive and finite",
            Self::NoConvergence => "solver did not converge",
            Self::EmptyInput => "input was empty",
            Self::LengthMismatch => "input lengths differ",
            Self::TooFewPoints => "too few points for this operation",
            Self::Degenerate => "points do not determine a unique answer",
            Self::InvalidParameter => "a parameter was out of range",
        };
        f.write_str(text)
    }
}

impl std::error::Error for GeomError {}
