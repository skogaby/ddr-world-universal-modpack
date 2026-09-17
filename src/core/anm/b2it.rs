//! `.b2it` / `.grp2it` bone-name → index tables (`docs/3d_model_format_research.md`
//! §4; `scripts/ktmdl_dump.py::parse_b2it`).
//!
//! ```text
//! 0x00 "B2IT"        0x04 u32 file size   0x08 u64 0
//! 0x10 u32 count
//! 0x14 u32 → u32[count] absolute offsets to NUL-terminated names, sorted ordinally
//! 0x18 u32 → u32[count] target (bone) index, parallel to the names
//! ```
//! The game binary-searches the sorted names for part attachment
//! (`head/face → Head`, `hips → Hips`, `chest → Spine2`, `forearm →
//! LeftForeArmRoll`).

use super::{FormatError, Le};

pub const MAGIC: &[u8; 4] = b"B2IT";
/// Stock tables hold ≤ 33 names; anything past this is garbage.
const MAX_ENTRIES: u32 = 4096;

/// Entries in FILE order (already sorted by ordinal byte order).
pub fn parse(bytes: &[u8]) -> Result<Vec<(String, u32)>, FormatError> {
    if bytes.get(0..4) != Some(&MAGIC[..]) {
        return Err(FormatError::BadMagic);
    }
    let r = Le(bytes);
    let count = r.u32(0x10).ok_or(FormatError::Truncated)?;
    let names_off = r.u32(0x14).ok_or(FormatError::Truncated)? as usize;
    let idx_off = r.u32(0x18).ok_or(FormatError::Truncated)? as usize;
    if count > MAX_ENTRIES {
        return Err(FormatError::Malformed);
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let name_ptr = r.u32(names_off + 4 * i).ok_or(FormatError::Truncated)? as usize;
        let name = r.cstr(name_ptr).ok_or(FormatError::Truncated)?;
        let index = r.u32(idx_off + 4 * i).ok_or(FormatError::Truncated)?;
        out.push((name, index));
    }
    Ok(out)
}

/// Binary search by exact (case-sensitive, ordinal) name over a table in
/// file order.
pub fn index_of(table: &[(String, u32)], name: &str) -> Option<u32> {
    table
        .binary_search_by(|(n, _)| n.as_bytes().cmp(name.as_bytes()))
        .ok()
        .and_then(|i| table.get(i))
        .map(|(_, idx)| *idx)
}
