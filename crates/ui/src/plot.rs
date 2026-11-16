//! Geometry for the result plots, with no browser dependency so it can be unit-tested natively.
//!
//! A plot is drawn as SVG. These functions turn data values into SVG coordinates; the screen
//! only places the shapes they describe.

/// Where the data area sits inside the SVG, in SVG units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Whole width of the SVG.
    pub width: f64,
    /// Whole height of the SVG.
    pub height: f64,
    /// Space left of the data area, for the value labels.
    pub left: f64,
    /// Space right of the data area.
    pub right: f64,
    /// Space above the data area.
    pub top: f64,
    /// Space below the data area, for the time labels.
    pub bottom: f64,
}

impl Frame {
    /// The frame used by every plot on the results screen.
    pub const STANDARD: Self = Self {
        width: 640.0,
        height: 220.0,
        left: 48.0,
        right: 12.0,
        top: 12.0,
        bottom: 32.0,
    };

    /// X position of the data area's left edge.
    #[must_use]
    pub fn x0(&self) -> f64 {
        self.left
    }

    /// X position of the data area's right edge.
    #[must_use]
    pub fn x1(&self) -> f64 {
        self.width - self.right
    }

    /// Y position of the bottom of the data area (the smallest value).
    #[must_use]
    pub fn y0(&self) -> f64 {
        self.height - self.bottom
    }

    /// Y position of the top of the data area (the largest value).
    #[must_use]
    pub fn y1(&self) -> f64 {
        self.top
    }
}

/// The data value at `fraction` (0 to 1, clamped) of the way along `range`; used to turn a click
/// on a time axis into a time.
#[must_use]
pub fn value_at(fraction: f64, range: Range) -> f64 {
    let f = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    range.low + (range.high - range.low) * f
}

/// A closed range of data values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    /// Smallest value shown.
    pub low: f64,
    /// Largest value shown.
    pub high: f64,
}

impl Range {
    /// The range covering every finite value, with a little room above, never starting above
    /// zero and never empty. Returns `0..1` when there is nothing to show.
    #[must_use]
    pub fn covering<'a>(values: impl IntoIterator<Item = &'a f64>) -> Self {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for &v in values {
            if v.is_finite() {
                low = low.min(v);
                high = high.max(v);
            }
        }
        if !low.is_finite() || !high.is_finite() {
            return Self {
                low: 0.0,
                high: 1.0,
            };
        }
        low = low.min(0.0);
        if (high - low).abs() < f64::EPSILON {
            high = low + 1.0;
        }
        Self {
            low,
            high: high + (high - low) * 0.05,
        }
    }

    /// Map `value` onto `out_low..out_high`. Values outside the range are clamped to it.
    #[must_use]
    pub fn map(&self, value: f64, out_low: f64, out_high: f64) -> f64 {
        let t = ((value - self.low) / (self.high - self.low)).clamp(0.0, 1.0);
        out_low + t * (out_high - out_low)
    }
}

