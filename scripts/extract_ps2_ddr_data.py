#!/usr/bin/env python3
"""Extract assets from a PS2 DanceDanceRevolution disc (FILEDATA.BIN + FILEDTnn.BIN).

The archive is one flat, 0x800-sector-aligned address space split over several disc files:
FILEDATA.BIN, then FILEDT02.BIN, FILEDT03.BIN, ..., laid out back to back on the disc.
The game holds only the disc LBA of FILEDATA.BIN. Every other location is a sector offset
from that LBA, so the files have to be read as one concatenated span.

File table (TOC). This is a static array in the game ELF, so it is exact: no heuristics.
Each entry is 8 bytes, little-endian:

    u16 id | u24 sector_offset | u24 sector_count        (terminated by id 0xFFFF)

- Entries are sorted by id, because the game binary-searches the table by id.
- The table has no names; files are known only by id.
- The game uses a raw id OR a pointer to an entry as a file handle, and it tells the two
  apart with (handle & 7) != 0. That is why no id is a multiple of 8.
- Sizes are whole sectors, so each extracted file keeps its zero padding.
- A few sector ranges are listed in no entry. They are extracted too, under hidden/, split
  into payloads where a payload describes its own size.

This layout also matches root670's ddr-tools `filedata-tool.py` for DDR MAX..Party
Collection. The RE is for DDR STRIKE (SLPM_662.42); see docs/ps2_ddr_filedata_research.md.

Payload formats. These are the parts ported from RhythmCodex (Source/RhythmCodex.Lib, MIT)
and ddr-tools:

- Bemani LZ (`lz_decode`): 8-flag control bytes, a 0x400 window, 0xFF ends the stream.
  It is the same codec as extract_sys573_data.decode_lz.
- TCB image (`decode_tcb`). This is TIM2 with a "TCB\\0" file header:
  - picture header at 0x10: { total, clut_size, image_size, u16 header_size,
    u16 clut_colors, u8 clut_type, u8 mips, u8 image_type, u8 -, u16 w, u16 h, gs regs };
  - the pixels start at 0x10 + header_size + 0x10, after a 16-byte GIF tag;
  - the CLUT starts at 0x10 + header_size + image_size + 0x10;
  - image_type 4 = 4bpp and 5 = 8bpp (both indexed), 1/2/3 = 16/24/32-bit direct;
  - 256-colour CLUTs are CSM1-swizzled;
  - alpha 0x80 = opaque.
- Svag audio (`decode_svag`): { "Svag", data_size, rate, channels, interleave }.
  PS-ADPCM data starts at 0x800, channel blocks are `interleave` bytes each.
- MPEG-1/2 video elementary streams (the song background clips); these are written as-is.
- Polygon dancers, in the System 573 formats of scripts/sys573_dancer_dump.py. They are
  recognized by structure and named for it: `.cmd` meshes and `.cmm` motion sets. The rig
  tables (chara.lst, chara.pos) are data in the ELF, so they are copied to elf/.

Containers (--unpack). The game knows each file's format from code. This tool recognizes
four layouts by structure only, so this step is heuristic:

    pairs   { u32 offset, u32 size } x n; n = smallest offset / 8; (0,0) = empty slot
    offsets { u32 offset } x n; n = smallest offset / 4 (RhythmCodex's "unbound table");
            0 = empty slot; a member ends at the next larger offset
    counted { u32 n, u32 offset[n+1] }; the last offset is the end of the table's data
    lzseq   back-to-back Bemani LZ streams on a fixed alignment (animated sprites)

The slots of pairs/offsets tables may be unsorted. A table is accepted only if its
structure is consistent and at least one member is recognized: known magic, a clean Bemani
LZ stream, or a nested table. Members are Bemani-LZ compressed or raw, and are unpacked
recursively, named by slot. The same checks apply to whole files.
With --png, every TCB gets a .png written next to it; with --wav, every Svag a .wav.

Usage:
    extract_ps2_ddr_data.py extract <game> <game_dir> <out_dir> [--unpack] [--png] [--wav]
                            [--exclude mpeg,svag] [--ids 0xc91-0xd3b]
    extract_ps2_ddr_data.py find-toc <elf> <archive files...>  # locate a TOC for a new game
    extract_ps2_ddr_data.py lz <in> <out> [--offset N]          # decompress one stream
    extract_ps2_ddr_data.py tcb <in.tcb> <out.png>
    extract_ps2_ddr_data.py svag <in.svag> <out.wav>

    game_dir is the disc's file tree: SYSTEM.CNF, the ELF, and DATA/.

Examples:
    extract_ps2_ddr_data.py extract strike_jp ~/Desktop/ddr_strike ./strike --unpack --png --exclude mpeg

Outputs:
- files/<id>.<ext> and hidden/<sector>.<ext>;
- manifest.csv (TOC entries and hidden ranges);
- unpacked/<id>/<slot>[/<slot>...].<ext> and unpack_manifest.csv;
- elf/ (per-game ELF data tables, e.g. the dancer rig).

Import-safe: `from extract_ps2_ddr_data import lz_decode, decode_tcb, read_toc, unpack`.
Only the standard library is needed. Tests: scripts/validate_ps2_ddr_tools.sh.
"""

