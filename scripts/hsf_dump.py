#!/usr/bin/env python3
"""Reference decoder for Hudson Soft's HSF 3D format (`HSFV037`) as used by Dancing Stage /
DanceDanceRevolution HOTTEST PARTY (Wii, 2007 -- developed with Hudson on its Mario Party
GameCube/Wii engine): the polygon dancers, their dance routines, the stages and the song cameras,
plus the conversion math the DDR World port uses (tools/blender_ddr_addon/examples/
port_character_hottest.py, port_stage_hottest.py). Formats and RE: docs/wii_ddr_hottest_party_research.md.

The layout is the one the Mario Party 4 decompilation documents (mariopartyrd/marioparty4:
include/game/hsfformat.h, src/game/hsfload.c, hsfmotion.c, EnvelopeExec.c, hsfdraw.c); every
structure below was checked against Hottest Party's own files. All values are big-endian.

  header    char magic[8] "HSFV037\\0", then 21 x {s32 offset, s32 count}: scene, color,
            material, attribute, vertex, normal, st, face, object, bitmap, palette, motion,
            cenv, skeleton, part, cluster, shape, mapAttr, matrix, symbol, string. The string
            section's count is its size in bytes; a name field is a byte offset into it.
            The symbol section is a u32 index array that list fields (children, material
            attributes, ...) point into.
  buffers   (color / vertex / normal / st / face) {u32 name, s32 count, u32 data} x n, then
            the data area the `data` offsets are relative to: vertex = f32 xyz, st = f32 st,
            color = RGBA8, normal = f32 xyz in a skinned file (cenv count > 0) and s8 xyz
            (/64) otherwise.
  face      0x30 per face: s16 type (2 tri, 3 quad, 4 strip; & 7), s16 material, s16
            indices[4][4] ({position, normal, colour, st} per corner), f32 nbt[3]. A strip
            keeps its first triangle in indices[0..2] and puts {u32 count, u32 index} in
            place of indices[3]: `count` more corners at strip_area + 8 * index, where the
            strip area follows the face records of the LAST face buffer (hsfload.c FaceLoad).
            GX draw order: a triangle is corners (0, 2, 1), a quad (0, 2, 3, 1), a strip
            0, 2, 1, then the extra corners.
  object    0x144: u32 name, u32 type (0 null, 1 replica, 2 mesh, 3 root, 4 joint, 5/6 null,
            7 camera, 8 light, 9 map), u32 constData, u32 flags, s32 parent, u32 nchildren,
            u32 children (symbol index), f32 base T/R/S[9] (rotation = Euler degrees,
            M = T . Rz . Ry . Rx . S on column vectors), f32 curr[9], f32 min[3], max[3],
            baseMorph, morphWeight[33], s32 face, vertex, normal, color, st, material,
            attribute (buffer indices, -1 = none), u8 x4, u32 nshape, shape, ncluster, cluster,
            ncenv, cenv, file[2] (the rest-pose vertex / normal copies at runtime).
  material  0x3C: u32 name, 4, u16 pass, u8 vtxMode (5 = vertex colours), u8 litColor[3],
            color[3], shadowColor[3], f32 hilite, f32, f32 invAlpha (transparency), f32[2],
            f32 refAlpha, f32, u32 flags, u32 nattr, u32 attr (symbol index -> attribute ids).
  attribute 0x84: u32 name (-1 = none), ..., f32 kColor @0x0C, nbtTpLvl @0x14, scale @0x28,
            trans @0x30 (UV), wrapS @0x64, wrapT @0x68, maxLod @0x78, flag @0x7C, s32 bitmap.
  bitmap    0x20: u32 name, u32 maxLod, u8 dataFmt (0 I4, 1 I8, 2 IA4, 3 IA8, 4 RGB565,
            5 RGB5A3, 6 RGBA8, 7 CMPR, 9/10/11 CI with an RGB565 / RGB5A3 / IA8 palette),
            u8 pixSize (4 / 8 for CI), s16 w, h, palSize, u32 tint, s32 palette (index),
            u32, u32 data (relative to the end of the bitmap records). Pixels are GX-tiled.
  palette   0x10: u32 name, s32, u32 count, u32 data (relative to the end of the records).
  cenv      0x24 per envelope: u32 name, u32 single, dual, multi (offsets into the data area
            after the records), u32 nsingle, ndual, nmulti, vtxCount, copyCount. single 0xC
            {u32 target, u16 pos, posNum, nrm, nrmNum}: posNum vertices from pos ride object
            `target` rigidly; dual 0x10 {u32 target1, target2, nweights, weights}: weights
            0xC {f32 w, u16 pos, posNum, nrm, nrmNum} blend target1 (w) and target2 (1 - w);
            multi 0x10 {u32 nweights, u16 pos, posNum, nrm, nrmNum, weights}: weights 8 {u32
            target, f32 w}. The dual / multi weight offsets are byte offsets into the area
            after ALL envelopes' single / dual / multi records. Then `copyCount` vertices from
            `vtxCount` keep their rest position.
  skeleton  0x28: u32 name, f32 T/R/S[9] -- the bind pose of the object with that name (it
            overrides the object's base transform; EnvelopeExec.c SetMtx).
  matrix    {u32 base, u32 count, u32 0} + Mtx[]: runtime scratch.
  motion    0x10 per motion: u32 name, u32 ntracks, u32 track, f32 maxTime (frames, 60 Hz),
            then ntracks tracks of 0x10: u8 type (2 object transform, 3 morph, 5 cluster, 6
            cluster weight, 9 material, 10 attribute), u8 start, u16 target (string offset
            of the target object's name), u16 index (material / attribute / morph), u16
            channel, u16 curve (0 step, 1 linear, 2 Hermite, 3 bitmap, 4 constant), u16 nkeys,
            u32 data (offset into the key area after the tracks; a constant's value inline).
            Keys: step / linear {f32 time, value}, Hermite {f32 time, value, out-slope,
            in-slope} evaluated per segment as hermite(t) with the slopes NOT scaled by the
            segment length (hsfmotion.c GetBezier). Object channels: 8/9/10 = T xyz, 28/29/30
            = R xyz (degrees), 31/32/33 = S xyz; material 0/1/2 litColor, 0x31.. color, 0x39
            invAlpha; attribute 8/9/10, 28/29/30, 31/32/33 = UV transform, 0x43 bitmap.

Skinning (EnvelopeExec.c): every mesh vertex is stored in the mesh object's rest frame; with
W_j the rest world of object j (base transforms, skeleton overriding) and C_j the current one,
a vertex bound to j lands at C_j . W_j^-1 . W_mesh . v (dual / multi: the weighted sum). A
dancer's joints are `<Joint>` objects (type 4 / 0) with `<Joint>*root` / `<Joint>*leaf` /
`<Joint>*end` helper objects MayaConverter adds around them; the dance clips (c_000.bin and the
per-song c_000_NN.bin bundles) animate the `<Joint>` names only.

Usage:
    hsf_dump.py info  <file.hsf>                 # sections, objects, meshes, motions
    hsf_dump.py tree  <file.hsf>                 # object hierarchy
    hsf_dump.py png   <file.hsf> <out dir>       # every bitmap as PNG
    hsf_dump.py survey <dir>...                  # parse every .hsf under the dirs

Import-safe: `from hsf_dump import parse_hsf, rest_worlds, skinned_mesh, ...`. Needs numpy.
GX texture decoding is in scripts/extract_wii_ddr_data.py (imported).
"""
import argparse
import math
import os
import struct
import sys
from typing import Any

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import extract_wii_ddr_data as W  # noqa: E402  (GX textures, PNG)

