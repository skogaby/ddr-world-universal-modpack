//! DYNAMIC BONES — DanceDanceRevolution Winx Club's hair / side-lock /
//! ribbon / wing physics, ported from the Wii's own solver (main.dol
//! `FUN_8010d430`, research `docs/wii_ddr_winx_club_research.md` §7.1). PURE
//! (std only, harness-mounted as `dynbones_logic` by
//! `scripts/validate_background_dancers.sh`); the reference it follows
//! number for number is `scripts/zan_dump.py::dynbone_step` (tests share a
//! scenario with `test_zan_formats.TestWinxClub`).
//!
//! Content: `pl_<key>/dynbones.txt` (written by `port_character_hottest2.py`
//! with `GAME=winx`, grammar in [`parse`]): per `DB_*` bone its parent, length,
//! gravity flag, the z / y axis blocks (enable / limit / damping flags, limits,
//! stiffness, damping) and the DBBOX collider ids it ignores; per DBBOX
//! collider a joint-local AABB.
//!
//! The solver is an angle-space LAG SPRING per bone, root first: each 60 Hz
//! step the bone (local +x) is aimed along the lag of its origin (previous −
//! current position in zan units, + gravity), the two angles (yaw about y,
//! pitch about z, in the rest frame `rest local · parent world`) clamped,
//! pushed out of a hit collider, then the current angles move toward that
//! target by `k = 0.06·(dt·60)·speed/20` (≤ 0.99; `(dt·60)·(1 − damping)`
//! with the damping flag — the Wii crosses the blocks: the y block's damping
//! drives the pitch gain, the z block's the yaw gain) and decay toward rest by
//! `1.1 · stiffness · |angle|`. The state is the aim DIRECTION in model space,
//! so a body turn makes the hair lag behind it. Fixed 60 Hz steps (the Wii's
//! tick) with a catch-up cap; between steps the stored aim is re-applied to
//! this frame's parent.
//!
//! EXACT vs APPROXIMATE (research §7.1 "port"): the config fields, the
//! constants, the angle convention, clamps, gains (incl. the crossing) and
//! the decay are the decompile's; the collision (tip vs box, pushed to the
//! nearest face — `FUN_8010dcd4` / `FUN_8010e67c` are not decoded), the
//! teleport rule (a root jump > [`TELEPORT_M`] per step, or the caller's
//! `teleport` flag at a clip cut, reads as no lag — the Wii's `+0x84` offset)
//! and the omitted 5-frame settle blend after a reset are the port's.
//!
//! Matrices are row-major ROW-vector (`v' = v · M`, translation in row 3),
//! model space in metres — the director's `Session::bones`.

/// Row-major 4×4, row-vector convention.
pub type Mat4 = [f32; 16];
type V3 = [f32; 3];

pub const GRAVITY_UNIT: f32 = 0.1;
pub const GRAVITY_MUL: f32 = 3.0;
/// The Wii's step (and the gain normaliser).
pub const STEP_S: f32 = 1.0 / 60.0;
pub const GAIN: f32 = 0.06;
pub const SPEED_DIV: f32 = 20.0;
pub const GAIN_CAP: f32 = 0.99;
pub const DECAY: f32 = 1.1;
pub const COLLIDE_GAIN: f32 = 1.5;
/// PORT: a root moving further than this in one step is a teleport (no lag).
pub const TELEPORT_M: f32 = 0.5;
/// Steps one frame may run at most (a hitch is skipped, not simulated).
pub const MAX_STEPS_PER_FRAME: u32 = 4;
/// Frame dt above this (seconds) is clamped (pause / hitch).
pub const MAX_DT_S: f32 = 0.1;

/// One axis block of the config (`+0x30` z / `+0x50` y).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    pub enable: bool,
    pub limit: bool,
    pub damp: bool,
    pub min: f32,
    pub max: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Axis {
    pub const OFF: Axis = Axis {
        enable: false,
        limit: false,
        damp: false,
        min: 0.0,
        max: 0.0,
        stiffness: 0.0,
        damping: 0.0,
    };
}

/// One `bone` line.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneCfg {
    pub name: String,
    pub parent: String,
    pub len_m: f32,
    pub gravity: bool,
    pub z: Axis,
    pub y: Axis,
    pub exclude: Vec<u8>,
}