import argparse
import csv
import re
import struct
import sys
import wave
import zlib
from pathlib import Path

SECTOR = 0x800

# Per-game parameters, found by reverse-engineering the game ELF.
#   elf:        boot executable (SYSTEM.CNF BOOT2), relative to game_dir
#   toc_va:     virtual address of the first TOC entry
#   toc_count:  entries before the 0xFFFF terminator
#   archives:   files forming the sector span, in disc order (relative to game_dir)
#   elf_assets: [optional] {name: (va, size)} data tables to copy out of the ELF into elf/
GAMES = {
    # DDR STRIKE (JP, 2006). Addresses are Ghidra VAs (ELF image base 0x100000).
    # - FUN_0011dd50 passes FILEDATA.BIN's LBA as a constant, FUN_00191b70("filedata.bin", 4000).
    # - FUN_00192150 adds an entry's sector offset to that base LBA.
    # - FUN_00193c30 splits an entry into offset and count; FUN_001939a0 binary-searches by id.
    # - On the disc, FILEDT02 starts at LBA 528316 = 4000 + 0x8001C (FILEDATA's sector
    #   count), and FILEDT03 starts at 1052814.
    "strike_jp": {
        "elf": "SLPM_662.42",
        "toc_va": 0x2CB668,
        "toc_count": 2951,
        "archives": ["DATA/FILEDATA.BIN", "DATA/FILEDT02.BIN", "DATA/FILEDT03.BIN"],
        # The polygon dancers are the System 573 engine's, and the rig tables live in the ELF
        # in 573's file layouts (docs/sys573_dancers_research.md section 2):
        # - FUN_001af050 takes the object table from 0x296AB0 + type * 0x40;
        # - FUN_001b21d0 reads the rest offsets from 0x296CB0 and skips entry 0.
        "elf_assets": {
            "chara.lst": (0x296AF0, 57),  # type 1: u8 nobj (28), then (joint, parent) per object
            "chara20.lst": (0x296AB0, 41),  # type 0: the 20-object variant
            "chara.pos": (0x296CB0, 102),  # 17 x int16 xyz; entry j+1 = joint j's rest offset
        },
    },
}

TOC_ENTRY_SIZE = 8
TOC_END_ID = 0xFFFF


class ArchiveError(Exception):
    """The ELF, TOC or archive files don't match what the game config expects."""


