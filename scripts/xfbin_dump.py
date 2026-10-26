#!/usr/bin/env python3
"""CyberConnect2 XFBIN (NUCC) container reader: NUD meshes, NTP3 textures, coord / clump rigs.

Used by the JoJo's Bizarre Adventure: Eyes of Heaven (PS4) and All Star Battle R ports
(docs/jojo_eoh_research.md). Pure Python (struct only), so it runs inside Blender. The
container layout follows SutandoTsukai181's xfbin_lib (MIT) and Smash Forge's NUD / NUT
readers; everything is big-endian on every platform.

    NUCC header (0x1C) | chunk table | pages...
    chunk table: counts/sizes of the type / path / name string tables, then the strings,
                 chunk maps (type, path, name index triples), chunk references, and the
                 per-page chunk-map index list
    page: chunks `size:u32 map_index:u32 nucc_id:u16 unk:u16 data[size]` ending with a
          nuccChunkPage whose data is `page_size:u32 reference_size:u32` -- every chunk
          index stored inside a chunk's data is relative to its page's slice of the index
          list

Chunk kinds decoded here: nuccChunkTexture (NTP3 -> DDS), nuccChunkCoord (local TRS: cm,
Euler degrees, scale), nuccChunkClump (the coord tree + the models that ride it),
nuccChunkModel (rigging flag, bone coord, NUD, material chunks), nuccChunkMaterial (texture
groups), nuccChunkAnm (clips: coord / material / camera / light curves, addressed through the
chunk reference table -- `AnmChunk`). Everything else keeps its raw bytes.

NUD (`NDP3`): mesh groups -> meshes; a mesh is one material pass over one vertex buffer.
Vertices: position f32[3]; normal per `vertex_size & 0xF` (1 float+pad, 3 float + tangent /
bitangent, 6/7 half); then bones per `vertex_size & 0xF0` (0x10 u32 ids + f32 weights, 0x20
u16 + f16, 0x40 u8 + u8/255) -- when a mesh HAS bones its colour (`uv_size & 0xF`: 2 = u8[4],
4 = f16[4]) and `uv_size >> 4` half-float UV sets live in a separate buffer before the
vertices, otherwise they trail each vertex. Faces are triangle strips with -1 restarts
(the winding flips every triangle), or plain lists when `face_size & 0x40`; the file order is
clockwise seen from outside (checked against the vertex normals on every EoH body), and
`faces` is emitted counter-clockwise. Bone ids index the clump's coord list directly.

Rig: a coord's local matrix is T(position) R(rotation) S(scale) with R = Rx * Ry * Rz (a
column vector is rotated about Z first, then Y, then X -- checked on the EoH bodies: the
other order folds the figure flat); the world matrix is parent_world * local. The mesh
vertices are in the model's root space (Z-up centimetres, the character facing -Y, left at
+X), so world(bone)^-1 is each bone's inverse bind.
"""

import math
import os
import struct
import sys

# ----------------------------------------------------------------------------------------
# NUCC container
# ----------------------------------------------------------------------------------------


class Chunk:
    """A raw chunk; subclasses decode their payload in `decode(page)`."""

    def __init__(self, kind, path, name, data):
        self.kind, self.path, self.name, self.data = kind, path, name, data
        self.external = False   # referenced by this page but stored in another xfbin

    def decode(self, page):
        pass

    def __repr__(self):
        return f'<{self.kind} {self.name!r}>'


def _cstr(buf, off):
    end = buf.index(b'\0', off)
    return buf[off:end].decode('cp932', 'replace')


class Page:
    def __init__(self, chunks, index_slice):
        self.chunks = chunks            # Chunk objects in file order (incl. Null / Page)
        self.index = index_slice        # page-relative chunk index -> Chunk
        self.refs = []                  # page-relative chunk reference index -> ChunkRef

    def by_kind(self, kind):
        return [c for c in self.chunks if c.kind == kind]


class ChunkRef:
    """A chunk-table reference: `name` (the instance name an animation addresses) -> the chunk
    map `map` (the template chunk's type / path / name; `chunk` is the stored Chunk, resolved
    once the whole file is read, None when it lives in another xfbin)."""

    def __init__(self, name, map_index, m, types, paths, names):
        self.name, self.map = name, map_index
        self.kind, self.path, self.target = types[m[0]], paths[m[1]], names[m[2]]
        self.chunk = None

    def __repr__(self):
        return f'<ref {self.name!r} -> {self.kind} {self.target!r}>'