MAGIC = b'HSFV037\0'
SECTIONS = ('scene color material attribute vertex normal st face object bitmap palette motion '
            'cenv skeleton part cluster shape mapAttr matrix symbol string').split()
OBJECT_SIZE = 0x144
FACE_SIZE = 0x30
MATERIAL_SIZE = 0x3C
ATTRIBUTE_SIZE = 0x84
BITMAP_SIZE = 0x20
PALETTE_SIZE = 0x10
CENV_SIZE = 0x24
SKELETON_SIZE = 0x28
TRACK_SIZE = 0x10
MOTION_SIZE = 0x10

OBJ_NULL1, OBJ_REPLICA, OBJ_MESH, OBJ_ROOT, OBJ_JOINT, OBJ_NULL2, OBJ_NULL3, OBJ_CAMERA, OBJ_LIGHT, OBJ_MAP = range(10)
OBJECT_TYPES = {0: 'null', 1: 'replica', 2: 'mesh', 3: 'root', 4: 'joint', 5: 'null2', 6: 'null3', 7: 'camera',
                8: 'light', 9: 'map'}
FACE_TRI, FACE_QUAD, FACE_STRIP = 2, 3, 4
TRACK_TRANSFORM, TRACK_MORPH, TRACK_CLUSTER, TRACK_CLUSTER_WEIGHT, TRACK_MATERIAL, TRACK_ATTRIBUTE = 2, 3, 5, 6, 9, 10
CURVE_STEP, CURVE_LINEAR, CURVE_HERMITE, CURVE_BITMAP, CURVE_CONST = 0, 1, 2, 3, 4
CH_T = (8, 9, 10)
CH_R = (28, 29, 30)
CH_S = (31, 32, 33)
CHANNEL_SLOT = {8: 0, 9: 1, 10: 2, 28: 3, 29: 4, 30: 5, 31: 6, 32: 7, 33: 8}
MATERIAL_VTX_COLOUR = 5
MATERIAL_FLAG_NOCULL = 1 << 1
MATERIAL_FLAG_ADDCOL = 1 << 4
MATERIAL_FLAG_INVCOL = 1 << 5
MATERIAL_FLAG_NO_ZWRITE = 1 << 9


def _u32(d, o):
    return struct.unpack_from('>I', d, o)[0]


def _s32(d, o):
    return struct.unpack_from('>i', d, o)[0]


def is_hsf(blob) -> bool:
    return len(blob) >= 0xB0 and blob[:8] == MAGIC


def header(data) -> dict[str, tuple[int, int]]:
    v = struct.unpack_from('>42i', data, 8)
    return {SECTIONS[i]: (v[2 * i], v[2 * i + 1]) for i in range(21)}


class _Reader:
    def __init__(self, data):
        if not is_hsf(data):
            raise ValueError('not an HSFV037 file')
        self.d = data
        self.h = header(data)
        so, sn = self.h['string']
        self.strings = data[so:so + sn]
        syo, syn = self.h['symbol']
        self.symbols = list(struct.unpack_from('>%dI' % syn, data, syo)) if syn else []

    def name(self, off):
        if off == 0xFFFFFFFF or off < 0 or off >= len(self.strings):
            return None
        end = self.strings.find(b'\0', off)
        return self.strings[off:end if end >= 0 else len(self.strings)].decode('latin1')

    def sym(self, index, count):
        return self.symbols[index:index + count]


def _buffers(r, section, decode):
    off, n = r.h[section]
    out = []
    base = off + 12 * n
    for i in range(n):
        name, count, data = struct.unpack_from('>IiI', r.d, off + 12 * i)
        out.append(dict(name=r.name(name), count=count, data=decode(base + data, count)))
    return out, base


def _vec3(r):
    return lambda o, n: np.frombuffer(r.d, '>f4', 3 * n, o).reshape(n, 3).astype(np.float64)


def _faces(r):
    """Face buffers -> list of {name, faces: [(type, material, [(pos, nrm, col, st)...])]} with the
    strip corners resolved (the strip area follows the last buffer's face records)."""
    off, n = r.h['face']
    heads = [struct.unpack_from('>IiI', r.d, off + 12 * i) for i in range(n)]
    base = off + 12 * n
    strip_area = base + max((d + FACE_SIZE * c for _nm, c, d in heads), default=0)
    out = []
    for name, count, data in heads:
        faces = []
        for k in range(count):
            o = base + data + FACE_SIZE * k
            ftype, mat = struct.unpack_from('>hh', r.d, o)
            idx = struct.unpack_from('>16h', r.d, o + 4)
            corners = [tuple(idx[4 * c:4 * c + 4]) for c in range(4)]
            kind = ftype & 7
            if kind == FACE_TRI:
                seq = [corners[0], corners[2], corners[1]]
            elif kind == FACE_QUAD:
                seq = [corners[0], corners[2], corners[3], corners[1]]
            elif kind == FACE_STRIP:
                cnt, sidx = struct.unpack_from('>II', r.d, o + 4 + 24)
                extra = struct.unpack_from('>%dh' % (4 * cnt), r.d, strip_area + 8 * sidx)
                seq = [corners[0], corners[2], corners[1]] + [tuple(extra[4 * c:4 * c + 4]) for c in range(cnt)]
            else:
                seq = []
            faces.append(dict(type=kind, material=mat & 0xFFF, corners=seq))
        out.append(dict(name=r.name(name), faces=faces))
    return out


def _transform(vals):
    return dict(T=np.array(vals[0:3], dtype=np.float64), R=np.array(vals[3:6], dtype=np.float64),
                S=np.array(vals[6:9], dtype=np.float64))


def _objects(r):
    off, n = r.h['object']
    out = []
    for i in range(n):
        o = off + OBJECT_SIZE * i
        name, otype, const, flags, parent, nchild, child = struct.unpack_from('>IIIIiII', r.d, o)
        base = struct.unpack_from('>9f', r.d, o + 0x1C)
        lo = struct.unpack_from('>3f', r.d, o + 0x64)
        hi = struct.unpack_from('>3f', r.d, o + 0x70)
        refs = struct.unpack_from('>7i', r.d, o + 0x104)
        nshape, shape, ncluster, cluster, ncenv, cenv = struct.unpack_from('>6I', r.d, o + 0x124)
        obj = dict(index=i, name=r.name(name), type=otype, flags=flags, const=const,
                   parent=parent if parent >= 0 else None, children=r.sym(child, nchild),
                   base=_transform(base))
        if otype == OBJ_MESH:
            obj.update(face=refs[0], vertex=refs[1], normal=refs[2], color=refs[3], st=refs[4],
                       material=refs[5], attribute=refs[6], bbox=(lo, hi), cenv=cenv if ncenv else None,
                       ncenv=ncenv, nshape=nshape, ncluster=ncluster)
        elif otype == OBJ_REPLICA:
            obj['replica'] = _s32(r.d, o + 0x64)
        elif otype == OBJ_CAMERA:
            obj['camera'] = dict(zip(('pos', 'target'), (np.array(base[0:3]), np.array(base[3:6]))))
        out.append(obj)
    return out


