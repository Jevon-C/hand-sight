//! Small helpers for uploaded files: type detection by content, safe display names, ids.

/// Container formats the app accepts, detected from the first bytes of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// MP4 (ISO base media).
    Mp4,
    /// QuickTime movie, as written by many phones.
    Mov,
    /// WebM or Matroska.
    Webm,
}

impl Container {
    /// File extension, without the dot, used for the stored copy.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Webm => "webm",
        }
    }

    /// Recognise a container from the first bytes of a file, ignoring the file's name.
    #[must_use]
    pub fn sniff(head: &[u8]) -> Option<Self> {
        if head.len() >= 12 && &head[4..8] == b"ftyp" {
            return Some(if &head[8..12] == b"qt  " {
                Self::Mov
            } else {
                Self::Mp4
            });
        }
        if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            return Some(Self::Webm);
        }
        None
    }
}

/// True when `id` has the shape of a video id (64 lowercase hex digits). Checked before an id
/// is ever used in a file path or query.
#[must_use]
pub fn is_video_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Lowercase hex text of `bytes`.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            // Writing to a String cannot fail.
            let _ = write!(out, "{b:02x}");
            out
        })
}

const MAX_NAME_CHARS: usize = 200;

/// A name safe to show and store: the last path component of what the browser sent, with
/// control characters removed and the length limited. It is never used to build a path.
#[must_use]
pub fn display_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "video".to_owned()
    } else {
        trimmed.chars().take(MAX_NAME_CHARS).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_are_recognised_by_content() {
        assert_eq!(
            Container::sniff(b"\0\0\0\x18ftypisom\0\0\0\0"),
            Some(Container::Mp4)
        );
        assert_eq!(
            Container::sniff(b"\0\0\0\x14ftypqt  \0\0\0\0"),
            Some(Container::Mov)
        );
        assert_eq!(
            Container::sniff(&[0x1A, 0x45, 0xDF, 0xA3, 1, 2, 3]),
            Some(Container::Webm)
        );
    }

    #[test]
    fn other_content_is_rejected() {
        assert_eq!(Container::sniff(b"MZ\x90\0this is an exe!"), None);
        assert_eq!(Container::sniff(b"<html></html>"), None);
        assert_eq!(Container::sniff(b""), None);
        assert_eq!(Container::sniff(b"ftyp"), None);
    }

    #[test]
    fn video_ids_must_be_lowercase_hex_of_the_right_length() {
        assert!(is_video_id(&"a1".repeat(32)));
        for bad in [
            "",
            "../etc/passwd",
            &"A1".repeat(32),
            &"a1".repeat(31),
            &"g1".repeat(32),
        ] {
            assert!(!is_video_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn hex_encodes_bytes() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn display_names_drop_paths_and_control_characters() {
        assert_eq!(display_name("C:\\Users\\me\\clip 1.mp4"), "clip 1.mp4");
        assert_eq!(display_name("../../etc/passwd"), "passwd");
        assert_eq!(display_name("a\u{0}b\nc.mp4"), "abc.mp4");
        assert_eq!(display_name(""), "video");
        assert_eq!(display_name("dir/"), "video");
        assert_eq!(display_name(&"x".repeat(500)).chars().count(), 200);
    }
}
