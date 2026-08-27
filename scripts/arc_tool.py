#!/usr/bin/env python3
"""
arc_tool — Unpack and repack DDR World ARC archives.

- Pack: walks the input directory recursively. The directory name itself is
  preserved as the leading path component inside the ARC. Files larger than
  1000 bytes are LZ77-compressed; smaller files are stored verbatim.
- Unpack: extracts each cue's stored path into the output directory.

ARC v1 layout:
    +0x00  u32  magic        0x19751120
    +0x04  u32  version      1
    +0x08  u32  file_count
    +0x0C  u32  flag         2
    +0x10  cue table:        file_count * 16 bytes
                u32  path_offset
                u32  data_offset
                u32  decompressed_size
                u32  compressed_size
    + ...   path strings (null-terminated, ASCII)
    + ...   file data blobs (raw or Konami-LZ77 compressed)

Konami LZ77 (Lz77 with a 4096-byte sliding window):
    - Stream of 8-bit flag bytes; each flag governs the next 8 tokens.
    - LSB-first: bit=1 → next byte is a verbatim literal; bit=0 → next two
      bytes encode a back-reference (12-bit distance, 4-bit length-3).
    - A back-reference with distance=0 marks end-of-stream.
"""

import argparse
import os
import struct
import sys
from pathlib import Path
from typing import List, Tuple


ARC_MAGIC = 0x19751120
COMPRESS_THRESHOLD = 1000


# ---------------------------------------------------------------------------
# Konami LZ77
# ---------------------------------------------------------------------------


class KonamiLz77:
    WINDOW_SIZE = 0x1000
    WINDOW_MASK = WINDOW_SIZE - 1
    MIN_MATCH = 3
    MAX_MATCH = MIN_MATCH + 15  # 18

    @classmethod
    def decompress(cls, data: bytes) -> bytes:
        """Decompress a Konami-LZ77 stream. Stops on the EOF back-reference."""
        if not data:
            return b""

        out = bytearray()
        window = bytearray(cls.WINDOW_SIZE)
        wpos = 0
        i = 0
        n = len(data)

        while i < n:
            flags = data[i]
            i += 1
            for bit in range(8):
                if i >= n:
                    return bytes(out)
                if (flags >> bit) & 1:
                    b = data[i]
                    i += 1
                    out.append(b)
                    window[wpos] = b
                    wpos = (wpos + 1) & cls.WINDOW_MASK
                else:
                    if i + 1 >= n:
                        return bytes(out)
                    w = (data[i] << 8) | data[i + 1]
                    i += 2
                    if w == 0:
                        return bytes(out)
                    distance = w >> 4
                    length = (w & 0x0F) + cls.MIN_MATCH
                    src = (wpos - distance) & cls.WINDOW_MASK
                    for _ in range(length):
                        b = window[src & cls.WINDOW_MASK]
                        out.append(b)
                        window[wpos] = b
                        wpos = (wpos + 1) & cls.WINDOW_MASK
                        src += 1
        return bytes(out)

    @classmethod
    def compress(cls, data: bytes) -> bytes:
        """
        The compressor maintains 256 per-byte linked lists of positions
        within a 4096-byte ring buffer. For each input byte it walks the
        list to locate matches up to 18 bytes long. Matches are flushed as
        back-references; non-matches are flushed as verbatim literals.
        """
        out = bytearray()
        ring = bytearray(cls.WINDOW_SIZE)
        NULL = -1
        heads = [NULL] * 256
        tails = [NULL] * 256
        links = [NULL] * cls.WINDOW_SIZE

        match_buf = bytearray(cls.MAX_MATCH)
        cursors = [0] * cls.WINDOW_SIZE

        state = {
            "ring_pos": 0,
            "ncursors": 0,
            "nmatched": 0,
            "pkt_pos": 0,
            "flags": 0x10000,
            "packet": bytearray(32),
        }

        def write_packet():
            out.append(state["flags"] & 0xFF)
            out.extend(state["packet"][: state["pkt_pos"]])
            state["pkt_pos"] = 0
            state["flags"] = 0x10000

        def push_verbatim(b: int):
            state["packet"][state["pkt_pos"]] = b & 0xFF
            state["pkt_pos"] += 1
            state["flags"] = (state["flags"] >> 1) | 0x80
            if state["flags"] & 0x100:
                write_packet()

        def emit_match():
            n = state["nmatched"]
            if n < cls.MIN_MATCH:
                # Too short to be worth a back-reference — flush as literals.
                for k in range(n):
                    push_verbatim(match_buf[k])
            else:
                # Encode (distance, length-3) as 12+4 bits across two bytes.
                state["flags"] >>= 1
                start = (state["ring_pos"] - n) & cls.WINDOW_MASK
                distance = (start - cursors[0]) & cls.WINDOW_MASK
                state["packet"][state["pkt_pos"]] = (distance >> 4) & 0xFF
                state["pkt_pos"] += 1
                state["packet"][state["pkt_pos"]] = (
                    ((distance & 0x0F) << 4) | (n - cls.MIN_MATCH)
                ) & 0xFF
                state["pkt_pos"] += 1
                if state["flags"] & 0x100:
                    write_packet()
            state["nmatched"] = 0

        def advance(new_value: int):
            old = ring[state["ring_pos"]]
            head = heads[old]
            if head != NULL:
                nxt = links[head]
                if nxt == NULL:
                    tails[old] = NULL
                heads[old] = nxt
            tail = tails[new_value]
            new_pos = state["ring_pos"]
            if tail == NULL:
                heads[new_value] = new_pos
            else:
                links[tail] = new_pos
            tails[new_value] = new_pos
            links[new_pos] = NULL
            ring[state["ring_pos"]] = new_value & 0xFF
            state["ring_pos"] = (state["ring_pos"] + 1) & cls.WINDOW_MASK

        def init_cursors(b: int):
            state["ncursors"] = 0
            cur = heads[b]
            while cur != NULL:
                if cur != (state["ring_pos"] & cls.WINDOW_MASK):
                    cursors[state["ncursors"]] = cur
                    state["ncursors"] += 1
                cur = links[cur]
            if state["ncursors"] > 0:
                match_buf[state["nmatched"]] = b & 0xFF
                state["nmatched"] += 1
            else:
                push_verbatim(b)

        def update_cursors(b: int):
            i = 0
            while i < state["ncursors"]:
                pos = (cursors[i] + state["nmatched"]) & cls.WINDOW_MASK
                if ring[pos] != (b & 0xFF):
                    if state["ncursors"] <= 1:
                        emit_match()
                        init_cursors(b)
                        return
                    state["ncursors"] -= 1
                    cursors[i] = cursors[state["ncursors"]]
                else:
                    i += 1
            match_buf[state["nmatched"]] = b & 0xFF
            state["nmatched"] += 1

        def write_byte(b: int):
            if state["nmatched"] == 0:
                init_cursors(b)
            elif state["nmatched"] == cls.MAX_MATCH:
                emit_match()
                init_cursors(b)
            else:
                update_cursors(b)
            advance(b)

        for b in data:
            write_byte(b)

        # Close: flush trailing match and emit the EOF back-reference (00 00).
        emit_match()
        state["flags"] >>= 1
        state["packet"][state["pkt_pos"]] = 0
        state["pkt_pos"] += 1
        state["packet"][state["pkt_pos"]] = 0
        state["pkt_pos"] += 1
        # Right-shift flags until the sentinel bit lands at bit 8. 
        # The sentinel was seeded at 0x10000, so this terminates after
        # at most 8 shifts.
        while (state["flags"] & 0x100) == 0:
            if state["flags"] == 0:
                raise RuntimeError("Konami LZ77 compressor lost its sentinel bit")
            state["flags"] >>= 1
        out.append(state["flags"] & 0xFF)
        out.extend(state["packet"][: state["pkt_pos"]])
        return bytes(out)