def _materials(r):
    off, n = r.h['material']
    out = []
    for i in range(n):
        o = off + MATERIAL_SIZE * i
        name = _u32(r.d, o)
        pass_, vtx = struct.unpack_from('>HB', r.d, o + 8)
        lit = tuple(r.d[o + 11:o + 14])
        col = tuple(r.d[o + 14:o + 17])
        shadow = tuple(r.d[o + 17:o + 20])
        hilite, u18, inv_alpha, u20a, u20b, ref_alpha, u2c = struct.unpack_from('>7f', r.d, o + 0x14)
        flags, nattr, attr = struct.unpack_from('>III', r.d, o + 0x30)
        out.append(dict(name=r.name(name), pass_=pass_, vtx_mode=vtx, lit_color=lit, color=col, shadow_color=shadow,
                        inv_alpha=inv_alpha, ref_alpha=ref_alpha, flags=flags, attributes=r.sym(attr, nattr)))
    return out


def _attributes(r):
    off, n = r.h['attribute']
    out = []
    for i in range(n):
        o = off + ATTRIBUTE_SIZE * i
        name = _u32(r.d, o)
        k_color = struct.unpack_from('>f', r.d, o + 0x0C)[0]
        nbt = struct.unpack_from('>f', r.d, o + 0x14)[0]
        scale = struct.unpack_from('>2f', r.d, o + 0x28)
        trans = struct.unpack_from('>2f', r.d, o + 0x30)
        wrap_s, wrap_t = struct.unpack_from('>II', r.d, o + 0x64)
        max_lod, flag, bitmap = struct.unpack_from('>IIi', r.d, o + 0x78)
        out.append(dict(name=r.name(name) if name != 0xFFFFFFFF else None, k_color=k_color, nbt=nbt, scale=scale,
                        trans=trans, wrap=(wrap_s, wrap_t), flag=flag, bitmap=bitmap))
    return out


def _palettes(r):
    off, n = r.h['palette']
    base = off + PALETTE_SIZE * n
    out = []
    for i in range(n):
        name, _x, count, data = struct.unpack_from('>IiII', r.d, off + PALETTE_SIZE * i)
        out.append(dict(name=r.name(name), count=count, data=r.d[base + data:base + data + 2 * count]))
    return out


def _bitmaps(r, palettes):
    off, n = r.h['bitmap']
    base = off + BITMAP_SIZE * n
    heads = []
    for i in range(n):
        o = off + BITMAP_SIZE * i
        name, max_lod = struct.unpack_from('>II', r.d, o)
        fmt, pix = r.d[o + 8], r.d[o + 9]
        w, h, pal_size = struct.unpack_from('>hhh', r.d, o + 10)
        tint, pal, _u, data = struct.unpack_from('>IiII', r.d, o + 0x10)
        heads.append(dict(name=r.name(name), max_lod=max_lod, format=fmt, pix_size=pix, width=w, height=h,
                          pal_size=pal_size, tint=tint, palette=pal, offset=base + data))
    for b in heads:
        b['data'] = r.d[b['offset']:b['offset'] + W.gx_size(hsf_gx_format(b), b['width'], b['height'])]
        b['palette_data'] = palettes[b['palette']]['data'] if 0 <= b['palette'] < len(palettes) else None
    return heads


def hsf_gx_format(b):
    """(GX texture format, GX palette format or None) of an HSF bitmap record."""
    fmt = b['format']
    plain = {0: W.GX_I4, 1: W.GX_I8, 2: W.GX_IA4, 3: W.GX_IA8, 4: W.GX_RGB565, 5: W.GX_RGB5A3, 6: W.GX_RGBA8,
             7: W.GX_CMPR}
    if fmt in plain:
        return plain[fmt]
    if fmt in (9, 10, 11):
        return W.GX_C4 if b['pix_size'] < 8 else W.GX_C8
    raise ValueError('unknown HSF bitmap format %d' % fmt)


HSF_PALETTE_FORMAT = {9: W.TL_RGB565, 10: W.TL_RGB5A3, 11: W.TL_IA8}


def decode_bitmap(b) -> np.ndarray:
    """RGBA uint8 (h, w, 4) of one HSF bitmap."""
    gx = hsf_gx_format(b)
    pal = None
    if b['format'] in HSF_PALETTE_FORMAT:
        pdata = b['palette_data'] or b''
        if b['format'] == 11 and b['pix_size'] < 8:   # hsfdraw.c: CI_IA8 4 bpp skips (palSize+15)&~15 entries
            pdata = pdata[2 * ((b['pal_size'] + 0xF) & ~0xF):] or pdata
        pal = W.decode_palette(pdata, HSF_PALETTE_FORMAT[b['format']])
    return W.decode_gx(b['data'], b['width'], b['height'], gx, pal)


def _cenvs(r):
    off, n = r.h['cenv']
    base = off + CENV_SIZE * n
    heads = [struct.unpack_from('>9I', r.d, off + CENV_SIZE * i) for i in range(n)]
    weight_base = base + sum(12 * h[4] + 16 * h[5] + 16 * h[6] for h in heads)
    out = []
    for name, single, dual, multi, ns, nd, nm, vtx_count, copy_count in heads:
        e = dict(name=r.name(name), single=[], dual=[], multi=[], vtx_count=vtx_count, copy_count=copy_count)
        for k in range(ns):
            t, p, pn, q, qn = struct.unpack_from('>IHHHH', r.d, base + single + 12 * k)
            e['single'].append(dict(target=t, pos=p, pos_num=pn, nrm=q, nrm_num=qn))
        for k in range(nd):
            t1, t2, nw, wo = struct.unpack_from('>IIII', r.d, base + dual + 16 * k)
            ws = []
            for j in range(nw):
                w, p, pn, q, qn = struct.unpack_from('>fHHHH', r.d, weight_base + wo + 12 * j)
                ws.append(dict(weight=w, pos=p, pos_num=pn, nrm=q, nrm_num=qn))
            e['dual'].append(dict(target1=t1, target2=t2, weights=ws))
        for k in range(nm):
            nw, p, pn, q, qn, wo = struct.unpack_from('>IHHHHI', r.d, base + multi + 16 * k)
            ws = [struct.unpack_from('>If', r.d, weight_base + wo + 8 * j) for j in range(nw)]
            e['multi'].append(dict(pos=p, pos_num=pn, nrm=q, nrm_num=qn, weights=ws))
        out.append(e)
    return out


def _skeleton(r):
    off, n = r.h['skeleton']
    out = {}
    for i in range(n):
        o = off + SKELETON_SIZE * i
        out[r.name(_u32(r.d, o))] = _transform(struct.unpack_from('>9f', r.d, o + 4))
    return out