/// The `points` attribute of an SVG polyline for a regularly sampled signal. Values that are
/// not finite break nothing: they are skipped.
#[must_use]
pub fn polyline(
    values: &[f64],
    start_seconds: f64,
    step_seconds: f64,
    time: Range,
    value: Range,
    frame: Frame,
) -> String {
    values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .map(|(i, &v)| {
            let t = start_seconds + step_seconds * i as f64;
            format!(
                "{:.1},{:.1}",
                time.map(t, frame.x0(), frame.x1()),
                value.map(v, frame.y0(), frame.y1())
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Round tick positions inside `range`, about `target` of them, at 1, 2 or 5 times a power of
/// ten. Always includes values from the range's start.
#[must_use]
pub fn ticks(range: Range, target: usize) -> Vec<f64> {
    let span = range.high - range.low;
    if span <= 0.0 || target == 0 {
        return Vec::new();
    }
    let rough = span / target as f64;
    let magnitude = 10_f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * magnitude)
        .find(|s| *s >= rough)
        .unwrap_or(rough);
    let first = (range.low / step).ceil();
    let mut out = Vec::new();
    let mut k = first;
    while k * step <= range.high + step * 1e-9 {
        out.push(k * step);
        k += 1.0;
    }
    out
}

/// A tick value as short text without trailing zeros: `0`, `0.5`, `12`.
#[must_use]
pub fn tick_label(value: f64) -> String {
    let text = format!("{value:.2}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn value_at_walks_along_the_range_and_clamps() {
        let r = Range {
            low: 2.0,
            high: 6.0,
        };
        assert!((value_at(0.0, r) - 2.0).abs() < 1e-12);
        assert!((value_at(0.5, r) - 4.0).abs() < 1e-12);
        assert!((value_at(2.0, r) - 6.0).abs() < 1e-12);
        assert!((value_at(f64::NAN, r) - 2.0).abs() < 1e-12);
    }

    use super::*;

    const FRAME: Frame = Frame::STANDARD;

    #[test]
    fn a_range_covers_the_data_and_starts_at_zero() {
        let r = Range::covering(&[0.2, 0.8, 0.5]);
        assert!((r.low - 0.0).abs() < 1e-12);
        assert!(r.high > 0.8 && r.high < 0.9);
    }

    #[test]
    fn a_range_keeps_negative_values_and_ignores_non_finite_ones() {
        let r = Range::covering(&[-1.0, f64::NAN, 2.0, f64::INFINITY]);
        assert!((r.low + 1.0).abs() < 1e-12);
        assert!(r.high >= 2.0);
    }

    #[test]
    fn an_empty_or_flat_range_is_never_degenerate() {
        let none = Range::covering(&[]);
        assert_eq!((none.low, none.high), (0.0, 1.0));
        let flat = Range::covering(&[0.0, 0.0]);
        assert!(flat.high > flat.low);
    }

    #[test]
    fn mapping_clamps_to_the_output() {
        let r = Range {
            low: 0.0,
            high: 10.0,
        };
        assert!((r.map(5.0, 0.0, 100.0) - 50.0).abs() < 1e-9);
        assert!((r.map(-3.0, 0.0, 100.0)).abs() < 1e-9);
        assert!((r.map(99.0, 0.0, 100.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn larger_values_are_higher_on_screen() {
        let v = Range {
            low: 0.0,
            high: 1.0,
        };
        assert!(v.map(0.9, FRAME.y0(), FRAME.y1()) < v.map(0.1, FRAME.y0(), FRAME.y1()));
    }

    #[test]
    fn the_polyline_has_one_point_per_finite_value_inside_the_frame() {
        let t = Range {
            low: 0.0,
            high: 2.0,
        };
        let v = Range {
            low: 0.0,
            high: 1.0,
        };
        let text = polyline(&[0.0, f64::NAN, 0.5, 1.0], 0.0, 0.5, t, v, FRAME);
        let points: Vec<(f64, f64)> = text
            .split(' ')
            .filter_map(|p| p.split_once(','))
            .filter_map(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))
            .collect();
        assert_eq!(points.len(), 3);
        assert!(points.iter().all(|&(x, y)| x >= FRAME.x0()
            && x <= FRAME.x1()
            && y >= FRAME.y1()
            && y <= FRAME.y0()));
        assert!(points[0].0 < points[1].0);
    }

    #[test]
    fn an_empty_signal_gives_an_empty_polyline() {
        let r = Range {
            low: 0.0,
            high: 1.0,
        };
        assert_eq!(polyline(&[], 0.0, 1.0, r, r, FRAME), "");
    }

    #[test]
    fn ticks_are_round_and_inside_the_range() {
        let r = Range {
            low: 0.0,
            high: 7.3,
        };
        let t = ticks(r, 5);
        assert_eq!(t.first(), Some(&0.0));
        assert!(t.iter().all(|x| (0.0..=7.3).contains(x)));
        assert!(t.len() >= 3 && t.len() <= 8, "{t:?}");
        assert_eq!(
            ticks(
                Range {
                    low: 1.0,
                    high: 1.0
                },
                5
            ),
            Vec::<f64>::new()
        );
    }

    #[test]
    fn labels_drop_trailing_zeros() {
        assert_eq!(tick_label(0.0), "0");
        assert_eq!(tick_label(0.5), "0.5");
        assert_eq!(tick_label(12.0), "12");
        assert_eq!(tick_label(-0.001), "0");
    }
}
