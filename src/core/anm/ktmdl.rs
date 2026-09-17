//! KTMDL (`.model`) bone table — `docs/3d_model_format_research.md` §3.1/§3.2;
//! `scripts/ktmdl_dump.py::parse_model` (bone loop only).
//!
//! ```text
//! 0x00 "KTMDL\0\0\0"   0x08 u32 major (2)   0x0C u32 minor (> 1)   0x10 u16 1
//! 0x18 u32 bone_count  0x1C u32 bone_off (absolute)
//! bone (0xB0): 0x10 f32[16] bind (MODEL-space, row-vector), 0x50 f32[16] inverse bind,
//!              0xAC i16 parent (−1 = root)
//! ```
//! Bones are topologically ordered (`parent < index`). Only the fields the
//! pose chain needs are read — the GPU resource carries the same three
//! arrays (`FUN_18018a010`), so the DLL can also build a `Skeleton` from the
//! resident model instead of the file.

use super::pose::Skeleton;
use super::{FormatError, Le};

pub const MAGIC: &[u8; 5] = b"KTMDL";
pub const BONE_STRIDE: usize = 0xB0;
pub const BONE_BIND_OFF: usize = 0x10;
pub const BONE_INVERSE_OFF: usize = 0x50;
pub const BONE_PARENT_OFF: usize = 0xAC;
/// Stock: 33 for dancers, up to ~60 on stage props.
const MAX_BONES: u32 = 4096;

pub fn bone_table(bytes: &[u8]) -> Result<Skeleton, FormatError> {
    if bytes.get(0..5) != Some(&MAGIC[..]) {
        return Err(FormatError::BadMagic);
    }
    let r = Le(bytes);
    let major = r.u32(0x08).ok_or(FormatError::Truncated)?;
    let minor = r.u32(0x0C).ok_or(FormatError::Truncated)?;
    let flag10 = r.u16(0x10).ok_or(FormatError::Truncated)?;
    if major != 2 || minor < 2 || flag10 != 1 {
        return Err(FormatError::Malformed);
    }
    let bone_count = r.u32(0x18).ok_or(FormatError::Truncated)?;
    let bone_off = r.u32(0x1C).ok_or(FormatError::Truncated)? as usize;
    if bone_count > MAX_BONES {
        return Err(FormatError::Malformed);
    }
    let n = bone_count as usize;
    let mut parents = Vec::with_capacity(n);
    let mut bind_world = Vec::with_capacity(n);
    let mut inverse_bind = Vec::with_capacity(n);
    for i in 0..n {
        let b = bone_off
            .checked_add(i.checked_mul(BONE_STRIDE).ok_or(FormatError::Malformed)?)
            .ok_or(FormatError::Truncated)?;
        bind_world.push(
            r.f32s::<16>(b + BONE_BIND_OFF)
                .ok_or(FormatError::Truncated)?,
        );
        inverse_bind.push(
            r.f32s::<16>(b + BONE_INVERSE_OFF)
                .ok_or(FormatError::Truncated)?,
        );
        parents.push(r.i16(b + BONE_PARENT_OFF).ok_or(FormatError::Truncated)?);
    }
    Ok(Skeleton {
        parents,
        bind_world,
        inverse_bind,
    })
}