class Xfbin:
    def __init__(self, data):
        if data[:4] != b'NUCC':
            raise ValueError('not an XFBIN (magic %r)' % data[:4])
        (tcount, tsize, pcount, psize, ncount, nsize, mcount, msize,
         icount, rcount) = struct.unpack('>10I', data[0x1C:0x44])
        p = 0x44
        types, paths, names = [], [], []
        for lst, n in ((types, tcount), (paths, pcount), (names, ncount)):
            for _ in range(n):
                s = _cstr(data, p)
                lst.append(s)
                p += len(data[p:data.index(b'\0', p)]) + 1
        p = (p + 3) & ~3
        maps = [struct.unpack('>3I', data[p + 12 * i:p + 12 * i + 12]) for i in range(mcount)]
        p += 12 * mcount
        # chunk references: (name, chunk map) pairs -- a per-page slice (nuccChunkPage's
        # reference_size) that nuccChunkAnm addresses its animated clumps / coords / models
        # through: the reference NAME is the instance the animation drives, the chunk map the
        # template chunk it instantiates (several `sd04a_eff_lightred00N` refs -> one coord)
        refs = [ChunkRef(names[n], m, maps[m], types, paths, names)
                for n, m in (struct.unpack('>2I', data[p + 8 * i:p + 8 * i + 8]) for i in range(rcount))]
        p += 8 * rcount
        indices = struct.unpack('>%dI' % icount, data[p:p + 4 * icount])
        p += 4 * icount
        self.chunks = []
        self.pages = []
        page_start = 0
        ref_start = 0
        page_chunks = []
        by_map = {}
        while p < len(data):
            size, map_idx, _nucc_id, _unk = struct.unpack('>IIHH', data[p:p + 12])
            payload = data[p + 12:p + 12 + size]
            p += 12 + size
            gmap = indices[page_start + map_idx]
            t, fp, nm = maps[gmap]
            kind = types[t]
            cls = CHUNK_CLASSES.get(kind, Chunk)
            c = cls(kind, paths[fp], names[nm], payload)
            c.map_index = map_idx
            by_map.setdefault(gmap, c)
            page_chunks.append(c)
            self.chunks.append(c)
            if kind == 'nuccChunkPage':
                page_size, ref_size = struct.unpack('>II', payload[:8])
                present = {c2.map_index: c2 for c2 in page_chunks}
                sl = {}
                for k in range(page_size):
                    if k in present:
                        sl[k] = present[k]
                    else:
                        # referenced here, stored in another xfbin (textures of a `col` file, ...)
                        t2, fp2, nm2 = maps[indices[page_start + k]]
                        stub = CHUNK_CLASSES.get(types[t2], Chunk)(types[t2], paths[fp2], names[nm2], b'')
                        stub.map_index = k
                        stub.external = True
                        sl[k] = stub
                page = Page(page_chunks, sl)
                page.refs = refs[ref_start:ref_start + ref_size]
                for c2 in page_chunks:
                    c2.decode(page)
                self.pages.append(page)
                page_start += page_size
                ref_start += ref_size
                page_chunks = []
        for r in refs:
            r.chunk = by_map.get(r.map)

    def by_kind(self, kind):
        return [c for c in self.chunks if c.kind == kind]

    def find(self, name_suffix, kind=None):
        for c in self.chunks:
            if c.name.endswith(name_suffix) and (kind is None or c.kind == kind):
                return c
        return None


def read_xfbin(path):
    with open(path, 'rb') as f:
        return Xfbin(f.read())


def list_chunks(path):
    """[(kind, name)] of the chunks STORED in an xfbin (external references excluded), without
    decoding any payload -- for indexing thousands of files."""
    with open(path, 'rb') as f:
        data = f.read()
    if data[:4] != b'NUCC':
        return []
    (tcount, tsize, pcount, psize, ncount, nsize, mcount, msize, icount, rcount) = struct.unpack('>10I', data[0x1C:0x44])
    p = 0x44
    types, paths, names = [], [], []
    for lst, n in ((types, tcount), (paths, pcount), (names, ncount)):
        for _ in range(n):
            end = data.index(b'\0', p)
            lst.append(data[p:end].decode('cp932', 'replace'))
            p = end + 1
    p = (p + 3) & ~3
    maps = [struct.unpack('>3I', data[p + 12 * i:p + 12 * i + 12]) for i in range(mcount)]
    p += 12 * mcount + 8 * rcount
    indices = struct.unpack('>%dI' % icount, data[p:p + 4 * icount])
    p += 4 * icount
    out = []
    page_start = 0
    while p + 12 <= len(data):
        size, map_idx = struct.unpack('>II', data[p:p + 8])
        t, _fp, nm = maps[indices[page_start + map_idx]]
        kind = types[t]
        if kind == 'nuccChunkPage':
            page_start += struct.unpack('>I', data[p + 12:p + 16])[0]
        elif kind != 'nuccChunkNull':
            out.append((kind, names[nm]))
        p += 12 + size
    return out


# ----------------------------------------------------------------------------------------
# NTP3 textures
# ----------------------------------------------------------------------------------------

# NTP3 pixel format -> (DDS FourCC, bytes per 4x4 block) for the block-compressed kinds
NUT_DXT = {0: (b'DXT1', 8), 1: (b'DXT3', 16), 2: (b'DXT5', 16)}


