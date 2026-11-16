//! The stream of raw frames that `python -m axonhand.tools.decode_frames` writes: one header
//! line `AXONFRAMES 1 WIDTH HEIGHT FPS`, then every frame as `WIDTH * HEIGHT * 3` bytes of RGB.

use std::io::Read;

use axonhand_hand::RgbImage;

const MAGIC: &str = "AXONFRAMES";
const VERSION: &str = "1";
const MAX_HEADER: usize = 200;
/// Largest frame accepted, in bytes. Far above 8K video; it stops a damaged header from asking
/// for gigabytes.
const MAX_FRAME_BYTES: usize = 1 << 28;

/// Why a frame stream could not be read.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// The stream ended before the header.
    #[error("the frame stream is empty")]
    Empty,
    /// The first line is not a header this program understands.
    #[error("the frame stream does not start with a valid header: {0}")]
    BadHeader(String),
    /// The stream ended in the middle of a frame.
    #[error("the frame stream ended in the middle of a frame")]
    Truncated,
    /// Reading failed.
    #[error("cannot read the frame stream: {0}")]
    Io(#[from] std::io::Error),
    /// A frame could not be built from the bytes.
    #[error("invalid frame: {0}")]
    Frame(String),
}

/// What the header says about the video.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Header {
    /// Width of every frame, in pixels.
    pub width: usize,
    /// Height of every frame, in pixels.
    pub height: usize,
    /// Frames per second.
    pub fps: f64,
}

/// Reads the frames one at a time.
pub struct FrameStream<R: Read> {
    reader: R,
    header: Header,
}

impl<R: Read> FrameStream<R> {
    /// Read the header from `reader` and be ready to read frames.
    ///
    /// # Errors
    ///
    /// [`StreamError::Empty`] if there is nothing to read, [`StreamError::BadHeader`] if the first
    /// line is wrong.
    pub fn open(mut reader: R) -> Result<Self, StreamError> {
        let mut line = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            if reader.read(&mut byte)? == 0 {
                return if line.is_empty() {
                    Err(StreamError::Empty)
                } else {
                    Err(StreamError::BadHeader("no end of line".into()))
                };
            }
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
            if line.len() > MAX_HEADER {
                return Err(StreamError::BadHeader("the line is too long".into()));
            }
        }
        let header = parse_header(&String::from_utf8_lossy(&line))?;
        Ok(Self { reader, header })
    }

    /// The header.
    #[must_use]
    pub fn header(&self) -> Header {
        self.header
    }

    /// The next frame, or `None` at the end of the stream.
    ///
    /// # Errors
    ///
    /// [`StreamError::Truncated`] if the stream ends part way through a frame.
    pub fn next_frame(&mut self) -> Result<Option<RgbImage>, StreamError> {
        let length = self.header.width * self.header.height * 3;
        let mut data = vec![0_u8; length];
        let mut filled = 0;
        while filled < length {
            let n = self.reader.read(&mut data[filled..])?;
            if n == 0 {
                return if filled == 0 {
                    Ok(None)
                } else {
                    Err(StreamError::Truncated)
                };
            }
            filled += n;
        }
        RgbImage::new(self.header.width, self.header.height, data)
            .map(Some)
            .map_err(|e| StreamError::Frame(e.to_string()))
    }
}

fn parse_header(line: &str) -> Result<Header, StreamError> {
    let bad = |why: &str| StreamError::BadHeader(why.to_owned());
    let mut parts = line.split_whitespace();
    if parts.next() != Some(MAGIC) {
        return Err(bad("it does not start with AXONFRAMES"));
    }
    if parts.next() != Some(VERSION) {
        return Err(bad("unknown version"));
    }
    let mut number = |name: &str| -> Result<&str, StreamError> {
        parts
            .next()
            .ok_or_else(|| bad(&format!("{name} is missing")))
    };
    let width: usize = number("width")?
        .parse()
        .map_err(|_| bad("width is not a number"))?;
    let height: usize = number("height")?
        .parse()
        .map_err(|_| bad("height is not a number"))?;
    let fps: f64 = number("fps")?
        .parse()
        .map_err(|_| bad("fps is not a number"))?;
    if parts.next().is_some() {
        return Err(bad("there is more than five fields"));
    }
    if width == 0 || height == 0 || width.saturating_mul(height).saturating_mul(3) > MAX_FRAME_BYTES
    {
        return Err(bad("the frame size is impossible"));
    }
    if !fps.is_finite() || fps <= 0.0 {
        return Err(bad("fps must be positive"));
    }
    Ok(Header { width, height, fps })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(text: &[u8]) -> Result<FrameStream<&[u8]>, StreamError> {
        FrameStream::open(text)
    }

    #[test]
    fn frames_are_read_whole_and_the_end_is_clean() -> Result<(), StreamError> {
        let mut data = b"AXONFRAMES 1 2 1 29.97\n".to_vec();
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        data.extend_from_slice(&[7, 8, 9, 10, 11, 12]);
        let mut frames = stream(&data)?;
        assert_eq!(
            frames.header(),
            Header {
                width: 2,
                height: 1,
                fps: 29.97
            }
        );
        let first = frames.next_frame()?.ok_or(StreamError::Empty)?;
        assert_eq!(first.as_bytes(), &[1, 2, 3, 4, 5, 6]);
        assert!(frames.next_frame()?.is_some());
        assert!(frames.next_frame()?.is_none());
        Ok(())
    }

    #[test]
    fn a_frame_cut_short_is_an_error_not_a_shorter_video() -> Result<(), StreamError> {
        let mut data = b"AXONFRAMES 1 2 1 30.0\n".to_vec();
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
        let mut frames = stream(&data)?;
        assert!(frames.next_frame()?.is_some());
        assert!(matches!(frames.next_frame(), Err(StreamError::Truncated)));
        Ok(())
    }

    #[test]
    fn a_header_with_no_frames_reads_as_no_frames() -> Result<(), StreamError> {
        let mut frames = stream(b"AXONFRAMES 1 2 1 30.0\n")?;
        assert!(frames.next_frame()?.is_none());
        Ok(())
    }

    #[test]
    fn nothing_at_all_is_an_empty_stream() {
        assert!(matches!(stream(b""), Err(StreamError::Empty)));
    }

    #[test]
    fn bad_headers_are_refused() {
        for text in [
            &b"hello\n"[..],
            b"AXONFRAMES 2 2 1 30\n",
            b"AXONFRAMES 1 2 1\n",
            b"AXONFRAMES 1 0 1 30\n",
            b"AXONFRAMES 1 2 1 0\n",
            b"AXONFRAMES 1 2 1 nan\n",
            b"AXONFRAMES 1 a 1 30\n",
            b"AXONFRAMES 1 2 1 30 extra\n",
            b"AXONFRAMES 1 99999 99999 30\n",
            b"AXONFRAMES 1 2 1 30",
        ] {
            assert!(
                matches!(stream(text), Err(StreamError::BadHeader(_))),
                "{}",
                String::from_utf8_lossy(text)
            );
        }
        let long = vec![b'x'; 300];
        assert!(matches!(stream(&long), Err(StreamError::BadHeader(_))));
    }
}
