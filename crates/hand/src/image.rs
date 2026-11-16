//! A small RGB image and the two resampling operations the pipeline needs.
//!
//! Both operations follow Pillow's, because the Python pipeline uses Pillow and the two
//! implementations must give the same landmarks. Coordinates are continuous: the top-left corner of
//! the top-left pixel is (0, 0), so a pixel's centre is at (i + 0.5, j + 0.5).

use crate::error::{HandError, Result};

/// An 8-bit RGB picture, rows top to bottom, three bytes per pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbImage {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl RgbImage {
    /// Wrap pixel bytes.
    ///
    /// # Errors
    ///
    /// Fails if a side is zero or `data` is not exactly `width * height * 3` bytes.
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(HandError::Image("width and height must be positive".into()));
        }
        let expected = width.checked_mul(height).and_then(|n| n.checked_mul(3));
        if expected != Some(data.len()) {
            return Err(HandError::Image(format!(
                "{} bytes do not fill a {width} x {height} RGB image",
                data.len()
            )));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Read a binary PPM (`P6`, 8 bits) file, the plain format `parity_fixture --clip` writes.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read or is not such a PPM file.
    pub fn from_ppm(path: &std::path::Path) -> Result<Self> {
        let bad = |why: &str| HandError::Image(format!("{}: {why}", path.display()));
        let bytes = std::fs::read(path).map_err(|e| bad(&e.to_string()))?;
        let mut fields: Vec<&[u8]> = Vec::new();
        let mut start = 0;
        for (i, byte) in bytes.iter().enumerate() {
            if byte.is_ascii_whitespace() {
                if i > start {
                    fields.push(&bytes[start..i]);
                }
                start = i + 1;
                if fields.len() == 4 {
                    break;
                }
            }
        }
        let number = |field: &[u8]| -> Result<usize> {
            std::str::from_utf8(field)
                .ok()
                .and_then(|t| t.parse().ok())
                .ok_or_else(|| bad("the header is not valid"))
        };
        match fields.as_slice() {
            [b"P6", width, height, b"255"] => {
                Self::new(number(width)?, number(height)?, bytes[start..].to_vec())
            }
            _ => Err(bad("not a P6 PPM file with 255 levels")),
        }
    }

    /// An all-black picture.
    ///
    /// # Errors
    ///
    /// Fails if a side is zero.
    pub fn black(width: usize, height: usize) -> Result<Self> {
        let len = width.saturating_mul(height).saturating_mul(3);
        Self::new(width, height, vec![0; len])
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// The pixel bytes, rows top to bottom.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// The three bytes of one pixel, or black when the position is outside the picture.
    fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        if x >= self.width || y >= self.height {
            return [0; 3];
        }
        let i = (y * self.width + x) * 3;
        match self.data.get(i..i + 3) {
            Some([r, g, b]) => [*r, *g, *b],
            _ => [0; 3],
        }
    }

    /// Copy `other` onto this picture with its top-left corner at (`x`, `y`); parts outside are
    /// dropped.
    pub fn paste(&mut self, other: &Self, x: usize, y: usize) {
        for row in 0..other.height {
            let target_y = y + row;
            if target_y >= self.height {
                break;
            }
            let columns = other.width.min(self.width.saturating_sub(x));
            let from = row * other.width * 3;
            let to = (target_y * self.width + x) * 3;
            if let (Some(source), Some(target)) = (
                other.data.get(from..from + columns * 3),
                self.data.get_mut(to..to + columns * 3),
            ) {
                target.copy_from_slice(source);
            }
        }
    }

    /// The picture as network input: `height * width * 3` numbers in 0..1, in the order of the
    /// picture itself (height, width, colour).
    #[must_use]
    pub fn to_blob(&self) -> Vec<f32> {
        self.data.iter().map(|&v| f32::from(v) / 255.0).collect()
    }

    /// Resize with a triangle filter that widens when shrinking (Pillow's `BILINEAR` resize).
    ///
    /// # Errors
    ///
    /// Fails if the new size has a zero side.
    pub fn resize_bilinear(&self, width: usize, height: usize) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(HandError::Image("the new size must be positive".into()));
        }
        let across = Taps::new(self.width, width);
        let down = Taps::new(self.height, height);
        // Horizontal pass first, then vertical, each rounded to a byte, as Pillow does.
        let mut middle = vec![0u8; width * self.height * 3];
        for y in 0..self.height {
            for x in 0..width {
                let (start, weights) = across.at(x);
                for c in 0..3 {
                    let mut sum = ROUNDING;
                    for (k, w) in weights.iter().enumerate() {
                        sum += i64::from(*w) * i64::from(self.pixel(start + k, y)[c]);
                    }
                    if let Some(slot) = middle.get_mut((y * width + x) * 3 + c) {
                        *slot = fixed_to_byte(sum);
                    }
                }
            }
        }
        let middle = Self {
            width,
            height: self.height,
            data: middle,
        };
        let mut out = vec![0u8; width * height * 3];
        for y in 0..height {
            let (start, weights) = down.at(y);
            for x in 0..width {
                for c in 0..3 {
                    let mut sum = ROUNDING;
                    for (k, w) in weights.iter().enumerate() {
                        sum += i64::from(*w) * i64::from(middle.pixel(x, start + k)[c]);
                    }
                    if let Some(slot) = out.get_mut((y * width + x) * 3 + c) {
                        *slot = fixed_to_byte(sum);
                    }
                }
            }
        }
        Self::new(width, height, out)
    }

    /// Cut out a square `size` x `size` picture through an affine map (Pillow's `AFFINE`
    /// transform with bilinear sampling; like Pillow it truncates rather than rounds).
    ///
    /// `m` is `(a, b, c, d, e, f)`: the crop position (u, v) in 0..`size` lies at image position
    /// `x = a*u + b*v + c`, `y = d*u + e*v + f`. Positions outside this picture are black.
    ///
    /// # Errors
    ///
    /// Fails if `size` is zero.
    pub fn affine_crop(&self, m: [f64; 6], size: usize) -> Result<Self> {
        if size == 0 {
            return Err(HandError::Image("the crop size must be positive".into()));
        }
        let [a, b, c, d, e, f] = m;
        let (w, h) = (self.width as f64, self.height as f64);
        let mut out = vec![0u8; size * size * 3];
        for row in 0..size {
            for col in 0..size {
                let u = col as f64 + 0.5;
                let v = row as f64 + 0.5;
                let x = a * u + b * v + c;
                let y = d * u + e * v + f;
                if x < 0.0 || x >= w || y < 0.0 || y >= h {
                    continue;
                }
                let (x, y) = (x - 0.5, y - 0.5);
                let (fx, fy) = (x.floor(), y.floor());
                let (dx, dy) = (x - fx, y - fy);
                let x0 = clamp_index(fx, self.width);
                let x1 = clamp_index(fx + 1.0, self.width);
                let y0 = clamp_index(fy, self.height);
                let y1 = clamp_index(fy + 1.0, self.height);
                let (p00, p10) = (self.pixel(x0, y0), self.pixel(x1, y0));
                let (p01, p11) = (self.pixel(x0, y1), self.pixel(x1, y1));
                for ch in 0..3 {
                    let top = f64::from(p00[ch]) + dx * (f64::from(p10[ch]) - f64::from(p00[ch]));
                    let bottom =
                        f64::from(p01[ch]) + dx * (f64::from(p11[ch]) - f64::from(p01[ch]));
                    if let Some(slot) = out.get_mut((row * size + col) * 3 + ch) {
                        *slot = (top + dy * (bottom - top)).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        Self::new(size, size, out)
    }
}

/// Fractional bits of the integer filter weights; Pillow uses the same, so results are identical.
const PRECISION_BITS: u32 = 22;
const ROUNDING: i64 = 1 << (PRECISION_BITS - 1);

/// Back from a fixed-point sum to a byte, saturating.
fn fixed_to_byte(sum: i64) -> u8 {
    (sum >> PRECISION_BITS).clamp(0, 255) as u8
}

/// A pixel index from a possibly out-of-range coordinate, pinned to the nearest edge pixel.
fn clamp_index(value: f64, len: usize) -> usize {
    value.clamp(0.0, (len - 1) as f64) as usize
}

/// Which input pixels, with which weights, make each output pixel along one axis.
struct Taps {
    rows: Vec<(usize, Vec<i32>)>,
}

impl Taps {
    fn new(input: usize, output: usize) -> Self {
        let scale = input as f64 / output as f64;
        let stretch = scale.max(1.0);
        let support = stretch;
        let rows = (0..output)
            .map(|i| {
                let centre = (i as f64 + 0.5) * scale;
                let start = ((centre - support + 0.5) as i64).max(0) as usize;
                let end = (((centre + support + 0.5) as i64).max(0) as usize).min(input);
                let mut weights: Vec<f64> = (start..end)
                    .map(|k| triangle((k as f64 - centre + 0.5) / stretch))
                    .collect();
                let total: f64 = weights.iter().sum();
                if total != 0.0 {
                    weights.iter_mut().for_each(|w| *w /= total);
                }
                let fixed = weights
                    .iter()
                    .map(|w| {
                        (w * f64::from(1_u32 << PRECISION_BITS) + if *w < 0.0 { -0.5 } else { 0.5 })
                            as i32
                    })
                    .collect();
                (start, fixed)
            })
            .collect();
        Self { rows }
    }

    fn at(&self, i: usize) -> (usize, &[i32]) {
        match self.rows.get(i) {
            Some((start, weights)) => (*start, weights),
            None => (0, &[]),
        }
    }
}

fn triangle(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        1.0 - x
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(width: usize, height: usize) -> RgbImage {
        let data = (0..width * height * 3)
            .map(|i| (i * 7 % 256) as u8)
            .collect();
        RgbImage::new(width, height, data).unwrap_or_else(|_| unreachable_image())
    }

    fn unreachable_image() -> RgbImage {
        RgbImage {
            width: 1,
            height: 1,
            data: vec![0; 3],
        }
    }

    #[test]
    fn new_rejects_wrong_length_and_zero_sides() {
        assert!(RgbImage::new(2, 2, vec![0; 11]).is_err());
        assert!(RgbImage::new(0, 2, vec![]).is_err());
        assert!(RgbImage::black(0, 4).is_err());
    }

    #[test]
    fn resizing_to_the_same_size_changes_nothing() {
        let image = ramp(9, 5);
        assert_eq!(image.resize_bilinear(9, 5).ok(), Some(image));
    }

    #[test]
    fn a_flat_picture_stays_flat_when_resized() {
        let image = RgbImage::new(10, 6, vec![77; 10 * 6 * 3]).ok();
        let small = image.and_then(|i| i.resize_bilinear(4, 3).ok());
        assert!(small.is_some_and(|s| s.as_bytes().iter().all(|&v| v == 77)));
    }

    #[test]
    fn paste_places_and_clips() {
        let mut canvas = RgbImage::black(4, 4).unwrap_or_else(|_| unreachable_image());
        let patch = RgbImage::new(2, 2, vec![9; 12]).unwrap_or_else(|_| unreachable_image());
        canvas.paste(&patch, 3, 3);
        assert_eq!(canvas.pixel(3, 3), [9, 9, 9]);
        assert_eq!(canvas.pixel(2, 3), [0, 0, 0]);
    }

    #[test]
    fn identity_crop_copies_the_picture() {
        let image = ramp(8, 8);
        let crop = image.affine_crop([1.0, 0.0, 0.0, 0.0, 1.0, 0.0], 8).ok();
        assert_eq!(crop, Some(image));
    }

    #[test]
    fn crop_outside_the_picture_is_black() {
        let image = ramp(8, 8);
        let crop = image
            .affine_crop([1.0, 0.0, 100.0, 0.0, 1.0, 100.0], 4)
            .ok();
        assert!(crop.is_some_and(|c| c.as_bytes().iter().all(|&v| v == 0)));
    }

    #[test]
    fn blob_is_scaled_to_unit_range() {
        let image = RgbImage::new(1, 1, vec![0, 51, 255]).unwrap_or_else(|_| unreachable_image());
        assert_eq!(image.to_blob(), vec![0.0, 0.2, 1.0]);
    }
}
