#!/usr/bin/env python3
"""Extract assets from DDR Ultramix archive files.

Two archive formats are supported, both flat and sector-aligned to 0x800:

    x_data bin (textures, charts, song-info files, etc.)
      The table-of-contents is a static array embedded in default.xbe.
      Each 16-byte entry: { name_va, size, size_aligned, offset }.
      `name_va` points at a NUL-terminated Shift-JIS filename in the XBE;
      it is resolved to a file offset via the XBE section headers.

    music sng (audio streams; XBOX-IMA ADPCM, 44.1 kHz stereo, headerless)
      The table-of-contents lives in the first sector of the archive itself.
      Layout: u32 count, then `count` 20-byte entries of
      { tag[4], offset, size, loop_offset, loop_size }.
      `loop_*` describes a looping preview stream for song-wheel (zero for
      UI/non-song audio). Main streams are extracted as `{tag}.wavm`,
      loops as `{tag}_loop.wavm`.

Usage:
    extract_ultramix_data.py <game> <game_dir> <out_dir>

Example:
    extract_ultramix_data.py ultramix_us ~/Desktop/ultramix ./extracted
"""

import argparse
import csv
import struct
import sys
from pathlib import Path

# Per-game parameters, discovered by reverse-engineering default.xbe.
#   xdata_toc_offset / xdata_count: x_data bin TOC (file offset, entry count) in the XBE
#   xdata_bin:                      x_data bin filename in game dir
#   sng:                            music .sng filename in game dir
GAMES = {
    "ultramix_us": {
        "xdata_toc_offset": 0x1AD890,
        "xdata_count": 737,
        "xdata_bin": "x_data_US.bin",
        "sng": "music_US.sng",
    },
    "ultramix_uk": {
        "xdata_toc_offset": 0x1B06B0,
        "xdata_count": 737,
        "xdata_bin": "x_data_UK.bin",
        "sng": "music_UK.sng",
    },
}

XBE_SECTION_HEADER_SIZE = 56


class ArchiveError(Exception):
    """An archive or TOC doesn't match what the game config expects."""


def xbe_va_to_file_offset(xbe_bytes):
    """Return a function mapping an XBE virtual address to its offset in the file.

    Each section is loaded at a different VA-to-file delta, so the mapping is
    taken from the section headers rather than hardcoded per game.
    """
    if xbe_bytes[:4] != b"XBEH":
        raise ArchiveError("default.xbe is missing the XBEH magic")
    (base_va,) = struct.unpack_from("<I", xbe_bytes, 0x104)
    section_count, section_headers_va = struct.unpack_from("<II", xbe_bytes, 0x11C)

    sections = []
    for i in range(section_count):
        header_off = section_headers_va - base_va + i * XBE_SECTION_HEADER_SIZE
        _flags, va, _virtual_size, raw_offset, raw_size = struct.unpack_from(
            "<IIIII", xbe_bytes, header_off
        )
        sections.append((va, raw_offset, raw_size))

    def to_file_offset(va):
        for section_va, raw_offset, raw_size in sections:
            if section_va <= va < section_va + raw_size:
                return raw_offset + (va - section_va)
        raise ArchiveError(f"address 0x{va:x} is not inside any XBE section")

    return to_file_offset


def check_output_name(name, source):
    """Reject names that can't safely be used as a file in the output dir.

    A bad name almost always means the TOC location or format is wrong for this
    game, so fail before anything is written rather than emit junk files.
    """
    if not name or name in (".", "..") or any(c in name for c in "/\\:") or not name.isprintable():
        raise ArchiveError(
            f"{source}: implausible filename {name!r}; "
            "the TOC settings in GAMES probably don't match this game's files"
        )


def read_exact(f, offset, size, what):
    f.seek(offset)
    data = f.read(size)
    if len(data) != size:
        raise ArchiveError(f"{what}: expected {size} bytes at 0x{offset:x}, file ends after {len(data)}")
    return data


