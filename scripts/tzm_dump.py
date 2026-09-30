#!/usr/bin/env python3
"""Reference decoders for the `.TZM` 3D model packs of the PS2 DDR SuperNova engine (SuperNova,
SuperNova 2, X, X2: the polygon dancers, the dance routines, the stages and their cameras), and
the conversion math the DDR World port uses (tools/blender_ddr_addon/examples/
port_character_supernova.py). Formats and RE: docs/ps2_ddr_filedata_research.md §7.4.

A TZM is an XSI export ("globalSRT" roots, "DefaultLib.Material"): a directory of named chunks
on 0x800 sectors, most of them a small header followed by a TGCD stream
(extract_ps2_ddr_data.tgcd_decode). In the decoded data, offsets are relative to the chunk
start with the chunk's own header in place (MODEL 0x30, MOTION 8), so this module subtracts
that header size when it indexes the decoded bytes.

  IMAGELIST      {u32 hash, u32 n} then n x char[0x50]: the texture chunk names
  <texture>      {u32 hash, char name[0x40]}, u16 w, h at 0x54, {u32 payload, clut bytes, stream
                 bytes, row bytes, 1} at 0x5C, a LINEAR RGBA32 CLUT at 0x70 (256 or 16 colours;
                 alpha 0x80 = opaque), then a TGCD stream -> 8 bpp or 4 bpp indices
  MODEL          {u32 hash, u32 1, (u32 count, u32 offset) x 5} + TGCD. Sections: 0 root
                 (name only), 1 nodes (objects), 2 object -> first mesh, 3 meshes, 4 bones.
                 Node record (0xD0): char name[0x40], 0x10 zero, f32 T[4] R[4] S[4] (the file's
                 pose, local to the parent), f32 T2[4] R2[4] S2[4] (the bind pose, GLOBAL), then
                 i32 links[8]: bones {parent, first_child, next_sibling, prev_sibling, 0, 3, 0,
                 0}; objects {mesh_slot, 0x80000000, parent, first_child, next_sibling,
                 prev_sibling, 0, 0} (mesh_slot -> the section-2 table, -1 = no mesh). A skin's
                 node lists start with their `globalSRT` root, a stage's objects with the
                 blend-layer roots `add` / `glo` / `dec` / `ble`. Rotations are XYZ Euler radians
                 (M = Rz . Ry . Rx on column vectors); a joint's X axis runs along the bone.
                 Mesh record: char material[0x18], 0x38 zero, i32 hdr[24] = {-1, format,
                 nverts, stride, nbytes, 4, nverts-2, ntri_or_index, a, b, palette[9] (bone
                 indices, -1 = empty), palette_count, next_mesh, 0, 0, 0}, the vertices, and
                 (skinned meshes) 2 x stride zero bytes. A vertex is `stride/16` float4 rows: position (w 0); [weights
                 w0 w1 w2 + 4 bone PALETTE indices as bytes, format bit 0x08]; normal;
                 [colour RGBA on a 0..128 scale, bit 0x40]; uv (u, v, 1, flags). The stream is
                 triangle strips: a vertex kicks the triangle (i-2, i-1, i) unless its uv flags
                 carry 0x8000 (the PS2 ADC bit); every strip after the first starts with two
                 such vertices. Facing is not consistent (the GS does not cull).
  MOTION         {u32 hash, u32 nrecords} + TGCD. nrecords headers of 0x7C: char name[0x50],
                 u32 ntracks, s32 first, s32 last (60 Hz scene frames), s32 a, s32 b, u32 0,
                 f32 fps, f32 fps/60, u32 table+8, u32 size, u32 1. Each record's track table
                 (u32 offsets, relative to the table) and tracks follow the headers, in record
                 order. Track: char name[0x50], u32 kind, u32 n, u32 flag (2 = a single static
                 key), u32 nbytes, u32 nkeys, keys. Character clips: kind 2003 = local rotation
                 quaternion (x, y, z, w), 2004 = local translation (x, y, z, 0), one key per
                 1/fps at fps = 30 (60 Hz scene frames first, first+2, ...); the Hip has both,
                 every other joint only 2003, `globalSRT` a static identity. Stage / camera
                 clips use kinds 0..4 for node SRT (10 floats S Q T, 7 = Q T, 4 = T), 5 / 6 =
                 camera position / interest (x, y, z, 0), 7 = horizontal FOV in radians, 8 =
                 roll, and material tracks named after the material: 500 + 100 * texture_stage
                 + {0 scale, 1 rotation, 3 translation} with `n` = 8 / 9 / 10 for the x / y / z
                 component, 504.. = the stage's RGBA colour (`n` 8..11), 1302 = the glow
                 strength (`n` 7). Key layout by `flag`: 0 = one value per record frame, 2 =
                 one static value, 3 = an XSI fcurve of 7-float keys {time, left handle time,
                 right handle time, u32 interpolation (1 linear, 2 Bezier), value, left handle
                 value, right handle value} (fcurve_value). A stage pack has a `stageNNN` and a
                 `cameraNNN` record. The MATERIALLIST chunk maps material names to texture
                 chunks (parse_materiallist).

Character skinning (verified on AFRO / FF_HH_01): v_world = sum_i w_i . v . Object . Bind_i^-1 .
World_i with Bind_i = (R2_i, T2_i) global, Object = the owning object's (T, R, S), and World_i
composed down the hierarchy from the clip's local quaternion + the file's local T (the Hip's
from its 2004 track). The `SCALE` node under `globalSRT` (BABYLON, the `DDR_cspigs*` stand-ins)
carries a uniform S in its pose and no bind: it is a whole-character scale.

Usage:
    tzm_dump.py info    <file.tzm>                       # chunks, nodes, meshes, tracks
    tzm_dump.py png     <file.tzm> <out dir>             # every texture as PNG
    tzm_dump.py preview <skin.TZM> <out.png> [--motion <clip.TZM> --frame N]
    tzm_dump.py survey  <dir>...                         # parse every TZM under the dirs

Import-safe: `from tzm_dump import parse_tzm, parse_model, parse_motion, decode_texture, ...`.
Needs numpy (array math); extract_ps2_ddr_data.py beside it (TGCD, PNG).
"""
import argparse
import glob
import math
import os
import struct
import sys
from typing import Any

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import extract_ps2_ddr_data as P  # noqa: E402  (tgcd_decode, write_png)

TZM_MAGIC = 0xA094F23B
DIR_ENTRY = 0x5C
SECTOR = 0x800
NODE_SIZE = 0xD0
MESH_HEADER = 0x50 + 0x60  # material name + params, then the 24 header ints
MODEL_HEADER = 0x30  # the MODEL chunk header the decoded offsets count from
MOTION_HEADER = 8
MOTION_RECORD = 0x7C
TRACK_NAME = 0x50
FMT_WEIGHTS = 0x08
FMT_COLOUR = 0x40
ADC = 0x8000
KIND_ROTATION = 2003
KIND_TRANSLATION = 2004
KIND_SRT = 3
# camera records (`cameraNNN` / `stageNNN_cam`, `stage_chara_camera.TZM`)
KIND_CAM_SRT = 4
KIND_CAM_POSITION = 5
KIND_CAM_INTEREST = 6
KIND_CAM_FOV = 7
KIND_CAM_ROLL = 8
# material tracks: 500 + 100 * texture_stage + component (SLPM_666.09 FUN_00149a60)
KIND_TEX_SCALE = 500
KIND_TEX_ROTATION = 501
KIND_TEX_TRANSLATION = 503
KIND_TEX_COLOUR = 504
KIND_GLOW = 1302
FCURVE_LINEAR = 1
FCURVE_BEZIER = 2
FLAG_STATIC = 2
FLAG_FCURVE = 3
# XSI's default camera field of view, HORIZONTAL: 53.638 deg -- the value on 244 of the 262
# SuperNova cameras, and exactly the horizontal angle of DDR A3's stock Maya cameras
# (41.53 deg vertical at film aspect 1.333 -> 2 atan(tan(20.765 deg) * 4/3) = 53.638 deg).
XSI_DEFAULT_FOV = 0.9361597
STANDARD_BONES = ['Hip', 'Spine', 'Spine1', 'Spine2', 'RightShoulder', 'RightArm', 'RightForeArm', 'RightHand',
                  'LeftShoulder', 'LeftArm', 'LeftForeArm', 'LeftHand', 'Neck', 'Head', 'RightUpLeg', 'RightLeg',
                  'RightFoot', 'RightToes', 'LeftUpLeg', 'LeftLeg', 'LeftFoot', 'LeftToes']
