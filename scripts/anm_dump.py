#!/usr/bin/env python3
"""ANM-family parser / dumper for DDR (A3 / World) animations.

Handles .anm (skeletal), .camanm (camera), .sanm (material colour), .tanm
(material UV) — all share the 0xFF010001 chunked container documented in
docs/3d_model_format_research.md (sections 5-7). Includes reference decoders
for every track kind the game ships (48-bit smallest-three quaternions, half
floats, 64-bit axis-angle) and a pose reconstruction that mirrors the game's
evaluator (local TRS, Maya segment-scale compensation).

Usage:
    anm_dump.py <file.anm|camanm|sanm|tanm>            # structure + first values
    anm_dump.py <file.anm> --pose <frame> [--model X]   # decoded local TRS per bone (world t if --model)
    anm_dump.py --survey <dir>                          # statistics over every animation file

Import-safe: `from anm_dump import parse_anm, decode_track, evaluate_pose`.
"""
import argparse
import glob
import math
import os
import struct
import sys

MAGIC = 0xFF010001
CHUNK_BASE = 0xFF010002

CHUNK_NAMES = {
    0: "bone_tracks", 1: "hierarchy", 2: "type2", 4: "camera", 5: "type5", 6: "material_names",
    9: "type9_tracks", 10: "material_uv_tracks", 11: "lights", 14: "material_color_tracks",
    15: "material_targets",
}
TRACK_LIST_TYPES = {0, 9, 10, 11, 14}

# kind -> (channel, bytes per key)   (doc 5.1)
KINDS = {
    1: ("rotation", 16), 2: ("rotation", 32), 4: ("translation", 16), 7: ("translation", 32),
    8: ("scalar", 4), 9: ("scalar", 8), 10: ("scale", 16), 11: ("scale", 32),
    0x16: ("rotation", 8), 0x17: ("rotation", 16), 0x19: ("translation", 6),
    0x1A: ("rotation", 8), 0x1B: ("scalar", 4), 0x1C: ("rotation", 6), 0x1D: ("translation", 12),
    0x1E: ("translation", 6), 0x1F: ("translation", 6), 0x20: ("visibility", 0), 0x22: ("scalar", 8),
}
# Kinds decoded by STEP (no interpolation; the game picks key i while u < 1, else key i+1):
# 0x1B (float) and 0x20 (1 bit per key: set -> 1 "visible", clear -> 2 "hidden"; drives the
# chunk-9 / record-type-6 node-visibility channel).
STEP_KINDS = {0x1B, 0x20}

Q15_OFFSET = 16383.5
Q15_SCALE = 23169.767578125  # = 16383.5 * sqrt(2)
Q20_MAX = float((1 << 20) - 1)


def decode6(v):
    out = ""
    for sh in range(54, -1, -6):
        c = (v >> sh) & 0x3F
        if c == 0:
            continue
        out += chr(c + 0x60) if c < 0x1C else chr(c + 0x14) if c < 0x26 else "?"
    return out


def half_to_float(h):
    s = (h >> 15) & 1
    e = (h >> 10) & 0x1F
    m = h & 0x3FF
    if e == 0:
        v = (m / 1024.0) * 2.0 ** -14
    elif e == 31:
        v = float("inf")
    else:
        v = (1.0 + m / 1024.0) * 2.0 ** (e - 15)
    return -v if s else v


def float_to_half(f):
    return struct.unpack("<H", struct.pack("<e", f))[0]


# ---------------------------------------------------------------------------
# Rotation codecs (doc 5.2). Quaternions are (x, y, z, w).
# ---------------------------------------------------------------------------
def decode_q48(b6):
    v = int.from_bytes(b6, "little")
    a = (v >> 32) & 0x7FFF
    b = (v >> 17) & 0x7FFF
    c = (v >> 2) & 0x7FFF
    m = v & 3
    A, B, C = ((x - Q15_OFFSET) / Q15_SCALE for x in (a, b, c))
    D = math.sqrt(max(0.0, 1.0 - (A * A + B * B + C * C)))
    return [(D, A, B, C), (A, D, B, C), (A, B, D, C), (A, B, C, D)][m]


