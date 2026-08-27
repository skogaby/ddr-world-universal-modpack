//! SSQ chunk walker — pure function over the SSQ blob.
//!
//! See `docs/ssq_format.md` §2 for the chunk header layout. Each chunk is:
//!
//! ```text
//! +0x00  u32  length   (total size including header, multiple of 4)
//! +0x04  u16  type
//! +0x06  u16  param2
//! +0x08  u16  param3
//! +0x0A  u16  param4
//! +0x0C  ...  body
//! ```
//!
//! The parser walks from offset 0, advancing by `length` at each step.
//! It stops on `length == 0` (end-of-file terminator) or `param2 == 0xFFFF`
//! (forward-compat sentinel — matches the game's own chunk walkers).

pub const CHUNK_HEADER_SIZE: usize = 12;

/// A chunk located within an SSQ blob.
pub struct SsqChunk<'a> {
    pub kind: u16,
    pub param2: u16,
    pub param3: u16,
    pub param4: u16,
    /// The chunk body — `length - 12` bytes. Empty for zero-body chunks.
    pub body: &'a [u8],
}

/// Find the first chunk whose `(type, param2)` match and return its body.
///
/// Returns `None` if no match is found, the blob is malformed (truncated
/// header, length < 12, length overrunning the blob), or the `0xFFFF`
/// sentinel is hit before the target.
pub fn find_chunk<'a>(blob: &'a [u8], kind: u16, param2: u16) -> Option<SsqChunk<'a>> {
    let mut offset = 0usize;
    while offset + CHUNK_HEADER_SIZE <= blob.len() {
        let length = read_u32_le(blob, offset) as usize;
        if length == 0 {
            return None; // file terminator
        }
        if length < CHUNK_HEADER_SIZE || offset + length > blob.len() {
            return None; // malformed
        }
        let this_kind = read_u16_le(blob, offset + 4);
        let this_param2 = read_u16_le(blob, offset + 6);
        if this_param2 == 0xFFFF {
            return None; // forward-compat abort marker
        }
        if this_kind == kind && this_param2 == param2 {
            let body_start = offset + CHUNK_HEADER_SIZE;
            let body_end = offset + length;
            return Some(SsqChunk {
                kind: this_kind,
                param2: this_param2,
                param3: read_u16_le(blob, offset + 8),
                param4: read_u16_le(blob, offset + 10),
                body: &blob[body_start..body_end],
            });
        }
        offset += length;
    }
    None
}

#[inline]
fn read_u16_le(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

#[inline]
fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}
