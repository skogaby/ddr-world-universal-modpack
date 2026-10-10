#!/usr/bin/env python3
"""Reference decoders for the 3D background dancers of Konami's Dance Dance Revolution for
Windows (KCEA, 2002 -- "DDR PC"), the Direct3D 8 port of the System 573 dancer engine, and a
converter of their baked motion into World-space joint matrices for the Background Dancers port
(tools/blender_ddr_addon/examples/port_character_ddrpc.py).

Formats and the evidence behind every rule: docs/ddr_pc_dancers_research.md. In short, the PC
build compiled the 573 content INTO its executables as C data:

  DanceDanceRevolution.exe   .data holds the 24 built-in characters (a table of 44-byte
                             `dll_header` records), every dance routine as baked per-frame
                             4x3 joint matrices, and the offset table of data.bin
  data.bin                   concatenated files (BMP textures, ...) indexed by that table
  character_dll/<name>.dll   a downloadable character: exports `dll_header`, the same record
  character_dll/<name>.bin   its icon / portrait / splash / texture BMPs, back to back

`dll_header` (11 x u32):
  0 id (0..47)  1 sex (0 M, 1 F)  2 float model scale  3 -> char*[ntex] texture names
  4 -> u32[ntex] texture source (built-in: data.bin file id; DLL: index into the .bin table)
  5 -> model  6 -> u32[] .bin member offsets (built-in: 0)  7 -> ".bin" name (built-in: 0)
  8 portrait (built-in: data.bin id)  9 (0)  10 splash (built-in: data.bin id)

model: {u32 ntex, nmat, nvert, ntri, njoint (=16), -> mat[nmat] (76 B, unused D3DMATERIAL8 +
  u32 nframes + -> u32[nframes] texture indices), -> vert[nvert] (D3DFVF 0x152: float3 pos,
  float3 normal, D3DCOLOR diffuse, float2 uv; 36 B), -> u32[njoint+1] vertex start per joint,
  -> u16[3*ntri] indices, -> u32[nmat+1] index start per material}, then char[16][ntex] names.
  Vertices are joint-local; a joint's vertices are transformed by its motion matrix
  (ProcessVertices, FUN_004c1d60) and drawn per material (one SetTexture: frame 0 or the
  blink/mouth frame the state machine picked; D3DCOLORKEY 0xFFF800F8 = transparent).
  Lighting: D3DRS_DIFFUSEMATERIALSOURCE = COLOR1, texture stage MODULATE(TEXTURE, DIFFUSE),
  SetMaterial is never called: the shaded colour is the VERTEX diffuse (the material table is
  dead data), times the texture.

motion: a routine is a 0-terminated list of clips; clip = {u32 nframes (60), u32 njoint (16),
  -> float[nframes][16][12] frames} (4x3 D3D row matrices: three basis rows + translation),
  one clip = one measure, 60 frames per measure. 18 routines (M_/F_/MF_ prefixed: the sex that
  plays them); the per-character playlists are two 8-row tables indexed by `id & 7`.

Coordinates: D3D Y-up, left-handed, the model faces -Z. World output is Y-up facing +Z,
metres (0.1 per unit), via the mirror (x, y, z) -> (x, y, -z); that mirror turns D3D's clockwise
front faces into World's counter-clockwise ones, so the index order is kept.

Usage:
    ddrpc_dancer_dump.py info    <game dir> [<character_dll dir>]       # characters, routines
    ddrpc_dancer_dump.py survey  <game dir> [<character_dll dir>]       # parse + check everything
    ddrpc_dancer_dump.py obj     <game dir> <name> <out.obj> [--dll-dir D] [--routine R --frame N]
    ddrpc_dancer_dump.py textures <game dir> <name> <out dir> [--dll-dir D]

`<game dir>` holds DanceDanceRevolution.exe + data.bin (an installed game, or the Data.Cab
members `F74707_DanceDanceRevolution.exe` / `F91401_data.bin` renamed). `<character_dll dir>`
holds the <name>.dll / <name>.bin pairs of the downloadable characters.

Import-safe: `from ddrpc_dancer_dump import Game, load_dll, world_mesh, routine_worlds`.
Needs numpy only.
"""
import argparse
import glob
import os
import re
import struct
import sys
from typing import Any

import numpy as np

EXE_NAMES = ('DanceDanceRevolution.exe', 'F74707_DanceDanceRevolution.exe')
DATA_NAMES = ('data.bin', 'F91401_data.bin')
HEADER_WORDS = 11
MODEL_WORDS = 10
VERT_SIZE = 36
MAT_SIZE = 76
FRAMES_PER_MEASURE = 60
COLOR_KEY = (0xF8, 0x00, 0xF8)  # D3DCOLORKEY 0xFFF800F8 (FUN_00401bd0)
NJOINT = 16
MAX_CHARS = 48

# PC joint order, named by what the baked rest matrices and the joint-local meshes show (facing -Z
# in a left-handed frame, +x is the dancer's LEFT; docs/ddr_pc_dancers_research.md §3).
JOINT_NAMES = ['chest', 'head', 'hips', 'upperarm_R', 'foot_R', 'forearm_R', 'shin_R', 'thigh_R',
               'hand_R', 'neck', 'upperarm_L', 'foot_L', 'forearm_L', 'shin_L', 'thigh_L', 'hand_L']
_PARENT_NAME = {'hips': None, 'chest': 'hips', 'neck': 'chest', 'head': 'neck',
                'upperarm_R': 'chest', 'forearm_R': 'upperarm_R', 'hand_R': 'forearm_R',
                'upperarm_L': 'chest', 'forearm_L': 'upperarm_L', 'hand_L': 'forearm_L',
                'thigh_R': 'hips', 'shin_R': 'thigh_R', 'foot_R': 'shin_R',
                'thigh_L': 'hips', 'shin_L': 'thigh_L', 'foot_L': 'shin_L'}