def encode_q48(q):
    x, y, z, w = q
    n = math.sqrt(x * x + y * y + z * z + w * w) or 1.0
    q = [x / n, y / n, z / n, w / n]
    m = max(range(4), key=lambda i: abs(q[i]))
    if q[m] < 0:
        q = [-c for c in q]
    rest = [q[i] for i in range(4) if i != m]
    a, b, c = (max(0, min(0x7FFF, int(round(c * Q15_SCALE + Q15_OFFSET)))) for c in rest)
    v = (a << 32) | (b << 17) | (c << 2) | m
    return v.to_bytes(6, "little")


def decode_q64_axis_angle(v):
    angle = (v & 0xFFFFF) * math.pi / Q20_MAX
    ax = ((v >> 20) & 0xFFFFF) / Q20_MAX
    ay = ((v >> 40) & 0xFFFFF) / Q20_MAX
    az = 1.0 - ax - ay
    fl = v >> 60
    if fl & 1:
        ax = -ax
    if fl & 2:
        ay = -ay
    if fl & 4:
        az = -az
    n = math.sqrt(ax * ax + ay * ay + az * az)
    if n:
        ax, ay, az = ax / n, ay / n, az / n
    s, c = math.sin(angle * 0.5), math.cos(angle * 0.5)
    return (ax * s, ay * s, az * s, c)


# ---------------------------------------------------------------------------
# Container
# ---------------------------------------------------------------------------
class R:
    def __init__(self, d):
        self.d = d

    def u8(self, o): return self.d[o]
    def u16(self, o): return struct.unpack_from("<H", self.d, o)[0]
    def u32(self, o): return struct.unpack_from("<I", self.d, o)[0]
    def u64(self, o): return struct.unpack_from("<Q", self.d, o)[0]
    def f32(self, o): return struct.unpack_from("<f", self.d, o)[0]


def _parse_track(r, o):
    kind = r.u16(o)
    T: dict = dict(offset=o, kind=kind, channel=KINDS.get(kind, ("?", 0))[0], key_bytes=KINDS.get(kind, ("?", 0))[1],
             f2=r.u16(o + 2), key_count=r.u16(o + 4), target=r.u8(o + 6), sub=r.u8(o + 7),
             times_off=r.u32(o + 8), values_off=r.u32(o + 0xC))
    T["values"] = o + T["values_off"]
    T["times"] = [r.u16(o + T["times_off"] + 2 * i) for i in range(T["key_count"])] if T["times_off"] else None
    return T


def parse_anm(data: bytes) -> dict:
    r = R(data)
    if r.u32(0) != MAGIC:
        raise ValueError("not an ANM-family file (magic %08x)" % r.u32(0))
    hdr: dict = dict(frame_count=r.u16(4), flag=r.u16(6), fps_or_one=r.u32(8), d=r.u32(0xC))
    offs = []
    o = 0x10
    while r.u32(o):
        offs.append(r.u32(o))
        o += 4
    chunks = []
    for co in offs:
        typ = r.u32(co) - CHUNK_BASE
        C: dict = dict(offset=co, type=typ, name=CHUNK_NAMES.get(typ, "type%d" % typ), h4=r.u16(co + 4), h6=r.u16(co + 6))
        if typ in TRACK_LIST_TYPES:
            tr = []
            p = co + 8
            while r.u32(p):
                tr.append(_parse_track(r, co + r.u32(p)))
                p += 4
            C["tracks"] = tr
        elif typ == 1:
            n = C["h4"]
            pairs_off = co + r.u32(co + 8)
            C["pairs"] = [(data[pairs_off + 2 * i], data[pairs_off + 2 * i + 1]) for i in range(n)]
            C["trailer"] = r.u16(co + r.u32(co + 0xC))
        elif typ == 4:
            slots = [r.u32(co + 8 + 4 * i) for i in range(6)]
            C["tracks"] = [_parse_track(r, co + s) if s else None for s in slots]
        elif typ == 6:
            C["names"] = [decode6(r.u64(co + 8 + 8 * i)) for i in range(C["h4"])]
            C["identities"] = [r.u64(co + 8 + 8 * i) for i in range(C["h4"])]
        elif typ == 11:
            C["names"] = [decode6(r.u64(co + 8 + 8 * i)) for i in range(C["h4"])]
        elif typ == 15:
            C["entries"] = [dict(identity=r.u64(co + 8 + 32 * i), name=decode6(r.u64(co + 8 + 32 * i)),
                                 identity2=r.u64(co + 16 + 32 * i), u32=r.u32(co + 24 + 32 * i))
                            for i in range(C["h4"])]
        chunks.append(C)
    return dict(header=hdr, chunks=chunks, data=data)