class NutTexture:
    def __init__(self, width, height, fmt, mips, data, mip_sizes):
        self.width, self.height, self.fmt, self.mips, self.data = width, height, fmt, mips, data
        self.mip_sizes = mip_sizes  # None when a single level

    def mip_levels(self):
        """[(w, h, bytes)] for every level, from the data block."""
        out = []
        off = 0
        w, h = self.width, self.height
        for i in range(self.mips):
            if self.mip_sizes:
                n = self.mip_sizes[i]
            elif self.fmt in NUT_DXT:
                n = max(1, (w + 3) // 4) * max(1, (h + 3) // 4) * NUT_DXT[self.fmt][1]
            else:
                n = len(self.data) - off
            out.append((w, h, self.data[off:off + n]))
            off += n
            w, h = max(1, w // 2), max(1, h // 2)
        return out

    def to_dds(self, max_size=None, build_mips=True):
        """DDS bytes; levels larger than max_size (power of two) are dropped from the top. A DXT
        texture that is still too large (no mip chain) is downsampled in block space
        (`dxt_chain`), which also gives it a chain when `build_mips`."""
        levels = self.mip_levels()
        if max_size:
            while len(levels) > 1 and (levels[0][0] > max_size or levels[0][1] > max_size):
                levels.pop(0)
        if self.fmt in NUT_DXT:
            fourcc, bb = NUT_DXT[self.fmt]
            w, h = levels[0][0], levels[0][1]
            if fourcc != b'DXT3' and ((max_size and max(w, h) > max_size) or (build_mips and len(levels) == 1 and max(w, h) > 4)):
                levels = dxt_chain(levels[0][2], w, h, fourcc, max_size or max(w, h))
                w, h = levels[0][0], levels[0][1]
            return dds_header(fourcc, w, h, len(levels), bb) + b''.join(lv[2] for lv in levels)
        raise ValueError('NTP3 pixel format %d is not block compressed' % self.fmt)


def dds_header(fourcc, width, height, mips, block_bytes):
    DDSD = 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000
    caps = 0x1000
    if mips > 1:
        DDSD |= 0x20000
        caps |= 0x8 | 0x400000
    linear = max(1, (width + 3) // 4) * max(1, (height + 3) // 4) * block_bytes
    pf = struct.pack('<II4s5I', 32, 0x4, fourcc, 0, 0, 0, 0, 0)
    return b'DDS ' + struct.pack('<7I', 124, DDSD, height, width, linear, 0, mips) + b'\0' * 44 + pf + struct.pack('<5I', caps, 0, 0, 0, 0)


def parse_ntp3(buf):
    off = buf.find(b'NTP3')
    if off < 0:
        raise ValueError('no NTP3 header')
    version, count = struct.unpack('>HH', buf[off + 4:off + 8])
    p = off + 0x10
    out = []
    for _ in range(count):
        total, _z, dsize, hsize, _c, _u0, mips, _u1, fmt, w, h, _u2, cube = struct.unpack('>IIIHHBBBBHHII', buf[p:p + 0x20])
        q = p + 0x20
        if version < 0x200:
            # the pixel data follows the texture header (header_size counts from p; the file's
            # total = header_size + data_size, which pins it)
            data_off = p + hsize
            q += 16
        else:
            data_off = p + struct.unpack('>I', buf[q:q + 4])[0]
            q += 16
        if cube & 0x200:
            q += 16
        mip_sizes = None
        if mips > 1:
            mip_sizes = list(struct.unpack('>%dI' % mips, buf[q:q + 4 * mips]))
        out.append(NutTexture(w, h, fmt, mips, buf[data_off:data_off + dsize], mip_sizes))
        p += total
    return out


class TextureChunk(Chunk):
    def decode(self, page):
        self.width, self.height = struct.unpack('>HH', self.data[2:6])
        try:
            self.textures = parse_ntp3(self.data)
        except (ValueError, struct.error):
            self.textures = []


# ----------------------------------------------------------------------------------------
# rig chunks
# ----------------------------------------------------------------------------------------


class CoordChunk(Chunk):
    def decode(self, page):
        f = struct.unpack('>10f', self.data[:40])
        self.position, self.rotation, self.scale = f[0:3], f[3:6], f[6:9]
        self.unk_float = f[9]
        self.parent = None
        self.clump = None


class ClumpChunk(Chunk):
    def decode(self, page):
        d = self.data
        self.field00, ncoord, self.coord_flag0, self.coord_flag1 = struct.unpack('>IHBB', d[:8])
        p = 8
        self.parents = list(struct.unpack('>%dh' % ncoord, d[p:p + 2 * ncoord]))
        p += 2 * ncoord
        idx = struct.unpack('>%dI' % ncoord, d[p:p + 4 * ncoord])
        p += 4 * ncoord
        self.coords = [page.index[i] for i in idx]
        for c, par in zip(self.coords, self.parents):
            c.parent = self.coords[par] if par >= 0 else None
            c.clump = self
        nmodel, self.model_flag0, self.model_flag1 = struct.unpack('>HBB', d[p:p + 4])
        p += 8
        midx = struct.unpack('>%dI' % nmodel, d[p:p + 4 * nmodel])
        p += 4 * nmodel
        self.models = [page.index[i] for i in midx]
        self.model_groups = []
        while p + 2 <= len(d):
            n = struct.unpack('>h', d[p:p + 2])[0]
            if n == -1:
                break
            flag0, flag1, unk = struct.unpack('>BBi', d[p + 2:p + 8])
            ids = struct.unpack('>%di' % n, d[p + 8:p + 8 + 4 * n])
            self.model_groups.append(([page.index.get(i) for i in ids if i >= 0], flag0, flag1, unk))
            p += 8 + 4 * n

    def coord_index(self, coord):
        return self.coords.index(coord)

    def world_matrices(self):
        """{coord: 4x4 row-major tuple-of-rows} for every coord, parents first."""
        W = {}
        for c in self.coords:
            L = trs_matrix(c.position, c.rotation, c.scale)
            W[c] = mat_mul(W[c.parent], L) if c.parent is not None else L
        return W


class MaterialChunk(Chunk):
    def decode(self, page):
        d = self.data
        ngroups, self.field02 = struct.unpack('>HB', d[:3])
        self.field04 = struct.unpack('>f', d[4:8])[0]
        fmt = d[11]
        self.format = fmt
        nf = (1 if fmt & 0x40 else 0) + (1 if fmt & 0x20 else 0) + (2 if fmt & 0x10 else 0) + 4 * bin(fmt & 0x0F).count('1')
        p = 12
        self.floats = struct.unpack('>%df' % nf, d[p:p + 4 * nf])
        p += 4 * nf
        self.texture_groups = []
        for _ in range(ngroups):
            n, _z, unk = struct.unpack('>hHi', d[p:p + 8])
            ids = struct.unpack('>%dI' % n, d[p + 8:p + 8 + 4 * n])
            self.texture_groups.append(([page.index.get(i) for i in ids], unk))
            p += 8 + 4 * n

    def textures(self):
        return [t for g, _ in self.texture_groups for t in g if t is not None]


class ModelChunk(Chunk):
    def decode(self, page):
        d = self.data
        self.field00, self.rigging_flag = struct.unpack('>HH', d[:4])
        self.material_flags = tuple(d[4:8])
        clump_idx, hit_idx = struct.unpack('>II', d[12:20])
        self.clump = page.index.get(clump_idx)
        nud_start = d.find(b'NDP3')
        if nud_start < 0:
            raise ValueError('model %s has no NDP3' % self.name)
        nud_size = struct.unpack('>I', d[nud_start + 4:nud_start + 8])[0]
        p = 20
        bone_idx = struct.unpack('>I', d[p:p + 4])[0]
        if bone_idx == nud_size:
            bone_idx = -1
        self.coord_index = bone_idx     # into the clump's coord list (the clump decodes after us)
        self.flag1_floats = ()
        if self.material_flags[1] & 0x04:
            q = nud_start - 24
            self.flag1_floats = struct.unpack('>6f', d[q:q + 24])
        self.nud = Nud(d[nud_start:nud_start + nud_size])
        p = nud_start + nud_size
        nmat = struct.unpack('>H', d[p:p + 2])[0]
        ids = struct.unpack('>%dI' % nmat, d[p + 2:p + 2 + 4 * nmat])
        self.materials = [page.index.get(i) for i in ids]

    @property
    def coord(self):
        """The coord this model rides (`None`: skinned / not attached)."""
        if self.coord_index < 0 or self.clump is None or not hasattr(self.clump, 'coords'):
            return None
        return self.clump.coords[self.coord_index]


CHUNK_CLASSES = {
    'nuccChunkTexture': TextureChunk,
    'nuccChunkCoord': CoordChunk,
    'nuccChunkClump': ClumpChunk,
    'nuccChunkMaterial': MaterialChunk,
    'nuccChunkModel': ModelChunk,
}

# ----------------------------------------------------------------------------------------
# nuccChunkAnm
# ----------------------------------------------------------------------------------------

# curve format -> (struct per key, keyed: leading s32 frame in ticks, scale applied to the values)
ANM_CURVE = {
    0x05: ('>3f', False, 1.0),          # vec3 per frame (location / scale; 1 key = constant)
    0x06: ('>i3f', True, 1.0),          # keyed vec3 (location)
    0x08: ('>3f', False, 1.0),          # euler per frame (rotation, degrees)
    0x0A: ('>i4f', True, 1.0),          # keyed quaternion
    0x0B: ('>f', False, 1.0),           # float per frame (toggle / opacity; material constants)
    0x0C: ('>if', True, 1.0),           # keyed float (camera FOV)
    0x0F: ('>h', False, 1.0 / 0x8000),  # short per frame (toggle)
    0x10: ('>3h', False, 1.0 / 0x1000),  # short vec3 per frame (scale)
    0x11: ('>4h', False, 1.0 / 0x4000),  # short quaternion per frame (rotation, x y z w)
    0x14: ('>3b', False, 1.0),          # byte vec3 (light colour)
    0x15: ('>3f', False, 1.0),          # vec3 per frame (scale)
    0x16: ('>f', False, 1.0),           # float per frame (material / light parameter)
    0x18: ('>f', False, 1.0),           # float per frame (material parameter)
}
# entry kinds
ANM_COORD, ANM_CAMERA, ANM_MATERIAL, ANM_LIGHTDIRC, ANM_LIGHTPOINT = 1, 2, 4, 5, 6


class AnmCurve:
    """One animated channel: `keys` = [(frame, value tuple)] in frames (ticks / frame_size)."""

    def __init__(self, index, fmt, keys):
        self.index, self.format, self.keys = index, fmt, keys

    @property
    def constant(self):
        return len(self.keys) <= 1

    def sample(self, frame):
        """Value at `frame` (clamped; linear between keys, a quaternion is renormalised)."""
        ks = self.keys
        if len(ks) == 1 or frame <= ks[0][0]:
            return ks[0][1]
        if frame >= ks[-1][0]:
            return ks[-1][1]
        lo, hi = 0, len(ks) - 1
        while hi - lo > 1:
            mid = (lo + hi) // 2
            if ks[mid][0] <= frame:
                lo = mid
            else:
                hi = mid
        (f0, a), (f1, b) = ks[lo], ks[hi]
        t = (frame - f0) / (f1 - f0)
        if len(a) == 4 and self.format in (0x0A, 0x11):
            if sum(x * y for x, y in zip(a, b)) < 0:
                b = tuple(-v for v in b)
            v = tuple(x + (y - x) * t for x, y in zip(a, b))
            n = math.sqrt(sum(x * x for x in v)) or 1.0
            return tuple(x / n for x in v)
        return tuple(x + (y - x) * t for x, y in zip(a, b))


class AnmEntry:
    """The curves of one animated target. `clump` is an index into AnmChunk.clumps (-1: `target`
    is one of AnmChunk.others -- a camera / light chunk), `bone` an index into that clump's
    `bones` (a coord or material reference). Coord curves: 0 location (cm), 1 rotation (euler
    degrees or quaternion), 2 scale, 3 visibility. Material curves: 18 floats (0..3 the UV
    offsets of texture slots 0 and 1 -- see docs/jojo_eoh_research.md section 3)."""

    def __init__(self, clump, bone, kind, curves):
        self.clump, self.bone, self.kind, self.curves = clump, bone, kind, curves
        self.target = None

    def curve(self, index):
        return self.curves.get(index)

    def animated(self, indices=(0, 1, 2)):
        """True when one of the curves `indices` takes more than one value."""
        for i in indices:
            c = self.curves.get(i)
            if c is not None and len(c.keys) > 1:
                first = c.keys[0][1]
                if any(max(abs(a - b) for a, b in zip(v, first)) > 1e-5 for _, v in c.keys):
                    return True
        return False

    def local_matrix(self, frame, coord):
        """A coord entry's local T * R * S at `frame` (row-major 4x4 tuples, the CoordChunk
        convention); a missing curve keeps `coord`'s rest value. Euler curves are degrees in the
        coord order (trs_matrix); a quaternion (x, y, z, w) is the INVERSE of the coord rotation
        -- conjugated here (checked on sd02a's horse: the conjugate reproduces the rest coords to
        1e-3, the raw quaternion is off by up to 1.7)."""
        loc = self.curves[0].sample(frame) if 0 in self.curves else coord.position
        scl = self.curves[2].sample(frame) if 2 in self.curves else coord.scale
        rc = self.curves.get(1)
        if rc is None:
            return trs_matrix(loc, coord.rotation, scl)
        rot = rc.sample(frame)
        if len(rot) == 4:
            x, y, z, w = rot
            x, y, z = -x, -y, -z
            R = ((1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)),
                 (2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)),
                 (2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)))
            return tuple(tuple(R[i][j] * scl[j] for j in range(3)) + (loc[i],) for i in range(3)) + ((0, 0, 0, 1),)
        return trs_matrix(loc, rot, scl)


