#!/usr/bin/env python3
"""List and extract files from an Xbox / Xbox 360 disc image (XDVDFS, "XISO").

Handles plain XDVDFS images (an `extract-xiso` rebuild, game partition at 0) and full
redump-style dumps, whose game partition sits at a fixed per-layout offset:

    XGD1 (Xbox)          0x18300000
    XGD2 (Xbox 360)      0x0FD90000
    XGD3 (Xbox 360)      0x02080000

The volume descriptor is sector 32 (0x10000) of the partition: the 20-byte magic
`MICROSOFT*XBOX*MEDIA`, then u32 root-directory sector and u32 root-directory size. A
directory is a sector-aligned buffer of entries forming a binary search tree:

    u16 left, u16 right   child entry offsets in 4-byte units (0 = none)
    u32 sector, u32 size  file data (sectors are 0x800 bytes, partition-relative)
    u8  attributes        0x10 = directory
    u8  name length, then the name (no terminator); entries are 4-byte aligned
    0xFFFF padding fills the rest of a sector

Usage:
    xdvdfs_extract.py list    <image.iso> [glob ...]
    xdvdfs_extract.py extract <image.iso> <out_dir> [glob ...]

Globs match the image path without the leading `/` (fnmatch, case-insensitive), e.g.
`'x_data_*' 'resource_*' default.xex`. With no glob every file is taken.
"""
import fnmatch
import struct
import sys
from pathlib import Path

SECTOR = 0x800
VOLUME_SECTOR = 32
MAGIC = b"MICROSOFT*XBOX*MEDIA"
PARTITION_OFFSETS = (0, 0x0FD90000, 0x18300000, 0x02080000)
ATTR_DIRECTORY = 0x10


class Image:
    def __init__(self, path):
        self.f = open(path, "rb")
        for base in PARTITION_OFFSETS:
            self.f.seek(base + VOLUME_SECTOR * SECTOR)
            head = self.f.read(28)
            if head[:20] == MAGIC:
                self.base = base
                self.root_sector, self.root_size = struct.unpack_from("<II", head, 20)
                return
        sys.exit(f"{path}: no XDVDFS volume descriptor at any known partition offset")

    def read(self, sector, size):
        self.f.seek(self.base + sector * SECTOR)
        data = self.f.read(size)
        if len(data) != size:
            sys.exit(f"read past the end of the image (sector {sector}, {size} bytes)")
        return data

    def walk(self, sector=None, size=None, prefix=""):
        """Yield (path, sector, size, is_dir) for every entry, parents before children."""
        if sector is None:
            sector, size = self.root_sector, self.root_size
        if size == 0:
            return
        table = self.read(sector, size)
        entries, pending, seen = [], [0], set()
        while pending:
            off = pending.pop()
            if off in seen or off + 14 > len(table):
                continue
            seen.add(off)
            left, right, start, length, attr, name_len = struct.unpack_from("<HHIIBB", table, off)
            if left == 0xFFFF and right == 0xFFFF:
                continue  # sector padding
            name = table[off + 14 : off + 14 + name_len].decode("latin1")
            if not name or name in (".", "..") or "/" in name or "\\" in name:
                sys.exit(f"implausible entry name {name!r} in directory at sector {sector}")
            entries.append((name, start, length, bool(attr & ATTR_DIRECTORY)))
            pending.extend(4 * child for child in (left, right) if child)
        for name, start, length, is_dir in sorted(entries, key=lambda e: e[0].lower()):
            path = prefix + name
            yield path, start, length, is_dir
            if is_dir:
                yield from self.walk(start, length, path + "/")


def selected(path, globs):
    return not globs or any(fnmatch.fnmatch(path.lower(), g.lower()) for g in globs)


def main():
    if len(sys.argv) < 3 or sys.argv[1] not in ("list", "extract"):
        sys.exit(__doc__)
    image = Image(sys.argv[2])
    if sys.argv[1] == "list":
        globs = sys.argv[3:]
        total = 0
        for path, sector, size, is_dir in image.walk():
            if not is_dir and selected(path, globs):
                print(f"{size:12d}  {path}")
                total += size
        print(f"{total / 1e6:.1f} MB (partition at 0x{image.base:x})", file=sys.stderr)
        return
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    out_dir, globs = Path(sys.argv[3]), sys.argv[4:]
    count = total = 0
    for path, sector, size, is_dir in image.walk():
        if is_dir or not selected(path, globs):
            continue
        dest = out_dir / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        with open(dest, "wb") as out:
            remaining, at = size, sector
            while remaining:
                chunk = min(remaining, 64 * SECTOR * 128)
                out.write(image.read(at, chunk))
                at += chunk // SECTOR
                remaining -= chunk
        count += 1
        total += size
        print(f"\r{count} files, {total / 1e6:.1f} MB", end="", flush=True)
    print(f"\nDone -> {out_dir}")


if __name__ == "__main__":
    main()
