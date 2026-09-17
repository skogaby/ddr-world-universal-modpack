//! Skeleton, bind-pose seeding and the world-matrix chain —
//! `scripts/anm_dump.py::evaluate_pose` (pose → matrix, `FUN_18013bc20` /
//! `FUN_18013c000` / `FUN_18013be60`) plus the A3 bind-pose seed the Python
//! reference does not model (`FUN_18013ba50`; §5.1 "Keys are LOCAL
//! (parent-relative) TRS").
//!
//! Per bone, per frame:
//! ```text
//! local = diag(s) · R(q)          rows scaled by s; translation in row 3
//! non-root: local[r][c] /= parentScale[c]   (Maya segment-scale compensation, columns)
//! world = local · world[parent]   (row-vector)
//! ```
//! Channels without a track keep the SEED: for roots the decomposition of
//! `bindWorld[i]`; for children the decomposition of `P = diag(parentScale) ·
//! bindWorld[i] · inverse(bindWorld[parent])` — the row prescale by the
//! parent's seed scale is what the chain's column division cancels, so an
//! untracked rig evaluates back to its bind pose (exactly for uniform parent
//! scales; the stock rigs are unit-scale). A3 hands the SCALED matrix to its
//! matrix→quaternion routine without normalising; so do we.

use super::anm::{Anm, Channel};
use super::sample::{sample, Sample};
use super::{mat_inverse, mat_mul, mat_to_quat, quat_to_rowmat, vec3_len, Mat4, Quat, Vec3};

/// The bone table of a KTMDL model (`ktmdl::bone_table`), or any synthetic
/// rig. `parents[i] < 0` marks a root; bones are topologically ordered in
/// every stock file (`parent < index`) — a parent index `>= i` is treated as
/// a root by the chain so a malformed table cannot read an unevaluated slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    pub parents: Vec<i16>,
    pub bind_world: Vec<Mat4>,
    pub inverse_bind: Vec<Mat4>,
}

impl Skeleton {
    #[inline]
    pub fn bone_count(&self) -> usize {
        self.parents.len()
    }
    /// Parent index of bone `i`, or `None` for roots / malformed entries.
    #[inline]
    pub fn parent_of(&self, i: usize) -> Option<usize> {
        let p = *self.parents.get(i)?;
        if p < 0 {
            return None;
        }
        let p = p as usize;
        if p < i && p < self.parents.len() {
            Some(p)
        } else {
            None
        }
    }
}

/// Local translation / rotation / scale of one bone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trs {
    pub q: Quat,
    pub t: Vec3,
    pub s: Vec3,
}

impl Trs {
    pub const IDENTITY: Trs = Trs {
        q: [0.0, 0.0, 0.0, 1.0],
        t: [0.0, 0.0, 0.0],
        s: [1.0, 1.0, 1.0],
    };
}

/// `diag(s) · R(q)` with `t` in row 3 — `FUN_18013c000` over `FUN_180138780`
/// (`anm_dump.py::_mat4`).
pub fn trs_to_local(trs: &Trs) -> Mat4 {
    let r = quat_to_rowmat(&trs.q);
    let mut m = [0.0f32; 16];
    for i in 0..3 {
        for j in 0..3 {
            m[i * 4 + j] = r[i][j] * trs.s[i];
        }
    }
    m[12] = trs.t[0];
    m[13] = trs.t[1];
    m[14] = trs.t[2];
    m[15] = 1.0;
    m
}

/// Row lengths of the upper 3×3 (`FUN_180190ce0`).
fn row_scales(m: &Mat4) -> Vec3 {
    [
        vec3_len(&[m[0], m[1], m[2]]),
        vec3_len(&[m[4], m[5], m[6]]),
        vec3_len(&[m[8], m[9], m[10]]),
    ]
}