class AnmChunk(Chunk):
    """nuccChunkAnm (layout after SutandoTsukai181's xfbin_lib `anm_wip`, checked on every ASBR
    stage file: each chunk parses to its exact payload size).

        length:u32 (ticks)  frame_size:u32 (ticks per frame, 100)
        entry_count:u16  loop:u16 (1 = looping clip)  clump_count:u16  other_count:u16
        coord_parent_count:u32
        clumps[clump_count]: clump_ref:u32  bone_count:u16  model_count:u16
                             bone_refs:u32[bone_count]  model_refs:u32[model_count]
            (indices into the page's chunk REFERENCES: the reference name is the instance the
             clip drives, its chunk map the template -- one coord may be instanced many times)
        others: u32[other_count]            page chunk indices (camera / light chunks)
        coord_parents[n]: parent_clump:s16 parent_bone:u16 child_clump:s16 child_bone:u16
            (attach one clump's coord under another's: Wamuu on his chariot)
        entries[entry_count]: clump:s16 bone:u16 kind:u16 curve_count:u16
                              curve headers[curve_count]: index:u16 format:u16 keys:u16 flags:s16
                              then each curve's keys (ANM_CURVE), 4-byte aligned per curve
    Per-frame curves hold frames 0..length/frame_size (frames + 1 keys; 1 key = constant);
    keyed curves carry their own frame (ticks) and end with a frame -1 sentinel (dropped)."""

    def decode(self, page):
        d = self.data
        (self.length, self.frame_size, ne, self.loop_flag, nc, no, ncp) = struct.unpack('>IIHHHHI', d[:20])
        self.frames = self.length / float(self.frame_size or 100)
        self.loop = self.loop_flag == 1
        p = 20
        refs = page.refs
        self.clumps = []
        for _ in range(nc):
            ci, nb, nm = struct.unpack('>IHH', d[p:p + 8])
            p += 8
            bones = struct.unpack('>%dI' % nb, d[p:p + 4 * nb])
            p += 4 * nb
            models = struct.unpack('>%dI' % nm, d[p:p + 4 * nm])
            p += 4 * nm
            self.clumps.append((refs[ci] if ci < len(refs) else None,
                                [refs[b] if b < len(refs) else None for b in bones],
                                [refs[m] if m < len(refs) else None for m in models]))
        self.others = [page.index.get(i) for i in struct.unpack('>%dI' % no, d[p:p + 4 * no])]
        p += 4 * no
        self.coord_parents = [struct.unpack('>hHhH', d[p + 8 * i:p + 8 * i + 8]) for i in range(ncp)]
        p += 8 * ncp
        self.entries = []
        fs = float(self.frame_size or 100)
        for _ in range(ne):
            cl, bi, kind, ncur = struct.unpack('>hHHH', d[p:p + 8])
            p += 8
            heads = [struct.unpack('>HHHh', d[p + 8 * i:p + 8 * i + 8]) for i in range(ncur)]
            p += 8 * ncur
            curves = {}
            for idx, fmt, nkey, _flags in heads:
                st, keyed, scale = ANM_CURVE[fmt]
                sz = struct.calcsize(st)
                keys = []
                for k in range(nkey):
                    v = struct.unpack(st, d[p + sz * k:p + sz * (k + 1)])
                    if keyed:
                        if v[0] < 0:
                            continue
                        keys.append((v[0] / fs, tuple(x * scale for x in v[1:])))
                    else:
                        keys.append((float(k), tuple(x * scale for x in v)))
                p = (p + sz * nkey + 3) & ~3
                curves[idx] = AnmCurve(idx, fmt, keys)
            e = AnmEntry(cl, bi, kind, curves)
            if cl >= 0:
                e.target = self.clumps[cl][1][bi] if cl < len(self.clumps) and bi < len(self.clumps[cl][1]) else None
            else:
                e.target = self.others[bi] if bi < len(self.others) else None
            self.entries.append(e)
        self.parsed_size = p

    def entry_name(self, e):
        return e.target.name if e.target is not None else '?'


