//! Pure math behind the director (design §4.3.4) — std-only so the host
//! harness mounts it.
//!
//! - dancer body world = `diag(s,s,s,1) · T(x, 0, 0)` with `s` = rlist
//!   `model_scale` and `x = (i − (n−1)·0.5)·1.6` (A3 placement);
//! - stage parts sit at the origin (identity);
//! - clip → frame: `clip_time(local_t, dur, loops)` then `× fps`.

use super::selection::dancer_x;

pub type Mat4 = [f32; 16];

pub const IDENTITY: Mat4 = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Row-vector `diag(s,s,s,1)` with translation `(x, y, z)` in row 3.
pub fn scale_translation(s: f32, x: f32, y: f32, z: f32) -> Mat4 {
    [
        s, 0.0, 0.0, 0.0, //
        0.0, s, 0.0, 0.0, //
        0.0, 0.0, s, 0.0, //
        x, y, z, 1.0,
    ]
}

/// Dancer `i` of `n`: uniform rlist scale, A3 lateral pitch, on the floor.
pub fn body_world(model_scale: f32, i: usize, n: usize) -> Mat4 {
    scale_translation(model_scale, dancer_x(i, n), 0.0, 0.0)
}

/// Stage parts: identity (A3 places every `gm_*` node at the origin).
pub fn stage_world() -> Mat4 {
    IDENTITY
}

/// Map a clip-local time onto the clip and convert to a frame index:
/// looping clips wrap, one-shots clamp at their last frame.
pub fn clip_frame(local_t: f32, duration_s: f32, fps: f32, loops: bool) -> f32 {
    let (ct, _) = clip_time(local_t, duration_s, loops);
    ct * fps
}

/// `core::anm::sample::clip_time`, repeated here so this file stays
/// harness-mountable (identical arithmetic; pinned by a test in the
/// director's engine-side module against the real one).
pub fn clip_time(t: f32, dur: f32, loops: bool) -> (f32, bool) {
    if !(dur > 0.0) {
        return (0.0, true);
    }
    if loops {
        let mut r = t % dur;
        if r < 0.0 {
            r += dur;
        }
        (r, false)
    } else if t <= 0.0 {
        (0.0, false)
    } else if t >= dur {
        (dur, true)
    } else {
        (t, false)
    }
}

// ---------------------------------------------------------------------------
// Step 8: part attachment + shadow (A3 `FUN_18005d5d0` / `FUN_18005e560`)
// ---------------------------------------------------------------------------

/// `a · b` in the row-vector convention: apply `a` first, then `b`. A local
/// copy of `core::anm::mat_mul` (pinned equal by a test in `director.rs`) so
/// this file stays harness-mountable.
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

/// `p · m` for a point (`w = 1`), returning the first three components.
pub fn transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    [
        p[0] * m[0] + p[1] * m[4] + p[2] * m[8] + m[12],
        p[0] * m[1] + p[1] * m[5] + p[2] * m[9] + m[13],
        p[0] * m[2] + p[1] * m[6] + p[2] * m[10] + m[14],
    ]
}

