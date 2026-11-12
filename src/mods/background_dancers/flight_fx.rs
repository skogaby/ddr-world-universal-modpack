//! HOTTEST PARTY flight effects: a std-only port of Konami's zan `CzanEff`
//! particle runtime (`.TEB` effect banks) — the flyer's light orb, rainbow
//! trail, hand-orbiting stars with light trails and the take-off burst, run
//! from the game's own `boss_ddr3.TEB` (MUSIC FIT `game/GAME_CHR_EFF.bin`).
//!
//! RE record: `docs/wii_ddr_zan_effects_research.md` (format §2, runtime §3,
//! the world scroll §3.4); reference implementation `scripts/teb_dump.py`
//! (`parse_teb`, `simulate`) — this file follows it line for line. Units are
//! zan units, matrices COLUMN-vector (`p_world = r · p + t`) like the
//! reference; the director converts from / to World's row-vector metres
//! ([`Affine::from_world_row`], [`sprite_bone`], [`ribbon_bone`]).
//!
//! * [`parse_teb`] — bounds-checked: every read is an `Option`, a truncated
//!   or garbage bank yields `None`, never a panic. Load time only (allocates).
//! * [`EffectSim`] — one playing effect instance: node clocks / tracks, every
//!   part's emitter, particles and ribbons. Pools are sized at construction
//!   from the TEB (`max` particles per part, `segments` points per ribbon,
//!   capped at [`RIBBON_MAX_POINTS`]); [`EffectSim::step`] never allocates,
//!   never indexes unchecked, never panics. Fixed 60 Hz steps
//!   ([`SIM_DT`], the game's tick) keep ribbon lengths what the game shows
//!   whatever World's frame rate; [`EffectSim::advance`] runs the catch-up.
//! * The world scroll: after the flight switch MUSIC FIT translates camera,
//!   stage and dancers by +3 units per frame ([`FLIGHT_SCROLL`]); only state
//!   kept in WORLD space (ribbon points, world-space particles) streams
//!   behind the flyer. The port keeps the caller's frame still and instead
//!   moves every world-space point by `−scroll · dt` per step — the same
//!   picture (research §3.4), and the numbers stay small.
//!
//! * Drawing — World has no particle path, so the content port
//!   (`tools/blender_ddr_addon/examples/port_flight_fx.py`) ships POOL models
//!   in the flight stage's arc: per player sprite quads (one mesh / material /
//!   bone each, identity bind) and ribbon strips (a vertex pair per point on
//!   its own bone), laid out in `flight_fx.txt` ([`FxLayout`]). Per frame
//!   [`PoolWriter`] turns the drawables into bone matrices
//!   ([`sprite_bone`] / [`ribbon_bone`], metres), draw-record colours
//!   ([`record_colour`]: additive premultiplied, alpha 1 — an entry with
//!   colour alpha < 1 is forced into the alpha blend group) and
//!   `m_vTexAnime` writes (flip-book cell offsets, a ribbon's v scale);
//!   unused entries collapse ([`HIDDEN_BONE`]). [`DancerFx`] is one flying
//!   dancer: `FUN_8004b5a8`'s effects ([`mode_effects`]) — the leap (mode 2,
//!   take-off frame [`LEAP_FRAME`]) and the flight (mode 0, from the switch)
//!   — on the real clock, and its pool frames.
//!
//! * The flight INTRO (after the first cabinet run): [`StageFx`] runs the
//!   stage bank's sky burst at the layout's `stage_effect` point from intro
//!   frame 180 (pseudo-player [`STAGE_PLAYER`]'s pools), the layout's
//!   `stage_sound` names the intro sound, and [`flight_schedule_time`] puts
//!   the take-off on the real clock (the Wii's 60 Hz intro) and the flight
//!   on the dance clock, continuous at the switch. Gravity (part flag 0x2)
//!   is ported for it (research §3.2).
//!
//! Harness-mounted (`scripts/validate_background_dancers.sh`, module
//! `flight_fx`): no `crate::` imports.

use std::collections::VecDeque;
use std::f32::consts::PI;

/// The manager's effect scale (`+0x294`): set_mtx scales the attach
/// matrix's rotation columns by it.
pub const EFFECT_SCALE: f32 = 10.0;
/// The game's effect tick (`FUN_8012a970`: 1/60 s × play speed).
pub const SIM_DT: f32 = 1.0 / 60.0;
/// The flight's world scroll, zan units / s (+3.0 z per 60 Hz frame from
/// the switch: `FUN_800377e0`).
pub const FLIGHT_SCROLL: [f32; 3] = [0.0, 0.0, 180.0];
/// A ribbon point's life (ribbon `+0x1c`).
pub const RIBBON_POINT_LIFE: f32 = 1.0;
/// Points kept per ribbon at most (a 1 s point life at 60 Hz keeps ≤ 61).
pub const RIBBON_MAX_POINTS: usize = 64;
/// Catch-up steps run by one [`EffectSim::advance`] at most (a longer gap
/// is skipped, not simulated — a hitch must not stall the frame).
pub const MAX_CATCH_UP_STEPS: u32 = 8;

/// Part flags (`+0x00`): sub-blocks present + behaviour bits.
pub const P_EMITTER: u32 = 0x1;
pub const P_GRAVITY: u32 = 0x2;
pub const P_DRAW: u32 = 0x4;
pub const P_CHAIN: u32 = 0x8;
pub const P_RIBBON: u32 = 0x10;
pub const P_SPIN: u32 = 0x20;
pub const P_FOLLOW: u32 = 0x40;
pub const P_WORLD: u32 = 0x80;
pub const P_FOLLOW_SCALE: u32 = 0x100;
pub const P_NO_DEPTH: u32 = 0x200;
pub const P_FLIP_UV: u32 = 0x400;
pub const P_IMMORTAL: u32 = 0x800;
/// Draw flags.
pub const D_FLIPBOOK: u32 = 0x2;
pub const D_COLOUR_KEYS: u32 = 0x20;
/// Draw blend 2 = additive (SRCALPHA / ONE), anything else alpha blending.
pub const BLEND_ADDITIVE: u8 = 2;

/// The unit quad (`0x80274f70`, a strip) and its default / flipped UVs
/// (`0x80274f08` / `0x80274f28`).
pub const QUAD: [[f32; 3]; 4] = [
    [0.5, -0.5, 0.0],
    [-0.5, -0.5, 0.0],
    [0.5, 0.5, 0.0],
    [-0.5, 0.5, 0.0],
];
pub const UV_DEFAULT: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 1.0], [1.0, 0.0]];
pub const UV_FLIPPED: [[f32; 2]; 4] = [[1.0, 1.0], [0.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

// ── small column-vector math ─────────────────────────────────────────

pub type V3 = [f32; 3];
/// 3×3, `m[row][col]`; columns are the frame's axes.
pub type M3 = [[f32; 3]; 3];
pub type Quat = [f32; 4]; // x y z w

pub const M3_IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
const Q_IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];
/// The lay-flat billboard (`0x80274eb8`).
const LAY_FLAT: M3 = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];

#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline]
fn norm(a: V3) -> f32 {
    dot(a, a).sqrt()
}
#[inline]
fn col(m: &M3, c: usize) -> V3 {
    [m[0][c], m[1][c], m[2][c]]
}
#[inline]
fn mv(m: &M3, v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}
fn mm(a: &M3, b: &M3) -> M3 {
    let mut o = [[0.0f32; 3]; 3];
    for (r, row) in o.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    o
}
fn transpose(m: &M3) -> M3 {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}
/// Scale column `c` of `m` by `s[c]` (numpy's `m * np.array(s)`).
fn scale_cols(m: &M3, s: V3) -> M3 {
    let mut o = *m;
    for row in o.iter_mut() {
        for (c, v) in row.iter_mut().enumerate() {
            *v *= s[c];
        }
    }
    o
}
fn col_lengths(m: &M3) -> V3 {
    [norm(col(m, 0)), norm(col(m, 1)), norm(col(m, 2))]
}
/// `m` with unit columns (a zero column stays zero instead of NaN).
fn unit_cols(m: &M3) -> M3 {
    let s = col_lengths(m);
    let inv = |x: f32| if x > 1e-12 { 1.0 / x } else { 0.0 };
    scale_cols(m, [inv(s[0]), inv(s[1]), inv(s[2])])
}

/// A column-vector affine transform: `p' = r · p + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub r: M3,
    pub t: V3,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        r: M3_IDENTITY,
        t: [0.0; 3],
    };

    pub fn mul(&self, o: &Affine) -> Affine {
        Affine {
            r: mm(&self.r, &o.r),
            t: add(mv(&self.r, o.t), self.t),
        }
    }

    /// A World ROW-vector matrix (metres, `p_world = p · M`, rows 0..2 the
    /// local axes, row 3 the translation) as a zan-unit attach frame: the
    /// translation divided by `metres_per_unit`, the rotation's columns made
    /// unit (a joint frame; set_mtx's ×[`EFFECT_SCALE`] is the sim's job).
    /// World's game space and zan's share their axes (Y up, facing +Z).
    pub fn from_world_row(m: &[f32; 16], metres_per_unit: f32) -> Affine {
        let r = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
        let k = if metres_per_unit.abs() > 1e-12 {
            1.0 / metres_per_unit
        } else {
            1.0
        };
        Affine {
            r: unit_cols(&r),
            t: [m[12] * k, m[13] * k, m[14] * k],
        }
    }

    /// The same frame with an identity rotation (joint 5: position only).
    pub fn position_only(&self) -> Affine {
        Affine {
            r: M3_IDENTITY,
            t: self.t,
        }
    }
}

fn quat_mtx(q: Quat) -> M3 {
    let [x, y, z, w] = q;
    if x == 0.0 && y == 0.0 && z == 0.0 && w == 0.0 {
        return M3_IDENTITY;
    }
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn mtx_quat(m: &M3) -> Quat {
    let t = m[0][0] + m[1][1] + m[2][2];
    if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        return [
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            0.25 * s,
        ];
    }
    let i = if m[1][1] > m[0][0] {
        if m[2][2] > m[1][1] {
            2
        } else {
            1
        }
    } else if m[2][2] > m[0][0] {
        2
    } else {
        0
    };
    let (j, k) = ((i + 1) % 3, (i + 2) % 3);
    let s = (1.0 + m[i][i] - m[j][j] - m[k][k]).max(0.0).sqrt() * 2.0;
    if s < 1e-12 {
        return Q_IDENTITY;
    }
    let mut q = [0.0f32; 4];
    q[i] = 0.25 * s;
    q[j] = (m[j][i] + m[i][j]) / s;
    q[k] = (m[k][i] + m[i][k]) / s;
    q[3] = (m[k][j] - m[j][k]) / s;
    q
}

fn slerp(p: Quat, q: Quat, t: f32) -> Quat {
    let zero = |a: &Quat| a.iter().all(|&v| v == 0.0);
    let p = if zero(&p) { Q_IDENTITY } else { p };
    let mut q = if zero(&q) { Q_IDENTITY } else { q };
    let mut d = p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3];
    if d < 0.0 {
        q = [-q[0], -q[1], -q[2], -q[3]];
        d = -d;
    }
    if d > 0.9995 {
        let r = [
            p[0] + t * (q[0] - p[0]),
            p[1] + t * (q[1] - p[1]),
            p[2] + t * (q[2] - p[2]),
            p[3] + t * (q[3] - p[3]),
        ];
        let n = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]).sqrt();
        return if n > 1e-12 {
            [r[0] / n, r[1] / n, r[2] / n, r[3] / n]
        } else {
            Q_IDENTITY
        };
    }
    let th = d.min(1.0).acos();
    let st = th.sin();
    if st.abs() < 1e-12 {
        return p;
    }
    let (a, b) = (((1.0 - t) * th).sin() / st, (t * th).sin() / st);
    [
        a * p[0] + b * q[0],
        a * p[1] + b * q[1],
        a * p[2] + b * q[2],
        a * p[3] + b * q[3],
    ]
}

/// `FUN_801325c8`: rotation by `a` about the unit `axis`.
fn axis_angle(axis: V3, a: f32) -> M3 {
    let [x, y, z] = axis;
    let (s, c) = a.sin_cos();
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}

/// `FUN_801326d0`: Ry · Rz · Rx (x applied first).
fn euler_yzx(rx: f32, ry: f32, rz: f32) -> M3 {
    let a = axis_angle([0.0, 1.0, 0.0], ry);
    let b = axis_angle([0.0, 0.0, 1.0], rz);
    let c = axis_angle([1.0, 0.0, 0.0], rx);
    mm(&mm(&a, &b), &c)
}

// ── the TEB (format: research §2) ────────────────────────────────────

/// One position / rotation key of a node track (0x20 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub t: f32,
    pub pos: V3,
    pub quat: Quat,
}

/// A node's local motion while `t0 ≤ clock ≤ t1`.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub t0: f32,
    pub t1: f32,
    pub spline: bool,
    pub keys: Vec<Key>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Emitter {
    pub life: f32,
    pub life_rand: f32,
    pub interval: f32,
    pub spread: V3,
    pub rot_spread: V3,
    pub speed: f32,
    pub speed_rand: f32,
    pub max: usize,
    pub per_spawn: usize,
    /// 0 camera-facing, 1 laid flat in the emitter's frame.
    pub billboard: u8,
}

/// A part's gravity block (flag 0x2; `FUN_8012c790` / `FUN_8012c438`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gravity {
    pub dir: V3,
    pub rot_spread: V3,
    pub accel: f32,
    pub accel_rand: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flipbook {
    pub width: u16,
    pub cell: u16,
    pub frame_s: f32,
    pub frames: u8,
}

impl Flipbook {
    /// Cells per row (`width / cell`, ≥ 1) and the cell size in UV.
    pub fn grid(&self) -> (u32, f32) {
        let cols = (self.width / self.cell.max(1)).max(1) as u32;
        (cols, 1.0 / cols as f32)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub flags: u32,
    /// The single colour (no colour keys).
    pub colour: [u8; 4],
    /// Colour keys {r, g, b, _, t%} (flag 0x20).
    pub colour_keys: Vec<[u8; 5]>,
    pub rect: [f32; 4],
    pub scale_keys: Vec<(f32, f32)>,
    pub alpha_keys: Vec<(f32, f32)>,
    pub blend: u8,
    /// TPL texture index (< 0 untextured).
    pub tex: i16,
    /// Size variation % (lo, hi).
    pub size_var: (u8, u8),
    pub flipbook: Option<Flipbook>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ribbon {
    pub width: f32,
    pub segments: u8,
    pub tex: i16,
}

/// Spawn shape block (ellipsoid / ring): `{u32 flags, f32 radii[3]}`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeBlock {
    pub flags: u32,
    pub radii: V3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub flags: u32,
    /// 0 box, 1 ellipsoid, 2 ring.
    pub shape: u8,
    pub tracks: Vec<Track>,
    pub emitter: Option<Emitter>,
    /// Gravity (flag 0x2): `{dir[3], rot_spread[3], accel, accel_rand}`.
    pub gravity: Option<Gravity>,
    pub draw: Option<Draw>,
    pub ribbon: Option<Ribbon>,
    /// roll0, roll0_rand, roll_rate, rate_rand, roll_accel, accel_rand,
    /// orbit_rate, orbit_accel.
    pub spin: Option<[f32; 8]>,
    /// (follow, scale).
    pub follow: Option<(f32, f32)>,
    pub ellipsoid: Option<ShapeBlock>,
    pub ring: Option<ShapeBlock>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub child: u8,
    pub next: u8,
    /// 0 root, 1 part, 2 model (not drawn here).
    pub kind: u8,
    pub parent: Option<usize>,
    /// The root's tracks, or a part's own.
    pub tracks: Vec<Track>,
    pub part: Option<Part>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub nodes: Vec<Node>,
    /// The root's loop flag: restart every node clock at the track end.
    pub loops: bool,
    /// The root's track end (max `t1`).
    pub span: f32,
    /// Nodes parent-first (the update recursion's order).
    pub order: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Teb {
    pub effects: Vec<Effect>,
}

/// Bound on any count read from the file (keys, tracks): a garbage count
/// must not turn into a huge allocation.
const MAX_LIST: usize = 4096;

struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn bytes<const N: usize>(&self, o: usize) -> Option<[u8; N]> {
        let s = self.0.get(o..o.checked_add(N)?)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Some(a)
    }
    fn u8(&self, o: usize) -> Option<u8> {
        self.0.get(o).copied()
    }
    fn u16(&self, o: usize) -> Option<u16> {
        self.bytes::<2>(o).map(u16::from_be_bytes)
    }
    fn i16(&self, o: usize) -> Option<i16> {
        self.bytes::<2>(o).map(i16::from_be_bytes)
    }
    fn u32(&self, o: usize) -> Option<u32> {
        self.bytes::<4>(o).map(u32::from_be_bytes)
    }
    fn off(&self, o: usize) -> Option<usize> {
        self.u32(o).map(|v| v as usize)
    }
    fn f32(&self, o: usize) -> Option<f32> {
        self.u32(o).map(f32::from_bits)
    }
    fn v3(&self, o: usize) -> Option<V3> {
        Some([self.f32(o)?, self.f32(o + 4)?, self.f32(o + 8)?])
    }
}

fn parse_keys(r: &Rd, off: usize, n: usize) -> Option<Vec<(f32, f32)>> {
    if n == 0 {
        return Some(Vec::new());
    }
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push((r.f32(off + 8 * i)?, r.f32(off + 8 * i + 4)?));
    }
    Some(v)
}

