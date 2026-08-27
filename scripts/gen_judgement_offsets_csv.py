#!/usr/bin/env python3
"""
gen_judgement_offsets_csv — one-time pre-seed generator for the Per-Song
Judgement Offsets mod's `judgement_offsets.csv`.

Cross-references a community-maintained sync-offset list (keyed by each
song's numeric mcode, one `mcode<ws>offset` pair per line) against the game's
musicdb (`data/arc/startup.arc` entry `data/gamedata/musicdb.xml`, which
carries both `<mcode>` and the alphabetical `<basename>` the mod keys on),
and emits the runtime CSV:

    code,p1_offset,p2_offset

One row per musicdb entry, in musicdb order. Songs with a known offset get
the same value seeded for BOTH players; the rest get blank cells (= no
override, the player's stock JUDGEMENT OFFSET applies).

Sign convention: the community list records how far OFF each song's sync is;
the player compensates by adjusting in the OPPOSITE direction. The mod's CSV
stores the compensating JUDGEMENT OFFSET directly, so every incoming value is
NEGATED during conversion.

Input tolerance (matches the known real-world file):
  * CRLF / stray whitespace.
  * A line with three or more fields uses the FIRST value (warned).
  * Unparseable lines are warned and skipped.
  * Values (post-negation) outside -100..+100 are clamped (warned).
  * mcodes absent from the musicdb are warned and skipped.

Usage:
    gen_judgement_offsets_csv.py <ddr-install-dir> <offsets-file> \
        [--out judgement_offsets.csv]

Warnings never fail the run; missing inputs / an unreadable ARC do.
"""

import argparse
import struct
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# Reuse the existing project ARC parser/decompressor.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from arc_tool import ARC_MAGIC, KonamiLz77, _read_cstring  # type: ignore

OFFSET_MIN = -100
OFFSET_MAX = 100
HEADER = "code,p1_offset,p2_offset"


def extract_musicdb(startup_arc: Path) -> bytes:
    """Pull data/gamedata/musicdb.xml out of startup.arc, decompressing if needed."""
    buf = startup_arc.read_bytes()
    if len(buf) < 16:
        raise SystemExit(f"{startup_arc}: file too small to be an ARC")
    magic, _version, count, _flag = struct.unpack_from("<IIII", buf, 0)
    if magic != ARC_MAGIC:
        raise SystemExit(f"{startup_arc}: bad ARC magic 0x{magic:08X}")
    for i in range(count):
        path_off, data_off, decomp, comp = struct.unpack_from("<IIII", buf, 16 + i * 16)
        name = _read_cstring(buf, path_off)
        if name == "data/gamedata/musicdb.xml":
            raw = buf[data_off : data_off + comp]
            if comp != decomp:
                data = KonamiLz77.decompress(raw)
                if len(data) != decomp:
                    raise SystemExit(
                        f"{startup_arc}: decompressed musicdb.xml is "
                        f"{len(data)} bytes, expected {decomp}"
                    )
                return data
            return raw
    raise SystemExit(f"{startup_arc}: data/gamedata/musicdb.xml not found in ARC")


def parse_music_entries(xml_bytes: bytes) -> List[Tuple[int, str]]:
    """(mcode, basename) for every <music> entry, in musicdb order."""
    root = ET.fromstring(xml_bytes)
    entries: List[Tuple[int, str]] = []
    for music in root.findall("music"):
        mcode_text = (music.findtext("mcode") or "").strip()
        basename = (music.findtext("basename") or "").strip()
        if not mcode_text or not basename:
            print(
                f"warning: <music> with mcode={mcode_text!r} basename={basename!r} "
                "missing a key field, skipping",
                file=sys.stderr,
            )
            continue
        entries.append((int(mcode_text), basename))
    return entries


def parse_offsets_file(path: Path) -> Dict[int, int]:
    """mcode -> negated, clamped offset. Warns on multi-field lines, junk, clamps.

    The community list records each song's sync ERROR; the CSV stores the
    COMPENSATION, so the sign is flipped here (see the module docstring).
    """
    offsets: Dict[int, int] = {}
    clamp_count = 0
    for line_no, raw in enumerate(path.read_text(encoding="ascii").splitlines(), 1):
        line = raw.strip()
        if not line:
            continue
        fields = line.split()
        if len(fields) < 2:
            print(f"warning: line {line_no}: {raw!r} — not a pair, skipped", file=sys.stderr)
            continue
        if len(fields) > 2:
            print(
                f"warning: line {line_no}: {raw!r} — {len(fields)} fields, "
                f"taking the first value ({fields[1]})",
                file=sys.stderr,
            )
        try:
            mcode = int(fields[0])
            value = -int(fields[1])  # sync error -> compensating offset
        except ValueError:
            print(f"warning: line {line_no}: {raw!r} — not integers, skipped", file=sys.stderr)
            continue
        clamped = max(OFFSET_MIN, min(OFFSET_MAX, value))
        if clamped != value:
            clamp_count += 1
            print(
                f"warning: line {line_no}: value {value} clamped to {clamped}",
                file=sys.stderr,
            )
        if mcode in offsets:
            print(
                f"warning: line {line_no}: duplicate mcode {mcode}, keeping the first",
                file=sys.stderr,
            )
            continue
        offsets[mcode] = clamped
    if clamp_count:
        print(f"warning: {clamp_count} value(s) clamped to +/-100", file=sys.stderr)
    return offsets


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Pre-seed judgement_offsets.csv from an mcode-keyed offsets list."
    )
    ap.add_argument("install", type=Path, help="DDR World install dir (contains data/)")
    ap.add_argument("offsets", type=Path, help="mcode-keyed offsets file")
    ap.add_argument(
        "--out",
        type=Path,
        default=Path("judgement_offsets.csv"),
        help="output CSV path (default: ./judgement_offsets.csv)",
    )
    args = ap.parse_args()

    startup_arc = args.install / "data" / "arc" / "startup.arc"
    if not startup_arc.is_file():
        raise SystemExit(f"{startup_arc}: not found (is this a DDR World install dir?)")
    if not args.offsets.is_file():
        raise SystemExit(f"{args.offsets}: not found")

    entries = parse_music_entries(extract_musicdb(startup_arc))
    if not entries:
        raise SystemExit("musicdb parsed to zero entries")
    offsets = parse_offsets_file(args.offsets)

    # First basename wins on (unexpected) duplicates.
    seen: Dict[str, int] = {}
    rows: List[Tuple[str, Optional[int]]] = []
    for mcode, basename in entries:
        if basename in seen:
            print(
                f"warning: duplicate basename {basename!r} (mcode {mcode}), "
                "keeping the first occurrence",
                file=sys.stderr,
            )
            continue
        seen[basename] = mcode
        rows.append((basename, offsets.get(mcode)))

    mapped_mcodes = {seen[code] for code, _ in rows}
    unmapped = sorted(m for m in offsets if m not in mapped_mcodes)
    for mcode in unmapped:
        print(f"warning: offsets mcode {mcode} not present in musicdb, skipped", file=sys.stderr)

    seeded = sum(1 for _, v in rows if v is not None)
    with args.out.open("w", encoding="ascii", newline="\n") as f:
        f.write(HEADER + "\n")
        for code, value in rows:
            cell = "" if value is None else str(value)
            f.write(f"{code},{cell},{cell}\n")

    print(
        f"wrote {args.out}: {len(rows)} rows, {seeded} seeded (P1=P2), "
        f"{len(rows) - seeded} blank; {len(unmapped)} offsets mcode(s) unmapped"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