PARENT = [JOINT_NAMES.index(_PARENT_NAME[n]) if _PARENT_NAME[n] else -1 for n in JOINT_NAMES]
HIPS = JOINT_NAMES.index('hips')

# the 18 routine names in the exe's motion list (`inst_motions` is the instructor's; skipped)
ROUTINE_NAMES = ['M_normal', 'F_normal', 'M_y31', 'M_capoera1', 'M_y11', 'M_thouse2', 'MF_hopping1',
                 'MF_thouse3', 'MF_hiphop2', 'MF_hiphop1', 'F_soul2', 'F_n31', 'F_mhouse1', 'F_jazz1',
                 'F_jazz2', 'F_sino_', 'F_lock1', 'F_soul1']
MALE_PLAYLIST_LEN, FEMALE_PLAYLIST_LEN = 8, 12  # FUN_00402240: 8 x 9 and 8 x 13 pointer tables

# the 24 built-in characters by id, as the select screen names them (data.bin portraits)
BUILTIN_NAMES = {0: 'Rage', 1: 'Johnny', 2: 'Boldo', 3: 'Akira', 4: 'Izam', 5: 'Astro', 6: 'Emi',
                 7: 'Jenny', 8: 'Tracy', 9: 'Yuni', 10: 'Ni-Na', 11: 'Charmy', 12: 'Robo2000',
                 13: 'Angel-Zukin', 14: 'Afro', 15: 'Dred', 16: 'Brother', 17: 'Sapphire',
                 18: 'Maid-Zukin', 19: 'Evil-Zukin', 20: 'Lilly', 21: 'Janet', 22: 'Lady', 23: 'Ruby'}


def _u32(d, o):
    return struct.unpack_from('<I', d, o)[0]


# ---------------------------------------------------------------------------
# PE images (the exe and the character DLLs are both read as flat images)
# ---------------------------------------------------------------------------
class Image:
    """A PE file with virtual-address reads."""

    def __init__(self, data):
        self.data = data
        pe = _u32(data, 0x3C)
        nsec = struct.unpack_from('<H', data, pe + 6)[0]
        optsz = struct.unpack_from('<H', data, pe + 20)[0]
        self.base = _u32(data, pe + 24 + 28)
        self.sections = []
        for i in range(nsec):
            s = pe + 24 + optsz + i * 40
            name = data[s:s + 8].rstrip(b'\0').decode('latin1')
            vs, va, rs, ro = struct.unpack_from('<IIII', data, s + 8)
            self.sections.append((name, va, max(vs, rs), ro, rs))
        self.exports = self._exports(data, pe)

    def _exports(self, data, pe):
        rva = _u32(data, pe + 24 + 96)
        if not rva:
            return {}
        o = self.off(self.base + rva)
        nn, fn_rva, nm_rva, ord_rva = _u32(data, o + 24), _u32(data, o + 28), _u32(data, o + 32), _u32(data, o + 36)
        out = {}
        for i in range(nn):
            name = self.cstr(self.base + _u32(data, self.off(self.base + nm_rva) + 4 * i))
            ordn = struct.unpack_from('<H', data, self.off(self.base + ord_rva) + 2 * i)[0]
            out[name] = self.base + _u32(data, self.off(self.base + fn_rva) + 4 * ordn)
        return out

    def section(self, name):
        return next(s for s in self.sections if s[0] == name)

    def off(self, va):
        """File offset of a VA, or None for a BSS address (inside a section's virtual size but
        past its raw data: zero at run time -- some material texture-index pointers land there)."""
        rva = va - self.base
        for _n, sva, size, ro, rs in self.sections:
            if sva <= rva < sva + size:
                return rva - sva + ro if rva - sva < rs else None
        raise ValueError('VA %#x outside the image' % va)

    def in_image(self, va):
        try:
            self.off(va)
            return True
        except ValueError:
            return False

    def bytes(self, va, n):
        o = self.off(va)
        if o is None:
            return b'\0' * n
        return self.data[o:o + n]

    def u32(self, va):
        return _u32(self.bytes(va, 4), 0)

    def u32s(self, va, n):
        return list(struct.unpack_from('<%dI' % n, self.bytes(va, 4 * n)))

    def f32(self, va):
        return struct.unpack_from('<f', self.bytes(va, 4))[0]

    def cstr(self, va):
        o = self.off(va)
        if o is None:
            return ''
        return self.data[o:self.data.index(b'\0', o)].decode('latin1')

    def floats(self, va, n):
        return np.frombuffer(self.bytes(va, 4 * n), dtype='<f4', count=n)


# ---------------------------------------------------------------------------
# dll_header + model
# ---------------------------------------------------------------------------
def parse_header(img, va) -> dict[str, Any]:
    w = img.u32s(va, HEADER_WORDS)
    return dict(va=va, id=w[0], sex='F' if w[1] else 'M', scale=img.f32(va + 8), tex_names=w[3],
                tex_src=w[4], model=w[5], bin_table=w[6], bin_name=w[7], portrait=w[8], splash=w[10])


