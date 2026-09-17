//! `.camanm` → camera sample — the game's CameraNode recipe
//! (`docs/3d_model_format_research.md` §6, `FUN_18001cb50` +
//! `camera_set_perspective_fov_aspect`, VERIFIED in-game on A3 2026-09-15;
//! `tools/blender_ddr_addon/import_anm.py::game_camera_half_tangent`).
//!
//! Six fixed slots: 0 quat `(x,y,z,w)`, 1 position in CENTIMETRES, 2 Maya
//! vertical angle of view in DEGREES, 3 near, 4 far, 5 Maya film aspect.
//! ```text
//! R      = quat_to_rowmat(q)
//! eye    = pos · 0.01
//! target = eye − 10.0 · R.row2        (the A3 1000·row2 shifted by the same 0.01)
//! up     = normalize(R.row1)
//! t'     = tan(½ · atan2(2, 2·tan(fovV·π/360)·aspect_file·aspect_mul))
//! l/r    = ∓t'        b/t = ∓t'/(16/9)
//! near   = slot3 · near_mul   far = slot4
//! ```
//! `t'` DECREASES as the file FOV grows (`atan2(2, r−l)` is `90° − hFOV/2`) —
//! surprising but decompiled and cabinet-confirmed; stock cameras were tuned
//! against the in-game result.

use super::anm::Anm;
use super::sample::{sample, Sample};
use super::{quat_to_rowmat, vec3_normalize, Quat, Vec3};

/// Slot defaults when a `.camanm` omits a slot (`DAT_*` fallbacks of the
/// CameraNode; the stock files always carry all six).
pub const DEFAULT_FOV_V_DEG: f32 = 41.53;
pub const DEFAULT_NEAR: f32 = 0.1;
pub const DEFAULT_FAR: f32 = 10000.0;
pub const DEFAULT_ASPECT_FILE: f32 = 4.0 / 3.0;
/// The game re-projects every camera to 16:9 (`DAT_180265264`).
pub const OUTPUT_ASPECT: f32 = 16.0 / 9.0;
/// Centimetres → metres (`DAT_1802921e8`).
pub const POS_SCALE: f32 = 0.01;
/// `1000 · 0.01`: the look-at distance along the local −Z axis.
pub const TARGET_DISTANCE: f32 = 10.0;

/// The same field shape as `services::scene3d::scene_graph::CamSample`
/// (kept structurally identical so the camera director can hand it over
/// field-for-field).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamSample {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub l: f32,
    pub r: f32,
    pub b: f32,
    pub t: f32,
    pub near: f32,
    pub far: f32,
}

/// The raw six slot values at a frame (absent slots = defaults / identity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamSlots {
    pub q: Quat,
    pub pos_cm: Vec3,
    pub fov_v_deg: f32,
    pub near: f32,
    pub far: f32,
    pub aspect_file: f32,
    /// Which slots were actually present in the file.
    pub present: [bool; 6],
}

impl Default for CamSlots {
    fn default() -> Self {
        CamSlots {
            q: [0.0, 0.0, 0.0, 1.0],
            pos_cm: [0.0, 0.0, 0.0],
            fov_v_deg: DEFAULT_FOV_V_DEG,
            near: DEFAULT_NEAR,
            far: DEFAULT_FAR,
            aspect_file: DEFAULT_ASPECT_FILE,
            present: [false; 6],
        }
    }
}

/// The horizontal half-tangent the GAME ends up with for a file FOV / aspect
/// pair — `import_anm.py::game_camera_half_tangent` (with the extra
/// `aspect_mul` the music-camera rlist rows can carry; 1.0 for stage sets).
pub fn half_tangent(fov_v_deg: f32, aspect_file: f32, aspect_mul: f32) -> f32 {
    let h = (fov_v_deg.to_radians() * 0.5).tan() * aspect_file * aspect_mul;
    let fov_prime = 2.0f32.atan2(2.0 * h);
    (0.5 * fov_prime).tan()
}

/// Sample the six camera slots at `frame`.
pub fn sample_slots(anm: &Anm, bytes: &[u8], frame: f32) -> CamSlots {
    let mut s = CamSlots::default();
    for (slot, track) in anm.camera_slots.iter().enumerate() {
        let Some(t) = track else { continue };
        let Some(v) = sample(bytes, t, frame) else {
            continue;
        };
        match (slot, v) {
            (0, Sample::Quat(q)) => s.q = q,
            (1, Sample::Vec3(p)) => s.pos_cm = p,
            (2, Sample::Scalar(x)) => s.fov_v_deg = x,
            (3, Sample::Scalar(x)) => s.near = x,
            (4, Sample::Scalar(x)) => s.far = x,
            (5, Sample::Scalar(x)) => s.aspect_file = x,
            _ => continue,
        }
        s.present[slot] = true;
    }
    s
}

/// The recipe over already-sampled slots.
pub fn camera_from_slots(s: &CamSlots, near_mul: f32, aspect_mul: f32) -> CamSample {
    let r = quat_to_rowmat(&s.q);
    let eye = [
        s.pos_cm[0] * POS_SCALE,
        s.pos_cm[1] * POS_SCALE,
        s.pos_cm[2] * POS_SCALE,
    ];
    let target = [
        eye[0] - TARGET_DISTANCE * r[2][0],
        eye[1] - TARGET_DISTANCE * r[2][1],
        eye[2] - TARGET_DISTANCE * r[2][2],
    ];
    let up = vec3_normalize(&[r[1][0], r[1][1], r[1][2]]);
    let tp = half_tangent(s.fov_v_deg, s.aspect_file, aspect_mul);
    let tv = tp / OUTPUT_ASPECT;
    CamSample {
        eye,
        target,
        up,
        l: -tp,
        r: tp,
        b: -tv,
        t: tv,
        near: s.near * near_mul,
        far: s.far,
    }
}

/// `.camanm` frame → camera sample. `near_mul` / `aspect_mul` are the
/// music-camera rlist multipliers (both 1.0 for the stage camera sets).
pub fn sample_camera(
    anm: &Anm,
    bytes: &[u8],
    frame: f32,
    near_mul: f32,
    aspect_mul: f32,
) -> CamSample {
    camera_from_slots(&sample_slots(anm, bytes, frame), near_mul, aspect_mul)
}
