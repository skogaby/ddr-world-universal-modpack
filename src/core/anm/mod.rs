//! `core/anm` — the PURE format layer for the DDR 3D animation/model data
//! the Background Dancers feature reads straight from the stock `.arc`
//! members: the ANM family container (`.anm` skeletal clips, `.camanm`
//! cameras), the KTMDL bone table, `.b2it` bone-name tables and `MRL0`
//! resource lists.
//!
//! It is a byte-for-byte port of the verified Python reference codecs
//! (`scripts/anm_dump.py`, `scripts/ktmdl_dump.py`; format record in
//! `docs/3d_model_format_research.md` §3–§6) plus the ONE piece of A3 runtime
//! behaviour the Python side does not model: seeding untracked bones from the
//! bind pose (`pose::seed_local_trs`, A3 `FUN_18013ba50`).
//!
//! Dependency-free by design: no `crate::` imports, no `unsafe`, std only, so
//! `scripts/validate_background_dancers.sh` can `#[path]`-mount `mod.rs` into
//! a host crate and run the suites against Python-generated fixtures
//! (`tests/fixtures/anm/`). Everything engine-facing (arc loading, render
//! items, the camera slot write) lives in `services/scene3d`.
//!
//! Conventions shared by every sub-module:
//! - **Row-vector matrices**: `Mat4 = [f32; 16]` row-major, `p' = p · M`,
//!   translation in elements 12..14 (the KTMDL bind-matrix convention).
//! - Quaternions are `(x, y, z, w)`.
//! - All arithmetic is `f32` (the game's), in the Python operation order
//!   where it affects the last bits.
//! - Every read of FILE bytes is bounds-checked (`Le`), never indexed.

pub mod anm;
pub mod b2it;
pub mod camera;
pub mod ktmdl;
pub mod pose;
pub mod rlist;
pub mod sample;

#[cfg(test)]
mod tests;

// Convenience re-exports for the engine-side consumers (Steps 7/9); the
// cdylib has no external users, so the lint would otherwise fire until the
// director lands.
#[allow(unused_imports)]
pub use anm::{parse, Anm, AnmError, Channel, Track};
#[allow(unused_imports)]
pub use camera::{sample_camera, CamSample};
#[allow(unused_imports)]
pub use pose::{evaluate, seed_local_trs, Skeleton, Trs};
#[allow(unused_imports)]
pub use sample::{clip_time, decode_q48, sample, Sample};

/// Row-major 4×4, row-vector convention (translation at `[12..15]`).
pub type Mat4 = [f32; 16];
pub type Vec3 = [f32; 3];
/// `(x, y, z, w)`.
pub type Quat = [f32; 4];

pub const MAT4_IDENTITY: Mat4 = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];
pub const QUAT_IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

/// Errors shared by the fixed-layout table formats (`b2it`, `rlist`, `ktmdl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// Wrong magic / signature bytes.
    BadMagic,
    /// A field or table runs past the end of the buffer.
    Truncated,
    /// A self-describing size field disagrees with the buffer length.
    SizeMismatch,
    /// A count/offset field is outside any plausible range.
    Malformed,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            FormatError::BadMagic => "bad magic",
            FormatError::Truncated => "truncated",
            FormatError::SizeMismatch => "size field mismatch",
            FormatError::Malformed => "malformed table",
        };
        f.write_str(s)
    }
}

/// Bounds-checked little-endian reader over a byte slice. Every accessor
/// returns `None` past the end instead of panicking — file bytes are
/// untrusted input for the hook DLL.
#[derive(Clone, Copy)]
pub struct Le<'a>(pub &'a [u8]);