/// One `box` line: a joint-local AABB (metres).
#[derive(Debug, Clone, PartialEq)]
pub struct BoxCfg {
    pub id: u8,
    pub joint: String,
    pub min: V3,
    pub max: V3,
}

/// A parsed `dynbones.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct DynFile {
    pub units_per_metre: f32,
    pub bones: Vec<BoneCfg>,
    pub boxes: Vec<BoxCfg>,
}

fn floats<const N: usize>(s: &str) -> Option<[f32; N]> {
    let mut out = [0.0f32; N];
    let mut it = s.split(',');
    for o in out.iter_mut() {
        *o = it
            .next()?
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())?;
    }
    it.next().is_none().then_some(out)
}

fn axis(s: &str) -> Option<Axis> {
    let v: Vec<&str> = s.split(',').collect();
    if v.len() != 7 {
        return None;
    }
    let flag = |x: &str| match x.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    };
    let f = |x: &str| x.trim().parse::<f32>().ok().filter(|v| v.is_finite());
    Some(Axis {
        enable: flag(v[0])?,
        limit: flag(v[1])?,
        damp: flag(v[2])?,
        min: f(v[3])?,
        max: f(v[4])?,
        stiffness: f(v[5])?,
        damping: f(v[6])?,
    })
}

/// Parse the sidecar (grammar: `zan_dump.format_dynbones`):
/// ```text
/// dynbones 1
/// units_per_metre <f>
/// bone <name> parent=<joint> len=<m> gravity=<0|1> z=<en,lim,damp,min,max,stiff,damping>
///      y=<…> exclude=<id,id|-> [rest=<…> informational]
/// box <id> joint=<joint> min=<x,y,z> max=<x,y,z>
/// ```
/// Errors name the line.
pub fn parse(text: &str) -> Result<DynFile, String> {
    let mut out = DynFile {
        units_per_metre: 0.0,
        bones: Vec::new(),
        boxes: Vec::new(),
    };
    let mut version = false;
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let bad = || format!("line {}: {:?}", ln + 1, line);
        let kv = |key: &str| -> Option<&str> {
            f.iter()
                .skip(2)
                .find_map(|x| x.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
        };
        match f.first().copied() {
            Some("dynbones") => {
                if f.get(1).copied() != Some("1") {
                    return Err(format!("{}: unsupported version", bad()));
                }
                version = true;
            }
            Some("units_per_metre") => {
                out.units_per_metre = f
                    .get(1)
                    .and_then(|v| v.parse::<f32>().ok())
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .ok_or_else(bad)?;
            }
            Some("bone") => {
                let name = f.get(1).ok_or_else(bad)?.to_string();
                let exclude = match kv("exclude").ok_or_else(bad)? {
                    "-" => Vec::new(),
                    s => s
                        .split(',')
                        .map(|x| x.trim().parse::<u8>().map_err(|_| bad()))
                        .collect::<Result<Vec<u8>, String>>()?,
                };
                out.bones.push(BoneCfg {
                    name,
                    parent: kv("parent").ok_or_else(bad)?.to_string(),
                    len_m: kv("len")
                        .and_then(|v| v.parse::<f32>().ok())
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .ok_or_else(bad)?,
                    gravity: match kv("gravity") {
                        Some("1") => true,
                        Some("0") => false,
                        _ => return Err(bad()),
                    },
                    z: kv("z").and_then(axis).ok_or_else(bad)?,
                    y: kv("y").and_then(axis).ok_or_else(bad)?,
                    exclude,
                });
            }
            Some("box") => {
                out.boxes.push(BoxCfg {
                    id: f
                        .get(1)
                        .and_then(|v| v.parse::<u8>().ok())
                        .ok_or_else(bad)?,
                    joint: kv("joint").ok_or_else(bad)?.to_string(),
                    min: kv("min").and_then(floats::<3>).ok_or_else(bad)?,
                    max: kv("max").and_then(floats::<3>).ok_or_else(bad)?,
                });
            }
            _ => return Err(format!("{}: unknown record", bad())),
        }
    }
    if !version {
        return Err("no `dynbones 1` header".to_string());
    }
    if out.units_per_metre <= 0.0 {
        return Err("no units_per_metre".to_string());
    }
    Ok(out)
}

