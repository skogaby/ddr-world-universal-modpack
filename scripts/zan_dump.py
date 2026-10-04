#!/usr/bin/env python3
"""Reference decoder for Konami's `zan` engine formats as used by DanceDanceRevolution
FuruFuru Party (Wii, JP 2008 = HOTTEST PARTY 2, `RD4JA4`) and DanceDanceRevolution MUSIC FIT
(Wii, JP 2009 = HOTTEST PARTY 3, `RJRJA4`): the polygon dancers, their dance clips, the stages and
the cameras, plus the conversion math the DDR World ports use
(tools/blender_ddr_addon/examples/port_character_hottest2.py, port_stage_hottest2.py).
Formats and RE: docs/wii_ddr_hottest_party_2_3_research.md. All values are big-endian.

Unlike HOTTEST PARTY 1 (Hudson's Mario Party engine, scripts/hsf_dump.py) these two games run on
Konami's own Wii library (`CzanFileManager`, `CzanModel`, ... in main.dol). Every container on the
disc is a `WII\\0` archive:

  archive   char magic[4] "WII\\0", f32 1.0, u32 count, u32 name_words, then count x {u32 offset,
            u32 size} (relative to the archive start), then -- when name_words > 0 -- count names
            of name_words * 4 bytes. Members are ZMB models, ZAB motions, GX TPL textures, `@@`
            cameras, nested archives, ...
  ZMB       char magic[8] "ZMB GC\\0\\0", u32[3], f32 1.0, u32 textures, materials, nodes (block
            offsets; 0 = absent). Each block starts {u32 count, f32 version, u32 offset, u32 0}
            (the texture block's count is the low u16; MUSIC FIT sets the high u16 to 1).
    textures  0x20-byte names (`tex_c_04.tga`). The pictures are NOT in the ZMB: a character's
              textures are the TPL of its costume file, a stage model's the TPL beside it; the
              material texture index is the TPL image index (the TPL may hold more images than
              the name list).
    material  version 1.0 = 0x38 bytes, 3.0 = 0x50: u32 color0, color1, color2 (RGBA8), f32,
              u8 flags[4], u32 ntex (low u16), u32 tex_list (u32 TPL image indices), ...
              +0x28 u32 nlayers, +0x2C u32 layers (more material-shaped records: environment /
              eye / mouth passes), +0x30 u32 nframes, +0x34 u32 frames (texture animation: TPL
              indices, e.g. the seven eye frames).
    node      0xA0: char name[0x30] (Shift-JIS), f32 local[16] (ROW-vector: v' = v . M,
              translation in row 3), f32 bbmin[4], bbmax[4], f32 bone length, s32 parent,
              u32 nsub, u32 mesh (offset; 0 = none). world = local . parent world.
    submesh   nsub x 0x40 at `mesh`: u32 material, u32 flags (bit 0 skinned, bit 16 a second UV
              set), u32 npackets, npos, nskin, nnrm, nuv, ncol, u32 packets, pos (f32 xyz),
              skin, nrm (f32 xyz), uv (f32 st, v down), col (RGBA8), u32 0, 0.
    packet    a triangle strip: u16 kind, u16 corners, then u32 offsets of per-corner u32 index
              arrays {position, normal, colour, uv0} (0x14 bytes) or {.., uv1, 0, 0} (0x20 when
              flags bit 16). Every packet is a strip whatever its kind (the kind is a matrix /
              state group of the draw code, not a primitive type).
    skin      per position {u32 n, u32 offset} -> n x {char joint[0x3C], f32 weight}: the joint
              by NAME. Positions are in model space at the rest pose (the nodes' rest worlds),
              so a vertex lands at sum w_j . v . W_j^-1 . C_j (row vectors).
  ZAB       char magic[8] "ZAB GC\\0\\0", f32 1.0, u32 nbones, u32 length (frames, 60 Hz), u32[2],
            s32 -1, u32 bones (offset, 0x30); per bone 0x40: char name[0x30], s32 -1, u32
            nchannels, u32 flags, u32 channels; per channel 0x10: u32 kind (0 translation, 1
            rotation, 2 scale), u32 key size (0x10 / 0x14), u32 nkeys, u32 keys; a key is
            {u32 frame, f32 value[3 | 4]}: translation / scale xyz, rotation a quaternion
            (x, y, z, w) whose row-vector matrix is the bone's local rotation. Values replace
            the node's local transform; keys are linear (slerp for rotation).
  camera    f32 length (seconds; 3.0 = "@@\\0\\0" for most), then 6 x {u32 nkeys, u32 offset}:
            position {f32 time (s), xyz}, rotation {time, quaternion (its row 1 is minus the view
            direction; no roll)}, fov {time, deg -- MTXPerspective's fovY}, aim {time, xyz}, near
            {time, v}, far {time, v}; 12 constant bytes at 0x34 (28f81200 7cf71200 5cf71200).

Usage:
    zan_dump.py ls <file>                       # archive tree with member kinds
    zan_dump.py info <file> [member]            # ZMB / ZAB / camera summary
    zan_dump.py survey <dir>...                 # parse every ZMB / ZAB / camera under the dirs

Import-safe: `from zan_dump import walk, parse_zmb, parse_zab, ...`. Needs numpy.
GX texture decoding is in scripts/extract_wii_ddr_data.py (imported).
"""
import argparse
import math
import os
import re
import struct
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import extract_wii_ddr_data as W  # noqa: E402  (GX textures, TPL, PNG)

ARCHIVE_MAGIC = b'WII\0'
ZMB_MAGIC = b'ZMB GC\0\0'
ZAB_MAGIC = b'ZAB GC\0\0'
CAM_SIGNATURE = bytes.fromhex('28f812007cf712005cf71200')   # at 0x34 of every camera
NODE_SIZE = 0xA0
SUBMESH_SIZE = 0x40
MATERIAL_SIZE = {1.0: 0x38, 3.0: 0x50}
SKIN_INFLUENCE = 0x40


def _u32(b, o):
    return struct.unpack_from('>I', b, o)[0]


