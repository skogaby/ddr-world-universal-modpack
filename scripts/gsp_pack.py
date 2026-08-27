#!/usr/bin/env python3
"""GSPW shader container packer/inspector for DDR World.

Wraps D3D9 SM1-3 bytecode blobs (from fxc, vkd3d-compiler, or any conforming
compiler -- the container is compiler-agnostic) into the game's `.gsp` GSPW
container, or inspects/validates an existing `.gsp`. Supports multi-program
containers (N programs sharing VS/PS blobs by table index).

Container layout (validated against all 35 stock files + the engine parser;
full RE record in docs/shader_replacement_research.md section 2):

    0x00  u32   magic 'GSPW'
    0x04  u32   FNV-1 (32-bit) hash of the bare shader name (lookup identity)
    0x08  u32   0
    0x0C  u32   ptr -> program table
    0x10  u32   ptr -> VS table
    0x14  u32   ptr -> PS table
    0x18  u8*3  entry counts (programs, VS, PS) + pad
    0x1C  u32   0
    ...   8*N   program entries {flags u8 (bit0 = no PS), pad*3,
                                 vs_idx u8 @+4, ps_idx u8 @+5, pad*2}
    ...   8*N   VS entries {u32 blob_offset, u32 blob_size}
    ...   8*N   PS entries {u32 blob_offset, u32 blob_size}
    ...   blobs, 16-byte aligned, in table order (VS then PS)

The engine dedupes program entries on {u32@+0, u8@+4, u8@+5}; entry bytes
+6/+7 are never read. A blob shared by multiple programs appears once,
referenced by index.

Usage:
    gsp_pack.py pack --name gs_screencommand_arrow --vs vs.d3dbc --ps ps.d3dbc -o out.gsp
    gsp_pack.py pack --name gs_screencommand_arrow \
        --vs vs0.d3dbc --vs vs1.d3dbc --ps ps0.d3dbc \
        --program 0:0 --program 1:0 -o out.gsp
    gsp_pack.py inspect file.gsp
    gsp_pack.py selftest
"""

import argparse
import struct
import sys

MAGIC = b"GSPW"
HEADER_SIZE = 0x20
PROGRAM_TABLE_OFF = 0x20
BLOB_ALIGN = 16

VS_VERSION_HI = 0xFFFE  # vs_x_y version token high word
PS_VERSION_HI = 0xFFFF  # ps_x_y version token high word


def fnv1_32(data: bytes) -> int:
    """FNV-1 (32-bit) -- the hash the engine computes over shader names."""
    h = 0x811C9DC5
    for b in data:
        h = (h * 0x01000193) & 0xFFFFFFFF
        h ^= b
    return h


def blob_kind(blob: bytes) -> str:
    """Classify a blob by its D3D9 version token. Raises on garbage."""
    if len(blob) < 4:
        raise ValueError("blob too small for a version token")
    tok = struct.unpack_from("<I", blob, 0)[0]
    hi, major, minor = tok >> 16, (tok >> 8) & 0xFF, tok & 0xFF
    if hi == VS_VERSION_HI:
        return f"vs_{major}_{minor}"
    if hi == PS_VERSION_HI:
        return f"ps_{major}_{minor}"
    raise ValueError(f"not D3D9 shader bytecode (version token 0x{tok:08X})")


def align(n: int, a: int) -> int:
    return (n + a - 1) & ~(a - 1)


def sm3_instr_count(blob: bytes) -> tuple[int, int]:
    """Walk an SM1-3 token stream; return (instructions, texld_taps).

    def/defi/defb and dcl tokens are excluded (they occupy no execution
    slots); comment blocks (CTAB etc.) are skipped by their length field.
    Same method as the fxc-vs-vkd3d comparison in the perf research note.
    """
    ntoks = len(blob) // 4
    toks = struct.unpack_from(f"<{ntoks}I", blob, 0)
    i = 1  # skip version token
    total = texld = 0
    while i < ntoks:
        t = toks[i]
        if t == 0x0000FFFF:  # END
            break
        op = t & 0xFFFF
        if op == 0xFFFE:  # comment block: length (dwords) in bits 16..30
            i += 1 + ((t >> 16) & 0x7FFF)
            continue
        length = (t >> 24) & 0x0F  # operand count, SM2+
        if op not in (0x1F, 0x51, 0x52, 0x53):  # dcl, def, defi, defb
            total += 1
            if op == 0x42:  # texld family
                texld += 1
        i += 1 + length
    return total, texld


# -- pack -------------------------------------------------------------------


