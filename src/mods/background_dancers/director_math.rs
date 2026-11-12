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

/// Name prefix of a FLIGHT stage's launch-platform parts (shown until the
/// take-off ends).
pub const PRE_PART_PREFIX: &str = "pre_";
/// Name prefix of a FLIGHT stage's tunnel parts (shown from the end of the
/// take-off, their clock starting there).
pub const FLY_PART_PREFIX: &str = "fly_";
/// The take-off length assumed when a flight stage has no dancer to time it
/// (the BACKGROUND STAGE preview): the ported take-off, 600 frames @ 60.
pub const DEFAULT_TAKEOFF_S: f32 = 10.0;

/// A flight stage part's phase (by its `map_resources` part name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartPhase {
    /// Shown throughout, on the scene clock.
    Always,
    /// `pre_*`: the launch platform.
    Pre,
    /// `fly_*`: the tunnel.
    Fly,
}

pub fn part_phase(part: &str) -> PartPhase {
    if part.starts_with(PRE_PART_PREFIX) {
        PartPhase::Pre
    } else if part.starts_with(FLY_PART_PREFIX) {
        PartPhase::Fly
    } else {
        PartPhase::Always
    }
}

/// `(shown, clock)` of a part of `phase` at scene time `t` when the flight
/// starts at `switch` (the take-off's end; `None` = it never starts — a
/// flight stage nobody can fly keeps its platform): `Pre` until the switch,
/// `Fly` from it on its own clock `t − switch` (a one-shot opening then
/// starts at the take-off instead of the song start).
pub fn phase_clock(phase: PartPhase, switch: Option<f32>, t: f32) -> (bool, f32) {
    match (phase, switch) {
        (PartPhase::Always, _) => (true, t),
        (PartPhase::Pre, None) => (true, t),
        (PartPhase::Pre, Some(s)) => (t < s, t),
        (PartPhase::Fly, None) => (false, 0.0),
        (PartPhase::Fly, Some(s)) => (t >= s, (t - s).max(0.0)),
    }
}

/// The launch-platform roles of a FLIGHT stage's `pre_*` parts, named by the
/// port (`port_stage_hottest2.FLIGHT_PHASES`) after the zan stage objects the
/// flight intro drives: `pre_plat` (STG109's stage, object 5), `pre_sky`
/// (STG109's sky + sea, object 3), `pre_hole` (the tunnel mouth, object 6);
/// `pre_space` (object 4) and any other `pre_*` part just show until the
/// switch.
pub const INTRO_PLATFORM: &str = "pre_plat";
pub const INTRO_SKY: &str = "pre_sky";
pub const INTRO_HOLE: &str = "pre_hole";

/// How a `pre_*` part looks at intro time `t` (seconds of the take-off).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntroLook {
    /// The instance tint (rgb dim, alpha fade).
    pub tint: [f32; 4],
    /// Drawn at all (the game hides a faded-out object: alpha ≤ 1e-5).
    pub shown: bool,
    /// The part's own clip clock (the tunnel mouth's opening starts late).
    pub clock: f32,
}

