#!/usr/bin/env python3
"""KTMDL (.model) parser / dumper for DDR (A3 / World) 3D models.

Reference implementation of the layout documented in
docs/3d_model_format_research.md (section 3). Validated against every .model
in a DDR A3 install (284 files, all version 2.2).

Usage:
    ktmdl_dump.py <file.model> [--verts N] [--json]
    ktmdl_dump.py --survey <dir>          # parse every *.model under <dir>, report anomalies

The module is import-safe: `from ktmdl_dump import parse_model, pack_identity, decode6`.
"""
import argparse
import glob
import json
import math
import os
import struct
import sys

MAGIC = b"KTMDL\0\0\0"

# ---------------------------------------------------------------------------
# 6-bit name packing (doc section 2)
# ---------------------------------------------------------------------------
SPECIAL_TOKENS = ["alp", "nrm", "rep", "cube", "dxt", "merged"]


def pack_identity(name):
    """Bone/material/texture-node identity: one u64, 10 six-bit slots, XOR-folded.
    Bits 63..60 = number of dropped (non-alphanumeric) characters."""
    v, i, dropped = 0, 0, 0
    for ch in name.lower():
        if "a" <= ch <= "z":
            c = ord(ch) - 0x60
        elif "0" <= ch <= "9":
            c = ord(ch) - 0x14
        else:
            dropped += 1
            continue
        v ^= c << (54 - 6 * (i % 10))
        i += 1
    return v | ((dropped & 0xF) << 60)