impl<'a> Le<'a> {
    #[inline]
    pub fn u8(&self, o: usize) -> Option<u8> {
        self.0.get(o).copied()
    }
    #[inline]
    pub fn u16(&self, o: usize) -> Option<u16> {
        let b = self.0.get(o..o.checked_add(2)?)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
    #[inline]
    pub fn i16(&self, o: usize) -> Option<i16> {
        self.u16(o).map(|v| v as i16)
    }
    #[inline]
    pub fn u32(&self, o: usize) -> Option<u32> {
        let b = self.0.get(o..o.checked_add(4)?)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    #[inline]
    pub fn f32(&self, o: usize) -> Option<f32> {
        self.u32(o).map(f32::from_bits)
    }
    /// `count` consecutive `f32`s starting at `o`.
    pub fn f32s<const N: usize>(&self, o: usize) -> Option<[f32; N]> {
        let mut out = [0.0f32; N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.f32(o.checked_add(i.checked_mul(4)?)?)?;
        }
        Some(out)
    }
    /// NUL-terminated string at `o` (ASCII; non-ASCII bytes are replaced).
    pub fn cstr(&self, o: usize) -> Option<String> {
        let tail = self.0.get(o..)?;
        let end = tail.iter().position(|&b| b == 0)?;
        Some(
            tail[..end]
                .iter()
                .map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' })
                .collect(),
        )
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

// ---------------------------------------------------------------------------
// Small linear algebra — exactly the operations the pose chain and the
// camera recipe need, in the row-vector convention.
// ---------------------------------------------------------------------------

/// `a · b` (row-vector: apply `a` first, then `b`).
pub fn mat_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            let mut acc = 0.0f32;
            for k in 0..4 {
                acc += a[r * 4 + k] * b[k * 4 + c];
            }
            out[r * 4 + c] = acc;
        }
    }
    out
}

/// General 4×4 inverse (cofactor expansion). Returns `None` when the matrix
/// is singular (|det| below `1e-12`), which no stock bind matrix is.
pub fn mat_inverse(m: &Mat4) -> Option<Mat4> {
    let mut inv = [0.0f32; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    for v in inv.iter_mut() {
        *v *= inv_det;
    }
    Some(inv)
}

/// Rotation matrix (3×3 rows) of a unit quaternion in the row-vector
/// convention — `scripts/anm_dump.py::quat_to_rowmat`, the same element
/// order as the game's `FUN_180138780`.
pub fn quat_to_rowmat(q: &Quat) -> [[f32; 3]; 3] {
    let [x, y, z, w] = *q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
        ],
        [
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
        ],
        [
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// The game's matrix → quaternion (`FUN_180190a90`): the classic trace test
/// with the three diagonal fallbacks, in the row-vector convention that is
/// the inverse of [`quat_to_rowmat`]. Applied to whatever 3×3 it is given —
/// A3 feeds it the SCALED bind-derived matrix (see `pose::seed_local_trs`).
pub fn mat_to_quat(m: &Mat4) -> Quat {
    let m00 = m[0];
    let m01 = m[1];
    let m02 = m[2];
    let m10 = m[4];
    let m11 = m[5];
    let m12 = m[6];
    let m20 = m[8];
    let m21 = m[9];
    let m22 = m[10];
    let trace = m11 + m00 + m22;
    let mut q = [0.0f32; 4];
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt();
        let inv = 0.5 / s;
        q[3] = s * 0.5;
        q[0] = (m12 - m21) * inv;
        q[1] = (m20 - m02) * inv;
        q[2] = (m01 - m10) * inv;
    } else if m11 < m00 && m22 < m00 {
        let s = ((m00 - m11) - m22 + 1.0).sqrt();
        let inv = if s != 0.0 { 0.5 / s } else { s * 0.5 };
        q[0] = s * 0.5;
        q[1] = (m10 + m01) * inv;
        q[2] = (m20 + m02) * inv;
        q[3] = (m12 - m21) * inv;
    } else if m22 < m11 {
        let s = ((m11 - m22) - m00 + 1.0).sqrt();
        let inv = if s != 0.0 { 0.5 / s } else { s * 0.5 };
        q[1] = s * 0.5;
        q[0] = (m10 + m01) * inv;
        q[2] = (m21 + m12) * inv;
        q[3] = (m20 - m02) * inv;
    } else {
        let s = ((m22 - m00) - m11 + 1.0).sqrt();
        let inv = if s != 0.0 { 0.5 / s } else { s * 0.5 };
        q[2] = s * 0.5;
        q[0] = (m20 + m02) * inv;
        q[1] = (m21 + m12) * inv;
        q[3] = (m01 - m10) * inv;
    }
    q
}

#[inline]
pub fn vec3_len(v: &Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Unit vector, or the input unchanged when its length is 0.
pub fn vec3_normalize(v: &Vec3) -> Vec3 {
    let n = vec3_len(v);
    if n > 0.0 {
        [v[0] / n, v[1] / n, v[2] / n]
    } else {
        *v
    }
}