# ---------------------------------------------------------------------------
# ELF + TOC
# ---------------------------------------------------------------------------
def elf_segments(elf):
    """Return [(va, file_offset, file_size)] for the PT_LOAD segments of a 32-bit LE ELF."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        raise ArchiveError("not a 32-bit little-endian ELF")
    phoff, = struct.unpack_from("<I", elf, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x2A)
    segs = []
    for i in range(phnum):
        p_type, p_offset, p_vaddr, _paddr, p_filesz = struct.unpack_from("<5I", elf, phoff + i * phentsize)
        if p_type == 1 and p_filesz:
            segs.append((p_vaddr, p_offset, p_filesz))
    return segs


def va_to_offset(segs, va, size=1):
    for seg_va, off, fsz in segs:
        if seg_va <= va and va + size <= seg_va + fsz:
            return off + va - seg_va
    raise ArchiveError(f"address 0x{va:x} is not file-backed in the ELF")


def unpack_toc_entry(raw8):
    v, = struct.unpack("<Q", raw8)
    return v & 0xFFFF, (v >> 16) & 0xFFFFFF, v >> 40


def read_toc(elf, toc_va, count, span_sectors):
    """Return [(id, sector, sectors)] and validate it against the archive span."""
    segs = elf_segments(elf)
    off = va_to_offset(segs, toc_va, (count + 1) * TOC_ENTRY_SIZE)
    entries = [unpack_toc_entry(elf[off + i * 8: off + i * 8 + 8]) for i in range(count)]
    end_id = unpack_toc_entry(elf[off + count * 8: off + count * 8 + 8])[0]
    if end_id != TOC_END_ID:
        raise ArchiveError(f"TOC entry {count} is id 0x{end_id:x}, not the 0xFFFF terminator; wrong toc_va/toc_count?")
    prev = -1
    for fid, sector, sectors in entries:
        if fid <= prev:
            raise ArchiveError(f"TOC ids are not ascending at id 0x{fid:x}")
        if sectors == 0 or sector + sectors > span_sectors:
            raise ArchiveError(f"TOC id 0x{fid:x} (sectors 0x{sector:x}+0x{sectors:x}) is outside the archive span")
        prev = fid
    return entries


def find_toc_candidates(elf, span_sectors, min_entries=16):
    """Scan an ELF for 8-byte TOC-shaped runs: ascending ids, in-span, mostly contiguous.

    For bringing up a new game: prints where the table probably is. Returns
    [(va, count, terminated, contiguous_fraction)] best first.
    """
    found = []
    for seg_va, seg_off, fsz in elf_segments(elf):
        for phase in range(0, TOC_ENTRY_SIZE, 2):
            pos = phase
            while pos + TOC_ENTRY_SIZE <= fsz:
                start, prev_id, n, contiguous, prev_end = pos, -1, 0, 0, None
                while pos + TOC_ENTRY_SIZE <= fsz:
                    fid, sector, sectors = unpack_toc_entry(elf[seg_off + pos: seg_off + pos + 8])
                    if fid <= prev_id or sectors == 0 or sector + sectors > span_sectors:
                        break
                    contiguous += prev_end == sector
                    prev_id, prev_end, n, pos = fid, sector + sectors, n + 1, pos + 8
                if n >= min_entries:
                    terminated = pos + 8 <= fsz and unpack_toc_entry(elf[seg_off + pos: seg_off + pos + 8])[0] == TOC_END_ID
                    found.append((seg_va + start, n, terminated, contiguous / max(1, n - 1)))
                pos = max(pos, start + TOC_ENTRY_SIZE)
    return sorted(found, key=lambda c: (-c[2], -c[1]))


class Span:
    """The archive files read as one sector-addressed span."""

    def __init__(self, paths):
        self.parts = []
        first = 0
        for p in paths:
            size = p.stat().st_size
            if size % SECTOR:
                raise ArchiveError(f"{p.name} is not a whole number of sectors")
            self.parts.append((first, size // SECTOR, p))
            first += size // SECTOR
        self.sectors = first
        self._fh = {}

    def locate(self, sector, sectors):
        """Return (path, byte offset) of a range; it must not straddle two files."""
        for first, count, p in self.parts:
            if first <= sector < first + count:
                if sector + sectors > first + count:
                    raise ArchiveError(f"sector range 0x{sector:x}+0x{sectors:x} straddles {p.name}")
                return p, (sector - first) * SECTOR
        raise ArchiveError(f"sector 0x{sector:x} is past the archive span")

    def read(self, sector, sectors):
        p, off = self.locate(sector, sectors)
        fh = self._fh.get(p) or self._fh.setdefault(p, open(p, "rb"))
        fh.seek(off)
        return fh.read(sectors * SECTOR)


def gaps(entries, span_sectors):
    """Sector ranges not covered by any TOC entry: [(sector, sectors)]."""
    out, pos = [], 0
    for _fid, sector, sectors in sorted(entries, key=lambda e: e[1]):
        if sector > pos:
            out.append((pos, sector - pos))
        pos = max(pos, sector + sectors)
    if pos < span_sectors:
        out.append((pos, span_sectors - pos))
    return out


# ---------------------------------------------------------------------------
# Bemani LZ (port of RhythmCodex BemaniLzDecoder)
# ---------------------------------------------------------------------------
LZ_WINDOW = 0x400


def lz_decode(src, pos=0, end=None):
    """Decode a Bemani LZ stream starting at src[pos], reading no further than src[end].

    Returns (data, stream_end, clean). `clean` is True if the 0xFF end code was reached,
    False if the input ran out first. Copies that reach before the start of the output
    read zeros, as from the game's zero-initialized window.
    """
    end = len(src) if end is None else end
    out = bytearray(LZ_WINDOW)  # the zero window, stripped on return
    control = 0
    try:
        while True:
            control >>= 1
            if control < 0x100:
                if pos >= end:
                    break
                control = src[pos] | 0xFF00
                pos += 1
            if pos >= end:
                break
            b = src[pos]
            pos += 1
            if not control & 1:
                out.append(b)
                continue
            if not b & 0x80:  # long distance: 10-bit distance, 3-34 bytes
                if pos >= end:
                    break
                dist = src[pos] | ((b & 3) << 8)
                pos += 1
                length = (b >> 2) + 3
            elif not b & 0x40:  # short distance: 1-16 back, 2-5 bytes
                dist = (b & 0x0F) + 1
                length = (b >> 4) - 6
            elif b == 0xFF:
                return bytes(out[LZ_WINDOW:]), pos, True
            else:  # 0xC0-0xFE: literal run of 8-70 bytes
                length = b - 0xB8
                if pos + length > end:
                    break
                out += src[pos:pos + length]
                pos += length
                continue
            start = len(out) - dist
            if dist >= length:
                out += out[start:start + length]
            else:
                for k in range(length):
                    out.append(out[start + k])
    except IndexError:
        pass
    return bytes(out[LZ_WINDOW:]), pos, False


def lz_member(blob, start, extent):
    """Decode blob[start:extent] if it is exactly one clean LZ stream (zero padding allowed)."""
    data, stop, clean = lz_decode(blob, start, extent)
    if not clean or any(blob[stop:extent]) or len(data) < (stop - start) * 0.9 or not data:
        return None
    return data


# ---------------------------------------------------------------------------
# Leaf types
# ---------------------------------------------------------------------------
TCB_MAGIC = b"TCB\x00" + b"\x00" * 12


def _is_dancer_cmd(blob):
    """System 573 dancer mesh (.cmd): 8 zero bytes, nobj, then {offset, nsub, 0x1000} per object."""
    if len(blob) < 0x2C or any(blob[:8]):
        return False
    nobj = struct.unpack_from("<I", blob, 8)[0]
    if not 1 <= nobj <= 64 or 0x20 + 12 * nobj > len(blob):
        return False
    return all(0 < off < len(blob) and 1 <= nsub <= 16 and scale == 0x1000
               for off, nsub, scale in struct.iter_unpack("<3I", blob[0x20:0x20 + 12 * nobj]))


def _is_dancer_cmm(blob):
    """System 573 dancer motion (.cmm): u16 'S', u16 clip count, u32 8, {name, clip} offsets."""
    if len(blob) < 16 or blob[0] != 0x53 or blob[1] or struct.unpack_from("<I", blob, 4)[0] != 8:
        return False
    n = struct.unpack_from("<H", blob, 2)[0]
    return 1 <= n <= 256 and struct.unpack_from("<I", blob, 8)[0] == 8 + 8 * n


def sniff(blob):
    """Classify a payload by magic. Returns (type, extension) or None for unknown data."""
    if blob[:16] == TCB_MAGIC:
        return "tcb", "tcb"
    if blob[:4] == b"Svag":
        return "svag", "svag"
    if blob[:4] == b"\x00\x00\x01\xb3":
        return ("mpeg2", "m2v") if b"\x00\x00\x01\xb5" in blob[:0x100] else ("mpeg1", "m1v")
    if blob[:4] == b"ipum":
        return "ipu", "ipu"
    if _is_dancer_cmd(blob):
        return "dancer_mesh", "cmd"
    if _is_dancer_cmm(blob):
        return "dancer_motion", "cmm"
    return None


def leaf_extent(blob, kind):
    """Self-described byte size of a known leaf, or None."""
    if kind == "tcb" and len(blob) >= 0x14:
        return 0x10 + struct.unpack_from("<I", blob, 0x10)[0]
    if kind == "svag" and len(blob) >= 8:
        return SECTOR + struct.unpack_from("<I", blob, 4)[0]
    return None


# ---------------------------------------------------------------------------
# Containers (heuristic; see module docstring)
# ---------------------------------------------------------------------------
MAX_TABLE_HEADER = 0x10000
_NONZERO = re.compile(rb"[^\x00]")


def _words(blob, count, off=0):
    return struct.unpack_from(f"<{count}I", blob, off)


def parse_pairs(blob):
    """{offset, size} pairs; the slot count is the smallest offset / 8, and (0, 0) is empty.

    Slots may be unsorted, but the members, sorted by offset, must follow each other with
    less than a sector of padding. Returns [(slot, offset, extent)] or None.
    """
    count, slot, members = None, 0, []
    while count is None or slot < count:
        if 8 * slot + 8 > len(blob) or slot >= MAX_TABLE_HEADER // 8:
            return None
        off, size = _words(blob, 2, 8 * slot)
        if off or size:
            if off < 8 or size == 0 or off + size > len(blob):
                return None
            count = off // 8 if count is None else min(count, off // 8)
            members.append((slot, off, off + size))
        elif count is None:
            return None
        slot += 1
    members = [m for m in members if m[0] < count]
    prev_end = 8 * count
    for _slot, off, end in sorted(members, key=lambda m: m[1]):
        if off < prev_end or off - prev_end >= (0x10 if prev_end == 8 * count else SECTOR):
            return None
        prev_end = end
    return members


def parse_offsets(blob):
    """Offset list, RhythmCodex's "unbound table": the slot count is the smallest offset / 4.

    Slots may be unsorted, and a 0 slot is empty. Each member ends at the next larger
    offset, or at the end of the blob. Returns [(slot, offset, extent)] or None.
    """
    count, slot = None, 0
    while count is None or slot < count:
        if 4 * slot + 4 > len(blob) or slot >= MAX_TABLE_HEADER // 4:
            return None
        off = _words(blob, 1, 4 * slot)[0]
        if off:
            if off < 4 or off >= len(blob):
                return None
            count = off // 4 if count is None else min(count, off // 4)
        elif count is None:
            return None
        slot += 1
    slots = _words(blob, count)
    starts = sorted({off for off in slots if off})
    if starts[0] - 4 * count >= 4:
        return None
    extent = dict(zip(starts, starts[1:] + [len(blob)]))
    return [(k, off, extent[off]) for k, off in enumerate(slots) if off]


def parse_counted(blob):
    """{n, offset[n+1]}; the last offset is the end of the data. Returns [(slot, offset, extent)] or None."""
    if len(blob) < 12:
        return None
    n = _words(blob, 1)[0]
    if not 1 <= n <= 0x4000 or 4 * (n + 2) > len(blob):
        return None
    offs = _words(blob, n + 1, 4)
    header = 4 * (n + 2)
    if offs[0] not in (header, (header + 15) & ~15) or offs[-1] > len(blob):
        return None
    if any(b < a for a, b in zip(offs, offs[1:])):
        return None
    return [(k, offs[k], offs[k + 1]) for k in range(n) if offs[k + 1] > offs[k]]


def parse_lz_sequence(blob):
    """Back-to-back Bemani LZ streams, each starting on the alignment the first gap implies
    (e.g. 0x400 in STRIKE's animated-sprite files). The streams must cover the whole blob,
    with only zeros after it. A zero gap must follow the first stream, and every stream must
    expand, so random data doesn't pass. Returns [(slot, offset, extent)] or None."""
    members, pos, align = [], 0, None
    while True:
        data, stop, clean = lz_decode(blob, pos)
        if not clean or len(data) < max(0x40, stop - pos):
            return None
        members.append((len(members), pos, stop))
        found = _NONZERO.search(blob, stop)
        if not found:
            break
        nxt = found.start()
        if align is None:
            if nxt == stop:
                return None
            align = SECTOR
            while align > 0x10 and nxt % align:
                align //= 2
            if nxt % align:
                return None
        pos = (stop + align - 1) // align * align
        if pos > nxt:
            return None
    return members if len(members) >= 2 else None


CONTAINER_PARSERS = (("pairs", parse_pairs), ("counted", parse_counted), ("offsets", parse_offsets))


class Node:
    """An unpacked payload: a leaf (type, data) or a container (kind, children)."""

    def __init__(self, data, kind, ext, compressed=False, offset=0, children=None, index=0):
        self.data, self.kind, self.ext, self.compressed = data, kind, ext, compressed
        self.offset, self.children, self.index = offset, children or [], index


def _container(blob, kind, layout, depth, compressed, offset, max_depth):
    children, recognized = [], 0
    for slot, start, extent in layout:
        child, ok = identify_member(blob, start, extent, depth + 1, max_depth)
        child.index = slot
        children.append(child)
        recognized += ok
    if not recognized:
        return None
    return Node(blob, kind, None, compressed, offset, children)


def identify(blob, depth, compressed=False, offset=0, max_depth=4):
    """Classify a payload. Returns (Node, recognized) where `recognized` means the
    payload is a known leaf, a clean LZ stream or a validated table."""
    known = sniff(blob)
    if known:
        return Node(blob, known[0], known[1], compressed, offset), True
    if depth < max_depth:
        for kind, parse in CONTAINER_PARSERS:
            layout = parse(blob)
            node = _container(blob, kind, layout, depth, compressed, offset, max_depth) if layout else None
            if node:
                return node, True
        if not compressed:
            data = lz_member(blob, 0, len(blob))
            if data is not None:
                node, _ = identify(data, depth, True, offset, max_depth)
                return node, True
            layout = parse_lz_sequence(blob)
            node = _container(blob, "lzseq", layout, depth, compressed, offset, max_depth) if layout else None
            if node and node.children[0].kind != "unknown":  # the first frame must be a known type
                return node, True
    return Node(blob, "unknown", "bin", compressed, offset), False


def identify_member(blob, start, extent, depth, max_depth):
    """A container member: raw, LZ, or LZ behind RhythmCodex's 12-byte TCB-table header."""
    raw = blob[start:extent]
    if sniff(raw) is None:
        data = lz_member(blob, start, extent)
        if data is not None:
            node, _ = identify(data, depth, True, start, max_depth)
            return node, True
        if extent - start > 0xC:
            data = lz_member(blob, start + 0xC, extent)
            if data is not None and sniff(data):
                node, _ = identify(data, depth, True, start, max_depth)
                node.kind += "+hdr12"
                return node, True
    node, ok = identify(raw, depth, False, start, max_depth)
    return node, ok


def unpack(blob, max_depth=4):
    """Unpack a TOC payload into a Node tree (the root may itself be a leaf)."""
    return identify(blob, 0, max_depth=max_depth)[0]


def payload_extent(blob):
    """Self-described byte length of the payload at blob[0], or None if it can't be told."""
    known = sniff(blob)
    if known:
        if known[0].startswith("mpeg"):
            end = blob.find(b"\x00\x00\x01\xb7")  # sequence end code
            return end + 4 if end >= 0 else None
        return leaf_extent(blob, known[0])
    for kind, parse in CONTAINER_PARSERS:
        layout = parse(blob)
        if not layout:
            continue
        if kind != "offsets":
            return max(extent for _slot, _off, extent in layout)
        _data, stop, clean = lz_decode(blob, max(off for _slot, off, _extent in layout))  # implicit end
        return stop if clean else None
    data, stop, clean = lz_decode(blob)
    if clean and len(data) >= 0x40 and (sniff(data) or any(parse(data) for _, parse in CONTAINER_PARSERS)):
        return stop
    return None


def split_hidden(blob):
    """Split an unreferenced sector range into payloads: [(byte offset, byte size)].

    Payloads start on sector boundaries. Each piece ends at the next sector boundary after
    its self-described extent; the rest of the range becomes one piece once an extent can't
    be told. All-zero sectors are skipped. Heuristic, like --unpack.
    """
    pieces, pos = [], 0
    while pos < len(blob):
        if not any(blob[pos:pos + SECTOR]):
            pos += SECTOR
            continue
        extent = payload_extent(blob[pos:])
        if not extent or pos + extent > len(blob):
            pieces.append((pos, len(blob) - pos))
            break
        size = (extent + SECTOR - 1) // SECTOR * SECTOR
        pieces.append((pos, size))
        pos += size
    return pieces


# ---------------------------------------------------------------------------
# TCB -> RGBA (ports of RhythmCodex TcbImageDecoder / ddr-tools tcb-convert.c)
# ---------------------------------------------------------------------------
def _csm1_unswizzle(colors):
    """Undo the PS2 CSM1 CLUT order (swap entries 8-15 and 16-23 of every 32)."""
    out = list(colors)
    for base in range(0, len(colors) - 31, 32):
        out[base + 8:base + 16], out[base + 16:base + 24] = colors[base + 16:base + 24], colors[base + 8:base + 16]
    return out


def _alpha(a):
    return min(255, a * 2) if a < 0x80 else 255


def _rgba32(c):
    return bytes((c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF, _alpha(c >> 24)))


def _rgba16(c):
    r, g, b = (c & 0x1F) << 3, ((c >> 5) & 0x1F) << 3, ((c >> 10) & 0x1F) << 3
    return bytes((r | r >> 5, g | g >> 5, b | b >> 5, 255 if c & 0x8000 else 0))


def decode_tcb(tcb):
    """Return (width, height, rgba bytes) for the base level of a TCB image."""
    if tcb[:16] != TCB_MAGIC or len(tcb) < 0x40:
        raise ValueError("not a TCB image")
    (_total, clut_size, image_size, header_size, clut_colors, clut_type, _mips, image_type, _bpp,
     width, height) = struct.unpack_from("<3I2H4B2H", tcb, 0x10)
    pixels_at = 0x10 + header_size + 0x10
    clut_at = 0x10 + header_size + image_size + 0x10
    count = width * height
    if image_type in (4, 5):
        fmt = clut_type & 0x3F
        step = 2 if fmt == 1 else 4
        n = min(clut_colors or (16 if image_type == 4 else 256), (clut_size - 0x10) // step)
        raw = struct.unpack_from(f"<{n}{'H' if step == 2 else 'I'}", tcb, clut_at)
        if n == 256 and not clut_type & 0x80:
            raw = _csm1_unswizzle(raw)
        palette = [(_rgba16 if step == 2 else _rgba32)(c) for c in raw]
        palette += [b"\x00\x00\x00\x00"] * (256 - len(palette))
        src = tcb[pixels_at:pixels_at + ((count + 1) // 2 if image_type == 4 else count)]
        if image_type == 5:
            return width, height, b"".join(palette[i] for i in src)
        out = bytearray()
        for byte in src:
            out += palette[byte & 0xF] + palette[byte >> 4]
        return width, height, bytes(out[:count * 4])
    if image_type == 3:
        words = struct.unpack_from(f"<{count}I", tcb, pixels_at)
        return width, height, b"".join(_rgba32(c) for c in words)
    if image_type == 2:
        src = tcb[pixels_at:pixels_at + count * 3]
        return width, height, b"".join(src[i:i + 3] + b"\xff" for i in range(0, len(src), 3))
    if image_type == 1:
        words = struct.unpack_from(f"<{count}H", tcb, pixels_at)
        return width, height, b"".join(_rgba16(c) for c in words)
    raise ValueError(f"unsupported TCB image type {image_type}")


def write_png(path, width, height, rgba):
    rows = b"".join(b"\x00" + rgba[y * width * 4:(y + 1) * width * 4] for y in range(height))

    def chunk(tag, body):
        return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body))

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(rows, 6)) + chunk(b"IEND", b""))
    Path(path).write_bytes(png)