fn parse_tracks(r: &Rd, off: usize) -> Option<Vec<Track>> {
    if off == 0 {
        return Some(Vec::new());
    }
    let n = r.off(off)?;
    let table = r.off(off + 4)?;
    if n > MAX_LIST {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let q = table + 0x14 * i;
        let nk = r.off(q)?;
        let kp = r.off(q + 4)?;
        if nk > MAX_LIST {
            return None;
        }
        let mut keys = Vec::with_capacity(nk);
        for k in 0..nk {
            let o = kp + 0x20 * k;
            keys.push(Key {
                t: r.f32(o)?,
                pos: r.v3(o + 4)?,
                quat: [
                    r.f32(o + 16)?,
                    r.f32(o + 20)?,
                    r.f32(o + 24)?,
                    r.f32(o + 28)?,
                ],
            });
        }
        out.push(Track {
            t0: r.f32(q + 8)?,
            t1: r.f32(q + 12)?,
            spline: r.u8(q + 0x10)? != 0,
            keys,
        });
    }
    Some(out)
}

fn parse_part(r: &Rd, o: usize) -> Option<Part> {
    let w = |i: usize| r.off(o + 4 * i);
    let flags = r.u32(o)?;
    let shape = r.u8(o + 0x2C)?;
    let mut p = Part {
        flags,
        shape,
        tracks: parse_tracks(r, r.off(o + 0x28)?)?,
        emitter: None,
        gravity: None,
        draw: None,
        ribbon: None,
        spin: None,
        follow: None,
        ellipsoid: None,
        ring: None,
    };
    if flags & P_EMITTER != 0 {
        let e = w(1)?;
        let max = r.i16(e + 0x2C)?.max(0) as usize;
        let per = r.i16(e + 0x2E)?.max(0) as usize;
        if max > MAX_LIST {
            return None;
        }
        p.emitter = Some(Emitter {
            life: r.f32(e)?,
            life_rand: r.f32(e + 4)?,
            interval: r.f32(e + 8)?,
            spread: r.v3(e + 12)?,
            rot_spread: r.v3(e + 24)?,
            speed: r.f32(e + 36)?,
            speed_rand: r.f32(e + 40)?,
            max,
            per_spawn: per,
            billboard: r.u8(e + 0x32)?,
        });
    }
    if flags & P_GRAVITY != 0 {
        let g = w(2)?;
        p.gravity = Some(Gravity {
            dir: r.v3(g)?,
            rot_spread: r.v3(g + 12)?,
            accel: r.f32(g + 0x18)?,
            accel_rand: r.f32(g + 0x1C)?,
        });
    }
    if flags & P_DRAW != 0 {
        let d = w(3)?;
        let df = r.u32(d)?;
        let colours = r.off(d + 4)?;
        let ncolour = r.u8(d + 0x2A)?;
        let mut draw = Draw {
            flags: df,
            colour: [255; 4],
            colour_keys: Vec::new(),
            rect: [
                r.f32(d + 8)?,
                r.f32(d + 12)?,
                r.f32(d + 16)?,
                r.f32(d + 20)?,
            ],
            scale_keys: parse_keys(r, r.off(d + 0x18)?, r.u8(d + 0x24)? as usize)?,
            alpha_keys: parse_keys(r, r.off(d + 0x1C)?, r.u8(d + 0x25)? as usize)?,
            blend: r.u8(d + 0x26)?,
            tex: r.i16(d + 0x28)?,
            size_var: (r.u8(d + 0x2B)?, r.u8(d + 0x2C)?),
            flipbook: None,
        };
        if df & D_COLOUR_KEYS != 0 {
            for i in 0..(ncolour.max(1) as usize) {
                draw.colour_keys.push(r.bytes::<5>(colours + 8 * i)?);
            }
        } else {
            draw.colour = r.bytes::<4>(colours)?;
        }
        if df & D_FLIPBOOK != 0 {
            let f = r.off(d + 0x20)?;
            draw.flipbook = Some(Flipbook {
                width: r.u16(f)?,
                cell: r.u16(f + 2)?,
                frame_s: r.f32(f + 4)?,
                frames: r.u8(f + 8)?,
            });
        }
        p.draw = Some(draw);
    }
    if flags & P_RIBBON != 0 {
        let b = w(5)?;
        p.ribbon = Some(Ribbon {
            width: r.f32(b)?,
            segments: r.u8(b + 4)?,
            tex: r.i16(b + 6)?,
        });
    }
    if flags & P_SPIN != 0 {
        let s = w(6)?;
        let mut v = [0.0f32; 8];
        for (i, x) in v.iter_mut().enumerate() {
            *x = r.f32(s + 4 * i)?;
        }
        p.spin = Some(v);
    }
    if flags & P_FOLLOW != 0 {
        let f = w(7)?;
        p.follow = Some((r.f32(f)?, r.f32(f + 4)?));
    }
    let shape_block = |o: usize| -> Option<ShapeBlock> {
        Some(ShapeBlock {
            flags: r.u32(o)?,
            radii: r.v3(o + 4)?,
        })
    };
    match shape {
        1 => p.ellipsoid = Some(shape_block(w(8)?)?),
        2 => p.ring = Some(shape_block(w(9)?)?),
        _ => {}
    }
    Some(p)
}

/// Nodes parent-first, children in sibling order (`teb_dump.node_order`);
/// a malformed child / next chain (out of range, cyclic) is cut short.
fn node_order(nodes: &[Node]) -> Vec<usize> {
    let mut out = Vec::with_capacity(nodes.len());
    if nodes.is_empty() {
        return out;
    }
    let mut seen = vec![false; nodes.len()];
    let mut todo = vec![0usize];
    while let Some(i) = todo.pop() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        out.push(i);
        let mut kids = Vec::new();
        let mut kid = nodes[i].child as usize;
        while kid != 0 && kid < nodes.len() && !seen[kid] && kids.len() < nodes.len() {
            kids.push(kid);
            kid = nodes[kid].next as usize;
        }
        // a stack: push in reverse so the first child is visited first
        todo.extend(kids.into_iter().rev());
    }
    out
}

/// Parse a TEB bank (the member's bytes; offsets are TEB-relative).
/// `None` on a bad magic or any out-of-range read.
pub fn parse_teb(b: &[u8]) -> Option<Teb> {
    let r = Rd(b);
    if b.get(0..4)? != b"TEB\0" {
        return None;
    }
    let table = r.off(0x10)?;
    let n = (r.u32(0x14)? >> 16) as usize;
    let mut effects = Vec::with_capacity(n);
    for e in 0..n {
        let eo = table + 0x10 * e;
        let list = r.off(eo)?;
        let cnt = r.u8(eo + 8)? as usize;
        let mut nodes = Vec::with_capacity(cnt);
        for k in 0..cnt {
            let no = list + 8 * k;
            let kind = r.u8(no + 2)?;
            let data = r.off(no + 4)?;
            let mut nd = Node {
                child: r.u8(no)?,
                next: r.u8(no + 1)?,
                kind,
                parent: None,
                tracks: Vec::new(),
                part: None,
            };
            match kind {
                // the root's loop flag is read below from node 0's data
                0 => nd.tracks = parse_tracks(&r, r.off(data + 4)?)?,
                1 => {
                    let p = parse_part(&r, data)?;
                    nd.tracks = p.tracks.clone();
                    nd.part = Some(p);
                }
                _ => {}
            }
            nodes.push(nd);
        }
        // parents from the child / next chains (bounded walk)
        for i in 0..nodes.len() {
            let mut kid = nodes[i].child as usize;
            let mut steps = 0;
            while kid != 0 && kid < nodes.len() && steps < nodes.len() {
                nodes[kid].parent = Some(i);
                kid = nodes[kid].next as usize;
                steps += 1;
            }
        }
        let (loops, span) = match nodes.first() {
            Some(root) if root.kind == 0 => {
                let data = r.off(list + 4)?;
                (
                    r.u32(data)? != 0,
                    root.tracks.iter().map(|t| t.t1).fold(0.0f32, f32::max),
                )
            }
            _ => (false, 0.0),
        };
        let order = node_order(&nodes);
        effects.push(Effect {
            nodes,
            loops,
            span,
            order,
        });
    }
    Some(Teb { effects })
}

// ── evaluation (research §3) ─────────────────────────────────────────

/// `FUN_8013212c`: piecewise-linear `(t, v)` keys at normalised life `t`
/// (clamped at 0; past the last key the last segment extrapolates). No keys
/// = 1.
pub fn key_eval(keys: &[(f32, f32)], t: f32) -> f32 {
    let n = keys.len();
    if n == 0 {
        return 1.0;
    }
    let t = t.max(0.0);
    if n == 1 {
        return keys[0].1;
    }
    let i = (1..n).find(|&i| keys[i].0 > t).unwrap_or(n - 1);
    let (a, b) = (keys[i - 1], keys[i]);
    if b.0 == a.0 {
        return b.1;
    }
    (t - a.0) / (b.0 - a.0) * (b.1 - a.1) + a.1
}

/// Colour keys `{r, g, b, _, t%}` at life percentage `pct` (`FUN_8012ce18`).
pub fn colour_eval(keys: &[[u8; 5]], pct: f32) -> V3 {
    let rgb = |k: &[u8; 5]| [k[0] as f32, k[1] as f32, k[2] as f32];
    let n = keys.len();
    if n == 0 {
        return [255.0; 3];
    }
    if n < 2 {
        return rgb(&keys[0]);
    }
    let (a, b) = if pct >= keys[n - 1][4] as f32 {
        (keys[n - 2], keys[n - 1])
    } else {
        let (mut a, mut b) = (keys[0], keys[0]);
        for k in &keys[1..] {
            a = b;
            b = *k;
            if k[4] as f32 > pct {
                break;
            }
        }
        (a, b)
    };
    let span = b[4] as f32 - a[4] as f32;
    let span = if span == 0.0 { 1.0 } else { span };
    let f = (pct - a[4] as f32) / span;
    let (a, b) = (rgb(&a), rgb(&b));
    [
        (a[0] + f * (b[0] - a[0])).clamp(0.0, 255.0),
        (a[1] + f * (b[1] - a[1])).clamp(0.0, 255.0),
        (a[2] + f * (b[2] - a[2])).clamp(0.0, 255.0),
    ]
}

/// A node's local transform at its clock `t` (`FUN_8012f788`), or `None`
/// when no track holds `t` (the node is inactive: a part stops spawning).
pub fn track_local(tracks: &[Track], t: f32) -> Option<Affine> {
    let tr = tracks.iter().find(|x| x.t0 <= t && t <= x.t1)?;
    let ks = &tr.keys;
    let n = ks.len();
    let first = ks.first()?;
    let (ia, a, b, f) = if n == 1 {
        (0, *first, *first, 0.0)
    } else {
        let ia = if first.t <= t {
            ks.iter().rposition(|k| k.t <= t).unwrap_or(0)
        } else {
            0
        }
        .min(n - 2);
        let (a, b) = (ks[ia], ks[ia + 1]);
        let f = if b.t != a.t {
            (t - a.t) / (b.t - a.t)
        } else {
            0.0
        };
        (ia, a, b, f)
    };
    let pos = if tr.spline && n > 1 {
        // Hermite with Catmull-Rom tangents, each scaled to the segment
        let d = sub(b.pos, a.pos);
        let seg = b.t - a.t;
        let m1 = match ks.get(ia + 2) {
            Some(c) if c.t != a.t => scale(sub(c.pos, a.pos), seg / (c.t - a.t)),
            _ => d,
        };
        let m0 = match ia.checked_sub(1).and_then(|i| ks.get(i)) {
            Some(p) if b.t != p.t => scale(sub(b.pos, p.pos), seg / (b.t - p.t)),
            _ => d,
        };
        let (f2, f3) = (f * f, f * f * f);
        let h = [
            2.0 * f3 - 3.0 * f2 + 1.0,
            -2.0 * f3 + 3.0 * f2,
            f3 - 2.0 * f2 + f,
            f3 - f2,
        ];
        add(
            add(scale(a.pos, h[0]), scale(b.pos, h[1])),
            add(scale(m0, h[2]), scale(m1, h[3])),
        )
    } else {
        add(a.pos, scale(sub(b.pos, a.pos), f))
    };
    Some(Affine {
        r: quat_mtx(slerp(a.quat, b.quat, f)),
        t: pos,
    })
}

/// `rand() % 10000` uniforms of `FUN_801322bc` (± x) / `FUN_80132354`
/// ([0, x)) — the reference's LCG; any uniform source gives the same look.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(seed)
    }
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (self.0 >> 16) & 0x7FFF
    }
    /// Uniform in `[-x, x)`.
    pub fn pm(&mut self, x: f32) -> f32 {
        if x == 0.0 {
            return 0.0;
        }
        (2.0 * x / 10000.0) * (self.next() % 10000) as f32 - x
    }
    /// Uniform in `[0, x)`.
    pub fn unit(&mut self, x: f32) -> f32 {
        if x == 0.0 {
            return 0.0;
        }
        x / 10000.0 * (self.next() % 10000) as f32
    }
}

/// The camera the billboards and ribbon edges face, in zan units:
/// `view_rot` rows = the camera's right / up / back axes (world → camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub view_rot: M3,
    pub pos: V3,
}

impl Camera {
    /// A look-at camera (GX / World convention: it looks down its −z).
    /// Degenerate input keeps an identity basis.
    pub fn look_at(eye: V3, target: V3, up: V3) -> Camera {
        let z = sub(eye, target);
        let nz = norm(z);
        let mut rot = M3_IDENTITY;
        if nz > 1e-9 {
            let z = scale(z, 1.0 / nz);
            let x = cross(up, z);
            let nx = norm(x);
            if nx > 1e-9 {
                let x = scale(x, 1.0 / nx);
                rot = [x, cross(z, x), z];
            }
        }
        Camera {
            view_rot: rot,
            pos: eye,
        }
    }

    /// [`look_at`](Self::look_at) from World metres.
    pub fn look_at_world(eye: V3, target: V3, up: V3, metres_per_unit: f32) -> Camera {
        let k = if metres_per_unit.abs() > 1e-12 {
            1.0 / metres_per_unit
        } else {
            1.0
        };
        Camera::look_at(scale(eye, k), scale(target, k), up)
    }
}

/// One drawn particle: the unit quad ([`QUAD`]) placed by `axes` (columns
/// x / y already sized, z the facing) at `centre`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    /// The emitting node (index in its effect).
    pub node: u8,
    pub centre: V3,
    pub axes: M3,
    pub uv: [[f32; 2]; 4],
    /// The flip-book cell's origin `(u0, v0)` (0, 0 without a flip-book).
    pub cell: [f32; 2],
    /// The flip-book's cell size in UV (0 without a flip-book).
    pub cell_size: f32,
    /// 0..255 each; the effect fade is in `a`.
    pub rgba: [f32; 4],
    pub tex: i16,
    pub blend: u8,
    /// Flipped default UVs (part flag 0x400).
    pub flip: bool,
    pub depth: bool,
}

