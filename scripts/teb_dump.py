#!/usr/bin/env python3
"""Reference decoder + simulator for Konami's `zan` particle effects (`.TEB`, the `CzanEff`
runtime of DanceDanceRevolution MUSIC FIT (Wii, JP) = HOTTEST PARTY 3 and its siblings).
RE record: docs/wii_ddr_zan_effects_research.md (addresses in MUSIC FIT's main.dol). Values are
big-endian; a TEB sits in a `WII\\0` archive next to the TPL its parts index
(`game/GAME_CHR_EFF.bin` = boss_ddr3.TEB + boss_ddr3.tpl: the flight stages' flyer effects).

  TEB       char magic[4] "TEB\\0", u32[3] 0, u32 table (0x18), u32 (count << 16), then count x
            {u32 list, u32, u8 nnodes, u8[3], u32} effect entries at `table`.
  node list nnodes x {u8 child, u8 next, u8 type, u8, u32 data}: type 0 the effect root, 1 a
            particle PART (`CzanEffPart`), 2 a model (`CzanEffMdl`, unused by boss_ddr3);
            child / next are node indices (0 = none) -- a tree whose parts inherit their
            parent's world transform.
  root      {u32 loop, u32 tracks, ...}: loop 1 = the effect restarts at the end of its track
            (`FUN_801302dc`: every node's clock back to 0), 0 = it ends.
  tracks    {u32 n, u32 table} -> n x 0x14 {u32 nkeys, u32 keys, f32 t0, f32 t1, u8 spline}:
            the node's local motion while t0 <= node time <= t1 (no track = inactive: a part
            stops emitting); keys 0x20 {f32 t, pos xyz, quat xyzw (all 0 = identity)}; spline 1 =
            a Catmull-Rom position curve, else linear; slerped rotation (`FUN_8012f788`).
  part      u32 flags (which sub-blocks follow: 0x1 emitter, 0x2 gravity, 0x4 draw, 0x8 chain,
            0x10 ribbon, 0x20 spin / orbit, 0x40 follow; behaviour bits 0x80 world-space
            particles, 0x100 follow scale, 0x200 no depth test, 0x400 flipped UVs, 0x800 immortal
            particles), u32 offsets of those blocks (emitter, gravity, draw, chain, ribbon, spin,
            follow), +0x20 / +0x24 shape blocks, +0x28 tracks, u8 shape (+0x2C: 0 box, 1 ellipsoid,
            2 ring).

Usage:
    teb_dump.py ls <file>          # effects / nodes / parts of every TEB in a WII archive
Import-safe (`parse_teb`, `simulate`); needs numpy.
"""
import argparse
import math
import os
import struct
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import zan_dump as Z  # noqa: E402