SCALE_NODE = 'SCALE'
ROOT_NODE = 'globalSRT'


def _cstr(data, off, size):
    return bytes(data[off:off + size]).split(b'\0')[0].decode('latin1')


# ---------------------------------------------------------------------------
# container
# ---------------------------------------------------------------------------
def parse_tzm(data) -> list[tuple[str, bytes]]:
    """[(chunk name, chunk bytes)] in directory order."""
    if len(data) < 12:
        raise ValueError('too short for a TZM')
    magic, _version, count = struct.unpack_from('<III', data, 0)
    if magic != TZM_MAGIC:
        raise ValueError('not a TZM (magic 0x%08X)' % magic)
    if 12 + count * DIR_ENTRY > len(data):
        raise ValueError('TZM directory of %d entries overruns the file' % count)
    chunks = []
    for i in range(count):
        o = 12 + i * DIR_ENTRY
        name = _cstr(data, o, 0x40)
        sector, _sectors, size = struct.unpack_from('<III', data, o + 0x50)
        start = sector * SECTOR
        if start + size > len(data):
            raise ValueError('chunk %r (%d bytes at sector %d) overruns the file' % (name, size, sector))
        chunks.append((name, data[start:start + size]))
    return chunks


def chunk_payload(chunk, limit=0x480):
    """(header bytes before the TGCD stream, decoded stream), or (chunk, None) when the chunk
    carries no TGCD in its first `limit` bytes (a few test packs store raw data)."""
    i = chunk.find(P.TGCD_MAGIC, 0, limit)
    if i < 0:
        return chunk, None
    return chunk[:i], P.tgcd_decode(chunk[i:])


def load_tzm(path):
    return parse_tzm(open(path, 'rb').read())


def parse_imagelist(chunk):
    n = struct.unpack_from('<I', chunk, 4)[0]
    return [_cstr(chunk, 8 + 0x50 * i, 0x50) for i in range(n)]


MATERIAL_RECORD_HEADER = 0x10


def parse_materiallist(chunk) -> dict[str, dict[str, Any]]:
    """{material name: dict(textures=[chunk names], diffuse (4 floats), index)} from a MATERIALLIST
    chunk: {u32 hash, u32 count, 8 x 0xCD} then `count` records of (len - 0x10) / count bytes:
    char name[0x40], the first texture chunk name at +0x50, RGBA diffuse at +0xA0, u32 texture
    count at +0x350, RGBA ambient at +0x360, the second (glow) texture name at +0x370."""
    count = struct.unpack_from('<I', chunk, 4)[0]
    out = {}
    if not count or len(chunk) <= MATERIAL_RECORD_HEADER:
        return out
    stride = (len(chunk) - MATERIAL_RECORD_HEADER) // count
    for i in range(count):
        r = MATERIAL_RECORD_HEADER + i * stride
        name = _cstr(chunk, r, 0x40)
        tex = [_cstr(chunk, r + o, 0x40) for o in (0x50, 0x370) if r + o + 0x40 <= len(chunk)]
        diffuse = struct.unpack_from('<4f', chunk, r + 0xA0) if r + 0xB0 <= len(chunk) else (1.0,) * 4
        out[name] = dict(index=i, textures=[t for t in tex if t], diffuse=diffuse)
    return out


def material_for(materials, mesh_material):
    """The MATERIALLIST record a mesh's 0x18-byte material name refers to (the mesh field
    truncates long names: `DefaultLib.add_tex1_uvan` is `DefaultLib.add_tex1_uvani`)."""
    if mesh_material in materials:
        return materials[mesh_material]
    hits = [m for n, m in materials.items() if n.startswith(mesh_material)]
    return hits[0] if len(hits) == 1 else None