# ---------------------------------------------------------------------------
# Track decoding
# ---------------------------------------------------------------------------
def decode_key(data, T, i):
    """Decode key i of a track to a tuple (quat xyzw / vec3 / scalar). Tangent-carrying
    kinds return only the value part."""
    r = R(data)
    k = T["kind"]
    v = T["values"]
    if k == 1:
        return tuple(struct.unpack_from("<4f", data, v + 16 * i))
    if k == 2:
        return tuple(struct.unpack_from("<4f", data, v + 32 * i))
    if k in (4, 10):
        return tuple(struct.unpack_from("<3f", data, v + 16 * i))
    if k in (7, 11):
        return tuple(struct.unpack_from("<3f", data, v + 32 * i))
    if k in (8, 0x1B):
        return (r.f32(v + 4 * i),)
    if k in (9, 0x22):
        return (r.f32(v + 8 * i),)
    if k in (0x16, 0x1A):
        return decode_q64_axis_angle(r.u64(v + 8 * i)) if k == 0x16 else decode_q48(data[v + 8 * i:v + 8 * i + 6])
    if k == 0x17:
        return decode_q64_axis_angle(r.u64(v + 16 * i))
    if k == 0x1C:
        return decode_q48(data[v + 6 * i:v + 6 * i + 6])
    if k == 0x1D:
        return tuple(struct.unpack_from("<3f", data, v + 12 * i))
    if k in (0x19, 0x1E):
        return tuple(half_to_float(r.u16(v + 6 * i + 2 * c)) for c in range(3))
    if k == 0x1F:
        base = struct.unpack_from("<3f", data, v)
        return tuple(base[c] + half_to_float(r.u16(v + 12 + 6 * i + 2 * c)) for c in range(3))
    if k == 0x20:
        return (1 if (data[v + (i >> 3)] >> (i & 7)) & 1 else 2,)
    raise ValueError("unknown track kind 0x%x" % k)


def decode_track(data, T):
    return [decode_key(data, T, i) for i in range(T["key_count"])]


# ---------------------------------------------------------------------------
# Writer (doc 5 / 5.3 / 6).  Produces the game's own layout: header, absolute chunk
# offset list, chunks; 16-byte tracks followed by their (optional) u16 times and
# their 16-byte-aligned values; the next track starts 16-byte aligned.
# ---------------------------------------------------------------------------
def _align(n, a=16):
    return (n + a - 1) & ~(a - 1)


def encode_key(kind, key, base=None):
    """Inverse of decode_key for the kinds an exporter needs (1, 4, 8, 10, 0x1C, 0x1D,
    0x1E, 0x1F). Returns the bytes of ONE key (0x1F: the 6-byte half delta vs base)."""
    if kind == 1:
        return struct.pack("<4f", *key)
    if kind in (4, 10):
        return struct.pack("<4f", key[0], key[1], key[2], 1.0)  # stock pads with 1.0
    if kind in (8, 0x1B):
        return struct.pack("<f", key[0])
    if kind == 0x1C:
        return encode_q48(key)
    if kind == 0x1D:
        return struct.pack("<3f", *key)
    if kind == 0x1E:
        return struct.pack("<3H", *(float_to_half(c) for c in key))
    if kind == 0x1F:
        if base is None:
            raise ValueError("kind 0x1F needs a base")
        return struct.pack("<3H", *(float_to_half(c - b) for c, b in zip(key, base)))
    raise NotImplementedError("no encoder for track kind 0x%x" % kind)


def _encode_track(T):
    """T = dict(kind, target, sub=0, times=None|[u16], keys=[tuple], base=None (0x1F)).
    Returns (header16 without offsets, times_bytes, values_bytes)."""
    kind, keys = T["kind"], T["keys"]
    if kind == 0x1F:
        base = T.get("base") or keys[0]
        values = struct.pack("<3f", *base) + b"".join(encode_key(kind, k, base) for k in keys)
    else:
        values = b"".join(encode_key(kind, k) for k in keys)
    times = T.get("times")
    if times is not None:
        if len(times) != len(keys):
            raise ValueError("times/keys length mismatch")
        times_b = struct.pack("<%dH" % len(times), *times)
    else:
        times_b = b""
    return times_b, values