def _motions(r):
    off, n = r.h['motion']
    if not n:
        return []
    out = []
    track_base = off + MOTION_SIZE * n
    heads = [struct.unpack_from('>IIIf', r.d, off + MOTION_SIZE * i) for i in range(n)]
    total = sum(h[1] for h in heads)
    key_base = track_base + TRACK_SIZE * total
    t0 = 0
    for name, ntracks, _track, max_time in heads:
        tracks = []
        for k in range(ntracks):
            o = track_base + TRACK_SIZE * (t0 + k)
            ttype, start, target, index, channel, curve, nkeys = struct.unpack_from('>BBHHHHH', r.d, o)
            raw = r.d[o + 12:o + 16]
            tr = dict(type=ttype, target=r.name(target) if ttype not in (TRACK_MATERIAL,) else None,
                      target_raw=target, index=index, channel=channel, curve=curve, nkeys=nkeys)
            if curve == CURVE_CONST:
                tr['value'] = struct.unpack('>f', raw)[0]
                tr['keys'] = np.array([[0.0, tr['value']]])
            else:
                data = struct.unpack('>I', raw)[0]
                width = {CURVE_STEP: 2, CURVE_LINEAR: 2, CURVE_HERMITE: 4, CURVE_BITMAP: 2}.get(curve, 2)
                k = np.frombuffer(r.d, '>f4', width * nkeys, key_base + data).reshape(nkeys, width)
                if curve == CURVE_BITMAP:   # {f32 time, s32 bitmap}
                    k = np.frombuffer(r.d, '>f4', 2 * nkeys, key_base + data).reshape(nkeys, 2).astype(np.float64)
                    k[:, 1] = np.frombuffer(r.d, '>i4', 2 * nkeys, key_base + data).reshape(nkeys, 2)[:, 1]
                tr['keys'] = k.astype(np.float64)
            tracks.append(tr)
        out.append(dict(name=r.name(name), tracks=tracks, max_time=max_time))
        t0 += ntracks
    return out


def parse_hsf(data) -> dict[str, Any]:
    """Every section of an HSFV037 file (see the module docstring)."""
    r = _Reader(data)
    skinned = r.h['cenv'][1] > 0

    def normals(o, n):
        if skinned:
            return np.frombuffer(r.d, '>f4', 3 * n, o).reshape(n, 3).astype(np.float64)
        return np.frombuffer(r.d, 'i1', 3 * n, o).reshape(n, 3).astype(np.float64) / 64.0

    vertex, _ = _buffers(r, 'vertex', _vec3(r))
    normal, _ = _buffers(r, 'normal', normals)
    st, _ = _buffers(r, 'st', lambda o, n: np.frombuffer(r.d, '>f4', 2 * n, o).reshape(n, 2).astype(np.float64))
    color, _ = _buffers(r, 'color', lambda o, n: np.frombuffer(r.d, 'u1', 4 * n, o).reshape(n, 4).copy())
    palettes = _palettes(r)
    model = dict(
        header=r.h, vertex=vertex, normal=normal, st=st, color=color, face=_faces(r), objects=_objects(r),
        materials=_materials(r), attributes=_attributes(r), palettes=palettes, bitmaps=_bitmaps(r, palettes),
        cenv=_cenvs(r), skeleton=_skeleton(r), motions=_motions(r))
    by_name = {}
    for o in model['objects']:
        by_name.setdefault(o['name'], o['index'])
    model['by_name'] = by_name
    return model


# ---------------------------------------------------------------------------
# transforms
# ---------------------------------------------------------------------------
def _rx(a):
    c, s = math.cos(a), math.sin(a)
    return np.array([[1, 0, 0], [0, c, -s], [0, s, c]], dtype=np.float64)


def _ry(a):
    c, s = math.cos(a), math.sin(a)
    return np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]], dtype=np.float64)


def _rz(a):
    c, s = math.cos(a), math.sin(a)
    return np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]], dtype=np.float64)


def euler_matrix(r_deg):
    """Rz . Ry . Rx of an HSF rotation (degrees), column-vector convention."""
    rx, ry, rz = (math.radians(float(a)) for a in r_deg)
    return _rz(rz) @ _ry(ry) @ _rx(rx)


def local_matrix(T, R, S):
    """4x4 column-vector matrix T . Rz . Ry . Rx . S (EnvelopeExec.c SetMtx)."""
    m = np.eye(4)
    m[:3, :3] = euler_matrix(R) * np.asarray(S, dtype=np.float64)[None, :]
    m[:3, 3] = T
    return m


def rest_transform(model, obj):
    """The object's rest T/R/S: its skeleton entry when one has its name, else its base."""
    sk = model['skeleton'].get(obj['name'])
    return sk if sk is not None else obj['base']


def worlds_from_locals(model, locals_):
    """World matrices (column vectors) composing `locals_[i]` down the object hierarchy."""
    objs = model['objects']
    out = [None] * len(objs)

    def walk(i, parent_world):
        out[i] = parent_world @ locals_[i]
        for c in objs[i]['children']:
            if 0 <= c < len(objs) and out[c] is None:
                walk(c, out[i])

    for o in objs:
        if o['parent'] is None and out[o['index']] is None:
            walk(o['index'], np.eye(4))
    for i, w in enumerate(out):   # unreachable objects (should not happen)
        if w is None:
            out[i] = locals_[i]
    return out


def rest_locals(model):
    return [local_matrix(**rest_transform(model, o)) for o in model['objects']]


def rest_worlds(model):
    return worlds_from_locals(model, rest_locals(model))


# ---------------------------------------------------------------------------
# meshes
# ---------------------------------------------------------------------------
def mesh_vertex_weights(model, obj) -> list[list[tuple[int, float]]]:
    """Per position of the mesh's vertex buffer: [(object index, weight)] from its envelopes
    (an unskinned mesh, or a position no envelope names, rides the mesh object itself)."""
    nv = model['vertex'][obj['vertex']]['count'] if obj['vertex'] >= 0 else 0
    weights: list[list[tuple[int, float]]] = [[] for _ in range(nv)]
    if obj.get('cenv') is None:
        return [[(obj['index'], 1.0)] for _ in range(nv)]
    for e in model['cenv'][obj['cenv']:obj['cenv'] + obj['ncenv']]:
        for s in e['single']:
            for p in range(s['pos'], s['pos'] + s['pos_num']):
                weights[p] = [(s['target'], 1.0)]
        for d in e['dual']:
            for w in d['weights']:
                for p in range(w['pos'], w['pos'] + w['pos_num']):
                    weights[p] = [(d['target1'], w['weight']), (d['target2'], 1.0 - w['weight'])]
        for m in e['multi']:
            for p in range(m['pos'], m['pos'] + m['pos_num']):
                weights[p] = [(t, w) for t, w in m['weights']]
    for p, ws in enumerate(weights):
        if not ws:
            weights[p] = [(obj['index'], 1.0)]
    return weights