# ---------------------------------------------------------------------------
# textures
# ---------------------------------------------------------------------------
def decode_texture(chunk) -> dict[str, Any]:
    """dict(name, width, height, bpp, clut (n x 4 uint8, alpha as stored: 0x80 = opaque),
    indices (h x w uint8), rgba (h x w x 4 uint8 with alpha doubled to 0..255)).
    Header: {u32 hash, char name[0x40]}, u16 w, h at 0x54, at 0x5C {u32 payload, u32 clut
    bytes, u32 stream bytes, u32 row bytes, u32 1}, the CLUT at 0x70, the TGCD after it."""
    name = _cstr(chunk, 4, 0x40)
    w, h = struct.unpack_from('<HH', chunk, 0x54)
    _payload, clut_bytes, _stream, row_bytes, _one = struct.unpack_from('<5I', chunk, 0x5C)
    if clut_bytes not in (0x40, 0x400) or 0x70 + clut_bytes > len(chunk):
        raise ValueError('texture %r: %d-byte CLUT' % (name, clut_bytes))
    clut = np.frombuffer(chunk[0x70:0x70 + clut_bytes], np.uint8).reshape(clut_bytes // 4, 4)
    _hdr, dec = chunk_payload(chunk, 0x70 + clut_bytes + 4)
    bpp = 4 if clut_bytes == 0x40 else 8
    if dec is None or len(dec) < w * h * bpp // 8 or row_bytes * 8 != w * bpp:
        raise ValueError('texture %r: no %dx%d %d bpp index stream' % (name, w, h, bpp))
    raw = np.frombuffer(dec, np.uint8)[:w * h * bpp // 8]
    if bpp == 4:  # two pixels per byte, low nibble first (PSMT4)
        idx = np.stack([raw & 0x0F, raw >> 4], axis=1).reshape(h, w)
    else:
        idx = raw.reshape(h, w)
    rgba = clut[idx].copy()
    rgba[..., 3] = np.minimum(rgba[..., 3].astype(np.uint16) * 2, 255).astype(np.uint8)
    return dict(name=name, width=w, height=h, bpp=bpp, clut=clut, indices=idx, rgba=rgba)


def textures_of(chunks) -> dict[str, dict[str, Any]]:
    """{chunk name: decode_texture(...)} for the chunks the IMAGELIST names."""
    d = dict(chunks)
    out = {}
    for n in parse_imagelist(d['IMAGELIST']) if 'IMAGELIST' in d else []:
        if n in d:
            out[n] = decode_texture(d[n])
    return out


# ---------------------------------------------------------------------------
# MODEL
# ---------------------------------------------------------------------------
def _node(dec, off, kind) -> dict[str, Any]:
    """`kind` 'bone': links = {parent, first_child, next_sibling, prev_sibling, 0, 3, 0, 0};
    'object': links = {mesh_slot (-1 = none), 0x80000000, parent, first_child, next_sibling,
    prev_sibling, 0, 0} -- `mesh_slot` indexes the object -> first mesh table."""
    T, R, S, T2, R2, S2 = (np.array(struct.unpack_from('<3f', dec, off + o), dtype=float)
                           for o in (0x50, 0x60, 0x70, 0x80, 0x90, 0xA0))
    links = struct.unpack_from('<8i', dec, off + 0xB0)
    tree = links[2:6] if kind == 'object' else links[0:4]
    return dict(name=_cstr(dec, off, 0x40), T=T, R=R, S=S, T2=T2, R2=R2, S2=S2, kind=kind,
                mesh_slot=links[0] if kind == 'object' else -1,
                parent=tree[0], first_child=tree[1], next_sibling=tree[2], prev_sibling=tree[3], links=links)


def _nodes(dec, count, offset, kind) -> list[dict[str, Any]]:
    base = offset - MODEL_HEADER
    if base < 0 or base + count * NODE_SIZE > len(dec):
        raise ValueError('node section (%d at 0x%X) overruns the data' % (count, offset))
    return [_node(dec, base + k * NODE_SIZE, kind) for k in range(count)]


def strip_triangles(flags):
    """Triangle index triples of one ADC-flagged strip stream (see the module doc)."""
    kick = (np.asarray(flags) & ADC) == 0
    kick[:2] = False
    i = np.nonzero(kick)[0]
    return np.stack([i - 2, i - 1, i], axis=1) if len(i) else np.zeros((0, 3), np.int64)


def _mesh(dec, off) -> dict[str, Any]:
    material = _cstr(dec, off, 0x18)
    ints = struct.unpack_from('<24i', dec, off + 0x50)
    fmt, nv, stride, nbytes = ints[1], ints[2], ints[3], ints[4]
    if stride % 16 or nbytes != nv * stride:
        raise ValueError('mesh %r: %d vertices x %d bytes != %d' % (material, nv, stride, nbytes))
    rows = stride // 16
    expect = 3 + bool(fmt & FMT_WEIGHTS) + bool(fmt & FMT_COLOUR)
    if rows != expect:
        raise ValueError('mesh %r: format 0x%X wants %d rows, stride gives %d' % (material, fmt, expect, rows))
    vb = off + MESH_HEADER
    trailer = 2 * stride if fmt & FMT_WEIGHTS else 0  # skinned meshes carry two zero vertices of scratch
    if vb + nbytes + trailer > len(dec):
        raise ValueError('mesh %r: vertex data overruns the model' % material)
    F = np.frombuffer(dec[vb:vb + nbytes], '<f4').reshape(nv, rows, 4).astype(float)
    U = np.frombuffer(dec[vb:vb + nbytes], '<u4').reshape(nv, rows, 4)
    r = 0
    pos = F[:, r, :3]
    r += 1
    weights = np.zeros((nv, 0))
    bone_slots = np.zeros((nv, 4), np.int64)
    skinned = bool(fmt & FMT_WEIGHTS)
    if skinned:
        weights = F[:, r, :3]
        packed = U[:, r, 3]
        bone_slots = np.stack([(packed >> (8 * s)) & 0xFF for s in range(4)], axis=1).astype(np.int64)
        r += 1
    normal = F[:, r, :3]
    r += 1
    colour = None
    if fmt & FMT_COLOUR:
        colour = np.clip(F[:, r, :4] / 128.0, 0.0, 1.0)
        r += 1
    uv = F[:, r, :2]
    flags = U[:, r, 3]
    palette = [b for b in ints[10:19] if b >= 0][:max(0, ints[19])]
    if skinned:
        used = bone_slots[:, :3][weights > 0]
        if len(used) and int(used.max()) >= len(palette):
            raise ValueError('mesh %r: bone slot beyond its %d-entry palette' % (material, len(palette)))
    return dict(material=material, format=fmt, count=nv, stride=stride, header=ints, palette=palette,
                next_mesh=ints[20], positions=pos, normals=normal, uv=uv, flags=flags, colours=colour,
                weights=weights if skinned else None, bone_slots=bone_slots if skinned else None,
                triangles=strip_triangles(flags), end=vb + nbytes + trailer)


def parse_model(chunk) -> dict[str, Any]:
    """dict(sections, root, objects, object_meshes, meshes, bones): see the module doc.
    `objects` / `bones` are node lists (a skin's start with `globalSRT`; a stage's objects
    start with their blend-layer roots `add` / `glo` / `dec` / `ble`); `object_meshes[slot]` is
    the first mesh of the object whose `mesh_slot` is `slot` (follow `next_mesh`);
    `mesh_object[m]` = the owning object's index."""
    hdr, dec = chunk_payload(chunk, 0x40)
    if dec is None:
        raise ValueError('MODEL chunk is not TGCD wrapped')
    hv = struct.unpack_from('<12I', hdr, 0)
    sections = [(hv[2 + 2 * i], hv[3 + 2 * i]) for i in range(5)]
    root = _cstr(dec, sections[0][1] - MODEL_HEADER, 0x40)
    objects = _nodes(dec, *sections[1], 'object')
    n_om, off_om = sections[2]
    object_meshes = [struct.unpack_from('<i', dec, off_om - MODEL_HEADER + 16 * k)[0] for k in range(n_om)]
    n_mesh, off_tab = sections[3]
    tab = off_tab - MODEL_HEADER
    meshes = []
    if n_mesh:
        offs = struct.unpack_from('<%dI' % n_mesh, dec, tab)
        for o in offs:
            meshes.append(_mesh(dec, tab + o))
    bones = _nodes(dec, *sections[4], 'bone') if sections[4][0] else []
    mesh_object = {}
    for oi, o in enumerate(objects):
        slot = o['mesh_slot']
        if not 0 <= slot < len(object_meshes):
            continue
        m = object_meshes[slot]
        guard = 0
        while 0 <= m < len(meshes) and guard <= len(meshes):
            mesh_object[m] = oi
            m = meshes[m]['next_mesh']
            guard += 1
    return dict(sections=sections, root=root, objects=objects, object_meshes=object_meshes, meshes=meshes,
                bones=bones, mesh_object=mesh_object, size=len(dec))


# ---------------------------------------------------------------------------
# MOTION
# ---------------------------------------------------------------------------
def parse_motion(chunk) -> list[dict[str, Any]]:
    """[record] with record = dict(name, ntracks, first, last, a, b, fps, tracks) and
    track = dict(name, kind, n, flag, keys (nkeys x floats))."""
    hdr, dec = chunk_payload(chunk, 0x40)
    if dec is None:
        raise ValueError('MOTION chunk is not TGCD wrapped')
    nrec = struct.unpack_from('<I', hdr, 4)[0] if len(hdr) >= 8 else 1
    records = []
    for k in range(nrec):
        h = k * MOTION_RECORD
        if h + MOTION_RECORD > len(dec):
            raise ValueError('motion record %d header overruns the data' % k)
        ntracks, first, last, a, b, _zero = struct.unpack_from('<I5i', dec, h + 0x50)
        fps, step = struct.unpack_from('<2f', dec, h + 0x68)
        table_plus, size, _one = struct.unpack_from('<3I', dec, h + 0x70)
        table = table_plus - MOTION_HEADER
        if table < 0 or table + 4 * ntracks > len(dec) or table + size > len(dec):
            raise ValueError('motion record %r: track table at 0x%X overruns the data' % (_cstr(dec, h, TRACK_NAME), table))
        offs = struct.unpack_from('<%dI' % ntracks, dec, table)
        tracks = []
        for o in offs:
            t = table + o
            kind, n, flag, nbytes, nkeys = struct.unpack_from('<5I', dec, t + TRACK_NAME)
            kb = t + TRACK_NAME + 0x14
            if nkeys == 0 or nbytes % (4 * nkeys) or kb + nbytes > len(dec):
                raise ValueError('track %r: %d bytes for %d keys' % (_cstr(dec, t, TRACK_NAME), nbytes, nkeys))
            keys = np.frombuffer(dec[kb:kb + nbytes], '<f4').reshape(nkeys, nbytes // (4 * nkeys)).astype(float)
            tracks.append(dict(name=_cstr(dec, t, TRACK_NAME), kind=kind, n=n, flag=flag, keys=keys))
        records.append(dict(name=_cstr(dec, h, TRACK_NAME), ntracks=ntracks, first=first, last=last, a=a, b=b,
                            fps=fps, step=step, size=size, tracks=tracks))
    return records


def clip_tracks(record):
    """({bone: rotation keys}, {bone: translation keys}) of a character clip."""
    rot = {t['name']: t['keys'] for t in record['tracks'] if t['kind'] == KIND_ROTATION}
    trn = {t['name']: t['keys'] for t in record['tracks'] if t['kind'] == KIND_TRANSLATION}
    return rot, trn


def clip_frames(record):
    """Number of keys the clip's animated tracks carry (1 for a static clip)."""
    return max((len(t['keys']) for t in record['tracks'] if t['flag'] != 2), default=1)


def srt_key(key):
    """(scale, quaternion xyzw, translation) of one stage / camera key: 10 floats = S(3) Q(4)
    T(3), 7 = Q(4) T(3) (scale 1), 4 = T(3) + 0 (identity rotation)."""
    k = list(key)
    if len(k) >= 10:
        return k[0:3], k[3:7], k[7:10]
    if len(k) >= 7:
        return [1.0, 1.0, 1.0], k[0:4], k[4:7]
    return [1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0], k[0:3]


def object_tracks(record):
    """{object name: SRT keys array} for the node tracks of a stage record (kinds 0..4)."""
    return {t['name']: t['keys'] for t in record['tracks'] if t['kind'] <= 4 and t['keys'].shape[1] in (4, 7, 10)}


def animated_names(record, tol=1e-5):
    """Names whose node track actually changes over the clip."""
    out = set()
    for name, keys in object_tracks(record).items():
        if len(keys) > 1 and float(np.abs(keys - keys[0]).max()) > tol:
            out.add(name)
    return out


# ---------------------------------------------------------------------------
# pose math (column-vector 4x4 affines internally; `_row` transposes for the add-on)
# ---------------------------------------------------------------------------
def rot_x(a):
    c, s = math.cos(a), math.sin(a)
    return np.array([[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]])


def rot_y(a):
    c, s = math.cos(a), math.sin(a)
    return np.array([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]])


def rot_z(a):
    c, s = math.cos(a), math.sin(a)
    return np.array([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]])


def euler_xyz(r):
    """XSI / Maya XYZ order: rotate about X first, then Y, then Z (column vectors)."""
    return rot_z(r[2]) @ rot_y(r[1]) @ rot_x(r[0])


def quat_mat(q):
    x, y, z, w = q[:4]
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def affine(m3, t, s=None):
    a = np.eye(4)
    a[:3, :3] = m3 if s is None else m3 @ np.diag(s)
    a[:3, 3] = t
    return a


def node_local(node, rotation=None, translation=None):
    """The node's local matrix from its file pose, with the clip's rotation matrix / translation
    substituted when given. Its S is kept (the SCALE node)."""
    m3 = euler_xyz(node['R']) if rotation is None else rotation
    t = node['T'] if translation is None else translation
    return affine(m3, t, node['S'])


def bind_matrix(node):
    """The node's global bind (T2, R2); nodes without a bind (S2 = 0) get the identity."""
    if not np.any(node['S2']):
        return np.eye(4)
    return affine(euler_xyz(node['R2']), node['T2'])


def world_matrices(nodes, locals_):
    """Compose `locals_[i]` down the parent links of `nodes`."""
    W = [None] * len(nodes)
    for i, n in enumerate(nodes):
        p = n['parent']
        W[i] = locals_[i] if p < 0 else W[p] @ locals_[i]
    return W


def rest_worlds(bones):
    return world_matrices(bones, [node_local(b) for b in bones])


def clip_worlds(bones, record, frame):
    """World matrices of `bones` at key `frame` of a character clip record (tracks matched by
    bone name; joints without a track keep the file pose)."""
    rot, trn = clip_tracks(record)
    locals_ = []
    for b in bones:
        n = b['name']
        r = quat_mat(rot[n][frame % len(rot[n])]) if n in rot else None
        t = trn[n][frame % len(trn[n])][:3] if n in trn else None
        locals_.append(node_local(b, r, t))
    return world_matrices(bones, locals_)


def object_matrix(node, unit_scale=False):
    return affine(euler_xyz(node['R']), node['T'], None if unit_scale else node['S'])


def object_worlds(model, unit_scale=False):
    """Every object's world matrix from its file pose down the object links (`unit_scale`:
    every node's scale taken as 1 -- the rigid frames, for binds)."""
    return world_matrices(model['objects'], [object_matrix(o, unit_scale) for o in model['objects']])


def object_worlds_at(model, record, frame, unit_scale=False):
    """Every object's world matrix at key `frame` of a stage record: tracked objects take their
    SRT key (matched by name), the others keep the file pose."""
    tracks = object_tracks(record)
    locals_ = []
    for o in model['objects']:
        keys = tracks.get(o['name'])
        if keys is None:
            locals_.append(object_matrix(o, unit_scale))
        else:
            s, q, t = srt_key(keys[frame % len(keys)])
            locals_.append(affine(quat_mat(q), t, None if unit_scale else s))
    return world_matrices(model['objects'], locals_)


def object_chain(model, index):
    """Object indices from the root down to `index`."""
    chain = []
    while index >= 0:
        chain.append(index)
        index = model['objects'][index]['parent']
    return chain[::-1]


def mesh_bind_positions(model, mesh_index):
    """The mesh's vertices and normals in the skeleton's bind frame (the owning object's world
    transform applied; an unowned mesh stays put)."""
    m = model['meshes'][mesh_index]
    oi = model['mesh_object'].get(mesh_index)
    om = object_worlds(model)[oi] if oi is not None else np.eye(4)
    pos = np.c_[m['positions'], np.ones(m['count'])] @ om.T
    lin = om[:3, :3]
    nrm = m['normals'] @ np.linalg.pinv(lin)  # inverse transpose, row form (pinv: stages flatten props with S = 0)
    nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-12)
    return pos[:, :3], nrm