def _layout_tracks(tracks, base_off):
    """Place tracks starting at base_off. Returns (blob, [track_abs_offsets])."""
    out = bytearray()
    offsets = []
    for T in tracks:
        times_b, values_b = _encode_track(T)
        start = base_off + len(out)
        offsets.append(start)
        times_rel = 16 if times_b else 0
        values_rel = _align(16 + len(times_b))  # values always start 16-byte aligned
        hdr = struct.pack("<HHHBBII", T["kind"], T.get("tag", 3 if not times_b else 0), len(T["keys"]),
                          T["target"], T.get("sub", 0), times_rel, values_rel)
        body = bytearray(hdr) + times_b
        body += b"\0" * (values_rel - len(body))
        body += values_b
        body += b"\0" * (_align(len(body)) - len(body))
        out += body
    return bytes(out), offsets


def write_anm(spec):
    """Build an ANM-family file.

    spec = dict(
        frame_count=int,                  # last frame index (keys = frame_count+1 when uniform)
        fps=None|int,                     # camanm/sanm: header+8; None -> skeletal header (1, 0)
        flag=int,                         # header+6 (unread by the game; stock anm 1, camanm 0)
        hierarchy=[parent_or_-1, ...],    # optional -> type-1 chunk (skeletal files)
        tracks=[track, ...],              # optional -> type-0 chunk (bone tracks, target = bone index)
        camera=[track|None]*6,            # optional -> type-4 chunk (slots 0..5)
    )
    track = dict(kind, target, sub=0, times=None|[frame,...], keys=[tuple,...], base=None)
    """
    chunks = []  # list of (builder(abs_off) -> bytes)
    if spec.get("hierarchy") is not None:
        parents = spec["hierarchy"]

        def build_h(off, parents=parents):
            pairs = b"".join(struct.pack("<BB", i, 0xFF if p < 0 else p) for i, p in enumerate(parents))
            body = struct.pack("<IHHII", CHUNK_BASE + 1, len(parents), 0, 0x10, 0x10 + len(pairs)) + pairs
            body += struct.pack("<H", spec.get("hierarchy_trailer", 0))
            return body + b"\0" * (_align(len(body), 4) - len(body))
        chunks.append(build_h)
    if spec.get("tracks") is not None:
        tracks = spec["tracks"]

        def build_t(off, tracks=tracks):
            list_len = 8 + 4 * (len(tracks) + 1)
            blob, offs = _layout_tracks(tracks, off + list_len)
            head = struct.pack("<IHH", CHUNK_BASE + 0, 0, 0) + struct.pack("<%dI" % len(offs), *(o - off for o in offs)) + b"\0\0\0\0"
            return head + blob
        chunks.append(build_t)
    if spec.get("camera") is not None:
        slots = spec["camera"]
        if len(slots) != 6:
            raise ValueError("camera needs 6 slots")

        def build_c(off, slots=slots):
            present = [t for t in slots if t is not None]
            blob, offs = _layout_tracks(present, off + 8 + 24)
            it = iter(offs)
            rel = [(next(it) - off) if t is not None else 0 for t in slots]
            return struct.pack("<IHH", CHUNK_BASE + 4, 0, 0) + struct.pack("<6I", *rel) + blob
        chunks.append(build_c)
    if not chunks:
        raise ValueError("nothing to write")

    fps = spec.get("fps")
    if fps is None:
        header = struct.pack("<IHHII", MAGIC, spec["frame_count"], spec.get("flag", 1), 1, 0)
    else:
        header = struct.pack("<IHHII", MAGIC, spec["frame_count"], spec.get("flag", 0), int(fps), 1)
    offs_len = 4 * (len(chunks) + 1)
    out = bytearray(header) + bytearray(offs_len)
    abs_offsets = []
    for build in chunks:
        off = len(out)
        abs_offsets.append(off)
        out += build(off)
    out += b"\0" * (_align(len(out)) - len(out))
    struct.pack_into("<%dI" % len(abs_offsets), out, 0x10, *abs_offsets)
    return bytes(out)


