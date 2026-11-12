#!/usr/bin/env python3
"""Reference decoders for the polygon background dancers of Konami System 573 DDR
(3rdMIX PLUS, 4thMIX PLUS, 5thMIX — the same engine and the same 18 shared dance routines),
and exporters to OBJ (one pose) and glTF 2.0 binary (rig + every routine as an animation).

Formats, addresses and the evidence behind every rule: docs/sys573_dancers_research.md.
Extract a mix first with scripts/extract_sys573_data.py; the arguments below are paths inside
that output (`data/chara/...`, `data/motion/...`).

  <chara>/<name>.cmd   mesh: 28 objects, each 1+ sub-meshes (textured GT3 / flat G3 strips)
  <chara>/<name>.ctx   3rd/4th texture: 16 x 64x64 4bpp TIM, 12 used, tiled column-major
  <chara>/<name>.cmt   5th texture: one 256x256 8bpp TIM
  chara.lst            u8 nobj, then per object (u8 joint, u8 parent joint)
  chara.pos            17 x int16 xyz; entry j+1 = joint j's offset in its parent
  <motion>.cmm         'S' container of named clips, one clip = one measure (1920 time units)

DDR STRIKE (PS2) ships the same engine's data: `parse_cmd` reads its meshes unchanged, and
`parse_cmm` / `sample` also read its PS2 key-block layout (docs/ps2_ddr_filedata_research.md §4;
port: tools/blender_ddr_addon/examples/port_character_strike.py).

Rig: a root (track 0, carries the travel) + 16 joints (tracks 1..16). Per joint, per frame:
R = Rx(rx) . Rz(rz) . Ry(ry) (PSX 4096 = 360 deg; column vectors), t = pos[j+1] unless the
track animates translation. Tracks 14/15/16 (hand L, hand R, head) also carry a stepped
selector that picks the drawn alternate object: hands draw first+sel, the head draws its
base object plus first+sel+1 (a face). Between measures the game re-bases the root on the
previous measure's end pose, so a routine travels; `routine_frames` reproduces that.

Coordinates: PSX Y-down, the model faces +Z. OBJ/glTF output is Y-up facing +Z, metres
(0.001 per unit) via the proper rotation (x, y, z) -> (-x, -y, z); left/right joint names
follow from that facing.

Usage:
    sys573_dancer_dump.py info    <chara dir> <name>                 # objects / joints / textures
    sys573_dancer_dump.py cmm     <file.cmm>                         # clips, tracks, channels
    sys573_dancer_dump.py obj     <chara dir> <name> <out.obj> [--motion f.cmm --clip hiphop1a --frame 480]
    sys573_dancer_dump.py glb     <chara dir> <name> <motion dir> <out.glb> [--bpm 130] [--fps 30]
    sys573_dancer_dump.py preview <chara dir> <name> <out.png> [--motion f.cmm --routine hiphop1]
    sys573_dancer_dump.py video   <chara dir> <name> <file.cmm> <routine> <out.mp4> [--bpm 130]
    sys573_dancer_dump.py survey  <extracted mix dir>...             # parse every model + clip

Import-safe: `from sys573_dancer_dump import load_character, load_motion, pose, routine_frames`.
Needs numpy; Pillow for textures/previews; ffmpeg on PATH for `video`.
"""
import argparse
import glob
import json
import math
import os
import struct
import subprocess
import sys
from typing import Any

import numpy as np

UNITS_PER_MEASURE = 1920  # clip time units; the game maps phase (4096 / measure) onto them
SCALE = 0.001  # model units -> metres (hips ~1.06 m)
TAU = 2 * math.pi / 4096

# Joint hierarchy: aout.exe table 0x80013198 (3rdMIX PLUS), identical in 4th/5th; each joint's
# parent, root = -1. Names follow the rest offsets in chara.pos (see module doc for facing).
PARENT = [-1, 0, 1, 2, 0, 4, 5, 0, 7, 8, 7, 10, 7, 9, 11, 12]
JOINT_NAMES = ['hips', 'thigh_L', 'shin_L', 'foot_L', 'thigh_R', 'shin_R', 'foot_R', 'chest',
               'upperarm_L', 'forearm_L', 'upperarm_R', 'forearm_R', 'neck', 'hand_L', 'hand_R', 'head']
HEAD = 15
CH_RX, CH_RY, CH_RZ, CH_TX, CH_TY, CH_TZ, CH_SEL = 0, 1, 2, 6, 7, 8, 10


def _u16(d, o):
    return struct.unpack_from('<H', d, o)[0]


def _s16(d, o):
    return struct.unpack_from('<h', d, o)[0]


def _u32(d, o):
    return struct.unpack_from('<I', d, o)[0]


# ---------------------------------------------------------------------------
# textures
# ---------------------------------------------------------------------------
def load_tims(data):
    """Consecutive TIMs -> [(rgba HxWx4 uint8, raw 15-bit texel array)]. 4/8/16 bpp."""
    out = []
    o = 0
    while o + 8 <= len(data) and _u32(data, o) == 0x10:
        flags = _u32(data, o + 4)
        p = o + 8
        clut = None
        if flags & 8:
            size, _x, _y, cw, ch = struct.unpack_from('<IHHHH', data, p)
            clut = np.frombuffer(data, '<u2', cw * ch, p + 12).reshape(ch, cw)
            p += size
        size, _x, _y, w, h = struct.unpack_from('<IHHHH', data, p)
        px = np.frombuffer(data, 'u1', w * h * 2, p + 12)
        bpp = flags & 3
        if bpp in (0, 1) and clut is None:
            raise ValueError('paletted TIM without CLUT')
        if bpp == 0:
            texel = clut[0][np.stack([px & 15, px >> 4], -1).reshape(h, w * 4)]  # type: ignore[index]
        elif bpp == 1:
            texel = clut[0][px.reshape(h, w * 2)]  # type: ignore[index]
        else:
            texel = px.view('<u2').reshape(h, w)
        out.append(texel)
        o = p + size
    return out


def texels_to_rgba(texel, magenta_key):
    """3rd/4th loader (FUN_800414ac): CLUT 0x7C1F -> transparent, 0x0000 -> opaque black.
    5th keeps the PSX hardware rule: texel 0x0000 is transparent."""
    t = texel.astype(np.uint32)
    rgb = np.stack([(t & 31) << 3, ((t >> 5) & 31) << 3, ((t >> 10) & 31) << 3], -1)
    if magenta_key:
        alpha = np.where((t & 0x7FFF) == 0x7C1F, 0, 255)
    else:
        alpha = np.where(t == 0, 0, 255)
    return np.concatenate([rgb, alpha[..., None]], -1).astype(np.uint8)