def mesh_skin(model, mesh_index):
    """[(bone index, weight), ...] per vertex (empty list -> unskinned)."""
    m = model['meshes'][mesh_index]
    if m['weights'] is None:
        return [[] for _ in range(m['count'])]
    pal = m['palette']
    out = []
    for w, s in zip(m['weights'], m['bone_slots']):
        out.append([(pal[int(s[k])], float(w[k])) for k in range(3) if w[k] > 0])
    return out


def skin_positions(model, worlds, binds=None):
    """Every mesh's vertices posed by `worlds` (one 4x4 per bone): list of (positions, normals,
    uv, triangles) with unskinned meshes left in the bind frame."""
    binds = binds or [bind_matrix(b) for b in model['bones']]
    skin_mats = [w @ np.linalg.inv(b) for w, b in zip(worlds, binds)]
    out = []
    for k, m in enumerate(model['meshes']):
        pos, nrm = mesh_bind_positions(model, k)
        if m['weights'] is None:
            out.append((pos, nrm, m['uv'], m['triangles']))
            continue
        ph = np.c_[pos, np.ones(len(pos))]
        posed = np.zeros((len(pos), 3))
        pnrm = np.zeros((len(pos), 3))
        pal = np.array(m['palette'], dtype=np.int64)
        for slot in range(3):
            w = m['weights'][:, slot]
            bi = pal[np.clip(m['bone_slots'][:, slot], 0, len(pal) - 1)]
            for b in np.unique(bi):
                sel = (bi == b) & (w > 0)
                if sel.any():
                    posed[sel] += w[sel, None] * (ph[sel] @ skin_mats[b].T)[:, :3]
                    pnrm[sel] += w[sel, None] * (nrm[sel] @ skin_mats[b][:3, :3].T)
        pnrm /= np.maximum(np.linalg.norm(pnrm, axis=1, keepdims=True), 1e-12)
        out.append((posed, pnrm, m['uv'], m['triangles']))
    return out


def consistent_winding(pos, nrm, tris):
    """Flip the triangles whose geometric normal opposes their vertex normals (the exporter's
    strips face either way). Returns (triangles, flipped count)."""
    if not len(tris):
        return tris, 0
    a, b, c = pos[tris[:, 0]], pos[tris[:, 1]], pos[tris[:, 2]]
    gn = np.cross(b - a, c - a)
    vn = nrm[tris[:, 0]] + nrm[tris[:, 1]] + nrm[tris[:, 2]]
    flip = (gn * vn).sum(axis=1) < 0
    t = tris.copy()
    t[flip] = t[flip][:, [0, 2, 1]]
    return t, int(flip.sum())