def header_plausible(img, va):
    """A `dll_header` record: small id, sex 0/1, a sane scale, four data pointers and a model whose
    counts hang together (16 joints, vertex table ends at nvert, index table at 3*ntri)."""
    try:
        w = img.u32s(va, HEADER_WORDS)
    except (ValueError, struct.error):
        return False
    if w[0] >= MAX_CHARS or w[1] > 1 or not (0.5 <= img.f32(va + 8) <= 1.5):
        return False
    if not all(img.in_image(p) for p in w[3:6]):
        return False
    try:
        m = img.u32s(w[5], MODEL_WORDS)
    except (ValueError, struct.error):
        return False
    ntex, nmat, nv, nt, nj = m[:5]
    if nj != NJOINT or not (0 < ntex <= 32) or not (0 < nmat <= 64) or not (0 < nv <= 65535) or not (0 < nt <= 65535):
        return False
    if not all(img.in_image(p) for p in m[5:10]):
        return False
    return img.u32(m[7] + 4 * NJOINT) == nv and img.u32(m[9] + 4 * nmat) == 3 * nt


def parse_model(img, va) -> dict[str, Any]:
    ntex, nmat, nv, nt, nj, pmat, pv, pjs, pidx, pms = img.u32s(va, MODEL_WORDS)
    raw = np.frombuffer(img.bytes(pv, nv * VERT_SIZE), dtype=np.dtype([
        ('pos', '<f4', 3), ('nrm', '<f4', 3), ('col', '<u4'), ('uv', '<f4', 2)]))
    col = raw['col']
    colors = np.stack([(col >> 16) & 0xFF, (col >> 8) & 0xFF, col & 0xFF, (col >> 24) & 0xFF], 1).astype(np.uint8)
    mats = []
    for i in range(nmat):
        a = pmat + MAT_SIZE * i
        n, p = img.u32(a + 0x44), img.u32(a + 0x48)
        mats.append(dict(d3d_material=img.floats(a, 17).copy(), tex=img.u32s(p, n) if (n and p) else []))
    names = [img.bytes(va + 4 * MODEL_WORDS + 16 * i, 16).split(b'\0')[0].decode('latin1') for i in range(ntex)]
    out: dict[str, Any] = dict(ntex=ntex, model_tex_names=names, materials=mats,
                pos=raw['pos'].astype(float), normals=raw['nrm'].astype(float), colors=colors,
                uvs=raw['uv'].astype(float), joint_vert_start=img.u32s(pjs, nj + 1),
                tris=np.frombuffer(img.bytes(pidx, 6 * nt), dtype='<u2').reshape(nt, 3).astype(np.int64),
                mat_tri_start=img.u32s(pms, nmat + 1))
    return out


def decode_bmp(data):
    """Uncompressed Windows BMP (4 / 8 bpp palette, 24 bpp) -> RGBA (H, W, 4) uint8, top-down,
    with the game's colour key (248, 0, 248) as alpha 0. No Pillow: Blender's Python lacks it."""
    assert data[:2] == b'BM', 'not a BMP'
    off = _u32(data, 10)
    hdr = _u32(data, 14)
    w, h = struct.unpack_from('<ii', data, 18)
    bpp, comp = struct.unpack_from('<HI', data, 28)
    assert comp == 0 and bpp in (4, 8, 24), (bpp, comp)
    ncol = _u32(data, 46) if hdr >= 40 else 0
    top_down = h < 0
    h = abs(h)
    stride = (w * bpp + 31) // 32 * 4
    rows = np.frombuffer(data, dtype=np.uint8, count=stride * h, offset=off).reshape(h, stride)
    if bpp == 24:
        rgb = rows[:, :w * 3].reshape(h, w, 3)[:, :, ::-1]
    else:
        if not ncol:
            ncol = 1 << bpp
        pal = np.frombuffer(data, dtype=np.uint8, count=4 * ncol, offset=14 + hdr).reshape(ncol, 4)[:, 2::-1]
        if bpp == 8:
            idx = rows[:, :w]
        else:
            hi = rows[:, :(w + 1) // 2]
            idx = np.stack([hi >> 4, hi & 15], 2).reshape(h, -1)[:, :w]
        rgb = pal[idx]
    if not top_down:
        rgb = rgb[::-1]
    rgba = np.concatenate([rgb, np.full((h, w, 1), 255, np.uint8)], 2)
    rgba[(rgb == np.array(COLOR_KEY, np.uint8)).all(2), 3] = 0
    return np.ascontiguousarray(rgba)


def write_png(path, rgba):
    """Minimal PNG writer (RGBA8), so the tools need no Pillow."""
    import zlib
    h, w = rgba.shape[:2]
    raw = b''.join(b'\0' + rgba[y].tobytes() for y in range(h))

    def chunk(tag, body):
        return struct.pack('>I', len(body)) + tag + body + struct.pack('>I', zlib.crc32(tag + body) & 0xFFFFFFFF)
    png = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
           + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))
    with open(path, 'wb') as f:
        f.write(png)


# ---------------------------------------------------------------------------
# the game: exe + data.bin
# ---------------------------------------------------------------------------
def find_data_table(img, data_size):
    """The data.bin offset table (FUN_00402f30 reads `offs[id+1] - offs[id]`): the longest run of
    non-decreasing u32 that starts at 0 and ends exactly at the file size."""
    d = img.data
    _n, va, size, ro, rs = img.section('.data')
    words = np.frombuffer(d[ro:ro + rs - rs % 4], dtype='<u4')
    ends = np.flatnonzero(words == data_size)
    best = None
    for e in ends:
        s = e
        while s > 0 and words[s - 1] <= words[s]:
            s -= 1
        if words[s] == 0 and (best is None or e - s > best[1] - best[0]):
            best = (s, e)
    if best is None or best[1] - best[0] < 16:
        raise ValueError('data.bin offset table not found')
    return img.base + va + 4 * int(best[0]), int(best[1] - best[0])