/// MUSIC FIT's flight intro, ported from main.dol: a song whose play setup
/// carries flag 0x200 arms it at the song start (`FUN_8003729c`: tunnel +
/// space hidden, platform / sky / intro space shown, the mouth hidden) and
/// `FUN_80037354` runs it per 60 fps frame `f` of the intro clock:
/// the platform's colour `0.85·(210 − f)/150 + 0.15` over f 60..210 (dims to
/// 0.15), the mouth fades in over f 300..360 and starts its (one-shot)
/// motion at 300, the sky fades out over f 360..420; the switch to the
/// tunnel is the end of the intro camera (STG201_CAM00_01..03, 3 + 4 + 3 s =
/// the 600-frame take-off). Here `f = 60·t` on the take-off's own clock, so
/// the script stays in step with the dancer at any tempo.
pub fn intro_look(part: &str, t: f32) -> IntroLook {
    let is = |role: &str| {
        part.strip_prefix(role).is_some_and(|rest| {
            rest.is_empty()
                || rest.starts_with('_')
                || rest.starts_with(|c: char| c.is_ascii_digit())
        })
    };
    let mut look = IntroLook {
        tint: WHITE,
        shown: true,
        clock: t,
    };
    if is(INTRO_PLATFORM) {
        let v = ramp(t, 1.0, 1.0, 3.5, 0.15);
        look.tint = [v, v, v, 1.0];
    } else if is(INTRO_SKY) {
        let a = ramp(t, 6.0, 1.0, 7.0, 0.0);
        look.tint[3] = a;
        look.shown = a > 1e-5;
    } else if is(INTRO_HOLE) {
        let a = ramp(t, 5.0, 0.0, 6.0, 1.0);
        look.tint[3] = a;
        look.shown = a > 1e-5;
        look.clock = (t - 5.0).max(0.0);
    }
    look
}