/// The bind-derived local TRS every bone starts from (A3 `FUN_18013ba50`).
/// Bones whose parent bind matrix is singular fall back to their own bind
/// world (as if root) — cannot happen on stock data.
pub fn seed_local_trs(sk: &Skeleton) -> Vec<Trs> {
    let n = sk.bone_count();
    let mut out: Vec<Trs> = Vec::with_capacity(n);
    for i in 0..n {
        let bind = match sk.bind_world.get(i) {
            Some(b) => b,
            None => {
                out.push(Trs::IDENTITY);
                continue;
            }
        };
        let parent = sk
            .parent_of(i)
            .and_then(|p| Some((p, sk.bind_world.get(p)?)));
        let trs = match parent {
            None => Trs {
                q: mat_to_quat(bind),
                t: [bind[12], bind[13], bind[14]],
                s: row_scales(bind),
            },
            Some((p, parent_bind)) => match mat_inverse(parent_bind) {
                None => Trs {
                    q: mat_to_quat(bind),
                    t: [bind[12], bind[13], bind[14]],
                    s: row_scales(bind),
                },
                Some(inv_parent) => {
                    // M = bindWorld[i] · inverse(bindWorld[parent])
                    let m = mat_mul(bind, &inv_parent);
                    let t = [m[12], m[13], m[14]];
                    // P = diag(parentScale) · M  (rows scaled)
                    let ps = out.get(p).map(|o| o.s).unwrap_or([1.0, 1.0, 1.0]);
                    let mut pm = m;
                    for r in 0..3 {
                        for c in 0..3 {
                            pm[r * 4 + c] = m[r * 4 + c] * ps[r];
                        }
                    }
                    Trs {
                        q: mat_to_quat(&pm),
                        t,
                        s: row_scales(&pm),
                    }
                }
            },
        };
        out.push(trs);
    }
    out
}

/// Evaluate the clip at `frame` (already mapped by `sample::clip_time`) into
/// world matrices. `seed` supplies every untracked channel (normally
/// [`seed_local_trs`]; `Trs::IDENTITY`s reproduce the Python reference).
/// Tracks targeting `>= bone_count` are skipped, like the game. `out_world`
/// receives `min(bone_count, out_world.len())` matrices; `scratch` must hold
/// at least `bone_count` entries and is overwritten (no allocation here).
pub fn evaluate_into(
    anm: &Anm,
    bytes: &[u8],
    frame: f32,
    sk: &Skeleton,
    seed: &[Trs],
    scratch: &mut [Trs],
    out_world: &mut [Mat4],
) {
    let n = sk.bone_count().min(scratch.len()).min(out_world.len());
    for i in 0..n {
        scratch[i] = seed.get(i).copied().unwrap_or(Trs::IDENTITY);
    }
    for t in &anm.bone_tracks {
        let b = t.target as usize;
        if b >= n {
            continue;
        }
        let Some(v) = sample(bytes, t, frame) else {
            continue;
        };
        match (t.channel, v) {
            (Channel::Rotation, Sample::Quat(q)) => scratch[b].q = q,
            (Channel::Translation, Sample::Vec3(p)) => scratch[b].t = p,
            (Channel::Scale, Sample::Vec3(s)) => scratch[b].s = s,
            _ => {}
        }
    }
    for i in 0..n {
        let mut local = trs_to_local(&scratch[i]);
        match sk.parent_of(i) {
            None => out_world[i] = local,
            Some(p) => {
                let ps = scratch[p].s;
                for r in 0..3 {
                    for c in 0..3 {
                        let d = ps[c];
                        if d != 0.0 {
                            local[r * 4 + c] /= d;
                        }
                    }
                }
                out_world[i] = mat_mul(&local, &out_world[p]);
            }
        }
    }
}

/// Allocating convenience over [`evaluate_into`] (init-time / tests).
pub fn evaluate(anm: &Anm, bytes: &[u8], frame: f32, sk: &Skeleton, seed: &[Trs]) -> Vec<Mat4> {
    let n = sk.bone_count();
    let mut scratch = vec![Trs::IDENTITY; n];
    let mut out = vec![super::MAT4_IDENTITY; n];
    evaluate_into(anm, bytes, frame, sk, seed, &mut scratch, &mut out);
    out
}

/// Per-bone local TRS after applying the clip (the pre-chain state) — what
/// the fixture tests compare for partially-tracked clips.
pub fn sampled_trs(
    anm: &Anm,
    bytes: &[u8],
    frame: f32,
    bone_count: usize,
    seed: &[Trs],
) -> Vec<Trs> {
    let mut scratch: Vec<Trs> = (0..bone_count)
        .map(|i| seed.get(i).copied().unwrap_or(Trs::IDENTITY))
        .collect();
    for t in &anm.bone_tracks {
        let b = t.target as usize;
        if b >= bone_count {
            continue;
        }
        let Some(v) = sample(bytes, t, frame) else {
            continue;
        };
        match (t.channel, v) {
            (Channel::Rotation, Sample::Quat(q)) => scratch[b].q = q,
            (Channel::Translation, Sample::Vec3(p)) => scratch[b].t = p,
            (Channel::Scale, Sample::Vec3(s)) => scratch[b].s = s,
            _ => {}
        }
    }
    scratch
}