impl Sprite {
    const ZERO: Sprite = Sprite {
        node: 0,
        centre: [0.0; 3],
        axes: [[0.0; 3]; 3],
        uv: UV_DEFAULT,
        cell: [0.0; 2],
        cell_size: 0.0,
        rgba: [0.0; 4],
        tex: -1,
        blend: 0,
        flip: false,
        depth: true,
    };
}

/// One ribbon point (world space, the caller's frame): the particle's
/// position and the strip's two edges there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RibbonPoint {
    pub pos: V3,
    pub left: V3,
    pub right: V3,
    life: f32,
}

/// A ribbon to draw: `points` oldest first; one colour for the strip.
pub struct RibbonView<'a> {
    pub node: u8,
    pub points: &'a VecDeque<RibbonPoint>,
    pub rgba: [f32; 4],
    pub tex: i16,
    pub blend: u8,
}

/// `FUN_80131764`: v at point `i` (oldest = 0) of `n`: 1 at the oldest,
/// 0 at the newest.
pub fn ribbon_v(i: usize, n: usize) -> f32 {
    if n < 2 || i + 1 >= n {
        return 0.0;
    }
    if i == 0 {
        return 1.0;
    }
    1.0 - i as f32 / n as f32
}

/// Receives an effect's drawables ([`EffectSim::emit`]).
pub trait FxSink {
    fn sprite(&mut self, s: &Sprite);
    fn ribbon(&mut self, r: &RibbonView);
}

struct Particle {
    alive: bool,
    age: f32,
    life: f32,
    speed: f32,
    pos0: V3,
    vdir: V3,
    size: f32,
    rgb: V3,
    spin: Option<[f32; 3]>,
    /// Gravity direction (unnormalised, as the game keeps it) and accel.
    gravity: Option<(V3, f32)>,
    anchor: Option<(V3, Quat)>,
    ribbon: VecDeque<RibbonPoint>,
    pose: Sprite,
    posed: bool,
}

struct PartState {
    node: usize,
    acc: f32,
    prev_t: Option<V3>,
    particles: Vec<Particle>,
    ribbon_cap: usize,
}

impl PartState {
    fn new(node: usize, part: &Part) -> PartState {
        let em = part.emitter.unwrap_or_default();
        let ribbon_cap = part
            .ribbon
            .map_or(0, |r| (r.segments as usize).min(RIBBON_MAX_POINTS));
        let particles = (0..em.max)
            .map(|_| Particle {
                alive: false,
                age: 0.0,
                life: 0.0,
                speed: 0.0,
                pos0: [0.0; 3],
                vdir: [0.0; 3],
                size: 1.0,
                rgb: [255.0; 3],
                spin: None,
                gravity: None,
                anchor: None,
                ribbon: VecDeque::with_capacity(ribbon_cap),
                pose: Sprite::ZERO,
                posed: false,
            })
            .collect();
        PartState {
            node,
            // the first round spawns at once (`FUN_8012be48`)
            acc: em.interval,
            prev_t: None,
            particles,
            ribbon_cap,
        }
    }

    fn reset(&mut self, part: &Part) {
        self.acc = part.emitter.map_or(0.0, |e| e.interval);
        self.prev_t = None;
        for q in &mut self.particles {
            q.alive = false;
            q.posed = false;
            q.ribbon.clear();
        }
    }

    fn alive(&self) -> usize {
        self.particles.iter().filter(|q| q.alive).count()
    }

    /// Move every world-space point by `d` (the world scroll).
    fn shift(&mut self, d: V3) {
        if let Some(p) = self.prev_t.as_mut() {
            *p = add(*p, d);
        }
        for q in self.particles.iter_mut().filter(|q| q.alive) {
            if let Some((p, _)) = q.anchor.as_mut() {
                *p = add(*p, d);
            }
            for pt in q.ribbon.iter_mut() {
                pt.pos = add(pt.pos, d);
                pt.left = add(pt.left, d);
                pt.right = add(pt.right, d);
            }
        }
    }