def find_character_table(img):
    """The built-in `dll_header[24]` table (DAT_00fee410; records are 44 bytes, scanned 4-aligned)."""
    _n, va, size, ro, rs = img.section('.data')
    found = []
    for o in range(ro, ro + rs - 4 * HEADER_WORDS, 4):
        a = img.base + va + (o - ro)
        if _u32(img.data, o) < MAX_CHARS and _u32(img.data, o + 4) <= 1 and header_plausible(img, a):
            if not found or a >= found[-1] + 4 * HEADER_WORDS:
                found.append(a)
    if not found:
        raise ValueError('character table not found')
    return found


def find_motions(img):
    """{routine name: [clip va, ...]} from the motion name list (FUN_00402240 walks the pointer
    lists; the 19 name pointers sit right after the 18 list pointers, `inst_motions` first)."""
    d = img.data
    _n, va, size, ro, rs = img.section('.data')
    i = d.find(b'\0inst_motions\0', ro, ro + rs)
    assert i >= 0, 'motion name list'
    first_name = img.base + va + (i + 1 - ro)
    i = d.find(struct.pack('<I', first_name), ro, ro + rs)
    assert i >= 0, 'motion name pointer array'
    names_arr = img.base + va + (i - ro)
    got = [img.cstr(p) for p in img.u32s(names_arr, 1 + len(ROUTINE_NAMES))]
    assert got == ['inst_motions'] + ROUTINE_NAMES, got
    lists_arr = names_arr - 4 * len(ROUTINE_NAMES)
    out = {}
    for k, name in enumerate(ROUTINE_NAMES):
        p = img.u32(lists_arr + 4 * k)
        clips = []
        while img.u32(p):
            clips.append(img.u32(p))
            p += 4
        out[name] = clips
    return out, lists_arr


def find_playlists(img, lists_arr):
    """(male 8x8, female 8x12) routine names per `id & 7` (PTR_PTR_00fee14c / 00fee26c): the two
    0-terminated pointer tables into the named lists, near the list-pointer array."""
    known = {img.u32(lists_arr + 4 * k): ROUTINE_NAMES[k] for k in range(len(ROUTINE_NAMES))}
    d = img.data
    _n, va, size, ro, rs = img.section('.data')
    centre = img.off(lists_arr)
    male = female = None
    for o in range(max(ro, centre - 0x4000), min(ro + rs - 52, centre + 0x4000), 4):
        if male is None:
            row = struct.unpack_from('<9I', d, o)
            if row[8] == 0 and all(p in known for p in row[:8]) and len(set(row[:8])) == 8:
                male = img.base + va + (o - ro)
        elif female is None:
            row = struct.unpack_from('<13I', d, o)
            if row[12] == 0 and all(p in known for p in row[:12]) and len(set(row[:12])) == 12:
                female = img.base + va + (o - ro)
                break
    assert male and female, 'playlist tables'
    mt = [[known[p] for p in img.u32s(male + 36 * r, 8)] for r in range(8)]
    ft = [[known[p] for p in img.u32s(female + 52 * r, 12)] for r in range(8)]
    assert all(n.startswith(('M_', 'MF_')) for row in mt for n in row)
    assert all(n.startswith(('F_', 'MF_')) for row in ft for n in row)
    return mt, ft


def load_clip(img, va):
    """(nframes, 16, 4, 4) D3D row matrices."""
    n, nj, pframes = img.u32s(va, 3)
    assert nj == NJOINT, nj
    out = np.zeros((n, nj, 4, 4))
    out[:, :, 3, 3] = 1.0
    for f in range(n):
        m = img.floats(img.u32(pframes + 4 * f), nj * 12).reshape(nj, 4, 3)
        out[f, :, :, :3] = m
    return out


class Game:
    def __init__(self, game_dir):
        exe = next((os.path.join(game_dir, n) for n in EXE_NAMES if os.path.exists(os.path.join(game_dir, n))), None)
        dat = next((os.path.join(game_dir, n) for n in DATA_NAMES if os.path.exists(os.path.join(game_dir, n))), None)
        if not exe or not dat:
            raise FileNotFoundError('need DanceDanceRevolution.exe and data.bin in %s' % game_dir)
        self.img = Image(open(exe, 'rb').read())
        self.data = open(dat, 'rb').read()
        self.table_va, self.nfiles = find_data_table(self.img, len(self.data))
        self.offsets = self.img.u32s(self.table_va, self.nfiles + 1)
        self.header_vas = find_character_table(self.img)
        self.motion_lists, lists_arr = find_motions(self.img)
        self.male_playlists, self.female_playlists = find_playlists(self.img, lists_arr)
        self._clips = {}

    def file(self, fid):
        return self.data[self.offsets[fid]:self.offsets[fid + 1]]

    def headers(self):
        return [parse_header(self.img, va) for va in self.header_vas]

    def character(self, header) -> dict[str, Any]:
        ch = _character(self.img, header)
        ch['textures'] = [(n, decode_bmp(self.file(fid))) for n, fid in zip(ch['tex_names'], ch['tex_src'])]
        ch['portrait'] = self.file(header['portrait'])
        ch['name'] = BUILTIN_NAMES.get(header['id'], 'char%02d' % header['id'])
        ch['source'] = 'builtin'
        return ch

    def characters(self):
        return [self.character(h) for h in self.headers()]

    def clip(self, va):
        if va not in self._clips:
            self._clips[va] = load_clip(self.img, va)
        return self._clips[va]

    def routine(self, name):
        """[(nframes, 16, 4, 4)] clips of a routine."""
        return [self.clip(va) for va in self.motion_lists[name]]

    def playlist(self, ch):
        table = self.female_playlists if ch['sex'] == 'F' else self.male_playlists
        return table[ch['id'] & 7]