def _s32(b, o):
    return struct.unpack_from('>i', b, o)[0]


def _f32(b, o):
    return struct.unpack_from('>f', b, o)[0]


def _cstr(b, o, n):
    return b[o:o + n].split(b'\0')[0].decode('shift_jis', 'replace')


# ---------------------------------------------------------------------------
# archives
# ---------------------------------------------------------------------------
def is_archive(blob, at=0):
    return blob[at:at + 4] == ARCHIVE_MAGIC and len(blob) >= at + 0x10


def _name_stride(blob, names_at, end, n, name_words):
    """Byte stride of an archive's name table. `name_words` 16 means 64-byte names; the
    smaller classes (2, 4) pack the names at a stride of the longest name + NUL rounded up to
    8 (16 / 24 on the disc). Picked as the first stride that puts every name on a printable
    first byte after a NUL."""
    cands = [name_words * 4] + list(range(8, 257, 8))
    for s in cands:
        if s < 8 or names_at + n * s > end + 32:
            continue
        ok = True
        for k in range(n):
            c = blob[names_at + k * s] if names_at + k * s < len(blob) else 0
            if not 0x20 < c < 0x7F or (k and blob[names_at + k * s - 1] != 0):
                ok = False
                break
        if ok:
            return s
    return name_words * 4


def archive_members(blob, at=0):
    """[(name or None, absolute offset, size)] of the archive at `at`."""
    _magic, _ver, n, name_words = struct.unpack_from('>4sfII', blob, at)
    names_at = at + 0x10 + 8 * n
    table = [struct.unpack_from('>II', blob, at + 0x10 + 8 * i) for i in range(n)]
    stride = 0
    if name_words:
        first = min((o for o, s in table if o), default=len(blob) - at)
        stride = _name_stride(blob, names_at, at + first, n, name_words)
    out = []
    for i, (o, s) in enumerate(table):
        name = _cstr(blob, names_at + i * stride, stride) or None if stride else None
        out.append((name, at + o, s))
    return out


def kind_of(blob, at=0):
    m = blob[at:at + 8]
    if m[:4] == ARCHIVE_MAGIC:
        return 'archive'
    if m == ZMB_MAGIC:
        return 'zmb'
    if m == ZAB_MAGIC:
        return 'zab'
    if blob[at + 0x34:at + 0x40] == CAM_SIGNATURE:
        return 'cam'
    if m[:4] == W.TPL_MAGIC:
        return 'tpl'
    if m[:4] == b'ZMS\0':
        return 'zms'
    if m[:4] == b'TEB\0':
        return 'teb'
    return 'bin'


def walk(blob, at=0, path=''):
    """Yield (path, name, offset, size, kind) for every member, depth first; a member path is
    `/<name or #index>` per level."""
    for i, (name, o, s) in enumerate(archive_members(blob, at)):
        p = '%s/%s' % (path, name if name else '#%d' % i)
        k = kind_of(blob, o) if s else 'empty'
        yield p, name, o, s, k
        if k == 'archive':
            yield from walk(blob, o, p)


def members(blob, kind=None):
    """[(path, name, bytes)] of every member (of `kind`)."""
    return [(p, n, blob[o:o + s]) for p, n, o, s, k in walk(blob) if kind is None or k == kind]


def tpl_images(blob):
    """RGBA arrays of a TPL."""
    return W.tpl_images(blob)


# ---------------------------------------------------------------------------
# ZMB models
# ---------------------------------------------------------------------------
def _block(b, off):
    if not off:
        return 0, None, 0
    return _u32(b, off), _f32(b, off + 4), _u32(b, off + 8)