    fn spawn(&mut self, part: &Part, em: &Emitter, world: &Affine, age0: f32, rng: &mut Rng) {
        let Some(q) = self.particles.iter_mut().find(|q| !q.alive) else {
            return;
        };
        let (pos, radial) = match (part.shape, part.ellipsoid, part.ring) {
            (1, Some(el), _) => {
                let m = euler_yzx(rng.pm(2.0 * PI), rng.pm(2.0 * PI), rng.pm(2.0 * PI));
                let d = m[2];
                let r = if el.flags & 1 != 0 {
                    el.radii
                } else {
                    [
                        rng.pm(el.radii[0]),
                        rng.pm(el.radii[1]),
                        rng.pm(el.radii[2]),
                    ]
                };
                ([d[0] * r[0], d[1] * r[1], d[2] * r[2]], el.flags & 4 != 0)
            }
            (2, _, Some(rg)) => {
                let a = rng.pm(2.0 * PI);
                let (s, c) = a.sin_cos();
                let mut p = if rg.flags & 8 != 0 {
                    [s * rg.radii[0], c * rg.radii[1], 0.0]
                } else if rg.flags & 0x10 != 0 {
                    [c * rg.radii[0], 0.0, s * rg.radii[2]]
                } else {
                    [0.0, c * rg.radii[1], s * rg.radii[2]]
                };
                if rg.flags & 2 != 0 {
                    p = scale(p, rng.unit(1.0));
                }
                (p, rg.flags & 4 != 0)
            }
            _ => (
                [
                    rng.pm(em.spread[0]),
                    rng.pm(em.spread[1]),
                    rng.pm(em.spread[2]),
                ],
                false,
            ),
        };
        let vdir = if radial {
            let n = norm(pos);
            if n > 1e-4 {
                scale(pos, 1.0 / n)
            } else {
                [0.0; 3]
            }
        } else {
            let rs = em.rot_spread;
            let m = euler_yzx(rng.pm(rs[0]), rng.pm(rs[1]), rng.pm(rs[2]));
            col(&m, 1)
        };
        q.alive = true;
        q.posed = false;
        q.age = age0;
        q.life = (em.life + rng.pm(em.life_rand)).max(0.0);
        q.speed = em.speed + rng.pm(em.speed_rand);
        q.pos0 = pos;
        q.vdir = vdir;
        q.size = 1.0;
        q.rgb = [255.0; 3];
        q.ribbon.clear();
        // gravity: the block's direction turned by a random rotation, or
        // (an ellipsoid with radial velocity) inward; accel ± rand
        // (`FUN_8012c790` / `FUN_8012c92c`, then `FUN_8012c438`)
        q.gravity = part.gravity.map(|g| {
            let dir = if part.shape == 1 && radial {
                let n = norm(vdir);
                if n > 1e-12 {
                    scale(vdir, -1.0 / n)
                } else {
                    [0.0; 3]
                }
            } else {
                let rs = g.rot_spread;
                let m = euler_yzx(rng.pm(rs[0]), rng.pm(rs[1]), rng.pm(rs[2]));
                mv(&m, g.dir)
            };
            (dir, g.accel + rng.pm(g.accel_rand))
        });
        q.spin = part.spin.map(|s| {
            [
                s[0] + rng.pm(s[1]),
                s[2] + rng.pm(s[3]),
                s[4] + rng.pm(s[5]),
            ]
        });
        q.anchor = if part.flags & P_WORLD != 0 {
            Some((world.t, mtx_quat(&unit_cols(&world.r))))
        } else {
            None
        };
        if let Some(d) = &part.draw {
            let (lo, hi) = d.size_var;
            if lo != 0 || hi != 0 {
                q.size = 1.0 - rng.unit((100.0 - lo as f32) / 100.0)
                    + rng.unit((hi as f32 - 100.0) / 100.0);
            }
            if d.flags & D_COLOUR_KEYS == 0 {
                q.rgb = [d.colour[0] as f32, d.colour[1] as f32, d.colour[2] as f32];
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        part: &Part,
        world: &Affine,
        active: bool,
        cam: &Camera,
        fade: f32,
        dt: f32,
        rng: &mut Rng,
    ) {
        let em = part.emitter.unwrap_or_default();
        let sc = col_lengths(&world.r);
        let delta = self.prev_t.map_or([0.0; 3], |p| sub(world.t, p));
        let wquat = mtx_quat(&unit_cols(&world.r));
        self.prev_t = Some(world.t);
        if active && em.max > 0 && self.acc >= em.interval {
            let mut rnd = 0u32;
            loop {
                let free = em.max.saturating_sub(self.alive());
                for _ in 0..free.min(em.per_spawn) {
                    self.spawn(part, &em, world, em.interval * rnd as f32, rng);
                }
                rnd += 1;
                // (no free slot: the remaining rounds would spawn nothing)
                if em.interval <= 0.0 || free == 0 {
                    break;
                }
                self.acc -= em.interval;
                if self.acc <= em.interval {
                    break;
                }
            }
            self.acc = 0.0;
        }
        let node = self.node.min(u8::MAX as usize) as u8;
        let cap = self.ribbon_cap;
        for q in self.particles.iter_mut().filter(|q| q.alive) {
            if q.age >= q.life {
                if part.flags & P_IMMORTAL != 0 {
                    q.age = 0.0;
                } else {
                    q.alive = false;
                    q.posed = false;
                    q.ribbon.clear();
                    continue;
                }
            }
            pose(
                part, &em, q, world, sc, delta, wquat, cam, fade, dt, node, cap,
            );
            q.age += dt;
        }
        self.acc += dt;
    }
}

/// `_pose` of the reference: the particle's sprite at its age (stored in
/// `q.pose`) and its ribbon push.
#[allow(clippy::too_many_arguments)]
fn pose(
    part: &Part,
    em: &Emitter,
    q: &mut Particle,
    world: &Affine,
    sc: V3,
    delta: V3,
    wquat: Quat,
    cam: &Camera,
    fade: f32,
    dt: f32,
    node: u8,
    ribbon_cap: usize,
) {
    let d = part.draw.as_ref();
    let age = q.age;
    let tn = if q.life > 0.0 { age / q.life } else { 0.0 };
    let mut pos = add(q.pos0, scale(q.vdir, q.speed * age));
    if let Some(s) = part.spin {
        pos = mv(
            &axis_angle([0.0, 1.0, 0.0], s[6] * age + s[7] * age * age * 0.5),
            pos,
        );
    }
    // gravity (`FUN_8012ce18`): a radial ring falls in the emitter's frame;
    // every other part in WORLD space, × the manager scale
    let ring_radial = part.shape == 2 && part.ring.is_some_and(|r| r.flags & 4 != 0);
    if let (Some((gdir, accel)), true) = (q.gravity, ring_radial) {
        pos = add(pos, scale(gdir, accel * age * age * 0.5));
    }
    let (mut wpos, r0) = match q.anchor.as_mut() {
        Some((apos, aq)) if part.flags & P_WORLD != 0 && age > 0.0 => {
            let fol = part.follow.map_or(0.0, |f| f.0);
            *aq = slerp(*aq, wquat, fol);
            *apos = add(*apos, scale(delta, fol));
            let r = scale_cols(&quat_mtx(*aq), sc);
            (add(mv(&r, pos), *apos), r)
        }
        _ => (add(mv(&world.r, pos), world.t), world.r),
    };
    if let (Some((gdir, accel)), false) = (q.gravity, ring_radial) {
        wpos = add(wpos, scale(gdir, age * age * accel * EFFECT_SCALE * 0.5));
    }
    // billboard: the camera's basis (mode 0) or the emitter's frame laid flat
    let mut r = if em.billboard == 0 {
        scale_cols(&transpose(&cam.view_rot), [sc[0], sc[1], 1.0])
    } else {
        mm(&r0, &LAY_FLAT)
    };
    if let Some([a0, a1, a2]) = q.spin {
        let z = col(&r, 2);
        let n = norm(z);
        let ax = if n > 1e-12 { scale(z, 1.0 / n) } else { z };
        r = mm(&axis_angle(ax, a2 * age * age * 0.5 + a1 * age + a0), &r);
    }
    let s = d.map_or(1.0, |d| key_eval(&d.scale_keys, tn));
    let rect = d.map_or([0.0, 0.0, 1.0, 1.0], |d| d.rect);
    let (w, h) = (
        q.size * s * (rect[2] - rect[0]),
        q.size * s * (rect[3] - rect[1]),
    );
    r = scale_cols(&r, [w, h, 1.0]);
    let (rgb, mut alpha) = match d {
        Some(d) if d.flags & D_COLOUR_KEYS != 0 => {
            let pct = if q.life > 0.0 {
                100.0 / q.life * age
            } else {
                0.0
            };
            (
                colour_eval(&d.colour_keys, pct),
                255.0 * key_eval(&d.alpha_keys, tn),
            )
        }
        Some(d) => (q.rgb, key_eval(&d.alpha_keys, tn) * d.colour[3] as f32),
        None => (q.rgb, 255.0),
    };
    alpha *= fade;
    let flip = part.flags & P_FLIP_UV != 0;
    let mut uv = if flip { UV_FLIPPED } else { UV_DEFAULT };
    let mut cell = [0.0f32; 2];
    let mut cell_size = 0.0f32;
    if let Some(fb) = d.and_then(|d| d.flipbook.filter(|_| d.flags & D_FLIPBOOK != 0)) {
        let (cols, cw) = fb.grid();
        let frame = if fb.frame_s > 0.0 {
            ((age / fb.frame_s) as u32) % (fb.frames.max(1) as u32)
        } else {
            0
        };
        let (u0, v0) = (cw * (frame % cols) as f32, cw * (frame / cols) as f32);
        cell = [u0, v0];
        cell_size = cw;
        uv = if flip {
            [[u0 + cw, v0 + cw], [u0, v0 + cw], [u0 + cw, v0], [u0, v0]]
        } else {
            [[u0, v0], [u0, v0 + cw], [u0 + cw, v0], [u0 + cw, v0 + cw]]
        };
    }
    let rgba = [rgb[0], rgb[1], rgb[2], alpha];
    q.pose = Sprite {
        node,
        centre: wpos,
        axes: r,
        uv,
        cell,
        cell_size,
        rgba,
        tex: d.map_or(-1, |d| d.tex),
        blend: d.map_or(0, |d| d.blend),
        flip,
        depth: part.flags & P_NO_DEPTH == 0,
    };
    q.posed = true;
    if let Some(rb) = part.ribbon {
        for pt in q.ribbon.iter_mut() {
            pt.life -= dt;
        }
        while q.ribbon.front().is_some_and(|p| p.life <= 0.0) {
            q.ribbon.pop_front();
        }
        if dt > 0.0 && ribbon_cap > 0 {
            let prev = q.ribbon.back().map_or(wpos, |p| p.pos);
            let side = cross(sub(cam.pos, wpos), sub(wpos, prev));
            let n = norm(side);
            let side = if n > 1e-4 {
                scale(side, rb.width * s * sc[0] / n)
            } else {
                [0.0; 3]
            };
            while q.ribbon.len() >= ribbon_cap {
                q.ribbon.pop_front();
            }
            q.ribbon.push_back(RibbonPoint {
                pos: wpos,
                left: add(wpos, side),
                right: sub(wpos, side),
                life: RIBBON_POINT_LIFE,
            });
        }
    }
}

/// One playing instance of a TEB effect (`CzanEff`): node clocks, every
/// part's emitter state and particle / ribbon pools. Built once (allocates);
/// stepping never allocates.
pub struct EffectSim {
    effect: usize,
    seed: u32,
    rng: Rng,
    clock: f32,
    steps: u64,
    started: bool,
    /// Per node: its part state (non-part nodes `None`).
    parts: Vec<Option<PartState>>,
    worlds: Vec<Affine>,
    /// The effect fade (`set_f_draw` / a stop's fade-out); 1 = full.
    pub fade: f32,
}

impl EffectSim {
    /// `None` when `effect` is not in the bank.
    pub fn new(teb: &Teb, effect: usize, seed: u32) -> Option<EffectSim> {
        let e = teb.effects.get(effect)?;
        let parts = e
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| n.part.as_ref().map(|p| PartState::new(i, p)))
            .collect();
        Some(EffectSim {
            effect,
            seed,
            rng: Rng::new(seed),
            clock: 0.0,
            steps: 0,
            started: false,
            parts,
            worlds: vec![Affine::IDENTITY; e.nodes.len()],
            fade: 1.0,
        })
    }

    pub fn effect(&self) -> usize {
        self.effect
    }

    /// Back to the moment before the first step (clocks 0, no particles).
    pub fn reset(&mut self, teb: &Teb) {
        self.rng = Rng::new(self.seed);
        self.clock = 0.0;
        self.steps = 0;
        self.started = false;
        let Some(e) = teb.effects.get(self.effect) else {
            return;
        };
        for (ps, n) in self.parts.iter_mut().zip(e.nodes.iter()) {
            if let (Some(ps), Some(p)) = (ps.as_mut(), n.part.as_ref()) {
                ps.reset(p);
            }
        }
    }

    /// Steps run since the start (or the last reset).
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Live particles over all parts.
    pub fn live_particles(&self) -> usize {
        self.parts.iter().flatten().map(|p| p.alive()).sum()
    }

    /// A non-looping effect past its track with nothing left alive.
    pub fn finished(&self, teb: &Teb) -> bool {
        let Some(e) = teb.effects.get(self.effect) else {
            return true;
        };
        self.started && !e.loops && self.clock > e.span && self.live_particles() == 0
    }

    /// One [`SIM_DT`] step. `attach` = the joint frame in zan units (unit
    /// rotation; ×[`EFFECT_SCALE`] applied here like set_mtx), `cam` the
    /// camera, `scroll` the world scroll velocity (units / s; zero before
    /// the flight switch).
    pub fn step(&mut self, teb: &Teb, attach: &Affine, cam: &Camera, scroll: V3) {
        let Some(e) = teb.effects.get(self.effect) else {
            return;
        };
        let dt = SIM_DT;
        if self.started && scroll != [0.0; 3] {
            let d = scale(scroll, -dt);
            for ps in self.parts.iter_mut().flatten() {
                ps.shift(d);
            }
        }
        let a = Affine {
            r: scale_cols(&attach.r, [EFFECT_SCALE; 3]),
            t: attach.t,
        };
        for &k in &e.order {
            let Some(nd) = e.nodes.get(k) else {
                continue;
            };
            let loc = track_local(&nd.tracks, self.clock);
            let par = match nd.parent.and_then(|p| self.worlds.get(p)) {
                Some(w) => *w,
                None => a,
            };
            let w = par.mul(&loc.unwrap_or(Affine::IDENTITY));
            if let Some(slot) = self.worlds.get_mut(k) {
                *slot = w;
            }
            if let (Some(Some(ps)), Some(p)) = (self.parts.get_mut(k), nd.part.as_ref()) {
                ps.step(p, &w, loc.is_some(), cam, self.fade, dt, &mut self.rng);
            }
        }
        self.started = true;
        self.steps += 1;
        self.clock += dt;
        if e.loops && e.span > 0.0 && self.clock > e.span {
            self.clock = 0.0;
        }
    }

    /// Catch the simulation up to `elapsed_s` since the effect started
    /// (steps at 0, Δ, 2Δ …): at most [`MAX_CATCH_UP_STEPS`] steps run, a
    /// longer gap is skipped; time going backwards (`elapsed_s` before the
    /// last step) resets. Returns the steps run.
    pub fn advance(
        &mut self,
        teb: &Teb,
        elapsed_s: f32,
        attach: &Affine,
        cam: &Camera,
        scroll: V3,
    ) -> u32 {
        if elapsed_s.is_nan() || elapsed_s < 0.0 {
            return 0;
        }
        let due = (elapsed_s / SIM_DT) as u64 + 1;
        if due < self.steps {
            self.reset(teb);
        }
        let want = due - self.steps;
        let run = want.min(MAX_CATCH_UP_STEPS as u64) as u32;
        for _ in 0..run {
            self.step(teb, attach, cam, scroll);
        }
        // a gap longer than the catch-up: skip the rest of it
        self.steps = self.steps.max(due);
        run
    }

    /// Hand every live particle's sprite and ribbon (≥ 2 points) to `sink`.
    pub fn emit(&self, teb: &Teb, sink: &mut impl FxSink) {
        let Some(e) = teb.effects.get(self.effect) else {
            return;
        };
        for ps in self.parts.iter().flatten() {
            let Some(part) = e.nodes.get(ps.node).and_then(|n| n.part.as_ref()) else {
                continue;
            };
            for q in ps.particles.iter().filter(|q| q.alive && q.posed) {
                sink.sprite(&q.pose);
                if let Some(rb) = part.ribbon {
                    if q.ribbon.len() >= 2 {
                        sink.ribbon(&RibbonView {
                            node: q.pose.node,
                            points: &q.ribbon,
                            rgba: q.pose.rgba,
                            tex: rb.tex,
                            blend: q.pose.blend,
                        });
                    }
                }
            }
        }
    }
}

// ── World bones (row-vector, metres) ─────────────────────────────────

/// A bone that collapses everything skinned to it (an unused pool entry).
pub const HIDDEN_BONE: [f32; 16] = [
    0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// The World row-vector bone placing a sprite-pool quad (the model's
/// vertices = [`QUAD`] in bone space, identity bind) on `s`, in metres.
pub fn sprite_bone(s: &Sprite, metres_per_unit: f32) -> [f32; 16] {
    let k = metres_per_unit;
    let (x, y, z) = (col(&s.axes, 0), col(&s.axes, 1), col(&s.axes, 2));
    [
        x[0] * k,
        x[1] * k,
        x[2] * k,
        0.0,
        y[0] * k,
        y[1] * k,
        y[2] * k,
        0.0,
        z[0] * k,
        z[1] * k,
        z[2] * k,
        0.0,
        s.centre[0] * k,
        s.centre[1] * k,
        s.centre[2] * k,
        1.0,
    ]
}

/// The World row-vector bone placing a ribbon-pool vertex pair (bone space
/// `(−1, 0, 0)` = the left edge, `(+1, 0, 0)` = the right) on `p`, metres.
pub fn ribbon_bone(p: &RibbonPoint, metres_per_unit: f32) -> [f32; 16] {
    let k = metres_per_unit;
    let half = scale(sub(p.right, p.left), 0.5 * k);
    let c = scale(add(p.left, p.right), 0.5 * k);
    [
        half[0], half[1], half[2], 0.0, //
        0.0, k, 0.0, 0.0, //
        0.0, 0.0, k, 0.0, //
        c[0], c[1], c[2], 1.0,
    ]
}

/// The ribbon pool's v scale for `n` live points of a strip baked with
/// `v = j / (cap − 1)` at bone `j` (bone 0 = the newest point): the
/// `m_vTexAnime.y` that maps the oldest live point to v = 1 (`uv / y`).
pub fn ribbon_v_scale(n: usize, cap: usize) -> f32 {
    if n < 2 || cap < 2 {
        return 1.0;
    }
    (n - 1) as f32 / (cap - 1) as f32
}

/// Premultiplied colour for an ADDITIVE draw (0..1, alpha 1): World's
/// collector forces an entry whose colour alpha is below 1 into the alpha
/// blend group, so the alpha rides the rgb (SRCALPHA / ONE is the same).
pub fn additive_colour(rgba: [f32; 4]) -> [f32; 4] {
    let a = (rgba[3] / 255.0).clamp(0.0, 1.0);
    [
        (rgba[0] / 255.0 * a).clamp(0.0, 1.0),
        (rgba[1] / 255.0 * a).clamp(0.0, 1.0),
        (rgba[2] / 255.0 * a).clamp(0.0, 1.0),
        1.0,
    ]
}

// ── the pool layout (`flight_fx.txt`, written by the content port) ───

/// Players with their own effect set (`FUN_8004b5a8`: P1..P4 colours).
pub const PLAYERS: usize = 4;
/// The layout's pseudo-player owning the stage effect's pools.
pub const STAGE_PLAYER: usize = 4;

/// Quads `[first, first + count)` of a sprite pool draw TPL `tex` with
/// these UVs / blend / depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteGroup {
    pub first: usize,
    pub count: usize,
    pub tex: i16,
    pub flip: bool,
    /// The flip-book cell size the quads' UVs are baked for (0 = none).
    pub cell: f32,
    pub additive: bool,
    pub depth: bool,
}

/// One ribbon strip of a ribbon pool: bones `[first_bone, + points)`, its
/// draw records `[first_record, + records)`; its material = its index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub first_bone: usize,
    pub points: usize,
    pub first_record: usize,
    pub records: usize,
    pub tex: i16,
    pub additive: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PoolKind {
    /// Quad `i` = mesh / draw record / material / bone `i`.
    Sprites {
        quads: usize,
        groups: Vec<SpriteGroup>,
    },
    Ribbons {
        bones: usize,
        records: usize,
        strips: Vec<Strip>,
    },
}

/// One pool model of one player.
#[derive(Clone, Debug, PartialEq)]
pub struct Pool {
    pub player: usize,
    pub model: String,
    pub kind: PoolKind,
}

impl Pool {
    /// Bones the model has (= what its instance publishes).
    pub fn bones(&self) -> usize {
        match &self.kind {
            PoolKind::Sprites { quads, .. } => *quads,
            PoolKind::Ribbons { bones, .. } => *bones,
        }
    }
    /// Draw records the model has.
    pub fn records(&self) -> usize {
        match &self.kind {
            PoolKind::Sprites { quads, .. } => *quads,
            PoolKind::Ribbons { records, .. } => *records,
        }
    }
}

/// The intro's stage effect (the sky burst): `effect` of the stage bank at
/// intro frame `frame` (60 Hz), anchored at `at` (zan units, no rotation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageEffect {
    pub frame: f32,
    pub at: V3,
    pub effect: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FxLayout {
    pub metres_per_unit: f32,
    pub pools: Vec<Pool>,
    pub stage_effect: Option<StageEffect>,
    /// The intro sound: `(intro frame, flight_fx/ member)`.
    pub stage_sound: Option<(f32, String)>,
}

impl FxLayout {
    /// Parse the manifest (grammar: the port script's docstring). Errors
    /// name the line; every range is checked against its pool.
    pub fn parse(text: &str) -> Result<FxLayout, String> {
        let mut out = FxLayout {
            metres_per_unit: 0.0,
            pools: Vec::new(),
            stage_effect: None,
            stage_sound: None,
        };
        let mut version = false;
        for (ln, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split_whitespace().collect();
            let bad = || format!("line {}: {:?}", ln + 1, line);
            let num = |i: usize| -> Result<usize, String> {
                f.get(i)
                    .and_then(|v| v.parse::<usize>().ok())
                    .ok_or_else(bad)
            };
            let flag = |i: usize| -> Result<bool, String> { Ok(num(i)? != 0) };
            let tex = |i: usize| -> Result<i16, String> {
                f.get(i).and_then(|v| v.parse::<i16>().ok()).ok_or_else(bad)
            };
            match f.first().copied() {
                Some("flight_fx") => {
                    if num(1)? != 1 {
                        return Err(format!("{}: unsupported version", bad()));
                    }
                    version = true;
                }
                Some("metres_per_unit") => {
                    out.metres_per_unit = f
                        .get(1)
                        .and_then(|v| v.parse::<f32>().ok())
                        .filter(|v| v.is_finite() && *v > 0.0)
                        .ok_or_else(bad)?;
                }
                Some("sprites") | Some("ribbons") => {
                    let player = num(1)?;
                    let model = f.get(2).ok_or_else(bad)?.to_string();
                    if player > STAGE_PLAYER || model.is_empty() {
                        return Err(bad());
                    }
                    let kind = if f[0] == "sprites" {
                        PoolKind::Sprites {
                            quads: num(3)?,
                            groups: Vec::new(),
                        }
                    } else {
                        PoolKind::Ribbons {
                            bones: num(3)?,
                            records: num(4)?,
                            strips: Vec::new(),
                        }
                    };
                    out.pools.push(Pool {
                        player,
                        model,
                        kind,
                    });
                }
                Some("group") => {
                    let cell = f
                        .get(5)
                        .and_then(|v| v.parse::<f32>().ok())
                        .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                        .ok_or_else(bad)?;
                    let g = SpriteGroup {
                        first: num(1)?,
                        count: num(2)?,
                        tex: tex(3)?,
                        flip: flag(4)?,
                        cell,
                        additive: flag(6)?,
                        depth: flag(7)?,
                    };
                    match out.pools.last_mut().map(|p| &mut p.kind) {
                        Some(PoolKind::Sprites { quads, groups })
                            if g.first.checked_add(g.count).is_some_and(|e| e <= *quads) =>
                        {
                            groups.push(g)
                        }
                        _ => return Err(format!("{}: outside a sprite pool", bad())),
                    }
                }
                Some("strip") => {
                    let st = Strip {
                        first_bone: num(1)?,
                        points: num(2)?,
                        first_record: num(3)?,
                        records: num(4)?,
                        tex: tex(5)?,
                        additive: flag(6)?,
                    };
                    match out.pools.last_mut().map(|p| &mut p.kind) {
                        Some(PoolKind::Ribbons {
                            bones,
                            records,
                            strips,
                        }) if st.points >= 2
                            && st
                                .first_bone
                                .checked_add(st.points)
                                .is_some_and(|e| e <= *bones)
                            && st
                                .first_record
                                .checked_add(st.records)
                                .is_some_and(|e| e <= *records) =>
                        {
                            strips.push(st)
                        }
                        _ => return Err(format!("{}: outside a ribbon pool", bad())),
                    }
                }
                Some("stage_effect") => {
                    let fl = |i: usize| -> Result<f32, String> {
                        f.get(i)
                            .and_then(|v| v.parse::<f32>().ok())
                            .filter(|v| v.is_finite())
                            .ok_or_else(bad)
                    };
                    out.stage_effect = Some(StageEffect {
                        frame: fl(1)?.max(0.0),
                        at: [fl(2)?, fl(3)?, fl(4)?],
                        effect: num(5)?,
                    });
                }
                Some("stage_sound") => {
                    let frame = f
                        .get(1)
                        .and_then(|v| v.parse::<f32>().ok())
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .ok_or_else(bad)?;
                    let member = f.get(2).ok_or_else(bad)?;
                    if member.contains('/') || member.contains("..") {
                        return Err(bad());
                    }
                    out.stage_sound = Some((frame, member.to_string()));
                }
                _ => return Err(format!("{}: unknown record", bad())),
            }
        }
        if !version {
            return Err("no `flight_fx 1` header".to_string());
        }
        if out.metres_per_unit <= 0.0 {
            return Err("no metres_per_unit".to_string());
        }
        Ok(out)
    }

    /// Indices (into `pools`) of player `p`'s pools, in file order.
    pub fn pools_of(&self, p: usize) -> Vec<usize> {
        (0..self.pools.len())
            .filter(|&i| self.pools[i].player == p)
            .collect()
    }
}

/// One material-parameter write (`frame_board::MatParam`'s shape, spelled
/// locally — this file mounts without `crate::`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatWrite {
    pub material: u16,
    pub index: u8,
    pub value: f32,
}

/// `m_vTexAnime` floats of a material's parameter block: (scaleU, scaleV,
/// offU, offV) — `uv' = uv / scale + off`.
pub const TEX_ANIME_SCALE_V: u8 = 1;
pub const TEX_ANIME_OFF_U: u8 = 2;
pub const TEX_ANIME_OFF_V: u8 = 3;

/// One pool instance's frame: the bones, record colours and material
/// writes it publishes. Sized once; [`clear`](Self::clear) per frame.
pub struct PoolFrame {
    pub bones: Vec<[f32; 16]>,
    pub colours: Vec<[f32; 4]>,
    pub mats: Vec<MatWrite>,
    /// Per group / strip: entries used this frame.
    used: Vec<usize>,
    /// Entries drawn this frame.
    pub drawn: usize,
}

/// A hidden entry's record colour (black: adds nothing even if drawn).
const HIDDEN_COLOUR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