/// The right-forearm mirror: A3 builds `Scale(s) · MirrorX · RotX(π)` whose
/// product is the point inversion `diag(−s, −s, −s, 1)`; with the body
/// scale carried by `body_world` the part-side factor is `diag(−1, −1, −1, 1)`.
pub const MIRROR: Mat4 = [
    -1.0, 0.0, 0.0, 0.0, //
    0.0, -1.0, 0.0, 0.0, //
    0.0, 0.0, -1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// World matrix of a rigid part attached to `bone` (the body's MODEL-space
/// animated bone matrix): `E · bone · body_world`, `E = I` or [`MIRROR`].
/// Parts are authored in their attach bone's bind frame (format doc §8), so
/// at rest (`bone = bind`) a vertex lands at `v · E · Bind[attach] · body`.
pub fn part_world(mirror: bool, bone: &Mat4, body_world: &Mat4) -> Mat4 {
    if mirror {
        mat_mul(&mat_mul(&MIRROR, bone), body_world)
    } else {
        mat_mul(bone, body_world)
    }
}

/// Shadow rule constants (A3 `FUN_18005d5d0`, research `a3-runtime-rules.md` §5).
pub const SHADOW_FLOOR_Y: f32 = 0.02;
pub const SHADOW_SPREAD_GAIN: f32 = 1.5;
pub const SHADOW_MAX: f32 = 2.0;
pub const SHADOW_LOWPASS: f32 = 0.1;
pub const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// One frame of the A3 shadow rule. `ground` = the ground bones' MODEL-space
/// translations (their `y` is replaced by [`SHADOW_FLOOR_Y`]); `bind_hips_y`
/// / `anim_hips_y` = the Hips bone's bind vs animated MODEL-space height.
/// Returns `(centre, target size)`:
/// `centre = mean`, `spread = max ‖p − centre‖`,
/// `u = 1 + (bind − anim)`, `h = u ≤ 1 ? u² : (u − 1)² + 1` (a crouch grows
/// the shadow, a jump shrinks it), `target = clamp(h · clamp(1 + 1.5·spread,
/// 1, 2), 0, 2) · shadow_scale`. `None` for an empty ground set.
pub fn shadow_target(
    ground: &[[f32; 3]],
    bind_hips_y: f32,
    anim_hips_y: f32,
    shadow_scale: f32,
) -> Option<([f32; 3], f32)> {
    if ground.is_empty() {
        return None;
    }
    let n = ground.len() as f32;
    let mut centre = [0.0f32; 3];
    for p in ground {
        centre[0] += p[0];
        centre[1] += SHADOW_FLOOR_Y;
        centre[2] += p[2];
    }
    centre[0] /= n;
    centre[1] /= n;
    centre[2] /= n;
    let mut spread = 0.0f32;
    for p in ground {
        let dx = p[0] - centre[0];
        let dy = SHADOW_FLOOR_Y - centre[1];
        let dz = p[2] - centre[2];
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d > spread {
            spread = d;
        }
    }
    let spread_factor = (1.0 + SHADOW_SPREAD_GAIN * spread).clamp(1.0, SHADOW_MAX);
    let u = 1.0 + (bind_hips_y - anim_hips_y);
    let h = if u <= 1.0 {
        u * u
    } else {
        (u - 1.0) * (u - 1.0) + 1.0
    };
    let target = (h * spread_factor).clamp(0.0, SHADOW_MAX) * shadow_scale;
    Some((centre, target))
}

/// The per-frame low-pass: `prev + 0.1·(target − prev)`.
pub fn shadow_step(prev: f32, target: f32) -> f32 {
    prev + SHADOW_LOWPASS * (target - prev)
}

/// The shadow quad's world: `diag(size) · T(centre_world)` (the quad is a
/// 1×1 square on `y = 0` about the origin).
pub fn shadow_world(size: f32, centre_world: [f32; 3]) -> Mat4 {
    scale_translation(size, centre_world[0], centre_world[1], centre_world[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: &Mat4, b: &Mat4, eps: f32) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= eps)
    }

    /// A rotation about Y by 90° with a translation — row-vector layout.
    fn rot_y90_t(t: [f32; 3]) -> Mat4 {
        [
            0.0, 0.0, -1.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, 0.0, //
            t[0], t[1], t[2], 1.0,
        ]
    }

    #[test]
    fn mat_mul_identity_and_order() {
        let m = rot_y90_t([1.0, 2.0, 3.0]);
        assert!(approx(&mat_mul(&IDENTITY, &m), &m, 0.0));
        assert!(approx(&mat_mul(&m, &IDENTITY), &m, 0.0));
        // apply T(1,0,0) then S(2): the translation is scaled
        let t = scale_translation(1.0, 1.0, 0.0, 0.0);
        let s = scale_translation(2.0, 0.0, 0.0, 0.0);
        let ts = mat_mul(&t, &s);
        assert_eq!(ts[12], 2.0);
        // apply S(2) then T(1,0,0): it is not
        let st = mat_mul(&s, &t);
        assert_eq!(st[12], 1.0);
        // associativity
        let a = rot_y90_t([0.5, 0.0, 0.0]);
        let l = mat_mul(&mat_mul(&a, &t), &s);
        let r = mat_mul(&a, &mat_mul(&t, &s));
        assert!(approx(&l, &r, 1e-6));
    }

    #[test]
    fn transform_point_row_vector() {
        let m = rot_y90_t([10.0, 0.0, 0.0]);
        // (1,0,0) · R_y90 = (0,0,-1), then + t
        let p = transform_point(&m, [1.0, 0.0, 0.0]);
        assert!((p[0] - 10.0).abs() < 1e-6 && p[1].abs() < 1e-6 && (p[2] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn part_world_rest_pose_is_the_bind() {
        let bind = rot_y90_t([0.1, 1.5, 0.0]);
        assert!(approx(&part_world(false, &bind, &IDENTITY), &bind, 0.0));
    }

    #[test]
    fn part_world_mirror_negates_the_rotation_rows_only() {
        let bind = rot_y90_t([0.1, 1.5, 0.0]);
        let m = part_world(true, &bind, &IDENTITY);
        for r in 0..3 {
            for c in 0..3 {
                assert!((m[r * 4 + c] + bind[r * 4 + c]).abs() < 1e-6);
            }
        }
        assert_eq!(&m[12..15], &bind[12..15]);
        assert_eq!(m[15], 1.0);
        // a part vertex lands at v · diag(-1) · bind: the point inversion
        let v = transform_point(&m, [0.3, 0.0, 0.0]);
        let w = transform_point(&bind, [-0.3, 0.0, 0.0]);
        assert!(v.iter().zip(w.iter()).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn part_world_scales_once_through_the_body() {
        let bone = rot_y90_t([0.2, 1.4, -0.1]);
        let body = body_world(0.5, 1, 2); // s = 0.5, x = +0.8
        let m = part_world(false, &bone, &body);
        // translation = s·t + (x, 0, 0)
        assert!((m[12] - (0.5 * 0.2 + 0.8)).abs() < 1e-6);
        assert!((m[13] - 0.5 * 1.4).abs() < 1e-6);
        assert!((m[14] - 0.5 * -0.1).abs() < 1e-6);
        // upper 3×3 = s·R (never s²)
        for r in 0..3 {
            for c in 0..3 {
                assert!((m[r * 4 + c] - 0.5 * bone[r * 4 + c]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn shadow_rest_pose_is_the_rlist_scale() {
        let (centre, target) = shadow_target(&[[0.0, 0.9, 0.0]], 0.9, 0.9, 0.75).unwrap();
        assert_eq!(centre, [0.0, SHADOW_FLOOR_Y, 0.0]);
        assert!((target - 0.75).abs() < 1e-6);
        assert!(shadow_target(&[], 0.0, 0.0, 1.0).is_none());
    }

    #[test]
    fn shadow_centre_y_is_the_floor_regardless_of_input() {
        let (centre, _) =
            shadow_target(&[[1.0, 5.0, 1.0], [-1.0, -3.0, -1.0]], 0.0, 0.0, 1.0).unwrap();
        assert!(centre[0].abs() < 1e-6 && centre[2].abs() < 1e-6);
        assert!((centre[1] - SHADOW_FLOOR_Y).abs() < 1e-6);
    }

    #[test]
    fn shadow_spread_factor_clamps_at_two() {
        // two points 0.2 apart: spread 0.1 → factor 1.15
        let (_, t) = shadow_target(&[[0.1, 0.0, 0.0], [-0.1, 0.0, 0.0]], 0.0, 0.0, 1.0).unwrap();
        assert!((t - 1.15).abs() < 1e-5);
        // spread 1.0 → factor clamps to 2
        let (_, t) = shadow_target(&[[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]], 0.0, 0.0, 1.0).unwrap();
        assert!((t - 2.0).abs() < 1e-6);
    }

    #[test]
    fn shadow_height_branches_and_clamp() {
        // crouch by 1 m: u = 2, h = 2 → target 2 (clamp), × scale
        let (_, t) = shadow_target(&[[0.0, 0.0, 0.0]], 1.0, 0.0, 0.5).unwrap();
        assert!((t - 1.0).abs() < 1e-6);
        // crouch by 0.5: u = 1.5, h = 1.25
        let (_, t) = shadow_target(&[[0.0, 0.0, 0.0]], 1.0, 0.5, 1.0).unwrap();
        assert!((t - 1.25).abs() < 1e-6);
        // jump by 0.5: u = 0.5, h = 0.25
        let (_, t) = shadow_target(&[[0.0, 0.0, 0.0]], 1.0, 1.5, 1.0).unwrap();
        assert!((t - 0.25).abs() < 1e-6);
        // jump by 1: h = 0
        let (_, t) = shadow_target(&[[0.0, 0.0, 0.0]], 1.0, 2.0, 1.0).unwrap();
        assert!(t.abs() < 1e-6);
        // crouch by 3 with max spread: 10 · 2 clamps to 2
        let (_, t) = shadow_target(&[[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]], 3.0, 0.0, 1.0).unwrap();
        assert!((t - 2.0).abs() < 1e-6);
    }

    #[test]
    fn shadow_lowpass_converges() {
        let mut s = 0.0f32;
        for _ in 0..100 {
            s = shadow_step(s, 1.5);
        }
        assert!((s - 1.5).abs() < 1e-3);
        assert!((shadow_step(1.0, 2.0) - 1.1).abs() < 1e-6);
    }

    #[test]
    fn shadow_world_is_scale_then_translate() {
        let m = shadow_world(0.75, [0.8, 0.02, -0.1]);
        assert_eq!(m[0], 0.75);
        assert_eq!(m[5], 0.75);
        assert_eq!(m[10], 0.75);
        assert_eq!(&m[12..15], &[0.8, 0.02, -0.1]);
        // a quad corner (0.5, 0, 0.5) lands at centre + 0.375
        let p = transform_point(&m, [0.5, 0.0, 0.5]);
        assert!((p[0] - 1.175).abs() < 1e-6 && (p[2] - 0.275).abs() < 1e-6);
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::*;

    #[test]
    fn body_world_placement_and_scale() {
        let solo = body_world(0.9, 0, 1);
        assert_eq!(solo[0], 0.9);
        assert_eq!(solo[5], 0.9);
        assert_eq!(solo[10], 0.9);
        assert_eq!(solo[15], 1.0);
        assert_eq!(solo[12], 0.0);
        assert_eq!(solo[13], 0.0);
        let left = body_world(1.0, 0, 2);
        let right = body_world(0.65, 1, 2);
        assert!((left[12] + 0.8).abs() < 1e-6);
        assert!((right[12] - 0.8).abs() < 1e-6);
        assert_eq!(right[0], 0.65);
        // the scale never touches the translation
        assert_eq!(right[13], 0.0);
        assert_eq!(right[14], 0.0);
        assert_eq!(stage_world(), IDENTITY);
    }

    #[test]
    fn clip_frame_wraps_and_clamps() {
        // 60 fps, 2 s loop
        assert!((clip_frame(0.5, 2.0, 60.0, true) - 30.0).abs() < 1e-4);
        assert!((clip_frame(2.5, 2.0, 60.0, true) - 30.0).abs() < 1e-4);
        assert!((clip_frame(-0.5, 2.0, 60.0, true) - 90.0).abs() < 1e-4);
        // one-shot clamps at the end
        assert_eq!(clip_frame(2.5, 2.0, 60.0, false), 120.0);
        assert_eq!(clip_frame(-1.0, 2.0, 60.0, false), 0.0);
        // degenerate duration
        assert_eq!(clip_frame(1.0, 0.0, 60.0, true), 0.0);
    }
}