// ── small row-vector math (own copies: this file mounts without `crate::`) ──

fn mat_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [0.0f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            let mut s = 0.0f32;
            for k in 0..4 {
                s += a[r * 4 + k] * b[k * 4 + c];
            }
            m[r * 4 + c] = s;
        }
    }
    m
}

/// The inverse of an affine row-vector matrix (`None` when singular).
pub fn affine_inverse(m: &Mat4) -> Option<Mat4> {
    let (a, b, c) = (m[0], m[1], m[2]);
    let (d, e, f) = (m[4], m[5], m[6]);
    let (g, h, i) = (m[8], m[9], m[10]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let k = 1.0 / det;
    let inv = [
        (e * i - f * h) * k,
        (c * h - b * i) * k,
        (b * f - c * e) * k,
        (f * g - d * i) * k,
        (a * i - c * g) * k,
        (c * d - a * f) * k,
        (d * h - e * g) * k,
        (b * g - a * h) * k,
        (a * e - b * d) * k,
    ];
    let (tx, ty, tz) = (m[12], m[13], m[14]);
    Some([
        inv[0],
        inv[1],
        inv[2],
        0.0,
        inv[3],
        inv[4],
        inv[5],
        0.0,
        inv[6],
        inv[7],
        inv[8],
        0.0,
        -(tx * inv[0] + ty * inv[3] + tz * inv[6]),
        -(tx * inv[1] + ty * inv[4] + tz * inv[7]),
        -(tx * inv[2] + ty * inv[5] + tz * inv[8]),
        1.0,
    ])
}

/// `v · M3×3` (a direction through a row-vector matrix).
fn dir_apply(v: V3, m: &Mat4) -> V3 {
    [
        v[0] * m[0] + v[1] * m[4] + v[2] * m[8],
        v[0] * m[1] + v[1] * m[5] + v[2] * m[9],
        v[0] * m[2] + v[1] * m[6] + v[2] * m[10],
    ]
}

fn point_apply(v: V3, m: &Mat4) -> V3 {
    let d = dir_apply(v, m);
    [d[0] + m[12], d[1] + m[13], d[2] + m[14]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn translation(m: &Mat4) -> V3 {
    [m[12], m[13], m[14]]
}

/// Rows of `RotY(yaw) · RotZ(pitch)` (GX column order; row 0 = the aim).
pub fn rot_rows(yaw: f32, pitch: f32) -> [V3; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    [
        [cp * cy, sp, -cp * sy],
        [-sp * cy, cp, sp * sy],
        [sy, 0.0, cy],
    ]
}

/// `(yaw, pitch)` aiming local +x of rest frame `f` (whose inverse is `inv`)
/// along model-space `d` (`FUN_8010e30c`).
fn angles(inv: &Mat4, d: V3) -> (f32, f32) {
    let l = dir_apply(d, inv);
    let h = (l[0] * l[0] + l[2] * l[2]).sqrt();
    let yaw = if h > 0.0 { (-l[2]).atan2(l[0]) } else { 0.0 };
    (yaw, l[1].atan2(h))
}

fn direction(f: &Mat4, a: (f32, f32)) -> V3 {
    let v = dir_apply(rot_rows(a.0, a.1)[0], f);
    let n = len(v);
    if n > 0.0 {
        scale(v, 1.0 / n)
    } else {
        v
    }
}

/// `FUN_8010e224`.
pub fn clamp(a: (f32, f32), y: &Axis, z: &Axis) -> (f32, f32) {
    let (mut yaw, mut pitch) = a;
    if !y.enable {
        yaw = 0.0;
    } else if y.limit {
        yaw = yaw.max(y.min).min(y.max);
    }
    if !z.enable {
        pitch = 0.0;
    } else if z.limit {
        pitch = pitch.max(-z.max).min(-z.min);
    }
    (yaw, pitch)
}

/// The spring gain of one axis block.
pub fn gain(axis: &Axis, speed: f32, dt: f32) -> f32 {
    let mut k = GAIN * dt / STEP_S;
    if axis.enable && axis.damp {
        k = dt / STEP_S * (1.0 - axis.damping);
    }
    (k * speed / SPEED_DIV).min(GAIN_CAP)
}

/// `FUN_8010e138`: toward 0 by `1.1 · stiffness · |v|`.
pub fn decay(v: f32, axis: &Axis) -> f32 {
    let f = DECAY * axis.stiffness * v.abs();
    if v > 0.0 {
        v - f
    } else if v < 0.0 {
        v + f
    } else {
        v
    }
}

// ── the resolved rig + state ─────────────────────────────────────────

/// A dynamic bone resolved against a skeleton.
#[derive(Debug, Clone, PartialEq)]
pub struct RigBone {
    pub index: usize,
    pub parent: usize,
    /// `bind[index] · inverse(bind[parent])`.
    pub rest_local: Mat4,
    pub len_m: f32,
    pub gravity: bool,
    pub z: Axis,
    pub y: Axis,
    pub exclude: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RigBox {
    pub id: u8,
    pub joint: usize,
    pub min: V3,
    pub max: V3,
}

/// A dancer's dynamic bones, in solve order (parents first), plus the
/// non-dynamic bones hanging below one (re-derived from their rest local).
#[derive(Debug, Clone, PartialEq)]
pub struct Rig {
    pub units_per_metre: f32,
    pub bones: Vec<RigBone>,
    pub boxes: Vec<RigBox>,
    /// `(index, parent, rest local)` of rigid bones under a dynamic one.
    pub followers: Vec<(usize, usize, Mat4)>,
}

/// Resolve `file` against a skeleton (`index_of` = the `.b2it` name lookup,
/// `bind_world` / `parents` its bone table). Bones / boxes naming unknown
/// joints are dropped and reported in the second value (one line each).
pub fn resolve(
    file: &DynFile,
    index_of: impl Fn(&str) -> Option<usize>,
    bind_world: &[Mat4],
    parents: &[i16],
) -> (Rig, Vec<String>) {
    let mut notes = Vec::new();
    let n = bind_world.len().min(parents.len());
    let local_of = |i: usize, p: usize| -> Option<Mat4> {
        let inv = affine_inverse(bind_world.get(p)?)?;
        Some(mat_mul(bind_world.get(i)?, &inv))
    };
    let mut bones: Vec<RigBone> = Vec::new();
    for b in &file.bones {
        let (Some(i), Some(p)) = (index_of(&b.name), index_of(&b.parent)) else {
            notes.push(format!(
                "dynamic bone {} (parent {}): not in the rig",
                b.name, b.parent
            ));
            continue;
        };
        if i >= n || p >= n || parents.get(i).copied() != Some(p as i16) {
            notes.push(format!(
                "dynamic bone {}: parent {} does not match the rig -- skipped",
                b.name, b.parent
            ));
            continue;
        }
        let Some(rest_local) = local_of(i, p) else {
            notes.push(format!("dynamic bone {}: singular bind -- skipped", b.name));
            continue;
        };
        bones.push(RigBone {
            index: i,
            parent: p,
            rest_local,
            len_m: b.len_m,
            gravity: b.gravity,
            z: b.z,
            y: b.y,
            exclude: b.exclude.clone(),
        });
    }
    bones.sort_by_key(|b| b.index);
    let mut boxes = Vec::new();
    for x in &file.boxes {
        match index_of(&x.joint).filter(|&j| j < n) {
            Some(joint) => boxes.push(RigBox {
                id: x.id,
                joint,
                min: x.min,
                max: x.max,
            }),
            None => notes.push(format!(
                "collider {}: joint {} not in the rig",
                x.id, x.joint
            )),
        }
    }
    // Rigid bones below a dynamic one follow it at their rest local.
    let mut moved = vec![false; n];
    for b in &bones {
        if let Some(m) = moved.get_mut(b.index) {
            *m = true;
        }
    }
    let mut followers = Vec::new();
    for i in 0..n {
        let p = parents[i];
        if p < 0 || (p as usize) >= i || moved[i] {
            continue;
        }
        let p = p as usize;
        if moved[p] {
            if let Some(l) = local_of(i, p) {
                followers.push((i, p, l));
                moved[i] = true;
            }
        }
    }
    (
        Rig {
            units_per_metre: file.units_per_metre,
            bones,
            boxes,
            followers,
        },
        notes,
    )
}

/// Per-dancer solver state: per rig bone the aim direction (model space;
/// `None` = rest) and the previous origin.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub dirs: Vec<Option<V3>>,
    pub prev: Vec<Option<V3>>,
    acc: f32,
}

impl State {
    pub fn new(rig: &Rig) -> State {
        State {
            dirs: vec![None; rig.bones.len()],
            prev: vec![None; rig.bones.len()],
            acc: 0.0,
        }
    }

    /// Back to the rest pose, no history (song (re)start, rewind, a body
    /// switch).
    pub fn reset(&mut self) {
        self.dirs.iter_mut().for_each(|d| *d = None);
        self.prev.iter_mut().for_each(|p| *p = None);
        self.acc = 0.0;
    }

    /// Keep the pose, forget the previous origins (a teleport / clip cut).
    pub fn teleport(&mut self) {
        self.prev.iter_mut().for_each(|p| *p = None);
    }
}

/// Collision (APPROXIMATION, see the module doc): `(direction, depth in zan
/// units)` of the deepest hit, or `None`.
fn collide(
    origin: V3,
    d: V3,
    length: f32,
    boxes: &[RigBox],
    exclude: &[u8],
    bones: &[Mat4],
    upm: f32,
) -> Option<(V3, f32)> {
    let tip = [
        origin[0] + d[0] * length,
        origin[1] + d[1] * length,
        origin[2] + d[2] * length,
    ];
    let mut best: Option<(V3, f32)> = None;
    for b in boxes {
        if exclude.contains(&b.id) {
            continue;
        }
        let Some(j) = bones.get(b.joint) else {
            continue;
        };
        let Some(inv) = affine_inverse(j) else {
            continue;
        };
        let lt = point_apply(tip, &inv);
        let inside = (0..3).all(|k| lt[k] > b.min[k] && lt[k] < b.max[k]);
        if !inside {
            continue;
        }
        let pen = [
            lt[0] - b.min[0],
            lt[1] - b.min[1],
            lt[2] - b.min[2],
            b.max[0] - lt[0],
            b.max[1] - lt[1],
            b.max[2] - lt[2],
        ];
        let mut k = 0usize;
        for (q, &v) in pen.iter().enumerate() {
            if v < pen[k] {
                k = q;
            }
        }
        let mut out = lt;
        out[k % 3] = if k < 3 { b.min[k % 3] } else { b.max[k % 3] };
        if best.map_or(true, |(_, depth)| pen[k] > depth) {
            best = Some((point_apply(out, j), pen[k]));
        }
    }
    let (p, depth) = best?;
    let nd = sub(p, origin);
    let n = len(nd);
    (n > 0.0).then(|| (scale(nd, 1.0 / n), depth * upm))
}

/// One step of one bone (`FUN_8010d430`) in rest frame `f`.
#[allow(clippy::too_many_arguments)]
fn step_bone(
    b: &RigBone,
    f: &Mat4,
    inv: &Mat4,
    dir: &mut Option<V3>,
    prev: &mut Option<V3>,
    boxes: &[RigBox],
    bones: &[Mat4],
    upm: f32,
    dt: f32,
) {
    let origin = translation(f);
    let mut lag = [0.0f32; 3];
    if let Some(p) = *prev {
        let delta = sub(p, origin);
        if len(delta) <= TELEPORT_M {
            lag = scale(delta, upm);
        }
    }
    if b.gravity {
        lag[1] -= GRAVITY_MUL * GRAVITY_UNIT;
    }
    let n = len(lag);
    let mut speed = n / GRAVITY_UNIT;
    let lag_n = if n > 0.0 { scale(lag, 1.0 / n) } else { lag };
    let mut aim = direction(f, clamp(angles(inv, lag_n), &b.y, &b.z));
    let hit = collide(origin, aim, b.len_m, boxes, &b.exclude, bones, upm);
    if let Some((d, depth)) = hit {
        aim = d;
        speed *= COLLIDE_GAIN * depth.abs();
    }
    let tgt = angles(inv, aim);
    let cur = dir.map_or((0.0, 0.0), |d| angles(inv, d));
    // The Wii's crossing: the y block's damping drives the PITCH gain.
    let k_pitch = gain(&b.y, speed, dt);
    let k_yaw = gain(&b.z, speed, dt);
    let mut yaw = cur.0 + k_yaw * (tgt.0 - cur.0);
    let mut pitch = cur.1 + k_pitch * (tgt.1 - cur.1);
    if hit.is_none() {
        if b.y.enable {
            yaw = decay(yaw, &b.y);
        }
        if b.z.enable {
            pitch = decay(pitch, &b.z);
        }
    }
    *dir = Some(direction(f, clamp((yaw, pitch), &b.y, &b.z)));
    *prev = Some(origin);
}

/// Solve every rig bone into `bones` (already evaluated by the clip: the
/// dynamic bones' parents are current; their own entries are overwritten).
/// `dt` = real seconds since the last call (clamped); `teleport` = the root
/// jumped this frame (a clip cut). No allocation, no panic.
pub fn solve(rig: &Rig, state: &mut State, bones: &mut [Mat4], dt: f32, teleport: bool) {
    if state.dirs.len() != rig.bones.len() || state.prev.len() != rig.bones.len() {
        return;
    }
    if teleport {
        state.teleport();
    }
    let dt = if dt.is_finite() {
        dt.clamp(0.0, MAX_DT_S)
    } else {
        0.0
    };
    state.acc += dt;
    let mut steps = (state.acc / STEP_S + 1e-3).floor() as u32;
    state.acc -= steps as f32 * STEP_S;
    if state.acc < 0.0 {
        state.acc = 0.0;
    }
    if steps > MAX_STEPS_PER_FRAME {
        steps = MAX_STEPS_PER_FRAME;
        state.acc = 0.0;
    }
    let upm = rig.units_per_metre;
    for (k, b) in rig.bones.iter().enumerate() {
        let Some(parent) = bones.get(b.parent).copied() else {
            continue;
        };
        let f = mat_mul(&b.rest_local, &parent);
        let Some(inv) = affine_inverse(&f) else {
            continue;
        };
        let (Some(dir), Some(prev)) = (state.dirs.get_mut(k), state.prev.get_mut(k)) else {
            continue;
        };
        for _ in 0..steps {
            step_bone(b, &f, &inv, dir, prev, &rig.boxes, bones, upm, STEP_S);
        }
        let a = dir.map_or((0.0, 0.0), |d| clamp(angles(&inv, d), &b.y, &b.z));
        let r = rot_rows(a.0, a.1);
        let rm: Mat4 = [
            r[0][0], r[0][1], r[0][2], 0.0, //
            r[1][0], r[1][1], r[1][2], 0.0, //
            r[2][0], r[2][1], r[2][2], 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        if let Some(out) = bones.get_mut(b.index) {
            *out = mat_mul(&rm, &f);
        }
    }
    for &(i, p, ref l) in &rig.followers {
        if let Some(parent) = bones.get(p).copied() {
            if let Some(out) = bones.get_mut(i) {
                *out = mat_mul(l, &parent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Mat4 = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    fn ax(e: bool, min: f32, max: f32, stiff: f32, damping: f32) -> Axis {
        Axis {
            enable: e,
            limit: e,
            damp: e,
            min,
            max,
            stiffness: stiff,
            damping,
        }
    }

    fn translate(x: f32, y: f32, z: f32) -> Mat4 {
        let mut m = IDENTITY;
        m[12] = x;
        m[13] = y;
        m[14] = z;
        m
    }

    /// The `test_zan_formats.TestWinxClub.chain` scenario: root (bone 0,
    /// the moving parent), two dynamic bones along +x, a box in the root's
    /// frame. Returns the solved rig + skeleton binds.
    fn chain_rig(gravity: bool, with_box: bool) -> (Rig, Vec<Mat4>, Vec<i16>) {
        let binds = vec![IDENTITY, IDENTITY, translate(0.2, 0.0, 0.0)];
        let parents = vec![-1i16, 0, 1];
        let names = ["root", "DB_a", "DB_b"];
        let file = DynFile {
            units_per_metre: 1.0 / (0.970 / 8.593),
            bones: vec![
                BoneCfg {
                    name: "DB_a".into(),
                    parent: "root".into(),
                    len_m: 0.2,
                    gravity,
                    z: ax(true, -0.7, 0.7, 0.1, 0.3),
                    y: ax(true, -0.3, 1.2, 0.1, 0.3),
                    exclude: vec![],
                },
                BoneCfg {
                    name: "DB_b".into(),
                    parent: "DB_a".into(),
                    len_m: 0.15,
                    gravity,
                    z: ax(true, -1.4, 0.87, 0.08, 0.07),
                    y: ax(true, -0.8, 0.9, 0.08, 0.1),
                    exclude: vec![],
                },
            ],
            boxes: if with_box {
                vec![BoxCfg {
                    id: 1,
                    joint: "root".into(),
                    min: [0.0, -0.5, -0.5],
                    max: [0.6, -0.05, 0.5],
                }]
            } else {
                vec![]
            },
        };
        let (rig, notes) = resolve(
            &file,
            |n| names.iter().position(|x| *x == n),
            &binds,
            &parents,
        );
        assert!(notes.is_empty(), "{notes:?}");
        (rig, binds, parents)
    }

    /// Run the chain with the root at `motion(f)`; tip of bone 2 per frame.
    fn run(rig: &Rig, frames: usize, motion: impl Fn(usize) -> [f32; 3]) -> Vec<[f32; 3]> {
        let mut st = State::new(rig);
        let mut tips = Vec::new();
        for f in 0..frames {
            let m = motion(f);
            let root = translate(m[0], m[1], m[2]);
            // what evaluate_into leaves: the chain at rest under the root
            let mut bones = vec![root, root, mat_mul(&translate(0.2, 0.0, 0.0), &root)];
            solve(rig, &mut st, &mut bones, STEP_S, false);
            tips.push(point_apply([0.15, 0.0, 0.0], &bones[2]));
        }
        tips
    }

    /// Cross-check against `zan_dump.dynbone_step` (the numbers below come
    /// from the Python reference on the same scenario, f64; f32 here).
    #[test]
    fn matches_the_python_reference() {
        let (rig, _, _) = chain_rig(true, true);
        let tips = run(&rig, 91, |f| {
            [0.3 * (f as f32 * 0.1).sin(), 1.0, 0.002 * f as f32]
        });
        let want: [(usize, [f32; 3]); 4] = [
            (5, [0.44266796, 0.92488609, -0.08414001]),
            (30, [0.35791005, 0.95886072, -0.04927148]),
            (60, [-0.01670544, 0.77063782, -0.12749374]),
            (90, [0.41377686, 0.93909594, 0.02277456]),
        ];
        for (f, w) in want {
            let g = tips[f];
            let err = (0..3).map(|k| (g[k] - w[k]).abs()).fold(0.0f32, f32::max);
            assert!(err < 2e-3, "frame {f}: got {g:?} want {w:?} (err {err})");
        }
    }

    #[test]
    fn rest_without_gravity_or_motion_and_hanging_with_gravity() {
        let (rig, _, _) = chain_rig(false, false);
        let tips = run(&rig, 30, |_| [0.0, 1.0, 0.0]);
        let t = tips[29];
        assert!((t[0] - 0.35).abs() < 1e-5 && (t[1] - 1.0).abs() < 1e-5 && t[2].abs() < 1e-5);
        let (rig, _, _) = chain_rig(true, false);
        let tips = run(&rig, 240, |_| [0.0, 1.0, 0.0]);
        assert!(tips[239][1] < 0.95, "hangs: {:?}", tips[239]);
        assert!((tips[239][1] - tips[238][1]).abs() < 1e-5, "settled");
    }

    #[test]
    fn clip_cut_teleport_does_not_fling() {
        let (rig, _, _) = chain_rig(false, false);
        let mut st = State::new(&rig);
        let mut tip = [0.0; 3];
        for f in 0..21 {
            let x = if f < 20 { 0.0 } else { 3.0 };
            let root = translate(x, 1.0, 0.0);
            let mut bones = vec![root, root, mat_mul(&translate(0.2, 0.0, 0.0), &root)];
            solve(&rig, &mut st, &mut bones, STEP_S, f == 20);
            tip = point_apply([0.15, 0.0, 0.0], &bones[2]);
        }
        assert!((tip[0] - 3.35).abs() < 1e-5 && (tip[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn steps_follow_real_time_and_cap() {
        let (rig, _, _) = chain_rig(true, false);
        let mut st = State::new(&rig);
        let root = translate(0.0, 1.0, 0.0);
        let mut bones = vec![root, root, mat_mul(&translate(0.2, 0.0, 0.0), &root)];
        // 120 Hz: a step every other frame; the pose still re-applies
        solve(&rig, &mut st, &mut bones, STEP_S * 0.5, false);
        assert!(st.dirs[0].is_none(), "no step yet");
        solve(&rig, &mut st, &mut bones, STEP_S * 0.5, false);
        assert!(st.dirs[0].is_some(), "one step");
        // a huge dt is clamped + capped, never NaN
        solve(&rig, &mut st, &mut bones, 10.0, false);
        solve(&rig, &mut st, &mut bones, f32::NAN, false);
        assert!(bones.iter().all(|m| m.iter().all(|v| v.is_finite())));
        st.reset();
        assert!(st.dirs.iter().all(Option::is_none));
    }

    #[test]
    fn parse_round_trip_and_errors() {
        let text = "# comment\ndynbones 1\nunits_per_metre 8.858763\n\
bone DB_hair01 parent=rot_DBbone len=0.120570 gravity=1 z=1,1,1,-0.698132,0.698132,0.100000,0.400000 y=1,1,1,-0.261799,1.221730,0.100000,0.300000 exclude=11,12 rest=1,0,0,0,1,0,0,0,1,0,0,0\n\
bone DB_Lwing_01 parent=wing_root len=0.5 gravity=0 z=1,1,1,-0.349,1.222,0.06,0.02 y=0,0,0,0,0,0.02,0.3 exclude=-\n\
box 1 joint=Spine min=-0.225,0,-0.089 max=0.225,0.474,0.089\n";
        let f = parse(text).unwrap();
        assert_eq!(f.bones.len(), 2);
        assert_eq!(f.bones[0].exclude, vec![11, 12]);
        assert!(f.bones[0].gravity && !f.bones[1].gravity);
        assert!(!f.bones[1].y.enable && f.bones[1].z.damp);
        assert_eq!(f.boxes[0].joint, "Spine");
        assert!((f.units_per_metre - 8.858763).abs() < 1e-5);
        assert!(parse("units_per_metre 1\n").is_err());
        assert!(parse("dynbones 2\n").is_err());
        assert!(parse("dynbones 1\nunits_per_metre 1\nbone X parent=Y\n").is_err());
        assert!(parse("dynbones 1\nunits_per_metre 1\nfoo\n").is_err());
    }

    #[test]
    fn resolve_drops_unknown_and_mismatched_bones_and_tracks_followers() {
        let binds = vec![
            IDENTITY,
            IDENTITY,
            translate(0.2, 0.0, 0.0),
            translate(0.3, 0.0, 0.0),
        ];
        let parents = vec![-1i16, 0, 1, 2];
        let names = ["root", "DB_a", "DB_b", "tip"];
        let mk = |name: &str, parent: &str| BoneCfg {
            name: name.into(),
            parent: parent.into(),
            len_m: 0.1,
            gravity: true,
            z: Axis::OFF,
            y: Axis::OFF,
            exclude: vec![],
        };
        let file = DynFile {
            units_per_metre: 8.0,
            bones: vec![mk("DB_a", "root"), mk("DB_x", "root"), mk("DB_b", "root")],
            boxes: vec![BoxCfg {
                id: 3,
                joint: "nope".into(),
                min: [0.0; 3],
                max: [1.0; 3],
            }],
        };
        let (rig, notes) = resolve(
            &file,
            |n| names.iter().position(|x| *x == n),
            &binds,
            &parents,
        );
        assert_eq!(rig.bones.len(), 1);
        assert_eq!(notes.len(), 3, "{notes:?}");
        // DB_b is rigid now (its line was rejected) and hangs below DB_a;
        // `tip` hangs below DB_b: both follow
        assert_eq!(
            rig.followers.iter().map(|f| f.0).collect::<Vec<_>>(),
            vec![2, 3]
        );
    }
}
