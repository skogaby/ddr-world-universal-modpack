//! KTMDL (`.model`) bone table — `docs/3d_model_format_research.md` §3.1/§3.2;
//! `scripts/ktmdl_dump.py::parse_model` (bone loop only).
//!
//! ```text
//! 0x00 "KTMDL\0\0\0"   0x08 u32 major (2)   0x0C u32 minor (> 1)   0x10 u16 1
//! 0x18 u32 bone_count  0x1C u32 bone_off (absolute)
//! 0x48 u32 material_count  0x4C u32 material_off (absolute)
//! 0x58 u32 info_count (1)  0x5C u32 info_off (absolute)
//! bone (0xB0): 0x10 f32[16] bind (MODEL-space, row-vector), 0x50 f32[16] inverse bind,
//!              0xAC i16 parent (−1 = root)
//! material (0xA0): 0x00 u64 packed shading-node name identity (§2) — the key a
//!              `.sanm` type-15 entry addresses (`sanm::bind_targets`)
//! info (0x30, §3.10): 0x10 f32[4] bbox max, 0x20 f32[4] bbox min (MODEL space)
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
pub const MATERIAL_STRIDE: usize = 0xA0;
pub const INFO_BBOX_MAX_OFF: usize = 0x10;
pub const INFO_BBOX_MIN_OFF: usize = 0x20;
/// Stock: 33 for dancers, up to ~60 on stage props.
const MAX_BONES: u32 = 4096;
/// Stock: ≤ 30 per model.
const MAX_MATERIALS: u32 = 4096;

fn check_header(r: &Le<'_>) -> Result<(), FormatError> {
    if r.0.get(0..5) != Some(&MAGIC[..]) {
        return Err(FormatError::BadMagic);
    }
    let major = r.u32(0x08).ok_or(FormatError::Truncated)?;
    let minor = r.u32(0x0C).ok_or(FormatError::Truncated)?;
    let flag10 = r.u16(0x10).ok_or(FormatError::Truncated)?;
    if major != 2 || minor < 2 || flag10 != 1 {
        return Err(FormatError::Malformed);
    }
    Ok(())
}

/// The material identities (`+0x00` u64 of each 0xA0 material record) in file
/// order — the order the converter builds the GPU resource's material array
/// and therefore the render items' private copies in.
pub fn material_identities(bytes: &[u8]) -> Result<Vec<u64>, FormatError> {
    let r = Le(bytes);
    check_header(&r)?;
    let count = r.u32(0x48).ok_or(FormatError::Truncated)?;
    let off = r.u32(0x4C).ok_or(FormatError::Truncated)? as usize;
    if count > MAX_MATERIALS {
        return Err(FormatError::Malformed);
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let m = off
            .checked_add(
                i.checked_mul(MATERIAL_STRIDE)
                    .ok_or(FormatError::Malformed)?,
            )
            .ok_or(FormatError::Truncated)?;
        let lo = r.u32(m).ok_or(FormatError::Truncated)? as u64;
        let hi = r.u32(m + 4).ok_or(FormatError::Truncated)? as u64;
        out.push(lo | (hi << 32));
    }
    Ok(out)
}

/// The model's reach from its own origin, MODEL units: the largest |axis
/// component| of the info bbox (§3.10). A skydome / backdrop that encloses
/// the scene reads in the hundreds (SuperNova skies 500–630, Hottest Party
/// ~500); props and dancers stay in the low tens. `None` without an info
/// block (`info_off` 0) — stock files always carry one.
pub fn bbox_extent(bytes: &[u8]) -> Result<Option<f32>, FormatError> {
    let r = Le(bytes);
    check_header(&r)?;
    let count = r.u32(0x58).ok_or(FormatError::Truncated)?;
    let off = r.u32(0x5C).ok_or(FormatError::Truncated)? as usize;
    if count == 0 || off == 0 {
        return Ok(None);
    }
    let max = r
        .f32s::<4>(off + INFO_BBOX_MAX_OFF)
        .ok_or(FormatError::Truncated)?;
    let min = r
        .f32s::<4>(off + INFO_BBOX_MIN_OFF)
        .ok_or(FormatError::Truncated)?;
    let extent = max[..3]
        .iter()
        .chain(&min[..3])
        .map(|v| v.abs())
        .filter(|v| v.is_finite())
        .fold(0.0f32, f32::max);
    Ok(Some(extent))
}

pub fn bone_table(bytes: &[u8]) -> Result<Skeleton, FormatError> {
    let r = Le(bytes);
    check_header(&r)?;
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