def pack(
    name: str,
    vs_blobs: list[bytes],
    ps_blobs: list[bytes],
    programs: list[tuple[int, int, int]] | None = None,
) -> bytes:
    """Build a GSPW container.

    programs: list of (flags, vs_idx, ps_idx); defaults to [(0, 0, 0)] —
    the degenerate single-program case, bit-identical to the historical
    single-program output (tables at 0x20/0x28/0x30, first blob at 0x40).
    """
    if programs is None:
        programs = [(0, 0, 0)]
    if not vs_blobs or not ps_blobs or not programs:
        raise ValueError("need at least one VS blob, one PS blob, one program")
    if len(vs_blobs) > 255 or len(ps_blobs) > 255 or len(programs) > 255:
        raise ValueError("counts are u8 fields (max 255)")

    for i, blob in enumerate(vs_blobs):
        k = blob_kind(blob)
        if not k.startswith("vs_"):
            raise ValueError(f"--vs blob {i} is {k}, expected a vertex shader")
    for i, blob in enumerate(ps_blobs):
        k = blob_kind(blob)
        if not k.startswith("ps_"):
            raise ValueError(f"--ps blob {i} is {k}, expected a pixel shader")
    for flags, vsi, psi in programs:
        if not (0 <= flags <= 0xFF):
            raise ValueError(f"program flags 0x{flags:X} out of u8 range")
        if not 0 <= vsi < len(vs_blobs):
            raise ValueError(f"program vs_idx {vsi} out of range")
        if not 0 <= psi < len(ps_blobs):
            raise ValueError(f"program ps_idx {psi} out of range")

    prog_off = PROGRAM_TABLE_OFF
    vs_tab = prog_off + 8 * len(programs)
    ps_tab = vs_tab + 8 * len(vs_blobs)
    first_blob = align(ps_tab + 8 * len(ps_blobs), BLOB_ALIGN)

    # Lay blobs out sequentially, 16-aligned, in table order (VS then PS).
    offsets: list[int] = []
    pos = first_blob
    for blob in vs_blobs + ps_blobs:
        offsets.append(pos)
        pos = align(pos + len(blob), BLOB_ALIGN)
    total = offsets[-1] + len((vs_blobs + ps_blobs)[-1])

    out = bytearray(total)
    out[0:4] = MAGIC
    struct.pack_into("<I", out, 0x04, fnv1_32(name.encode()))
    struct.pack_into("<III", out, 0x0C, prog_off, vs_tab, ps_tab)
    out[0x18:0x1C] = bytes([len(programs), len(vs_blobs), len(ps_blobs), 0])
    for i, (flags, vsi, psi) in enumerate(programs):
        e = prog_off + 8 * i
        out[e] = flags  # +1..+3 stay zero (dedupe key includes the u32 at +0)
        out[e + 4] = vsi
        out[e + 5] = psi  # +6/+7 never read by the parser; stay zero
    for i, blob in enumerate(vs_blobs):
        struct.pack_into("<II", out, vs_tab + 8 * i, offsets[i], len(blob))
        out[offsets[i] : offsets[i] + len(blob)] = blob
    for i, blob in enumerate(ps_blobs):
        j = len(vs_blobs) + i
        struct.pack_into("<II", out, ps_tab + 8 * i, offsets[j], len(blob))
        out[offsets[j] : offsets[j] + len(blob)] = blob
    return bytes(out)


# -- inspect / validate -----------------------------------------------------