def mesh_corners(model, obj):
    """(triangles as corner index triples, corners [(pos, nrm, col, st)], material per triangle)
    of one mesh object, strips and quads triangulated in GX order."""
    if obj.get('face', -1) < 0:
        return [], [], []
    corners, tris, mats = [], [], []
    for f in model['face'][obj['face']]['faces']:
        seq = f['corners']
        base = len(corners)
        corners.extend(seq)
        if f['type'] == FACE_TRI:
            tris.append((base, base + 1, base + 2))
            mats.append(f['material'])
        elif f['type'] == FACE_QUAD:
            tris.extend([(base, base + 1, base + 2), (base, base + 2, base + 3)])
            mats.extend([f['material']] * 2)
        elif f['type'] == FACE_STRIP:
            for k in range(len(seq) - 2):
                tri = (base + k, base + k + 1, base + k + 2) if k % 2 == 0 else (base + k + 1, base + k, base + k + 2)
                tris.append(tri)
                mats.append(f['material'])
    return tris, corners, mats


def mesh_bitmap(model, material_index):
    """The first bitmap index of a material (via its first attribute), or None."""
    if not (0 <= material_index < len(model['materials'])):
        return None
    for a in model['materials'][material_index]['attributes']:
        if 0 <= a < len(model['attributes']):
            b = model['attributes'][a]['bitmap']
            if 0 <= b < len(model['bitmaps']):
                return b
    return None


def skinned_positions(model, obj, worlds, rest=None):
    """The mesh object's positions under `worlds` (a full per-object world list): v' = sum_j w_j
    C_j W_j^-1 W_mesh v (EnvelopeExec.c). Returns (n, 3)."""
    rest = rest if rest is not None else rest_worlds(model)
    pos = model['vertex'][obj['vertex']]['data']
    ph = np.c_[pos, np.ones(len(pos))]
    wm = rest[obj['index']]
    out = np.zeros((len(pos), 3))
    cache = {}
    for p, ws in enumerate(mesh_vertex_weights(model, obj)):
        acc = np.zeros(3)
        for j, w in ws:
            if j not in cache:
                cache[j] = worlds[j] @ np.linalg.inv(rest[j]) @ wm
            acc += w * (cache[j] @ ph[p])[:3]
        out[p] = acc
    return out


# ---------------------------------------------------------------------------
# motion evaluation (hsfmotion.c GetCurve)
# ---------------------------------------------------------------------------
def curve_value(track, t):
    """One channel at time t (frames) -- sample_curve on a single time."""
    return float(sample_curve(track, [t])[0])


def object_tracks(motion) -> dict[str, dict[int, dict]]:
    """name -> channel -> track for the object-transform tracks of one motion."""
    out: dict[str, dict[int, dict]] = {}
    for tr in motion['tracks']:
        if tr['type'] == TRACK_TRANSFORM and tr['target'] is not None:
            out.setdefault(tr['target'], {})[tr['channel']] = tr
    return out


def posed_transform(rest, tracks, t):
    """T/R/S of one object at motion time t: rest values with every animated channel replaced."""
    vals = list(rest['T']) + list(rest['R']) + list(rest['S'])
    for ch, tr in (tracks or {}).items():
        slot = CHANNEL_SLOT.get(ch)
        if slot is not None:
            vals[slot] = curve_value(tr, t)
    return dict(T=np.array(vals[0:3]), R=np.array(vals[3:6]), S=np.array(vals[6:9]))


def posed_worlds(model, motion, t):
    """Per-object world matrices of `model` driven by `motion` at time t (frames). Channels a
    motion does not animate keep the object's rest value (Hu3DMotionExec resets curr = base)."""
    tracks = object_tracks(motion) if motion else {}
    locs = [local_matrix(**posed_transform(rest_transform(model, o), tracks.get(o['name']), t))
            for o in model['objects']]
    return worlds_from_locals(model, locs)


def motion_length(motion):
    """Frames of a motion: maxTime, else the last key time."""
    if motion['max_time'] > 0:
        return float(motion['max_time'])
    return max((float(tr['keys'][-1, 0]) for tr in motion['tracks'] if len(tr['keys'])), default=0.0)


# ---------------------------------------------------------------------------
# DDR World conversion (port_character_hottest.py / port_stage_hottest.py)
# ---------------------------------------------------------------------------
# A dancer's file pose puts the Hips at 133.4 units: one uniform scale lands them at World's
# 0.97 m (the SuperNova ports' convention). The file frame is already World's (Y up, facing +Z,
# left at +X: LeftArm at +x, the toes at +z), so nothing else changes.
HIPS_UNITS = 133.4
GAME_SCALE = 0.970 / HIPS_UNITS
JOINT_TYPE = OBJ_JOINT
ROLE_ALIASES = {'Spine2': 'Spine1'}   # World role bone -> the HSF joint playing it


def rig_joints(model):
    """[(joint name, parent joint name or None)], parents first: every type-4 object, its
    parent the nearest type-4 ancestor (MayaConverter's `*root` / `*leaf` helpers between two
    joints are identity transforms and are dropped). The file lists children first."""
    objs = model['objects']
    out = []
    for o in objs:
        if o['type'] != JOINT_TYPE:
            continue
        p = o['parent']
        while p is not None and objs[p]['type'] != JOINT_TYPE:
            p = objs[p]['parent']
        out.append((o['name'], objs[p]['name'] if p is not None else None))
    parent_of = dict(out)

    def depth(n):
        return 0 if parent_of[n] is None else depth(parent_of[n]) + 1

    order = {n: i for i, (n, _p) in enumerate(out)}
    return sorted(out, key=lambda jp: (depth(jp[0]), order[jp[0]]))


def _scaled_row(m, s):
    """Column-vector world matrix in file units -> ROW-vector matrix in metres (rotation kept)."""
    S, Si = np.diag([s, s, s, 1.0]), np.diag([1 / s, 1 / s, 1 / s, 1.0])
    return (S @ m @ Si).T


def game_bind_matrices(model, s=GAME_SCALE):
    """{joint: 4x4 ROW-vector bind (joint local -> game world, metres)} from the rest worlds."""
    rest = rest_worlds(model)
    return {n: _scaled_row(rest[model['by_name'][n]], s) for n, _p in rig_joints(model)}


def _joint_of(model, index):
    """The rig joint a skin target object belongs to (itself or the nearest joint ancestor)."""
    objs = model['objects']
    i = index
    while i is not None and objs[i]['type'] != JOINT_TYPE:
        i = objs[i]['parent']
    return objs[i]['name'] if i is not None else None