def anm_to_spec(parsed):
    """Rebuild a write_anm spec from a parsed file (decoded keys, kinds/times kept)."""
    data = parsed["data"]
    hdr = parsed["header"]
    has_camera = any(c["type"] == 4 for c in parsed["chunks"])
    spec = dict(frame_count=hdr["frame_count"], flag=hdr["flag"], fps=hdr["fps_or_one"] if has_camera else None)

    def conv(T):
        t: dict = dict(kind=T["kind"], target=T["target"], sub=T["sub"], tag=T["f2"], times=T["times"],
                       keys=decode_track(data, T))
        if T["kind"] == 0x1F:
            t["base"] = tuple(struct.unpack_from("<3f", data, T["values"]))
        return t
    for c in parsed["chunks"]:
        if c["type"] == 1:
            spec["hierarchy"] = [(-1 if p == 0xFF else p) for _, p in c["pairs"]]
            spec["hierarchy_trailer"] = c["trailer"]
        elif c["type"] == 0:
            spec["tracks"] = [conv(T) for T in c["tracks"]]
        elif c["type"] == 4:
            spec["camera"] = [conv(T) if T else None for T in c["tracks"]]
    return spec


def _slerp(a, b, t):
    dot = sum(x * y for x, y in zip(a, b))
    if dot < 0:
        b = tuple(-x for x in b)
        dot = -dot
    if 1.0 - dot <= 1e-5:
        return tuple((1 - t) * x + t * y for x, y in zip(a, b))
    th = math.acos(min(1.0, dot))
    s = math.sin(th)
    return tuple((math.sin((1 - t) * th) / s) * x + (math.sin(t * th) / s) * y for x, y in zip(a, b))


def sample_track(data, T, frame):
    """Game-equivalent sampling at a fractional frame (uniform or explicit times)."""
    n = T["key_count"]
    if T["times"] is None:
        i = int(frame)
        if n == 1 or i >= n - 1:
            return decode_key(data, T, n - 1)
        u = frame - i
        i1 = i + 1
    else:
        times = T["times"]
        fi = int(frame)
        if fi >= times[-1]:
            return decode_key(data, T, n - 1)
        i = max(k for k in range(n) if times[k] <= fi)
        i1 = i + 1
        while i1 < n - 1 and times[i1] == times[i]:
            i1 += 1
        if i1 >= n:
            return decode_key(data, T, i)
        u = (frame - times[i]) / float(times[i1] - times[i])
    a, b = decode_key(data, T, i), decode_key(data, T, i1)
    if T["kind"] in STEP_KINDS:
        return a if u < 1.0 else b
    if T["channel"] == "rotation":
        return _slerp(a, b, u)
    return tuple((1 - u) * x + u * y for x, y in zip(a, b))


# ---------------------------------------------------------------------------
# Pose reconstruction (doc 5.1): local TRS, row-vector matrices, parent-scale compensation
# ---------------------------------------------------------------------------
def quat_to_rowmat(q):
    x, y, z, w = q
    return [[1 - 2 * (y * y + z * z), 2 * (x * y + z * w), 2 * (x * z - y * w)],
            [2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w)],
            [2 * (x * z + y * w), 2 * (y * z - x * w), 1 - 2 * (x * x + y * y)]]


def _mat4(rot3, scale, t):
    m = [[0.0] * 4 for _ in range(4)]
    for i in range(3):
        for j in range(3):
            m[i][j] = rot3[i][j] * scale[i]
    m[3][0], m[3][1], m[3][2], m[3][3] = t[0], t[1], t[2], 1.0
    return m