def texture_atlas(path):
    """The 256x256 texture page the model's UVs address."""
    data = open(path, 'rb').read()
    tims = load_tims(data)
    page = np.zeros((256, 256, 4), np.uint8)
    if path.endswith('.cmt') and len(tims) == 1:  # 5thMIX: one 8bpp sheet
        rgba = texels_to_rgba(tims[0], False)[:256, :256]
        page[:rgba.shape[0], :rgba.shape[1]] = rgba
        return page
    # 3rd/4th: TIM i goes to VRAM x = 0x200 + (i // 4) * 16, y = (i % 4) * 64 (4bpp: 16
    # halfwords = 64 texels), i.e. texture-page tile (u, v) = ((i // 4) * 64, (i % 4) * 64).
    for i, texel in enumerate(tims[:12]):
        rgba = texels_to_rgba(texel, True)
        u, v = (i // 4) * 64, (i % 4) * 64
        page[v:v + 64, u:u + 64] = rgba[:64, :64]
    return page


# ---------------------------------------------------------------------------
# meshes
# ---------------------------------------------------------------------------
def parse_cmd(data):
    """.cmd -> list of objects; object = list of sub-meshes:
    {verts Nx3, normals Mx3 (unit), tris [(v0,v1,v2),(n0,n1,n2)], uvs [3x(u,v)] | None, color}.
    28 objects (chara.lst), or 20 (chara20.lst: one hand shape per hand, PS2 Party Collection)."""
    if data[:8] != bytes(8) or _u32(data, 8) not in (0x14, 0x1C):
        raise ValueError('not a 573 dancer .cmd')
    nobj = _u32(data, 8)
    objects = []
    for i in range(nobj):
        off, nsub, _scale = struct.unpack_from('<III', data, 0x20 + 12 * i)
        o = 0x20 + off
        subs = []
        for s in range(nsub):
            h = struct.unpack_from('<12I', data, o + 0x30 * s)
            kind, v_off, nv, n_off, idx_off, uv_off, ntri = h[0] & 0xFF, h[1], h[2], h[5], h[9], h[10], h[11]
            nn = max(h[3], h[7], nv)
            verts = np.array([struct.unpack_from('<3h', data, o + v_off + 8 * k) for k in range(nv)], float)
            idx = struct.unpack_from('<%dH' % (ntri * 6), data, o + idx_off)
            nmax = max(idx[1::2]) + 1 if ntri else 0
            normals = np.array([struct.unpack_from('<3h', data, o + n_off + 8 * k) for k in range(max(nn, nmax))],
                               float) / 4096.0
            tris = [((idx[6 * t], idx[6 * t + 2], idx[6 * t + 4]), (idx[6 * t + 1], idx[6 * t + 3], idx[6 * t + 5]))
                    for t in range(ntri)]
            sub: dict[str, Any] = dict(verts=verts, normals=normals, tris=tris, uvs=None, color=None, kind=kind)
            if kind == 0x34:  # textured: per tri (u,v,cba) (u,v,tsb) (u,v,pad)
                sub['uvs'] = []
                for t in range(ntri):
                    e = struct.unpack_from('<BBHBBHBBH', data, o + uv_off + 12 * t)
                    sub['uvs'].append(((e[0], e[1]), (e[3], e[4]), (e[6], e[7])))
            else:  # 0x30: untextured lit sub-mesh, one RGB drawn as is (0xFF = 1.0; only a TEXTURED
                # primitive's colour modulates with 0x80 = 1.0): docs/sys573_dancers_research.md §2
                sub['color'] = tuple(data[o + uv_off:o + uv_off + 3])
            subs.append(sub)
        objects.append(subs)
    return objects


def parse_lst(data):
    return [(data[1 + 2 * i], data[2 + 2 * i]) for i in range(data[0])]


def parse_pos(data):
    return [np.array(struct.unpack_from('<3h', data, 6 * i), float) for i in range(len(data) // 6)]


def load_character(chara_dir, name):
    base = os.path.join(chara_dir, name, name)
    objects = parse_cmd(open(base + '.cmd', 'rb').read())
    tex = next((base + e for e in ('.ctx', '.cmt') if os.path.exists(base + e)), None)
    lst = parse_lst(open(os.path.join(chara_dir, 'chara.lst'), 'rb').read())
    pos = parse_pos(open(os.path.join(chara_dir, 'chara.pos'), 'rb').read())
    groups = {}
    for i, (joint, _parent) in enumerate(lst[:len(objects)]):
        groups.setdefault(joint, []).append(i)
    return dict(name=name, objects=objects, lst=lst, rest=pos[1:17], groups=groups,
                texture=texture_atlas(tex) if tex else np.full((256, 256, 4), 255, np.uint8),
                texture_path=tex)


def visible_objects(ch, sel):
    """FUN_800441ac: per joint draw first + sel (clamped to the joint's objects); the head
    joint also draws its first object (the head itself). sel: {joint: selector}; the head
    selector already carries the evaluator's +1."""
    out = []
    for j in range(16):
        objs = ch['groups'].get(j, [])
        if not objs:
            continue
        s = min(max(sel.get(j, 1 if j == HEAD else 0), 0), len(objs) - 1)
        if j == HEAD:
            out.append((j, objs[0]))
        if not (j == HEAD and s == 0):
            out.append((j, objs[s]))
    return out


# ---------------------------------------------------------------------------
# motion
# ---------------------------------------------------------------------------
def cmm_layout(data, track):
    """'573' or 'ps2' for the track header at `track`. The PS2 port (DDR STRIKE; SLPM_662.42
    FUN_001b3140) drops the 573 track's `size` word, so its channel-table offset (0xC) sits
    where 573 has the channel count (at most 7)."""
    return 'ps2' if _u32(data, track + 8) == 0xC else '573'


def parse_cmm(data):
    """.cmm -> {clip name: clip}. clip = {last, layout, tracks: [{trans, chans: {type: key block}}]}.

    Both key-block layouts are read (`sample` takes the clip's `layout`):
      573  track {u8 index, u8, u8 has_translation, u8, u32 size, u32 nchan, u32 -> channels};
           channel {u8 type, ..., +8 u32 -> key block}
      ps2  track {u8 index, u8, u8 has_translation, u8, u32 nchan, u32 -> channels};
           channel {u8 type, u8 x3}, key block inline at +4
    The container and clip header are the same in both."""
    if data[0] != 0x53:
        raise ValueError('not a .cmm')
    clips = {}
    for i in range(_u16(data, 2)):
        name_off, clip = struct.unpack_from('<II', data, 8 + 8 * i)
        name = data[name_off:data.index(b'\0', name_off)].decode('latin1')
        ntracks, last = _u16(data, clip + 4), _u16(data, clip + 6)
        table = clip + _u32(data, clip + 8)
        layout = cmm_layout(data, clip + _u32(data, table))
        tracks = []
        for k in range(ntracks):
            t = clip + _u32(data, table + 4 * k)
            if layout == 'ps2':
                nchan, ctab = _u32(data, t + 4), t + _u32(data, t + 8)
            else:
                nchan, ctab = _u32(data, t + 8), t + _u32(data, t + 0xC)
            chans = {}
            for c in range(nchan):
                ch = t + _u32(data, ctab + 4 * c)
                chans[data[ch]] = ch + 4 if layout == 'ps2' else ch + _u32(data, ch + 8)
            tracks.append(dict(trans=bool(data[t + 2]), chans=chans))
        clips[name] = dict(name=name, last=last, layout=layout, tracks=tracks, data=data)
    return clips


def sample(data, blk, t, step=False, layout='573'):
    """FUN_8003c304 / the selector read in FUN_8003c420 (PS2: SLPM_662.42 FUN_001b3030).
    573 key block: +2 u16 and +0xD u8 sum to the segment-index shift, +6 u16 = value shift,
    +0x16 + 4*i u16 = segment i offset. PS2 key block: +0 u16 = value shift, +2 + 2*i u16 =
    segment i offset, a fixed segment shift of 8. A segment is a run of (u16 time, s16 value)
    keys. Linear interpolation, C division."""
    if layout == 'ps2':
        p = blk + _u16(data, blk + 2 + 2 * (t >> 8)) + 4
        vs = _u16(data, blk) & 31
    else:
        sh = (_u16(data, blk + 2) + data[blk + 0xD]) & 31
        p = blk + _u16(data, blk + ((t >> sh) * 4) + 0x16) + 4
        vs = _u16(data, blk + 6) & 31
    while _u16(data, p) < t:
        p += 4
    t0, v0, t1, v1 = _u16(data, p - 4), _s16(data, p - 2), _u16(data, p), _s16(data, p + 2)
    if step or v0 == v1 or t == t0:
        return v0 >> vs
    if t == t1:
        return v1 >> vs
    q = ((v1 - v0) >> vs) * (t - t0)
    return int(q / (t1 - t0)) + (v0 >> vs)


def _rx(a):
    c, s = math.cos(a * TAU), math.sin(a * TAU)
    return np.array([[1, 0, 0], [0, c, -s], [0, s, c]])


def _ry(a):
    c, s = math.cos(a * TAU), math.sin(a * TAU)
    return np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])


def _rz(a):
    c, s = math.cos(a * TAU), math.sin(a * TAU)
    return np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]])