TEB_MAGIC = b'TEB\0'
# main.dol constants: the manager's effect scale (`+0x294`, set_mtx scales the attach matrix's
# rotation columns by it), the default sprite UVs (`0x80274f08` / `0x80274f28` with flag 0x400) and
# the unit quad (`0x80274f70`, a strip), the lay-flat billboard (`0x80274eb8`)
EFFECT_SCALE = 10.0
QUAD = np.array([[0.5, -0.5, 0.0], [-0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]])
UV_DEFAULT = np.array([[0.0, 1.0], [0.0, 0.0], [1.0, 1.0], [1.0, 0.0]])
UV_FLIPPED = np.array([[1.0, 1.0], [0.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
LAY_FLAT = np.array([[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]])
P_EMITTER, P_GRAVITY, P_DRAW, P_CHAIN, P_RIBBON, P_SPIN, P_FOLLOW = 0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x40
P_WORLD, P_FOLLOW_SCALE, P_NO_DEPTH, P_FLIP_UV, P_IMMORTAL = 0x80, 0x100, 0x200, 0x400, 0x800
D_FLIPBOOK, D_COLOUR_KEYS = 0x2, 0x20
# the flight's world scroll (zan units / s): +3.0 z per 60 Hz frame from the switch on (`simulate`)
FLIGHT_SCROLL = (0.0, 0.0, 180.0)


def _u32(b, o):
    return struct.unpack_from('>I', b, o)[0]


def _f32(b, o, n=1):
    v = struct.unpack_from('>%df' % n, b, o)
    return v if n > 1 else v[0]


def _keys(b, T, off, n):
    return [tuple(_f32(b, T + off + 8 * i, 2)) for i in range(n)]


def parse_tracks(b, T, off):
    if not off:
        return []
    n, table = _u32(b, T + off), _u32(b, T + off + 4)
    out = []
    for i in range(n):
        q = T + table + 0x14 * i
        nk, kp = _u32(b, q), _u32(b, q + 4)
        t0, t1 = _f32(b, q + 8, 2)
        keys = [_f32(b, T + kp + 0x20 * k, 8) for k in range(nk)]
        out.append(dict(t0=t0, t1=t1, spline=b[q + 0x10], keys=[(k[0], np.array(k[1:4]), np.array(k[4:8])) for k in keys]))
    return out


def parse_part(b, T, do):
    w = struct.unpack_from('>10I', b, T + do)
    flags = w[0]
    p = dict(offset=do, flags=flags, shape=b[T + do + 0x2C], tracks=parse_tracks(b, T, _u32(b, T + do + 0x28)))
    if flags & P_EMITTER:
        o = T + w[1]
        f = _f32(b, o, 11)
        p['emitter'] = dict(life=f[0], life_rand=f[1], interval=f[2], spread=f[3:6], rot_spread=f[6:9], speed=f[9],
                            speed_rand=f[10], max=struct.unpack_from('>h', b, o + 0x2C)[0],
                            per_spawn=struct.unpack_from('>h', b, o + 0x2E)[0], billboard=b[o + 0x32])
    if flags & P_GRAVITY:
        o = T + w[2]
        p['gravity'] = dict(dir=_f32(b, o, 3), rot_spread=_f32(b, o + 12, 3), accel=_f32(b, o + 0x18),
                            accel_rand=_f32(b, o + 0x1C), raw=_f32(b, o, 8))
    if flags & P_DRAW:
        o = T + w[3]
        df = _u32(b, o)
        d = dict(flags=df, colours=_u32(b, o + 4), rect=_f32(b, o + 8, 4),
                 scale_keys=_keys(b, T, _u32(b, o + 0x18), b[o + 0x24]),
                 alpha_keys=_keys(b, T, _u32(b, o + 0x1C), b[o + 0x25]),
                 blend=b[o + 0x26], tex=struct.unpack_from('>h', b, o + 0x28)[0], ncolour=b[o + 0x2A],
                 size_var=(b[o + 0x2B], b[o + 0x2C]))
        if df & D_COLOUR_KEYS:
            d['colour_keys'] = [tuple(b[T + d['colours'] + 8 * i:T + d['colours'] + 8 * i + 5]) for i in range(max(1, d['ncolour']))]
        else:
            d['colour'] = tuple(b[T + d['colours']:T + d['colours'] + 4])
        if df & D_FLIPBOOK:
            fo = T + _u32(b, o + 0x20)
            d['flipbook'] = dict(width=struct.unpack_from('>H', b, fo)[0], cell=struct.unpack_from('>H', b, fo + 2)[0],
                                 frame_s=_f32(b, fo + 4), frames=b[fo + 8])
        p['draw'] = d
    if flags & P_RIBBON:
        o = T + w[5]
        p['ribbon'] = dict(width=_f32(b, o), segments=b[o + 4], tex=struct.unpack_from('>h', b, o + 6)[0])
    if flags & P_SPIN:
        o = T + w[6]
        p['spin'] = dict(raw=_f32(b, o, 8))
    if flags & P_FOLLOW:
        o = T + w[7]
        p['follow'] = dict(follow=_f32(b, o), scale=_f32(b, o + 4))
    if p['shape'] == 1:
        o = T + w[8]
        p['ellipsoid'] = dict(flags=_u32(b, o), radii=_f32(b, o + 4, 3))
    elif p['shape'] == 2:
        o = T + w[9]
        p['ring'] = dict(flags=_u32(b, o), radii=_f32(b, o + 4, 3))
    return p


def parse_teb(b, T=0):
    """dict(effects=[dict(index, root, nodes)]): `T` = the TEB's offset in `b`."""
    if b[T:T + 4] != TEB_MAGIC:
        raise ValueError('not a TEB')
    table, n = _u32(b, T + 0x10), _u32(b, T + 0x14) >> 16
    effects = []
    for e in range(n):
        eo = T + table + 0x10 * e
        lo, cnt = _u32(b, eo), b[eo + 8]
        nodes = []
        for k in range(cnt):
            c, nx, ty, _f = b[T + lo + 8 * k:T + lo + 8 * k + 4]
            do = _u32(b, T + lo + 8 * k + 4)
            nd = dict(index=k, child=c, next=nx, type=ty, data=do)
            if ty == 0:
                nd['loop'] = bool(_u32(b, T + do))
                nd['tracks'] = parse_tracks(b, T, _u32(b, T + do + 4))
            elif ty == 1:
                nd['part'] = parse_part(b, T, do)
            nodes.append(nd)
        for nd in nodes:
            nd['parent'] = None
        for nd in nodes:
            kid = nd['child']
            while kid:
                nodes[kid]['parent'] = nd['index']
                kid = nodes[kid]['next']
        effects.append(dict(index=e, nodes=nodes, root=nodes[0] if nodes else None))
    return dict(effects=effects)


def teb_members(blob):
    """[(path, TEB bytes, TPL bytes or None)] of every TEB in a WII archive (the TPL beside it)."""
    ms = list(Z.walk(blob))
    out = []
    for i, (p, _n, o, s, k) in enumerate(ms):
        if k == 'teb':
            tpl = next((blob[o2:o2 + s2] for _p2, _n2, o2, s2, k2 in ms[i + 1:i + 2] if k2 == 'tpl'), None)
            out.append((p, blob[o:o + s], tpl))
    return out


# ---------------------------------------------------------------------------
# evaluation
# ---------------------------------------------------------------------------
def key_eval(keys, t):
    """`FUN_8013212c`: piecewise-linear (t, v) keys at normalised life `t` (clamped at 0)."""
    if not keys:
        return 1.0
    t = max(t, 0.0)
    if len(keys) == 1:
        return keys[0][1]
    for i in range(1, len(keys)):
        if keys[i][0] > t:
            a, b = keys[i - 1], keys[i]
            break
    else:
        a, b = keys[-2], keys[-1]
    if b[0] == a[0]:
        return b[1]
    return (t - a[0]) / (b[0] - a[0]) * (b[1] - a[1]) + a[1]


def colour_eval(ckeys, pct):
    """Colour keys {r, g, b, _, t%} at life percentage `pct` (`FUN_8012ce18`)."""
    if len(ckeys) < 2:
        return np.array(ckeys[0][:3], dtype=float)
    if pct >= ckeys[-1][4]:
        a, b = ckeys[-2], ckeys[-1]
    else:
        a = b = ckeys[0]
        for k in ckeys[1:]:
            a, b = b, k
            if k[4] > pct:
                break
    span = float(b[4] - a[4]) or 1.0
    f = (pct - a[4]) / span
    return np.clip(np.array(a[:3], float) + f * (np.array(b[:3], float) - np.array(a[:3], float)), 0, 255)


def quat_mtx(q):
    x, y, z, w = q
    if not (x or y or z or w):
        return np.eye(3)
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def mtx_quat(m):
    t = np.trace(m)
    if t > 0:
        s = math.sqrt(t + 1.0) * 2
        return np.array([(m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s])
    i = int(np.argmax(np.diag(m)))
    j, k = (i + 1) % 3, (i + 2) % 3
    s = math.sqrt(1.0 + m[i, i] - m[j, j] - m[k, k]) * 2
    q = np.zeros(4)
    q[i] = 0.25 * s
    q[j] = (m[j, i] + m[i, j]) / s
    q[k] = (m[k, i] + m[i, k]) / s
    q[3] = (m[k, j] - m[j, k]) / s
    return q


def slerp(p, q, t):
    p, q = np.asarray(p, float), np.asarray(q, float)
    if not p.any():
        p = np.array([0, 0, 0, 1.0])
    if not q.any():
        q = np.array([0, 0, 0, 1.0])
    d = float(np.dot(p, q))
    if d < 0:
        q, d = -q, -d
    if d > 0.9995:
        r = p + t * (q - p)
        return r / np.linalg.norm(r)
    th = math.acos(min(1.0, d))
    return (math.sin((1 - t) * th) * p + math.sin(t * th) * q) / math.sin(th)


def axis_angle(axis, a):
    """`FUN_801325c8`: rotation by `a` about the unit `axis` (column vectors)."""
    x, y, z = axis
    c, s, t = math.cos(a), math.sin(a), 1 - math.cos(a)
    return np.array([[t * x * x + c, t * x * y - s * z, t * x * z + s * y],
                     [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
                     [t * x * z - s * y, t * y * z + s * x, t * z * z + c]])


def euler_yzx(rx, ry, rz):
    """`FUN_801326d0`: Ry . Rz . Rx (x applied first)."""
    return axis_angle((0, 1, 0), ry) @ axis_angle((0, 0, 1), rz) @ axis_angle((1, 0, 0), rx)


def track_local(tracks, t):
    """(4x4 local matrix, active) of a node at its clock `t` (`FUN_8012f788`)."""
    tr = next((x for x in tracks if x['t0'] <= t <= x['t1']), None)
    if tr is None or not tr['keys']:
        return None, False
    ks = tr['keys']
    if len(ks) == 1:
        a = b = ks[0]
        f, ia = 0.0, 0
    else:
        ia = max(0, min(len(ks) - 2, max(i for i, k in enumerate(ks) if k[0] <= t) if ks[0][0] <= t else 0))
        a, b = ks[ia], ks[ia + 1]
        f = (t - a[0]) / (b[0] - a[0]) if b[0] != a[0] else 0.0
    if tr['spline'] and len(ks) > 1:
        # Hermite with Catmull-Rom tangents, each scaled to the segment (main.dol's form)
        d = b[1] - a[1]
        m1 = d if ia + 2 >= len(ks) else (ks[ia + 2][1] - a[1]) * ((b[0] - a[0]) / (ks[ia + 2][0] - a[0]))
        m0 = d if ia == 0 else (b[1] - ks[ia - 1][1]) * ((b[0] - a[0]) / (b[0] - ks[ia - 1][0]))
        f2, f3 = f * f, f * f * f
        pos = (2 * f3 - 3 * f2 + 1) * a[1] + (-2 * f3 + 3 * f2) * b[1] + (f3 - 2 * f2 + f) * m0 + (f3 - f2) * m1
    else:
        pos = a[1] + f * (b[1] - a[1])
    q = slerp(a[2], b[2], f)
    m = np.eye(4)
    m[:3, :3] = quat_mtx(q)
    m[:3, 3] = pos
    return m, True


class Rng:
    """`rand() % 10000` uniforms of `FUN_801322bc` (+-x) / `FUN_80132354` ([0, x)); the game's
    generator is MSL rand -- any uniform source gives the same look."""

    def __init__(self, seed=1):
        self.s = seed & 0xFFFFFFFF

    def _r(self):
        self.s = (self.s * 1103515245 + 12345) & 0xFFFFFFFF
        return (self.s >> 16) & 0x7FFF

    def pm(self, x):
        return 0.0 if x == 0 else (2 * x / 10000.0) * (self._r() % 10000) - x

    def unit(self, x):
        return 0.0 if x == 0 else x / 10000.0 * (self._r() % 10000)


def _col_scale(m):
    return [float(np.linalg.norm(m[:3, i])) for i in range(3)]


class PartSim:
    def __init__(self, part, rng):
        self.p, self.rng = part, rng
        em = part.get('emitter') or dict(life=0, life_rand=0, interval=0, spread=(0, 0, 0), rot_spread=(0, 0, 0),
                                          speed=0, speed_rand=0, max=0, per_spawn=0, billboard=0)
        self.em = em
        self.ptcls = []
        self.acc = em['interval']        # the first round spawns at once (`FUN_8012be48`)
        self.prev_world = None

    def spawn(self, world, age0):
        p, em, rng = self.p, self.em, self.rng
        if p['shape'] == 1:
            d = euler_yzx(rng.pm(2 * math.pi), rng.pm(2 * math.pi), rng.pm(2 * math.pi))[2]
            el = p['ellipsoid']
            r = el['radii'] if el['flags'] & 1 else [rng.pm(x) for x in el['radii']]
            pos = np.array([d[0] * r[0], d[1] * r[1], d[2] * r[2]])
            radial = bool(el['flags'] & 4)
        elif p['shape'] == 2:
            rg = p['ring']
            a = rng.pm(2 * math.pi)
            if rg['flags'] & 8:
                pos = np.array([math.sin(a) * rg['radii'][0], math.cos(a) * rg['radii'][1], 0.0])
            elif rg['flags'] & 0x10:
                pos = np.array([math.cos(a) * rg['radii'][0], 0.0, math.sin(a) * rg['radii'][2]])
            else:
                pos = np.array([0.0, math.cos(a) * rg['radii'][1], math.sin(a) * rg['radii'][2]])
            if rg['flags'] & 2:
                pos = pos * rng.unit(1.0)
            radial = bool(rg['flags'] & 4)
        else:
            pos = np.array([rng.pm(em['spread'][i]) for i in range(3)])
            radial = False
        if radial:
            n = np.linalg.norm(pos)
            vdir = pos / n if n > 1e-4 else np.zeros(3)
        else:
            rs = em['rot_spread']
            vdir = euler_yzx(rng.pm(rs[0]), rng.pm(rs[1]), rng.pm(rs[2])) @ np.array([0.0, 1.0, 0.0])
        q = dict(age=age0, life=max(0.0, em['life'] + rng.pm(em['life_rand'])), speed=em['speed'] + rng.pm(em['speed_rand']),
                 pos0=pos, vdir=vdir, size=1.0, rgb=np.array([255.0, 255.0, 255.0]), ribbon=[])
        g = p.get('gravity')
        if g:
            # FUN_8012c790 / FUN_8012c92c: the block's direction under a random rotation, or inward
            # for a radial ellipsoid; accel +- rand (FUN_8012c438)
            if p['shape'] == 1 and radial:
                n = np.linalg.norm(vdir)
                gdir = -vdir / n if n > 1e-12 else np.zeros(3)
            else:
                rs = g['rot_spread']
                gdir = euler_yzx(rng.pm(rs[0]), rng.pm(rs[1]), rng.pm(rs[2])) @ np.array(g['dir'], float)
            q['gravity'] = (gdir, g['accel'] + rng.pm(g['accel_rand']))
        if 'spin' in p:
            s = p['spin']['raw']
            q['spin'] = (s[0] + rng.pm(s[1]), s[2] + rng.pm(s[3]), s[4] + rng.pm(s[5]))
        if p['flags'] & P_WORLD:
            q['anchor'] = (world[:3, 3].copy(), mtx_quat(world[:3, :3] / np.array(_col_scale(world))))
        d = p.get('draw')
        if d:
            lo, hi = d['size_var']
            if lo or hi:
                q['size'] = 1.0 - rng.unit((100 - lo) / 100.0) + rng.unit((hi - 100) / 100.0)
            if not d['flags'] & D_COLOUR_KEYS:
                q['rgb'] = np.array(d['colour'][:3], float)
        self.ptcls.append(q)

    def step(self, dt, world, active, view_rot, cam_pos, fade=1.0):
        p, em = self.p, self.em
        scale = _col_scale(world)
        delta = world[:3, 3] - self.prev_world[:3, 3] if self.prev_world is not None else np.zeros(3)
        wquat = mtx_quat(world[:3, :3] / np.array(scale))
        self.prev_world = world.copy()
        if active and em['max'] > 0 and self.acc >= em['interval']:
            rnd = 0
            while True:
                free = em['max'] - len(self.ptcls)
                for _ in range(min(free, em['per_spawn'])):
                    self.spawn(world, em['interval'] * rnd)
                rnd += 1
                if em['interval'] == 0:
                    break
                self.acc -= em['interval']
                if not self.acc > em['interval']:
                    break
            self.acc = 0.0
        out = []
        keep = []
        for q in self.ptcls:
            if q['age'] >= q['life']:
                if p['flags'] & P_IMMORTAL:
                    q['age'] = 0.0
                else:
                    continue
            keep.append(q)
            out.append(self._pose(q, world, scale, delta, wquat, view_rot, cam_pos, fade, dt))
            q['age'] += dt
        self.ptcls = keep
        self.acc += dt
        return out

    def _pose(self, q, world, scale, delta, wquat, view_rot, cam_pos, fade, dt):
        p, d = self.p, self.p.get('draw') or {}
        age = q['age']
        tn = age / q['life'] if q['life'] > 0 else 0.0
        pos = q['pos0'] + q['vdir'] * (q['speed'] * age)
        if 'spin' in p:
            s = p['spin']['raw']
            pos = axis_angle((0, 1, 0), s[6] * age + s[7] * age * age * 0.5) @ pos
        # gravity (FUN_8012ce18): a radial ring falls in the emitter's frame, every other part in
        # world space x the manager scale
        ring_radial = p['shape'] == 2 and bool(p.get('ring', {}).get('flags', 0) & 4)
        if 'gravity' in q and ring_radial:
            pos = pos + q['gravity'][0] * (q['gravity'][1] * age * age * 0.5)
        if p['flags'] & P_WORLD and age > 0 and 'anchor' in q:
            apos, aq = q['anchor']
            fol = p.get('follow', {}).get('follow', 0.0)
            aq = slerp(aq, wquat, fol)
            apos = apos + delta * fol
            q['anchor'] = (apos, aq)
            R = quat_mtx(aq) * np.array(scale)
            wpos = R @ pos + apos
            R0 = R
        else:
            wpos = world[:3, :3] @ pos + world[:3, 3]
            R0 = world[:3, :3]
        if 'gravity' in q and not ring_radial:
            wpos = wpos + q['gravity'][0] * (age * age * q['gravity'][1] * EFFECT_SCALE * 0.5)
        # billboard: the camera's basis (mode 0) or the emitter's frame laid flat (mode 1)
        if self.em['billboard'] == 0:
            R = view_rot.T * np.array([scale[0], scale[1], 1.0])
        else:
            R = R0 @ LAY_FLAT
        if 'spin' in q:
            a0, a1, a2 = q['spin']
            ax = R[:, 2] / (np.linalg.norm(R[:, 2]) or 1.0)
            R = axis_angle(ax, a2 * age * age * 0.5 + a1 * age + a0) @ R
        s = key_eval(d.get('scale_keys', []), tn) if d else 1.0
        rect = d.get('rect', (0, 0, 1, 1))
        w, h = q['size'] * s * (rect[2] - rect[0]), q['size'] * s * (rect[3] - rect[1])
        R = R * np.array([w, h, 1.0])
        if d and d['flags'] & D_COLOUR_KEYS:
            alpha = 255.0 * key_eval(d['alpha_keys'], tn)
            rgb = colour_eval(d['colour_keys'], 100.0 / q['life'] * age if q['life'] > 0 else 0.0)
        else:
            alpha = key_eval(d.get('alpha_keys', []), tn) * (d.get('colour', (255, 255, 255, 255))[3])
            rgb = q['rgb']
        alpha *= fade
        uv = UV_FLIPPED if p['flags'] & P_FLIP_UV else UV_DEFAULT
        if d and d['flags'] & D_FLIPBOOK:
            fb = d['flipbook']
            cols = max(1, fb['width'] // max(1, fb['cell']))
            frame = int(age / fb['frame_s']) % max(1, fb['frames']) if fb['frame_s'] > 0 else 0
            cw = 1.0 / cols
            u0, v0 = cw * (frame % cols), cw * (frame // cols)
            uv = np.array([[u0, v0], [u0, v0 + cw], [u0 + cw, v0], [u0 + cw, v0 + cw]]) if not p['flags'] & P_FLIP_UV else \
                np.array([[u0 + cw, v0 + cw], [u0, v0 + cw], [u0 + cw, v0], [u0, v0]])
        sprite = dict(centre=wpos, axes=R, uv=uv, rgba=(*rgb, alpha), tex=d.get('tex', -1), blend=d.get('blend', 0),
                      depth=not p['flags'] & P_NO_DEPTH)
        rb = p.get('ribbon')
        if rb:
            pts = q['ribbon']
            for pt in pts:
                pt['life'] -= dt
            pts[:] = [pt for pt in pts if pt['life'] > 0]
            if dt > 0:
                prev = pts[-1]['pos'] if pts else wpos
                side = np.cross(cam_pos - wpos, wpos - prev)
                n = np.linalg.norm(side)
                side = side / n * (rb['width'] * s * scale[0]) if n > 1e-4 else np.zeros(3)
                pts.append(dict(pos=wpos.copy(), left=wpos + side, right=wpos - side, life=1.0))
                del pts[:-rb['segments']]
            sprite['ribbon'] = dict(points=[(pt['left'], pt['right']) for pt in pts], tex=rb['tex'],
                                    v=_ribbon_v(len(pts)), rgba=(*rgb, alpha))
        return sprite


def _ribbon_v(n):
    """`FUN_80131764`: v along a ribbon of `n` points, oldest first (1 .. 0)."""
    if n < 2:
        return [0.0] * n
    v = [1.0] + [1.0 - (1.0 / n) * i for i in range(1, n - 1)] + [0.0]
    return v


def node_order(effect):
    """Nodes parent-first (the update recursion's order)."""
    out, todo = [], [0] if effect['nodes'] else []
    while todo:
        i = todo.pop(0)
        out.append(i)
        kid = effect['nodes'][i]['child']
        kids = []
        while kid:
            kids.append(kid)
            kid = effect['nodes'][kid]['next']
        todo = kids + todo
    return out


def simulate(effect, attach, frames, dt=1.0 / 60.0, view_rot=None, cam_pos=None, seed=1, start=0.0, scroll=None):
    """Yield per frame the sprite list of `effect` attached at `attach(i)` (a 4x4 world matrix in
    zan units; its rotation columns are scaled by EFFECT_SCALE like `set_mtx`). `view_rot(i)` /
    `cam_pos(i)`: the camera (identity / origin + z when None). A looping root restarts every
    node's clock at the end of its track; `start` pre-rolls that clock.

    `scroll` (units / s): the flight's world scroll. After the switch MUSIC FIT moves the whole
    scene -- camera, stage, dancers -- by a world offset (play object +0x3ac: (0, 0, -50000) at the
    switch, z += 3.0 per 60 Hz frame, `FUN_80037354` / `FUN_800377e0`), so nothing moves on screen
    except what the effects leave in world space: ribbon points and world-space (0x80) particles
    stream behind the flyer -- the rainbow trail. The simulation runs in that scrolled world and
    the sprites come back in the caller's (unscrolled) frame. MUSIC FIT's value: (0, 0, 180)."""
    rng = Rng(seed)
    sims = {nd['index']: PartSim(nd['part'], rng) for nd in effect['nodes'] if nd['type'] == 1}
    root = effect['root']
    span = max((tr['t1'] for tr in root['tracks']), default=0.0)
    clock = start
    order = node_order(effect)
    vel = np.zeros(3) if scroll is None else np.asarray(scroll, float)
    for i in range(frames):
        off = vel * (i * dt)
        a = np.array(attach(i), dtype=float)
        a[:3, :3] *= EFFECT_SCALE
        a[:3, 3] += off
        vr = np.eye(3) if view_rot is None else np.asarray(view_rot(i), float)
        cp = a[:3, 3] + np.array([0.0, 0.0, 100.0]) if cam_pos is None else np.asarray(cam_pos(i), float) + off
        worlds, sprites = {}, []
        for k in order:
            nd = effect['nodes'][k]
            loc, active = track_local(nd.get('tracks', nd.get('part', {}).get('tracks', [])), clock)
            par = a if nd['parent'] is None else worlds[nd['parent']]
            worlds[k] = par @ (loc if loc is not None else np.eye(4))
            if k in sims:
                sprites += sims[k].step(dt, worlds[k], active, vr, cp)
        if vel.any():
            sprites = [_unscroll(s, off) for s in sprites]
        yield sprites
        clock += dt
        if root['loop'] and span > 0 and clock > span:
            clock = 0.0


def _unscroll(s, off):
    s = dict(s, centre=s['centre'] - off)
    if 'ribbon' in s:
        s['ribbon'] = dict(s['ribbon'], points=[(l - off, r - off) for l, r in s['ribbon']['points']])
    return s


def cmd_ls(a):
    blob = open(a.file, 'rb').read()
    for path, teb, _tpl in teb_members(blob):
        t = parse_teb(teb)
        print('%s: %d effects' % (path, len(t['effects'])))
        for e in t['effects']:
            print('  effect %d (%s, %.2f s)' % (e['index'], 'loop' if e['root'] and e['root']['loop'] else 'once',
                                                max((x['t1'] for x in e['root']['tracks']), default=0) if e['root'] else 0))
            for nd in e['nodes'][1:]:
                p = nd.get('part')
                if not p:
                    continue
                d, em = p.get('draw', {}), p.get('emitter', {})
                print('    node %d (parent %s) flags %04x shape %d tex %s blend %s life %.2f every %.3f max %d%s' % (
                    nd['index'], nd['parent'], p['flags'], p['shape'], d.get('tex'), d.get('blend'), em.get('life', 0),
                    em.get('interval', 0), em.get('max', 0),
                    ' ribbon tex %d w %.2f x%d' % (p['ribbon']['tex'], p['ribbon']['width'], p['ribbon']['segments']) if 'ribbon' in p else ''))


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    s = sub.add_parser('ls')
    s.add_argument('file')
    s.set_defaults(fn=cmd_ls)
    a = ap.parse_args(argv)
    a.fn(a)


if __name__ == '__main__':
    main(sys.argv[1:])