def _material(b, o, size):
    w = struct.unpack_from('>%dI' % (size // 4), b, o)
    ntex = w[5] & 0xFFFF
    tex = [_u32(b, w[6] + 4 * k) for k in range(ntex)] if w[6] and ntex < 64 else []
    m = dict(offset=o, color0=w[0], color1=w[1], color2=w[2], value=_f32(b, o + 12),
             flags=bytes(b[o + 16:o + 20]), ntex_word=w[5], textures=tex, words=w)
    if size == 0x38:
        nlay, lay, nfr, fr = w[10] & 0xFFFF, w[11], w[12], w[13]
    else:
        nlay, lay, nfr, fr = w[10] & 0xFFFF, w[11], w[12], w[13]
    m['layer_offsets'] = [lay + size * k for k in range(nlay)] if lay and nlay < 16 else []
    m['frames'] = [_u32(b, fr + 4 * k) for k in range(nfr)] if fr and nfr < 256 else []
    return m


def parse_zmb(b):
    """dict(textures, materials, layers {offset: material}, nodes, by_name)."""
    if b[:8] != ZMB_MAGIC:
        raise ValueError('not a ZMB')
    tex_off, mat_off, node_off = _u32(b, 0x18), _u32(b, 0x1C), _u32(b, 0x20)
    textures = []
    if tex_off:
        n = _u32(b, tex_off) & 0xFFFF
        names = _u32(b, tex_off + 8)
        textures = [_cstr(b, names + 0x20 * i, 0x20) for i in range(n)]
    materials, layers, mat_version = [], {}, None
    if mat_off:
        n, mat_version, mo = _block(b, mat_off)
        size = MATERIAL_SIZE[mat_version]
        materials = [_material(b, mo + size * i, size) for i in range(n)]
        todo = [o for m in materials for o in m['layer_offsets']]
        while todo:
            o = todo.pop()
            if o not in layers:
                layers[o] = _material(b, o, size)
                todo += layers[o]['layer_offsets']
    nodes = []
    if node_off:
        n, _v, no = _block(b, node_off)
        for i in range(n):
            r = no + NODE_SIZE * i
            nodes.append(dict(index=i, name=_cstr(b, r, 0x30),
                              local=np.array(struct.unpack_from('>16f', b, r + 0x30), dtype=np.float64).reshape(4, 4),
                              bbmin=struct.unpack_from('>3f', b, r + 0x70), bbmax=struct.unpack_from('>3f', b, r + 0x80),
                              length=_f32(b, r + 0x90), parent=_s32(b, r + 0x94), flags=struct.unpack_from('>H', b, r + 0x98)[0],
                              nsub=struct.unpack_from('>H', b, r + 0x9A)[0],
                              mesh_offset=_u32(b, r + 0x9C), submeshes=[]))
    for nd in nodes:
        if nd['mesh_offset'] and nd['nsub']:
            nd['submeshes'] = [_submesh(b, nd['mesh_offset'] + SUBMESH_SIZE * j) for j in range(nd['nsub'])]
    return dict(textures=textures, materials=materials, layers=layers, material_version=mat_version,
                nodes=nodes, by_name={nd['name']: nd['index'] for nd in nodes})


def _submesh(b, o):
    w = struct.unpack_from('>8I6I', b, o)
    mat, flags, npk, npos, nskin, nnrm, nuv, ncol = w[:8]
    p_pk, p_pos, p_skin, p_nrm, p_uv, p_col = w[8:]
    rec = 0x20 if flags & 0x10000 else 0x14
    nptr = 7 if rec == 0x20 else 4
    packets = []
    for q in range(npk):
        kind, n = struct.unpack_from('>HH', b, p_pk + rec * q)
        ptrs = struct.unpack_from('>%dI' % nptr, b, p_pk + rec * q + 4)
        idx = [np.frombuffer(b, '>u4', n, p).astype(np.int64) if p else None for p in ptrs]
        packets.append(dict(kind=kind, corners=n, pos=idx[0], nrm=idx[1], col=idx[2], uv0=idx[3],
                            uv1=idx[4] if nptr > 4 else None))
    skin = None
    if nskin:
        skin = []
        for v in range(nskin):
            k, ptr = struct.unpack_from('>II', b, p_skin + 8 * v)
            skin.append([(_cstr(b, ptr + SKIN_INFLUENCE * z, 0x3C), _f32(b, ptr + SKIN_INFLUENCE * z + 0x3C))
                         for z in range(k)])
    col = None
    if ncol and p_col:
        col = np.frombuffer(b, np.uint8, ncol * 4, p_col).reshape(-1, 4).astype(np.float64) / 255.0
    return dict(material=mat, flags=flags, packets=packets,
                pos=np.frombuffer(b, '>f4', npos * 3, p_pos).reshape(-1, 3).astype(np.float64) if npos else np.zeros((0, 3)),
                nrm=np.frombuffer(b, '>f4', nnrm * 3, p_nrm).reshape(-1, 3).astype(np.float64) if nnrm else np.zeros((0, 3)),
                uv=np.frombuffer(b, '>f4', nuv * 2, p_uv).reshape(-1, 2).astype(np.float64) if nuv else np.zeros((0, 2)),
                col=col, skin=skin)


def strip_triangles(n):
    """Corner triples of a GX triangle strip of `n` corners (every other one flipped)."""
    return [(i, i + 1, i + 2) if i % 2 == 0 else (i + 1, i, i + 2) for i in range(n - 2)]


def rest_worlds(model):
    """[4x4 ROW-vector rest world per node] (world = local . parent world)."""
    out = [None] * len(model['nodes'])

    def get(i):
        if out[i] is None:
            nd = model['nodes'][i]
            out[i] = nd['local'] if nd['parent'] < 0 else nd['local'] @ get(nd['parent'])
        return out[i]

    for i in range(len(out)):
        get(i)
    return out


# ---------------------------------------------------------------------------
# ZAB motions
# ---------------------------------------------------------------------------
CH_T, CH_R, CH_S = 0, 1, 2


def parse_zab(b):
    """dict(length, bones {name: {kind: (frames (n,), values (n, 3|4))}}, order [names])."""
    if b[:8] != ZAB_MAGIC:
        raise ValueError('not a ZAB')
    n, length = _u32(b, 0x0C), _u32(b, 0x10)
    table = _u32(b, 0x20)
    bones, order = {}, []
    for i in range(n):
        r = table + 0x40 * i
        name = _cstr(b, r, 0x30)
        nch, _flags, ch = _u32(b, r + 0x34), _u32(b, r + 0x38), _u32(b, r + 0x3C)
        chans = {}
        for c in range(nch):
            kind, size, nk, ptr = struct.unpack_from('>4I', b, ch + 0x10 * c)
            raw = np.frombuffer(b, '>u4', nk * size // 4, ptr).reshape(nk, size // 4)
            frames = raw[:, 0].astype(np.float64)
            vals = raw[:, 1:].copy().view('>f4').astype(np.float64)
            chans[kind] = (frames, vals)
        bones[name] = chans
        order.append(name)
    return dict(length=length, bones=bones, order=order)


def _interp(frames, vals, t):
    """Linear key interpolation at times `t` (clamped)."""
    if len(frames) == 1:
        return np.repeat(vals[:1], len(t), 0)
    out = np.empty((len(t), vals.shape[1]))
    for c in range(vals.shape[1]):
        out[:, c] = np.interp(t, frames, vals[:, c])
    return out


def _slerp_series(frames, quats, t):
    """Quaternion slerp of (n, 4) keys at times `t`, neighbours sign-aligned."""
    q = quats.copy()
    for i in range(1, len(q)):
        if np.dot(q[i - 1], q[i]) < 0:
            q[i] = -q[i]
    if len(frames) == 1:
        return np.repeat(q[:1], len(t), 0)
    t = np.clip(t, frames[0], frames[-1])
    i = np.clip(np.searchsorted(frames, t, side='right') - 1, 0, len(frames) - 2)
    a, b = q[i], q[i + 1]
    span = frames[i + 1] - frames[i]
    s = np.where(span > 0, (t - frames[i]) / np.where(span > 0, span, 1.0), 0.0)
    d = np.clip((a * b).sum(1), -1.0, 1.0)
    th = np.arccos(d)
    sn = np.sin(th)
    small = sn < 1e-6
    wa = np.where(small, 1 - s, np.sin((1 - s) * th) / np.where(small, 1.0, sn))
    wb = np.where(small, s, np.sin(s * th) / np.where(small, 1.0, sn))
    out = wa[:, None] * a + wb[:, None] * b
    return out / np.linalg.norm(out, axis=1, keepdims=True)


def quat_rows(q):
    """(..., 4) quaternions (x, y, z, w) -> (..., 3, 3) ROW-vector rotation matrices."""
    x, y, z, w = q[..., 0], q[..., 1], q[..., 2], q[..., 3]
    m = np.empty(q.shape[:-1] + (3, 3))
    m[..., 0, 0] = 1 - 2 * (y * y + z * z)
    m[..., 0, 1] = 2 * (x * y + z * w)
    m[..., 0, 2] = 2 * (x * z - y * w)
    m[..., 1, 0] = 2 * (x * y - z * w)
    m[..., 1, 1] = 1 - 2 * (x * x + z * z)
    m[..., 1, 2] = 2 * (y * z + x * w)
    m[..., 2, 0] = 2 * (x * z + y * w)
    m[..., 2, 1] = 2 * (y * z - x * w)
    m[..., 2, 2] = 1 - 2 * (x * x + y * y)
    return m


def posed_locals(model, motion, times, names=None, unit_scale=False):
    """{node name: (F, 4, 4) ROW-vector local} under `motion` at `times` (frames): a channel
    replaces the node's rest value; unanimated nodes keep their rest local. `unit_scale` sets
    every scale to 1 (the rotation frame of a flattened, zero-scaled prop stays proper)."""
    t = np.asarray(times, dtype=np.float64)
    out = {}
    for nd in model['nodes']:
        if names is not None and nd['name'] not in names:
            continue
        loc = np.repeat(nd['local'][None], len(t), 0).copy()
        ch = motion['bones'].get(nd['name']) if motion else None
        if unit_scale and not ch:
            sc = np.linalg.norm(nd['local'][:3, :3], axis=1)
            loc[:, :3, :3] = nd['local'][:3, :3] / np.where(sc > 1e-12, sc, 1.0)[:, None]
        if ch:
            rest = nd['local']
            scale = np.linalg.norm(rest[:3, :3], axis=1)
            rot = rest[:3, :3] / np.where(scale > 1e-12, scale, 1.0)[:, None]
            if CH_R in ch:
                rot = quat_rows(_slerp_series(ch[CH_R][0], ch[CH_R][1], t))
            else:
                rot = np.repeat(rot[None], len(t), 0)
            if unit_scale:
                scale = np.ones((len(t), 3))
            elif CH_S in ch:
                scale = _interp(ch[CH_S][0], ch[CH_S][1], t)
            else:
                scale = np.repeat(scale[None], len(t), 0)
            loc[:, :3, :3] = scale[:, :, None] * rot
            if CH_T in ch:
                loc[:, 3, :3] = _interp(ch[CH_T][0], ch[CH_T][1], t)
        out[nd['name']] = loc
    return out


def posed_worlds(model, motion, times, unit_scale=False):
    """(F, nodes, 4, 4) ROW-vector worlds of every node under `motion`."""
    loc = posed_locals(model, motion, times, unit_scale=unit_scale)
    n = len(model['nodes'])
    out = np.empty((len(times), n, 4, 4))
    done = [False] * n

    def get(i):
        if not done[i]:
            nd = model['nodes'][i]
            L = loc[nd['name']]
            out[:, i] = L if nd['parent'] < 0 else np.einsum('fij,fjk->fik', L, get(nd['parent']))
            done[i] = True
        return out[:, i]

    for i in range(n):
        get(i)
    return out


# ---------------------------------------------------------------------------
# dancers (port_character_hottest2.py)
# ---------------------------------------------------------------------------
# Every dancer of both games shares one 37-bone Maya rig (Hips 8.593 units up, facing +Z, left at
# +X -- World's frame), plus the AccL/RForeArm, AccL/RHand and mii_head attach joints. A costume file
# sound/stream/character/CHR<nn>0.bin holds the body ZMB (skinned to the rig by joint NAME) and the
# head ZMB (rigid, in the frame of the body's `mii_head` joint -- the game swaps it for a Mii head);
# CHR<nn><k>.bin (k = 1..) is colour variant k: {body TPL, head TPL}. MUSIC FIT wraps the rig in
# identity `trans` / `scale` nodes and parents a few rigid meshes (`HP3A_01_obj_body`) to joints.
HIPS_UNITS = 8.593
GAME_SCALE = 0.970 / HIPS_UNITS
ATTACH_NODE = 'mii_head'
ROLE_ALIASES = {'Spine2': 'Spine1'}   # World role bone -> the zan joint playing it


def _skin_targets(model):
    out = set()
    for nd in model['nodes']:
        for sm in nd['submeshes']:
            for ws in sm['skin'] or ():
                out.update(j for j, _w in ws)
    return out


def rig_joints(model, keep=()):
    """[(joint, parent joint or None)], parents first. Joints are the nodes that are skin
    targets, motion bones (`keep`), attach points or ancestors of those -- not the scene root
    (parent -1), not mesh holders, not identity wrappers (`trans` / `scale`)."""
    nodes = model['nodes']
    by = model['by_name']
    want = set(_skin_targets(model)) | set(keep) | {n['name'] for n in nodes if n['name'].startswith('Acc')}
    want.add(ATTACH_NODE)
    want = {n for n in want if n in by}

    def is_joint(i):
        nd = nodes[i]
        if nd['parent'] < 0 or nd['submeshes']:
            return False
        if nd['name'] in want:
            return True
        ident = np.allclose(nd['local'], np.eye(4), atol=1e-6)
        return not ident and _has_wanted_descendant(model, i, want)

    joint = [is_joint(i) for i in range(len(nodes))]

    def parent_joint(i):
        p = nodes[i]['parent']
        while p >= 0 and not joint[p]:
            p = nodes[p]['parent']
        return nodes[p]['name'] if p >= 0 else None

    out = [(nodes[i]['name'], parent_joint(i)) for i in range(len(nodes)) if joint[i]]
    depth = {}
    parent_of = dict(out)

    def d(n):
        if n not in depth:
            depth[n] = 0 if parent_of[n] is None else d(parent_of[n]) + 1
        return depth[n]

    order = {n: k for k, (n, _p) in enumerate(out)}
    return sorted(out, key=lambda jp: (d(jp[0]), order[jp[0]]))


def _has_wanted_descendant(model, i, want):
    kids = [n['index'] for n in model['nodes'] if n['parent'] == i]
    return any(model['nodes'][k]['name'] in want or _has_wanted_descendant(model, k, want) for k in kids)


def game_row(m, s=GAME_SCALE):
    """A ROW-vector world in file units -> metres (rotation kept)."""
    out = np.array(m, dtype=np.float64, copy=True)
    out[..., 3, :3] *= s
    return out


def game_bind_matrices(model, joints, s=GAME_SCALE):
    rest = rest_worlds(model)
    return {n: game_row(rest[model['by_name'][n]], s) for n, _p in joints}


def _nearest_joint(model, i, joint_names):
    while i >= 0 and model['nodes'][i]['name'] not in joint_names:
        i = model['nodes'][i]['parent']
    return model['nodes'][i]['name'] if i >= 0 else None


def _row_apply(v, m):
    return v @ m[:3, :3] + m[3, :3]


def _normal_apply(n, m):
    out = n @ np.linalg.inv(m[:3, :3]).T
    return out / np.maximum(np.linalg.norm(out, axis=1, keepdims=True), 1e-12)


def model_pieces(model, joint_names, frame=None, default_joint=None):
    """Every submesh of a ZMB in MODEL space at rest: [dict(node, sub, material, flags, pos,
    nrm, uv, col, corners (n, 5) {pos, nrm, col, uv0, uv1} indices per strip corner, tris
    (corner triples), weights per position [(joint, w)])]. Skinned submeshes are in model
    space already; a rigid one is in its node's frame and rides its nearest joint ancestor
    (`default_joint` when none). `frame` (4x4 row) is applied on top (the head's mii_head)."""
    rest = rest_worlds(model)
    out = []
    for nd in model['nodes']:
        for j, sm in enumerate(nd['submeshes']):
            if not len(sm['pos']) or not sm['packets']:
                continue
            m = np.eye(4) if sm['skin'] is not None else rest[nd['index']]
            if frame is not None:
                m = m @ frame
            pos = _row_apply(sm['pos'], m)
            nrm = _normal_apply(sm['nrm'], m) if len(sm['nrm']) else np.zeros((0, 3))
            if sm['skin'] is not None:
                weights = [[(jn, w) for jn, w in ws if w > 0] for ws in sm['skin']]
            else:
                jn = _nearest_joint(model, nd['index'], joint_names) or default_joint
                weights = [[(jn, 1.0)] for _ in range(len(pos))]
            corners, tris = [], []
            for pk in sm['packets']:
                n = pk['corners']
                base = len(corners)
                z = np.zeros(n, dtype=np.int64)
                cols = [pk['pos'], pk['nrm'] if pk['nrm'] is not None else z,
                        pk['col'] if pk['col'] is not None else z, pk['uv0'] if pk['uv0'] is not None else z,
                        pk['uv1'] if pk['uv1'] is not None else z]
                corners.extend(np.stack(cols, 1).tolist())
                tris.extend((base + a, base + b, base + c) for a, b, c in strip_triangles(n))
            out.append(dict(node=nd['name'], sub=j, material=sm['material'], flags=sm['flags'], pos=pos, nrm=nrm,
                            uv=sm['uv'], col=sm['col'], corners=np.array(corners, dtype=np.int64),
                            tris=np.array(tris, dtype=np.int64).reshape(-1, 3), weights=weights))
    return out


def weld(piece, uv_set=0, s=GAME_SCALE):
    """One GX vertex per distinct (position, normal, colour, uv) corner of a piece: (pos (n, 3)
    metres, nrm, uv (v down), col RGBA or None, weights, triangles), degenerate strip joints
    dropped."""
    index, remap = {}, []
    P, N, UV, C, Wt = [], [], [], [], []
    pc = piece
    for c in pc['corners']:
        key = (c[0], c[1], c[2], c[3 + uv_set])
        if key not in index:
            index[key] = len(P)
            P.append(pc['pos'][c[0]] * s)
            N.append(pc['nrm'][c[1]] if len(pc['nrm']) and c[1] < len(pc['nrm']) else (0.0, 1.0, 0.0))
            UV.append(pc['uv'][c[3 + uv_set]] if len(pc['uv']) and c[3 + uv_set] < len(pc['uv']) else (0.0, 0.0))
            C.append(pc['col'][c[2]] if pc['col'] is not None and c[2] < len(pc['col']) else (1.0, 1.0, 1.0, 1.0))
            Wt.append(pc['weights'][c[0]])
        remap.append(index[key])
    remap = np.array(remap, dtype=np.int64)
    T = remap[pc['tris']] if len(pc['tris']) else np.zeros((0, 3), dtype=np.int64)
    if len(T):
        T = T[(T[:, 0] != T[:, 1]) & (T[:, 1] != T[:, 2]) & (T[:, 0] != T[:, 2])]
    return (np.array(P, dtype=np.float64).reshape(-1, 3), np.array(N, dtype=np.float64).reshape(-1, 3),
            np.array(UV, dtype=np.float64).reshape(-1, 2), np.array(C, dtype=np.float64).reshape(-1, 4), Wt, T)


def material_layer(model, material_index):
    """The overlay pass of a material (its +0x2C layer when the layer is a plain pass, b3 == 0:
    the eye / mouth frames drawn through UV set 1), else None."""
    mt = model['materials'][material_index]
    for o in mt['layer_offsets']:
        lay = model['layers'][o]
        if lay['flags'][3] == 0 and lay['textures']:
            return lay
    return None


BLEND_OPAQUE, BLEND_ADD, BLEND_DARKEN, BLEND_ALPHA = 0, 1, 2, 3


def material_mode(material):
    """(blend, soft, two_sided, lit) of a zan material (main.dol FUN_8010def4 / FUN_8010dec8):
    flags[2] & 0x7F = blend (0 opaque, 1 additive SRCALPHA+ONE with z-write off, 2 ZERO +
    INVSRCALPHA, 3 alpha blend); flags[2] bit 7 = soft edge (alpha compare > 0; clear = alpha
    test > 160); flags[1] = no culling; flags[0] = GX lighting."""
    f = material['flags']
    return f[2] & 0x7F, bool(f[2] & 0x80), bool(f[1]), bool(f[0])


def bake_overlay(base_rgba, over_rgba, piece, scale=2):
    """Composite an overlay texture (UV set 1, e.g. the open-eyes frame) over the base texture
    (UV set 0) into a picture in UV-1 space, so the overlay mesh can be drawn with one texture:
    every UV-1 texel inside a triangle samples the base at the barycentric UV-0 and puts the
    overlay on top (alpha over). Returns an RGBA uint8 (h * scale, w * scale)."""
    oh, ow = over_rgba.shape[:2]
    H, Wd = oh * scale, ow * scale
    out = np.zeros((H, Wd, 4), dtype=np.float64)
    hit = np.zeros((H, Wd), dtype=bool)
    uv = piece['uv']
    bh, bw = base_rgba.shape[:2]
    base = base_rgba.astype(np.float64) / 255.0
    for t in piece['tris']:
        c = piece['corners'][t]
        u1 = uv[c[:, 4]] * (Wd, H)
        u0 = uv[c[:, 3]]
        x0, y0 = np.floor(u1.min(0)).astype(int)
        x1, y1 = np.ceil(u1.max(0)).astype(int)
        x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, Wd - 1), min(y1, H - 1)
        if x1 < x0 or y1 < y0:
            continue
        ys, xs = np.mgrid[y0:y1 + 1, x0:x1 + 1]
        px, py = xs.ravel() + 0.5, ys.ravel() + 0.5
        (ax, ay), (bx, by), (cx, cy) = u1
        den = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
        if abs(den) < 1e-12:
            continue
        l0 = ((by - cy) * (px - cx) + (cx - bx) * (py - cy)) / den
        l1 = ((cy - ay) * (px - cx) + (ax - cx) * (py - cy)) / den
        l2 = 1 - l0 - l1
        ok = (l0 >= -0.02) & (l1 >= -0.02) & (l2 >= -0.02)
        if not ok.any():
            continue
        st = l0[ok, None] * u0[0] + l1[ok, None] * u0[1] + l2[ok, None] * u0[2]
        sx = np.clip((st[:, 0] % 1.0) * bw, 0, bw - 1).astype(int)
        sy = np.clip((st[:, 1] % 1.0) * bh, 0, bh - 1).astype(int)
        yy, xx = ys.ravel()[ok], xs.ravel()[ok]
        out[yy, xx] = base[sy, sx]
        hit[yy, xx] = True
    # fill texels no triangle covers with their nearest covered neighbour (bilinear bleed)
    if hit.any() and not hit.all():
        from collections import deque
        q = deque(zip(*np.nonzero(hit)))
        while q:
            y, x = q.popleft()
            for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                v, u = y + dy, x + dx
                if 0 <= v < H and 0 <= u < Wd and not hit[v, u]:
                    hit[v, u] = True
                    out[v, u] = out[y, x]
                    q.append((v, u))
    ov = np.repeat(np.repeat(over_rgba.astype(np.float64) / 255.0, scale, 0), scale, 1)
    a = ov[..., 3:4]
    rgb = ov[..., :3] * a + out[..., :3] * (1 - a)
    alpha = np.maximum(out[..., 3:4], a)
    return (np.concatenate([rgb, alpha], -1) * 255.0 + 0.5).clip(0, 255).astype(np.uint8)


def joint_worlds(model, motion, joints, times):
    """(F, J, 4, 4) ROW-vector worlds (file units) of `joints` under `motion` at `times`."""
    w = posed_worlds(model, motion, times)
    idx = [model['by_name'][n] for n in joints]
    return w[:, idx]


def pose_positions(model, motion, joints, t):
    return joint_worlds(model, motion, joints, [t])[0][:, 3, :3]


def chain_motions(model, motions, joints, tol=0.05):
    """Group consecutive (id, motion) whose last pose equals the next one's first (within `tol`
    file units on every joint): a song's MOT file is its choreography as a run of one-bar
    clips with a few hard cuts. Returns [[id, ...]]."""
    chains, prev_end = [], None
    for mid, mo in motions:
        start = pose_positions(model, mo, joints, 0.0)
        end = pose_positions(model, mo, joints, float(mo['length']))
        if chains and prev_end is not None and np.abs(prev_end - start).max() < tol:
            chains[-1].append(mid)
        else:
            chains.append([mid])
        prev_end = end
    return chains


def clip_bars(length, bpm):
    """Bars of a one-bar-per-clip choreography piece of `length` frames (60 Hz) in a song at
    `bpm`: the k in {1, 2, 3, 4} whose 14400 k / length (the piece's authored tempo) is nearest
    the song's -- the library mixes 80 (180 BPM) .. 206 (70 BPM) frame bars, and a few 288 / 576
    frame pieces are 2 / 4 bars of a 100 BPM song."""
    return min((1, 2, 3, 4), key=lambda k: abs(math.log(14400.0 * k / length / bpm)))


def ssq_tempo(blob):
    """[(bar0, bar1, bpm)] of an SSQ's tempo chunk (little endian; docs/ssq_format.md)."""
    _ln, typ, tps, n = struct.unpack_from('<IHHI', blob, 0)
    if typ != 1:
        raise ValueError('no tempo chunk')
    mo = struct.unpack_from('<%di' % n, blob, 12)
    td = struct.unpack_from('<%di' % n, blob, 12 + 4 * n)
    out = []
    for i in range(1, n):
        dm, dt = mo[i] - mo[i - 1], (td[i] - td[i - 1]) / tps
        if dm > 0 and dt > 0:
            out.append((mo[i - 1] / 4096.0, mo[i] / 4096.0, 240.0 * dm / 4096.0 / dt))
    return out


def dominant_bpm(segments):
    w = {}
    for a, b, bpm in segments:
        w[round(bpm)] = w.get(round(bpm), 0.0) + (b - a)
    return max(w.items(), key=lambda kv: kv[1])[0] if w else 120.0


def chunk_take(pieces, target=8.0, min_bars=4.0, max_bars=12.0):
    """Cut a take [(id, bars)] into runs of ~`target` bars at piece boundaries (World plays a
    clip for its length minus 1.5 s, stock clips are ~10 bars): returns [[index, ...]]. A tail
    shorter than `min_bars` joins the previous run when that stays under `max_bars`."""
    runs, cur, acc = [], [], 0.0
    for k, (_id, bars) in enumerate(pieces):
        cur.append(k)
        acc += bars
        if acc >= target:
            runs.append(cur)
            cur, acc = [], 0.0
    if cur:
        tail = sum(pieces[k][1] for k in cur)
        if runs and (tail < min_bars) and sum(pieces[k][1] for k in runs[-1]) + tail <= max_bars:
            runs[-1].extend(cur)
        else:
            runs.append(cur)
    return runs


def deal_rotating(items, hands, per_hand, seed=0):
    """Deal `per_hand` items to each of `hands` hands from a seeded shuffle of `items`, hand h
    starting where h - 1 stopped (wrapping): every item is used, each about hands * per_hand /
    len(items) times, and no hand holds one twice (when per_hand <= len(items))."""
    import random
    order = list(items)
    random.Random(seed).shuffle(order)
    out, k = [], 0
    for _h in range(hands):
        take = [order[(k + i) % len(order)] for i in range(min(per_hand, len(order)))]
        out.append(take)
        k = (k + len(take)) % max(1, len(order))
    return out


# ---------------------------------------------------------------------------
# stages (port_stage_hottest2.py)
# ---------------------------------------------------------------------------
# stage/STG<nnn>.bin = { /#0: named members -- DRAW_* (the stage, floor, set) / BG_* (the
# backdrop) / OBJ[AB]_[NZS]_<name>_* (props) as {.zmb, .tpl, .zab}, and COL_*.zmb + .zab (the
# layout: OBJSET_<name>_<nn> nodes instance the prop <name> -- their own meshes are the props'
# swept cull hulls, never drawn --, EFF_* effect emitters, LIGPOS_* / LIGTAR_* lights),
# /#1: the stage's camera shots }. STG<nnn>_S.bin is a lighter copy (split-screen) and is not
# ported. Every model's .zab is one loop of its own length; a mesh node is drawn at its node world.
OBJ_PREFIX = re.compile(r'^OBJ[AB]_[NZS]_', re.I)


def _stem(name):
    return name.rsplit('.', 1)[0]


def stage_sources(blob):
    """dict(models [dict(stem, kind, model, textures, motion)], col (model, motion) or None,
    cams [(name, cam)])."""
    named = {}
    cams = []
    for p, n, o, s, k in walk(blob):
        if k == 'cam':
            cams.append((p, parse_cam(blob[o:o + s])))
        if n and p.startswith('/#0/'):
            named.setdefault(n, (k, blob[o:o + s]))
    zabs = {_stem(n): b for n, (k, b) in named.items() if k == 'zab'}

    def zab_for(stem):
        if stem in zabs:
            return parse_zab(zabs[stem])
        head = stem.rsplit('_', 1)[0]
        for z in sorted(zabs):
            if z.rsplit('_', 1)[0] == head:
                return parse_zab(zabs[z])
        return None

    models, col = [], None
    for n, (k, b) in sorted(named.items()):
        if k != 'zmb':
            continue
        stem = _stem(n)
        m = parse_zmb(b)
        mo = zab_for(stem)
        if stem.upper().startswith('COL'):
            col = (m, mo)
            continue
        tpl = named.get(stem + '.tpl')
        tex = tpl_images(tpl[1]) if tpl and tpl[0] == 'tpl' else []
        kind = 'bg' if stem.upper().startswith('BG') else 'obj' if stem.upper().startswith('OBJ') else 'draw'
        models.append(dict(stem=stem, kind=kind, model=m, textures=tex, motion=mo))
    order = {'bg': 0, 'draw': 1, 'obj': 2}
    models.sort(key=lambda e: (order[e['kind']], e['stem']))
    return dict(models=models, col=col, cams=cams)


def instance_key(objset_name):
    """`OBJSET_STG27_board01_03` -> `STG27_board01`."""
    return re.sub(r'_\d+$', '', objset_name[len('OBJSET_'):])


def prop_matches(stem, key):
    rest = OBJ_PREFIX.sub('', stem)
    return rest.lower() == key.lower() or rest.lower().startswith(key.lower() + '_')


def stage_instances(src):
    """{prop stem: [OBJSET node name]} -- every COL `OBJSET_<key>_<nn>` node matched to the prop
    whose name (after `OBJ<A|B>_<N|Z|S>_`) is <key> (the longest key wins)."""
    out = {e['stem']: [] for e in src['models'] if e['kind'] == 'obj'}
    if not src['col']:
        return out
    col = src['col'][0]
    for nd in col['nodes']:
        if not nd['name'].startswith('OBJSET_'):
            continue
        key = instance_key(nd['name'])
        hits = [s for s in out if prop_matches(s, key)]
        if hits:
            out[max(hits, key=len)].append(nd['name'])
    return out


def material_uv_keys(model, material):
    """(n, 3) [seconds, u, v] UV-offset keys of a version-3 material (+0x38 count, +0x3C
    offset; 16-byte keys {f32 time, u, v, u8 flags[4]}) and the step flags (n, 2) (flag pair
    byte 0 == 0xFF: hold until the next key), or None. +0x20 / +0x24 hold the mean per-frame
    speed of the same motion."""
    if model['material_version'] != 3.0:
        return None
    w = material['words']
    n, ptr = w[14], w[15]
    if not n or not ptr or n > 4096:
        return None
    return n, ptr


def uv_keys(blob, n, ptr):
    raw = np.frombuffer(blob, '>f4', n * 4, ptr).reshape(n, 4).astype(np.float64)
    flags = np.frombuffer(blob, np.uint8, n * 16, ptr).reshape(n, 16)[:, 12:16]
    return raw[:, :3], flags


def sample_uv(keys, flags, t):
    """(F, 2) UV offset at times `t` (seconds), linear or step per axis (flags)."""
    t = np.asarray(t, dtype=np.float64)
    out = np.empty((len(t), 2))
    kt = keys[:, 0]
    for ax in range(2):
        v = keys[:, 1 + ax]
        step = flags[:, 2 * ax] == 0xFF
        i = np.clip(np.searchsorted(kt, t, side='right') - 1, 0, len(kt) - 1)
        lin = np.interp(t, kt, v)
        out[:, ax] = np.where(step[i], v[i], lin)
    return out


# ---------------------------------------------------------------------------
# cameras
# ---------------------------------------------------------------------------
CAM_TRACKS = ('pos', 'rot', 'fov', 'aim', 'near', 'far')
CAM_KEY_FLOATS = {'pos': 4, 'rot': 5, 'fov': 2, 'aim': 4, 'near': 2, 'far': 2}


def parse_cam(b):
    """{track: (n, 1 + values) array of keys, time in seconds}."""
    if b[0x34:0x40] != CAM_SIGNATURE:
        raise ValueError('not a camera')
    out = {}
    for i, name in enumerate(CAM_TRACKS):
        n, ptr = struct.unpack_from('>II', b, 4 + 8 * i)
        k = CAM_KEY_FLOATS[name]
        out[name] = np.frombuffer(b, '>f4', n * k, ptr).reshape(n, k).astype(np.float64) if n else np.zeros((0, k))
    return out


def cam_length(cam):
    return max(float(v[-1, 0]) for v in cam.values() if len(v))


def cam_samples(cam, times):
    """{track: (F, values)} linearly interpolated at `times` (seconds)."""
    t = np.asarray(times, dtype=np.float64)
    out = {}
    for name, keys in cam.items():
        if not len(keys):
            continue
        if name == 'rot':
            out[name] = _slerp_series(keys[:, 0], keys[:, 1:], t)
        else:
            out[name] = _interp(keys[:, 0], keys[:, 1:], t)
    return out


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def _tilde(p):
    home = os.path.expanduser('~')
    return '~' + p[len(home):] if p.startswith(home) else p


def cmd_ls(a):
    b = open(a.file, 'rb').read()
    for p, _n, o, s, k in walk(b):
        print('%-60s %-8s %8x %8x' % (p, k, o, s))


def _find(b, member):
    for p, _n, o, s, k in walk(b):
        if member is None or p == member or p.endswith('/' + member):
            if k in ('zmb', 'zab', 'cam'):
                yield p, b[o:o + s], k


def cmd_info(a):
    b = open(a.file, 'rb').read()
    for p, blob, k in _find(b, a.member):
        print('==', p, k)
        if k == 'zmb':
            m = parse_zmb(blob)
            print('textures', m['textures'])
            for i, mt in enumerate(m['materials']):
                print('  mat %2d flags %s tex %s layers %d frames %s' % (i, mt['flags'].hex(), mt['textures'],
                                                                        len(mt['layer_offsets']), mt['frames']))
            for nd in m['nodes']:
                subs = ' '.join('m%d:%dv/%dp' % (s['material'], len(s['pos']), len(s['packets'])) for s in nd['submeshes'])
                print('  %3d %-36s parent %3d %s' % (nd['index'], nd['name'], nd['parent'], subs))
        elif k == 'zab':
            mo = parse_zab(blob)
            print('length', mo['length'], 'bones', len(mo['order']))
        else:
            c = parse_cam(blob)
            print({n: len(v) for n, v in c.items()}, 'length %.2f s' % cam_length(c))


def survey(paths):
    """Parse every archive under `paths` (ZMB / ZAB / camera members, TPL images). Returns
    (counts, problems)."""
    counts, problems = {}, []
    for root in paths:
        for dirpath, _dirs, names in os.walk(root):
            for nm in sorted(names):
                path = os.path.join(dirpath, nm)
                try:
                    b = open(path, 'rb').read()
                except OSError:
                    continue
                if not is_archive(b):
                    continue
                try:
                    for p, _n, o, s, k in walk(b):
                        blob = b[o:o + s]
                        if k == 'zmb':
                            m = parse_zmb(blob)
                            for nd in m['nodes']:
                                for sm in nd['submeshes']:
                                    for pk in sm['packets']:
                                        if len(pk['pos']) and pk['pos'].max() >= len(sm['pos']):
                                            raise ValueError('%s: position index out of range' % p)
                        elif k == 'zab':
                            parse_zab(blob)
                        elif k == 'cam':
                            parse_cam(blob)
                        elif k == 'tpl':
                            W.parse_tpl(blob)
                        counts[k] = counts.get(k, 0) + 1
                except (ValueError, IndexError, KeyError, struct.error) as e:
                    problems.append((path, repr(e)))
    return counts, problems


def cmd_survey(a):
    counts, problems = survey(a.dirs)
    for p, e in problems:
        print('PROBLEM', _tilde(p), e)
    print(counts, '%d problems' % len(problems))
    return 1 if problems else 0


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('ls')
    p.add_argument('file')
    p.set_defaults(fn=cmd_ls)
    p = sub.add_parser('info')
    p.add_argument('file')
    p.add_argument('member', nargs='?')
    p.set_defaults(fn=cmd_info)
    p = sub.add_parser('survey')
    p.add_argument('dirs', nargs='+')
    p.set_defaults(fn=cmd_survey)
    a = ap.parse_args(argv)
    return a.fn(a) or 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