/// `v0` before `t0`, `v1` after `t1`, linear between.
fn ramp(t: f32, t0: f32, v0: f32, t1: f32, v1: f32) -> f32 {
    if t <= t0 {
        v0
    } else if t >= t1 {
        v1
    } else {
        v0 + (v1 - v0) * (t - t0) / (t1 - t0)
    }
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

// ---------------------------------------------------------------------------
// Big Head (docs/big_head_mode_feasibility.md §1)
// ---------------------------------------------------------------------------

/// The Big Head factor: every dancer's `Head` subtree is drawn this many
/// times its size about the Head joint.
pub const BIG_HEAD_SCALE: f32 = 3.0;

/// `root` and every bone below it, root first, for a parent-first bone table
/// (`parents[i] < i`; a negative or `>= i` parent is a root — the rule of
/// `core::anm::pose::Skeleton::parent_of`). Empty when `root` is out of range.
/// One pass; built once per parse, never per frame.
pub fn subtree_of(parents: &[i16], root: usize) -> Vec<usize> {
    if root >= parents.len() {
        return Vec::new();
    }
    let mut member = vec![false; parents.len()];
    member[root] = true;
    let mut out = vec![root];
    for (i, &p) in parents.iter().enumerate().skip(root + 1) {
        let p = p as isize;
        if p >= 0 && (p as usize) < i && member.get(p as usize).copied().unwrap_or(false) {
            if let Some(m) = member.get_mut(i) {
                *m = true;
            }
            out.push(i);
        }
    }
    out
}

/// Uniform scale `k` about the joint of `subtree[0]`, in MODEL space, applied
/// to every listed bone matrix: `M' = M · C` with the row-vector
/// `C = [k·I 0; j·(1−k) 1]`, `j` = the root's translation. The root keeps its
/// joint position (only its rows 0–2 scale — `diag(k,k,k,1) · M`, a bone-local
/// scale about the joint); descendants move out from the joint by `k` too.
/// Skinning (`invBind · bone`) then maps a rest-pose vertex with root weight
/// `w` to `j + (v − j)(1 + (k − 1)·w)`, and a rigid part hung off the root
/// (`part_world`) scales by `k` about `j` as a whole. `k` must be uniform (the
/// lit / cel shaders' `view_frame` assumes a uniform World scale).
///
/// Per frame on the game thread: no allocation, no panics — an empty list is
/// a no-op and out-of-range indices are skipped.
pub fn scale_subtree_about_root(bones: &mut [Mat4], subtree: &[usize], k: f32) {
    let Some(root) = subtree.first().and_then(|&r| bones.get(r)) else {
        return;
    };
    let j = [root[12], root[13], root[14]];
    for &b in subtree {
        let Some(m) = bones.get_mut(b) else {
            continue;
        };
        for row in m.chunks_exact_mut(4) {
            let w = row[3];
            for (v, jc) in row.iter_mut().zip(j.iter()) {
                *v = k * *v + w * jc * (1.0 - k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn flight_phases_switch_at_the_takeoff_end() {
        use super::{part_phase, phase_clock, PartPhase};
        assert_eq!(part_phase("pre_dec"), PartPhase::Pre);
        assert_eq!(part_phase("fly_add2"), PartPhase::Fly);
        assert_eq!(part_phase("dec"), PartPhase::Always);
        assert_eq!(part_phase("bg"), PartPhase::Always);
        let s = Some(10.0);
        assert_eq!(phase_clock(PartPhase::Pre, s, 9.9), (true, 9.9));
        assert_eq!(phase_clock(PartPhase::Pre, s, 10.0), (false, 10.0));
        assert_eq!(phase_clock(PartPhase::Fly, s, 9.9), (false, 0.0));
        assert_eq!(phase_clock(PartPhase::Fly, s, 12.5), (true, 2.5));
        assert_eq!(phase_clock(PartPhase::Always, s, 3.0), (true, 3.0));
        // no flight: the platform stays, the tunnel never shows
        assert_eq!(phase_clock(PartPhase::Pre, None, 99.0), (true, 99.0));
        assert!(!phase_clock(PartPhase::Fly, None, 99.0).0);
    }

    #[test]
    fn intro_script_dims_fades_and_opens() {
        use super::{intro_look, WHITE};
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        // the platform: full colour to 1 s (frame 60), 0.15 from 3.5 s (210)
        assert_eq!(intro_look("pre_plat_dec", 0.5).tint, WHITE);
        let mid = intro_look("pre_plat_add2", 2.25).tint;
        assert!(near(mid[0], 0.575) && near(mid[3], 1.0));
        assert!(near(intro_look("pre_plat_ble", 9.0).tint[1], 0.15));
        // the sky: gone over 6..7 s (360..420), hidden once faded out
        assert!(intro_look("pre_sky_dec", 5.9).shown);
        assert!(near(intro_look("pre_sky_dec", 6.5).tint[3], 0.5));
        assert!(!intro_look("pre_sky_dec", 7.0).shown);
        // the mouth: hidden to 5 s, in by 6 s, its opening clocked from 5 s
        let h = intro_look("pre_hole_add", 4.0);
        assert!(!h.shown && h.clock == 0.0);
        let h = intro_look("pre_hole_ble2", 5.5);
        assert!(h.shown && near(h.tint[3], 0.5) && near(h.clock, 0.5));
        // other parts (the intro space) and look-alike names are untouched
        let s = intro_look("pre_space_dec", 8.0);
        assert!(s.shown && s.tint == WHITE && s.clock == 8.0);
        assert_eq!(intro_look("pre_skylight_dec", 9.0).tint, WHITE);
    }

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

#[cfg(test)]
mod big_head_tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= EPS)
    }

    /// A rigid bone matrix: rotation about Z by `deg`, then translation `t`.
    fn rigid(deg: f32, t: [f32; 3]) -> Mat4 {
        let (s, c) = deg.to_radians().sin_cos();
        [
            c, s, 0.0, 0.0, //
            -s, c, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            t[0], t[1], t[2], 1.0,
        ]
    }

    /// Inverse of a rigid row-vector matrix: `[Rᵀ 0; −t·Rᵀ 1]`.
    fn rigid_inverse(m: &Mat4) -> Mat4 {
        let mut o = IDENTITY;
        for r in 0..3 {
            for c in 0..3 {
                o[r * 4 + c] = m[c * 4 + r];
            }
        }
        for c in 0..3 {
            o[12 + c] = -(m[12] * o[c] + m[13] * o[4 + c] + m[14] * o[8 + c]);
        }
        o
    }

    /// The model-space scale about `j` as a matrix: `[k·I 0; j·(1−k) 1]`.
    fn about(j: [f32; 3], k: f32) -> Mat4 {
        scale_translation(k, j[0] * (1.0 - k), j[1] * (1.0 - k), j[2] * (1.0 - k))
    }

    fn head_rig() -> Vec<Mat4> {
        // 0 = Neck, 1 = Head (the joint at y 1.48, tilted), 2 = a bone under
        // Head (none exist on stock rigs, but the subtree form must hold).
        vec![
            rigid(0.0, [0.0, 1.39, 0.0]),
            rigid(20.0, [0.02, 1.48, 0.01]),
            rigid(-35.0, [0.05, 1.62, 0.04]),
        ]
    }

    #[test]
    fn root_keeps_its_joint_and_scales_its_rows() {
        let before = head_rig();
        let mut bones = before.clone();
        scale_subtree_about_root(&mut bones, &[1], BIG_HEAD_SCALE);
        let (b, a) = (&before[1], &bones[1]);
        for r in 0..3 {
            for c in 0..3 {
                assert!((a[r * 4 + c] - 3.0 * b[r * 4 + c]).abs() <= EPS);
            }
            assert_eq!(a[r * 4 + 3], 0.0);
        }
        assert_eq!(&a[12..16], &b[12..16]);
        // bones outside the list are untouched
        assert_eq!(bones[0], before[0]);
        assert_eq!(bones[2], before[2]);
    }

    #[test]
    fn every_member_equals_m_times_c() {
        let before = head_rig();
        let mut bones = before.clone();
        scale_subtree_about_root(&mut bones, &[1, 2], 3.0);
        let j = [before[1][12], before[1][13], before[1][14]];
        for b in [1usize, 2] {
            let want = mat_mul(&before[b], &about(j, 3.0));
            assert!(bones[b]
                .iter()
                .zip(want.iter())
                .all(|(x, y)| (x - y).abs() <= EPS));
        }
    }

    #[test]
    fn bone_local_points_move_out_from_the_joint() {
        let before = head_rig();
        let mut bones = before.clone();
        scale_subtree_about_root(&mut bones, &[1, 2], 3.0);
        let j = [before[1][12], before[1][13], before[1][14]];
        for b in [1usize, 2] {
            for p in [[0.0, 0.0, 0.0], [0.1, 0.2, -0.05], [-0.3, 0.0, 0.12]] {
                let v = transform_point(&before[b], p);
                let want = [
                    j[0] + 3.0 * (v[0] - j[0]),
                    j[1] + 3.0 * (v[1] - j[1]),
                    j[2] + 3.0 * (v[2] - j[2]),
                ];
                assert!(close(transform_point(&bones[b], p), want));
            }
        }
        // the joint itself is the fixed point
        assert!(close(transform_point(&bones[1], [0.0, 0.0, 0.0]), j));
    }

    /// At rest (bone = bind) the engine's linear blend of `invBind · bone`
    /// reproduces the A3 offline-bake formula `j + (v − j)(1 + (k−1)·w)`
    /// (docs/3d_model_format_research.md §10) — smooth across the neck.
    #[test]
    fn rest_pose_skinning_matches_the_a3_offline_formula() {
        let bind = head_rig();
        let mut posed = bind.clone();
        scale_subtree_about_root(&mut posed, &[1], 3.0);
        let skin = |i: usize| mat_mul(&rigid_inverse(&bind[i]), &posed[i]);
        let j = [bind[1][12], bind[1][13], bind[1][14]];
        for v in [[0.04, 1.43, 0.08], [0.0, 1.6, 0.1], [-0.07, 1.20, -0.02]] {
            for w in [0.0f32, 0.12, 0.5, 1.0] {
                let a = transform_point(&skin(1), v); // Head
                let b = transform_point(&skin(0), v); // Neck (unscaled: identity at rest)
                let blended = [
                    w * a[0] + (1.0 - w) * b[0],
                    w * a[1] + (1.0 - w) * b[1],
                    w * a[2] + (1.0 - w) * b[2],
                ];
                let f = 1.0 + 2.0 * w;
                let want = [
                    j[0] + (v[0] - j[0]) * f,
                    j[1] + (v[1] - j[1]) * f,
                    j[2] + (v[2] - j[2]) * f,
                ];
                assert!(
                    close(blended, want),
                    "v={v:?} w={w}: {blended:?} vs {want:?}"
                );
            }
        }
    }

    /// A rigid head part (`head00` / `face01`) follows: its world scales by k
    /// about the joint and stays UNIFORM (`|row r| = k · s_body` for every
    /// row — what the lit / cel `view_frame` recovery relies on).
    #[test]
    fn head_parts_scale_uniformly_about_the_joint() {
        let mut bones = head_rig();
        let body = body_world(0.9, 0, 2);
        let rest = part_world(false, &bones[1], &body);
        scale_subtree_about_root(&mut bones, &[1], 3.0);
        let big = part_world(false, &bones[1], &body);
        let jw = transform_point(&rest, [0.0, 0.0, 0.0]);
        let p = [0.1, 0.15, -0.08];
        let (r, g) = (transform_point(&rest, p), transform_point(&big, p));
        for c in 0..3 {
            assert!((g[c] - (jw[c] + 3.0 * (r[c] - jw[c]))).abs() <= EPS);
        }
        for row in 0..3 {
            let n =
                (big[row * 4].powi(2) + big[row * 4 + 1].powi(2) + big[row * 4 + 2].powi(2)).sqrt();
            assert!((n - 3.0 * 0.9).abs() <= EPS, "row {row}: |row| = {n}");
        }
    }

    #[test]
    fn identity_factor_and_degenerate_inputs_are_no_ops() {
        let before = head_rig();
        let mut bones = before.clone();
        scale_subtree_about_root(&mut bones, &[1, 2], 1.0);
        assert_eq!(bones, before);
        scale_subtree_about_root(&mut bones, &[], 3.0);
        assert_eq!(bones, before);
        // an out-of-range root does nothing; out-of-range members are skipped
        scale_subtree_about_root(&mut bones, &[7, 1], 3.0);
        assert_eq!(bones, before);
        scale_subtree_about_root(&mut bones, &[1, 9], 3.0);
        assert_eq!(&bones[1][12..16], &before[1][12..16]);
        assert!((bones[1][0] - 3.0 * before[1][0]).abs() <= EPS);
        let mut empty: Vec<Mat4> = Vec::new();
        scale_subtree_about_root(&mut empty, &[0], 3.0);
    }

    #[test]
    fn subtree_of_walks_a_parent_first_table() {
        // The stock 33-bone rig's shape near the head: Head (16) is a leaf.
        let mut stock = vec![-1i16; 33];
        for (i, p) in stock.iter_mut().enumerate().skip(1) {
            *p = (i as i16) - 1;
        }
        stock[16] = 11;
        stock[17] = 12;
        assert_eq!(subtree_of(&stock, 16), vec![16]);
        //        0
        //       / \
        //      1   2
        //     / \   \
        //    3   4   5
        //        |
        //        6
        let parents = [-1i16, 0, 0, 1, 1, 2, 4];
        assert_eq!(subtree_of(&parents, 1), vec![1, 3, 4, 6]);
        assert_eq!(subtree_of(&parents, 2), vec![2, 5]);
        assert_eq!(subtree_of(&parents, 0), vec![0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(subtree_of(&parents, 6), vec![6]);
        // malformed parents (>= own index) are roots, never members
        assert_eq!(subtree_of(&[-1i16, 0, 5, 1], 0), vec![0, 1, 3]);
        // out of range
        assert!(subtree_of(&parents, 7).is_empty());
        assert!(subtree_of(&[], 0).is_empty());
    }
}