def inspect(data: bytes, expect_name: str | None = None) -> dict:
    """Parse + validate a .gsp. Returns a dict of fields; raises on violation."""
    if data[0:4] != MAGIC:
        raise ValueError(f"bad magic {data[0:4]!r}")
    name_hash = struct.unpack_from("<I", data, 0x04)[0]
    if struct.unpack_from("<I", data, 0x08)[0] != 0:
        raise ValueError("field 0x08 not zero")
    ptr_a, ptr_b, ptr_c = struct.unpack_from("<III", data, 0x0C)
    cnt_a, cnt_b, cnt_c = data[0x18], data[0x19], data[0x1A]

    def table(ptr, cnt):
        return [
            struct.unpack_from("<II", data, ptr + 8 * i) for i in range(cnt)
        ]

    # Program entries: {flags u8 @+0, vs_idx u8 @+4, ps_idx u8 @+5}
    # (engine-verified; bytes +6/+7 are never read).
    programs = [
        (data[ptr_a + 8 * i], data[ptr_a + 8 * i + 4], data[ptr_a + 8 * i + 5])
        for i in range(cnt_a)
    ]
    for flags, vsi, psi in programs:
        if vsi >= cnt_b:
            raise ValueError(f"program vs_idx {vsi} >= VS count {cnt_b}")
        if not (flags & 1) and psi >= cnt_c:
            raise ValueError(f"program ps_idx {psi} >= PS count {cnt_c}")
    vs_entries, ps_entries = table(ptr_b, cnt_b), table(ptr_c, cnt_c)

    kinds = {"vs": [], "ps": []}
    instrs = {"vs": [], "ps": []}
    blobs = []
    for label, entries, want in (("vs", vs_entries, "vs_"), ("ps", ps_entries, "ps_")):
        for off, size in entries:
            if size == 0:
                kinds[label].append("-")
                instrs[label].append((0, 0))
                continue
            if off + size > len(data):
                raise ValueError(f"{label} blob @0x{off:X}+0x{size:X} out of bounds")
            if off % BLOB_ALIGN:
                raise ValueError(f"{label} blob @0x{off:X} not {BLOB_ALIGN}-aligned")
            kind = blob_kind(data[off : off + size])
            if not kind.startswith(want):
                raise ValueError(f"{label} table entry holds {kind}")
            kinds[label].append(kind)
            instrs[label].append(sm3_instr_count(data[off : off + size]))
            blobs.append((off, size))

    # Blobs must tile the file after the header (16-aligned; trailing
    # serializer slack tolerated on stock files, forbidden on our output).
    blobs.sort()
    pos = blobs[0][0] if blobs else len(data)
    for off, size in blobs:
        if off not in (pos, align(pos, BLOB_ALIGN)):
            raise ValueError(f"blob @0x{off:X} leaves a gap (expected 0x{pos:X})")
        pos = off + size
    slack = len(data) - pos

    if expect_name is not None:
        want = fnv1_32(expect_name.encode())
        if name_hash != want:
            raise ValueError(
                f"name hash 0x{name_hash:08X} != FNV-1({expect_name!r}) 0x{want:08X}"
            )

    return {
        "name_hash": name_hash,
        "counts": (cnt_a, cnt_b, cnt_c),
        "programs": programs,
        "vs": vs_entries,
        "ps": ps_entries,
        "kinds": kinds,
        "instrs": instrs,
        "slack": slack,
        "size": len(data),
    }


def print_report(info: dict) -> None:
    print(f"size        : {info['size']} bytes (+{info['slack']} trailing slack)")
    print(f"name hash   : 0x{info['name_hash']:08X} (FNV-1)")
    print(f"counts      : programs={info['counts'][0]} vs={info['counts'][1]} ps={info['counts'][2]}")
    for flags, vsi, psi in info["programs"]:
        print(f"program     : flags=0x{flags:02X} vs_idx={vsi} ps_idx={psi}")
    for (off, size), kind, (n, tex) in zip(
        info["vs"], info["kinds"]["vs"], info["instrs"]["vs"]
    ):
        print(f"vs blob     : @0x{off:04X} size=0x{size:X} {kind} ({n} instr)")
    for (off, size), kind, (n, tex) in zip(
        info["ps"], info["kinds"]["ps"], info["instrs"]["ps"]
    ):
        print(f"ps blob     : @0x{off:04X} size=0x{size:X} {kind} ({n} instr, {tex} texld)")


# -- selftest ---------------------------------------------------------------