# ---------------------------------------------------------------------------
# Svag -> PCM (PS-ADPCM; container per RhythmCodex SvagHeuristic)
# ---------------------------------------------------------------------------
VAG_COEFS = ((0, 0), (60, 0), (115, -52), (98, -55), (122, -60))


def _adpcm_channel(data):
    out, s1, s2 = [], 0, 0
    for f in range(0, len(data) - 15, 16):
        shift, pred = data[f] & 0xF, min(data[f] >> 4, 4)
        c1, c2 = VAG_COEFS[pred]
        for byte in data[f + 2:f + 16]:
            for nib in (byte & 0xF, byte >> 4):
                v = (nib - 16 if nib & 8 else nib) << 12
                s = (v >> shift) + (s1 * c1 + s2 * c2 + 32 >> 6)
                s = -32768 if s < -32768 else 32767 if s > 32767 else s
                out.append(s)
                s2, s1 = s1, s
    return out


def decode_svag(svag):
    """Return (rate, channels, interleaved int16 samples as bytes)."""
    magic, size, rate, channels, interleave = struct.unpack_from("<4s4I", svag, 0)
    if magic != b"Svag" or not 1 <= channels <= 8:
        raise ValueError("not an Svag stream")
    body = svag[SECTOR:SECTOR + size]
    if channels > 1:
        block = interleave * channels
        streams = [b"".join(body[b + c * interleave:b + (c + 1) * interleave] for b in range(0, len(body), block))
                   for c in range(channels)]
    else:
        streams = [body]
    pcm = [_adpcm_channel(s) for s in streams]
    frames = min(len(p) for p in pcm)
    interleaved = [pcm[c][i] for i in range(frames) for c in range(channels)]
    return rate, channels, struct.pack(f"<{len(interleaved)}h", *interleaved)