def _character(img, header) -> dict[str, Any]:
    m = parse_model(img, header['model'])
    ntex = m['ntex']
    tex_names = [img.cstr(p) for p in img.u32s(header['tex_names'], ntex)]
    ch: dict[str, Any] = dict(m)
    ch.update(id=header['id'], sex=header['sex'], scale=header['scale'], tex_names=tex_names,
              tex_src=img.u32s(header['tex_src'], ntex), header=header)
    return ch


# ---------------------------------------------------------------------------
# downloadable characters: <name>.dll + <name>.bin
# ---------------------------------------------------------------------------
def load_dll(dll_path, bin_path=None) -> dict[str, Any]:
    img = Image(open(dll_path, 'rb').read())
    hva = img.exports.get('dll_header')
    if hva is None:
        raise ValueError('%s exports no dll_header' % dll_path)
    header = parse_header(img, hva)
    assert header_plausible(img, hva), dll_path
    ch = _character(img, header)
    bin_name = img.cstr(header['bin_name'])
    bin_path = bin_path or os.path.join(os.path.dirname(dll_path), bin_name)
    if not os.path.exists(bin_path):  # the packs are not case-consistent (Elliot.bin / sher.bin)
        cand = [p for p in glob.glob(os.path.join(os.path.dirname(dll_path), '*.bin'))
                if os.path.basename(p).lower() == bin_name.lower()]
        bin_path = cand[0] if cand else bin_path
    blob = open(bin_path, 'rb').read()
    # the .bin member table: offsets until the first 0 after entry 0
    offs = [img.u32(header['bin_table'])]
    k = 1
    while True:
        v = img.u32(header['bin_table'] + 4 * k)
        if v == 0:
            break
        offs.append(v)
        k += 1
    offs.append(len(blob))

    def member(i):
        return blob[offs[i]:offs[i + 1]]
    ch['textures'] = [(n, decode_bmp(member(i))) for n, i in zip(ch['tex_names'], ch['tex_src'])]
    ch['portrait'] = member(header['portrait'])
    ch['name'] = os.path.splitext(os.path.basename(dll_path))[0]
    ch['source'] = 'dll'
    ch['bin_members'] = len(offs) - 1
    return ch


def load_dll_dir(dll_dir):
    out = []
    for p in sorted(glob.glob(os.path.join(dll_dir, '*.dll')), key=lambda s: s.lower()):
        try:
            out.append(load_dll(p))
        except ValueError as e:
            print('skip %s: %s' % (p, e), file=sys.stderr)
    return out


# ---------------------------------------------------------------------------
# World conversion
# ---------------------------------------------------------------------------
SCALE = 0.1  # model units -> metres (hips ~1.0 m, like the 573's 1060 mm)
MIRROR = np.diag([1.0, 1.0, -1.0, 1.0])  # D3D (faces -Z, left-handed) -> World (faces +Z)
MEASURE_FRAMES = 120  # World's dance clock: 120 frames per measure, keyed every 2nd frame
WORLD_ROLE_ALIASES = {'Hips': 'hips', 'Spine2': 'chest', 'Head': 'head',
                      'LeftToeBase': 'foot_L', 'RightToeBase': 'foot_R'}
# export rig: `root` then the joints parent-first (the add-on requires it); BONE_JOINT maps a
# bone to its PC joint index (-1 for root)
BONE_NAMES = ['root', 'hips', 'chest', 'neck', 'head', 'upperarm_L', 'forearm_L', 'hand_L',
              'upperarm_R', 'forearm_R', 'hand_R', 'thigh_L', 'shin_L', 'foot_L', 'thigh_R', 'shin_R', 'foot_R']
BONE_JOINT = [-1] + [JOINT_NAMES.index(n) for n in BONE_NAMES[1:]]
BONE_PARENTS = [-1] + [BONE_NAMES.index(_PARENT_NAME[n] or 'root') for n in BONE_NAMES[1:]]
assert all(p < i for i, p in enumerate(BONE_PARENTS)), BONE_PARENTS
TEX_UPSCALE = 2
SWATCH = 8


def d3d_to_world(m):
    """D3D row 4x4 (units) -> World row 4x4 (m): v_w = v_d S, so W = S M S with the scale on t."""
    g = MIRROR @ m @ MIRROR
    g = g.copy()
    g[3, :3] *= SCALE
    return g


def rest_matrices(clip):
    """Rest pose per joint (D3D row 4x4, units): frame 0 of the sex's `normal` idle, clip 0. The
    joint-local meshes are modelled in these ROTATED frames, so a bind must carry the rotation,
    not just the translation."""
    return clip[0].copy()


def world_binds(rest):
    """{bone: World row bind (bone local -> game world)}: the rest matrix of the joint; root at
    the origin."""
    out = {'root': np.eye(4)}
    for j, n in enumerate(JOINT_NAMES):
        out[n] = d3d_to_world(rest[j])
    return out


def _pack_textures(sizes):
    """Shelf-pack (h, w) rectangles into a power-of-two sheet: [(x, y)], (W, H)."""
    order = sorted(range(len(sizes)), key=lambda i: (-sizes[i][0], -sizes[i][1]))
    W = 256
    while True:
        pos: list = [None] * len(sizes)
        x = y = shelf = 0
        ok = True
        for i in order:
            h, w = sizes[i]
            if w > W:
                ok = False
                break
            if x + w > W:
                x, y, shelf = 0, y + shelf, 0
            pos[i] = (x, y)
            x += w
            shelf = max(shelf, h)
        H = y + shelf
        if ok and H <= W:
            Hp = 1
            while Hp < H:
                Hp *= 2
            return pos, (W, Hp)
        W *= 2
        if W > 4096:
            raise ValueError('textures do not fit an atlas')