# ---------------------------------------------------------------------------
# ARC unpack
# ---------------------------------------------------------------------------


def _read_cstring(buf: bytes, offset: int) -> str:
    end = buf.find(b"\x00", offset)
    if end < 0:
        end = len(buf)
    return buf[offset:end].decode("ascii", errors="replace")


def _read_cue_table(buf: bytes) -> Tuple[int, int, int, List[Tuple[int, int, int, int]]]:
    if len(buf) < 16:
        raise ValueError("File too small to be an ARC archive")
    magic, version, count, flag = struct.unpack_from("<IIII", buf, 0)
    if magic != ARC_MAGIC:
        raise ValueError(f"Bad ARC magic: 0x{magic:08X} (expected 0x{ARC_MAGIC:08X})")
    cues = []
    for i in range(count):
        cues.append(struct.unpack_from("<IIII", buf, 16 + i * 16))
    return version, count, flag, cues


def cmd_list(arc_path: Path) -> int:
    buf = arc_path.read_bytes()
    version, count, flag, cues = _read_cue_table(buf)
    print(f"ARC v{version} ({count} files, flag={flag})")
    print(f"{'#':>4}  {'comp_size':>10}  {'decomp_size':>11}  path")
    for i, (path_off, data_off, decomp, comp) in enumerate(cues):
        name = _read_cstring(buf, path_off)
        marker = "*" if comp != decomp else " "
        print(f"{i:>4}  {comp:>10}  {decomp:>11} {marker} {name}")
    return 0