CHUNK_CLASSES['nuccChunkAnm'] = AnmChunk

# ----------------------------------------------------------------------------------------
# NUD
# ----------------------------------------------------------------------------------------


class NudMesh:
    """One material pass: parallel per-vertex lists + triangle list (indices into them)."""

    def __init__(self):
        self.positions = []
        self.normals = []
        self.uvs = []        # [[(u, v) per channel] per vertex]
        self.colors = []     # [(r, g, b, a) 0..255] or []
        self.bone_ids = []
        self.bone_weights = []
        self.faces = []
        self.materials = []  # NudMaterial


class NudMaterial:
    def __init__(self, flags, src, dst, alpha_test, alpha_func, ref_alpha, cull, zofs, textures, props):
        self.flags, self.src_factor, self.dst_factor = flags, src, dst
        self.alpha_test, self.alpha_function, self.ref_alpha, self.cull_mode, self.z_offset = alpha_test, alpha_func, ref_alpha, cull, zofs
        self.textures = textures    # [(unk0, map_mode, wrap_s, wrap_t)]
        self.properties = props     # {name: [floats]}


class NudMeshGroup:
    def __init__(self, name, bone_flags, single_bind, bounding, meshes):
        self.name, self.bone_flags, self.single_bind, self.bounding, self.meshes = name, bone_flags, single_bind, bounding, meshes


def _strip_to_tris(idx):
    tris = []
    i, n = 0, len(idx)
    while i + 2 < n:
        a, b = idx[i], idx[i + 1]
        j = i + 2
        flip = False
        while j < n and idx[j] != -1:
            c = idx[j]
            if a != b and b != c and a != c:
                tris.append((a, b, c) if not flip else (a, c, b))
            flip = not flip
            a, b = b, c
            j += 1
        i = j + 1
    return tris