def pack_texname(name):
    """Texture *file* name: two u64s decoded sequentially (up to 20 chars, no fold)."""
    codes = []
    for ch in name.lower():
        if "a" <= ch <= "z":
            codes.append(ord(ch) - 0x60)
        elif "0" <= ch <= "9":
            codes.append(ord(ch) - 0x14)
    if len(codes) > 20:
        raise ValueError("texture name too long after stripping: %r" % name)
    words = [0, 0]
    for i, c in enumerate(codes):
        words[i // 10] |= c << (54 - 6 * (i % 10))
    return words[0], words[1]


def decode6(v):
    out = ""
    for sh in range(54, -1, -6):
        c = (v >> sh) & 0x3F
        if c == 0:
            continue
        if c < 0x1C:
            out += chr(c + 0x60)
        elif c < 0x26:
            out += chr(c + 0x14)
        elif c - 0x26 < len(SPECIAL_TOKENS):
            out += SPECIAL_TOKENS[c - 0x26]
        else:
            out += "?"
    return out


def decode_texname(w0, w1):
    return decode6(w0) + decode6(w1)


def fnv1(s):
    h = 0x811C9DC5
    for b in s.encode():
        h = (h * 0x01000193) & 0xFFFFFFFF
        h ^= b
    return h


def texture_registry_key(basename):
    """Key the DDS loader registers a texture under (FUN_180145b80 + FNV-1)."""
    return fnv1(basename.lower().replace("_", ""))


# ---------------------------------------------------------------------------
# D3D9 declaration tables (doc section 3.6)
# ---------------------------------------------------------------------------
DECL_TYPES = {
    0: "FLOAT1", 1: "FLOAT2", 2: "FLOAT3", 3: "FLOAT4", 5: "UBYTE4N", 6: "SHORT2N",
    7: "SHORT4N", 8: "USHORT2N", 9: "USHORT4N", 0xB: "UBYTE4", 0xC: "SHORT2",
    0xD: "SHORT4", 0x10: "FLOAT16_2", 0x11: "FLOAT16_4", 0x12: "D3DCOLOR",
    0x14: "UDEC3", 0x15: "DEC3N",
}
DECL_USAGES = {
    0x10: "POSITION", 0x11: "POSITION", 0x12: "NORMAL", 0x13: "COLOR0", 0x14: "COLOR1",
    0x16: "TEXCOORD0", 0x17: "TEXCOORD1", 0x18: "TEXCOORD2", 0x19: "TEXCOORD3",
    0x1A: "TEXCOORD4", 0x1B: "TEXCOORD5", 0x1C: "TEXCOORD6", 0x1D: "TEXCOORD7",
    0x1E: "BINORMAL", 0x1F: "TANGENT", 0x20: "BLENDWEIGHT", 0x21: "BLENDINDICES",
    0x22: "PSIZE", 0x23: "TESSFACTOR",
}
PRIMITIVES = {0: "TRIANGLESTRIP", 1: "TRIANGLELIST", 2: "TRIANGLEFAN", 3: "LINELIST", 4: "LINESTRIP"}
SHADER_NAMES = [
    "mdl_ch_constant", "mdl_ch_constant_c", "mdl_ch_constant_c_vc", "mdl_ch_constant_vc",
    "mdl_ch_constant_vc_notex", "mdl_ch_lambert", "mdl_bg_constant", "mdl_bg_constant_c",
    "mdl_bg_constant_c_vc", "mdl_bg_constant_vc", "mdl_bg_constant_vc_notex", "mdl_bg_lambert",
    "gs_model_default", "gs_model_skinning_default",
]
SHADER_BY_HASH = {fnv1(n): n for n in SHADER_NAMES}

# Mesh flag word (doc 3.3) -> render states, as applied by the game's draw loop
# (FUN_18018ac50 mask -> FUN_1801780b0 -> D3D9 SetRenderState).
MESH_FLAG_TWO_SIDED = 0x0001      # D3DRS_CULLMODE = NONE
MESH_FLAG_NO_ZTEST = 0x0020       # D3DRS_ZENABLE = FALSE
MESH_FLAG_TRANSPARENT = 0x0040    # TRANS pass (back-to-front), default blend = alpha
MESH_FLAG_NO_ZWRITE = 0x0400      # D3DRS_ZWRITEENABLE = FALSE
MESH_FLAG_NO_BLEND_GROUP = 0x0800 # suppresses every blend mode (incl. 0x40's implicit alpha)
MESH_FLAGS2_NO_ALPHATEST = 0x01   # D3DRS_ALPHATESTENABLE = FALSE
MESH_FLAGS2_BLEND_MASK = 0x3E     # 2 = alpha, 4 = additive, 8 = subtractive
BLEND_MODES = {0: "none", 2: "alpha", 4: "additive", 8: "subtractive"}


def decode_mesh_flags(flags, flags2):
    """Translate a mesh's (flags u16, flags2 u8) into the render states the game sets."""
    blend = "none"
    if not flags & MESH_FLAG_NO_BLEND_GROUP:
        grp = flags2 & MESH_FLAGS2_BLEND_MASK
        if grp in (2, 4, 8):
            blend = BLEND_MODES[grp]
        elif grp == 0 and flags & MESH_FLAG_TRANSPARENT:
            blend = "alpha"
    return dict(
        two_sided=bool(flags & MESH_FLAG_TWO_SIDED),
        ztest=not flags & MESH_FLAG_NO_ZTEST,
        zwrite=not flags & MESH_FLAG_NO_ZWRITE,
        transparent_pass=bool(flags & MESH_FLAG_TRANSPARENT),
        alpha_test=not flags2 & MESH_FLAGS2_NO_ALPHATEST,
        alpha_ref=0x7F if blend == "none" else 0,
        blend=blend,
        unread_bits=flags & ~(0x0001 | 0x0020 | 0x0040 | 0x0400 | 0x0800 | 0xF000) | ((flags2 & ~0x3F) << 16),
    )


def parse_rlist(data: bytes):
    """Konami 'MRL0' resource list (startup.arc data/*/*_resources.rlist): list of (key, [fields])."""
    if data[:4] != b"MRL0" or data[4:6] != b"LE":
        raise ValueError("not an MRL0 little-endian rlist")
    count, total = struct.unpack_from("<II", data, 8)
    if total != len(data):
        raise ValueError("rlist size mismatch")

    def cstr(o):
        return data[o:data.index(b"\0", o)].decode("ascii", "replace")

    off, rows = 0x10, []
    for _ in range(count):
        str_off, nfields, rec_len = struct.unpack_from("<III", data, off)
        offs = struct.unpack_from("<%dI" % nfields, data, off + 12)
        rows.append((cstr(off + str_off), [cstr(off + o) for o in offs]))
        off += rec_len
    return rows


def write_rlist(rows):
    """rows = iterable of (key, [field strings]) -> MRL0 bytes. Layout matches the stock
    files byte-for-byte (verified on all four A3 startup.arc rlists): per record
    ``{u32 key_off = 12 + 4n ; u32 n ; u32 rec_len ; u32 field_off[n] ; key\\0 ; fields\\0… ;
    pad to 4}``, offsets relative to the record start. Numeric fields are written as the
    strings given (the game parses them with atof), so pass e.g. "0.9" not 0.9."""
    out = bytearray(16)
    rows = list(rows)
    for key, fields in rows:
        fields = [str(f) for f in fields]
        n = len(fields)
        head = 12 + 4 * n
        strings = bytearray()
        offs = []
        strings += str(key).encode("ascii") + b"\0"
        for f in fields:
            offs.append(head + len(strings))
            strings += f.encode("ascii") + b"\0"
        rec_len = head + len(strings)
        rec_len += (-rec_len) % 4
        rec = bytearray(rec_len)
        struct.pack_into("<III", rec, 0, head, n, rec_len)
        struct.pack_into("<%dI" % n, rec, 12, *offs)
        rec[head:head + len(strings)] = strings
        out += rec
    struct.pack_into("<4s2sHII", out, 0, b"MRL0", b"LE", 0, len(rows), len(out))
    return bytes(out)


def upsert_rlist_row(rows, key, fields):
    """Replace the row with ``key`` (keeping its position) or append a new one; returns the
    new list. Used to register a new character/stage in an existing rlist."""
    out = []
    done = False
    for k, f in rows:
        if k == key and not done:
            out.append((key, list(fields)))
            done = True
        else:
            out.append((k, list(f)))
    if not done:
        out.append((key, list(fields)))
    return out


def half_to_float(h):
    s = (h >> 15) & 1
    e = (h >> 10) & 0x1F
    m = h & 0x3FF
    if e == 0:
        v = (m / 1024.0) * 2.0 ** -14
    elif e == 31:
        v = float("inf") if m == 0 else float("nan")
    else:
        v = (1.0 + m / 1024.0) * 2.0 ** (e - 15)
    return -v if s else v


# ---------------------------------------------------------------------------
# Parser
# ---------------------------------------------------------------------------
class Reader:
    def __init__(self, data):
        self.d = data

    def u8(self, o): return self.d[o]
    def u16(self, o): return struct.unpack_from("<H", self.d, o)[0]
    def i16(self, o): return struct.unpack_from("<h", self.d, o)[0]
    def u32(self, o): return struct.unpack_from("<I", self.d, o)[0]
    def i32(self, o): return struct.unpack_from("<i", self.d, o)[0]
    def u64(self, o): return struct.unpack_from("<Q", self.d, o)[0]
    def f32(self, o): return struct.unpack_from("<f", self.d, o)[0]
    def f32s(self, o, n): return list(struct.unpack_from("<%df" % n, self.d, o))
    def cstr(self, o): return self.d[o:self.d.index(b"\0", o)].decode("ascii", "replace")


def _parse_bufdesc(r, o):
    return dict(
        offset=o, data_offset=o + r.i32(o), count=r.u32(o + 4), is_vb=r.u8(o + 8),
        stride=r.u8(o + 9), element_count=r.u8(o + 0xA), is_vb2=r.u8(o + 0xB),
        elements_offset=(o + r.i32(o + 0xC)) if r.i32(o + 0xC) else 0,
        f10=r.u32(o + 0x10), type=r.u32(o + 0x14), f18=r.u32(o + 0x18), f1c=r.u32(o + 0x1C),
    )


def _parse_elements(r, o, n):
    out = []
    for i in range(n):
        e = o + i * 4
        out.append(dict(stream=r.u8(e), offset=r.u8(e + 1), type_raw=r.u8(e + 2),
                        usage_raw=r.u8(e + 3),
                        type=DECL_TYPES.get(r.u8(e + 2), "?%02x" % r.u8(e + 2)),
                        usage=DECL_USAGES.get(r.u8(e + 3), "?%02x" % r.u8(e + 3))))
    return out


def parse_model(data: bytes) -> dict:
    r = Reader(data)
    if data[:8] != MAGIC:
        raise ValueError("not a KTMDL file")
    H: dict = dict(
        version=(r.u32(8), r.u32(0xC)), flag10=r.u16(0x10),
        bone_count=r.u32(0x18), bone_off=r.u32(0x1C),
        palette_count=r.u32(0x20), palette_off=r.u32(0x24),
        mesh_count=r.u32(0x28), mesh_off=r.u32(0x2C),
        bufdesc_count=r.u32(0x30), bufdesc_off=r.u32(0x34),
        node_count=r.u32(0x40), node_off=r.u32(0x44),
        material_count=r.u32(0x48), material_off=r.u32(0x4C),
        texture_count=r.u32(0x50), texture_off=r.u32(0x54),
        info_count=r.u32(0x58), info_off=r.u32(0x5C),
        debug_off=r.u32(0x60), blob_size=r.u32(0x74), vdata_off=r.u32(0x78),
        texname_count=r.u32(0x7C), texname_off=r.u32(0x80),
        element_count=r.u32(0x88), element_off=r.u32(0x8C),
        file_size=r.u32(0x90), const98=r.u32(0x98),
    )
    if H["version"][0] != 2 or H["version"][1] < 2 or H["flag10"] != 1:
        raise ValueError("unsupported KTMDL version %r" % (H["version"],))

    bones: list = []
    for i in range(H["bone_count"]):
        b = H["bone_off"] + i * 0xB0
        bones.append(dict(
            index=i, identity=r.u64(b), identity_hi=r.u64(b + 8), name_folded=decode6(r.u64(b)),
            bind=r.f32s(b + 0x10, 16), inverse_bind=r.f32s(b + 0x50, 16),
            aabb_max=r.f32s(b + 0x90, 3), aabb_min=r.f32s(b + 0xA0, 3),
            parent=r.i16(b + 0xAC), flags=r.u16(b + 0xAE),
        ))

    meshes: list = []
    for i in range(H["mesh_count"]):
        m = H["mesh_off"] + i * 0x60
        npal = r.u32(m + 0x18)
        pal_off = m + r.i32(m + 0x1C)
        n_streams, n_ibs = r.u32(m + 0x10), r.u32(m + 0x20)
        M: dict = dict(
            index=i, flags=r.u16(m), primitive_raw=r.u8(m + 2),
            primitive=PRIMITIVES.get(r.u8(m + 2), "POINTLIST"), flags2=r.u8(m + 3),
            material=r.u32(m + 4), node=r.u16(m + 8), stream_count=n_streams,
            palette=[r.u16(pal_off + j * 2) for j in range(npal)],
            index_buffer_count=n_ibs, texture_slot_count=r.u8(m + 0x2C),
            texture_slots=[r.u32(m + 0x30 + j * 4) for j in range(8)],
            bounding_sphere=r.f32s(m + 0x50, 4),
            render=decode_mesh_flags(r.u16(m), r.u8(m + 3)),
        )
        M["vertex_buffers"] = [_parse_bufdesc(r, m + r.i32(m + 0x14) + j * 0x20) for j in range(n_streams)]
        M["index_buffers"] = [_parse_bufdesc(r, m + r.i32(m + 0x24) + j * 0x20) for j in range(n_ibs)]
        for vb in M["vertex_buffers"]:
            vb["elements"] = _parse_elements(r, vb["elements_offset"], vb["element_count"]) if vb["elements_offset"] else []
        meshes.append(M)

    nodes: list = []
    for i in range(H["node_count"]):
        n = H["node_off"] + i * 0x30
        nodes.append(dict(f0=r.u32(n), id=r.u32(n + 4), f8=r.u32(n + 8), parent=r.u16(n + 0xC),
                          fe=r.u16(n + 0xE), bbox_max=r.f32s(n + 0x10, 4), bbox_min=r.f32s(n + 0x20, 4)))

    materials: list = []
    for i in range(H["material_count"]):
        m = H["material_off"] + i * 0xA0
        n = r.u16(m + 0x10)
        # Params start at +0x20 (the loader copies mat+0x20.. into the runtime record and
        # uploads them verbatim as VS c24.. / PS c3..); +0x18..+0x1F is an always-zero pad.
        materials.append(dict(index=i, identity=r.u64(m), name_folded=decode6(r.u64(m)),
                              param_count=n, shader_hash=r.u32(m + 0x14), pad18=r.u64(m + 0x18),
                              shader=SHADER_BY_HASH.get(r.u32(m + 0x14)),
                              params=[r.f32s(m + 0x20 + j * 16, 4) for j in range(n)]))

    texnames: list = []
    for i in range(H["texname_count"]):
        t = H["texname_off"] + i * 0x10
        texnames.append(dict(words=(r.u64(t), r.u64(t + 8)), name=decode_texname(r.u64(t), r.u64(t + 8))))

    textures: list = []
    for i in range(H["texture_count"]):
        t = H["texture_off"] + i * 0x50
        idx = r.u16(t + 0xE)
        tex_name = texnames[idx]["name"] if idx < len(texnames) else None
        textures.append(dict(index=i, identity=r.u64(t), node_name_folded=decode6(r.u64(t)),
                             sampler_bytes=list(data[t + 8:t + 0xE]), kind=r.u8(t + 0xD),
                             texname_index=idx,
                             name=tex_name,
                             f10=r.f32(t + 0x10), f14=r.f32(t + 0x14)))

    io = H["info_off"]
    info = dict(const=r.u64(io), bbox_max=r.f32s(io + 0x10, 4), bbox_min=r.f32s(io + 0x20, 4))

    dbg = H["debug_off"]
    debug: dict = dict(texture_names=[], shader_ids=[], shader_names=[])
    if dbg:
        tex_tab = dbg + r.u32(dbg + 4)
        shd_tab = dbg + r.u32(dbg + 8)
        cnt = r.u32(dbg + 0xC)
        ids = dbg + r.u32(dbg + 0x10)
        ntex = (ids - tex_tab) // 4 if ids > tex_tab else 0
        # texture-name table: offsets are relative to the table; strings follow
        for j in range(ntex):
            so = tex_tab + r.u32(tex_tab + j * 4)
            if so >= ids:  # offsets table ends where strings start
                break
            debug["texture_names"].append(r.cstr(so))
        debug["shader_ids"] = [r.u32(ids + j * 4) for j in range(cnt)]
        debug["shader_names"] = [r.cstr(shd_tab + r.u32(shd_tab + j * 4)) for j in range(cnt)]
        # resolve material shader names through the debug table (what the game does)
        for mat in materials:
            if mat["shader_hash"] in debug["shader_ids"]:
                mat["shader_debug_name"] = debug["shader_names"][debug["shader_ids"].index(mat["shader_hash"])]

    return dict(header=H, bones=bones, meshes=meshes, nodes=nodes, materials=materials,
                texnames=texnames, textures=textures, info=info, debug=debug, data=data)


# ---------------------------------------------------------------------------
# Writer (doc section 3, layout rules verified byte-for-byte on all 284 stock files:
# sections in the fixed order below, 16-byte aligned, element lists de-duplicated,
# ONE shared bone palette, blob buffers 16-byte aligned, 16 zero bytes after the blob)
# ---------------------------------------------------------------------------
KTMDL_CONST_98 = 0x7D931373
KTMDL_INFO_CONST = 0x11CE18154C50230F
NODE_ID_ROOT = 0x034F1053
NODE_ID_CHILD = 0x11D23D53


def _align(n, a=16):
    return (n + a - 1) & ~(a - 1)


def _pad(buf, a=16):
    buf += b"\0" * (_align(len(buf), a) - len(buf))


def invert_matrix4(m):
    """Inverse of a 4x4 (16 floats, row-major) via Gauss-Jordan — for inverse bind matrices."""
    a = [list(m[i * 4:i * 4 + 4]) + [1.0 if i == j else 0.0 for j in range(4)] for i in range(4)]
    for c in range(4):
        p = max(range(c, 4), key=lambda r_: abs(a[r_][c]))
        a[c], a[p] = a[p], a[c]
        piv = a[c][c]
        if abs(piv) < 1e-12:
            raise ValueError("singular bind matrix")
        a[c] = [x / piv for x in a[c]]
        for r_ in range(4):
            if r_ != c and a[r_][c]:
                f = a[r_][c]
                a[r_] = [x - f * y for x, y in zip(a[r_], a[c])]
    return [a[i][4 + j] for i in range(4) for j in range(4)]


PALETTE_BLOCK = 52  # the loader copies 0x34 u16 slots per palette block (FUN_180189900)


def write_model(spec):
    """Serialize a KTMDL v2.2 file.

    spec = dict(
      bones=[dict(identity=u64, bind=[16 f32], inverse_bind=[16]|None, aabb_min=[3], aabb_max=[3],
                  parent=int(-1 root), flags=u16|None (default 0xFFFF for roots, 0 otherwise))],
      palette=[global bone index, ...],           # the shared table (<= 52) used by meshes without their own
      meshes=[dict(flags=u16, flags2=u8, primitive_raw=u8 (1 = TRIANGLELIST), material=int, node=int,
                   texture_slots=[u32]*<=8 (low u16 = texture index), texture_slot_count=int|None,
                   bounding_sphere=[cx, cy, cz, r],
                   elements=[(stream, offset, type_raw, usage_raw), ...], stride=int,
                   vertex_count=int, vertex_data=bytes, index_count=int, index_data=bytes,
                   palette_count=int|None,
                   palette=[global bone index, ...]|None)],   # per-mesh palette (<= 52); None = the shared one
      nodes=[dict(f0=u32, id=u32, f8=u32, parent=u16, fe=u16, bbox_max=[4], bbox_min=[4])],
      info=dict(bbox_max=[4], bbox_min=[4]),
      materials=[dict(identity=u64, shader_hash=u32|None, shader=str|None, params=[[4 f32], ...])],
      texnames=[str packed-name | (w0, w1)],
      textures=[dict(identity=u64, kind=u8, texname_index=int, f10=1.0, f14=1.0, sampler_bytes=None)],
      debug=dict(texture_names=[str], shader_names=[str]),   # ids = FNV-1(shader name)
    )
    Palettes: when every mesh uses the same table the stock single-table layout is written
    (byte-identical to every stock file, header palette_count = 1). Otherwise the distinct
    tables are laid out as consecutive 52-slot blocks (zero-padded), header palette_count =
    the block count, and each mesh's +0x1C points at its block — the loader remaps blend
    indices through the mesh's own slice (FUN_18018a340) and copies palette_count x 52
    slots for the skinned-mesh cull (FUN_180189900), so this is what lets a model reference
    more than 52 bones in total.
    Returns bytes.
    """
    bones = spec["bones"]
    shared_palette = list(spec["palette"])
    meshes = spec["meshes"]
    nodes: list = spec.get("nodes") or [dict(f0=0, id=NODE_ID_ROOT, f8=0, parent=0xFFFF, fe=0xFFFF,
                                             bbox_max=list(spec["info"]["bbox_max"]),
                                             bbox_min=list(spec["info"]["bbox_min"]))]
    materials = spec["materials"]
    texnames = spec.get("texnames", [])
    textures = spec.get("textures", [])
    debug = spec.get("debug") or dict(texture_names=[], shader_names=[])
    if len(bones) > 255:
        raise ValueError("more than 255 bones (blend indices are bytes after remap)")
    # distinct palettes in order of first use
    mesh_pals = [list(me["palette"]) if me.get("palette") is not None else shared_palette for me in meshes]
    distinct: list = []
    for p in mesh_pals:
        if p not in distinct:
            distinct.append(p)
    if not distinct:
        distinct = [shared_palette]
    for p in distinct:
        if len(p) > PALETTE_BLOCK:
            raise ValueError("bone palette has %d entries; the loader copies at most %d" % (len(p), PALETTE_BLOCK))
    mesh_pal_index = [distinct.index(p) for p in mesh_pals]

    out = bytearray(0xC0)
    # --- bones
    bone_off = len(out)
    for i, b in enumerate(bones):
        inv = b.get("inverse_bind") or invert_matrix4(b["bind"])
        flags = b.get("flags")
        if flags is None:
            flags = 0xFFFF if b["parent"] < 0 else 0
        out += struct.pack("<QQ", b["identity"], 0)
        out += struct.pack("<16f", *b["bind"]) + struct.pack("<16f", *inv)
        out += struct.pack("<3ff", *b["aabb_max"], 0.0) + struct.pack("<3f", *b["aabb_min"])
        out += struct.pack("<hH", b["parent"], flags)
    # --- palette(s): one tight table (stock shape) or N x 52-slot blocks
    palette_off = len(out)
    block_offs = []
    if len(distinct) == 1:
        block_offs.append(palette_off)
        out += struct.pack("<%dH" % len(distinct[0]), *distinct[0])
    else:
        for p in distinct:
            block_offs.append(len(out))
            out += struct.pack("<%dH" % PALETTE_BLOCK, *(list(p) + [0] * (PALETTE_BLOCK - len(p))))
    _pad(out)
    # --- meshes + bufdescs + elements (layout dedupe in order of first appearance)
    mesh_off = len(out)
    bufdesc_off = mesh_off + 0x60 * len(meshes)
    element_off = bufdesc_off + 0x40 * len(meshes)
    layouts = []
    for me in meshes:
        L = tuple(tuple(e) for e in me["elements"])
        if L not in layouts:
            layouts.append(L)
    elem_pos = {}
    o = element_off
    for L in layouts:
        elem_pos[L] = o
        o += 4 * len(L)
    elements_end = o
    node_off = _align(elements_end)
    info_off = node_off + 0x30 * len(nodes)
    material_off = info_off + 0x30
    texname_off = material_off + 0xA0 * len(materials)
    texture_off = texname_off + 0x10 * len(texnames)
    debug_off = texture_off + 0x50 * len(textures)

    # debug block bytes (needed to know where the blob starts)
    tex_strings = debug.get("texture_names", [])
    sh_strings = debug.get("shader_names", [])
    tex_tab = bytearray()
    so = 4 * len(tex_strings)
    for s in tex_strings:
        tex_tab += struct.pack("<I", so)
        so += len(s) + 1
    for s in tex_strings:
        tex_tab += s.encode("ascii") + b"\0"
    ids = b"".join(struct.pack("<I", fnv1(s)) for s in sh_strings)
    sh_tab = bytearray()
    so = 4 * len(sh_strings)
    for s in sh_strings:
        sh_tab += struct.pack("<I", so)
        so += len(s) + 1
    for s in sh_strings:
        sh_tab += s.encode("ascii") + b"\0"
    dbg = struct.pack("<IIIII", 0, 0x14, 0x14 + len(tex_tab) + len(ids), len(sh_strings), 0x14 + len(tex_tab))
    dbg = dbg + bytes(tex_tab) + ids + bytes(sh_tab)
    vdata_off = _align(debug_off + len(dbg))

    # blob layout: vb0, ib0, vb1, ib1, ... each 16-byte aligned
    blob = bytearray()
    buf_offs = []
    for me in meshes:
        vb = me["vertex_data"]
        if len(vb) != me["vertex_count"] * me["stride"]:
            raise ValueError("vertex_data size != count*stride")
        ib = me["index_data"]
        if len(ib) != 2 * me["index_count"]:
            raise ValueError("index_data size != 2*count")
        _pad(blob)
        vo = vdata_off + len(blob)
        blob += vb
        _pad(blob)
        io = vdata_off + len(blob)
        blob += ib
        buf_offs.append((vo, io))
    _pad(blob)
    blob_size = len(blob)

    # meshes
    for i, me in enumerate(meshes):
        mo = mesh_off + 0x60 * i
        vb_desc = bufdesc_off + 0x40 * i
        ib_desc = vb_desc + 0x20
        slots = list(me.get("texture_slots", []))[:8]
        nslots = me.get("texture_slot_count")
        if nslots is None:
            nslots = len(slots)
        slots += [0] * (8 - len(slots))
        rec = struct.pack("<HBBIHHI", me["flags"], me.get("primitive_raw", 1), me.get("flags2", 0),
                          me["material"], me.get("node", 0), 0, 0)
        rec += struct.pack("<Ii", 1, vb_desc - mo)
        npal = me.get("palette_count") or len(mesh_pals[i])
        rec += struct.pack("<Ii", npal, block_offs[mesh_pal_index[i]] - mo)
        rec += struct.pack("<Ii", 1, ib_desc - mo)
        rec += struct.pack("<IB3x", 0, nslots)
        rec += struct.pack("<8I", *slots)
        rec += struct.pack("<4f", *me["bounding_sphere"])
        assert len(rec) == 0x60
        out += rec
    # bufdescs
    for i, me in enumerate(meshes):
        vb_desc = bufdesc_off + 0x40 * i
        ib_desc = vb_desc + 0x20
        vo, io = buf_offs[i]
        L = tuple(tuple(e) for e in me["elements"])
        out += struct.pack("<iIBBBBiIIII", vo - vb_desc, me["vertex_count"], 1, me["stride"], len(L), 1,
                           elem_pos[L] - vb_desc, 0, 0, 0, 0)
        out += struct.pack("<iIBBBBiIIII", io - ib_desc, me["index_count"], 0, 0, 0, 0, 0, 0, 1, 0, 0)
    # elements
    for L in layouts:
        for (stream, offset, type_raw, usage_raw) in L:
            out += struct.pack("<BBBB", stream, offset, type_raw, usage_raw)
    _pad(out)
    assert len(out) == node_off
    # nodes
    for nd in nodes:
        out += struct.pack("<IIIHH", nd.get("f0", 0), nd["id"], nd.get("f8", 0), nd["parent"], nd["fe"])
        out += struct.pack("<4f", *nd["bbox_max"]) + struct.pack("<4f", *nd["bbox_min"])
    # info
    out += struct.pack("<QQ", KTMDL_INFO_CONST, 0)
    out += struct.pack("<4f", *spec["info"]["bbox_max"]) + struct.pack("<4f", *spec["info"]["bbox_min"])
    # materials
    for m in materials:
        h = m.get("shader_hash")
        if h is None:
            h = fnv1(m["shader"])
        params = m["params"]
        if not 1 <= len(params) <= 8:
            raise ValueError("material needs 1..8 float4 params")
        out += struct.pack("<QIIHHIQ", m["identity"], 0, 0, len(params), 0, h, 0)
        for p in params:
            out += struct.pack("<4f", *p)
        out += b"\0" * (16 * (8 - len(params)))
    # texnames
    for t in texnames:
        w0, w1 = pack_texname(t) if isinstance(t, str) else t
        out += struct.pack("<QQ", w0, w1)
    # textures
    for t in textures:
        sb = t.get("sampler_bytes") or [0, 0, 2, 2, 2]
        out += struct.pack("<Q", t["identity"]) + bytes(sb[:5]) + struct.pack("<BH", t["kind"], t["texname_index"])
        out += struct.pack("<ff", t.get("f10", 1.0), t.get("f14", 1.0))
        out += b"\0" * 0x38
    # debug
    assert len(out) == debug_off
    out += dbg
    _pad(out)
    assert len(out) == vdata_off
    out += blob
    out += b"\0" * 16
    file_size = len(out)

    # header
    struct.pack_into("<8sIIHHI", out, 0, MAGIC, 2, 2, 1, 0, 0)
    struct.pack_into("<IIIIIIIIIIII", out, 0x18,
                     len(bones), bone_off, len(distinct), palette_off, len(meshes), mesh_off,
                     2 * len(meshes), bufdesc_off, 0, mesh_off, len(nodes), node_off)
    struct.pack_into("<IIIIIIII", out, 0x48, len(materials), material_off, len(textures), texture_off,
                     1, info_off, debug_off, bone_off)
    struct.pack_into("<IIIIIIII", out, 0x68, 0, bufdesc_off, element_off, blob_size, vdata_off,
                     len(texnames), texname_off, 0)
    struct.pack_into("<IIIII", out, 0x88, sum(len(L) for L in layouts), element_off,
                     file_size, 0, KTMDL_CONST_98)
    return bytes(out)


# ---------------------------------------------------------------------------
# DDS writer — uncompressed A8R8G8B8 with a 3-level mip chain, the shape of 140 stock
# textures. The game's DDS reader (FUN_180164800) also accepts DXT1/3/5, X8R8G8B8,
# R5G6B5, A4R4G4B4, A8, L8, A8L8; uncompressed 32-bit needs no encoder.
# ---------------------------------------------------------------------------
def write_dds_a8r8g8b8(width, height, rgba_rows, mip_levels=3):
    """rgba_rows: list of `height` rows (TOP row first), each a bytes-like of width*4 RGBA
    bytes. Returns the DDS file bytes (BGRA in memory, D3DFMT_A8R8G8B8)."""
    if width <= 0 or height <= 0:
        raise ValueError("empty texture")
    levels = []
    cur = [bytearray(r) for r in rgba_rows]
    w, h = width, height
    for _ in range(max(1, mip_levels)):
        levels.append((w, h, cur))
        if w == 1 and h == 1:
            break
        nw, nh = max(1, w // 2), max(1, h // 2)
        nxt = []
        for y in range(nh):
            r0 = cur[min(2 * y, h - 1)]
            r1 = cur[min(2 * y + 1, h - 1)]
            row = bytearray(nw * 4)
            for x in range(nw):
                x0, x1 = min(2 * x, w - 1) * 4, min(2 * x + 1, w - 1) * 4
                for c in range(4):
                    row[4 * x + c] = (r0[x0 + c] + r0[x1 + c] + r1[x0 + c] + r1[x1 + c] + 2) >> 2
            nxt.append(row)
        cur, w, h = nxt, nw, nh
    # Stock header shape: DDSD_CAPS|HEIGHT|WIDTH|PIXELFORMAT|MIPMAPCOUNT|LINEARSIZE, linear size
    # = level-0 bytes, DDSCAPS_TEXTURE|MIPMAP|COMPLEX (0x401008) when there is a mip chain.
    DDSD = 0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | 0x80000
    caps1 = 0x1000 | (0x400008 if len(levels) > 1 else 0)
    hdr = struct.pack("<4sIIIIIII", b"DDS ", 124, DDSD, height, width, width * height * 4, 0, len(levels))
    hdr += b"\0" * 44
    hdr += struct.pack("<IIIIIIII", 32, 0x41, 0, 32, 0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000)
    hdr += struct.pack("<IIIII", caps1, 0, 0, 0, 0)
    assert len(hdr) == 128
    out = bytearray(hdr)
    for w, h, rows in levels:
        for row in rows:
            px = bytearray(row)
            px[0::4], px[2::4] = row[2::4], row[0::4]  # RGBA -> BGRA
            out += px
    return bytes(out)


def parse_dds_header(data):
    """Minimal DDS header decode: dict(width, height, mips, fourcc|None, bpp, masks)."""
    if data[:4] != b"DDS ":
        raise ValueError("not a DDS")
    _, flags, h, w, pitch, depth, mips = struct.unpack_from("<7I", data, 4)
    pf_flags, fourcc, bpp, rm, gm, bm, am = struct.unpack_from("<7I", data, 0x50)
    return dict(width=w, height=h, mips=mips, fourcc=(struct.pack("<I", fourcc).decode("ascii", "replace") if pf_flags & 4 else None),
                bpp=bpp, masks=(rm, gm, bm, am), flags=flags)


def model_to_spec(model):
    """Rebuild a write_model spec from a parsed model (round-trip helper)."""
    d = model["data"]
    H = model["header"]
    r = Reader(d)
    meshes = []
    for me in model["meshes"]:
        vb, ib = me["vertex_buffers"][0], me["index_buffers"][0]
        meshes.append(dict(
            flags=me["flags"], flags2=me["flags2"], primitive_raw=me["primitive_raw"], material=me["material"],
            node=me["node"], texture_slots=list(me["texture_slots"]), texture_slot_count=me["texture_slot_count"],
            bounding_sphere=list(me["bounding_sphere"]),
            elements=[(e["stream"], e["offset"], e["type_raw"], e["usage_raw"]) for e in vb["elements"]],
            stride=vb["stride"], vertex_count=vb["count"],
            vertex_data=d[vb["data_offset"]:vb["data_offset"] + vb["count"] * vb["stride"]],
            index_count=ib["count"], index_data=d[ib["data_offset"]:ib["data_offset"] + 2 * ib["count"]],
            palette_count=len(me["palette"]), palette=list(me["palette"]),
        ))
    npal = max((len(me["palette"]) for me in model["meshes"]), default=0)
    palette = [r.u16(H["palette_off"] + 2 * j) for j in range(npal)]
    return dict(
        bones=[dict(identity=b["identity"], bind=list(b["bind"]), inverse_bind=list(b["inverse_bind"]),
                    aabb_min=list(b["aabb_min"]), aabb_max=list(b["aabb_max"]), parent=b["parent"], flags=b["flags"])
               for b in model["bones"]],
        palette=palette,
        meshes=meshes,
        nodes=[dict(f0=n["f0"], id=n["id"], f8=n["f8"], parent=n["parent"], fe=n["fe"],
                    bbox_max=list(n["bbox_max"]), bbox_min=list(n["bbox_min"])) for n in model["nodes"]],
        info=dict(bbox_max=list(model["info"]["bbox_max"]), bbox_min=list(model["info"]["bbox_min"])),
        materials=[dict(identity=m["identity"], shader_hash=m["shader_hash"], params=[list(p) for p in m["params"]])
                   for m in model["materials"]],
        texnames=[t["words"] for t in model["texnames"]],
        textures=[dict(identity=t["identity"], kind=t["kind"], texname_index=t["texname_index"],
                       f10=t["f10"], f14=t["f14"], sampler_bytes=t["sampler_bytes"][:5]) for t in model["textures"]],
        debug=dict(texture_names=list(model["debug"]["texture_names"]), shader_names=list(model["debug"]["shader_names"])),
    )


# ---------------------------------------------------------------------------
# B2IT (.b2it / .grp2it) — doc section 4
# ---------------------------------------------------------------------------
def parse_b2it(data: bytes):
    """Returns list of (name, target_index) in file (sorted) order."""
    if data[:4] != b"B2IT":
        raise ValueError("not a B2IT file")
    count, names_off, idx_off = struct.unpack_from("<III", data, 0x10)
    offs = struct.unpack_from("<%dI" % count, data, names_off)
    idx = struct.unpack_from("<%dI" % count, data, idx_off)
    return [(data[o:data.index(b"\0", o)].decode("ascii", "replace"), i) for o, i in zip(offs, idx)]


def write_b2it(entries):
    """entries = iterable of (name, target_index). Names are sorted ordinally (as the
    game's binary search expects); layout matches the stock files byte-for-byte."""
    entries = sorted(entries, key=lambda e: e[0].encode("ascii"))
    n = len(entries)
    out = bytearray(0x20)
    names_off = len(out)
    out += bytearray(4 * n)
    for i, (name, _) in enumerate(entries):
        struct.pack_into("<I", out, names_off + 4 * i, len(out))
        out += name.encode("ascii") + b"\0"
    _pad(out, 4)
    idx_off = len(out)
    out += struct.pack("<%dI" % n, *(i for _, i in entries))
    _pad(out, 16)
    struct.pack_into("<4sIQIIII", out, 0, b"B2IT", len(out), 0, n, names_off, idx_off, 0)
    return bytes(out)


# ---------------------------------------------------------------------------
# Vertex decoding
# ---------------------------------------------------------------------------
def read_vertices(model, mesh):
    """Yield dicts with decoded attributes for every vertex of mesh.vertex_buffers[0].
    BLENDINDICES are palette-local; use mesh['palette'][i] for the global bone."""
    r = Reader(model["data"])
    vb = mesh["vertex_buffers"][0]
    out = []
    for v in range(vb["count"]):
        base = vb["data_offset"] + v * vb["stride"]
        vert = {}
        for e in vb["elements"]:
            o = base + e["offset"]
            t, u = e["type"], e["usage"]
            if t == "FLOAT3":
                val = r.f32s(o, 3)
            elif t == "FLOAT2":
                val = r.f32s(o, 2)
            elif t == "FLOAT4":
                val = r.f32s(o, 4)
            elif t == "FLOAT16_2":
                val = [half_to_float(r.u16(o)), half_to_float(r.u16(o + 2))]
            elif t == "FLOAT16_4":
                val = [half_to_float(r.u16(o + 2 * k)) for k in range(4)]
            elif t == "UBYTE4":
                val = list(model["data"][o:o + 4])
            elif t == "D3DCOLOR":
                b, g, rr, a = model["data"][o:o + 4]
                val = [rr, g, b, a]  # D3DCOLOR is BGRA in memory
            else:
                val = model["data"][o:o + 4].hex()
            vert[u] = val
        if "BLENDWEIGHT" in vert and len(vert["BLENDWEIGHT"]) == 3:
            w1, w2, w3 = vert["BLENDWEIGHT"]
            vert["WEIGHTS4"] = [1.0 - w1 - w2 - w3, w1, w2, w3]
        out.append(vert)
    return out


def read_indices(model, mesh):
    r = Reader(model["data"])
    ib = mesh["index_buffers"][0]
    return [r.u16(ib["data_offset"] + 2 * i) for i in range(ib["count"])]


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def _fmt_mat(m):
    return " / ".join("[%s]" % " ".join("%7.3f" % x for x in m[i * 4:i * 4 + 4]) for i in range(4))


def dump(path, n_verts=0, as_json=False):
    model: dict = parse_model(open(path, "rb").read())
    if as_json:
        out = {k: v for k, v in model.items() if k != "data"}
        print(json.dumps(out, indent=1, default=str))
        return
    H: dict = model["header"]
    print("== %s  (%d bytes, v%d.%d)" % (os.path.basename(path), len(model["data"]), *H["version"]))
    print("info bbox max %s min %s" % (model["info"]["bbox_max"][:3], model["info"]["bbox_min"][:3]))
    print("bones: %d" % len(model["bones"]))
    for b in model["bones"]:
        print("  [%2d] %-12s parent=%3d flags=%04x bind.t=(%7.3f %7.3f %7.3f) aabb=%s..%s" % (
            b["index"], b["name_folded"], b["parent"], b["flags"], *b["bind"][12:15],
            ["%.3f" % x for x in b["aabb_min"]], ["%.3f" % x for x in b["aabb_max"]]))
    print("materials: %d" % len(model["materials"]))
    for m in model["materials"]:
        print("  [%d] %-12s shader=%08x %s params=%s" % (
            m["index"], m["name_folded"], m["shader_hash"],
            m.get("shader_debug_name") or m["shader"] or "?", m["params"]))
    print("textures: %d  texnames=%s  debug_texnames=%s" % (
        len(model["textures"]), [t["name"] for t in model["texnames"]], model["debug"]["texture_names"]))
    for t in model["textures"]:
        print("  [%d] node=%-12s kind=%d -> %s  sampler=%s scale=(%g,%g)" % (
            t["index"], t["node_name_folded"], t["kind"], t["name"], t["sampler_bytes"], t["f10"], t["f14"]))
    print("nodes: %s" % [(hex(n["id"]), n["parent"]) for n in model["nodes"]])
    print("meshes: %d" % len(model["meshes"]))
    for m in model["meshes"]:
        vb = m["vertex_buffers"][0]
        ib = m["index_buffers"][0]
        print("  [%d] flags=%04x/%02x prim=%s material=%d node=%d tex=%s npal=%d sphere=%s" % (
            m["index"], m["flags"], m["flags2"], m["primitive"], m["material"], m["node"],
            [hex(x) for x in m["texture_slots"][:m["texture_slot_count"]]], len(m["palette"]),
            ["%.3f" % x for x in m["bounding_sphere"]]))
        rs = m["render"]
        print("      render: %s%s%s%s%s blend=%s alpha_ref=0x%02x" % (
            "two-sided " if rs["two_sided"] else "cull-back ", "ztest " if rs["ztest"] else "NO-ztest ",
            "zwrite " if rs["zwrite"] else "NO-zwrite ", "TRANS-pass " if rs["transparent_pass"] else "OPACITY-pass ",
            "alphatest " if rs["alpha_test"] else "no-alphatest ", rs["blend"], rs["alpha_ref"]))
        print("      palette=%s" % m["palette"])
        print("      VB: %d verts stride %d @0x%x  %s" % (
            vb["count"], vb["stride"], vb["data_offset"],
            ["%s:%s@%d" % (e["usage"], e["type"], e["offset"]) for e in vb["elements"]]))
        print("      IB: %d indices (%d tris) @0x%x" % (ib["count"], ib["count"] // 3, ib["data_offset"]))
        if n_verts:
            for v in read_vertices(model, m)[:n_verts]:
                print("        ", {k: (["%.3f" % x for x in val] if isinstance(val, list) and val and isinstance(val[0], float) else val) for k, val in v.items()})


def survey(root):
    import collections
    C = collections.defaultdict(collections.Counter)
    files = sorted(glob.glob(os.path.join(root, "**", "*.model"), recursive=True))
    for f in files:
        try:
            m: dict = parse_model(open(f, "rb").read())
        except Exception as e:  # noqa: BLE001
            C["errors"][str(e)] += 1
            continue
        H: dict = m["header"]
        C["version"][H["version"]] += 1
        C["const98"][hex(H["const98"])] += 1
        C["file_size==len"][H["file_size"] == len(m["data"])] += 1
        C["blob covers file"][H["vdata_off"] + H["blob_size"] + 16 == len(m["data"])] += 1
        for mat in m["materials"]:
            C["shader"][mat.get("shader_debug_name") or mat["shader"] or hex(mat["shader_hash"])] += 1
            C["material.pad18==0"][mat["pad18"] == 0] += 1
            C["material.params"][tuple(tuple(round(x, 3) for x in p) for p in mat["params"])] += 1
        for mesh in m["meshes"]:
            C["mesh.flags"][hex(mesh["flags"])] += 1
            C["mesh.render"][(mesh["render"]["blend"], mesh["render"]["transparent_pass"], mesh["render"]["two_sided"], mesh["render"]["zwrite"])] += 1
            C["mesh.node"][mesh["node"]] += 1
            C["layout"][tuple((e["usage"], e["type"], e["offset"]) for e in mesh["vertex_buffers"][0]["elements"])] += 1
            C["npal<=52"][len(mesh["palette"]) <= 52] += 1
        for b in m["bones"]:
            C["bone.flags"][(b["parent"] == -1, b["flags"])] += 1
    print("files:", len(files))
    for k, c in C.items():
        print(k, dict(c.most_common(12)))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("path", help=".model file, or a directory with --survey")
    ap.add_argument("--verts", type=int, default=0, help="print the first N decoded vertices per mesh")
    ap.add_argument("--json", action="store_true", help="dump the parsed structure as JSON")
    ap.add_argument("--survey", action="store_true", help="statistics over every *.model under <path>")
    a = ap.parse_args()
    if a.survey:
        survey(a.path)
    else:
        dump(a.path, a.verts, a.json)


if __name__ == "__main__":
    main()