def game_mesh(model, s=GAME_SCALE, skip_bitmaps=()):
    """Every mesh object joined, in game space at rest: (positions (n, 3) metres, normals,
    uv (v down), colours RGBA 0..1 or None, [(joint, weight)] per vertex, triangles, material
    index per triangle, source mesh object index per triangle). HSF indexes position, normal,
    colour and st separately per face corner; one output vertex per distinct
    (position, normal, colour, st) tuple of a mesh -- the GX vertex. Triangles whose material's
    bitmap is in `skip_bitmaps` are left out. Weights name the rig joint of each skin target."""
    rest = rest_worlds(model)
    P_, N_, UV_, C_, W_, T_, M_, O_ = [], [], [], [], [], [], [], []
    any_colour = False
    for o in model['objects']:
        if o['type'] != OBJ_MESH or o['vertex'] < 0 or o['face'] < 0:
            continue
        tris, corners, mats = mesh_corners(model, o)
        if not corners:
            continue
        pos = skinned_positions(model, o, rest, rest)
        weights = mesh_vertex_weights(model, o)
        nrm_buf = model['normal'][o['normal']]['data'] if o['normal'] >= 0 else None
        st_buf = model['st'][o['st']]['data'] if o['st'] >= 0 else None
        col_buf = model['color'][o['color']]['data'] if o['color'] >= 0 else None
        wm = rest[o['index']][:3, :3]
        index = {}
        remap = []
        for c in corners:
            key = tuple(c)
            if key not in index:
                index[key] = len(P_)
                P_.append(pos[c[0]] * s)
                n = wm @ nrm_buf[c[1]] if nrm_buf is not None and 0 <= c[1] < len(nrm_buf) else np.array([0, 1.0, 0])
                N_.append(n / (np.linalg.norm(n) or 1.0))
                UV_.append(st_buf[c[3]] if st_buf is not None and 0 <= c[3] < len(st_buf) else (0.0, 0.0))
                if col_buf is not None and 0 <= c[2] < len(col_buf):
                    C_.append(col_buf[c[2]] / 255.0)
                    any_colour = True
                else:
                    C_.append(np.ones(4))
                ws = {}
                for j, w in weights[c[0]]:
                    jn = _joint_of(model, j)
                    if jn is not None:
                        ws[jn] = ws.get(jn, 0.0) + w
                W_.append(sorted(ws.items(), key=lambda kv: -kv[1]))
            remap.append(index[key])
        for t, mt in zip(tris, mats):
            if mesh_bitmap(model, mt) in skip_bitmaps:
                continue
            a, b, c = remap[t[0]], remap[t[1]], remap[t[2]]
            if a == b or b == c or a == c:   # degenerate strip joints
                continue
            T_.append((a, b, c))
            M_.append(mt)
            O_.append(o['index'])
    return (np.array(P_), np.array(N_), np.array(UV_, dtype=np.float64), np.array(C_) if any_colour else None,
            W_, np.array(T_, dtype=np.int64).reshape(-1, 3), np.array(M_, dtype=np.int64), np.array(O_, dtype=np.int64))


def consistent_winding(pos, nrm, tris):
    """Flip each triangle whose geometric normal opposes its corners' mean normal (GX culls by
    the strip parity; World by the triangle order). Returns (triangles, flipped count)."""
    t = np.asarray(tris).copy()
    if not len(t):
        return t, 0
    g = np.cross(pos[t[:, 1]] - pos[t[:, 0]], pos[t[:, 2]] - pos[t[:, 0]])
    n = nrm[t[:, 0]] + nrm[t[:, 1]] + nrm[t[:, 2]]
    flip = (g * n).sum(1) < 0
    t[flip] = t[flip][:, [0, 2, 1]]
    return t, int(flip.sum())


def clip_game_worlds(model, motion, names, frames, s=GAME_SCALE, time_of=None):
    """(frames x joints x 4 x 4) ROW-vector game-space worlds of `names` driven by `motion`
    at output frames `frames` (source time = time_of(frame), default the frame itself)."""
    idx = [model['by_name'][n] for n in names]
    out = np.empty((len(frames), len(names), 4, 4))
    for k, f in enumerate(frames):
        w = posed_worlds(model, motion, time_of(f) if time_of else f)
        for b, i in enumerate(idx):
            out[k, b] = _scaled_row(w[i], s)
    return out


def worlds_to_anm_spec(worlds, names, parents, target_binds, binds, times, frame_count, root_shift=None):
    """A scripts/anm_dump.py::write_anm spec from per-frame game worlds (`worlds[k][b]`, the
    rig's own bind frames `binds[name]`) re-expressed against the EXPORTED binds
    (`target_binds[b]`, Q = B_target . B_game^-1). Rotation = kind 0x1C, translation = 0x1D,
    one key when a channel never changes; `root_shift` (x, z in metres) is subtracted from
    the root bones' translation. Returns (spec, exported-frame worlds)."""
    import tzm_dump as Z   # rowmat_to_quat
    q = [np.asarray(target_binds[b]) @ np.linalg.inv(binds[n]) for b, n in enumerate(names)]
    wq = np.einsum('bij,fbjk->fbik', np.array(q), worlds)
    if root_shift is not None:
        wq = wq.copy()
        wq[:, :, 3, 0] -= root_shift[0]
        wq[:, :, 3, 2] -= root_shift[1]
    tracks = []
    for b in range(len(names)):
        p = parents[b]
        locs = wq[:, b] if p < 0 else np.einsum('fij,fjk->fik', wq[:, b], np.linalg.inv(wq[:, p]))
        quats, prev = [], None
        for m in locs:
            qv = Z.rowmat_to_quat(m[:3, :3])
            if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
                qv = tuple(-c for c in qv)
            quats.append(qv)
            prev = qv
        trans = [tuple(float(x) for x in m[3, :3]) for m in locs]
        qa, ta = np.array(quats), np.array(trans)
        tracks.append(dict(kind=0x1C, target=b, keys=[quats[0]]) if np.abs(qa - qa[0]).max() < 1e-6
                      else dict(kind=0x1C, target=b, times=list(times), keys=quats))
        tracks.append(dict(kind=0x1D, target=b, keys=[trans[0]]) if np.abs(ta - ta[0]).max() < 1e-5
                      else dict(kind=0x1D, target=b, times=list(times), keys=trans))
    return dict(frame_count=frame_count, flag=0, hierarchy=list(parents), tracks=tracks), wq


def sample_curve(track, times):
    """A channel at an array of times (frames), with the game's key search (hsfmotion.c): keys
    are NOT necessarily sorted -- MayaConverter puts pre-roll keys at negative times after key 0
    -- so the segment is found by scanning, not bisecting.
      t == 0 or one key: key 0's value;
      step (GetConstant): the value of the key before the first key with t < time;
      linear (GetLinear): interpolate the key before the first key with t < time and that key;
      Hermite (GetBezier): the first segment (k[i-1], k[i]) with k[i-1].time <= t < k[i].time,
      else the first key with t < time; h(s) = v0 (2s^3 - 3s^2 + 1) + v1 (3s^2 - 2s^3)
      + out0 (s^3 - 2s^2 + s) + in1 (s^3 - s^2), the slopes not scaled by the segment length;
      past the last key: the last key's value. Constants carry their value inline."""
    t = np.asarray(times, dtype=np.float64)
    if track['curve'] == CURVE_CONST:
        return np.full(t.shape, float(track['value']))
    k = track['keys']
    n = len(k)
    if n == 0:
        return np.zeros(t.shape)
    out = np.full(t.shape, float(k[-1, 1]))
    if n == 1:
        return np.full(t.shape, float(k[0, 1]))
    kt = k[:, 0]
    before = t[None, :] < kt[:, None]                       # (n, F): t < key time
    first = np.where(before.any(0), before.argmax(0), n)    # first key with t < time
    if track['curve'] == CURVE_HERMITE:
        seg = t[None, :] >= kt[:-1, None]
        seg = seg & before[1:]                              # k[i-1] <= t < k[i], i >= 1
        hit = seg.any(0)
        i = np.where(hit, seg.argmax(0) + 1, first)
    else:
        i = first
    ok = (i > 0) & (i < n)
    if ok.any():
        ii = i[ok]
        a, b, tm = k[ii - 1], k[ii], t[ok]
        if track['curve'] == CURVE_HERMITE:
            x = (tm - a[:, 0]) / (b[:, 0] - a[:, 0])
            x2, x3 = x * x, x * x * x
            out[ok] = (a[:, 1] * (2 * x3 - 3 * x2 + 1) + b[:, 1] * (-2 * x3 + 3 * x2) + a[:, 2] * (x3 - 2 * x2 + x)
                       + b[:, 3] * (x3 - x2))
        elif track['curve'] == CURVE_LINEAR:
            out[ok] = a[:, 1] + (tm - a[:, 0]) * (b[:, 1] - a[:, 1]) / (b[:, 0] - a[:, 0])
        else:   # step: hold the previous key
            out[ok] = a[:, 1]
    out[i == 0] = k[0, 1]
    out[t == 0.0] = k[0, 1]
    return out