def flat_colors(ch):
    """{material index: (r, g, b, a)} of the untextured materials: the game draws them with the
    vertex diffuse alone (SetTexture(NULL), lit COLOR1), and every shipped one is a single flat
    colour per material -- the 573's flat-colour primitives baked into vertices."""
    out = {}
    for mi in range(len(ch['materials'])):
        if material_texture(ch, mi) is not None:
            continue
        t0, t1 = ch['mat_tri_start'][mi] // 3, ch['mat_tri_start'][mi + 1] // 3
        if t1 == t0:
            continue
        vi = np.unique(ch['tris'][t0:t1])
        cols = {tuple(int(x) for x in c) for c in ch['colors'][vi]}
        if len(cols) != 1:
            raise ValueError('material %d of %s is not a flat colour (%d colours)' % (mi, ch['name'], len(cols)))
        out[mi] = cols.pop()
    return out


def world_atlas(ch):
    """(RGBA H x W, [(x0, y0, w, h) per texture at TEX_UPSCALE], {flat colour: (x0, y0, w, h)}).
    Every texture at native size x TEX_UPSCALE (nearest), the colour key as alpha 0, plus one
    SWATCH cell per flat colour of the untextured materials (as is: the game lights it at face
    value), since World's character shader is texture x COLOR0 (white)."""
    imgs = [np.repeat(np.repeat(t, TEX_UPSCALE, 0), TEX_UPSCALE, 1) for _n, t in ch['textures']]
    colors = sorted(set(flat_colors(ch).values()))
    s = SWATCH * TEX_UPSCALE
    sizes = [i.shape[:2] for i in imgs] + [(s, s)] * len(colors)
    pos, (W, H) = _pack_textures(sizes)
    atlas = np.zeros((H, W, 4), np.uint8)
    rects = []
    for (x, y), im in zip(pos[:len(imgs)], imgs):
        h, w = im.shape[:2]
        atlas[y:y + h, x:x + w] = im
        rects.append((x, y, w, h))
    swatches = {}
    for (x, y), c in zip(pos[len(imgs):], colors):
        atlas[y:y + s, x:x + s] = c
        swatches[c] = (x, y, s, s)
    return atlas, rects, swatches


def material_texture(ch, mi):
    """Texture index (into ch['textures']) drawn for material `mi` at frame 0, or None when the
    material has no frame list (SetTexture(NULL): the colour is COLOR0 alone). A frame list that
    lives in BSS reads 0 at run time, i.e. texture 0 -- `Image.bytes` reproduces that."""
    tex = ch['materials'][mi]['tex']
    return tex[0] if tex and 0 <= tex[0] < ch['ntex'] else None


def world_mesh(ch, rest):
    """Rest mesh in World space as per-corner arrays: positions (m), unit normals, atlas uv
    (D3D v-down, 0..1), bone name per corner, COLOR0 RGBA bytes per corner (the file's diffuse;
    white on the swatch-coloured flat materials), triangles (CCW), and the atlas. Every vertex is rigid on its joint's bone (identity-rotation bind at the rest
    translation), so the World rest position is mirror(local) * SCALE + bind translation."""
    atlas, rects, swatches = world_atlas(ch)
    flats = flat_colors(ch)
    H, W = atlas.shape[:2]
    jvs = ch['joint_vert_start']
    joint_of = np.zeros(len(ch['pos']), np.int64)
    for j in range(NJOINT):
        joint_of[jvs[j]:jvs[j + 1]] = j
    binds = world_binds(rest)
    pos_w = np.zeros_like(ch['pos'])
    nrm_w = np.zeros_like(ch['normals'])
    for j, n in enumerate(JOINT_NAMES):
        sel = joint_of == j
        b = binds[n]
        local = (ch['pos'][sel] @ MIRROR[:3, :3]) * SCALE  # joint-local, World handedness + metres
        pos_w[sel] = local @ b[:3, :3] + b[3, :3]
        nrm_w[sel] = (ch['normals'][sel] @ MIRROR[:3, :3]) @ b[:3, :3]
    lens = np.linalg.norm(nrm_w, axis=1)
    nrm_w = nrm_w / np.where(lens == 0, 1.0, lens)[:, None]
    pos, nrm, uv, bone, col, tris = [], [], [], [], [], []
    for mi in range(len(ch['materials'])):
        t0, t1 = ch['mat_tri_start'][mi] // 3, ch['mat_tri_start'][mi + 1] // 3
        ti = material_texture(ch, mi)
        if t1 == t0:
            continue
        rect = rects[ti] if ti is not None else swatches[flats[mi]]
        for tri in ch['tris'][t0:t1]:
            base = len(pos)
            for vi in tri:
                pos.append(pos_w[vi])
                nrm.append(nrm_w[vi])
                bone.append(JOINT_NAMES[joint_of[vi]])
                col.append((255, 255, 255, 255) if ti is None else ch['colors'][vi])
                if ti is not None:
                    u, v = np.clip(ch['uvs'][vi], 0.0, 1.0)
                    uv.append(((rect[0] + u * rect[2]) / W, (rect[1] + v * rect[3]) / H))
                else:
                    uv.append(((rect[0] + rect[2] / 2) / W, (rect[1] + rect[3] / 2) / H))
            tris.append((base, base + 1, base + 2))  # the Z mirror already flips CW -> CCW
    return (np.array(pos), np.array(nrm), np.array(uv), bone, np.array(col, np.uint8),
            np.array(tris, dtype=np.int64), atlas)


