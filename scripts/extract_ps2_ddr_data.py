#!/usr/bin/env python3
"""Extract assets from a PS2 DanceDanceRevolution disc.

The PS2 mixes store their assets in one of two archive layouts. In both, the file table is
a static array in the game ELF, so cutting the archive into files is exact: no heuristics.
See docs/ps2_ddr_filedata_research.md for the RE of each game.

FILEDATA layout (MAX .. STRIKE: `filedata` games). The archive is one flat,
0x800-sector-aligned address space split over several disc files: FILEDATA.BIN, then
FILEDT02.BIN, FILEDT03.BIN, ..., laid out back to back on the disc. The game holds only the
disc LBA of FILEDATA.BIN. Every other location is a sector offset from that LBA, so the
files have to be read as one concatenated span. Each TOC entry is 8 bytes, little-endian:

    u16 id | u24 sector_offset | u24 sector_count

- Entries are sorted by id. STRIKE binary-searches them and ends the table with id 0xFFFF.
  Party Collection and Festival refer to files by pointers to entries, not by id, and
  their tables have no terminator; they end where the ascending id run ends.
- The table has no names; files are known only by id.
- The game uses a raw id OR a pointer to an entry as a file handle, and it tells the two
  apart with (handle & 7) != 0. That is why no id is a multiple of 8.
- Sizes are whole sectors, so each extracted file keeps its zero padding.
- A few sector ranges are listed in no entry. They are extracted too, under hidden/, split
  into payloads where a payload describes its own size.

This layout also matches root670's ddr-tools `filedata-tool.py` for DDR MAX..Party
Collection.

DAT layout (SuperNova .. X2: `dat` games). Four named archives, SYSTEM.DAT, IMAGE.DAT,
SOUND.DAT and MDB_<mix>.DAT, each mounted with its own table from the ELF:

    u32 count, then count entries of 11 u32 (12 in X2):
    index | type | [variant, X2 only] | size | sector | char *name | byte_sum | Y | M | D | h | m

- Entry 0 is the archive's 0x20-byte header at sector 0 (12 bytes, then the same date).
- Sizes are exact bytes. `byte_sum` is the byte sum of the file mod 2**32; the extractor
  verifies it for every file.
- X2 localizes some files in threes (English, French, Spanish): variant 0x80000000 | k
  marks the k-th copy, and the game skips to the copy for the current language.

Payload formats. These are the parts ported from RhythmCodex (Source/RhythmCodex.Lib, MIT),
ddr-tools and vgmstream, or taken from the game code:

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
- TIM2 image (`decode_tim2`, the DAT games' .TM2 / .cl2): "TIM2", u8 version, u8 format
  (0: pictures at 0x10, 1: at 0x80), u16 count, then the standard picture headers
  { ..., u8 pict_format, u8 mips, u8 clut_type, u8 image_type, ... }. There is no GIF tag:
  pixels follow the header, then the CLUT. Only the first picture's base level is decoded.
- TGCD (`tgcd_decode`, the DAT games' .TM2c): a TIM2 compressed by the game's own codec
  (DecompressTgcd, 0x14DF10 in SuperNova). Header { "TGCD", out_size, in_size, 0x7fff,
  0xffff, 0xff, 0x8000, byte sum of the stream }, the stream from 0x20, in 4-byte ops:
  - u16 a with bit 15 set: a literal run of u16 n bytes, padded to 16 bytes;
  - otherwise: copy u8 n bytes from max(0, written - 0x7fff) + a, then append one literal.
- Svag audio (`decode_svag`): { "Svag", data_size, rate, channels, interleave }.
  PS-ADPCM data starts at 0x800, channel blocks are `interleave` bytes each.
- VIG audio (`decode_vig`, vgmstream's vig_kces): { u32be 0x01006408, 0, data_offset,
  data_size, loop_start, loop_length, rate, channels, flags, interleave }. PS-ADPCM
  interleaved like Svag. flags 1 (encrypted, IIDX only) is not supported.
- MPEG-1/2 video elementary streams (the song background clips); these are written as-is.
- IPU video ("ipum", PS2 intra-only MPEG-2): written as-is. Party Collection and Festival
  put a "FrameInfo" index in front ({ u32 header_size, "FrameInfo", ... }, .fipu); --unpack
  writes the bare IPU stream that follows it.
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
recursively, named by slot. The same checks apply to whole files. TGCD and FrameInfo
wrappers are exact (the game's own formats) and are always unwrapped by --unpack.
With --png, every TCB / TIM2 / TGCD image gets a .png; with --wav, every Svag / VIG a .wav.

Usage:
    extract_ps2_ddr_data.py extract <game> <game_dir> <out_dir> [--unpack] [--png] [--wav]
                            [--exclude mpeg,svag,...] [--ids 0xc91-0xd3b] [--names 'IMAGE/LOGO/*']
    extract_ps2_ddr_data.py find-toc <elf> <archive files...>  # locate the table(s) for a new game
    extract_ps2_ddr_data.py lz <in> <out> [--offset N]          # decompress one stream
    extract_ps2_ddr_data.py png <in.tcb|.tm2|.tm2c> <out.png>   # (alias: tcb)
    extract_ps2_ddr_data.py wav <in.svag|.vig> <out.wav>        # (alias: svag)

    game_dir is the disc's file tree: SYSTEM.CNF, the ELF, and DATA/.
    --ids selects FILEDATA ids; --names selects DAT files by ARCHIVE/path glob.

Games (GAMES below): strike_jp, festival_jp, party_collection_jp (FILEDATA);
supernova_jp, supernova2_jp, x_jp, x2_us (DAT).

Examples:
    extract_ps2_ddr_data.py extract strike_jp ~/Desktop/ddr_strike ./strike --unpack --png --exclude mpeg
    extract_ps2_ddr_data.py extract supernova_jp ~/Desktop/ddr_sn ./sn --unpack --png --exclude vig

Outputs:
- files/<id>.<ext> and hidden/<sector>.<ext> (FILEDATA), or
  files/<ARCHIVE>/<path> and hidden/<ARCHIVE>/<sector>.<ext> (DAT, names from the table);
- manifest.csv (table entries and hidden ranges; DAT games add the checksum result);
- unpacked/<stem>[/<slot>...].<ext> and unpack_manifest.csv;
- elf/ (per-game ELF data tables, e.g. the dancer rig).
Sidecar .png / .wav files replace the extension of tool-named files (0c91.png) and are
appended to archive-named ones (logo.TM2c.png), which can share a stem.

Import-safe: `from extract_ps2_ddr_data import lz_decode, decode_tcb, read_toc, unpack`.
Only the standard library is needed. Tests: scripts/validate_ps2_ddr_tools.sh.
"""

import argparse
import contextlib
import csv
import fnmatch
import re
import struct
import sys
import wave
import zlib
from collections import namedtuple
from pathlib import Path