def selftest() -> int:
    # Minimal valid token streams: version token + END (0x0000FFFF).
    fake_vs = struct.pack("<II", 0xFFFE0300, 0x0000FFFF) + b"\x00" * 24
    fake_ps = struct.pack("<II", 0xFFFF0300, 0x0000FFFF) + b"\x00" * 4

    name = "gs_screencommand_arrow"
    blob = pack(name, [fake_vs], [fake_ps])
    info = inspect(blob, expect_name=name)
    assert info["name_hash"] == 0x9E93AC7B, "FNV-1 regression (known stock hash)"
    assert info["counts"] == (1, 1, 1)
    assert info["kinds"]["vs"] == ["vs_3_0"] and info["kinds"]["ps"] == ["ps_3_0"]
    assert info["slack"] == 0
    # Degenerate single-program geometry must match the historical constants
    # (tables 0x20/0x28/0x30, first blob 0x40) so existing artifacts repack
    # bit-identically.
    assert struct.unpack_from("<III", blob, 0x0C) == (0x20, 0x28, 0x30)
    assert info["vs"][0][0] == 0x40

    # Two-program round trip: 2 VS sharing 1 PS by index (the extended
    # container geometry: prog0 = stock pair, prog1 = perspective VS + same PS).
    fake_vs1 = struct.pack("<II", 0xFFFE0300, 0x0000FFFF) + b"\x00" * 8
    blob2 = pack(name, [fake_vs, fake_vs1], [fake_ps], [(0, 0, 0), (0, 1, 0)])
    info2 = inspect(blob2, expect_name=name)
    assert info2["counts"] == (2, 2, 1)
    assert info2["programs"] == [(0, 0, 0), (0, 1, 0)]
    # Raw entry bytes at the engine-verified offsets: VS idx @+4, PS idx @+5.
    ptr_a = struct.unpack_from("<I", blob2, 0x0C)[0]
    assert blob2[ptr_a : ptr_a + 8] == bytes([0, 0, 0, 0, 0, 0, 0, 0])
    assert blob2[ptr_a + 8 : ptr_a + 16] == bytes([0, 0, 0, 0, 1, 0, 0, 0])
    # Blob tiling: 16-aligned, in table order, PS shared blob appears once.
    (vs0_off, vs0_sz), (vs1_off, vs1_sz) = info2["vs"]
    ((ps0_off, ps0_sz),) = info2["ps"]
    assert vs0_off == 0x50, hex(vs0_off)  # tables: 0x20+0x10 progs, 2*8 vs, 1*8 ps
    assert vs1_off == align(vs0_off + vs0_sz, BLOB_ALIGN)
    assert ps0_off == align(vs1_off + vs1_sz, BLOB_ALIGN)
    assert blob2[vs1_off : vs1_off + vs1_sz] == fake_vs1
    assert blob2[ps0_off : ps0_off + ps0_sz] == fake_ps
    assert info2["slack"] == 0

    # Negative checks must all raise.
    for fn in (
        lambda: pack(name, [fake_ps], [fake_vs]),  # swapped blobs
        lambda: inspect(blob, expect_name="not_the_name"),  # wrong name
        lambda: pack(name, [fake_vs], [fake_ps], [(0, 1, 0)]),  # vs_idx OOB
        lambda: pack(name, [fake_vs], [fake_ps], [(0, 0, 1)]),  # ps_idx OOB
        lambda: pack(name, [fake_vs], [fake_ps], []),  # no programs
    ):
        try:
            fn()
        except ValueError:
            pass
        else:
            raise AssertionError("expected ValueError")

    print("selftest OK")
    return 0


# -- cli --------------------------------------------------------------------


def parse_program(spec: str) -> tuple[int, int, int]:
    """Parse a --program VSIDX:PSIDX[:FLAGS] spec."""
    parts = spec.split(":")
    if len(parts) not in (2, 3):
        raise ValueError(f"bad --program spec {spec!r} (want VSIDX:PSIDX[:FLAGS])")
    vsi, psi = int(parts[0]), int(parts[1])
    flags = int(parts[2], 0) if len(parts) == 3 else 0
    return (flags, vsi, psi)


def main() -> int:
    ap = argparse.ArgumentParser(description="GSPW shader container packer/inspector")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("pack", help="wrap VS+PS bytecode into a .gsp")
    p.add_argument("--name", required=True, help="bare shader name (hashed FNV-1)")
    p.add_argument(
        "--vs", required=True, action="append",
        help="vertex shader d3dbc blob (repeatable; table order)",
    )
    p.add_argument(
        "--ps", required=True, action="append",
        help="pixel shader d3dbc blob (repeatable; table order)",
    )
    p.add_argument(
        "--program", action="append", metavar="VSIDX:PSIDX[:FLAGS]",
        help="program entry (repeatable; default one program 0:0)",
    )
    p.add_argument("-o", "--output", required=True)

    i = sub.add_parser("inspect", help="parse + validate a .gsp")
    i.add_argument("file")
    i.add_argument("--expect-name", help="assert the name hash matches this name")

    sub.add_parser("selftest", help="run internal round-trip checks")

    args = ap.parse_args()
    if args.cmd == "selftest":
        return selftest()
    if args.cmd == "inspect":
        info = inspect(open(args.file, "rb").read(), expect_name=args.expect_name)
        print_report(info)
        return 0

    vs_blobs = [open(f, "rb").read() for f in args.vs]
    ps_blobs = [open(f, "rb").read() for f in args.ps]
    programs = [parse_program(s) for s in args.program] if args.program else None
    blob = pack(args.name, vs_blobs, ps_blobs, programs)
    inspect(blob, expect_name=args.name)  # self-verify before writing
    with open(args.output, "wb") as f:
        f.write(blob)
    print(f"wrote {args.output} ({len(blob)} bytes, hash 0x{fnv1_32(args.name.encode()):08X})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