def _euler_stack(rx, ry, rz):
    cx, sx, cy, sy, cz, sz = np.cos(rx), np.sin(rx), np.cos(ry), np.sin(ry), np.cos(rz), np.sin(rz)
    m = np.empty(rx.shape + (3, 3))
    # Rz . Ry . Rx
    m[..., 0, 0] = cz * cy
    m[..., 0, 1] = cz * sy * sx - sz * cx
    m[..., 0, 2] = cz * sy * cx + sz * sx
    m[..., 1, 0] = sz * cy
    m[..., 1, 1] = sz * sy * sx + cz * cx
    m[..., 1, 2] = sz * sy * cx - cz * sx
    m[..., 2, 0] = -sy
    m[..., 2, 1] = cy * sx
    m[..., 2, 2] = cy * cx
    return m


def joint_worlds(model, motion, times):
    """{joint: (F, 4, 4) column-vector world in file units} of the rig joints driven by `motion`
    at `times` -- posed_worlds restricted to rig_joints (the helpers between joints are identity
    and unanimated), vectorized over time."""
    t = np.asarray(times, dtype=np.float64)
    tracks = object_tracks(motion) if motion else {}
    objs = model['objects']
    rest = rest_worlds(model)
    out = {}
    joints = rig_joints(model)
    depth = {}
    parent_of = dict(joints)

    def d(n):
        if n not in depth:
            depth[n] = 0 if parent_of[n] is None else d(parent_of[n]) + 1
        return depth[n]

    for name, parent in sorted(joints, key=lambda jp: d(jp[0])):
        o = objs[model['by_name'][name]]
        r = rest_transform(model, o)
        vals = [np.full(t.shape, float(v)) for v in list(r['T']) + list(r['R']) + list(r['S'])]
        for ch, tr in tracks.get(name, {}).items():
            if ch in CHANNEL_SLOT:
                vals[CHANNEL_SLOT[ch]] = sample_curve(tr, t)
        loc = np.zeros(t.shape + (4, 4))
        loc[..., :3, :3] = _euler_stack(*(np.radians(v) for v in vals[3:6])) * np.stack(vals[6:9], -1)[..., None, :]
        loc[..., :3, 3] = np.stack(vals[0:3], -1)
        loc[..., 3, 3] = 1.0
        if parent is None:
            # whatever sits above the root joint (Reference, *root helpers) at rest
            above = rest[o['index']] @ np.linalg.inv(local_matrix(**r))
            out[name] = above @ loc
        else:
            out[name] = np.einsum('fij,fjk->fik', out[parent], loc)
    return out


def chain_clips(motions, tol=1.0, model=None):
    """Group consecutive motions whose last pose equals the next one's first pose (within `tol`
    file units on every joint position -- MayaConverter split continuous takes into pieces).
    `motions` is [(id, motion)]; returns [[id, ...]]."""
    if model is None:
        raise ValueError('chain_clips needs the body model to pose')
    joints = [n for n, _p in rig_joints(model)]
    idx = [model['by_name'][n] for n in joints]

    def pose(mo, t):
        w = posed_worlds(model, mo, t)
        return np.array([w[i][:3, 3] for i in idx])

    chains = []
    prev_end = None
    for mid, mo in motions:
        start, end = pose(mo, 0.0), pose(mo, motion_length(mo))
        if chains and prev_end is not None and np.abs(prev_end - start).max() < tol:
            chains[-1].append(mid)
        else:
            chains.append([mid])
        prev_end = end
    return chains


# ---------------------------------------------------------------------------
# stages (port_stage_hottest.py)
# ---------------------------------------------------------------------------
# A stage pack data/stgNN.bin is a list of (model, motion) pairs -- the motion of entry 2k+1
# animates the model of entry 2k (one loop, its own length) -- then a `chr*` / `look*` marker
# model (the dancers' formation spots, an 8x8 `none` texture) and six camera motions
# (`cameraShape1` + `camera1*aim`, 180 frames each).
CAM_POS = (8, 9, 10)
CAM_AIM = (11, 12, 13)
CAM_ROLL = 14
CAM_FOV = 15        # vertical, degrees (MTXPerspective)
CAM_NEAR, CAM_FAR = 17, 18
ATTR_UV = (8, 9)    # attribute T x / y: the texture matrix translates by -T (hsfdraw.c ANIM3D)
MATERIAL_LIT = (0, 1, 2)   # material litColor r / g / b (0..1)


def topo_objects(model):
    """Object indices, parents first."""
    objs = model['objects']
    depth = {}

    def d(i):
        if i not in depth:
            p = objs[i]['parent']
            depth[i] = 0 if p is None else d(p) + 1
        return depth[i]

    return sorted(range(len(objs)), key=lambda i: (d(i), i))


def object_worlds_series(model, motion, times, unit_scale=False):
    """(F, objects, 4, 4) column-vector worlds of every object at `times` (frames) under
    `motion` (None = rest). `unit_scale` composes the same chain with every scale set to 1 (the
    rotation frame of a flattened, zero-scaled prop stays proper)."""
    t = np.asarray(times, dtype=np.float64)
    tracks = object_tracks(motion) if motion else {}
    objs = model['objects']
    out = np.zeros((len(t), len(objs), 4, 4))
    for i in topo_objects(model):
        o = objs[i]
        r = rest_transform(model, o)
        vals = [np.full(t.shape, float(v)) for v in list(r['T']) + list(r['R']) + list(r['S'])]
        for ch, tr in tracks.get(o['name'], {}).items():
            if ch in CHANNEL_SLOT:
                vals[CHANNEL_SLOT[ch]] = sample_curve(tr, t)
        loc = np.zeros(t.shape + (4, 4))
        sc = np.ones(t.shape + (3,)) if unit_scale else np.stack(vals[6:9], -1)
        loc[..., :3, :3] = _euler_stack(*(np.radians(v) for v in vals[3:6])) * sc[..., None, :]
        loc[..., :3, 3] = np.stack(vals[0:3], -1)
        loc[..., 3, 3] = 1.0
        p = o['parent']
        out[:, i] = loc if p is None else np.einsum('fij,fjk->fik', out[:, p], loc)
    return out