def local_pose(clip, t, rest):
    """FUN_8003c420 for every track at clip time t (wraps past `last`). Returns
    (17 local 4x4 matrices: [root, joint0..15], {joint: selector})."""
    if t > clip['last']:
        t %= clip['last'] + 1
    mats, sel = [], {}
    for k, tr in enumerate(clip['tracks'][:17]):
        layout = clip.get('layout', '573')
        v = {c: sample(clip['data'], blk, t, c == CH_SEL, layout) for c, blk in tr['chans'].items()}
        m = np.eye(4)
        # RotMatrixZYX(0, ry, rz) = Rz.Ry, then RotMatrixX(rx) left-multiplies: Rx.Rz.Ry
        m[:3, :3] = _rx(v.get(CH_RX, 0)) @ _rz(v.get(CH_RZ, 0)) @ _ry(v.get(CH_RY, 0))
        if tr['trans'] and CH_TX in v:
            m[:3, 3] = (v[CH_TX], v.get(CH_TY, 0), v.get(CH_TZ, 0))
        elif k:
            m[:3, 3] = rest[k - 1]
        if CH_SEL in v:
            sel[k - 1] = v[CH_SEL] + (1 if k - 1 == HEAD else 0)
        mats.append(m)
    return mats, sel


def pose(clip, t, rest, anchor=None):
    """World matrices [root, joint0..15] (PSX space) + selectors."""
    local, sel = local_pose(clip, t, rest)
    world = [(anchor if anchor is not None else np.eye(4)) @ local[0]]
    for j in range(16):
        world.append(world[PARENT[j] + 1] @ local[j + 1])
    return world, sel


def rest_pose(rest):
    world = [np.eye(4)]
    for j in range(16):
        m = np.eye(4)
        m[:3, 3] = rest[j]
        world.append(world[PARENT[j] + 1] @ m)
    return world, {HEAD: 1}


def load_motion(path):
    return parse_cmm(open(path, 'rb').read())


def routines(clips):
    """Routine name -> ordered clip names. Lettered clips (`hiphop1a`..`hiphop1n`) form one
    routine played a..z (4th/5th phrase table: 'abcdefghijklm', hiphop1 'abcdefghijkln';
    3rdMIX splits the same order into phrases); other clips stand alone."""
    out = {}
    for name in sorted(clips):
        stem = name[:-1]
        if len(name) > 1 and name[-1].isalpha() and sum(n[:-1] == stem for n in clips) > 2:
            out.setdefault(stem, []).append(name)
        else:
            out[name] = [name]
    return out


def routine_frames(clips, names, step=32, recenter=False):
    """Yield (clip, t, anchor) for a routine: each measure is sampled every `step` units and
    re-based on the previous measure's end root (FUN_800824f0 copies the root's world matrix
    into the anchor coordinate at every measure change)."""
    anchor = np.eye(4)
    for name in names:
        clip = clips[name]
        for t in range(0, UNITS_PER_MEASURE, step):
            yield clip, t, anchor
        anchor = anchor @ local_pose(clip, clip['last'], [np.zeros(3)] * 16)[0][0]
        if recenter:
            anchor[0, 3] = anchor[2, 3] = 0.0


# ---------------------------------------------------------------------------
# geometry out
# ---------------------------------------------------------------------------
CONV = np.diag([-1.0, -1.0, 1.0])  # PSX Y-down -> Y-up, keeps +Z facing (a proper rotation)


def posed_triangles(ch, world, sel):
    """[(3x3 positions (PSX), 3x3 normals, uvs|None, color|None)] for the visible objects."""
    out = []
    for j, oi in visible_objects(ch, sel):
        m = world[j + 1]
        for sub in ch['objects'][oi]:
            vw = sub['verts'] @ m[:3, :3].T + m[:3, 3]
            nw = sub['normals'] @ m[:3, :3].T if len(sub['normals']) else sub['normals']
            for k, (vi, ni) in enumerate(sub['tris']):
                nrm = nw[list(ni)] if len(nw) else np.zeros((3, 3))
                out.append((vw[list(vi)], nrm, sub['uvs'][k] if sub['uvs'] else None, sub['color']))
    return out