impl PoolFrame {
    pub fn new(pool: &Pool) -> PoolFrame {
        let slots = match &pool.kind {
            PoolKind::Sprites { groups, .. } => groups.len(),
            PoolKind::Ribbons { strips, .. } => strips.len(),
        };
        let mats = match &pool.kind {
            PoolKind::Sprites { groups, .. } => groups
                .iter()
                .filter(|g| g.cell > 0.0)
                .map(|g| 2 * g.count)
                .sum(),
            PoolKind::Ribbons { strips, .. } => strips.len(),
        };
        PoolFrame {
            bones: vec![HIDDEN_BONE; pool.bones()],
            colours: vec![HIDDEN_COLOUR; pool.records()],
            mats: Vec::with_capacity(mats),
            used: vec![0; slots],
            drawn: 0,
        }
    }

    /// Everything hidden again (no allocation).
    pub fn clear(&mut self) {
        self.bones.iter_mut().for_each(|b| *b = HIDDEN_BONE);
        self.colours.iter_mut().for_each(|c| *c = HIDDEN_COLOUR);
        self.mats.clear();
        self.used.iter_mut().for_each(|u| *u = 0);
        self.drawn = 0;
    }

    fn mat(&mut self, w: MatWrite) {
        if self.mats.len() < self.mats.capacity() {
            self.mats.push(w);
        }
    }
}

/// A record colour for `rgba` (0..255): additive → premultiplied, alpha 1
/// ([`additive_colour`]); alpha-blended → straight 0..1.
pub fn record_colour(rgba: [f32; 4], additive: bool) -> [f32; 4] {
    if additive {
        additive_colour(rgba)
    } else {
        [
            (rgba[0] / 255.0).clamp(0.0, 1.0),
            (rgba[1] / 255.0).clamp(0.0, 1.0),
            (rgba[2] / 255.0).clamp(0.0, 1.0),
            (rgba[3] / 255.0).clamp(0.0, 1.0),
        ]
    }
}

/// Fills one player's pool frames from the effects' drawables: a sprite
/// takes the next free quad of the group matching its texture / UVs / blend
/// / depth, a ribbon the next free strip of its texture (its newest points
/// when longer). What finds no room is counted in `dropped`.
pub struct PoolWriter<'a> {
    pub pools: &'a [&'a Pool],
    pub frames: &'a mut [PoolFrame],
    pub metres_per_unit: f32,
    pub dropped: usize,
}

impl FxSink for PoolWriter<'_> {
    fn sprite(&mut self, s: &Sprite) {
        if s.tex < 0 {
            return;
        }
        let additive = s.blend == BLEND_ADDITIVE;
        for (pool, fr) in self.pools.iter().zip(self.frames.iter_mut()) {
            let PoolKind::Sprites { groups, .. } = &pool.kind else {
                continue;
            };
            for (gi, g) in groups.iter().enumerate() {
                let used = fr.used.get(gi).copied().unwrap_or(usize::MAX);
                if g.tex != s.tex
                    || g.flip != s.flip
                    || (g.cell - s.cell_size).abs() > 1e-4
                    || g.additive != additive
                    || g.depth != s.depth
                    || used >= g.count
                {
                    continue;
                }
                let q = g.first + used;
                if let (Some(b), Some(c)) = (fr.bones.get_mut(q), fr.colours.get_mut(q)) {
                    *b = sprite_bone(s, self.metres_per_unit);
                    *c = record_colour(s.rgba, additive);
                }
                if g.cell > 0.0 {
                    let material = q.min(u16::MAX as usize) as u16;
                    fr.mat(MatWrite {
                        material,
                        index: TEX_ANIME_OFF_U,
                        value: s.cell[0],
                    });
                    fr.mat(MatWrite {
                        material,
                        index: TEX_ANIME_OFF_V,
                        value: s.cell[1],
                    });
                }
                if let Some(u) = fr.used.get_mut(gi) {
                    *u += 1;
                }
                fr.drawn += 1;
                return;
            }
        }
        self.dropped += 1;
    }

    fn ribbon(&mut self, r: &RibbonView) {
        let additive = r.blend == BLEND_ADDITIVE;
        let k = self.metres_per_unit;
        for (pool, fr) in self.pools.iter().zip(self.frames.iter_mut()) {
            let PoolKind::Ribbons { strips, .. } = &pool.kind else {
                continue;
            };
            for (si, st) in strips.iter().enumerate() {
                if st.tex != r.tex
                    || st.additive != additive
                    || fr.used.get(si).copied().unwrap_or(1) != 0
                {
                    continue;
                }
                let n = r.points.len().min(st.points);
                let Some(oldest) = r.points.get(r.points.len() - n) else {
                    return;
                };
                let collapsed = RibbonPoint {
                    pos: oldest.pos,
                    left: oldest.pos,
                    right: oldest.pos,
                    life: 0.0,
                };
                // bone j = the j-th newest point; the rest collapse on the
                // oldest (zero width — degenerate, nothing drawn)
                for j in 0..st.points {
                    let p = if j < n {
                        r.points.get(r.points.len() - 1 - j).unwrap_or(&collapsed)
                    } else {
                        &collapsed
                    };
                    if let Some(b) = fr.bones.get_mut(st.first_bone + j) {
                        *b = ribbon_bone(p, k);
                    }
                }
                let c = record_colour(r.rgba, additive);
                for rec in st.first_record..st.first_record + st.records {
                    if let Some(x) = fr.colours.get_mut(rec) {
                        *x = c;
                    }
                }
                fr.mat(MatWrite {
                    material: si.min(u16::MAX as usize) as u16,
                    index: TEX_ANIME_SCALE_V,
                    value: ribbon_v_scale(n, st.points),
                });
                if let Some(u) = fr.used.get_mut(si) {
                    *u = 1;
                }
                fr.drawn += 1;
                return;
            }
        }
        self.dropped += 1;
    }
}

// ── who plays what, when (`FUN_8004b5a8`, research §1.1) ───────────────

/// Mode 0: the flight (from the switch). Mode 2: the take-off leap.
pub const MODE_FLY: usize = 0;
pub const MODE_LEAP: usize = 2;
/// The leap's frame in the 600-frame take-off (`FUN_80037354`: f 544).
pub const LEAP_FRAME: f32 = 544.0;
pub const TAKEOFF_FRAMES: f32 = 600.0;

/// The attach joints a dancer offers, in this order.
pub const JOINT_NAMES: [&str; 5] = ["Hips", "LeftHand", "RightHand", "LeftFoot", "RightFoot"];

/// `(effect, joint index into JOINT_NAMES, position only)` × 3 of player
/// `p` in `mode` (tables 0x80218ad8 / 0x80218b68: effects 8m+2p, 8m+2p+1
/// ×2; joints modes 0/1 [Hips, hands], mode 2 [Hips position-only, feet]).
pub fn mode_effects(mode: usize, p: usize) -> [(usize, usize, bool); 3] {
    let a = 8 * mode + 2 * p;
    if mode == MODE_LEAP {
        [(a, 0, true), (a + 1, 3, false), (a + 1, 4, false)]
    } else {
        [(a, 0, false), (a + 1, 1, false), (a + 1, 2, false)]
    }
}

/// One phase's three effect instances and when (real seconds) it began.
struct Phase {
    sims: Vec<(EffectSim, usize, bool)>,
    start: Option<f32>,
}

impl Phase {
    fn new(teb: &Teb, mode: usize, p: usize, seed: u32) -> Phase {
        let sims = mode_effects(mode, p)
            .iter()
            .enumerate()
            .filter_map(|(k, &(e, j, pos_only))| {
                let s = seed ^ ((mode as u32) << 8) ^ (k as u32).wrapping_mul(0x9E37_79B9);
                EffectSim::new(teb, e, s).map(|sim| (sim, j, pos_only))
            })
            .collect();
        Phase { sims, start: None }
    }

    /// Run the phase (`on` = its dance-time start has passed) to `real_s`.
    fn tick(
        &mut self,
        teb: &Teb,
        on: bool,
        real_s: f32,
        joints: &[Affine; 5],
        cam: &Camera,
        scroll: V3,
    ) {
        if !on {
            self.start = None;
            return;
        }
        let start = *self.start.get_or_insert(real_s);
        let elapsed = real_s - start;
        if elapsed < 0.0 {
            // the real clock went back without the dance clock: restart here
            self.start = Some(real_s);
        }
        for (sim, j, pos_only) in self.sims.iter_mut() {
            if sim.finished(teb) && elapsed >= 0.0 {
                continue;
            }
            let a = joints.get(*j).copied().unwrap_or(Affine::IDENTITY);
            let a = if *pos_only { a.position_only() } else { a };
            sim.advance(teb, elapsed.max(0.0), &a, cam, scroll);
        }
    }

    fn emit(&self, teb: &Teb, on: bool, sink: &mut impl FxSink) {
        if !on {
            return;
        }
        for (sim, _, _) in &self.sims {
            sim.emit(teb, sink);
        }
    }
}

/// A flying dancer's effects: the leap burst and the flight effects of
/// player `p`, and the frames of that player's pool instances.
pub struct DancerFx {
    pub player: usize,
    /// Pool indices into the layout (= `frames` order).
    pub pools: Vec<usize>,
    pub frames: Vec<PoolFrame>,
    leap: Phase,
    fly: Phase,
    /// Drawables that found no pool room (diagnostics).
    pub dropped: usize,
}

impl DancerFx {
    pub fn new(teb: &Teb, layout: &FxLayout, dancer: usize, seed: u32) -> DancerFx {
        let player = dancer % PLAYERS;
        let mut pools = layout.pools_of(player);
        pools.truncate(MAX_POOLS);
        let frames = pools
            .iter()
            .filter_map(|&i| layout.pools.get(i))
            .map(PoolFrame::new)
            .collect();
        let seed = seed ^ (dancer as u32).wrapping_mul(0x85EB_CA6B);
        DancerFx {
            player,
            pools,
            frames,
            leap: Phase::new(teb, MODE_LEAP, player, seed),
            fly: Phase::new(teb, MODE_FLY, player, seed),
            dropped: 0,
        }
    }

    /// One frame: `t` dance time (the leap at `leap_t`, the flight from
    /// `switch_t`), `real_s` the real clock the effects tick on, `joints` in
    /// [`JOINT_NAMES`] order (zan units). The world scrolls from the switch
    /// on (the leap's world-space leftovers stream too). Refills `frames`;
    /// returns the entries drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        teb: &Teb,
        layout: &FxLayout,
        t: f32,
        real_s: f32,
        leap_t: f32,
        switch_t: f32,
        joints: &[Affine; 5],
        cam: &Camera,
    ) -> usize {
        let flying = t >= switch_t;
        let scroll = if flying { FLIGHT_SCROLL } else { [0.0; 3] };
        let leaping = t >= leap_t;
        self.leap.tick(teb, leaping, real_s, joints, cam, scroll);
        self.fly.tick(teb, flying, real_s, joints, cam, scroll);
        for f in self.frames.iter_mut() {
            f.clear();
        }
        let mut flat: [&Pool; MAX_POOLS] = [&EMPTY_POOL; MAX_POOLS];
        for (dst, &i) in flat.iter_mut().zip(self.pools.iter()) {
            if let Some(p) = layout.pools.get(i) {
                *dst = p;
            }
        }
        let n = self.pools.len().min(self.frames.len()).min(MAX_POOLS);
        let mut w = PoolWriter {
            pools: &flat[..n],
            frames: &mut self.frames[..n],
            metres_per_unit: layout.metres_per_unit,
            dropped: 0,
        };
        self.leap.emit(teb, leaping, &mut w);
        self.fly.emit(teb, flying, &mut w);
        self.dropped += w.dropped;
        self.frames.iter().map(|f| f.drawn).sum()
    }
}

/// Pools one player may use (the port writes 1 sprite + 2 ribbon models).
pub const MAX_POOLS: usize = 8;

/// The intro's stage effect (the sky burst at the tunnel mouth): one
/// effect of the stage bank, started at intro frame `StageEffect::frame`,
/// ticked on the intro clock, drawn through the [`STAGE_PLAYER`] pools.
pub struct StageFx {
    pub spec: StageEffect,
    /// Pool indices into the layout (= `frames` order).
    pub pools: Vec<usize>,
    pub frames: Vec<PoolFrame>,
    sim: EffectSim,
    pub dropped: usize,
}

impl StageFx {
    /// `None` without a `stage_effect` line, its effect, or a pool.
    pub fn new(teb: &Teb, layout: &FxLayout, seed: u32) -> Option<StageFx> {
        let spec = layout.stage_effect?;
        let mut pools = layout.pools_of(STAGE_PLAYER);
        pools.truncate(MAX_POOLS);
        if pools.is_empty() {
            return None;
        }
        let frames = pools
            .iter()
            .filter_map(|&i| layout.pools.get(i))
            .map(PoolFrame::new)
            .collect();
        Some(StageFx {
            spec,
            pools,
            frames,
            sim: EffectSim::new(teb, spec.effect, seed ^ 0x5747_4346)?,
            dropped: 0,
        })
    }

    /// When the effect starts, seconds of intro clock.
    pub fn start_s(&self) -> f32 {
        self.spec.frame / 60.0
    }

    /// One frame at intro clock `intro_s` (real seconds since the take-off
    /// began); refills `frames`, returns the entries drawn. Before the start
    /// (or after a rewind past it) nothing draws.
    pub fn tick(&mut self, teb: &Teb, layout: &FxLayout, intro_s: f32, cam: &Camera) -> usize {
        for f in self.frames.iter_mut() {
            f.clear();
        }
        let elapsed = intro_s - self.start_s();
        if elapsed < 0.0 {
            if self.sim.steps() > 0 {
                self.sim.reset(teb);
            }
            return 0;
        }
        if self.sim.finished(teb) {
            return 0;
        }
        let at = Affine {
            r: M3_IDENTITY,
            t: self.spec.at,
        };
        self.sim.advance(teb, elapsed, &at, cam, [0.0; 3]);
        let mut flat: [&Pool; MAX_POOLS] = [&EMPTY_POOL; MAX_POOLS];
        for (dst, &i) in flat.iter_mut().zip(self.pools.iter()) {
            if let Some(p) = layout.pools.get(i) {
                *dst = p;
            }
        }
        let n = self.pools.len().min(self.frames.len()).min(MAX_POOLS);
        let mut w = PoolWriter {
            pools: &flat[..n],
            frames: &mut self.frames[..n],
            metres_per_unit: layout.metres_per_unit,
            dropped: 0,
        };
        self.sim.emit(teb, &mut w);
        self.dropped += w.dropped;
        self.frames.iter().map(|f| f.drawn).sum()
    }
}

/// The SCHEDULE time of a flight stage frame: the take-off (until `switch`,
/// = the take-off clip's length) runs on the REAL clock like the Wii's 60 Hz
/// intro (`real_s`, the music count in seconds), the flight after it on the
/// dance clock, continuous at the switch (`dance_at_switch` = the dance time
/// at real time `switch`). Without a flight it is the dance time.
pub fn flight_schedule_time(
    switch: Option<f32>,
    t_dance: f32,
    real_s: f32,
    dance_at_switch: f32,
) -> f32 {
    match switch {
        Some(s) if real_s < s => real_s,
        Some(s) => s + (t_dance - dance_at_switch),
        None => t_dance,
    }
}