def routine_worlds(clips, root_mode='recentre'):
    """World row matrices per sampled World frame for every bone (BONE_NAMES order): the clips
    back to back at 2 World frames per PC frame (60 per measure -> 120), plus the end pose
    repeated as the final key. root_mode: 'inplace' (the PC's own in-place motion, root at the
    origin), 'recentre' (hips x/z bounding-box centre moved to the dancer's mark), 'travel'
    (same as inplace: the PC never travels). Returns (frames, worlds [f][b] 4x4)."""
    seq = np.concatenate(clips, 0)
    seq = np.concatenate([seq, seq[-1:]], 0)
    frames = [2 * i for i in range(len(seq))]
    joints = np.array([[d3d_to_world(seq[f, j]) for j in range(NJOINT)] for f in range(len(seq))])
    root = np.tile(np.eye(4), (len(seq), 1, 1))
    if root_mode == 'recentre':
        xz = joints[:, HIPS, 3, [0, 2]]
        mid = (xz.min(0) + xz.max(0)) / 2
        root[:, 3, 0] = -mid[0]
        root[:, 3, 2] = -mid[1]
        joints[:, :, 3, 0] -= mid[0]
        joints[:, :, 3, 2] -= mid[1]
        root[:, 3, 0] = root[:, 3, 2] = 0.0
    elif root_mode not in ('inplace', 'travel'):
        raise ValueError('root_mode %r' % root_mode)
    worlds = np.concatenate([root[:, None], joints[:, BONE_JOINT[1:]]], 1)
    return frames, worlds


def retarget_worlds(worlds, bone_names, src_binds, target_binds):
    """Re-express per-bone worlds for EXPORTED binds (Q = B_target . B_src^-1 absorbs any rigid
    re-framing the exporter applied) and reorder to the file's bone order."""
    idx = [BONE_NAMES.index(n) for n in bone_names]
    q = [target_binds[i] @ np.linalg.inv(src_binds[n]) for i, n in enumerate(bone_names)]
    return np.array([[q[i] @ w[idx[i]] for i in range(len(bone_names))] for w in worlds])


def _quat(r):
    """3x3 (column form) -> (x, y, z, w)."""
    import math
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


def _quat_to_mat(q):
    """(x, y, z, w) -> 3x3 column-form rotation (test helper / inverse of _quat)."""
    x, y, z, w = q
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def worlds_to_anm_spec(frames, W, parents):
    """scripts/anm_dump.py write_anm spec: local rotation (0x1C) and translation (0x1D) tracks
    per bone (file order) at the given key frames; constant tracks collapse to one key."""
    tracks = []
    for b in range(W.shape[1]):
        p = parents[b]
        locs = W[:, b] if p < 0 else np.einsum('fij,fjk->fik', W[:, b], np.linalg.inv(W[:, p]))
        quats, prev = [], None
        for m in locs:
            r = m[:3, :3].T
            # strip any residual scale/shear the baked matrices carry (they are near-orthonormal)
            u, _s, vt = np.linalg.svd(r)
            r = u @ vt
            if np.linalg.det(r) < 0:
                u[:, -1] *= -1
                r = u @ vt
            qv = _quat(r)
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
    return dict(frame_count=frames[-1], flag=0, hierarchy=list(parents), tracks=tracks)


def orthonormality(clips):
    """Worst |R R^T - I| over a routine's rotation blocks (a data sanity check)."""
    seq = np.concatenate(clips, 0)
    r = seq[:, :, :3, :3]
    return float(np.abs(np.einsum('fjab,fjcb->fjac', r, r) - np.eye(3)).max())


# ---------------------------------------------------------------------------
# checks + commands
# ---------------------------------------------------------------------------
def check_character(ch):
    """Index ranges, UV range, winding (stored vs geometric normals after the mirror)."""
    nv = len(ch['pos'])
    assert ch['tris'].max() < nv and ch['tris'].min() >= 0
    assert ch['joint_vert_start'][0] == 0 and ch['joint_vert_start'][-1] == nv
    assert all(a <= b for a, b in zip(ch['joint_vert_start'], ch['joint_vert_start'][1:]))
    assert ch['mat_tri_start'][0] == 0 and ch['mat_tri_start'][-1] == 3 * len(ch['tris'])
    assert all(a % 3 == 0 for a in ch['mat_tri_start'])
    for mi, m in enumerate(ch['materials']):
        assert all(0 <= t < ch['ntex'] for t in m['tex']), (mi, m['tex'])
    textured = np.zeros(nv, bool)
    for mi in range(len(ch['materials'])):
        if material_texture(ch, mi) is not None:
            t0, t1 = ch['mat_tri_start'][mi] // 3, ch['mat_tri_start'][mi + 1] // 3
            textured[ch['tris'][t0:t1].ravel()] = True
    uv = ch['uvs'][textured]
    uv_ok = (len(uv) == 0) or (uv.min() >= -1e-3 and uv.max() <= 1.0 + 1e-3)
    p = ch['pos'] @ MIRROR[:3, :3]
    n = ch['normals'] @ MIRROR[:3, :3]
    t = ch['tris']
    geo = np.cross(p[t[:, 1]] - p[t[:, 0]], p[t[:, 2]] - p[t[:, 0]])
    ln = np.linalg.norm(geo, axis=1)
    ok = ln > 1e-9
    avg = (n[t[:, 0]] + n[t[:, 1]] + n[t[:, 2]])[ok]
    agree = float(((geo[ok] * avg).sum(1) > 0).mean()) if ok.any() else 1.0
    return dict(uv_in_range=bool(uv_ok), ccw_fraction=agree, nverts=nv, ntris=len(t),
                textured_tris=int(textured.sum()), ntex=ch['ntex'], nmat=len(ch['materials']))


def dancers(game, dll_dir=None):
    out = game.characters()
    if dll_dir:
        out += load_dll_dir(dll_dir)
    return out