def write_obj(ch, tris, out_path):
    stem = os.path.splitext(out_path)[0]
    name = os.path.basename(stem)
    from PIL import Image
    Image.fromarray(ch['texture']).save(stem + '.png')
    colors = sorted({c for *_, c in tris if c is not None})
    with open(stem + '.mtl', 'w') as f:
        f.write('newmtl tex\nKd 1 1 1\nmap_Kd %s.png\nmap_d %s.png\n' % (name, name))
        for c in colors:
            f.write('newmtl c%02x%02x%02x\nKd %.3f %.3f %.3f\n' % (c + tuple(x / 255 for x in c)))
    with open(out_path, 'w') as f:
        f.write('mtllib %s.mtl\n' % name)
        cur = None
        n = 0
        for pts, nrm, uvs, col in tris:
            mat = 'tex' if col is None else 'c%02x%02x%02x' % col
            if mat != cur:
                f.write('usemtl %s\n' % mat)
                cur = mat
            p = pts @ CONV.T * SCALE
            q = nrm @ CONV.T
            for k in range(3):
                f.write('v %.5f %.5f %.5f\n' % tuple(p[k]))
                f.write('vn %.4f %.4f %.4f\n' % tuple(q[k]))
                u, v = uvs[k] if uvs else (0, 0)
                f.write('vt %.5f %.5f\n' % ((u + .5) / 256, 1 - (v + .5) / 256))
            # PSX triangles are clockwise seen from outside (the stored normals point along
            # -(v1-v0)x(v2-v0)); OBJ wants counter-clockwise, and CONV is a proper rotation
            f.write('f %d/%d/%d %d/%d/%d %d/%d/%d\n' % tuple(x for k in (0, 2, 1) for x in (n + k + 1,) * 3))
            n += 3