def animated_objects(model, motion, length=None, samples=97, tol=1e-5):
    """Indices of the objects whose own local transform changes over the loop (a constant track
    only re-poses its object -- bake the geometry at the frame-0 pose, see
    object_worlds_series)."""
    if not motion:
        return set()
    length = motion_length(motion) if length is None else length
    t = np.linspace(0.0, max(length, 1.0), samples)
    tracks = object_tracks(motion)
    out = set()
    for o in model['objects']:
        for ch, tr in tracks.get(o['name'], {}).items():
            if ch not in CHANNEL_SLOT:
                continue
            v = sample_curve(tr, t)
            if np.abs(v - v[0]).max() > tol:
                out.add(o['index'])
                break
    return out


def is_marker_model(model):
    """The dancers' formation markers (`chr*N*M` / `look*N*M` meshes) or a camera holder."""
    meshes = [o['name'] for o in model['objects'] if o['type'] == OBJ_MESH]
    return any(o['type'] == OBJ_CAMERA for o in model['objects']) or (
        bool(meshes) and all(n.startswith(('chr', 'look')) for n in meshes))


def is_camera_motion(motion):
    return bool(motion) and any(tr['target'] == 'cameraShape1' for tr in motion['tracks'] if tr['type'] == TRACK_TRANSFORM)


def camera_samples(motion, times, name='cameraShape1'):
    """(pos (F, 3), aim (F, 3), roll degrees (F), vertical fov degrees (F), near, far) in file
    units of a camera motion (HSFCAMERA: pos, target, upRot, fov, near, far)."""
    t = np.asarray(times, dtype=np.float64)
    tr = object_tracks(motion).get(name, {})

    def ch(c, default):
        return sample_curve(tr[c], t) if c in tr else np.full(t.shape, float(default))

    pos = np.stack([ch(c, 0.0) for c in CAM_POS], -1)
    aim = np.stack([ch(c, 0.0) for c in CAM_AIM], -1)
    near = float(ch(CAM_NEAR, 1.0)[0])
    far = float(ch(CAM_FAR, 100000.0)[0])
    return pos, aim, ch(CAM_ROLL, 0.0), ch(CAM_FOV, 45.0), near, far


def attribute_tracks(motion):
    """{attribute index: {channel: track}} (type 10; the target string is -1, `index` names the
    attribute)."""
    out: dict[int, dict[int, dict]] = {}
    for tr in (motion or {}).get('tracks', []):
        if tr['type'] == TRACK_ATTRIBUTE:
            out.setdefault(tr['index'], {})[tr['channel']] = tr
    return out


def material_color_tracks(motion):
    """{material index: {channel: track}} (type 9)."""
    out: dict[int, dict[int, dict]] = {}
    for tr in (motion or {}).get('tracks', []):
        if tr['type'] == TRACK_MATERIAL:
            out.setdefault(tr['index'], {})[tr['channel']] = tr
    return out


def material_kind(material):
    """The World blend group of an HSF material: 'add' (ADDCOL), 'sub' (INVCOL), 'ble'
    (translucent pass: pass & 0xF or invAlpha), else 'dec' (opaque, alpha-tested)."""
    if material['flags'] & MATERIAL_FLAG_ADDCOL:
        return 'add'
    if material['flags'] & MATERIAL_FLAG_INVCOL:
        return 'sub'
    if (material['pass_'] & 0xF) or material['inv_alpha'] > 0:
        return 'ble'
    return 'dec'


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def _tilde(p):
    home = os.path.expanduser('~')
    return '~' + p[len(home):] if p.startswith(home) else p


def cmd_info(a):
    m = parse_hsf(open(a.file, 'rb').read())
    print({k: v[1] for k, v in m['header'].items() if v[1]})
    for o in m['objects']:
        extra = ''
        if o['type'] == OBJ_MESH:
            nv = m['vertex'][o['vertex']]['count'] if o['vertex'] >= 0 else 0
            nf = len(m['face'][o['face']]['faces']) if o['face'] >= 0 else 0
            extra = ' verts %d faces %d cenv %s' % (nv, nf, o['ncenv'])
        print('%3d %-8s %-40s parent %s%s' % (o['index'], OBJECT_TYPES.get(o['type'], o['type']), o['name'],
                                              o['parent'], extra))
    for b in m['bitmaps']:
        print('bitmap', b['name'], b['format'], b['pix_size'], b['width'], b['height'])
    for mo in m['motions']:
        kinds = {}
        for tr in mo['tracks']:
            kinds[(tr['type'], tr['curve'])] = kinds.get((tr['type'], tr['curve']), 0) + 1
        print('motion', mo['name'], len(mo['tracks']), 'tracks, max_time', mo['max_time'], kinds)


def cmd_tree(a):
    m = parse_hsf(open(a.file, 'rb').read())
    objs = m['objects']

    def walk(i, depth):
        o = objs[i]
        print('  ' * depth + '%s [%s]' % (o['name'], OBJECT_TYPES.get(o['type'], o['type'])))
        for c in o['children']:
            walk(c, depth + 1)

    for o in objs:
        if o['parent'] is None:
            walk(o['index'], 0)


def cmd_png(a):
    m = parse_hsf(open(a.file, 'rb').read())
    os.makedirs(a.out, exist_ok=True)
    for i, b in enumerate(m['bitmaps']):
        rgba = decode_bitmap(b)
        stem = '%02d_%s' % (i, W.safe_name(b['name'] or 'bitmap'))
        W.write_png(os.path.join(a.out, stem + '.png'), b['width'], b['height'], rgba.tobytes())
    print('%d bitmaps -> %s' % (len(m['bitmaps']), _tilde(a.out)))


def survey(paths):
    """Parse every .hsf under `paths`: decode bitmaps, skin meshes at rest, evaluate motions.
    Returns (files, problems)."""
    files, problems = 0, []
    for root in paths:
        for dirpath, _dirs, names in os.walk(root):
            for nm in sorted(names):
                if not nm.endswith('.hsf'):
                    continue
                path = os.path.join(dirpath, nm)
                files += 1
                try:
                    m = parse_hsf(open(path, 'rb').read())
                    for b in m['bitmaps']:
                        decode_bitmap(b)
                    rest = rest_worlds(m)
                    for o in m['objects']:
                        if o['type'] == OBJ_MESH and o['vertex'] >= 0:
                            mesh_corners(m, o)
                            if o.get('cenv') is not None:
                                skinned_positions(m, o, rest, rest)
                    for mo in m['motions']:
                        for tr in mo['tracks']:
                            curve_value(tr, motion_length(mo) * 0.5)
                except (ValueError, IndexError, struct.error, KeyError) as e:
                    problems.append((path, repr(e)))
    return files, problems


def cmd_survey(a):
    files, problems = survey(a.dirs)
    for p, e in problems:
        print('PROBLEM', _tilde(p), e)
    print('%d HSF files, %d problems' % (files, len(problems)))
    return 1 if problems else 0


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('info')
    p.add_argument('file')
    p.set_defaults(fn=cmd_info)
    p = sub.add_parser('tree')
    p.add_argument('file')
    p.set_defaults(fn=cmd_tree)
    p = sub.add_parser('png')
    p.add_argument('file')
    p.add_argument('out')
    p.set_defaults(fn=cmd_png)
    p = sub.add_parser('survey')
    p.add_argument('dirs', nargs='+')
    p.set_defaults(fn=cmd_survey)
    a = ap.parse_args(argv)
    return a.fn(a) or 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
