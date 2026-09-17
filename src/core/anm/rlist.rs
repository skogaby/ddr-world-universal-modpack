//! Konami `MRL0` resource lists (`startup.arc` `data/*/*_resources.rlist`) —
//! `scripts/ktmdl_dump.py::parse_rlist`.
//!
//! ```text
//! 0x00 "MRL0"  0x04 "LE"  0x06 u16 pad   0x08 u32 count   0x0C u32 total (== file length)
//! 0x10 records: u32 str_off, u32 nfields, u32 rec_len, u32 field_off[nfields]
//!               every offset relative to the RECORD start; next record at rec + rec_len
//! ```
//! Rows are `(key, fields)`; duplicate keys are preserved positionally
//! (`map_resources.rlist` has `boom00` at rows 0 AND 32 — the row index is
//! the stage id the game addresses).

use super::{FormatError, Le};

pub const MAGIC: &[u8; 4] = b"MRL0";
pub const ENDIAN_LE: &[u8; 2] = b"LE";
const MAX_ROWS: u32 = 65_536;
const MAX_FIELDS: u32 = 4096;

pub type Row = (String, Vec<String>);

pub fn parse(bytes: &[u8]) -> Result<Vec<Row>, FormatError> {
    if bytes.get(0..4) != Some(&MAGIC[..]) || bytes.get(4..6) != Some(&ENDIAN_LE[..]) {
        return Err(FormatError::BadMagic);
    }
    let r = Le(bytes);
    let count = r.u32(8).ok_or(FormatError::Truncated)?;
    let total = r.u32(0xC).ok_or(FormatError::Truncated)? as usize;
    if total != bytes.len() {
        return Err(FormatError::SizeMismatch);
    }
    if count > MAX_ROWS {
        return Err(FormatError::Malformed);
    }
    let mut rows = Vec::with_capacity(count as usize);
    let mut off = 0x10usize;
    for _ in 0..count {
        let str_off = r.u32(off).ok_or(FormatError::Truncated)? as usize;
        let nfields = r.u32(off + 4).ok_or(FormatError::Truncated)?;
        let rec_len = r.u32(off + 8).ok_or(FormatError::Truncated)? as usize;
        if nfields > MAX_FIELDS || rec_len < 12 {
            return Err(FormatError::Malformed);
        }
        let key = r
            .cstr(off.checked_add(str_off).ok_or(FormatError::Truncated)?)
            .ok_or(FormatError::Truncated)?;
        let mut fields = Vec::with_capacity(nfields as usize);
        for i in 0..nfields as usize {
            let fo = r.u32(off + 12 + 4 * i).ok_or(FormatError::Truncated)? as usize;
            fields.push(
                r.cstr(off.checked_add(fo).ok_or(FormatError::Truncated)?)
                    .ok_or(FormatError::Truncated)?,
            );
        }
        rows.push((key, fields));
        off = off.checked_add(rec_len).ok_or(FormatError::Truncated)?;
    }
    Ok(rows)
}

/// First row with this key (the stock lists are addressed by ROW index, but
/// the chara/camera lists are keyed uniquely).
pub fn find<'a>(rows: &'a [Row], key: &str) -> Option<(usize, &'a Row)> {
    rows.iter().enumerate().find(|(_, (k, _))| k == key)
}