# ---------------------------------------------------------------------------
# DDR World game space (Y-up metres, facing +Z, left at +X: the same handedness as the TZM
# frame, so only a scale and the root-frame shift apply)
# ---------------------------------------------------------------------------
# Metres per model unit: the standard rig's Hip sits 9.655 units above the root; World's
# Hips joint is 0.970 m high (pl_rage00 / pl_emi00 bind).
GAME_SCALE = 0.970 / 9.655


def character_scale(model):
    """The uniform scale a SCALE node applies to the whole character (1.0 without one)."""
    for b in model['bones']:
        if b['name'] == SCALE_NODE:
            return float(b['S'][0])
    return 1.0


def rig_bones(model):
    """The bones the export rig keeps: the file order minus the SCALE node (folded into the
    scale), as [(name, parent name)], plus the {file index: rig index} map."""
    keep = [i for i, b in enumerate(model['bones']) if b['name'] != SCALE_NODE]
    index = {i: k for k, i in enumerate(keep)}
    out = []
    for i in keep:
        b = model['bones'][i]
        p = b['parent']
        while p >= 0 and p not in index:  # skip a dropped ancestor
            p = model['bones'][p]['parent']
        out.append((b['name'], model['bones'][p]['name'] if p >= 0 else None))
    return out, index


def bind_shift(model):
    """The bind frame -> root frame translation used for the EXPORTED rest pose: the bind
    frame's origin sits near the pelvis (the Hip's T2 is 0.819), so the T-posed mesh is lifted
    until its lowest vertex stands on the floor. Only the rest pose sees this: the clips give
    absolute root-frame positions, and the shift cancels in the skinning product."""
    lowest = None
    for k, m in enumerate(model['meshes']):
        if m['count']:
            y = float(mesh_bind_positions(model, k)[0][:, 1].min())
            lowest = y if lowest is None else min(lowest, y)
    return np.array([0.0, -lowest if lowest is not None else 0.0, 0.0])


def unit_scale(model):
    return GAME_SCALE * character_scale(model)


def _row(m):
    return np.asarray(m, dtype=float).T


def game_bind_matrices(model):
    """{bone name: 4x4 ROW-vector bind (local -> game world)} for rig_bones(model)."""
    s = unit_scale(model)
    S, Si = np.diag([s, s, s, 1.0]), np.diag([1 / s, 1 / s, 1 / s, 1.0])
    D = affine(np.eye(3), bind_shift(model))
    out = {}
    for b in model['bones']:
        if b['name'] == SCALE_NODE:
            continue
        out[b['name']] = _row(S @ D @ bind_matrix(b) @ Si)  # metres on both sides, rotation kept
    return out


def game_mesh(model):
    """Rest mesh in game space, all meshes joined: (positions, unit normals, uv (v down),
    colours (RGBA 0..1, None when no mesh has them), [(bone name, weight)] per vertex,
    triangles with consistent winding, per-triangle source mesh index)."""
    s = unit_scale(model)
    d = bind_shift(model)
    P_, N_, UV_, C_, W_, T_, M_ = [], [], [], [], [], [], []
    base = 0
    any_colour = any(m['colours'] is not None for m in model['meshes'])
    names = [b['name'] for b in model['bones']]
    for k, m in enumerate(model['meshes']):
        pos, nrm = mesh_bind_positions(model, k)
        tris, _ = consistent_winding(pos, nrm, m['triangles'])
        P_.append((pos + d) * s)
        N_.append(nrm)
        UV_.append(m['uv'])
        if any_colour:
            C_.append(m['colours'] if m['colours'] is not None else np.ones((m['count'], 4)))
        W_.extend([(names[b], w) for b, w in v] for v in mesh_skin(model, k))
        T_.append(tris + base)
        M_.append(np.full(len(tris), k))
        base += m['count']
    return (np.concatenate(P_), np.concatenate(N_), np.concatenate(UV_).copy(),
            np.concatenate(C_) if any_colour else None, W_, np.concatenate(T_), np.concatenate(M_))


def unscaled_bones(model):
    """The bone list with the SCALE node's scale taken out (it is folded into unit_scale:
    S_game . SCALE(s) . H = S_(game . s) . H for a uniform s)."""
    out = []
    for b in model['bones']:
        if b['name'] == SCALE_NODE:
            b = dict(b, S=np.ones(3))
        out.append(b)
    return out


def clip_game_worlds(model, record, bone_names, target_binds):
    """Per key of a character clip, per rig bone (in `bone_names` order): the ROW-vector world
    matrix for the EXPORTED bind frames `target_binds` (Q = B_target . B_game^-1 absorbs any
    rigid re-framing, so the skinning product is unchanged). Returns (keys x bones x 4 x 4)."""
    s = unit_scale(model)
    S, Si = np.diag([s, s, s, 1.0]), np.diag([1 / s, 1 / s, 1 / s, 1.0])
    binds = game_bind_matrices(model)
    q = [np.asarray(target_binds[i]) @ np.linalg.inv(binds[n]) for i, n in enumerate(bone_names)]
    bones = unscaled_bones(model)
    by_name = {b['name']: i for i, b in enumerate(bones)}
    n_keys = clip_frames(record)
    out = np.empty((n_keys, len(bone_names), 4, 4))
    for f in range(n_keys):
        W = clip_worlds(bones, record, f)
        for bi, n in enumerate(bone_names):
            out[f, bi] = q[bi] @ _row(S @ W[by_name[n]] @ Si)
    return out


