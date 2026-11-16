//! Writes the landmark track in the format `axonhand.landmarks.LandmarkTrack.save` writes, so
//! the plug-ins read it with the code they already have: a NumPy `.npz` file holding `version`,
//! `fps` and `points` (frames x 21 x 3, NaN where no hand was found).
//!
//! An `.npz` file is a ZIP archive of `.npy` arrays. The archive here is stored without
//! compression (as `numpy.savez` does), so no compression library is needed.

use std::io::Write;

use axonhand_hand::rois::LANDMARK_COUNT;

use crate::track::Points;

/// The format version `LandmarkTrack` checks when it loads a file.
const FORMAT_VERSION: i64 = 1;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A `.npy` file (format 1.0) holding `data`, little-endian.
fn npy(descr: &str, shape: &str, data: &[u8]) -> Vec<u8> {
    let mut header = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {shape}, }}");
    // The magic string, version and length field take 10 bytes; the header, with its closing
    // newline, pads to a multiple of 64.
    let padding = (64 - (10 + header.len() + 1) % 64) % 64;
    header.push_str(&" ".repeat(padding));
    header.push('\n');
    let mut out = Vec::with_capacity(10 + header.len() + data.len());
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    // The header is a few dozen bytes; the cast cannot truncate.
    out.extend_from_slice(&(header.len() as u16).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(data);
    out
}

/// A ZIP archive holding `members` stored without compression.
fn zip(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    const DOS_DATE: u16 = (1 << 5) | 1; // 1980-01-01, which is what "no date" looks like
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in members {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_bytes = name.as_bytes();
        let common = |buffer: &mut Vec<u8>| {
            buffer.extend_from_slice(&20_u16.to_le_bytes()); // version needed
            buffer.extend_from_slice(&0_u16.to_le_bytes()); // flags
            buffer.extend_from_slice(&0_u16.to_le_bytes()); // stored
            buffer.extend_from_slice(&0_u16.to_le_bytes()); // time
            buffer.extend_from_slice(&DOS_DATE.to_le_bytes());
            buffer.extend_from_slice(&crc.to_le_bytes());
            buffer.extend_from_slice(&size.to_le_bytes()); // compressed size
            buffer.extend_from_slice(&size.to_le_bytes()); // uncompressed size
            buffer.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            buffer.extend_from_slice(&0_u16.to_le_bytes()); // extra field length
        };
        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        directory.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        directory.extend_from_slice(&20_u16.to_le_bytes()); // version made by
        common(&mut directory);
        directory.extend_from_slice(&0_u16.to_le_bytes()); // comment length
        directory.extend_from_slice(&0_u16.to_le_bytes()); // disk number
        directory.extend_from_slice(&0_u16.to_le_bytes()); // internal attributes
        directory.extend_from_slice(&0_u32.to_le_bytes()); // external attributes
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name_bytes);
    }
    let start = out.len() as u32;
    let count = members.len() as u16;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0_u16.to_le_bytes()); // disk with the directory
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes()); // comment length
    out
}

/// The bytes of the `.npz` file for a track.
#[must_use]
pub fn npz_bytes(fps: f64, frames: &[Option<Points>]) -> Vec<u8> {
    let mut points = Vec::with_capacity(frames.len() * LANDMARK_COUNT * 3 * 8);
    for frame in frames {
        for landmark in 0..LANDMARK_COUNT {
            for axis in 0..3 {
                let value = frame.as_ref().map_or(f64::NAN, |f| f[landmark][axis]);
                points.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    let shape = format!("({}, {LANDMARK_COUNT}, 3)", frames.len());
    zip(&[
        (
            "version.npy",
            npy("<i8", "()", &FORMAT_VERSION.to_le_bytes()),
        ),
        ("fps.npy", npy("<f8", "()", &fps.to_le_bytes())),
        ("points.npy", npy("<f8", &shape, &points)),
    ])
}

/// Write a track to `path` as `.npz`.
///
/// # Errors
///
/// The error from creating or writing the file.
pub fn write_npz(
    path: &std::path::Path,
    fps: f64,
    frames: &[Option<Points>],
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(&npz_bytes(fps, frames))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn an_npy_header_pads_to_a_multiple_of_64_and_ends_in_a_newline() {
        for shape in ["()", "(5, 21, 3)", "(123456, 21, 3)"] {
            let bytes = npy("<f8", shape, &[0; 8]);
            let header_len = usize::from(u16::from_le_bytes([bytes[8], bytes[9]]));
            assert_eq!((10 + header_len) % 64, 0, "{shape}");
            assert_eq!(bytes[10 + header_len - 1], b'\n');
            assert_eq!(bytes.len(), 10 + header_len + 8);
        }
    }

    #[test]
    fn the_archive_has_one_entry_for_each_array_and_a_matching_directory() {
        let frames = vec![None, None];
        let bytes = npz_bytes(30.0, &frames);
        assert_eq!(&bytes[..4], &0x0403_4b50_u32.to_le_bytes());
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], &0x0605_4b50_u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]), 3);
    }

    /// NumPy reads what this file writes. Runs when a Python with NumPy is named.
    #[test]
    fn numpy_reads_the_file_back() -> Result<(), Box<dyn std::error::Error>> {
        let Ok(python) = std::env::var("AXONHAND_PYTHON") else {
            eprintln!("skipped: set AXONHAND_PYTHON to run this test");
            return Ok(());
        };
        let mut hand = [[0.0; 3]; LANDMARK_COUNT];
        for (i, point) in hand.iter_mut().enumerate() {
            *point = [i as f64, i as f64 * 2.0, -0.5];
        }
        let dir = std::env::temp_dir().join(format!("axonhand-npz-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("track.npz");
        write_npz(&path, 29.97, &[Some(hand), None, Some(hand)])?;
        let script = "\
import sys
from pathlib import Path
import numpy as np
from axonhand.landmarks import LandmarkTrack
t = LandmarkTrack.load(Path(sys.argv[1]))
assert t.frames == 3, t.frames
assert abs(t.fps - 29.97) < 1e-12
assert np.isnan(t.points[1]).all()
assert t.points[0, 20, 1] == 40.0 and t.points[2, 3, 2] == -0.5
print('ok')
";
        let out = std::process::Command::new(python)
            .args(["-c", script])
            .arg(&path)
            .output()?;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(())
    }
}