class Nud:
    def __init__(self, d):
        if d[:4] != b'NDP3':
            raise ValueError('not a NUD')
        self.size, self.version, ngroups, self.bone_start, self.bone_end = struct.unpack('>IHHHH', d[4:16])
        poly_start, poly_size, vert_size, vadd_size = struct.unpack('>4I', d[16:32])
        poly_start += 0x30
        vert_start = poly_start + poly_size
        vadd_start = vert_start + vert_size
        name_start = vadd_start + vadd_size
        self.bounding = struct.unpack('>4f', d[32:48])
        self.mesh_groups = []
        p = 0x30
        groups = []
        for _ in range(ngroups):
            bs = struct.unpack('>8f', d[p:p + 32])
            noff, unk, bone_flags, single_bind, nmesh, mesh_off = struct.unpack('>IHHhHI', d[p + 32:p + 48])
            groups.append((bs, _cstr(d, name_start + noff), bone_flags, single_bind, nmesh))
            p += 48
        for bs, name, bone_flags, single_bind, nmesh in groups:
            meshes = []
            for _ in range(nmesh):
                m = self._read_mesh(d, p, poly_start, vert_start, vadd_start, name_start)
                meshes.append(m)
                p += 0x30
            self.mesh_groups.append(NudMeshGroup(name, bone_flags, single_bind, bs, meshes))

    @staticmethod
    def _read_mesh(d, p, poly_start, vert_start, vadd_start, name_start):
        po, vo, vao, nvert, vsize, uvsize = struct.unpack('>IIIHBB', d[p:p + 16])
        tex_props = struct.unpack('>4I', d[p + 16:p + 32])
        nface, fsize, fflag = struct.unpack('>HBB', d[p + 32:p + 36])
        m = NudMesh()
        m.face_flag = fflag
        idx = struct.unpack('>%dh' % nface, d[poly_start + po:poly_start + po + 2 * nface])
        m.faces = [(a, c, b) for a, b, c in zip(idx[0::3], idx[1::3], idx[2::3])] if fsize & 0x40 else _strip_to_tris(idx)
        bone_type, vtype = vsize & 0xF0, vsize & 0x0F
        uv_count, uv_type = uvsize >> 4, uvsize & 0x0F
        q = vert_start + vo
        colors, uvs = [], []
        if bone_type:
            for _ in range(nvert):
                if uv_type == 2:
                    colors.append(struct.unpack('>4B', d[q:q + 4]))
                    q += 4
                elif uv_type == 4:
                    colors.append(tuple(int(round(x * 255)) for x in struct.unpack('>4e', d[q:q + 8])))
                    q += 8
                uvs.append([struct.unpack('>2e', d[q + 4 * k:q + 4 * k + 4]) for k in range(uv_count)])
                q += 4 * uv_count
            q = vadd_start + vao
        for i in range(nvert):
            m.positions.append(struct.unpack('>3f', d[q:q + 12]))
            q += 12
            normal = None
            if vtype == 0:
                q += 4
            elif vtype == 1:
                normal = struct.unpack('>3f', d[q + 4:q + 16])
                q += 20
            elif vtype == 2:
                normal = struct.unpack('>3f', d[q:q + 12])
                q += 52
            elif vtype == 3:
                normal = struct.unpack('>3f', d[q + 4:q + 16])
                q += 52
            elif vtype == 6:
                normal = struct.unpack('>3e', d[q:q + 6])
                q += 8
            elif vtype == 7:
                normal = struct.unpack('>3e', d[q:q + 6])
                q += 24
            else:
                raise ValueError('vertex type %d' % vtype)
            m.normals.append(normal)
            if bone_type == 0:
                if uvsize >= 18:
                    colors.append(struct.unpack('>4B', d[q:q + 4]))
                    q += 4
                uvs.append([struct.unpack('>2e', d[q + 4 * k:q + 4 * k + 4]) for k in range(uv_count)])
                q += 4 * uv_count
            elif bone_type == 0x10:
                m.bone_ids.append(struct.unpack('>4I', d[q:q + 16]))
                m.bone_weights.append(struct.unpack('>4f', d[q + 16:q + 32]))
                q += 32
            elif bone_type == 0x20:
                m.bone_ids.append(struct.unpack('>4H', d[q:q + 8]))
                m.bone_weights.append(struct.unpack('>4e', d[q + 8:q + 16]))
                q += 16
            elif bone_type == 0x40:
                m.bone_ids.append(struct.unpack('>4B', d[q:q + 4]))
                m.bone_weights.append(tuple(b / 255.0 for b in d[q + 4:q + 8]))
                q += 8
            else:
                raise ValueError('bone type %#x' % bone_type)
        m.colors, m.uvs = colors, uvs
        for tp in tex_props:
            if tp == 0:
                break
            m.materials.append(Nud._read_material(d, tp, name_start))
        return m

    @staticmethod
    def _read_material(d, p, name_start):
        flags, _z, src, ntex, dst, atest, afunc, ref_alpha, cull, _u1, _u2, zofs = struct.unpack('>IIHHHBBHHffi', d[p:p + 32])
        q = p + 32
        textures = []
        for _ in range(ntex):
            unk0, _z, _h, map_mode, wrap_s, wrap_t = struct.unpack('>iIHHBB', d[q:q + 14])
            textures.append((unk0, map_mode, wrap_s, wrap_t))
            q += 24
        props = {}
        while True:
            att_size, noff = struct.unpack('>II', d[q:q + 8])
            nval = d[q + 11]
            name = _cstr(d, name_start + noff) if noff else ''
            vals = struct.unpack('>%df' % nval, d[q + 16:q + 16 + 4 * nval]) if nval else ()
            if name:
                props[name] = vals
            if att_size == 0:
                break
            q += att_size
        return NudMaterial(flags, src, dst, atest, afunc, ref_alpha, cull, zofs, textures, props)


# ----------------------------------------------------------------------------------------
# small matrix helpers (row-major 4x4 as tuples of rows; column vectors: M @ v)
# ----------------------------------------------------------------------------------------


def mat_mul(A, B):
    return tuple(tuple(sum(A[i][k] * B[k][j] for k in range(4)) for j in range(4)) for i in range(4))


def mat_apply(M, v):
    x, y, z = v
    return tuple(M[i][0] * x + M[i][1] * y + M[i][2] * z + M[i][3] for i in range(3))


def trs_matrix(t, r_deg, s, order='ZYX'):
    """T * R * S; `order` names the rotation applied first: 'ZYX' (the file's) is R = Rx * Ry * Rz."""
    rx, ry, rz = (math.radians(a) for a in r_deg)
    cx, sx, cy, sy, cz, sz = math.cos(rx), math.sin(rx), math.cos(ry), math.sin(ry), math.cos(rz), math.sin(rz)
    Rx = ((1, 0, 0), (0, cx, -sx), (0, sx, cx))
    Ry = ((cy, 0, sy), (0, 1, 0), (-sy, 0, cy))
    Rz = ((cz, -sz, 0), (sz, cz, 0), (0, 0, 1))
    seq = {'XYZ': (Rz, Ry, Rx), 'ZYX': (Rx, Ry, Rz)}[order]

    def m3(A, B):
        return tuple(tuple(sum(A[i][k] * B[k][j] for k in range(3)) for j in range(3)) for i in range(3))

    R = m3(m3(seq[0], seq[1]), seq[2])
    return tuple(tuple(R[i][j] * s[j] for j in range(3)) + (t[i],) for i in range(3)) + ((0, 0, 0, 1),)