def parse_xdata_toc(xbe_bytes, toc_offset, count):
    """Yield (name, size, offset) for each non-empty x_data TOC entry."""
    to_file_offset = xbe_va_to_file_offset(xbe_bytes)
    for i in range(count):
        name_va, size, _size_aligned, offset = struct.unpack_from("<IIII", xbe_bytes, toc_offset + i * 16)
        if name_va == 0 and size == 0 and offset == 0:
            continue  # unused slot
        name_off = to_file_offset(name_va)
        end = xbe_bytes.find(b"\x00", name_off)
        if end == -1:
            raise ArchiveError(f"x_data TOC entry {i}: filename at 0x{name_off:x} is not NUL-terminated")
        try:
            name = xbe_bytes[name_off:end].decode("cp932")
        except UnicodeDecodeError:
            raise ArchiveError(f"x_data TOC entry {i}: filename at 0x{name_off:x} is not valid Shift-JIS") from None
        check_output_name(name, f"x_data TOC entry {i}")
        yield name, size, offset


def parse_sng_toc(sng_f):
    """Yield (tag, offset, size, loop_offset, loop_size) for each .sng entry."""
    sng_f.seek(0)
    (count,) = struct.unpack("<I", sng_f.read(4))
    toc_bytes = sng_f.read(count * 20)
    for i in range(count):
        tag, offset, size, loop_offset, loop_size = struct.unpack_from(
            "<4sIIII", toc_bytes, i * 20
        )
        try:
            tag = tag.decode("ascii")
        except UnicodeDecodeError:
            raise ArchiveError(f"sng TOC entry {i}: tag {tag!r} is not ASCII") from None
        check_output_name(tag, f"sng TOC entry {i}")
        yield tag, offset, size, loop_offset, loop_size


def extract_xdata(entries, bin_path, out_dir):
    """Extract x_data bin contents and write a manifest. Returns file count."""
    written = 0
    with open(bin_path, "rb") as bin_f, open(out_dir / "xdata_manifest.csv", "w", newline="") as mf:
        w = csv.writer(mf)
        w.writerow(["name", "size", "offset"])
        for name, size, offset in entries:
            w.writerow([name, size, f"0x{offset:x}"])
            if size == 0:
                continue
            (out_dir / name).write_bytes(read_exact(bin_f, offset, size, name))
            written += 1
    return written


def extract_sng(entries, sng_path, out_dir):
    """Extract .sng audio streams as {tag}.wavm and {tag}_loop.wavm. Returns file count."""
    written = 0
    with open(sng_path, "rb") as sng_f, open(out_dir / "sng_manifest.csv", "w", newline="") as mf:
        w = csv.writer(mf)
        w.writerow(["tag", "stream", "size", "offset"])
        for tag, offset, size, loop_offset, loop_size in entries:
            for suffix, off, sz in [("", offset, size), ("_loop", loop_offset, loop_size)]:
                if sz == 0:
                    continue
                out_name = f"{tag}{suffix}.wavm"
                (out_dir / out_name).write_bytes(read_exact(sng_f, off, sz, out_name))
                w.writerow([tag, suffix.lstrip("_") or "main", sz, f"0x{off:x}"])
                written += 1
    return written


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("game", choices=sorted(GAMES), help="game identifier")
    ap.add_argument("game_dir", type=Path, help="directory containing default.xbe, x_data bin, and music .sng")
    ap.add_argument("out_dir", type=Path, help="output directory for extracted files")
    args = ap.parse_args()

    cfg = GAMES[args.game]
    xbe_path = args.game_dir / "default.xbe"
    bin_path = args.game_dir / cfg["xdata_bin"]
    sng_path = args.game_dir / cfg["sng"]
    for p in (xbe_path, bin_path, sng_path):
        if not p.is_file():
            sys.exit(f"error: {p} not found")

    try:
        # Parse and validate both TOCs up front so a bad config writes nothing.
        xdata_entries = list(
            parse_xdata_toc(xbe_path.read_bytes(), cfg["xdata_toc_offset"], cfg["xdata_count"])
        )
        with open(sng_path, "rb") as sng_f:
            sng_entries = list(parse_sng_toc(sng_f))

        args.out_dir.mkdir(parents=True, exist_ok=True)
        xdata_count = extract_xdata(xdata_entries, bin_path, args.out_dir)
        sng_count = extract_sng(sng_entries, sng_path, args.out_dir)
    except ArchiveError as e:
        sys.exit(f"error: {e}")

    print(f"x_data:  {xdata_count} files (manifest: xdata_manifest.csv)")
    print(f"sng:     {sng_count} files (manifest: sng_manifest.csv)")
    print(f"output:  {args.out_dir}")


if __name__ == "__main__":
    main()