def _mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def evaluate_pose(anm, frame, parents):
    """Return per-bone dict(q, t, s, world) for the bone_tracks chunk at `frame`.
    `parents` = list of parent indices (from the hierarchy chunk or the model)."""
    data = anm["data"]
    n = len(parents)
    pose: list = [dict(q=(0, 0, 0, 1), t=(0, 0, 0), s=(1, 1, 1)) for _ in range(n)]
    for C in anm["chunks"]:
        if C["type"] != 0:
            continue
        for T in C["tracks"]:
            b = T["target"]
            if b >= n:
                continue
            val = sample_track(data, T, frame)
            pose[b][{"rotation": "q", "translation": "t", "scale": "s"}[T["channel"]]] = val
    world: list = [None] * n
    for i in range(n):
        p = pose[i]
        local = _mat4(quat_to_rowmat(p["q"]), p["s"], p["t"])
        par = parents[i]
        if par < 0 or par >= n:
            world[i] = local
        else:
            ps = pose[par]["s"]
            for r_ in range(3):
                for c in range(3):
                    local[r_][c] /= ps[c] if ps[c] else 1.0
            world[i] = _mul(local, world[par])
        p["world"] = world[i]
    return pose


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def dump(path, pose_frame=None, model_path=None):
    anm: dict = parse_anm(open(path, "rb").read())
    d = anm["data"]
    H = anm["header"]
    print("== %s  (%d bytes) frames=%d flag=%d fps/one=%d d=%d" % (
        os.path.basename(path), len(d), H["frame_count"], H["flag"], H["fps_or_one"], H["d"]))
    for C in anm["chunks"]:
        print(" chunk @0x%x type %d (%s) h4=%d h6=%d" % (C["offset"], C["type"], C["name"], C["h4"], C["h6"]))
        if "pairs" in C:
            print("   hierarchy (index,parent):", C["pairs"], "trailer=%d" % C["trailer"])
        if "names" in C:
            print("   names:", C["names"])
        if "entries" in C:
            print("   entries:", [(e["name"], hex(e["u32"])) for e in C["entries"]])
        for T in C.get("tracks") or []:
            if T is None:
                print("   (empty slot)")
                continue
            first = decode_key(d, T, 0)
            print("   track @0x%x kind=0x%02x %-11s target=%d sub=%d f2=%d keys=%d %s first=%s" % (
                T["offset"], T["kind"], T["channel"], T["target"], T["sub"], T["f2"], T["key_count"],
                "uniform" if T["times"] is None else "times[..%d]" % T["times"][-1],
                tuple(round(x, 4) for x in first)))
    if pose_frame is not None:
        hier = next((c for c in anm["chunks"] if c["type"] == 1), None)
        if hier is None:
            print("no hierarchy chunk; --pose needs a skeletal .anm")
            return
        parents = [(-1 if p == 0xFF else p) for _, p in hier["pairs"]]
        names = None
        bind_t = None
        if model_path:
            sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
            from ktmdl_dump import parse_model  # noqa: E402
            m = parse_model(open(model_path, "rb").read())
            names = [b["name_folded"] for b in m["bones"]]
            bind_t = [b["bind"][12:15] for b in m["bones"]]
        pose: list = evaluate_pose(anm, pose_frame, parents)
        print("pose at frame %g:" % pose_frame)
        for i, p in enumerate(pose):
            w = p["world"]
            line = "  [%2d] %-12s q=(%6.3f %6.3f %6.3f %6.3f) t=(%6.3f %6.3f %6.3f) s=(%.2f %.2f %.2f) world.t=(%6.3f %6.3f %6.3f)" % (
                i, names[i] if names else "", *p["q"], *p["t"], *p["s"], w[3][0], w[3][1], w[3][2])
            if bind_t:
                line += "  bind.t=(%6.3f %6.3f %6.3f)" % tuple(bind_t[i])
            print(line)


def survey(root):
    import collections
    C = collections.defaultdict(collections.Counter)
    files = sorted(sum((glob.glob(os.path.join(root, "**", "*." + e), recursive=True) for e in ("anm", "camanm", "sanm", "tanm")), []))
    for f in files:
        ext = f.rsplit(".", 1)[1]
        try:
            a = parse_anm(open(f, "rb").read())
        except Exception as e:  # noqa: BLE001
            C["errors"][str(e)] += 1
            continue
        H = a["header"]
        C[ext + ".header"][(H["flag"], H["fps_or_one"], H["d"])] += 1
        C[ext + ".chunks"][tuple(c["type"] for c in a["chunks"])] += 1
        for c in a["chunks"]:
            for T in c.get("tracks") or []:
                if T:
                    C[ext + ".kind"][hex(T["kind"])] += 1
                    C[ext + ".f2"][T["f2"]] += 1
    print("files:", len(files))
    for k in sorted(C):
        print(k, dict(C[k].most_common(10)))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("path")
    ap.add_argument("--pose", type=float, help="reconstruct the skeletal pose at this frame")
    ap.add_argument("--model", help=".model to take bone names / bind translations from")
    ap.add_argument("--survey", action="store_true")
    a = ap.parse_args()
    if a.survey:
        survey(a.path)
    else:
        dump(a.path, a.pose, a.model)


if __name__ == "__main__":
    main()