/// Placeholder for unused writer slots (never matches: no groups/strips).
static EMPTY_POOL: Pool = Pool {
    player: 0,
    model: String::new(),
    kind: PoolKind::Sprites {
        quads: 0,
        groups: Vec::new(),
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    // ── a synthetic TEB writer (big-endian, TEB-relative offsets) ───────

    #[derive(Default)]
    struct B(Vec<u8>);

    impl B {
        fn at(&self) -> u32 {
            self.0.len() as u32
        }
        fn align(&mut self) {
            while self.0.len() % 4 != 0 {
                self.0.push(0);
            }
        }
        fn bytes(&mut self, v: &[u8]) -> u32 {
            let o = self.at();
            self.0.extend_from_slice(v);
            o
        }
        fn u32(&mut self, v: u32) -> u32 {
            self.bytes(&v.to_be_bytes())
        }
        fn f32s(&mut self, v: &[f32]) -> u32 {
            self.align();
            let o = self.at();
            for x in v {
                self.0.extend_from_slice(&x.to_be_bytes());
            }
            o
        }
        fn put_u32(&mut self, o: u32, v: u32) {
            self.0[o as usize..o as usize + 4].copy_from_slice(&v.to_be_bytes());
        }
        fn put(&mut self, o: u32, v: &[u8]) {
            self.0[o as usize..o as usize + v.len()].copy_from_slice(v);
        }
        fn zeros(&mut self, n: usize) -> u32 {
            self.align();
            let o = self.at();
            self.0.resize(self.0.len() + n, 0);
            o
        }
        /// `{u32 n, u32 table}` → n × 0x14 → keys.
        fn tracks(&mut self, tracks: &[(f32, f32, bool, Vec<Key>)]) -> u32 {
            let head = self.zeros(8);
            let table = self.zeros(0x14 * tracks.len());
            self.put_u32(head, tracks.len() as u32);
            self.put_u32(head + 4, table);
            for (i, (t0, t1, spline, keys)) in tracks.iter().enumerate() {
                let kp = self.zeros(0x20 * keys.len());
                for (k, key) in keys.iter().enumerate() {
                    let o = kp + 0x20 * k as u32;
                    let mut v = vec![key.t];
                    v.extend_from_slice(&key.pos);
                    v.extend_from_slice(&key.quat);
                    for (j, x) in v.iter().enumerate() {
                        self.put(o + 4 * j as u32, &x.to_be_bytes());
                    }
                }
                let q = table + 0x14 * i as u32;
                self.put_u32(q, keys.len() as u32);
                self.put_u32(q + 4, kp);
                self.put(q + 8, &t0.to_be_bytes());
                self.put(q + 12, &t1.to_be_bytes());
                self.put(q + 0x10, &[*spline as u8]);
            }
            head
        }
    }

    fn key(t: f32, pos: V3) -> Key {
        Key {
            t,
            pos,
            quat: [0.0; 4],
        }
    }

    /// A part as the writer serialises it (only the blocks whose flag is
    /// set are written).
    #[derive(Clone)]
    struct PartSpec {
        flags: u32,
        shape: u8,
        track: (f32, f32),
        em: Emitter,
        draw_flags: u32,
        colour: [u8; 4],
        colour_keys: Vec<[u8; 5]>,
        rect: [f32; 4],
        scale_keys: Vec<(f32, f32)>,
        alpha_keys: Vec<(f32, f32)>,
        tex: i16,
        size_var: (u8, u8),
        flipbook: (u16, u16, f32, u8),
        ribbon: (f32, u8, i16),
        spin: [f32; 8],
        follow: (f32, f32),
        shape_block: (u32, V3),
        gravity: [f32; 8],
    }

    fn base_part() -> PartSpec {
        PartSpec {
            flags: P_EMITTER | P_DRAW,
            shape: 0,
            track: (0.0, 100.0),
            em: Emitter {
                life: 1.0,
                interval: 0.1,
                max: 4,
                per_spawn: 1,
                ..Emitter::default()
            },
            draw_flags: 0,
            colour: [200, 100, 50, 128],
            colour_keys: Vec::new(),
            rect: [-0.5, -0.5, 0.5, 0.5],
            scale_keys: Vec::new(),
            alpha_keys: Vec::new(),
            tex: 3,
            size_var: (0, 0),
            flipbook: (64, 16, 0.05, 8),
            ribbon: (0.5, 10, 7),
            spin: [0.0; 8],
            follow: (0.0, 1.0),
            shape_block: (0, [1.0, 1.0, 1.0]),
            gravity: [0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0],
        }
    }

    fn write_part(b: &mut B, p: &PartSpec) -> u32 {
        let head = b.zeros(0x30);
        b.put_u32(head, p.flags);
        b.put(head + 0x2C, &[p.shape]);
        let tr = b.tracks(&[(
            p.track.0,
            p.track.1,
            false,
            vec![key(p.track.0, [0.0; 3]), key(p.track.1, [0.0; 3])],
        )]);
        b.put_u32(head + 0x28, tr);
        if p.flags & P_EMITTER != 0 {
            let e = &p.em;
            let o = b.f32s(&[
                e.life,
                e.life_rand,
                e.interval,
                e.spread[0],
                e.spread[1],
                e.spread[2],
                e.rot_spread[0],
                e.rot_spread[1],
                e.rot_spread[2],
                e.speed,
                e.speed_rand,
            ]);
            b.zeros(8);
            b.put(o + 0x2C, &(e.max as i16).to_be_bytes());
            b.put(o + 0x2E, &(e.per_spawn as i16).to_be_bytes());
            b.put(o + 0x32, &[e.billboard]);
            b.put_u32(head + 4, o);
        }
        if p.flags & P_DRAW != 0 {
            let colours = if p.draw_flags & D_COLOUR_KEYS != 0 {
                let o = b.zeros(8 * p.colour_keys.len().max(1));
                for (i, k) in p.colour_keys.iter().enumerate() {
                    b.put(o + 8 * i as u32, k);
                }
                o
            } else {
                b.bytes(&p.colour)
            };
            let flat = |ks: &[(f32, f32)]| ks.iter().flat_map(|&(t, v)| [t, v]).collect::<Vec<_>>();
            let sk = if p.scale_keys.is_empty() {
                0
            } else {
                b.f32s(&flat(&p.scale_keys))
            };
            let ak = if p.alpha_keys.is_empty() {
                0
            } else {
                b.f32s(&flat(&p.alpha_keys))
            };
            let fb = b.zeros(12);
            b.put(fb, &p.flipbook.0.to_be_bytes());
            b.put(fb + 2, &p.flipbook.1.to_be_bytes());
            b.put(fb + 4, &p.flipbook.2.to_be_bytes());
            b.put(fb + 8, &[p.flipbook.3]);
            let d = b.zeros(0x30);
            b.put_u32(d, p.draw_flags);
            b.put_u32(d + 4, colours);
            for (i, x) in p.rect.iter().enumerate() {
                b.put(d + 8 + 4 * i as u32, &x.to_be_bytes());
            }
            b.put_u32(d + 0x18, sk);
            b.put_u32(d + 0x1C, ak);
            b.put_u32(d + 0x20, fb);
            b.put(
                d + 0x24,
                &[
                    p.scale_keys.len() as u8,
                    p.alpha_keys.len() as u8,
                    BLEND_ADDITIVE,
                ],
            );
            b.put(d + 0x28, &p.tex.to_be_bytes());
            b.put(
                d + 0x2A,
                &[p.colour_keys.len() as u8, p.size_var.0, p.size_var.1],
            );
            b.put_u32(head + 12, d);
        }
        if p.flags & P_GRAVITY != 0 {
            let o = b.f32s(&p.gravity);
            b.put_u32(head + 8, o);
        }
        if p.flags & P_RIBBON != 0 {
            let o = b.zeros(8);
            b.put(o, &p.ribbon.0.to_be_bytes());
            b.put(o + 4, &[p.ribbon.1]);
            b.put(o + 6, &p.ribbon.2.to_be_bytes());
            b.put_u32(head + 20, o);
        }
        if p.flags & P_SPIN != 0 {
            let o = b.f32s(&p.spin);
            b.put_u32(head + 24, o);
        }
        if p.flags & P_FOLLOW != 0 {
            let o = b.f32s(&[p.follow.0, p.follow.1]);
            b.put_u32(head + 28, o);
        }
        if p.shape == 1 || p.shape == 2 {
            let o = b.zeros(16);
            b.put_u32(o, p.shape_block.0);
            for (i, x) in p.shape_block.1.iter().enumerate() {
                b.put(o + 4 + 4 * i as u32, &x.to_be_bytes());
            }
            b.put_u32(head + if p.shape == 1 { 0x20 } else { 0x24 }, o);
        }
        head
    }

    /// One effect: a root `(loop, span)` + `parts` as (parent node, spec)
    /// — node 0 is the root, part i is node i + 1.
    struct EffectSpec {
        loops: bool,
        span: f32,
        root_keys: Vec<Key>,
        parts: Vec<(usize, PartSpec)>,
    }

    fn effect(parts: Vec<(usize, PartSpec)>) -> EffectSpec {
        EffectSpec {
            loops: false,
            span: 2.0,
            root_keys: vec![key(0.0, [0.0; 3]), key(2.0, [0.0; 3])],
            parts,
        }
    }

    fn build(effects: &[EffectSpec]) -> Vec<u8> {
        let mut b = B::default();
        b.bytes(b"TEB\0");
        b.zeros(12);
        b.u32(0x18);
        b.u32((effects.len() as u32) << 16);
        let table = b.zeros(0x10 * effects.len());
        for (e, spec) in effects.iter().enumerate() {
            let n = spec.parts.len() + 1;
            let list = b.zeros(8 * n);
            // node 0: the root
            let tr = b.tracks(&[(0.0, spec.span, false, spec.root_keys.clone())]);
            let root = b.zeros(8);
            b.put_u32(root, spec.loops as u32);
            b.put_u32(root + 4, tr);
            b.put(list, &[0, 0, 0, 0]);
            b.put_u32(list + 4, root);
            for (i, (_parent, p)) in spec.parts.iter().enumerate() {
                let data = write_part(&mut b, p);
                let no = list + 8 * (i as u32 + 1);
                b.put(no, &[0, 0, 1, 0]);
                b.put_u32(no + 4, data);
            }
            // child / next chains from the parents
            let mut last_child: Vec<Option<usize>> = vec![None; n];
            for (i, (parent, _)) in spec.parts.iter().enumerate() {
                let idx = i + 1;
                match last_child[*parent] {
                    None => b.put(list + 8 * *parent as u32, &[idx as u8]),
                    Some(prev) => b.put(list + 8 * prev as u32 + 1, &[idx as u8]),
                }
                last_child[*parent] = Some(idx);
            }
            let eo = table + 0x10 * e as u32;
            b.put_u32(eo, list);
            b.put(eo + 8, &[n as u8]);
        }
        b.0
    }

    fn cam() -> Camera {
        Camera::look_at([0.0, 0.0, 100.0], [0.0; 3], [0.0, 1.0, 0.0])
    }

    struct Collect {
        sprites: Vec<Sprite>,
        ribbons: Vec<Vec<RibbonPoint>>,
    }

    impl Collect {
        fn new() -> Collect {
            Collect {
                sprites: Vec::new(),
                ribbons: Vec::new(),
            }
        }
    }

    impl FxSink for Collect {
        fn sprite(&mut self, s: &Sprite) {
            self.sprites.push(*s);
        }
        fn ribbon(&mut self, r: &RibbonView) {
            self.ribbons.push(r.points.iter().copied().collect());
        }
    }

    fn frame(sim: &EffectSim, teb: &Teb) -> Collect {
        let mut c = Collect::new();
        sim.emit(teb, &mut c);
        c
    }

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    fn approx3(a: V3, b: V3, eps: f32) -> bool {
        (0..3).all(|i| approx(a[i], b[i], eps))
    }

    // ── parsing ─────────────────────────────────────────────────────────

    #[test]
    fn parses_a_synthetic_bank() {
        let mut a = base_part();
        a.flags |= P_RIBBON | P_SPIN | P_FOLLOW | P_WORLD | P_FLIP_UV;
        a.spin = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        a.follow = (0.25, 1.0);
        a.alpha_keys = vec![(0.0, 1.0), (1.0, 0.0)];
        let mut b = base_part();
        b.shape = 2;
        b.shape_block = (0x10 | 4, [2.0, 3.0, 4.0]);
        b.draw_flags = D_COLOUR_KEYS | D_FLIPBOOK;
        b.colour_keys = vec![[255, 0, 0, 0, 0], [0, 0, 255, 0, 100]];
        let mut c = base_part();
        c.shape = 1;
        c.shape_block = (1, [5.0, 6.0, 7.0]);
        c.size_var = (80, 120);
        let mut e = effect(vec![(0, a), (0, b), (2, c)]);
        e.loops = true;
        let bank = build(&[e, effect(vec![])]);
        let teb = parse_teb(&bank).expect("parse");
        assert_eq!(teb.effects.len(), 2);
        let e0 = &teb.effects[0];
        assert!(e0.loops);
        assert_eq!(e0.span, 2.0);
        assert_eq!(e0.nodes.len(), 4);
        assert_eq!(e0.order, vec![0, 1, 2, 3]);
        assert_eq!(
            e0.nodes.iter().map(|n| n.parent).collect::<Vec<_>>(),
            vec![None, Some(0), Some(0), Some(2)]
        );
        let pa = e0.nodes[1].part.as_ref().unwrap();
        assert_eq!(pa.emitter.unwrap().max, 4);
        assert_eq!(pa.emitter.unwrap().interval, 0.1);
        let d = pa.draw.as_ref().unwrap();
        assert_eq!(
            (d.tex, d.blend, d.colour),
            (3, BLEND_ADDITIVE, [200, 100, 50, 128])
        );
        assert_eq!(d.alpha_keys, vec![(0.0, 1.0), (1.0, 0.0)]);
        assert!(d.flipbook.is_none());
        assert_eq!(
            pa.ribbon,
            Some(Ribbon {
                width: 0.5,
                segments: 10,
                tex: 7
            })
        );
        assert_eq!(pa.spin.unwrap()[7], 0.8);
        assert_eq!(pa.follow, Some((0.25, 1.0)));
        assert_eq!(pa.tracks.len(), 1);
        let pb = e0.nodes[2].part.as_ref().unwrap();
        assert_eq!(pb.ring.unwrap().radii, [2.0, 3.0, 4.0]);
        let db = pb.draw.as_ref().unwrap();
        assert_eq!(db.colour_keys.len(), 2);
        assert_eq!(db.flipbook.unwrap().grid(), (4, 0.25));
        let pc = e0.nodes[3].part.as_ref().unwrap();
        assert_eq!(pc.ellipsoid.unwrap().flags, 1);
        assert_eq!(pc.draw.as_ref().unwrap().size_var, (80, 120));
        assert!(teb.effects[1].nodes.len() == 1 && !teb.effects[1].loops);
    }

    #[test]
    fn truncated_or_garbage_banks_never_panic() {
        let mut p = base_part();
        p.flags |= P_RIBBON | P_SPIN;
        let bank = build(&[effect(vec![(0, p.clone()), (1, p)])]);
        assert!(parse_teb(&bank).is_some());
        for n in 0..bank.len() {
            // any prefix: no panic; a cut through a block is a clean None
            let _ = parse_teb(&bank[..n]);
        }
        assert!(parse_teb(&bank[..bank.len() / 2]).is_none());
        assert!(parse_teb(b"TEX\0").is_none());
        // garbage after a valid magic, and bit flips of the real bank
        let mut rng = Rng::new(7);
        for _ in 0..300 {
            let mut g = bank.clone();
            for _ in 0..8 {
                let i = (rng.next() as usize * 7 + rng.next() as usize) % g.len();
                g[i] ^= (rng.next() & 0xFF) as u8;
            }
            if let Some(teb) = parse_teb(&g) {
                // whatever parsed must also simulate without a panic
                for e in 0..teb.effects.len() {
                    if let Some(mut sim) = EffectSim::new(&teb, e, 1) {
                        for _ in 0..5 {
                            sim.step(&teb, &Affine::IDENTITY, &cam(), FLIGHT_SCROLL);
                        }
                        let _ = frame(&sim, &teb);
                    }
                }
            }
        }
    }

    // ── evaluation ──────────────────────────────────────────────────────

    #[test]
    fn keys_and_colours() {
        assert_eq!(key_eval(&[], 0.5), 1.0);
        assert_eq!(key_eval(&[(0.0, 3.0)], 0.5), 3.0);
        let k = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)];
        assert!(approx(key_eval(&k, 0.25), 0.5, 1e-6));
        assert!(approx(key_eval(&k, 0.75), 0.5, 1e-6));
        assert_eq!(key_eval(&k, -1.0), 0.0); // clamped at 0
        assert!(approx(key_eval(&k, 1.5), -1.0, 1e-6)); // last segment extrapolates
        let c = [[255, 0, 0, 0, 0], [0, 0, 255, 0, 50], [0, 255, 0, 0, 100]];
        assert_eq!(colour_eval(&c, 0.0), [255.0, 0.0, 0.0]);
        assert!(approx3(colour_eval(&c, 25.0), [127.5, 0.0, 127.5], 1e-3));
        assert!(approx3(colour_eval(&c, 75.0), [0.0, 127.5, 127.5], 1e-3));
        assert_eq!(colour_eval(&c, 150.0), [0.0, 255.0, 0.0]); // clamped
        assert_eq!(colour_eval(&c[..1], 40.0), [255.0, 0.0, 0.0]);
    }

    #[test]
    fn tracks_linear_spline_and_slerp() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let lin = Track {
            t0: 0.0,
            t1: 1.0,
            spline: false,
            keys: vec![
                key(0.0, [0.0; 3]),
                Key {
                    t: 1.0,
                    pos: [2.0, 4.0, 6.0],
                    quat: [0.0, s, 0.0, s], // 90° about y
                },
            ],
        };
        let m = track_local(std::slice::from_ref(&lin), 0.5).unwrap();
        assert!(approx3(m.t, [1.0, 2.0, 3.0], 1e-6));
        // halfway: 45° about y — x axis → (cos 45, 0, −sin 45)
        assert!(approx3(col(&m.r, 0), [s, 0.0, -s], 1e-5));
        assert!(track_local(std::slice::from_ref(&lin), 1.5).is_none());
        // a spline passes through its keys and is smooth between
        let sp = Track {
            t0: 0.0,
            t1: 3.0,
            spline: true,
            keys: (0..4)
                .map(|i| key(i as f32, [(i * i) as f32, 0.0, 0.0]))
                .collect(),
        };
        let tr = std::slice::from_ref(&sp);
        for i in 0..3 {
            assert!(approx(
                track_local(tr, i as f32).unwrap().t[0],
                (i * i) as f32,
                1e-5
            ));
        }
        let mid = track_local(tr, 1.5).unwrap().t[0];
        assert!(mid > 1.0 && mid < 4.0, "{mid}");
        // the identity (all-zero) quaternion
        assert_eq!(track_local(tr, 0.5).unwrap().r, M3_IDENTITY);
    }

    // ── spawning ────────────────────────────────────────────────────────

    fn sim_of(part: PartSpec) -> (Teb, EffectSim) {
        let teb = parse_teb(&build(&[effect(vec![(0, part)])])).unwrap();
        let sim = EffectSim::new(&teb, 0, 1).unwrap();
        (teb, sim)
    }

    #[test]
    fn spawn_rate_cap_and_death() {
        let mut p = base_part();
        p.em.life = 0.25;
        p.em.interval = 0.1;
        p.em.max = 3;
        p.track = (0.0, 1.0);
        let (teb, mut sim) = sim_of(p);
        let mut alive = Vec::new();
        for _ in 0..150 {
            sim.step(&teb, &Affine::IDENTITY, &cam(), [0.0; 3]);
            alive.push(sim.live_particles());
            assert!(sim.live_particles() <= 3);
        }
        assert_eq!(alive[0], 1, "the first round spawns at once");
        // one per 0.1 s, each living 0.25 s: 2–3 alive in the steady state
        assert!(
            alive[20..60].iter().all(|&n| (2..=3).contains(&n)),
            "{alive:?}"
        );
        // the part's track ends at 1 s: no new spawns, the last die by 1.3 s
        assert_eq!(alive[85], 0, "{alive:?}");
        assert!(sim.finished(&teb), "non-looping effect with nothing alive");
    }

    #[test]
    fn interval_zero_and_immortal() {
        let mut p = base_part();
        p.em.interval = 0.0;
        p.em.per_spawn = 2;
        p.em.max = 5;
        p.em.life = 0.05;
        p.flags |= P_IMMORTAL;
        let (teb, mut sim) = sim_of(p);
        let mut n = Vec::new();
        for _ in 0..30 {
            sim.step(&teb, &Affine::IDENTITY, &cam(), [0.0; 3]);
            n.push(sim.live_particles());
        }
        assert_eq!(&n[..4], &[2, 4, 5, 5]);
        assert!(n.iter().skip(2).all(|&x| x == 5), "immortal: none ever die");
        assert!(!sim.finished(&teb));
    }

    #[test]
    fn looping_root_restarts_the_clock() {
        let mut p = base_part();
        p.track = (0.0, 0.05); // spawns only in the first 0.05 s of a loop
        p.em.interval = 1.0;
        p.em.life = 0.1;
        let mut e = effect(vec![(0, p)]);
        e.loops = true;
        e.span = 0.5;
        e.root_keys = vec![key(0.0, [0.0; 3]), key(0.5, [0.0; 3])];
        let teb = parse_teb(&build(&[e])).unwrap();
        let mut sim = EffectSim::new(&teb, 0, 1).unwrap();
        let mut births = 0;
        let mut prev = 0;
        for _ in 0..90 {
            sim.step(&teb, &Affine::IDENTITY, &cam(), [0.0; 3]);
            let n = sim.live_particles();
            if n > prev {
                births += 1;
            }
            prev = n;
        }
        // 1.5 s = three loops; the first spawns at once, the later loops
        // once the accumulator has passed the interval
        assert!(births >= 2, "{births}");
    }

    // ── poses ───────────────────────────────────────────────────────────

    #[test]
    fn billboards_face_the_camera_or_lie_flat() {
        let mut p = base_part();
        p.rect = [0.0, 0.0, 2.0, 1.0];
        p.em.max = 1;
        let (teb, mut sim) = sim_of(p.clone());
        // a camera on +x looking at the origin
        let c = Camera::look_at([50.0, 0.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0]);
        sim.step(&teb, &Affine::IDENTITY, &c, [0.0; 3]);
        let f = frame(&sim, &teb);
        let s = f.sprites[0];
        let (x, y, z) = (col(&s.axes, 0), col(&s.axes, 1), col(&s.axes, 2));
        assert!(approx(norm(x), 2.0 * EFFECT_SCALE, 1e-4));
        assert!(approx(norm(y), 1.0 * EFFECT_SCALE, 1e-4));
        assert!(
            approx3(scale(z, 1.0 / norm(z)), [1.0, 0.0, 0.0], 1e-5),
            "faces +x"
        );
        assert_eq!(s.uv, UV_DEFAULT);
        assert_eq!(s.rgba, [200.0, 100.0, 50.0, 128.0]);
        // lay flat: the quad in the emitter's XZ plane
        p.em.billboard = 1;
        p.flags |= P_FLIP_UV;
        let (teb, mut sim) = sim_of(p);
        sim.step(&teb, &Affine::IDENTITY, &c, [0.0; 3]);
        let s = frame(&sim, &teb).sprites[0];
        assert!(approx3(col(&s.axes, 0), [20.0, 0.0, 0.0], 1e-4));
        assert!(approx3(col(&s.axes, 1), [0.0, 0.0, -10.0], 1e-4));
        assert_eq!(s.uv, UV_FLIPPED);
    }

    #[test]
    fn emitter_frame_scale_and_orbit() {
        let mut p = base_part();
        p.flags |= P_SPIN;
        p.em.max = 1;
        p.em.spread = [0.0; 3];
        p.em.speed = 1.0; // along +y
        p.spin = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, PI, 0.0]; // orbit π rad/s
        let (teb, mut sim) = sim_of(p);
        let attach = Affine {
            r: M3_IDENTITY,
            t: [5.0, 0.0, 0.0],
        };
        for _ in 0..31 {
            sim.step(&teb, &attach, &cam(), [0.0; 3]);
        }
        // posed at age 0.5 s: (0, 0.5, 0) local, ×10, orbit about y keeps y
        let s = frame(&sim, &teb).sprites[0];
        assert!(approx3(s.centre, [5.0, 5.0, 0.0], 1e-3), "{:?}", s.centre);
        let mut p = base_part();
        p.em.max = 1;
        p.em.spread = [1.0, 0.0, 0.0];
        p.flags |= P_SPIN;
        p.spin = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, PI, 0.0];
        let (teb, mut sim) = sim_of(p);
        for _ in 0..31 {
            sim.step(&teb, &Affine::IDENTITY, &cam(), [0.0; 3]);
        }
        let s = frame(&sim, &teb).sprites[0];
        // a point on x orbits about y: after 0.5 s (π/2) it sits on ∓z, r kept
        assert!(approx(s.centre[1], 0.0, 1e-4));
        assert!(approx(
            (s.centre[0].powi(2) + s.centre[2].powi(2)).sqrt(),
            norm(s.centre),
            1e-4
        ));
        assert!(approx(s.centre[0].abs(), 0.0, 0.6), "{:?}", s.centre);
    }

    #[test]
    fn flipbook_cells_follow_age() {
        let mut p = base_part();
        p.draw_flags = D_FLIPBOOK;
        p.em.max = 1;
        p.flipbook = (64, 16, 0.05, 8); // 4 × 4 grid, 8 frames
        let (teb, mut sim) = sim_of(p);
        let mut cells = Vec::new();
        for _ in 0..20 {
            sim.step(&teb, &Affine::IDENTITY, &cam(), [0.0; 3]);
            cells.push(frame(&sim, &teb).sprites[0].cell);
        }
        // posed at age i/60: frame = floor(age / 0.05)
        assert_eq!(cells[0], [0.0, 0.0]);
        assert_eq!(cells[7], [0.5, 0.0]); // age 0.1167 → frame 2
        assert_eq!(cells[16], [0.25, 0.25]); // age 0.2667 → frame 5
        let s = frame(&sim, &teb).sprites[0];
        assert!(approx(s.uv[3][0] - s.uv[0][0], 0.25, 1e-6));
    }

    #[test]
    fn colour_keys_and_alpha_keys_over_life() {
        let mut p = base_part();
        p.draw_flags = D_COLOUR_KEYS;
        p.colour_keys = vec![[0, 0, 255, 0, 0], [255, 0, 0, 0, 100]];
        p.alpha_keys = vec![(0.0, 1.0), (1.0, 0.0)];
        p.em.max = 1;
        p.em.life = 1.0;
        let (teb, mut sim) = sim_of(p);
        for _ in 0..31 {
            sim.step(&teb, &Affine::IDENTITY, &cam(), [0.0; 3]);
        }
        let s = frame(&sim, &teb).sprites[0]; // age 0.5
        assert!(approx(s.rgba[0], 127.5, 0.5) && approx(s.rgba[2], 127.5, 0.5));
        assert!(approx(s.rgba[3], 127.5, 0.5));
    }

    #[test]
    fn gravity_falls_in_world_space_by_the_manager_scale() {
        let mut p = base_part();
        p.flags |= P_GRAVITY;
        p.em.max = 1;
        p.em.life = 5.0;
        let (teb, mut sim) = sim_of(p);
        assert_eq!(
            teb.effects[0].nodes[1]
                .part
                .as_ref()
                .unwrap()
                .gravity
                .unwrap()
                .accel,
            2.0
        );
        // the emitter turned 90° about z: gravity is world-space (dir −y)
        let r = axis_angle([0.0, 0.0, 1.0], PI / 2.0);
        let a = Affine { r, t: [0.0; 3] };
        for _ in 0..61 {
            sim.step(&teb, &a, &cam(), [0.0; 3]);
        }
        // age 1 s: Δ = dir · accel · a² / 2 · 10 = (0, −10, 0)
        let s = frame(&sim, &teb).sprites[0];
        assert!(approx3(s.centre, [0.0, -10.0, 0.0], 1e-3), "{:?}", s.centre);
    }

    // ── ribbons and the world scroll ────────────────────────────────────

    fn ribbon_part(segments: u8) -> PartSpec {
        let mut p = base_part();
        p.flags |= P_RIBBON | P_IMMORTAL;
        p.em.max = 1;
        p.ribbon = (0.5, segments, 7);
        p
    }

    #[test]
    fn ribbon_edges_cap_and_point_life() {
        let (teb, mut sim) = sim_of(ribbon_part(10));
        // the emitter moves +x one unit a step; the camera looks down −z
        for i in 0..30 {
            let a = Affine {
                r: M3_IDENTITY,
                t: [i as f32, 0.0, 0.0],
            };
            sim.step(&teb, &a, &cam(), [0.0; 3]);
        }
        let f = frame(&sim, &teb);
        let pts = &f.ribbons[0];
        assert_eq!(pts.len(), 10, "capped at the segment count");
        let p = pts[5];
        let side = sub(p.left, p.pos);
        // half width = width × scale key × emitter scale x = 0.5 × 1 × 10
        assert!(approx(norm(side), 5.0, 1e-4));
        assert!(
            approx(dot(side, [1.0, 0.0, 0.0]), 0.0, 1e-4),
            "⟂ the motion"
        );
        assert!(approx3(sub(p.pos, p.right), side, 1e-4));
        // a long ribbon keeps 1 s of points (60 Hz)
        let (teb, mut sim) = sim_of(ribbon_part(255));
        for i in 0..200 {
            let a = Affine {
                r: M3_IDENTITY,
                t: [i as f32, 0.0, 0.0],
            };
            sim.step(&teb, &a, &cam(), [0.0; 3]);
        }
        let n = frame(&sim, &teb).ribbons[0].len();
        assert!((59..=61).contains(&n), "{n}");
        assert_eq!(ribbon_v(0, 5), 1.0);
        assert_eq!(ribbon_v(4, 5), 0.0);
        assert!(approx(ribbon_v(2, 5), 0.6, 1e-6));
    }

    #[test]
    fn world_scroll_streams_the_trail_behind() {
        let (teb, mut sim) = sim_of(ribbon_part(30));
        // still flyer: without the scroll the trail collapses on the emitter
        for _ in 0..20 {
            sim.step(&teb, &Affine::IDENTITY, &cam(), [0.0; 3]);
        }
        let f = frame(&sim, &teb);
        assert!(f.ribbons[0].iter().all(|p| norm(p.pos) < 1e-4));
        // with MUSIC FIT's scroll the points stream to −z, 3 units a step
        let (teb, mut sim) = sim_of(ribbon_part(30));
        // a side camera (one looking along the scroll sees an edge-on strip)
        let side = Camera::look_at([100.0, 0.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0]);
        for _ in 0..20 {
            sim.step(&teb, &Affine::IDENTITY, &side, FLIGHT_SCROLL);
        }
        let pts = frame(&sim, &teb).ribbons[0].clone();
        let newest = pts[pts.len() - 1];
        let oldest = pts[0];
        assert!(approx3(newest.pos, [0.0; 3], 1e-4));
        assert!(approx(oldest.pos[2], -3.0 * (pts.len() - 1) as f32, 1e-2));
        // the strip has width (the scroll gives it a direction; the very
        // first point, pushed with no predecessor, has none)
        assert!(approx(norm(sub(oldest.left, oldest.pos)), 0.0, 1e-6));
        let p = pts[1];
        let w = sub(p.left, p.pos);
        assert!(approx(norm(w), 5.0, 1e-3));
        assert!(
            approx3(scale(w, 1.0 / 5.0), [0.0, 1.0, 0.0], 1e-4)
                || approx3(scale(w, -1.0 / 5.0), [0.0, 1.0, 0.0], 1e-4)
        );
    }

    #[test]
    fn world_space_particles_drop_behind_unless_following() {
        for (follow, expect_z) in [(0.0f32, -3.0 * 30.0), (1.0, 0.0)] {
            let mut p = base_part();
            p.flags |= P_WORLD | P_FOLLOW;
            p.follow = (follow, 1.0);
            p.em.max = 1;
            p.em.life = 2.0;
            let (teb, mut sim) = sim_of(p);
            for _ in 0..31 {
                sim.step(&teb, &Affine::IDENTITY, &cam(), FLIGHT_SCROLL);
            }
            let s = frame(&sim, &teb).sprites[0];
            assert!(
                approx(s.centre[2], expect_z, 0.05),
                "follow {follow}: {:?}",
                s.centre
            );
        }
    }

    #[test]
    fn advance_catches_up_skips_and_resets() {
        let (teb, mut sim) = sim_of(base_part());
        let (a, c) = (Affine::IDENTITY, cam());
        assert_eq!(sim.advance(&teb, 0.0, &a, &c, [0.0; 3]), 1);
        assert_eq!(sim.advance(&teb, 0.0, &a, &c, [0.0; 3]), 0, "same frame");
        assert_eq!(sim.advance(&teb, 2.0 * SIM_DT + 1e-4, &a, &c, [0.0; 3]), 2);
        // a 1 s hitch: 8 steps run, the rest skipped
        assert_eq!(
            sim.advance(&teb, 1.0 + 1e-4, &a, &c, [0.0; 3]),
            MAX_CATCH_UP_STEPS
        );
        assert_eq!(sim.steps(), 61);
        // backwards: reset and replay from the start
        assert_eq!(sim.advance(&teb, 3.0 * SIM_DT + 1e-4, &a, &c, [0.0; 3]), 4);
        assert_eq!(sim.steps(), 4);
        assert_eq!(sim.advance(&teb, f32::NAN, &a, &c, [0.0; 3]), 0);
    }

    // ── pools ───────────────────────────────────────────────────────────

    const LAYOUT: &str = "# test\nflight_fx 1\nmetres_per_unit 0.5\n\
        sprites 0 fx_t_s0a 6\n\
        group 0 2 3 0 0.000000 1 1\n\
        group 2 4 5 0 0.250000 1 1\n\
        ribbons 0 fx_t_r0a 12 3\n\
        strip 0 8 0 2 7 1\n\
        strip 8 4 2 1 7 1\n\
        sprites 1 fx_t_s1a 1\n\
        group 0 1 3 0 0 1 1\n";

    #[test]
    fn layout_parses_and_rejects_bad_ranges() {
        let l = FxLayout::parse(LAYOUT).unwrap();
        assert_eq!(l.metres_per_unit, 0.5);
        assert_eq!(l.pools.len(), 3);
        assert_eq!(l.pools_of(0), vec![0, 1]);
        assert_eq!(l.pools_of(1), vec![2]);
        assert_eq!((l.pools[0].bones(), l.pools[0].records()), (6, 6));
        assert_eq!((l.pools[1].bones(), l.pools[1].records()), (12, 3));
        let PoolKind::Sprites { groups, .. } = &l.pools[0].kind else {
            panic!()
        };
        assert_eq!(groups[1].cell, 0.25);
        for bad in [
            "metres_per_unit 0.5\nsprites 0 m 1\n", // no header
            "flight_fx 2\nmetres_per_unit 0.5\n",   // version
            "flight_fx 1\n",                        // no scale
            "flight_fx 1\nmetres_per_unit 1\ngroup 0 1 3 0 0 1 1\n", // outside a pool
            "flight_fx 1\nmetres_per_unit 1\nsprites 0 m 2\ngroup 1 2 3 0 0 1 1\n", // past the pool
            "flight_fx 1\nmetres_per_unit 1\nribbons 0 m 4 1\nstrip 0 5 0 1 7 1\n",
            "flight_fx 1\nmetres_per_unit 1\nsprites 5 m 1\n", // player
            "flight_fx 1\nmetres_per_unit 1\nbogus\n",
        ] {
            assert!(FxLayout::parse(bad).is_err(), "{bad:?}");
        }
    }

    fn sprite(tex: i16, cell_size: f32, cell: [f32; 2]) -> Sprite {
        Sprite {
            centre: [2.0, 0.0, 0.0],
            axes: M3_IDENTITY,
            tex,
            cell,
            cell_size,
            blend: BLEND_ADDITIVE,
            rgba: [255.0, 0.0, 0.0, 127.5],
            ..Sprite::ZERO
        }
    }

    #[test]
    fn pool_writer_places_colours_and_drops_overflow() {
        let l = FxLayout::parse(LAYOUT).unwrap();
        let pools: Vec<&Pool> = l.pools_of(0).iter().map(|&i| &l.pools[i]).collect();
        let mut frames: Vec<PoolFrame> = pools.iter().map(|p| PoolFrame::new(p)).collect();
        let mut w = PoolWriter {
            pools: &pools,
            frames: &mut frames,
            metres_per_unit: 0.5,
            dropped: 0,
        };
        for _ in 0..3 {
            w.sprite(&sprite(3, 0.0, [0.0; 2])); // 2 fit, 1 dropped
        }
        w.sprite(&sprite(5, 0.25, [0.5, 0.25])); // the flip-book group
        w.sprite(&sprite(5, 0.0, [0.0; 2])); // no un-flip-booked tex-5 group
        w.sprite(&sprite(-1, 0.0, [0.0; 2])); // untextured: ignored, not dropped
        assert_eq!(w.dropped, 2);
        let f = &frames[0];
        assert_eq!(f.drawn, 3);
        assert_eq!(f.bones[0][12], 1.0, "centre × metres per unit");
        assert_eq!(f.colours[0], [0.5, 0.0, 0.0, 1.0], "premultiplied, alpha 1");
        assert_eq!(f.bones[3], HIDDEN_BONE);
        assert_eq!(f.colours[3], HIDDEN_COLOUR);
        assert_eq!(
            f.mats,
            vec![
                MatWrite {
                    material: 2,
                    index: TEX_ANIME_OFF_U,
                    value: 0.5
                },
                MatWrite {
                    material: 2,
                    index: TEX_ANIME_OFF_V,
                    value: 0.25
                }
            ]
        );
        // ribbons: newest point on bone 0, the rest collapsed, v rescaled
        let pts: VecDeque<RibbonPoint> = (0..3)
            .map(|i| RibbonPoint {
                pos: [i as f32, 0.0, 0.0],
                left: [i as f32, 1.0, 0.0],
                right: [i as f32, -1.0, 0.0],
                life: 1.0,
            })
            .collect();
        let view = |pts| RibbonView {
            node: 0,
            points: pts,
            rgba: [255.0, 255.0, 255.0, 255.0],
            tex: 7,
            blend: BLEND_ADDITIVE,
        };
        let mut w = PoolWriter {
            pools: &pools,
            frames: &mut frames,
            metres_per_unit: 1.0,
            dropped: 0,
        };
        w.ribbon(&view(&pts));
        w.ribbon(&view(&pts));
        w.ribbon(&view(&pts)); // both strips taken
        assert_eq!(w.dropped, 1);
        let r = &frames[1];
        assert_eq!(r.bones[0][12], 2.0, "bone 0 = the newest");
        assert_eq!(r.bones[0][1], -1.0, "half width (right − left) / 2 on y");
        assert_eq!(r.bones[2][12], 0.0, "bone 2 = the oldest");
        assert_eq!(r.bones[5][12], 0.0, "unused bones collapse on the oldest");
        assert_eq!(r.bones[5][1], 0.0, "with zero width");
        assert_eq!(r.colours[0], [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(
            r.colours[2],
            [1.0, 1.0, 1.0, 1.0],
            "the second strip's record"
        );
        assert_eq!(r.mats[0].index, TEX_ANIME_SCALE_V);
        assert!(approx(r.mats[0].value, 2.0 / 7.0, 1e-6));
        assert_eq!(r.mats[1].material, 1);
        // a frame clears without reallocating
        let cap = frames[0].mats.capacity();
        frames[0].clear();
        assert_eq!(frames[0].mats.capacity(), cap);
        assert!(frames[0].bones.iter().all(|b| *b == HIDDEN_BONE));
    }

    #[test]
    fn modes_follow_the_game_tables() {
        assert_eq!(
            mode_effects(MODE_FLY, 0),
            [(0, 0, false), (1, 1, false), (1, 2, false)]
        );
        assert_eq!(
            mode_effects(MODE_FLY, 3),
            [(6, 0, false), (7, 1, false), (7, 2, false)]
        );
        assert_eq!(
            mode_effects(MODE_LEAP, 1),
            [(18, 0, true), (19, 3, false), (19, 4, false)]
        );
    }

    #[test]
    fn dancer_fx_leaps_then_flies_into_its_pools() {
        // 24 effects: every one a single sprite emitter on tex 3 (+ a ribbon)
        let mut p = base_part();
        p.flags |= P_RIBBON | P_IMMORTAL;
        p.em.max = 1;
        p.track = (0.0, 100.0);
        let effects: Vec<EffectSpec> = (0..24).map(|_| effect(vec![(0, p.clone())])).collect();
        let teb = parse_teb(&build(&effects)).unwrap();
        let l = FxLayout::parse(
            "flight_fx 1\nmetres_per_unit 1\nsprites 2 s 6\ngroup 0 6 3 0 0 1 1\nribbons 2 r 30 3\nstrip 0 10 0 1 7 1\nstrip 10 10 1 1 7 1\nstrip 20 10 2 1 7 1\n",
        )
        .unwrap();
        let mut fx = DancerFx::new(&teb, &l, 2, 9);
        assert_eq!(fx.player, 2);
        assert_eq!(fx.pools, vec![0, 1]);
        let joints = [Affine::IDENTITY; 5];
        let (leap, switch) = (1.0, 2.0);
        let mut drawn = Vec::new();
        for i in 0..240 {
            let t = i as f32 / 60.0;
            drawn.push(fx.tick(&teb, &l, t, t, leap, switch, &joints, &cam()));
        }
        assert_eq!(drawn[30], 0, "nothing before the leap");
        // the leap: 3 sprites (+ ribbons once 2 points exist)
        assert_eq!(
            drawn[60], 3,
            "the first step: one ribbon point, no strip yet"
        );
        assert_eq!(drawn[64], 6);
        // the flight adds 3 more sprites; the 3 strips are full, 3 ribbons dropped
        assert_eq!(drawn[200], 9);
        assert!(fx.dropped > 0);
        // a rewind before the leap clears everything
        assert_eq!(
            fx.tick(&teb, &l, 0.5, 0.5, leap, switch, &joints, &cam()),
            0
        );
        assert_eq!(
            fx.tick(&teb, &l, 1.2, 1.2, leap, switch, &joints, &cam()),
            3
        );
        assert_eq!(
            fx.tick(&teb, &l, 1.3, 1.3, leap, switch, &joints, &cam()),
            3 + 3
        );
    }

    #[test]
    fn stage_effect_line_and_driver() {
        let l = FxLayout::parse(
            "flight_fx 1\nmetres_per_unit 1\nstage_effect 180 0 740 2730 0\nsprites 4 x 4\ngroup 0 4 3 0 0 1 1\n",
        )
        .unwrap();
        assert_eq!(
            l.stage_effect,
            Some(StageEffect {
                frame: 180.0,
                at: [0.0, 740.0, 2730.0],
                effect: 0
            })
        );
        assert_eq!(l.pools_of(STAGE_PLAYER), vec![0]);
        let ls =
            FxLayout::parse("flight_fx 1\nmetres_per_unit 1\nstage_sound 180 burst.pcm\n").unwrap();
        assert_eq!(ls.stage_sound, Some((180.0, "burst.pcm".to_string())));
        assert!(FxLayout::parse("flight_fx 1\nmetres_per_unit 1\nstage_sound 180 ../x\n").is_err());
        assert!(FxLayout::parse("flight_fx 1\nmetres_per_unit 1\nsprites 5 x 1\n").is_err());
        let mut p = base_part();
        p.em.max = 2;
        p.track = (0.0, 1.0);
        let teb = parse_teb(&build(&[effect(vec![(0, p)])])).unwrap();
        let mut fx = StageFx::new(&teb, &l, 1).unwrap();
        assert_eq!(fx.start_s(), 3.0);
        assert_eq!(fx.tick(&teb, &l, 2.9, &cam()), 0, "before frame 180");
        assert!(fx.tick(&teb, &l, 3.2, &cam()) > 0);
        let b = fx.frames[0].bones[0];
        assert!(
            approx(b[13], 740.0, 1e-3) && approx(b[14], 2730.0, 1e-3),
            "at the anchor"
        );
        assert_eq!(
            fx.tick(&teb, &l, 1.0, &cam()),
            0,
            "rewound before the start"
        );
        // the one-shot effect ends (track 2 s + particle life)
        let mut last = 1;
        for i in 0..400 {
            last = fx.tick(&teb, &l, 3.0 + i as f32 / 60.0, &cam());
        }
        assert_eq!(last, 0);
        // no stage_effect line: none
        let l2 = FxLayout::parse("flight_fx 1\nmetres_per_unit 1\n").unwrap();
        assert!(StageFx::new(&teb, &l2, 1).is_none());
    }

    #[test]
    fn schedule_time_runs_the_takeoff_on_real_time() {
        let s = Some(10.0);
        assert_eq!(flight_schedule_time(None, 7.5, 6.0, 0.0), 7.5);
        assert_eq!(
            flight_schedule_time(s, 12.5, 6.0, 12.5),
            6.0,
            "intro: real time"
        );
        // at and after the switch: the dance clock, continuous
        assert_eq!(flight_schedule_time(s, 12.5, 10.0, 12.5), 10.0);
        assert_eq!(flight_schedule_time(s, 15.0, 12.0, 12.5), 12.5);
        assert_eq!(flight_schedule_time(s, 0.0, -0.3, 0.0), -0.3, "pre-song");
    }

    // ── World conversions ───────────────────────────────────────────────

    fn row_mul(p: [f32; 4], m: &[f32; 16]) -> V3 {
        let mut o = [0.0f32; 3];
        for (c, v) in o.iter_mut().enumerate() {
            *v = (0..4).map(|r| p[r] * m[r * 4 + c]).sum();
        }
        o
    }

    #[test]
    fn world_bones_place_quads_and_ribbon_edges() {
        let s = Sprite {
            centre: [1.0, 2.0, 3.0],
            axes: [[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 1.0]],
            ..Sprite::ZERO
        };
        let k = 0.5;
        let b = sprite_bone(&s, k);
        for q in QUAD {
            let got = row_mul([q[0], q[1], q[2], 1.0], &b);
            let want = scale(add(s.centre, mv(&s.axes, q)), k);
            assert!(approx3(got, want, 1e-6));
        }
        let p = RibbonPoint {
            pos: [0.0, 1.0, 0.0],
            left: [-1.0, 1.0, 0.0],
            right: [1.0, 1.0, 0.0],
            life: 1.0,
        };
        let b = ribbon_bone(&p, k);
        assert!(approx3(
            row_mul([-1.0, 0.0, 0.0, 1.0], &b),
            scale(p.left, k),
            1e-6
        ));
        assert!(approx3(
            row_mul([1.0, 0.0, 0.0, 1.0], &b),
            scale(p.right, k),
            1e-6
        ));
        assert_eq!(row_mul([0.5, 0.5, 0.0, 1.0], &HIDDEN_BONE), [0.0; 3]);
        assert_eq!(ribbon_v_scale(64, 64), 1.0);
        assert!(approx(ribbon_v_scale(17, 65), 0.25, 1e-6));
        assert_eq!(ribbon_v_scale(1, 64), 1.0);
        assert_eq!(
            additive_colour([255.0, 51.0, 0.0, 127.5]),
            [0.5, 0.1, 0.0, 1.0]
        );
        assert_eq!(additive_colour([300.0, 0.0, 0.0, 400.0])[0], 1.0);
    }

    #[test]
    fn attach_from_a_world_row_matrix() {
        // a joint turned 90° about y (local x → world −z), 2× scaled, at 1 m
        let m: [f32; 16] = [
            0.0, 0.0, -2.0, 0.0, //
            0.0, 2.0, 0.0, 0.0, //
            2.0, 0.0, 0.0, 0.0, //
            1.0, 0.5, 0.0, 1.0,
        ];
        let a = Affine::from_world_row(&m, 0.25);
        assert!(
            approx3(col(&a.r, 0), [0.0, 0.0, -1.0], 1e-6),
            "unit columns"
        );
        assert!(approx3(col(&a.r, 2), [1.0, 0.0, 0.0], 1e-6));
        assert_eq!(a.t, [4.0, 2.0, 0.0]);
        assert_eq!(a.position_only().r, M3_IDENTITY);
        // the camera helper's basis: right / up / back rows
        let c = Camera::look_at_world([0.0, 0.0, 5.0], [0.0; 3], [0.0, 1.0, 0.0], 0.5);
        assert_eq!(c.pos, [0.0, 0.0, 10.0]);
        assert!(approx3(c.view_rot[2], [0.0, 0.0, 1.0], 1e-6));
        assert!(approx3(c.view_rot[0], [1.0, 0.0, 0.0], 1e-6));
        let d = Camera::look_at([0.0; 3], [0.0; 3], [0.0, 1.0, 0.0]);
        assert_eq!(d.view_rot, M3_IDENTITY, "degenerate keeps identity");
    }
}