def mat_inverse_rigid(M):
    """Inverse of a rigid (rotation + translation, unit scale) matrix."""
    R = [[M[j][i] for j in range(3)] for i in range(3)]
    t = [M[i][3] for i in range(3)]
    nt = [-(R[i][0] * t[0] + R[i][1] * t[1] + R[i][2] * t[2]) for i in range(3)]
    return tuple(tuple(R[i]) + (nt[i],) for i in range(3)) + ((0, 0, 0, 1),)


# ----------------------------------------------------------------------------------------
# CLI
# ----------------------------------------------------------------------------------------


def _info(x):
    for pi, page in enumerate(x.pages):
        for c in page.chunks:
            if c.kind in ('nuccChunkNull', 'nuccChunkPage'):
                continue
            extra = ''
            if isinstance(c, TextureChunk):
                extra = ' ' + ' '.join('%dx%d fmt%d mips%d' % (t.width, t.height, t.fmt, t.mips) for t in c.textures)
            elif isinstance(c, ModelChunk):
                nv = sum(len(m.positions) for g in c.nud.mesh_groups for m in g.meshes)
                nf = sum(len(m.faces) for g in c.nud.mesh_groups for m in g.meshes)
                extra = ' rig=%d bone=%s verts=%d tris=%d mats=%s' % (c.rigging_flag, c.coord.name if c.coord else None, nv, nf, [m.name for m in c.materials])
            elif isinstance(c, ClumpChunk):
                extra = ' coords=%d models=%d' % (len(c.coords), len(c.models))
            elif isinstance(c, MaterialChunk):
                extra = ' tex=%s floats=%s' % ([t.name for t in c.textures()], [round(f, 3) for f in c.floats])
            elif isinstance(c, CoordChunk):
                extra = ' p=%s r=%s s=%s' % (tuple(round(v, 2) for v in c.position), tuple(round(v, 1) for v in c.rotation), tuple(round(v, 2) for v in c.scale))
            print('p%-3d %-22s %-40s %s%s' % (pi, c.kind[9:], c.name, c.path, extra))


def _textures(x, out_dir, max_size):
    os.makedirs(out_dir, exist_ok=True)
    n = 0
    for c in x.by_kind('nuccChunkTexture'):
        for i, t in enumerate(c.textures):
            stem = c.name if len(c.textures) == 1 else '%s_%d' % (c.name, i)
            path = os.path.join(out_dir, stem + '.dds')
            with open(path, 'wb') as f:
                f.write(t.to_dds(max_size))
            print(path, '%dx%d fmt%d mips%d' % (t.width, t.height, t.fmt, t.mips))
            n += 1
    return n


def _bones(x):
    for cl in x.by_kind('nuccChunkClump'):
        W = cl.world_matrices()
        print('clump', cl.name, len(cl.coords), 'coords')
        for c in cl.coords:
            depth = 0
            q = c
            while q.parent is not None:
                q = q.parent
                depth += 1
            w = W[c]
            print('  %s%-30s world (%7.2f %7.2f %7.2f)  local p=%s r=%s' % ('  ' * depth, c.name, w[0][3], w[1][3], w[2][3],
                  tuple(round(v, 2) for v in c.position), tuple(round(v, 1) for v in c.rotation)))


def _anm(x, verbose=False):
    for c in x.by_kind('nuccChunkAnm'):
        print('anm %-40s %6.1f frames%s  clumps=%d others=%s entries=%d' % (
            c.name, c.frames, ' loop' if c.loop else '', len(c.clumps), [o.name for o in c.others if o is not None], len(c.entries)))
        for e in c.entries:
            animated = {i: (hex(cv.format), len(cv.keys)) for i, cv in e.curves.items() if not cv.constant}
            if not animated and not verbose:
                continue
            clump = c.clumps[e.clump][0].name if e.clump >= 0 and c.clumps[e.clump][0] else '-'
            tgt = e.target
            print('   %-6s %-34s %-34s %s' % ({1: 'coord', 2: 'camera', 4: 'mat', 5: 'ldirc', 6: 'lpoint'}.get(e.kind, e.kind),
                                         (tgt.name if tgt else '?')[:34], ('<- ' + clump)[:34], animated))


def main(argv):
    if len(argv) < 3:
        print('usage: xfbin_dump.py info|bones|anm [-v] <file.xfbin>\n       xfbin_dump.py textures <file.xfbin> <out dir> [max size]')
        return 2
    x = read_xfbin(argv[2])
    if argv[1] == 'info':
        _info(x)
    elif argv[1] == 'bones':
        _bones(x)
    elif argv[1] == 'anm':
        _anm(x, verbose='-v' in argv)
    elif argv[1] == 'textures':
        _textures(x, argv[3], int(argv[4]) if len(argv) > 4 else None)
    else:
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))


# ----------------------------------------------------------------------------------------
# DXT1 / DXT5 downsampling (block-space, for a texture without a mip chain)
# ----------------------------------------------------------------------------------------


def _rgb565_unpack(c):
    return ((c >> 11) & 31) << 3, ((c >> 5) & 63) << 2, (c & 31) << 3


def _rgb565_pack(r, g, b):
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)


