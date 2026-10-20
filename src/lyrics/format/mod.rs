//! Every parser expects hostile input and caps it at [`MAX_SOURCE_BYTES`],
//! [`MAX_LINES`] and [`MAX_WORDS`].

pub mod lrc;
pub mod lyricsfile;
pub mod ttml;

use super::error::{Error, Result};
use super::model::Lyrics;

pub const MAX_SOURCE_BYTES: usize = 512 << 10;
pub const MAX_SIDECAR_BYTES: u64 = 256 << 10;
pub const MAX_LINES: usize = 2000;
pub const MAX_WORDS: usize = 16000;

pub type Parser = fn(&[u8]) -> Result<Lyrics>;

/// Bytes to text: bounded, BOM dropped, UTF-8 or nothing.
pub(crate) fn decode(data: &[u8]) -> Result<&str> {
    if data.len() > MAX_SOURCE_BYTES {
        return Err(Error::TooLarge);
    }
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    std::str::from_utf8(data).map_err(|_| Error::Encoding)
}