def cmd_info(a):
    g = Game(a.game_dir)
    print('exe characters %d, data.bin files %d, routines %d' % (len(g.header_vas), g.nfiles, len(g.motion_lists)))
    for name, clips in g.motion_lists.items():
        print('  %-12s %2d measures x %d frames' % (name, len(clips), g.clip(clips[0]).shape[0]))
    for ch in dancers(g, a.dll_dir):
        print('%-7s id %2d %s scale %.3f  %4d verts %4d tris %2d mats %2d tex  %s' % (
            ch['source'], ch['id'], ch['sex'], ch['scale'], len(ch['pos']), len(ch['tris']),
            len(ch['materials']), ch['ntex'], ch['name']))
        print('         textures: %s' % ', '.join(n for n, _ in ch['textures']))
        print('         playlist: %s' % ' '.join(g.playlist(ch)))


def cmd_survey(a):
    g = Game(a.game_dir)
    bad = 0
    for name, clips in g.motion_lists.items():
        cl = g.routine(name)
        assert all(c.shape[0] == FRAMES_PER_MEASURE for c in cl), name
        print('%-12s %2d clips, orthonormality %.1e' % (name, len(cl), orthonormality(cl)))
    for ch in dancers(g, a.dll_dir):
        r = check_character(ch)
        # the game draws dancers CULL_NONE, so ~10-20 % of its normals disagree with the winding;
        # the port is two-sided too. Below 0.6 the mirror would be wrong.
        flag = '' if (r['uv_in_range'] and r['ccw_fraction'] > 0.6) else '  <-- CHECK'
        bad += bool(flag)
        print('%-7s %-12s %4d v %4d t %2d m %2d tex uv_ok=%s ccw=%.3f%s' % (
            ch['source'], ch['name'], r['nverts'], r['ntris'], r['nmat'], r['ntex'], r['uv_in_range'],
            r['ccw_fraction'], flag))
    print('survey done, %d flagged' % bad)
    return 1 if bad else 0


def find_dancer(g, name, dll_dir):
    for ch in dancers(g, dll_dir):
        if ch['name'].lower() == name.lower():
            return ch
    raise SystemExit('no dancer %r' % name)


def cmd_obj(a):
    g = Game(a.game_dir)
    ch = find_dancer(g, a.name, a.dll_dir)
    rest = rest_matrices(g.routine('%s_normal' % ch['sex'])[0])
    pos, nrm, uv, bone, col, tris, atlas = world_mesh(ch, rest)
    if a.routine:
        clips = g.routine(a.routine)
        frames, worlds = routine_worlds(clips, 'inplace')
        binds = world_binds(rest)
        f = min(a.frame, len(worlds) - 1)
        for i in range(len(pos)):
            b = BONE_NAMES.index(bone[i])
            m = np.linalg.inv(binds[bone[i]]) @ worlds[f][b]
            pos[i] = (np.append(pos[i], 1.0) @ m)[:3]
            nrm[i] = nrm[i] @ m[:3, :3]
    with open(a.out, 'w') as f:
        f.write('mtllib %s.mtl\nusemtl atlas\n' % os.path.basename(a.out))
        for p in pos:
            f.write('v %.6f %.6f %.6f\n' % tuple(p))
        for t in uv:
            f.write('vt %.6f %.6f\n' % (t[0], 1.0 - t[1]))
        for n in nrm:
            f.write('vn %.6f %.6f %.6f\n' % tuple(n))
        for t in tris:
            f.write('f %d/%d/%d %d/%d/%d %d/%d/%d\n' % tuple(x for i in t for x in (i + 1,) * 3))
    write_png(a.out + '.png', atlas)
    open(a.out + '.mtl', 'w').write('newmtl atlas\nmap_Kd %s.png\n' % os.path.basename(a.out))
    print('wrote', a.out, len(pos), 'corners', len(tris), 'tris')


def cmd_textures(a):
    g = Game(a.game_dir)
    ch = find_dancer(g, a.name, a.dll_dir)
    os.makedirs(a.out_dir, exist_ok=True)
    for n, t in ch['textures']:
        write_png(os.path.join(a.out_dir, re.sub(r'\.bmp$', '', n, flags=re.I) + '.png'), t)
    write_png(os.path.join(a.out_dir, 'portrait.png'), decode_bmp(ch['portrait']))
    atlas, _r, _s = world_atlas(ch)
    write_png(os.path.join(a.out_dir, 'atlas.png'), atlas)
    print('flat colours:', sorted(set(flat_colors(ch).values())))
    print('wrote', len(ch['textures']) + 2, 'files to', a.out_dir)


def main(argv):
    ap = argparse.ArgumentParser(description=(__doc__ or '').split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('info')
    p.add_argument('game_dir')
    p.add_argument('dll_dir', nargs='?')
    p.set_defaults(fn=cmd_info)
    p = sub.add_parser('survey')
    p.add_argument('game_dir')
    p.add_argument('dll_dir', nargs='?')
    p.set_defaults(fn=cmd_survey)
    p = sub.add_parser('obj')
    p.add_argument('game_dir')
    p.add_argument('name')
    p.add_argument('out')
    p.add_argument('--dll-dir')
    p.add_argument('--routine')
    p.add_argument('--frame', type=int, default=0)
    p.set_defaults(fn=cmd_obj)
    p = sub.add_parser('textures')
    p.add_argument('game_dir')
    p.add_argument('name')
    p.add_argument('out_dir')
    p.add_argument('--dll-dir')
    p.set_defaults(fn=cmd_textures)
    a = ap.parse_args(argv)
    return a.fn(a) or 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