def write_wav(path, rate, channels, pcm):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm)


# ---------------------------------------------------------------------------
# Extraction
# ---------------------------------------------------------------------------
def _tilde(path):
    """`path` for printing, with the home directory written as ~."""
    text, home = str(path), str(Path.home())
    return "~" + text[len(home):] if text == home or text.startswith(home + "/") else text


def parse_ids(spec):
    lo, _, hi = spec.partition("-")
    lo = int(lo, 0)
    return lo, int(hi, 0) if hi else lo


def convert_leaf(node, path, opts, stats):
    try:
        if opts.png and node.kind == "tcb":
            write_png(path.with_suffix(".png"), *decode_tcb(node.data))
            stats["png"] += 1
        elif opts.wav and node.kind == "svag":
            write_wav(path.with_suffix(".wav"), *decode_svag(node.data))
            stats["wav"] += 1
    except (ValueError, struct.error) as e:
        stats["convert_errors"].append(f"{_tilde(path)}: {e}")


def tcb_dims(node):
    if node.kind == "tcb" and len(node.data) >= 0x28:
        return struct.unpack_from("<HH", node.data, 0x24)
    return "", ""


def write_tree(node, path, rel, rows, opts, stats):
    """Write a Node tree under `path` (a stem without extension); record manifest rows."""
    if node.children:
        path.mkdir(parents=True, exist_ok=True)
        for child in node.children:
            name = f"{child.index:03d}"
            write_tree(child, path / name, f"{rel}/{name}", rows, opts, stats)
        rows.append([rel + "/", node.kind, len(node.children), f"0x{node.offset:x}", len(node.data),
                     int(node.compressed), "", ""])
        return
    out = path.with_name(f"{path.name}.{node.ext}")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(node.data)
    stats["members"] += 1
    rows.append([f"{rel}.{node.ext}", node.kind, "", f"0x{node.offset:x}", len(node.data), int(node.compressed),
                 *tcb_dims(node)])
    convert_leaf(node, out, opts, stats)