def rowmat_to_quat(r):
    """Row-vector 3x3 rotation -> (x, y, z, w)."""
    m = np.asarray(r).T
    t = m[0, 0] + m[1, 1] + m[2, 2]
    if t > 0:
        s = 2.0 * math.sqrt(t + 1.0)
        q = ((m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s)
    elif m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        s = 2.0 * math.sqrt(1.0 + m[0, 0] - m[1, 1] - m[2, 2])
        q = (0.25 * s, (m[0, 1] + m[1, 0]) / s, (m[0, 2] + m[2, 0]) / s, (m[2, 1] - m[1, 2]) / s)
    elif m[1, 1] > m[2, 2]:
        s = 2.0 * math.sqrt(1.0 + m[1, 1] - m[0, 0] - m[2, 2])
        q = ((m[0, 1] + m[1, 0]) / s, 0.25 * s, (m[1, 2] + m[2, 1]) / s, (m[0, 2] - m[2, 0]) / s)
    else:
        s = 2.0 * math.sqrt(1.0 + m[2, 2] - m[0, 0] - m[1, 1])
        q = ((m[0, 2] + m[2, 0]) / s, (m[1, 2] + m[2, 1]) / s, 0.25 * s, (m[1, 0] - m[0, 1]) / s)
    q = np.array(q)
    return tuple(q / np.linalg.norm(q))


def clip_to_anm_spec(model, record, bone_names, parents, target_binds):
    """A scripts/anm_dump.py::write_anm spec for one SuperNova clip on the exported rig
    (`bone_names` in file order, `parents` indices, `target_binds` row matrices from the
    exported .model). The 30 Hz keys land every 2nd frame of World's 60 fps timeline (explicit
    times; the evaluator slerps between them), a 60 Hz clip every frame. Rotation = kind 0x1C,
    translation = 0x1D, one key when a channel never changes. Returns (spec, worlds)."""
    worlds = clip_game_worlds(model, record, bone_names, target_binds)
    n_f, n_b = worlds.shape[:2]
    step = max(1, int(round(60.0 / record['fps']))) if record['fps'] > 0 else 2
    times = [step * i for i in range(n_f)]
    tracks = []
    for b in range(n_b):
        p = parents[b]
        locs = worlds[:, b] if p < 0 else np.einsum('fij,fjk->fik', worlds[:, b], np.linalg.inv(worlds[:, p]))
        quats, prev = [], None
        for m in locs:
            qv = rowmat_to_quat(m[:3, :3])
            if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
                qv = tuple(-c for c in qv)
            quats.append(qv)
            prev = qv
        trans = [tuple(float(x) for x in m[3, :3]) for m in locs]
        qa = np.array(quats)
        if np.abs(qa - qa[0]).max() < 1e-6:
            tracks.append(dict(kind=0x1C, target=b, keys=[quats[0]]))
        else:
            tracks.append(dict(kind=0x1C, target=b, times=times, keys=quats))
        ta = np.array(trans)
        if np.abs(ta - ta[0]).max() < 1e-5:
            tracks.append(dict(kind=0x1D, target=b, keys=[trans[0]]))
        else:
            tracks.append(dict(kind=0x1D, target=b, times=times, keys=trans))
    return dict(frame_count=max(times[-1], 1), flag=0, hierarchy=list(parents), tracks=tracks), worlds


# ---------------------------------------------------------------------------
# stage records: XSI fcurves, material tracks and cameras
# (SLPM_666.09: binder FUN_00149a60, key search FUN_0014a650, interpolation FUN_00149150,
#  Bezier FUN_00149340 / FUN_0013bd80; docs/ps2_ddr_filedata_research.md §7.4)
# ---------------------------------------------------------------------------
def record_frames(record):
    """The record frames a stage / camera record is played over: `first..last`, clamped to start
    at 0 (stage007's camera record carries a 22-frame lead-in at negative frames that the
    re-exported twin stage017 does not have). Key i of a per-frame (flag 0) track is frame
    `first + i`."""
    return list(range(max(record['first'], 0), record['last'] + 1))


def fcurve_interpolation(key):
    """The interpolation code of one 7-float fcurve key: the 4th float IS a u32 (1 linear, 2
    Bezier, anything else = hold the key's value)."""
    return int(np.asarray(key[3], dtype=np.float32).view(np.uint32))


def _bezier(s, k0, k1):
    """Point (value, time) at parameter s of the cubic through k0, k0's right handle, k1's left
    handle, k1 -- FUN_0013bd80's weights on FUN_00149150's control points."""
    u = 1.0 - s
    w = (u * u * u, 3.0 * u * u * s, 3.0 * u * s * s, s * s * s)
    val = w[0] * k0[4] + w[1] * k0[6] + w[2] * k1[5] + w[3] * k1[4]
    tim = w[0] * k0[0] + w[1] * k0[2] + w[2] * k1[1] + w[3] * k1[0]
    return val, tim


def fcurve_value(keys, t, tol=1e-3):
    """Evaluate a flag-3 fcurve (rows {time, hl_time, hr_time, interp, value, hl_value, hr_value})
    at record frame `t` the way the game does: hold before the first / after the last key
    (FUN_0014a650 clamps), linear between two keys of type 1, and for type 2 the cubic Bezier
    (k0, k0 right handle, k1 left handle, k1) solved for `t` by bisection on the parameter until
    the time error is <= `tol` frames (FUN_00149340 starts at s = 0.5, step 0.25; the game's own
    tolerance is 0.1 * the clock step). Type 0 holds k0's value."""
    k = np.asarray(keys, dtype=float)
    if len(k) == 0:
        raise ValueError('empty fcurve')
    if t <= k[0, 0] or len(k) == 1:
        return float(k[0, 4])
    if t >= k[-1, 0]:
        return float(k[-1, 4])
    i = int(np.searchsorted(k[:, 0], t, side='right')) - 1
    k0, k1 = k[i], k[i + 1]
    if t == k0[0]:
        return float(k0[4])
    kind = fcurve_interpolation(k0)
    if kind == FCURVE_LINEAR:
        return float(k0[4] + (k1[4] - k0[4]) * (t - k0[0]) / (k1[0] - k0[0]))
    if kind != FCURVE_BEZIER:
        return float(k0[4])
    s, step, last = 0.5, 0.25, None
    val = float(k0[4])
    for _ in range(64):
        val, tim = _bezier(s, k0, k1)
        err = abs(t - tim)
        if err <= tol or err == last:
            break
        s = s - step if t < tim else s + step
        step *= 0.5
        last = err
    return float(val)


def track_value(track, record, frame):
    """A track's value(s) at record frame `frame`: the static key (flag 2), the fcurve (flag 3,
    a 1-vector), or the per-frame key `frame - first` clamped into range (flag 0)."""
    keys = track['keys']
    if track['flag'] == FLAG_STATIC or len(keys) == 1:
        return keys[0]
    if track['flag'] == FLAG_FCURVE:
        return np.array([fcurve_value(keys, frame)])
    i = min(max(int(frame) - record['first'], 0), len(keys) - 1)
    return keys[i]


def material_tracks(record):
    """{material name: {(kind, component): track}} for the material tracks of a stage record
    (kinds >= 500): component = n - 8 for the vector kinds (0..3 = x/y/z/w), 0 for the scalar
    kind 1302 (n = 7)."""
    out: dict[str, dict[tuple[int, int], dict]] = {}
    for t in record['tracks']:
        if t['kind'] < KIND_TEX_SCALE:
            continue
        comp = 0 if t['kind'] == KIND_GLOW else t['n'] - 8
        out.setdefault(t['name'], {})[(t['kind'], comp)] = t
    return out


def material_animation(record, frames=None):
    """The ANIMATED material channels of a stage record, sampled over `frames` (default
    record_frames): {material: dict(uv_offset (n, 2) | None, colour (n, 4) | None, glow (n,) |
    None, unsupported [str])}. Only materials with at least one fcurve track are listed; a
    channel is None when none of its component tracks is an fcurve (a static translation 0 /
    colour 1 / glow 0 is the model's default). `unsupported` names fcurves the World side has no
    parameter for (an animated texture scale or rotation, texture stages > 0)."""
    frames = list(frames) if frames is not None else record_frames(record)
    out = {}
    for name, tracks in material_tracks(record).items():
        if not any(t['flag'] == FLAG_FCURVE for t in tracks.values()):
            continue
        entry: dict[str, Any] = dict(uv_offset=None, colour=None, glow=None, unsupported=[])

        def sample(kind, ncomp):
            comps = [tracks.get((kind, c)) for c in range(ncomp)]
            if not any(t is not None and t['flag'] == FLAG_FCURVE for t in comps):
                return None
            arr = np.zeros((len(frames), ncomp))
            for c, t in enumerate(comps):
                if t is None:
                    arr[:, c] = 1.0 if kind == KIND_TEX_COLOUR else 0.0
                    continue
                for i, f in enumerate(frames):
                    arr[i, c] = float(track_value(t, record, f)[0])
            return arr

        uv = sample(KIND_TEX_TRANSLATION, 3)
        entry['uv_offset'] = None if uv is None else uv[:, :2]
        entry['colour'] = sample(KIND_TEX_COLOUR, 4)
        glow = sample(KIND_GLOW, 1)
        entry['glow'] = None if glow is None else glow[:, 0]
        for (kind, comp), t in sorted(tracks.items()):
            if t['flag'] != FLAG_FCURVE:
                continue
            if kind in (KIND_TEX_SCALE, KIND_TEX_ROTATION) or (kind == KIND_TEX_TRANSLATION and comp == 2) \
                    or kind not in (KIND_TEX_SCALE, KIND_TEX_ROTATION, KIND_TEX_TRANSLATION, KIND_TEX_COLOUR, KIND_GLOW):
                entry['unsupported'].append('kind %d component %d' % (kind, comp))
        out[name] = entry
    return out


def camera_tracks(record):
    """{camera name: {kind: track}} for the camera tracks (kinds 4..8) of a camera record."""
    out: dict[str, dict[int, dict]] = {}
    for t in record['tracks']:
        if KIND_CAM_SRT <= t['kind'] <= KIND_CAM_ROLL:
            out.setdefault(t['name'], {})[t['kind']] = t
    return out


def camera_samples(record, name, frames=None):
    """(position (n, 3), interest (n, 3), fov (n,) radians, roll (n,) radians) of camera `name`
    in stage units over `frames` (default record_frames). The kind-4 SRT of the camera null is
    applied to position and interest when it is not the identity."""
    frames = list(frames) if frames is not None else record_frames(record)
    tracks = camera_tracks(record)[name]
    n = len(frames)
    pos, aim = np.zeros((n, 3)), np.zeros((n, 3))
    fov = np.full(n, XSI_DEFAULT_FOV)
    roll = np.zeros(n)
    for i, f in enumerate(frames):
        p = np.array(track_value(tracks[KIND_CAM_POSITION], record, f)[:3], dtype=float)
        a = np.array(track_value(tracks[KIND_CAM_INTEREST], record, f)[:3], dtype=float)
        if KIND_CAM_SRT in tracks:
            s, q, t = srt_key(track_value(tracks[KIND_CAM_SRT], record, f))
            m = affine(quat_mat(q), t, s)
            p = (m @ np.r_[p, 1.0])[:3]
            a = (m @ np.r_[a, 1.0])[:3]
        pos[i], aim[i] = p, a
        if KIND_CAM_FOV in tracks:
            fov[i] = float(track_value(tracks[KIND_CAM_FOV], record, f)[0])
        if KIND_CAM_ROLL in tracks:
            roll[i] = float(track_value(tracks[KIND_CAM_ROLL], record, f)[0])
    return pos, aim, fov, roll


def look_at_rows(pos, interest, roll=0.0, up=(0.0, 1.0, 0.0)):
    """Row-vector 3x3 rotation of a camera at `pos` looking at `interest` (the game's camera
    convention: local -Z forward, +Y up -- `camera_node_apply_camanm_record` reads
    `target = eye - R.row2`, `up = R.row1`), then rolled by `roll` radians about the view axis
    (positive = counter-clockwise on screen). Falls back to +Z as the hint when looking straight
    up / down."""
    fwd = np.asarray(interest, dtype=float) - np.asarray(pos, dtype=float)
    fwd /= max(np.linalg.norm(fwd), 1e-12)
    hint = np.asarray(up, dtype=float)
    if abs(float(fwd @ hint)) > 0.9999:
        hint = np.array([0.0, 0.0, 1.0])
    right = np.cross(fwd, hint)
    right /= max(np.linalg.norm(right), 1e-12)
    up_v = np.cross(right, fwd)
    if roll:
        c, s = math.cos(roll), math.sin(roll)
        right, up_v = c * right - s * up_v, s * right + c * up_v
    return np.array([right, up_v, -fwd])


def world_camanm_fov(half_tangent, aspect_file=4.0 / 3.0):
    """The `.camanm` slot-2 degrees that make World's CameraNode end up with the horizontal
    half-tangent `half_tangent` (docs/3d_model_format_research.md §6; the game computes t' =
    tan(0.5 atan(1 / (tan(fov/2) aspect))), this is its inverse --
    blender_ddr_addon.import_anm.camanm_fov_from_half_tangent without bpy)."""
    fov_prime = 2.0 * math.atan(half_tangent)
    h = 1.0 / math.tan(fov_prime)
    return math.degrees(2.0 * math.atan(h / aspect_file))


def sn_fov_to_camanm(fov_h, keep='vertical', aspect_file=4.0 / 3.0):
    """SuperNova's horizontal FOV (radians, of its 4:3 frame) -> the camanm slot-2 degrees.
    `keep='vertical'` shows the same vertical extent on World's 16:9 frame (the sides widen --
    what SuperNova showed top to bottom stays in frame); `keep='horizontal'` keeps the 4:3
    horizontal extent instead (top / bottom cropped)."""
    if keep == 'vertical':
        half_tangent = math.tan(fov_h / 2.0) * 0.75 * (16.0 / 9.0)
    elif keep == 'horizontal':
        half_tangent = math.tan(fov_h / 2.0)
    else:
        raise ValueError(keep)
    return world_camanm_fov(half_tangent, aspect_file)


def camera_to_camanm_spec(record, name, unit_scale, frames=None, keep='vertical', near=0.1, far=32768.0):
    """A scripts/anm_dump.py::write_anm camera spec for camera `name` of a camera record: one
    key per record frame at World's 60 fps (`step = 60 / fps` frames apart), position in
    centimetres (`unit_scale` metres per stage unit), orientation from the look-at (roll
    applied), the FOV mapped with sn_fov_to_camanm. Returns (spec, 60 fps key times, (pos_m,
    interest_m) the clip should reproduce)."""
    frames = list(frames) if frames is not None else record_frames(record)
    pos, aim, fov, roll = camera_samples(record, name, frames)
    step = max(1, int(round(60.0 / record['fps']))) if record['fps'] > 0 else 1
    times = [step * i for i in range(len(frames))]
    pos_m, aim_m = pos * unit_scale, aim * unit_scale
    quats, prev = [], None
    for i in range(len(frames)):
        qv = rowmat_to_quat(look_at_rows(pos_m[i], aim_m[i], roll[i]))
        if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
            qv = tuple(-c for c in qv)
        quats.append(qv)
        prev = qv
    pos_cm = [tuple(float(x) for x in p * 100.0) for p in pos_m]
    degs = [sn_fov_to_camanm(float(f), keep) for f in fov]

    def const(target, value):
        return dict(kind=8, target=target, times=[0], keys=[(float(value),)])

    def collapse(kind, target, keys, tol):
        """One key when the channel never moves (static shots), else one per frame."""
        if all(abs(c - c0) <= tol for k in keys for c, c0 in zip(k, keys[0])):
            return dict(kind=kind, target=target, times=[0], keys=[keys[0]])
        return dict(kind=kind, target=target, times=times, keys=keys)

    camera = [collapse(1, 0, quats, 1e-7), collapse(4, 1, pos_cm, 1e-4),
              collapse(8, 2, [(d,) for d in degs], 1e-4), const(3, near), const(4, far), const(5, 4.0 / 3.0)]
    spec = dict(frame_count=max(times[-1], 1), flag=0, fps=60, camera=camera)
    return spec, times, (pos_m, aim_m)


# ---------------------------------------------------------------------------
# software preview (orthographic, textured, z-buffered) for validation without Blender
# ---------------------------------------------------------------------------
def render(pos, uv, tris, tex, res=800, axis=(0, 1)):
    """Orthographic render of triangles: `axis` = (horizontal, vertical) coordinate indices;
    `tex` an h x w x 4 array sampled with v down. Returns res x res RGBA uint8."""
    lo, hi = pos.min(0), pos.max(0)
    span = float((hi - lo).max()) * 1.05 or 1.0
    ctr = (lo + hi) / 2
    img = np.zeros((res, res, 4), np.uint8)
    img[..., :3] = 40
    img[..., 3] = 255
    zbuf = np.full((res, res), -1e30)
    th, tw = tex.shape[:2]
    depth = 3 - axis[0] - axis[1]
    for t in tris:
        p = pos[t]
        sx = (p[:, axis[0]] - ctr[axis[0]]) / span * res + res / 2
        sy = res / 2 - (p[:, axis[1]] - ctr[axis[1]]) / span * res
        z = p[:, depth]
        x0, x1 = int(max(0, math.floor(sx.min()))), int(min(res - 1, math.ceil(sx.max())))
        y0, y1 = int(max(0, math.floor(sy.min()))), int(min(res - 1, math.ceil(sy.max())))
        if x1 < x0 or y1 < y0:
            continue
        xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
        det = (sx[1] - sx[0]) * (sy[2] - sy[0]) - (sx[2] - sx[0]) * (sy[1] - sy[0])
        if abs(det) < 1e-12:
            continue
        l1 = ((xs - sx[0]) * (sy[2] - sy[0]) - (sx[2] - sx[0]) * (ys - sy[0])) / det
        l2 = ((sx[1] - sx[0]) * (ys - sy[0]) - (xs - sx[0]) * (sy[1] - sy[0])) / det
        l0 = 1 - l1 - l2
        inside = (l0 >= -1e-6) & (l1 >= -1e-6) & (l2 >= -1e-6)
        if not inside.any():
            continue
        zz = l0 * z[0] + l1 * z[1] + l2 * z[2]
        u = l0 * uv[t[0], 0] + l1 * uv[t[1], 0] + l2 * uv[t[2], 0]
        v = l0 * uv[t[0], 1] + l1 * uv[t[1], 1] + l2 * uv[t[2], 1]
        tx = np.clip((u * tw).astype(int), 0, tw - 1)
        ty = np.clip((v * th).astype(int), 0, th - 1)
        sub = zbuf[y0:y1 + 1, x0:x1 + 1]
        upd = inside & (zz > sub)
        sub[upd] = zz[upd]
        img[y0:y1 + 1, x0:x1 + 1][upd] = tex[ty, tx][upd]
    return img


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def _tilde(p):
    home = os.path.expanduser('~')
    return '~' + p[len(home):] if p.startswith(home) else p


def cmd_info(a):
    chunks = load_tzm(a.file)
    d = dict(chunks)
    for name, c in chunks:
        print('%-24s %8d bytes' % (name, len(c)))
    if 'IMAGELIST' in d:
        for n, t in textures_of(chunks).items():
            print('texture %-20s %dx%d (%r)' % (n, t['width'], t['height'], t['name']))
    if 'MODEL' in d:
        m = parse_model(d['MODEL'])
        print('MODEL root %r, sections %s, %d bytes' % (m['root'], m['sections'], m['size']))
        for o in m['objects']:
            print('  object %-16s parent %2d T=%s R=%s S=%s' % (o['name'], o['parent'], np.round(o['T'], 3),
                                                              np.round(o['R'], 3), np.round(o['S'], 3)))
        for k, me in enumerate(m['meshes']):
            print('  mesh %d %-20s fmt 0x%X %5d verts %5d tris palette %s next %d object %s' % (
                k, me['material'], me['format'], me['count'], len(me['triangles']), me['palette'], me['next_mesh'],
                m['objects'][m['mesh_object'][k]]['name'] if k in m['mesh_object'] else '-'))
        for b in m['bones']:
            print('  bone %-16s parent %2d T=%s R=%s S=%s | bind T2=%s R2=%s' % (
                b['name'], b['parent'], np.round(b['T'], 3), np.round(b['R'], 3), np.round(b['S'], 2),
                np.round(b['T2'], 3), np.round(b['R2'], 3)))
    if 'MOTION' in d:
        for r in parse_motion(d['MOTION']):
            print('MOTION %r: %d tracks, frames %d..%d @ %g fps, %d keys' % (
                r['name'], r['ntracks'], r['first'], r['last'], r['fps'], clip_frames(r)))
            for t in r['tracks']:
                print('  track %-20s kind %4d flag %d %4d keys x %d' % (t['name'], t['kind'], t['flag'],
                                                                     len(t['keys']), t['keys'].shape[1]))


def cmd_png(a):
    chunks = load_tzm(a.file)
    os.makedirs(a.out, exist_ok=True)
    for n, t in textures_of(chunks).items():
        out = os.path.join(a.out, n + '.png')
        P.write_png(out, t['width'], t['height'], t['rgba'].tobytes())
        print('wrote', _tilde(out))


def cmd_preview(a):
    chunks = load_tzm(a.file)
    d = dict(chunks)
    model = parse_model(d['MODEL'])
    tex = next(iter(textures_of(chunks).values()))['rgba']
    if a.motion:
        rec = parse_motion(dict(load_tzm(a.motion))['MOTION'])[0]
        worlds = clip_worlds(model['bones'], rec, a.frame)
    else:
        worlds = rest_worlds(model['bones'])
    parts = skin_positions(model, worlds)
    pos = np.concatenate([p for p, _, _, _ in parts])
    uv = np.concatenate([u for _, _, u, _ in parts])
    tris, base = [], 0
    for p, n, _u, t in parts:
        t2, _ = consistent_winding(p, n, t)
        tris.append(t2 + base)
        base += len(p)
    img = render(pos, uv, np.concatenate(tris), tex, res=a.res, axis=(2, 1) if a.side else (0, 1))
    P.write_png(a.out, a.res, a.res, img.tobytes())
    print('wrote', _tilde(a.out), 'y range %.3f..%.3f' % (pos[:, 1].min(), pos[:, 1].max()))


def survey(paths):
    """Parse every chunk of every TZM: (files, models, meshes, motion records, tracks, problems)."""
    files = models = meshes = records = tracks = 0
    problems = []
    for path in paths:
        files += 1
        try:
            chunks = load_tzm(path)
            d = dict(chunks)
            if 'IMAGELIST' in d:
                textures_of(chunks)
            if 'MODEL' in d and chunk_payload(d['MODEL'], 0x40)[1] is not None:
                m = parse_model(d['MODEL'])
                models += 1
                meshes += len(m['meshes'])
                if m['meshes']:
                    end = m['meshes'][-1]['end']
                    nb = m['sections'][4]
                    bones_at = nb[1] - MODEL_HEADER if nb[0] else m['size']
                    if end != bones_at:
                        problems.append((path, 'mesh data ends at 0x%X, bones at 0x%X' % (end, bones_at)))
                if m['bones'] and (m['bones'][0]['name'] != ROOT_NODE or m['objects'][0]['name'] != ROOT_NODE):
                    problems.append((path, 'node lists do not start with globalSRT'))
                if len(m['mesh_object']) != len(m['meshes']):
                    problems.append((path, '%d of %d meshes owned by an object' % (len(m['mesh_object']), len(m['meshes']))))
                if m['bones']:
                    rest_worlds(m['bones'])
                    skin_positions(m, rest_worlds(m['bones']))
            if 'MOTION' in d and chunk_payload(d['MOTION'], 0x40)[1] is not None:
                recs = parse_motion(d['MOTION'])
                records += len(recs)
                tracks += sum(len(r['tracks']) for r in recs)
        except (ValueError, IndexError, KeyError, struct.error) as e:
            problems.append((path, str(e)))
    return files, models, meshes, records, tracks, problems


def cmd_survey(a):
    paths = []
    for d in a.dirs:
        paths += sorted(glob.glob(os.path.join(d, '**', '*.[Tt][Zz][Mm]'), recursive=True))
    if not a.include_tests:  # IMAGE/test/: developer assets in older layouts (raw textures, 29.97 fps)
        paths = [p for p in paths if 'test' not in os.path.relpath(p, os.path.commonpath(a.dirs)).split(os.sep)[:-1]]
    files, models, meshes, records, tracks, problems = survey(paths)
    print('%d TZM files: %d models (%d meshes), %d motion records (%d tracks), %d problems' % (
        files, models, meshes, records, tracks, len(problems)))
    for path, why in problems[:30]:
        print('  %s: %s' % (_tilde(path), why))
    return 1 if problems else 0


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('info')
    p.add_argument('file')
    p.set_defaults(fn=cmd_info)
    p = sub.add_parser('png')
    p.add_argument('file')
    p.add_argument('out')
    p.set_defaults(fn=cmd_png)
    p = sub.add_parser('preview')
    p.add_argument('file')
    p.add_argument('out')
    p.add_argument('--motion')
    p.add_argument('--frame', type=int, default=0)
    p.add_argument('--res', type=int, default=800)
    p.add_argument('--side', action='store_true')
    p.set_defaults(fn=cmd_preview)
    p = sub.add_parser('survey')
    p.add_argument('dirs', nargs='+')
    p.add_argument('--include-tests', action='store_true', help='also the IMAGE/test/ developer packs')
    p.set_defaults(fn=cmd_survey)
    a = ap.parse_args(argv)
    return a.fn(a) or 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