def render(tris, texture, size=384, yaw=0.0, frame_to=None):
    """Tiny z-buffered software rasterizer (preview only). yaw rotates about the up axis;
    yaw 0 looks at the dancer's front. frame_to: (lo, hi) world bbox to frame consistently."""
    c, s = math.cos(yaw), math.sin(yaw)
    rot = np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])
    view = np.diag([-1.0, 1.0, -1.0])  # camera on +Z looking back at the front, screen x right
    P = [(pts @ CONV.T) @ rot.T @ view.T for pts, *_ in tris]
    if not P:
        return np.full((size, size, 3), 40, np.uint8)
    allp = np.concatenate(P)
    lo, hi = frame_to if frame_to is not None else (allp.min(0), allp.max(0))
    sc = size * 0.9 / max(hi[0] - lo[0], hi[1] - lo[1], 1e-6)
    cx, cy = (lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2
    img = np.full((size, size, 3), 40, np.uint8)
    zb = np.full((size, size), np.inf)
    for p, (_pts, nrm, uvs, col) in zip(P, tris):
        x = (p[:, 0] - cx) * sc + size / 2
        y = size / 2 - (p[:, 1] - cy) * sc
        z = p[:, 2]
        x0, x1 = max(0, int(x.min())), min(size - 1, int(x.max()) + 1)
        y0, y1 = max(0, int(y.min())), min(size - 1, int(y.max()) + 1)
        den = (y[1] - y[2]) * (x[0] - x[2]) + (x[2] - x[1]) * (y[0] - y[2])
        if abs(den) < 1e-9 or x1 < x0 or y1 < y0:
            continue
        gx, gy = np.meshgrid(np.arange(x0, x1 + 1) + .5, np.arange(y0, y1 + 1) + .5)
        w0 = ((y[1] - y[2]) * (gx - x[2]) + (x[2] - x[1]) * (gy - y[2])) / den
        w1 = ((y[2] - y[0]) * (gx - x[2]) + (x[0] - x[2]) * (gy - y[2])) / den
        w2 = 1 - w0 - w1
        m = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
        if not m.any():
            continue
        zz = w0 * z[0] + w1 * z[1] + w2 * z[2]
        if uvs:
            uv = np.array(uvs, float)
            u = np.clip(w0 * uv[0, 0] + w1 * uv[1, 0] + w2 * uv[2, 0], 0, 255).astype(int)
            v = np.clip(w0 * uv[0, 1] + w1 * uv[1, 1] + w2 * uv[2, 1], 0, 255).astype(int)
            rgba = texture[v, u]
        else:
            rgba = np.empty(gx.shape + (4,), np.uint8)
            rgba[..., :3] = col
            rgba[..., 3] = 255
        fn = np.cross(p[1] - p[0], p[2] - p[0])
        shade = 0.55 + 0.45 * abs(fn[2]) / (np.linalg.norm(fn) + 1e-9)
        sub = zb[y0:y1 + 1, x0:x1 + 1]
        m &= (zz < sub) & (rgba[..., 3] > 0)
        sub[m] = zz[m]
        img[y0:y1 + 1, x0:x1 + 1][m] = (rgba[..., :3][m] * shade).astype(np.uint8)
    return img


# ---------------------------------------------------------------------------
# glTF 2.0 binary: rigid node rig, one mesh node per object, animations per routine
# ---------------------------------------------------------------------------
def _quat(r):
    """3x3 rotation -> glTF quaternion (x, y, z, w)."""
    tr = r[0, 0] + r[1, 1] + r[2, 2]
    if tr > 0:
        s = math.sqrt(tr + 1.0) * 2
        q = ((r[2, 1] - r[1, 2]) / s, (r[0, 2] - r[2, 0]) / s, (r[1, 0] - r[0, 1]) / s, 0.25 * s)
    elif r[0, 0] > r[1, 1] and r[0, 0] > r[2, 2]:
        s = math.sqrt(1.0 + r[0, 0] - r[1, 1] - r[2, 2]) * 2
        q = (0.25 * s, (r[0, 1] + r[1, 0]) / s, (r[0, 2] + r[2, 0]) / s, (r[2, 1] - r[1, 2]) / s)
    elif r[1, 1] > r[2, 2]:
        s = math.sqrt(1.0 + r[1, 1] - r[0, 0] - r[2, 2]) * 2
        q = ((r[0, 1] + r[1, 0]) / s, 0.25 * s, (r[1, 2] + r[2, 1]) / s, (r[0, 2] - r[2, 0]) / s)
    else:
        s = math.sqrt(1.0 + r[2, 2] - r[0, 0] - r[1, 1]) * 2
        q = ((r[0, 2] + r[2, 0]) / s, (r[1, 2] + r[2, 1]) / s, 0.25 * s, (r[1, 0] - r[0, 1]) / s)
    q = np.array(q)
    return q / np.linalg.norm(q)


class _Gltf:
    def __init__(self):
        self.bin = bytearray()
        self.j: dict[str, Any] = dict(asset=dict(version='2.0', generator='sys573_dancer_dump.py'), buffers=[], bufferViews=[],
                      accessors=[], nodes=[], meshes=[], materials=[], textures=[], images=[], samplers=[],
                      animations=[], scenes=[dict(nodes=[0])], scene=0)

    def view(self, raw, target=None):
        while len(self.bin) % 4:
            self.bin.append(0)
        bv = dict(buffer=0, byteOffset=len(self.bin), byteLength=len(raw))
        if target:
            bv['target'] = target
        self.bin += raw
        self.j['bufferViews'].append(bv)
        return len(self.j['bufferViews']) - 1

    def accessor(self, arr, kind, target=None, minmax=False):
        arr = np.ascontiguousarray(arr, dtype=np.float32 if arr.dtype.kind == 'f' else np.uint32)
        ctype = 5126 if arr.dtype == np.float32 else 5125
        acc = dict(bufferView=self.view(arr.tobytes(), target), componentType=ctype, count=len(arr), type=kind)
        if minmax:
            flat = arr.reshape(len(arr), -1)
            acc['min'] = flat.min(0).tolist()
            acc['max'] = flat.max(0).tolist()
        self.j['accessors'].append(acc)
        return len(self.j['accessors']) - 1

    def glb(self):
        self.j['buffers'] = [dict(byteLength=len(self.bin))]
        for k in [k for k, v in self.j.items() if isinstance(v, list) and not v]:
            del self.j[k]
        js = json.dumps(self.j, separators=(',', ':')).encode()
        js += b' ' * (-len(js) % 4)
        binb = bytes(self.bin) + b'\0' * (-len(self.bin) % 4)
        total = 12 + 8 + len(js) + 8 + len(binb)
        return (struct.pack('<III', 0x46546C67, 2, total) + struct.pack('<II', len(js), 0x4E4F534A) + js
                + struct.pack('<II', len(binb), 0x004E4942) + binb)


def _node_trs(m):
    r = CONV @ m[:3, :3] @ CONV.T
    t = CONV @ m[:3, 3] * SCALE
    return _quat(r), t


def export_glb(ch, motions, out_path, bpm=130.0, fps=30.0):
    """Rigid glTF: node 0 'root' (travel), 16 joint nodes, one child mesh node per object.
    Alternate hands/faces are toggled with STEP scale keys (0 = hidden). motions:
    {routine name: (clips dict, [clip names])}. Measure = 240/bpm seconds."""
    import io
    from PIL import Image
    g = _Gltf()
    buf = io.BytesIO()
    Image.fromarray(ch['texture']).save(buf, 'PNG')
    g.j['images'].append(dict(bufferView=g.view(buf.getvalue()), mimeType='image/png'))
    g.j['samplers'].append(dict(magFilter=9728, minFilter=9728))  # nearest: PSX point sampling
    g.j['textures'].append(dict(source=0, sampler=0))
    g.j['materials'] += [dict(name='tex', pbrMetallicRoughness=dict(baseColorTexture=dict(index=0),
                                                                     metallicFactor=0.0, roughnessFactor=1.0),
                              alphaMode='MASK', alphaCutoff=0.5, doubleSided=False)]
    flat_mats = {}
    rest_world, rest_sel = rest_pose(ch['rest'])
    nodes = g.j['nodes']
    nodes.append(dict(name='root', children=[]))
    for j in range(16):
        q, t = _node_trs(np.block([[np.eye(3), ch['rest'][j][:, None]], [np.zeros((1, 3)), np.ones((1, 1))]]))
        nodes.append(dict(name=JOINT_NAMES[j], translation=t.tolist(), children=[]))
    for j in range(16):
        nodes[PARENT[j] + 1]['children'].append(j + 1)
    default_vis = {oi for _, oi in visible_objects(ch, rest_sel)}
    obj_node = {}
    for j, objs in sorted(ch['groups'].items()):
        for rank, oi in enumerate(objs):
            prims = []
            keys = sorted({None if sub['uvs'] is not None else sub['color'] for sub in ch['objects'][oi]},
                          key=lambda k: (k is not None, k or ()))
            for key in keys:  # one primitive for the textured part, one per flat colour
                P, N, UV = [], [], []
                for sub in ch['objects'][oi]:
                    if (None if sub['uvs'] is not None else sub['color']) != key:
                        continue
                    for k, (vi, ni) in enumerate(sub['tris']):
                        P += list(sub['verts'][list(vi)])
                        N += list(sub['normals'][list(ni)]) if len(sub['normals']) else [np.array([0, 0, 1.0])] * 3
                        if key is None:
                            UV += [((u + .5) / 256, (v + .5) / 256) for u, v in sub['uvs'][k]]
                P = np.array(P) @ CONV.T * SCALE
                N = np.array(N) @ CONV.T
                N /= np.maximum(np.linalg.norm(N, axis=1, keepdims=True), 1e-9)
                attrs = dict(POSITION=g.accessor(P.astype(np.float32), 'VEC3', 34962, True),
                             NORMAL=g.accessor(N.astype(np.float32), 'VEC3', 34962))
                if key is None:
                    attrs['TEXCOORD_0'] = g.accessor(np.array(UV, np.float32), 'VEC2', 34962)
                    mat = 0
                else:
                    if key not in flat_mats:  # the PSX flat colour as is (sRGB bytes) -> linear factor
                        lin = [((x / 255) ** 2.2) for x in key] + [1.0]
                        g.j['materials'].append(dict(name='flat_%02x%02x%02x' % key, pbrMetallicRoughness=dict(
                            baseColorFactor=lin, metallicFactor=0.0, roughnessFactor=1.0)))
                        flat_mats[key] = len(g.j['materials']) - 1
                    mat = flat_mats[key]
                idx = np.arange(len(P), dtype=np.uint32).reshape(-1, 3)[:, ::-1].reshape(-1)  # PSX CW -> glTF CCW
                prims.append(dict(attributes=attrs, indices=g.accessor(idx, 'SCALAR', 34963), material=mat))
            if not prims:
                continue
            g.j['meshes'].append(dict(name='obj%02d' % oi, primitives=prims))
            label = '%s_%s%d' % (JOINT_NAMES[j], 'alt' if len(objs) > 1 else 'mesh', rank)
            node = dict(name=label, mesh=len(g.j['meshes']) - 1)
            if oi not in default_vis:
                node['scale'] = [0.0, 0.0, 0.0]
            nodes.append(node)
            obj_node[oi] = len(nodes) - 1
            nodes[j + 1]['children'].append(obj_node[oi])
    seconds_per_measure = 240.0 / bpm
    step = max(1, int(round(UNITS_PER_MEASURE / (fps * seconds_per_measure))))
    for rname, (clips, names) in motions.items():
        times, T = [], {k: [] for k in range(17)}
        R = {k: [] for k in range(17)}
        vis = {oi: [] for oi in obj_node}
        n = 0
        for clip, t, anchor in routine_frames(clips, names, step):
            local, sel = local_pose(clip, t, ch['rest'])
            local[0] = anchor @ local[0]
            times.append(n * step / UNITS_PER_MEASURE * seconds_per_measure)
            n += 1
            for k in range(17):
                q, tt = _node_trs(local[k])
                if R[k] and np.dot(R[k][-1], q) < 0:
                    q = -q
                R[k].append(q)
                T[k].append(tt)
            on = {oi for _, oi in visible_objects(ch, sel)}
            for oi in vis:
                vis[oi].append([1.0] * 3 if oi in on else [0.0] * 3)
        tacc = g.accessor(np.array(times, np.float32), 'SCALAR', minmax=True)
        samplers, channels = [], []
        for k in range(17):
            for path, vals, kind in (('rotation', R[k], 'VEC4'), ('translation', T[k], 'VEC3')):
                samplers.append(dict(input=tacc, output=g.accessor(np.array(vals, np.float32), kind), interpolation='LINEAR'))
                channels.append(dict(sampler=len(samplers) - 1, target=dict(node=k, path=path)))
        for oi, vals in vis.items():
            if len({v[0] for v in vals}) == 1 and (vals[0][0] == 1.0) == (oi in default_vis):
                continue
            samplers.append(dict(input=tacc, output=g.accessor(np.array(vals, np.float32), 'VEC3'), interpolation='STEP'))
            channels.append(dict(sampler=len(samplers) - 1, target=dict(node=obj_node[oi], path='scale')))
        g.j['animations'].append(dict(name=rname, samplers=samplers, channels=channels))
    with open(out_path, 'wb') as f:
        f.write(g.glb())
    return len(g.j['animations'])


# ---------------------------------------------------------------------------
# DDR World conversion (KTMDL body + own-motion .anm; the Background Dancers custom-dancer
# path of docs/dancing_stage_unleashed_dancers_port_feasibility.md, recipe in
# tools/blender_ddr_addon/examples/port_character_sys573.py). Pure numpy, no Blender.
#
# World game space: row-vector 4x4 (p_world = p_local @ M, translation in row 3), Y-up,
# metres, the dancer facing +Z with its left at +X; floor y = 0. The PSX data maps there by
# CONV and SCALE (a proper rotation: no mirror, windings stay).
# ---------------------------------------------------------------------------
# World role bone -> the 573 joint playing it (.b2it aliases; the DLL finds the shadow,
# Big Head and part bones by these names).
WORLD_ROLE_ALIASES = {'Hips': 'hips', 'Spine2': 'chest', 'Head': 'head',
                      'LeftToeBase': 'foot_L', 'RightToeBase': 'foot_R'}
WORLD_FPS = 60
MEASURE_FRAMES = 120  # World's dance clock runs the clips at 120 BPM (bpm_sync maps it)
HIDDEN_SCALE = 1e-3   # alternates are toggled by collapsing their helper bone
ATLAS_W, ATLAS_H = 512, 256  # PSX page (left 256) + flat-colour swatches (right 256)
SWATCH = 16


def _game_row(m):
    """PSX column-vector 4x4 (mm) -> World row-vector 4x4 (m)."""
    g = np.eye(4)
    g[:3, :3] = CONV @ m[:3, :3] @ CONV.T
    g[:3, 3] = CONV @ m[:3, 3] * SCALE
    return g.T


def world_bones(ch):
    """[(name, parent name | None, joint | -1, object | None)], parent first: `root`, the 16
    joints, then one helper bone per switchable object (hand shapes, faces). The head's base
    object is always drawn, so it rides `head` itself."""
    bones: list = [('root', None, -1, None)]
    for j in range(16):
        bones.append((JOINT_NAMES[j], JOINT_NAMES[PARENT[j]] if PARENT[j] >= 0 else 'root', j, None))
    for j in (13, 14, HEAD):
        objs = ch['groups'].get(j, [])
        if len(objs) < 2:
            continue
        for rank, oi in enumerate(objs):
            if j == HEAD and rank == 0:
                continue
            bones.append(('%s_alt%d' % (JOINT_NAMES[j], rank), JOINT_NAMES[j], j, oi))
    return bones


def world_binds(ch):
    """{bone: 4x4 row bind (bone local -> game world)}: identity rotation at the rest joint."""
    rest, _ = rest_pose(ch['rest'])
    out = {}
    for name, _p, j, _oi in world_bones(ch):
        out[name] = _game_row(rest[j + 1] if j >= 0 else rest[0])
    return out


def object_bone(ch, oi):
    for name, _p, _j, o in world_bones(ch):
        if o == oi:
            return name
    j = next(j for j, objs in ch['groups'].items() if oi in objs)
    return JOINT_NAMES[j]


def world_atlas(ch):
    """(RGBA ATLAS_H x ATLAS_W, {flat colour: swatch uv centre in texels}). The PSX page on the
    left; each flat colour, as is (an untextured lit PSX primitive draws its RGB unscaled: 0xFF = 1.0),
    as a SWATCH-square cell on the right, since World's character shader is texture x COLOR0 (white)."""
    img = np.zeros((ATLAS_H, ATLAS_W, 4), np.uint8)
    img[:, :256] = ch['texture']
    colors = sorted({sub['color'] for objs in ch['objects'] for sub in objs if sub['uvs'] is None})
    per_row = 256 // SWATCH
    if len(colors) > per_row * (ATLAS_H // SWATCH):
        raise ValueError('%d flat colours do not fit the swatch area' % len(colors))
    uv = {}
    for i, c in enumerate(colors):
        r, k = divmod(i, per_row)
        x0, y0 = 256 + k * SWATCH, r * SWATCH
        img[y0:y0 + SWATCH, x0:x0 + SWATCH, :3] = c
        img[y0:y0 + SWATCH, x0:x0 + SWATCH, 3] = 255
        uv[c] = (x0 + SWATCH / 2 - 0.5, y0 + SWATCH / 2 - 0.5)
    return img, uv


def world_mesh(ch):
    """Rest mesh in game space as flat per-corner arrays: positions (n x 3, m), unit normals,
    uv (D3D v-down, 0..1 over the atlas), bone name per vertex, triangles (counter-clockwise
    seen from outside, i.e. PSX order reversed). Every object is rigid on one bone."""
    rest, _ = rest_pose(ch['rest'])
    _img, swatch = world_atlas(ch)
    pos, nrm, uv, bone, tris = [], [], [], [], []
    for j, objs in sorted(ch['groups'].items()):
        m = rest[j + 1]
        for oi in objs:
            b = object_bone(ch, oi)
            for sub in ch['objects'][oi]:
                vw = (sub['verts'] @ m[:3, :3].T + m[:3, 3]) @ CONV.T * SCALE
                nw = (sub['normals'] @ m[:3, :3].T) @ CONV.T if len(sub['normals']) else None
                for k, (vi, ni) in enumerate(sub['tris']):
                    base = len(pos)
                    for c in range(3):
                        pos.append(vw[vi[c]])
                        n = nw[ni[c]] if nw is not None else np.array([0.0, 0.0, 1.0])
                        nrm.append(n / (np.linalg.norm(n) or 1.0))
                        if sub['uvs'] is not None:
                            u, v = sub['uvs'][k][c]
                            u, v = u + 0.5, v + 0.5
                        else:
                            u, v = swatch[sub['color']]
                            u, v = u + 0.5, v + 0.5
                        uv.append((u / ATLAS_W, v / ATLAS_H))
                        bone.append(b)
                    tris.append((base, base + 2, base + 1))
    return np.array(pos), np.array(nrm), np.array(uv), bone, np.array(tris, dtype=np.int64)


def routine_samples(clips, names, rest, root_mode='recentre'):
    """One sample every 2nd World frame (key time 2*i) through the whole routine, plus its end
    pose: [(frame, local PSX matrices [root, joints], selectors)]. The root carries the
    measure-to-measure travel (routine_frames). root_mode:
      'travel'   as the 573 plays it (a routine wanders up to ~3 m and ends turned);
      'recentre' the same path shifted so its x/z bounding-box centre is the dancer's mark;
      'inplace'  root x/z translation removed (turns, hops and bobbing kept)."""
    step = UNITS_PER_MEASURE * 2 // MEASURE_FRAMES
    zero = [np.zeros(3)] * 16
    out: list = []
    frame = 0
    anchor = np.eye(4)
    for ci, name in enumerate(names):
        clip = clips[name]
        ts = list(range(0, UNITS_PER_MEASURE, step))
        if ci == len(names) - 1:
            ts.append(clip['last'])
        for t in ts:
            local, sel = local_pose(clip, t, rest)
            local[0] = anchor @ local[0]
            out.append([frame, local, sel])
            frame += 2
        anchor = anchor @ local_pose(clip, clip['last'], zero)[0][0]
    if root_mode == 'inplace':
        for s in out:
            s[1][0][0, 3] = s[1][0][2, 3] = 0.0
    elif root_mode == 'recentre':
        xz = np.array([s[1][0][[0, 2], 3] for s in out])
        mid = (xz.min(0) + xz.max(0)) / 2
        for s in out:
            s[1][0][0, 3] -= mid[0]
            s[1][0][2, 3] -= mid[1]
    elif root_mode != 'travel':
        raise ValueError('root_mode %r' % root_mode)
    return out


def routine_game_worlds(ch, samples, bone_names, target_binds):
    """Per sample, per bone (file order): World world matrix for the EXPORTED binds (any rigid
    re-framing of our identity-rotation binds is absorbed by Q = B_target . B_src^-1), and per
    helper bone its visibility. Returns (frames, worlds [f][b] 4x4, visible {bone: [bool]})."""
    src = world_binds(ch)
    info = {n: (p, j, oi) for n, p, j, oi in world_bones(ch)}
    q = [target_binds[i] @ np.linalg.inv(src[n]) for i, n in enumerate(bone_names)]
    frames, worlds, visible = [], [], {n: [] for n in bone_names if info[n][2] is not None}
    for frame, local, sel in samples:
        w = [local[0]]
        for j in range(16):
            w.append(w[PARENT[j] + 1] @ local[j + 1])
        g = [_game_row(x) for x in w]
        on = {oi for _j, oi in visible_objects(ch, sel)}
        row = []
        for i, n in enumerate(bone_names):
            _p, j, oi = info[n]
            row.append(q[i] @ g[j + 1 if j >= 0 else 0])
            if oi is not None:
                visible[n].append(oi in on)
        frames.append(frame)
        worlds.append(row)
    return frames, worlds, visible


def routine_to_anm_spec(ch, samples, bone_names, parents, target_binds):
    """A scripts/anm_dump.py write_anm spec on the exported rig: local rotation (0x1C) and
    translation (0x1D) per bone at key times 2*i, plus a scale track (kind 10) per helper bone
    that steps between 1 and HIDDEN_SCALE (key pairs one frame apart at each change)."""
    frames, worlds, visible = routine_game_worlds(ch, samples, bone_names, target_binds)
    W = np.array(worlds)
    tracks = []
    for b in range(len(bone_names)):
        p = parents[b]
        locs = W[:, b] if p < 0 else np.einsum('fij,fjk->fik', W[:, b], np.linalg.inv(W[:, p]))
        quats, prev = [], None
        for m in locs:
            qv = _quat(m[:3, :3].T)  # row-vector rotation -> column form for the xyzw quaternion
            if prev is not None and float(np.dot(prev, qv)) < 0:
                qv = -qv
            quats.append(tuple(float(x) for x in qv))
            prev = qv
        trans = [tuple(float(x) for x in m[3, :3]) for m in locs]
        qa, ta = np.array(quats), np.array(trans)
        tracks.append(dict(kind=0x1C, target=b, keys=[quats[0]]) if np.abs(qa - qa[0]).max() < 1e-6
                      else dict(kind=0x1C, target=b, times=list(frames), keys=quats))
        tracks.append(dict(kind=0x1D, target=b, keys=[trans[0]]) if np.abs(ta - ta[0]).max() < 1e-5
                      else dict(kind=0x1D, target=b, times=list(frames), keys=trans))
        vis = visible.get(bone_names[b])
        if vis is not None:
            times, keys = [], []
            for k, (f, on) in enumerate(zip(frames, vis)):
                s = 1.0 if on else HIDDEN_SCALE
                if k == 0:
                    times.append(f)
                    keys.append((s, s, s))
                elif on != vis[k - 1]:
                    if f - 1 > times[-1]:
                        prev_s = keys[-1]
                        times.append(f - 1)
                        keys.append(prev_s)
                    times.append(f)
                    keys.append((s, s, s))
            if times[-1] != frames[-1]:
                times.append(frames[-1])
                keys.append(keys[-1])
            tracks.append(dict(kind=10, target=b, keys=[keys[0]]) if len(set(keys)) == 1
                          else dict(kind=10, target=b, times=times, keys=keys))
    return dict(frame_count=frames[-1], flag=0, hierarchy=list(parents), tracks=tracks), frames, worlds, visible


# ---------------------------------------------------------------------------
# commands
# ---------------------------------------------------------------------------
def _tilde(p):
    home = os.path.expanduser('~')
    return '~' + p[len(home):] if p.startswith(home) else p


def cmd_info(a):
    ch = load_character(a.chara_dir, a.name)
    print('%s: %d objects, texture %s' % (a.name, len(ch['objects']), _tilde(ch['texture_path'] or '-')))
    for j in range(16):
        objs = ch['groups'].get(j, [])
        desc = ['obj%02d(%s)' % (oi, '+'.join('%s%dv%dt' % ('T' if s['uvs'] else 'F', len(s['verts']), len(s['tris']))
                                            for s in ch['objects'][oi])) for oi in objs]
        print('  %-11s parent %-10s %s' % (JOINT_NAMES[j], JOINT_NAMES[PARENT[j]] if PARENT[j] >= 0 else 'root',
                                          ' '.join(desc)))


def cmd_cmm(a):
    clips = load_motion(a.file)
    for rname, names in routines(clips).items():
        print('%-10s %2d measure(s): %s' % (rname, len(names), ' '.join(names)))
    first = next(iter(clips.values()))
    for k, tr in enumerate(first['tracks']):
        print('  track %2d %-11s chans %s%s' % (k, 'root' if k == 0 else JOINT_NAMES[k - 1], sorted(tr['chans']),
                                                ' +trans' if tr['trans'] else ''))


def _pose_from_args(ch, a):
    if getattr(a, 'motion', None):
        clips = load_motion(a.motion)
        return pose(clips[a.clip], a.frame, ch['rest'])
    return rest_pose(ch['rest'])


def cmd_obj(a):
    ch = load_character(a.chara_dir, a.name)
    world, sel = _pose_from_args(ch, a)
    tris = posed_triangles(ch, world, sel)
    write_obj(ch, tris, a.out)
    print('%s: %d triangles -> %s' % (a.name, len(tris), _tilde(a.out)))


def cmd_glb(a):
    ch = load_character(a.chara_dir, a.name)
    motions = {}
    for path in sorted(glob.glob(os.path.join(a.motion_dir, '*', '*.cmm'))):
        if os.path.basename(path).startswith('inst'):
            continue  # the instructor's clips drive a different rig (inst.lst / inst.pos)
        clips = load_motion(path)
        for rname, names in routines(clips).items():
            motions[rname] = (clips, names)
    n = export_glb(ch, motions, a.out, a.bpm, a.fps)
    print('%s: %d animations -> %s' % (a.name, n, _tilde(a.out)))


def cmd_preview(a):
    from PIL import Image
    ch = load_character(a.chara_dir, a.name)
    tiles = []
    if a.motion:
        clips = load_motion(a.motion)
        names = routines(clips)[a.routine]
        frames = list(routine_frames(clips, names, UNITS_PER_MEASURE // 2, recenter=True))[:16]
        for clip, t, anchor in frames:
            world, sel = pose(clip, t, ch['rest'], anchor)
            tiles.append(render(posed_triangles(ch, world, sel), ch['texture'], 256))
        rows = [np.hstack(tiles[i:i + 4]) for i in range(0, len(tiles) - len(tiles) % 4 or 4, 4)]
        Image.fromarray(np.vstack(rows)).save(a.out)
    else:
        world, sel = rest_pose(ch['rest'])
        tris = posed_triangles(ch, world, sel)
        Image.fromarray(np.hstack([render(tris, ch['texture'], 384, y) for y in (0.0, 0.8, math.pi)])).save(a.out)
    print('-> %s' % _tilde(a.out))


def cmd_video(a):
    ch = load_character(a.chara_dir, a.name)
    clips = load_motion(a.file)
    names = routines(clips)[a.routine]
    size = a.size
    spm = 240.0 / a.bpm
    step = max(1, int(round(UNITS_PER_MEASURE / (30 * spm))))
    frames = list(routine_frames(clips, names, step))
    lo, hi = np.full(3, np.inf), np.full(3, -np.inf)
    posed = []
    for clip, t, anchor in frames:
        world, sel = pose(clip, t, ch['rest'], anchor)
        tris = posed_triangles(ch, world, sel)
        posed.append(tris)
        pts = np.concatenate([p for p, *_ in tris]) @ CONV.T @ np.diag([-1.0, 1.0, -1.0]).T
        lo, hi = np.minimum(lo, pts.min(0)), np.maximum(hi, pts.max(0))
    proc = subprocess.Popen(['ffmpeg', '-y', '-loglevel', 'error', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
                             '-s', '%dx%d' % (size, size), '-r', '%.4f' % (UNITS_PER_MEASURE / step / spm),
                             '-i', '-', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '18', a.out],
                            stdin=subprocess.PIPE)
    assert proc.stdin is not None
    for tris in posed:
        proc.stdin.write(render(tris, ch['texture'], size, 0.0, (lo, hi)).tobytes())
    proc.stdin.close()
    proc.wait()
    print('%s %s: %d frames -> %s' % (a.name, a.routine, len(posed), _tilde(a.out)))


def cmd_survey(a):
    for mix in a.dirs:
        chara = os.path.join(mix, 'data', 'chara')
        ok = bad = 0
        for d in sorted(glob.glob(os.path.join(chara, '*', '*.cmd'))):
            name = os.path.basename(os.path.dirname(d))
            try:
                ch = load_character(chara, name)
                for objs in ch['objects']:
                    for sub in objs:
                        for vi, ni in sub['tris']:
                            assert max(vi) < len(sub['verts']) and (not len(sub['normals']) or max(ni) < len(sub['normals']))
                ok += 1
            except (ValueError, AssertionError, struct.error) as e:
                bad += 1
                print('  skip %s: %s' % (name, e))
        nclip = 0
        for m in sorted(glob.glob(os.path.join(mix, 'data', 'motion', '*', '*.cmm'))):
            clips = load_motion(m)
            for clip in clips.values():
                assert len(clip['tracks']) == 17 and clip['last'] == UNITS_PER_MEASURE
                for t in range(0, UNITS_PER_MEASURE + 1, 240):
                    local_pose(clip, t, [np.zeros(3)] * 16)
            nclip += len(clips)
        print('%s: %d models ok, %d skipped, %d clips' % (_tilde(mix), ok, bad, nclip))


def main(argv):
    ap = argparse.ArgumentParser(description='System 573 DDR polygon dancer decoder / exporter.')
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('info')
    p.add_argument('chara_dir')
    p.add_argument('name')
    p = sub.add_parser('cmm')
    p.add_argument('file')
    for cmd in ('obj', 'preview'):
        p = sub.add_parser(cmd)
        p.add_argument('chara_dir')
        p.add_argument('name')
        p.add_argument('out')
        p.add_argument('--motion')
        if cmd == 'obj':
            p.add_argument('--clip')
            p.add_argument('--frame', type=int, default=0)
        else:
            p.add_argument('--routine')
    p = sub.add_parser('glb')
    p.add_argument('chara_dir')
    p.add_argument('name')
    p.add_argument('motion_dir')
    p.add_argument('out')
    p.add_argument('--bpm', type=float, default=130.0)
    p.add_argument('--fps', type=float, default=30.0)
    p = sub.add_parser('video')
    p.add_argument('chara_dir')
    p.add_argument('name')
    p.add_argument('file')
    p.add_argument('routine')
    p.add_argument('out')
    p.add_argument('--bpm', type=float, default=130.0)
    p.add_argument('--size', type=int, default=320)
    p = sub.add_parser('survey')
    p.add_argument('dirs', nargs='+')
    a = ap.parse_args(argv)
    dict(info=cmd_info, cmm=cmd_cmm, obj=cmd_obj, glb=cmd_glb, preview=cmd_preview, video=cmd_video,
         survey=cmd_survey)[a.cmd](a)


if __name__ == '__main__':
    main(sys.argv[1:])