SECTOR = 0x800

# Per-game parameters, found by reverse-engineering the game ELF. Addresses are Ghidra VAs
# (every ELF here has image base 0x100000).
#
# FILEDATA games (layout "filedata", the default):
#   elf:        boot executable (SYSTEM.CNF BOOT2), relative to game_dir
#   toc_va:     virtual address of the first TOC entry
#   toc_count:  number of entries
#   terminated: [default True] an id-0xFFFF entry follows the last one; if False, the table
#               ends where the ascending, in-span id run ends (checked)
#   archives:   files forming the sector span, in disc order (relative to game_dir)
#   elf_assets: [optional] {name: (va, size)} data tables to copy out of the ELF into elf/
#
# DAT games (layout "dat"):
#   elf:        boot executable
#   entry_size: 0x2C, or 0x30 with the X2 variant word
#   dats:       [(archive name, path relative to game_dir, VA of the table's u32 count)]
GAMES = {
    # DDR Party Collection (JP, 2003, SLPM_624.27, VER 1.01).
    # - InitializeFiledata (0x112B70) sets the base LBA: SetArchiveBaseLba("filedata.bin", 0xC00).
    # - Code and data use pointers to entries (e.g. the table at 0x2891C8), never ids, so the
    #   table has no terminator. The 538 data pointers into it all land in the 459 entries.
    # - ddr-tools' filedata-tool.py has the same table (file offset 0x1A1548, 459 entries).
    "party_collection_jp": {
        "elf": "SLPM_624.27",
        "toc_va": 0x2A14C8,
        "toc_count": 459,
        "terminated": False,
        "archives": ["DATA/FILEDATA.BIN"],
        # The same 573 rig as STRIKE, byte for byte: FUN_0023f530 takes the object table from
        # 0x28BED0 + type * 0x40, FUN_002424e0 the rest offsets from 0x28C0D6 (entry 1).
        # 12 of the 120 meshes are 20-object (chara20.lst) models.
        "elf_assets": {
            "chara.lst": (0x28BF10, 57),
            "chara20.lst": (0x28BED0, 41),
            "chara.pos": (0x28C0D0, 102),
        },
    },
    # DDR Festival (JP, 2004, SLPM_657.75, VER 1.01).
    # - InitializeFiledata (0x1163F0) sets the base LBA: SetArchiveBaseLba("filedata.bin", 4000).
    # - FUN_0025a240 loads the first file with FUN_001163e0(&toc[0], buf): entry pointers
    #   again, no terminator. The 1113 data pointers into the table land in its 1105 entries.
    "festival_jp": {
        "elf": "SLPM_657.75",
        "toc_va": 0x2A2098,
        "toc_count": 1105,
        "terminated": False,
        "archives": ["DATA/FILEDATA.BIN"],
        # Rig: FUN_002172a0 (object table 0x28A940 + type * 0x40), FUN_0021a590 (0x28AB46).
        "elf_assets": {
            "chara.lst": (0x28A980, 57),
            "chara20.lst": (0x28A940, 41),
            "chara.pos": (0x28AB40, 102),
        },
    },
    # DDR STRIKE (JP, 2006).
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
    # DDR SuperNova (JP, 2006, SLPM_666.09, VER 1.02). MountDatArchives (0x1C2250) mounts
    # archive k with its (count, table) pair and "<NAME>.DAT" / "<NAME>/": the pairs are the
    # tables below. Files open by name through the mount (cdrom0:\DATA\...), so no LBA is
    # hard-coded.
    "supernova_jp": {
        "layout": "dat",
        "elf": "SLPM_666.09",
        "entry_size": 0x2C,
        "dats": [
            ("SYSTEM", "DATA/SYSTEM.DAT", 0x3337A0),
            ("IMAGE", "DATA/IMAGE.DAT", 0x333A70),
            ("SOUND", "DATA/SOUND.DAT", 0x33FD90),
            ("MDB_SN1", "DATA/MDB_SN1.DAT", 0x34ACE0),
        ],
    },
    # DDR SuperNova 2 (JP, 2007, SLPM_669.30, VER 1.01). MountDatArchives: 0x189F10.
    "supernova2_jp": {
        "layout": "dat",
        "elf": "SLPM_669.30",
        "entry_size": 0x2C,
        "dats": [
            ("SYSTEM", "DATA/SYSTEM.DAT", 0x35F110),
            ("IMAGE", "DATA/IMAGE.DAT", 0x35F400),
            ("SOUND", "DATA/SOUND.DAT", 0x36C7A0),
            ("MDB_SN2", "DATA/MDB_SN2.DAT", 0x3711A0),
        ],
    },
    # DDR X (JP, 2008, SLPM_550.90, VER 1.02). MountDatArchives: 0x187320.
    "x_jp": {
        "layout": "dat",
        "elf": "SLPM_550.90",
        "entry_size": 0x2C,
        "dats": [
            ("SYSTEM", "DATA/SYSTEM.DAT", 0x2CF3B0),
            ("IMAGE", "DATA/IMAGE.DAT", 0x2CF6A0),
            ("SOUND", "DATA/SOUND.DAT", 0x2E3710),
            ("MDB_X1", "DATA/MDB_X1.DAT", 0x2EA620),
        ],
    },
    # DDR X2 (US, 2009, SLUS_219.17, VER 1.00). MountDatArchives: 0x18BF10. 0x30-byte entries:
    # the word at +8 is the language variant; OpenDatFileByIndex (0x18AF00) and its siblings
    # add (GetLanguageIndex() - k) to the index of an entry marked 0x80000000 | k.
    "x2_us": {
        "layout": "dat",
        "elf": "SLUS_219.17",
        "entry_size": 0x30,
        "dats": [
            ("SYSTEM", "DATA/SYSTEM.DAT", 0x28CB40),
            ("IMAGE", "DATA/IMAGE.DAT", 0x28CE20),
            ("SOUND", "DATA/SOUND.DAT", 0x2A2AC0),
            ("MDB_X1", "DATA/MDB_X1.DAT", 0x2AA390),
        ],
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


def _toc_continues(prev_id, entry, span_sectors):
    """True if `entry` would extend an ascending, in-span TOC run whose last id is prev_id."""
    fid, sector, sectors = entry
    return fid > prev_id and sectors != 0 and sector + sectors <= span_sectors


def read_toc(elf, toc_va, count, span_sectors, terminated=True):
    """Return [(id, sector, sectors)] and validate it against the archive span.

    terminated: the entry after the last is the id-0xFFFF terminator. Without one, the
    entry after the last must break the run (so `count` can't be too small).
    """
    segs = elf_segments(elf)
    off = va_to_offset(segs, toc_va, count * TOC_ENTRY_SIZE)
    entries = [unpack_toc_entry(elf[off + i * 8: off + i * 8 + 8]) for i in range(count)]
    prev = -1
    for entry in entries:
        if not _toc_continues(prev, entry, span_sectors):
            fid, sector, sectors = entry
            if fid <= prev:
                raise ArchiveError(f"TOC ids are not ascending at id 0x{fid:x}")
            raise ArchiveError(f"TOC id 0x{fid:x} (sectors 0x{sector:x}+0x{sectors:x}) is outside the archive span")
        prev = entry[0]
    try:
        after_off = va_to_offset(segs, toc_va + count * TOC_ENTRY_SIZE, TOC_ENTRY_SIZE)
        after = unpack_toc_entry(elf[after_off:after_off + TOC_ENTRY_SIZE])
    except ArchiveError:
        after = None  # the table ends its segment
    if terminated and (after is None or after[0] != TOC_END_ID):
        found = "the segment end" if after is None else f"id 0x{after[0]:x}"
        raise ArchiveError(f"TOC entry {count} is {found}, not the 0xFFFF terminator; wrong toc_va/toc_count?")
    if not terminated and after is not None and _toc_continues(prev, after, span_sectors):
        raise ArchiveError(f"TOC entry {count} (id 0x{after[0]:x}) continues the table; toc_count too small?")
    return entries


def find_toc_candidates(elf, span_sectors, min_entries=16):
    """Scan an ELF for 8-byte TOC-shaped runs: ascending ids, in-span, mostly contiguous.

    For bringing up a new game: prints where the table probably is. Returns
    [(va, count, terminated, contiguous_fraction, covers_span)] best first. A run that is
    0xFFFF-terminated or tiles the whole span exactly is the likely table.
    """
    found = []
    for seg_va, seg_off, fsz in elf_segments(elf):
        for phase in range(0, TOC_ENTRY_SIZE, 2):
            pos = phase
            while pos + TOC_ENTRY_SIZE <= fsz:
                start, prev_id, n, contiguous, prev_end = pos, -1, 0, 0, None
                first, total, last_end = None, 0, 0
                while pos + TOC_ENTRY_SIZE <= fsz:
                    entry = unpack_toc_entry(elf[seg_off + pos: seg_off + pos + 8])
                    if not _toc_continues(prev_id, entry, span_sectors):
                        break
                    fid, sector, sectors = entry
                    contiguous += prev_end == sector
                    first = sector if first is None else min(first, sector)
                    total, last_end = total + sectors, max(last_end, sector + sectors)
                    prev_id, prev_end, n, pos = fid, sector + sectors, n + 1, pos + 8
                if n >= min_entries:
                    terminated = pos + 8 <= fsz and unpack_toc_entry(elf[seg_off + pos: seg_off + pos + 8])[0] == TOC_END_ID
                    covers = first == 0 and last_end == span_sectors == total
                    found.append((seg_va + start, n, terminated, contiguous / max(1, n - 1), covers))
                pos = max(pos, start + TOC_ENTRY_SIZE)
    return sorted(found, key=lambda c: (-c[2], -c[4], -c[1]))


# ---------------------------------------------------------------------------
# DAT archives (SuperNova .. X2)
# ---------------------------------------------------------------------------
DatEntry = namedtuple("DatEntry", "pos index type variant size sector name byte_sum date")

DAT_HEADER_SIZE = 0x20
DAT_DATE_FIELDS = ("year", "month", "day", "hour", "minute")
DAT_ENTRY_FIELDS = {
    0x2C: ("index", "type", "size", "sector", "name_va", "byte_sum") + DAT_DATE_FIELDS,
    0x30: ("index", "type", "variant", "size", "sector", "name_va", "byte_sum") + DAT_DATE_FIELDS,
}
# Entry 0 (the archive header) of each layout, from `index`; find-toc searches for it.
DAT_TABLE_LEAD = {0x2C: struct.pack("<4I", 0, 1, DAT_HEADER_SIZE, 0),
                  0x30: struct.pack("<5I", 0, 1, 0, DAT_HEADER_SIZE, 0)}
MAX_DAT_ENTRIES = 0x10000


def dat_sectors(size):
    return (size + SECTOR - 1) // SECTOR


def _dat_name(elf, segs, va):
    """The file name at `va`, or None unless it is a printable ASCII relative path."""
    try:
        off = va_to_offset(segs, va)
    except ArchiveError:
        return None
    end = elf.find(b"\0", off, off + 0x100)
    if end < 0:
        return None
    name = elf[off:end].decode("latin-1")
    if not name.isascii() or not name.isprintable() or "\\" in name:
        return None
    return name


def _is_relative_path(name):
    return bool(name) and not name.startswith("/") and all(p not in ("", ".", "..") for p in name.split("/"))


def read_dat_table(elf, table_va, entry_size, dat_bytes, header=None):
    """Return [DatEntry] of one DAT archive's table (entry 0, the header, included).

    Validates it against the archive (dat_bytes long, starting with `header` if given):
    entry 0 is the 0x20-byte header at sector 0 and carries the header's date; the other
    entries have relative-path names, are non-empty, in bounds and don't overlap.
    """
    fields = DAT_ENTRY_FIELDS.get(entry_size)
    if fields is None:
        raise ArchiveError(f"DAT entry size 0x{entry_size:x} is not one of {', '.join(map(hex, DAT_ENTRY_FIELDS))}")
    segs = elf_segments(elf)
    count, = struct.unpack_from("<I", elf, va_to_offset(segs, table_va, 4))
    if not 1 <= count <= MAX_DAT_ENTRIES:
        raise ArchiveError(f"DAT table at 0x{table_va:x} has count {count}")
    off = va_to_offset(segs, table_va + 4, count * entry_size)
    entries = []
    for pos in range(count):
        f = dict(zip(fields, struct.unpack_from(f"<{len(fields)}I", elf, off + pos * entry_size)))
        name = _dat_name(elf, segs, f["name_va"])
        if name is None or (pos and not _is_relative_path(name)):
            raise ArchiveError(f"DAT table 0x{table_va:x} entry {pos} has no usable name")
        entries.append(DatEntry(pos, f["index"], f["type"], f.get("variant", 0), f["size"], f["sector"], name,
                                f["byte_sum"], tuple(f[k] for k in DAT_DATE_FIELDS)))
    head = entries[0]
    if (head.sector, head.size, head.name) != (0, DAT_HEADER_SIZE, ""):
        raise ArchiveError(f"DAT table 0x{table_va:x} entry 0 is not the 0x20-byte archive header")
    if header is not None and struct.unpack_from("<5I", header, 0x0C) != head.date:
        raise ArchiveError(f"DAT table 0x{table_va:x} is dated {head.date}, the archive header is not; other build?")
    prev_end = 1
    for e in sorted(entries[1:], key=lambda e: e.sector):
        if e.size == 0 or e.sector < prev_end or e.sector * SECTOR + e.size > dat_bytes:
            raise ArchiveError(f"DAT table 0x{table_va:x} entry {e.pos} ({e.name}, sector 0x{e.sector:x}"
                               f" + 0x{e.size:x} bytes) overlaps another or is outside the archive")
        prev_end = e.sector + dat_sectors(e.size)
    return entries


def find_dat_tables(elf, dats):
    """Scan an ELF for DAT tables. dats: {name: (archive bytes, archive header)}.

    Returns [(va, entry_size, count, matching archive names, first file name)]: every
    structurally valid table, with the archives it validates against (none: another disc
    or a table this tool can't bound).
    """
    found = []
    for entry_size, lead in DAT_TABLE_LEAD.items():
        for seg_va, seg_off, fsz in elf_segments(elf):
            seg = elf[seg_off:seg_off + fsz]
            pos = seg.find(lead, 4)
            while pos >= 0:
                va = seg_va + pos - 4
                if pos % 4 == 0:
                    try:
                        entries = read_dat_table(elf, va, entry_size, 1 << 40)
                    except ArchiveError:
                        entries = None
                    if entries:
                        matches = []
                        for name, (size, header) in dats.items():
                            try:
                                read_dat_table(elf, va, entry_size, size, header)
                                matches.append(name)
                            except ArchiveError:
                                pass
                        first = entries[1].name if len(entries) > 1 else ""
                        found.append((va, entry_size, len(entries), matches, first))
                pos = seg.find(lead, pos + 4)
    return sorted(found, key=lambda c: (not c[3], c[0]))


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

    def close(self):
        for fh in self._fh.values():
            fh.close()
        self._fh.clear()


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
TIM2_MAGIC = b"TIM2"
VIG_MAGIC = b"\x01\x00\x64\x08"  # u32be 0x01006408


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
    if frameinfo_stream(blob):
        return "ipu_frameinfo", "fipu"
    if blob[:4] == TIM2_MAGIC:
        return "tim2", "tm2"
    if blob[:4] == b"CLT2":
        return "tim2_clut", "cl2"
    if is_tgcd(blob):
        return "tgcd", "tm2c"
    if blob[:4] == VIG_MAGIC and len(blob) >= 0x28:
        return "vig", "vig"
    if blob[:4] in NAMED_MAGICS:
        return NAMED_MAGICS[blob[:4]]
    if _is_dancer_cmd(blob):
        return "dancer_mesh", "cmd"
    if _is_dancer_cmm(blob):
        return "dancer_motion", "cmm"
    return None


# DAT-game formats named for their file extension; recognized so that --unpack leaves them be.
NAMED_MAGICS = {
    b"\x89DTF": ("dtf", "dtf"),  # 2D layout / animation
    b"\x89DLD": ("dld", "dld"),
    b"\x3b\xf2\x94\xa0": ("tzm", "tzm"),  # 3D model pack (stages, characters)
    b"eim ": ("eim", "eim"),
}

# Leaves that --unpack still opens: they wrap an exactly-specified payload.
WRAPPERS = {"tgcd", "ipu_frameinfo"}


def leaf_extent(blob, kind):
    """Self-described byte size of a known leaf, or None."""
    if kind == "tcb" and len(blob) >= 0x14:
        return 0x10 + struct.unpack_from("<I", blob, 0x10)[0]
    if kind == "svag" and len(blob) >= 8:
        return SECTOR + struct.unpack_from("<I", blob, 4)[0]
    if kind == "ipu" and len(blob) >= 8:
        return 8 + struct.unpack_from("<I", blob, 4)[0]
    if kind == "ipu_frameinfo":
        stream = frameinfo_stream(blob)
        return stream[1] if stream else None
    if kind == "vig":
        data_offset, data_size = struct.unpack_from("<2I", blob, 8)
        return data_offset + data_size
    if kind == "tim2" and len(blob) >= 0x10:
        pos = 0x80 if blob[5] == 1 else 0x10
        for _ in range(struct.unpack_from("<H", blob, 6)[0]):
            if pos + 4 > len(blob):
                return None
            pos += struct.unpack_from("<I", blob, pos)[0]
        return pos
    return None


# ---------------------------------------------------------------------------
# TGCD (port of the SuperNova..X2 decompressor, DecompressTgcd, 0x14DF10 in SuperNova)
# ---------------------------------------------------------------------------
TGCD_MAGIC = b"TGCD"
TGCD_HEADER = 0x20
TGCD_WINDOW = 0x7FFF


def is_tgcd(blob):
    """The game's header check (CheckTgcdHeader in SuperNova): magic, sizes, four constants."""
    if len(blob) < TGCD_HEADER or blob[:4] != TGCD_MAGIC:
        return False
    out_size, in_size, *consts = struct.unpack_from("<6I", blob, 4)
    return out_size != 0 and in_size > 0x20 and consts == [0x7FFF, 0xFFFF, 0xFF, 0x8000]


def tgcd_decode(blob):
    """Decompress a TGCD stream. Raises ValueError unless it yields exactly the stated size."""
    if not is_tgcd(blob):
        raise ValueError("not a TGCD stream")
    size = struct.unpack_from("<I", blob, 4)[0]
    out, pos = bytearray(), TGCD_HEADER
    try:
        while len(out) < size:
            a, n = struct.unpack_from("<HH", blob, pos)
            if a & 0x8000:  # literal run of n bytes, padded to 16
                if pos + 4 + n > len(blob):
                    raise ValueError("truncated TGCD literal run")
                out += blob[pos + 4:pos + 4 + n]
                pos += 4 + ((n + 15) & ~15)
                continue
            n, literal = blob[pos + 2], blob[pos + 3]  # window copy, then one literal
            start = max(0, len(out) - TGCD_WINDOW) + a
            if start + n <= len(out):
                out += out[start:start + n]
            else:
                for k in range(n):
                    out.append(out[start + k])
            out.append(literal)
            pos += 4
    except (struct.error, IndexError) as e:
        raise ValueError(f"corrupt TGCD stream at 0x{pos:x}") from e
    if len(out) != size:
        raise ValueError(f"TGCD stream decodes to 0x{len(out):x} bytes, header says 0x{size:x}")
    return bytes(out)


# ---------------------------------------------------------------------------
# IPU behind a FrameInfo index (Party Collection, Festival)
# ---------------------------------------------------------------------------
FRAMEINFO_TAG = b"FrameInfo\x00\x00\x00"


def frameinfo_stream(blob):
    """(start, end) of the IPU stream behind a { u32 header_size, "FrameInfo", ... } index."""
    if len(blob) < 0x20 or blob[4:16] != FRAMEINFO_TAG:
        return None
    start = struct.unpack_from("<I", blob)[0]
    if not 0x10 <= start <= len(blob) - 8 or blob[start:start + 4] != b"ipum":
        return None
    end = start + 8 + struct.unpack_from("<I", blob, start + 4)[0]
    return (start, end) if end <= len(blob) else None


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
    """An unpacked payload: a leaf (type, data) or a container (kind, children).

    `wrapper` names what the data was unwrapped from: "" (raw), "lz", "tgcd" or "frameinfo".
    """

    def __init__(self, data, kind, ext, wrapper="", offset=0, children=None, index=0):
        self.data, self.kind, self.ext, self.wrapper = data, kind, ext, wrapper
        self.offset, self.children, self.index = offset, children or [], index

    @property
    def compressed(self):
        return bool(self.wrapper)


def _container(blob, kind, layout, depth, wrapper, offset, max_depth):
    children, recognized = [], 0
    for slot, start, extent in layout:
        child, ok = identify_member(blob, start, extent, depth + 1, max_depth)
        child.index = slot
        children.append(child)
        recognized += ok
    if not recognized:
        return None
    return Node(blob, kind, None, wrapper, offset, children)


def _unwrap(blob, wrapper, offset):
    """(data, wrapper, offset) of an exactly-specified wrapper's payload, or None."""
    if wrapper != "tgcd" and is_tgcd(blob):
        try:
            return tgcd_decode(blob), "tgcd", offset
        except ValueError:
            return None
    stream = frameinfo_stream(blob) if wrapper != "frameinfo" else None
    if stream:
        return blob[stream[0]:stream[1]], "frameinfo", offset + stream[0]
    return None


def identify(blob, depth, wrapper="", offset=0, max_depth=4):
    """Classify a payload. Returns (Node, recognized) where `recognized` means the
    payload is a known leaf, a clean LZ stream, a wrapper or a validated table."""
    inner = _unwrap(blob, wrapper, offset)
    if inner:
        node, _ = identify(inner[0], depth, inner[1], inner[2], max_depth)
        return node, True
    known = sniff(blob)
    if known:
        return Node(blob, known[0], known[1], wrapper, offset), True
    if depth < max_depth:
        for kind, parse in CONTAINER_PARSERS:
            layout = parse(blob)
            node = _container(blob, kind, layout, depth, wrapper, offset, max_depth) if layout else None
            if node:
                return node, True
        if not wrapper:
            data = lz_member(blob, 0, len(blob))
            if data is not None:
                node, _ = identify(data, depth, "lz", offset, max_depth)
                return node, True
            layout = parse_lz_sequence(blob)
            node = _container(blob, "lzseq", layout, depth, wrapper, offset, max_depth) if layout else None
            if node and node.children[0].kind != "unknown":  # the first frame must be a known type
                return node, True
    return Node(blob, "unknown", "bin", wrapper, offset), False


def identify_member(blob, start, extent, depth, max_depth):
    """A container member: raw, LZ, or LZ behind RhythmCodex's 12-byte TCB-table header."""
    raw = blob[start:extent]
    if sniff(raw) is None:
        data = lz_member(blob, start, extent)
        if data is not None:
            node, _ = identify(data, depth, "lz", start, max_depth)
            return node, True
        if extent - start > 0xC:
            data = lz_member(blob, start + 0xC, extent)
            if data is not None and sniff(data):
                node, _ = identify(data, depth, "lz", start, max_depth)
                node.kind += "+hdr12"
                return node, True
    node, ok = identify(raw, depth, "", start, max_depth)
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
# TCB / TIM2 -> RGBA (ports of RhythmCodex TcbImageDecoder / ddr-tools tcb-convert.c)
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


def _picture_rgba(buf, pixels_at, clut_at, clut_bytes, clut_colors, clut_type, image_type, width, height):
    """RGBA of one PS2 picture: indexed (4 = 4bpp, 5 = 8bpp) or direct (1/2/3 = 16/24/32 bit).

    clut_type: bits 0-5 the CLUT entry format (1 = 16-bit, else 32-bit); bit 7 clear means a
    256-colour CLUT is CSM1-swizzled.
    """
    count = width * height
    if image_type in (4, 5):
        step = 2 if clut_type & 0x3F == 1 else 4
        n = min(clut_colors or (16 if image_type == 4 else 256), clut_bytes // step)
        raw = struct.unpack_from(f"<{n}{'H' if step == 2 else 'I'}", buf, clut_at)
        if n == 256 and not clut_type & 0x80:
            raw = _csm1_unswizzle(raw)
        palette = [(_rgba16 if step == 2 else _rgba32)(c) for c in raw]
        palette += [b"\x00\x00\x00\x00"] * (256 - len(palette))
        src = buf[pixels_at:pixels_at + ((count + 1) // 2 if image_type == 4 else count)]
        if image_type == 5:
            return b"".join(palette[i] for i in src)
        out = bytearray()
        for byte in src:
            out += palette[byte & 0xF] + palette[byte >> 4]
        return bytes(out[:count * 4])
    if image_type == 3:
        words = struct.unpack_from(f"<{count}I", buf, pixels_at)
        return b"".join(_rgba32(c) for c in words)
    if image_type == 2:
        src = buf[pixels_at:pixels_at + count * 3]
        return b"".join(src[i:i + 3] + b"\xff" for i in range(0, len(src), 3))
    if image_type == 1:
        words = struct.unpack_from(f"<{count}H", buf, pixels_at)
        return b"".join(_rgba16(c) for c in words)
    raise ValueError(f"unsupported image type {image_type}")


def decode_tcb(tcb):
    """Return (width, height, rgba bytes) for the base level of a TCB image.

    TCB orders the picture header's type bytes clut_type, mips, image_type (TIM2: format,
    mips, clut_type, image_type) and puts a 16-byte GIF tag before the pixels and the CLUT.
    """
    if tcb[:16] != TCB_MAGIC or len(tcb) < 0x40:
        raise ValueError("not a TCB image")
    (_total, clut_size, image_size, header_size, clut_colors, clut_type, _mips, image_type, _bpp,
     width, height) = struct.unpack_from("<3I2H4B2H", tcb, 0x10)
    pixels_at = 0x10 + header_size + 0x10
    clut_at = 0x10 + header_size + image_size + 0x10
    rgba = _picture_rgba(tcb, pixels_at, clut_at, clut_size - 0x10, clut_colors, clut_type, image_type, width, height)
    return width, height, rgba


def decode_tim2(tim2, index=0):
    """Return (width, height, rgba bytes) for the base level of picture `index` of a TIM2."""
    if tim2[:4] != TIM2_MAGIC or len(tim2) < 0x10:
        raise ValueError("not a TIM2 image")
    _version, fmt, count = struct.unpack_from("<BBH", tim2, 4)
    if index >= count:
        raise ValueError(f"TIM2 has {count} pictures, not {index + 1}")
    pos = 0x80 if fmt == 1 else 0x10
    for _ in range(index):
        pos += struct.unpack_from("<I", tim2, pos)[0]
    (_total, clut_size, image_size, header_size, clut_colors, _pict_format, _mips, clut_type, image_type,
     width, height) = struct.unpack_from("<3I2H4B2H", tim2, pos)
    if not image_type or not width * height:
        raise ValueError("TIM2 picture has no image (a CLUT-only file)")
    pixels_at = pos + header_size
    rgba = _picture_rgba(tim2, pixels_at, pixels_at + image_size, clut_size, clut_colors, clut_type, image_type,
                         width, height)
    return width, height, rgba


def decode_image(blob):
    """(width, height, rgba) of a TCB, TIM2 or TGCD-compressed TIM2."""
    if blob[:16] == TCB_MAGIC:
        return decode_tcb(blob)
    if is_tgcd(blob):
        blob = tgcd_decode(blob)
    return decode_tim2(blob)


def write_png(path, width, height, rgba):
    rows = b"".join(b"\x00" + rgba[y * width * 4:(y + 1) * width * 4] for y in range(height))

    def chunk(tag, body):
        return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body))

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(rows, 6)) + chunk(b"IEND", b""))
    Path(path).write_bytes(png)


# ---------------------------------------------------------------------------
# Svag / VIG -> PCM (PS-ADPCM; containers per RhythmCodex SvagHeuristic, vgmstream vig_kces)
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


def _ps_adpcm_pcm(body, channels, interleave):
    """Interleaved int16 PCM bytes of PS-ADPCM `body` in `interleave`-byte channel blocks."""
    if channels > 1:
        if interleave <= 0:
            raise ValueError(f"{channels}-channel stream without an interleave")
        block = interleave * channels
        streams = [b"".join(body[b + c * interleave:b + (c + 1) * interleave] for b in range(0, len(body), block))
                   for c in range(channels)]
    else:
        streams = [body]
    pcm = [_adpcm_channel(s) for s in streams]
    frames = min(len(p) for p in pcm)
    interleaved = [pcm[c][i] for i in range(frames) for c in range(channels)]
    return struct.pack(f"<{len(interleaved)}h", *interleaved)


def decode_svag(svag):
    """Return (rate, channels, interleaved int16 samples as bytes)."""
    magic, size, rate, channels, interleave = struct.unpack_from("<4s4I", svag, 0)
    if magic != b"Svag" or not 1 <= channels <= 8:
        raise ValueError("not an Svag stream")
    return rate, channels, _ps_adpcm_pcm(svag[SECTOR:SECTOR + size], channels, interleave)


def decode_vig(vig):
    """Return (rate, channels, interleaved int16 samples as bytes). Loop points are ignored."""
    if vig[:4] != VIG_MAGIC or len(vig) < 0x28:
        raise ValueError("not a VIG stream")
    data_offset, data_size, _loop_start, _loop_length, rate, channels, flags, interleave = \
        struct.unpack_from("<8I", vig, 8)
    if not 1 <= channels <= 8:
        raise ValueError(f"VIG stream with {channels} channels")
    if flags == 1:
        raise ValueError("encrypted VIG (flags 1) is not supported")
    return rate, channels, _ps_adpcm_pcm(vig[data_offset:data_offset + data_size], channels, interleave)


def decode_audio(blob):
    """(rate, channels, pcm) of an Svag or VIG stream."""
    return decode_vig(blob) if blob[:4] == VIG_MAGIC else decode_svag(blob)


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


IMAGE_KINDS = {"tcb", "tim2", "tgcd"}
AUDIO_KINDS = {"svag", "vig"}


def _sidecar(path, ext, keep_name):
    """Where a .png / .wav for `path` goes: next to it, replacing or keeping its extension."""
    return path.with_name(path.name + ext) if keep_name else path.with_suffix(ext)


def convert_leaf(node, path, opts, stats, keep_name=False):
    try:
        if opts.png and node.kind in IMAGE_KINDS:
            write_png(_sidecar(path, ".png", keep_name), *decode_image(node.data))
            stats["png"] += 1
        elif opts.wav and node.kind in AUDIO_KINDS:
            write_wav(_sidecar(path, ".wav", keep_name), *decode_audio(node.data))
            stats["wav"] += 1
    except (ValueError, struct.error) as e:
        stats["convert_errors"].append(f"{_tilde(path)}: {e}")


def image_dims(node):
    data = node.data
    if node.kind == "tcb" and len(data) >= 0x28:
        return struct.unpack_from("<HH", data, 0x24)
    if node.kind == "tim2" and len(data) >= 0x10:
        pos = (0x80 if data[5] == 1 else 0x10) + 0x14
        if pos + 4 <= len(data):
            return struct.unpack_from("<HH", data, pos)
    return "", ""


def write_tree(node, path, rel, rows, opts, stats):
    """Write a Node tree under `path` (a stem without extension); record manifest rows."""
    if node.children:
        path.mkdir(parents=True, exist_ok=True)
        for child in node.children:
            name = f"{child.index:03d}"
            write_tree(child, path / name, f"{rel}/{name}", rows, opts, stats)
        rows.append([rel + "/", node.kind, len(node.children), f"0x{node.offset:x}", len(node.data),
                     node.wrapper, "", ""])
        return
    out = path.with_name(f"{path.name}.{node.ext}")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(node.data)
    stats["members"] += 1
    rows.append([f"{rel}.{node.ext}", node.kind, "", f"0x{node.offset:x}", len(node.data), node.wrapper,
                 *image_dims(node)])
    convert_leaf(node, out, opts, stats)


# --exclude groups -> the sniffed types they cover
EXCLUDABLE = {
    "mpeg": {"mpeg1", "mpeg2"},
    "ipu": {"ipu", "ipu_frameinfo"},
    "svag": {"svag"},
    "vig": {"vig"},
    "tcb": {"tcb"},
    "tim2": {"tim2", "tgcd"},
}

# One output file. row: the manifest columns before type/file. read(): the payload bytes.
# stem: key under unpacked/. name: the archive's own relative path (DAT), or None to name
# the file <stem>.<sniffed ext>. checksum: the expected byte sum, or None.
Job = namedtuple("Job", "row read stem name subdir checksum")


class DatReader:
    """Byte-range reads from one archive file."""

    def __init__(self, path):
        self.path, self._fh = path, None

    def read(self, offset, size):
        self._fh = self._fh or open(self.path, "rb")
        self._fh.seek(offset)
        return self._fh.read(size)

    def close(self):
        if self._fh:
            self._fh.close()
            self._fh = None


def _hidden_jobs(read_range, ranges, stem_prefix, row_prefix):
    """Jobs for the payloads of unreferenced sector ranges (see split_hidden)."""
    jobs = []
    for gap_sector, gap_sectors in ranges:
        for off, size in split_hidden(read_range(gap_sector, gap_sectors)):
            sector = gap_sector + off // SECTOR
            jobs.append(Job(row_prefix(sector, size // SECTOR),
                            lambda s=sector, n=size // SECTOR: read_range(s, n),
                            f"{stem_prefix}{sector:06x}", None, "hidden", None))
    return jobs


def plan_filedata(cfg, args, elf, stack):
    """(jobs, manifest columns, summary lines) for a FILEDATA game; readers close with `stack`."""
    if args.names:
        raise ArchiveError("--names selects DAT files; this game has a FILEDATA archive (use --ids)")
    id_range = parse_ids(args.ids) if args.ids else None
    span = Span([args.game_dir / p for p in cfg["archives"]])
    stack.callback(span.close)
    entries = read_toc(elf, cfg["toc_va"], cfg["toc_count"], span.sectors, cfg.get("terminated", True))
    hidden = [] if id_range else gaps(entries, span.sectors)
    for sector, sectors in [(s, n) for _fid, s, n in entries] + hidden:
        span.locate(sector, sectors)

    def row(label, sector, sectors):
        archive, archive_off = span.locate(sector, sectors)
        return [label, f"0x{sector:x}", sectors, archive.name, f"0x{archive_off:x}"]

    jobs = [Job(row(f"{fid:04x}", sector, sectors), lambda s=sector, n=sectors: span.read(s, n),
                f"{fid:04x}", None, "files", None)
            for fid, sector, sectors in entries if not id_range or id_range[0] <= fid <= id_range[1]]
    jobs += _hidden_jobs(span.read, hidden, "", lambda s, n: row("hidden", s, n))
    summary = [f"toc:      {len(entries)} entries at VA 0x{cfg['toc_va']:x} over {span.sectors} sectors"]
    if hidden:
        summary.append(f"hidden:   {len(hidden)} unreferenced ranges ({sum(n for _, n in hidden)} sectors)")
    return jobs, ["id", "sector", "sectors", "archive", "archive_offset"], summary


def _unique_name(name, seen, pos):
    """`name`, or name~<pos> if a case-insensitive file system would already have it."""
    if name.lower() in seen:
        stem, dot, ext = name.rpartition(".")
        name = f"{stem}~{pos}.{ext}" if dot and "/" not in ext else f"{name}~{pos}"
    seen.add(name.lower())
    return name


def plan_dat(cfg, args, elf, stack):
    """(jobs, manifest columns, summary lines) for a DAT game; readers close with `stack`."""
    if args.ids:
        raise ArchiveError("--ids selects FILEDATA ids; this game has DAT archives (use --names)")
    patterns = [p.strip().lower() for p in args.names.split(",") if p.strip()] if args.names else None
    jobs, summary = [], []
    for archive, rel_path, table_va in cfg["dats"]:
        reader = DatReader(args.game_dir / rel_path)
        stack.callback(reader.close)
        size = reader.path.stat().st_size
        entries = read_dat_table(elf, table_va, cfg["entry_size"], size, reader.read(0, DAT_HEADER_SIZE))
        seen = set()
        for e in entries[1:]:
            full = f"{archive}/{e.name}"
            name = _unique_name(full, seen, e.pos)
            if patterns and not any(fnmatch.fnmatchcase(full.lower(), p) for p in patterns):
                continue
            row = [archive, e.pos, e.index, e.type, f"0x{e.variant:x}" if e.variant else "", f"0x{e.sector:x}",
                   e.size, "%04d-%02d-%02d %02d:%02d" % e.date]
            jobs.append(Job(row, lambda o=e.sector * SECTOR, n=e.size, r=reader: r.read(o, n),
                            name, name, "files", e.byte_sum))
        hidden = [] if patterns else gaps([(e.pos, e.sector, dat_sectors(e.size)) for e in entries], dat_sectors(size))
        jobs += _hidden_jobs(lambda s, n, r=reader: r.read(s * SECTOR, n * SECTOR), hidden, f"{archive}/",
                             lambda s, n, a=archive: [a, "hidden", "", "", "", f"0x{s:x}", n * SECTOR, ""])
        line = (f"{archive + ':':9} {len(entries) - 1:5d} files, table at VA 0x{table_va:x},"
                f" built {'%04d-%02d-%02d %02d:%02d' % entries[0].date}")
        if hidden:
            line += f", {len(hidden)} unreferenced ranges ({sum(n for _, n in hidden)} sectors)"
        summary.append(line)
    return jobs, ["archive", "pos", "index", "type_code", "variant", "sector", "size", "date", "checksum"], summary


def cmd_extract(args):
    cfg = GAMES[args.game]
    exclude = {t.strip() for t in args.exclude.split(",") if t.strip()} if args.exclude else set()
    if exclude - EXCLUDABLE.keys():
        sys.exit(f"error: --exclude takes {', '.join(sorted(EXCLUDABLE))}, not {', '.join(sorted(exclude - EXCLUDABLE.keys()))}")
    excluded_kinds = set().union(*(EXCLUDABLE[t] for t in exclude))
    elf_path = args.game_dir / cfg["elf"]
    archives = [p for _a, p, _va in cfg["dats"]] if cfg.get("layout") == "dat" else cfg["archives"]
    for p in (elf_path, *(args.game_dir / a for a in archives)):
        if not p.is_file():
            sys.exit(f"error: {_tilde(p)} not found")

    with contextlib.ExitStack() as stack:
        try:
            elf = elf_path.read_bytes()
            plan = plan_dat if cfg.get("layout") == "dat" else plan_filedata
            jobs, columns, summary = plan(cfg, args, elf, stack)
            segs = elf_segments(elf)
            elf_assets = {name: elf[va_to_offset(segs, va, size):va_to_offset(segs, va, size) + size]
                          for name, (va, size) in cfg.get("elf_assets", {}).items()}
        except ArchiveError as e:
            sys.exit(f"error: {e}")
        out = args.out_dir
        (out / "files").mkdir(parents=True, exist_ok=True)
        if elf_assets:
            (out / "elf").mkdir(exist_ok=True)
            for name, data in elf_assets.items():
                (out / "elf" / name).write_bytes(data)
        stats = write_jobs(jobs, columns, args, excluded_kinds)

    for line in summary:
        print(line)
    if elf_assets:
        print(f"elf:      {', '.join(elf_assets)}")
    print(f"files:    {stats['files']} written, {stats['skipped']} skipped by --exclude (manifest: manifest.csv)")
    if stats["hidden"]:
        print(f"hidden:   {stats['hidden']} payloads written")
    if stats["checked"]:
        print(f"checksum: {stats['checked']} files verified, {len(stats['bad_checksums'])} mismatches")
        for name in stats["bad_checksums"][:20]:
            print(f"          BAD {name}")
    if args.unpack:
        print(f"unpacked: {stats['members']} members (manifest: unpack_manifest.csv)")
    if args.png or args.wav:
        print(f"convert:  {stats['png']} png, {stats['wav']} wav, {len(stats['convert_errors'])} errors")
        for err in stats["convert_errors"][:20]:
            print(f"          {err}")
    print(f"output:   {_tilde(out)}")
    if stats["bad_checksums"]:
        sys.exit("error: some files don't match their table checksum; the disc dump may be damaged")


def write_jobs(jobs, columns, args, excluded_kinds):
    """Write every job's payload (and its unpacked tree / conversions) and the manifests."""
    out = args.out_dir
    stats = {"files": 0, "hidden": 0, "members": 0, "png": 0, "wav": 0, "skipped": 0, "convert_errors": [],
             "checked": 0, "bad_checksums": []}
    unpack_rows = []
    with open(out / "manifest.csv", "w", newline="") as mf:
        w = csv.writer(mf)
        w.writerow(columns + ["type", "file"])
        for n, job in enumerate(jobs, 1):
            blob = job.read()
            row = list(job.row)
            if job.checksum is not None:
                ok = sum(blob) & 0xFFFFFFFF == job.checksum
                stats["checked"] += 1
                if not ok:
                    stats["bad_checksums"].append(job.name)
                row.append("ok" if ok else "BAD")
            elif "checksum" in columns:
                row.append("")
            known = sniff(blob)
            kind, ext = known or ("unknown", "bin")
            if kind in excluded_kinds:
                stats["skipped"] += 1
                w.writerow(row + [kind, ""])
                continue
            rel = f"{job.subdir}/{job.name}" if job.name else f"{job.subdir}/{job.stem}.{ext}"
            (out / rel).parent.mkdir(parents=True, exist_ok=True)
            (out / rel).write_bytes(blob)
            stats[job.subdir] += 1
            tree = unpack(blob) if args.unpack and (known is None or kind in WRAPPERS) else None
            if tree is not None and (tree.children or tree.wrapper):
                kind = tree.kind + (f"+{tree.wrapper}" if tree.wrapper else "")
                write_tree(tree, out / "unpacked" / job.stem, job.stem, unpack_rows, args, stats)
            elif known:
                convert_leaf(Node(blob, kind, ext), out / rel, args, stats, keep_name=bool(job.name))
            w.writerow(row + [kind, rel])
            if n % 250 == 0:
                print(f"  {n}/{len(jobs)} ...", flush=True)
    if unpack_rows:
        with open(out / "unpack_manifest.csv", "w", newline="") as mf:
            w = csv.writer(mf)
            w.writerow(["path", "type", "members", "offset_in_parent", "size", "wrapper", "width", "height"])
            w.writerows(unpack_rows)
    return stats


def cmd_find_toc(args):
    """Print the likely table(s): DAT tables if the archives are .DAT files, else TOC runs."""
    elf = args.elf.read_bytes()
    if any(p.suffix.lower() == ".dat" for p in args.archives):
        dats = {}
        for p in args.archives:
            with open(p, "rb") as fh:
                dats[p.stem.upper()] = (p.stat().st_size, fh.read(DAT_HEADER_SIZE))
        for va, entry_size, count, matches, first in find_dat_tables(elf, dats)[:20]:
            print(f"VA 0x{va:08x}  entry 0x{entry_size:x}  files {count - 1:5d}"
                  f"  archive {', '.join(matches) or '-':10}  first {first}")
        return
    span = sum(p.stat().st_size for p in args.archives) // SECTOR
    for va, count, terminated, contiguous, covers in find_toc_candidates(elf, span)[:10]:
        print(f"VA 0x{va:08x}  entries {count:5d}  0xFFFF-terminated {'yes' if terminated else 'no '}"
              f"  contiguous {contiguous:4.0%}  covers span {'yes' if covers else 'no'}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("extract", help="extract every table entry (and hidden range) of a game")
    e.add_argument("game", choices=sorted(GAMES))
    e.add_argument("game_dir", type=Path, help="disc file tree (SYSTEM.CNF, ELF, DATA/)")
    e.add_argument("out_dir", type=Path)
    e.add_argument("--unpack", action="store_true",
                   help="unpack LZ / TGCD / FrameInfo payloads and nested tables into unpacked/")
    e.add_argument("--png", action="store_true", help="write a .png next to every TCB / TIM2 / TGCD image")
    e.add_argument("--wav", action="store_true",
                   help="write a .wav next to every Svag / VIG (pure-Python ADPCM: seconds per song)")
    e.add_argument("--exclude", help=f"comma-separated types to skip: {', '.join(sorted(EXCLUDABLE))}")
    e.add_argument("--ids", help="FILEDATA games: only this id or id range, e.g. 0xc91-0xd3b (skips hidden ranges)")
    e.add_argument("--names", help="DAT games: only ARCHIVE/path matching these comma-separated globs,"
                                   " case-insensitive, e.g. 'IMAGE/LOGO/*' (skips hidden ranges)")
    f = sub.add_parser("find-toc", help="locate the file table(s) in an ELF (for adding a GAMES entry)")
    f.add_argument("elf", type=Path)
    f.add_argument("archives", type=Path, nargs="+",
                   help="FILEDATA archive files in disc order, or the .DAT archives")
    z = sub.add_parser("lz", help="decompress one Bemani LZ stream")
    z.add_argument("src", type=Path)
    z.add_argument("dst", type=Path)
    z.add_argument("--offset", type=lambda s: int(s, 0), default=0)
    t = sub.add_parser("png", aliases=["tcb"], help="convert one TCB, TIM2 or TGCD image to PNG")
    t.add_argument("src", type=Path)
    t.add_argument("dst", type=Path)
    s = sub.add_parser("wav", aliases=["svag"], help="convert one Svag or VIG stream to WAV")
    s.add_argument("src", type=Path)
    s.add_argument("dst", type=Path)
    args = ap.parse_args()

    if args.cmd == "extract":
        cmd_extract(args)
    elif args.cmd == "find-toc":
        cmd_find_toc(args)
    elif args.cmd == "lz":
        data, stop, clean = lz_decode(args.src.read_bytes(), args.offset)
        args.dst.write_bytes(data)
        print(f"{len(data)} bytes from 0x{args.offset:x}..0x{stop:x}" + ("" if clean else " (no end code: truncated)"))
    elif args.cmd in ("png", "tcb"):
        write_png(args.dst, *decode_image(args.src.read_bytes()))
    elif args.cmd in ("wav", "svag"):
        write_wav(args.dst, *decode_audio(args.src.read_bytes()))


if __name__ == "__main__":
    main()