def dxt_halve(data, width, height, fourcc):
    """Downsample a DXT1 / DXT5 level by 2 in each direction, in block space: every 2x2 group of
    4x4 blocks becomes one block whose 16 texels are the group's 2x2 texel averages. Texels are
    decoded exactly and re-encoded with a range fit (the block's min / max colour as endpoints,
    each texel snapped to the nearest of the 4 palette entries; alpha likewise with 8 entries) --
    plenty for a background sheet that only exists in 2048^2 without mips. Returns
    (bytes, width // 2, height // 2)."""
    bw, bh = max(1, width // 4), max(1, height // 4)
    bb = 8 if fourcc == b'DXT1' else 16
    nw, nh = max(1, width // 2), max(1, height // 2)
    nbw, nbh = max(1, nw // 4), max(1, nh // 4)

    def decode_block(off):
        """16 (r, g, b, a) texels of the block at `off`."""
        col = data[off + (8 if bb == 16 else 0):off + bb]
        c0, c1 = struct.unpack('<HH', col[:4])
        bits = struct.unpack('<I', col[4:8])[0]
        p0, p1 = _rgb565_unpack(c0), _rgb565_unpack(c1)
        if bb == 8 and c0 <= c1:
            pal = [p0, p1, tuple((a + b) // 2 for a, b in zip(p0, p1)), (0, 0, 0)]
            alpha_pal = [255, 255, 255, 0]
        else:
            pal = [p0, p1, tuple((2 * a + b) // 3 for a, b in zip(p0, p1)), tuple((a + 2 * b) // 3 for a, b in zip(p0, p1))]
            alpha_pal = [255] * 4
        out = []
        if bb == 16:
            a0, a1 = data[off], data[off + 1]
            abits = int.from_bytes(data[off + 2:off + 8], 'little')
            if a0 > a1:
                apal = [a0, a1] + [((7 - i) * a0 + i * a1) // 7 for i in range(1, 7)]
            else:
                apal = [a0, a1] + [((5 - i) * a0 + i * a1) // 5 for i in range(1, 5)] + [0, 255]
        for i in range(16):
            ci = (bits >> (2 * i)) & 3
            a = apal[(abits >> (3 * i)) & 7] if bb == 16 else alpha_pal[ci]
            out.append(pal[ci] + (a,))
        return out

    def encode_block(tex):
        """16 (r, g, b, a) -> block bytes (range fit). DXT1 has no alpha channel: a block with any
        texel under alpha 128 is written in its 3-colour + transparent mode (c0 <= c1, index 3 =
        transparent black) -- the opaque 4-colour mode turned every cut-out texel of a mip level
        black (ASBR's trees drew black boxes in the distance, 2026-10-26)."""
        punch = bb == 8 and any(t[3] < 128 for t in tex)
        solid = [t for t in tex if t[3] >= 128] if punch else tex
        if not solid:
            return struct.pack('<HHI', 0, 0, 0xFFFFFFFF)     # c0 == c1: 3-colour mode, all index 3
        rs = [t[0] for t in solid]
        gs = [t[1] for t in solid]
        bs = [t[2] for t in solid]
        lo = (min(rs), min(gs), min(bs))
        hi = (max(rs), max(gs), max(bs))
        c0, c1 = _rgb565_pack(*hi), _rgb565_pack(*lo)
        if punch:
            if c0 > c1:
                c0, c1 = c1, c0
            p0, p1 = _rgb565_unpack(c0), _rgb565_unpack(c1)
            pal = [p0, p1, tuple((a + b) // 2 for a, b in zip(p0, p1))]
            bits = 0
            for i, t in enumerate(tex):
                best = 3 if t[3] < 128 else min(range(3), key=lambda k: sum((pal[k][j] - t[j]) ** 2 for j in range(3)))
                bits |= best << (2 * i)
            return struct.pack('<HHI', c0, c1, bits)
        if c0 < c1:
            c0, c1 = c1, c0
        p0, p1 = _rgb565_unpack(c0), _rgb565_unpack(c1)
        if c0 == c1:
            pal = [p0, p0, p0, p0]
        else:
            pal = [p0, p1, tuple((2 * a + b) // 3 for a, b in zip(p0, p1)), tuple((a + 2 * b) // 3 for a, b in zip(p0, p1))]
        bits = 0
        for i, t in enumerate(tex):
            best = min(range(4), key=lambda k: sum((pal[k][j] - t[j]) ** 2 for j in range(3)))
            bits |= best << (2 * i)
        colour = struct.pack('<HHI', c0, c1, bits)
        if bb == 8:
            return colour
        als = [t[3] for t in tex]
        a0, a1 = max(als), min(als)
        if a0 == a1:
            apal = [a0] * 8
        else:
            apal = [a0, a1] + [((7 - i) * a0 + i * a1) // 7 for i in range(1, 7)]
        abits = 0
        for i, a in enumerate(als):
            best = min(range(8), key=lambda k: abs(apal[k] - a))
            abits |= best << (3 * i)
        return bytes([a0, a1]) + abits.to_bytes(6, 'little') + colour

    out = bytearray()
    for by in range(nbh):
        for bx in range(nbw):
            # the four source blocks (2x2) -> an 8x8 texel tile -> 4x4 averages
            tile = [[None] * 8 for _ in range(8)]
            for dy in range(2):
                for dx in range(2):
                    sx, sy = 2 * bx + dx, 2 * by + dy
                    if sx < bw and sy < bh:
                        tex = decode_block((sy * bw + sx) * bb)
                        for i in range(16):
                            tile[4 * dy + i // 4][4 * dx + i % 4] = tex[i]
            avg = []
            for ty in range(4):
                for tx in range(4):
                    q = [tile[2 * ty + j][2 * tx + k] or (0, 0, 0, 0) for j in range(2) for k in range(2)]
                    avg.append(tuple(sum(t[c] for t in q) // 4 for c in range(4)))
            out += encode_block(avg)
    return bytes(out), nw, nh


def dxt_chain(data, width, height, fourcc, max_size):
    """[(w, h, bytes)]: the level halved until <= max_size, then halved down to 4 px (a mip chain
    for a texture that shipped without one)."""
    levels = []
    w, h, d = width, height, data
    while max(w, h) > max_size:
        d, w, h = dxt_halve(d, w, h, fourcc)
    levels.append((w, h, d))
    while max(w, h) > 4:
        d, w, h = dxt_halve(d, w, h, fourcc)
        levels.append((w, h, d))
    return levels