def cmd_unpack(arc_path: Path, output_dir: Path) -> int:
    buf = arc_path.read_bytes()
    version, count, flag, cues = _read_cue_table(buf)
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Unpacking {count} files from {arc_path} → {output_dir}")
    for path_off, data_off, decomp_size, comp_size in cues:
        name = _read_cstring(buf, path_off)
        raw = buf[data_off : data_off + comp_size]

        if comp_size != decomp_size:
            data = KonamiLz77.decompress(raw)
            if len(data) != decomp_size:
                print(
                    f"  warning: {name}: decompressed {len(data)} bytes, "
                    f"expected {decomp_size}",
                    file=sys.stderr,
                )
        else:
            data = raw

        out_path = output_dir / name
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(data)
        print(f"  {name} ({len(data)} bytes)")
    return 0


# ---------------------------------------------------------------------------
# ARC pack
# ---------------------------------------------------------------------------


def _walk_for_pack(root: Path) -> List[Tuple[str, Path]]:
    """
    Walk `root` recursively and yield (stored_path, real_path) pairs.

    Stored paths preserve `root`'s name as the leading component, e.g.
    packing a directory `data/` containing `arc/foo.bin` produces stored
    path `data/arc/foo.bin`. Path separators are normalised to '/'.
    """
    if not root.is_dir():
        raise ValueError(f"{root} is not a directory")

    parent = root.resolve().parent
    entries: List[Tuple[str, Path]] = []
    for dirpath, dirnames, filenames in os.walk(root):
        # Stable iteration order — arcutil uses Win32 enum order which is
        # filesystem-dependent; sorting is more reproducible here.
        dirnames.sort()
        for name in sorted(filenames):
            full = Path(dirpath) / name
            stored = full.resolve().relative_to(parent).as_posix()
            entries.append((stored, full))
    return entries


def cmd_pack(input_dir: Path, arc_path: Path) -> int:
    files = _walk_for_pack(input_dir)
    if not files:
        print(f"Error: no files found under {input_dir}", file=sys.stderr)
        return 1

    print(f"Packing {len(files)} files from {input_dir} → {arc_path}")

    blobs: List[bytes] = []
    decomp_sizes: List[int] = []
    comp_sizes: List[int] = []
    path_bytes: List[bytes] = []

    for stored, real in files:
        raw = real.read_bytes()
        decomp_sizes.append(len(raw))
        if len(raw) > COMPRESS_THRESHOLD:
            blob = KonamiLz77.compress(raw)
            tag = "compressed"
        else:
            blob = raw
            tag = "stored"
        blobs.append(blob)
        comp_sizes.append(len(blob))
        path_bytes.append(stored.encode("ascii") + b"\x00")
        print(f"  {stored} ({len(raw)} → {len(blob)} bytes, {tag})")

    n = len(files)
    arc_path.parent.mkdir(parents=True, exist_ok=True)

    with arc_path.open("wb") as f:
        f.write(struct.pack("<IIII", ARC_MAGIC, 1, n, 2))

        cue_offsets = []
        for i in range(n):
            cue_offsets.append(f.tell())
            f.write(struct.pack("<IIII", 0, 0, decomp_sizes[i], comp_sizes[i]))

        path_offsets = []
        for pb in path_bytes:
            path_offsets.append(f.tell())
            f.write(pb)

        data_offsets = []
        for blob in blobs:
            data_offsets.append(f.tell())
            f.write(blob)

        for i, cue_off in enumerate(cue_offsets):
            f.seek(cue_off)
            f.write(
                struct.pack(
                    "<IIII",
                    path_offsets[i],
                    data_offsets[i],
                    decomp_sizes[i],
                    comp_sizes[i],
                )
            )

    print(f"Wrote {arc_path} ({arc_path.stat().st_size} bytes)")
    return 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Unpack and repack DDR ARC archives.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""\
Examples:
  %(prog)s list   foo.arc
  %(prog)s unpack foo.arc -o extracted/
  %(prog)s pack   data/    -o foo.arc
""",
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_list = sub.add_parser("list", help="List files in an ARC archive")
    p_list.add_argument("arc", type=Path, help="ARC file to inspect")

    p_unpack = sub.add_parser("unpack", help="Extract all files from an ARC archive")
    p_unpack.add_argument("arc", type=Path, help="ARC file to extract")
    p_unpack.add_argument(
        "-o", "--output", type=Path, default=Path("."),
        help="Output directory (default: current directory)",
    )

    p_pack = sub.add_parser("pack", help="Pack a directory into an ARC archive")
    p_pack.add_argument("dir", type=Path, help="Directory to pack")
    p_pack.add_argument("-o", "--output", type=Path, required=True, help="Output ARC path")

    args = parser.parse_args()

    if args.cmd == "list":
        return cmd_list(args.arc)
    if args.cmd == "unpack":
        return cmd_unpack(args.arc, args.output)
    if args.cmd == "pack":
        return cmd_pack(args.dir, args.output)
    return 1


if __name__ == "__main__":
    sys.exit(main())