EXCLUDABLE = {"mpeg", "svag", "tcb"}


def cmd_extract(args):
    cfg = GAMES[args.game]
    elf_path = args.game_dir / cfg["elf"]
    archive_paths = [args.game_dir / p for p in cfg["archives"]]
    exclude = {t.strip() for t in args.exclude.split(",") if t.strip()} if args.exclude else set()
    if exclude - EXCLUDABLE:
        sys.exit(f"error: --exclude takes {', '.join(sorted(EXCLUDABLE))}, not {', '.join(sorted(exclude - EXCLUDABLE))}")
    id_range = parse_ids(args.ids) if args.ids else None
    for p in (elf_path, *archive_paths):
        if not p.is_file():
            sys.exit(f"error: {_tilde(p)} not found")

    try:
        span = Span(archive_paths)
        elf = elf_path.read_bytes()
        entries = read_toc(elf, cfg["toc_va"], cfg["toc_count"], span.sectors)
        segs = elf_segments(elf)
        elf_assets = {name: elf[va_to_offset(segs, va, size):va_to_offset(segs, va, size) + size]
                      for name, (va, size) in cfg.get("elf_assets", {}).items()}
        for _fid, sector, sectors in entries:
            span.locate(sector, sectors)
        hidden = [] if id_range else gaps(entries, span.sectors)
        for sector, sectors in hidden:
            span.locate(sector, sectors)
    except ArchiveError as e:
        sys.exit(f"error: {e}")

    jobs = [(f"{fid:04x}", fid, sector, sectors, "files") for fid, sector, sectors in entries
            if not id_range or id_range[0] <= fid <= id_range[1]]
    for gap_sector, gap_sectors in hidden:
        for off, size in split_hidden(span.read(gap_sector, gap_sectors)):
            sector = gap_sector + off // SECTOR
            jobs.append((f"{sector:06x}", None, sector, size // SECTOR, "hidden"))

    out = args.out_dir
    (out / "files").mkdir(parents=True, exist_ok=True)
    if elf_assets:
        (out / "elf").mkdir(exist_ok=True)
        for name, data in elf_assets.items():
            (out / "elf" / name).write_bytes(data)
    if hidden:
        (out / "hidden").mkdir(exist_ok=True)
    stats = {"files": 0, "hidden": 0, "members": 0, "png": 0, "wav": 0, "skipped": 0, "convert_errors": []}
    unpack_rows = []
    with open(out / "manifest.csv", "w", newline="") as mf:
        w = csv.writer(mf)
        w.writerow(["id", "sector", "sectors", "archive", "archive_offset", "type", "file"])
        for n, (stem, fid, sector, sectors, subdir) in enumerate(jobs, 1):
            archive, archive_off = span.locate(sector, sectors)
            row = [stem if fid is not None else "hidden", f"0x{sector:x}", sectors, archive.name, f"0x{archive_off:x}"]
            blob = span.read(sector, sectors)
            known = sniff(blob)
            kind, ext = known or ("unknown", "bin")
            if kind.startswith("mpeg") and "mpeg" in exclude or kind in exclude:
                stats["skipped"] += 1
                w.writerow(row + [kind, ""])
                continue
            rel = f"{subdir}/{stem}.{ext}"
            (out / rel).write_bytes(blob)
            stats[subdir] += 1
            if args.unpack and known is None:
                tree = unpack(blob)
                if tree.children or tree.compressed:
                    kind = tree.kind + ("+lz" if tree.compressed else "")
                    write_tree(tree, out / "unpacked" / stem, stem, unpack_rows, args, stats)
            elif known:
                convert_leaf(Node(blob, kind, ext), out / rel, args, stats)
            w.writerow(row + [kind, rel])
            if n % 250 == 0:
                print(f"  {n}/{len(jobs)} ...", flush=True)
    if unpack_rows:
        with open(out / "unpack_manifest.csv", "w", newline="") as mf:
            w = csv.writer(mf)
            w.writerow(["path", "type", "members", "offset_in_parent", "size", "lz", "width", "height"])
            w.writerows(unpack_rows)

    print(f"toc:      {len(entries)} entries at VA 0x{cfg['toc_va']:x} over {span.sectors} sectors")
    if elf_assets:
        print(f"elf:      {', '.join(elf_assets)}")
    print(f"files:    {stats['files']} written, {stats['skipped']} skipped by --exclude (manifest: manifest.csv)")
    if hidden:
        print(f"hidden:   {stats['hidden']} payloads in {len(hidden)} unreferenced ranges"
              f" ({sum(s for _, s in hidden)} sectors)")
    if args.unpack:
        print(f"unpacked: {stats['members']} members (manifest: unpack_manifest.csv)")
    if args.png or args.wav:
        print(f"convert:  {stats['png']} png, {stats['wav']} wav, {len(stats['convert_errors'])} errors")
        for err in stats["convert_errors"][:20]:
            print(f"          {err}")
    print(f"output:   {_tilde(out)}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("extract", help="extract every TOC entry (and hidden range) of a game")
    e.add_argument("game", choices=sorted(GAMES))
    e.add_argument("game_dir", type=Path, help="disc file tree (SYSTEM.CNF, ELF, DATA/)")
    e.add_argument("out_dir", type=Path)
    e.add_argument("--unpack", action="store_true", help="unpack LZ payloads and nested tables into unpacked/")
    e.add_argument("--png", action="store_true", help="write a .png next to every TCB")
    e.add_argument("--wav", action="store_true", help="write a .wav next to every Svag (pure-Python ADPCM: seconds per song)")
    e.add_argument("--exclude", help="comma-separated top-level types to skip: mpeg, svag, tcb")
    e.add_argument("--ids", help="only this id or id range, e.g. 0xc91-0xd3b (skips hidden ranges)")
    f = sub.add_parser("find-toc", help="locate candidate TOCs in an ELF (for adding a GAMES entry)")
    f.add_argument("elf", type=Path)
    f.add_argument("archives", type=Path, nargs="+", help="the archive files, in disc order")
    z = sub.add_parser("lz", help="decompress one Bemani LZ stream")
    z.add_argument("src", type=Path)
    z.add_argument("dst", type=Path)
    z.add_argument("--offset", type=lambda s: int(s, 0), default=0)
    t = sub.add_parser("tcb", help="convert one TCB image to PNG")
    t.add_argument("src", type=Path)
    t.add_argument("dst", type=Path)
    s = sub.add_parser("svag", help="convert one Svag stream to WAV")
    s.add_argument("src", type=Path)
    s.add_argument("dst", type=Path)
    args = ap.parse_args()

    if args.cmd == "extract":
        cmd_extract(args)
    elif args.cmd == "find-toc":
        span = sum(p.stat().st_size for p in args.archives) // SECTOR
        for va, count, terminated, contiguous in find_toc_candidates(args.elf.read_bytes(), span)[:10]:
            print(f"VA 0x{va:08x}  entries {count:5d}  0xFFFF-terminated {'yes' if terminated else 'no '}"
                  f"  contiguous {contiguous:.0%}")
    elif args.cmd == "lz":
        data, stop, clean = lz_decode(args.src.read_bytes(), args.offset)
        args.dst.write_bytes(data)
        print(f"{len(data)} bytes from 0x{args.offset:x}..0x{stop:x}" + ("" if clean else " (no end code: truncated)"))
    elif args.cmd == "tcb":
        write_png(args.dst, *decode_tcb(args.src.read_bytes()))
    elif args.cmd == "svag":
        write_wav(args.dst, *decode_svag(args.src.read_bytes()))


if __name__ == "__main__":
    main()
