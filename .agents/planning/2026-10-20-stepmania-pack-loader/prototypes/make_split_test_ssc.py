#!/usr/bin/env python3
"""Step 10 fixture generator — a per-chart-timing `.ssc` from a `.sm` song. NOT shipped.

No pack on hand has a song whose difficulties carry different timing, so this crafts
one: from a `.sm` song folder it writes a sibling `<Song> (split test)/` folder with an
`.ssc` in which EVERY chart keeps the song timing except one (default: dance-single
Hard = DDR Expert), which gets its own `#BPMS` at 2x the song BPM (and `#OFFSET`) with
its note data re-measured so every note lands at the identical time:

  * each original 4-beat measure of R rows becomes TWO 4-beat measures holding the
    first and second halves of the rows (R made even first by doubling resolution —
    an EMPTY row after every row, never a duplicated note);
  * `beat=bpm` entries become `2*beat=2*bpm`; stops/delays keep their seconds at
    `2*beat`; warps/fakes scale their beats and lengths by 2;
  * hold/roll tails (`3`) stay per row, so a hold spanning the split stays valid.

The writer then groups the charts by timing and emits `<b>_N.ssq` per level (Expert ->
`<b>_4.ssq`) and no base file — exactly what the `ssq_path` chain must serve. Audio and
art are copied; `#TITLE` gets the ` (split test)` suffix so both songs can sit on the
wheel.

Usage:
  python3 make_split_test_ssc.py "<path to song folder with a .sm>" \
      [--out <folder>] [--stepstype dance-single] [--difficulty Hard] [--factor 2]

Prints paths ~-relative. Pure stdlib.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import sys
from fractions import Fraction
from pathlib import Path

SUFFIX = " (split test)"
SIMFILE_EXTS = {".sm", ".ssc", ".dwi", ".old", ".bak"}
HEADER_ORDER = [
    "TITLE", "SUBTITLE", "ARTIST", "TITLETRANSLIT", "SUBTITLETRANSLIT", "ARTISTTRANSLIT",
    "GENRE", "CREDIT", "BANNER", "BACKGROUND", "JACKET", "CDTITLE", "MUSIC", "OFFSET",
    "SAMPLESTART", "SAMPLELENGTH", "SELECTABLE", "DISPLAYBPM", "BPMS", "STOPS", "DELAYS",
    "WARPS", "FAKES",
]


def tilde(p: Path) -> str:
    home = os.path.expanduser("~")
    s = str(p)
    return "~" + s[len(home):] if s.startswith(home) else s


def strip_comments(text: str) -> str:
    return re.sub(r"//[^\n]*", "", text)


def parse_msd(text: str) -> list[list[str]]:
    """`#A:B:C;` -> ['A', 'B', 'C'] per tag (values un-trimmed; the caller trims)."""
    out: list[list[str]] = []
    for m in re.finditer(r"#([^;]*);", strip_comments(text), re.S):
        out.append(m.group(1).split(":"))
    return out


def scale_pairs(value: str, beat_k: Fraction, val_k: Fraction) -> str:
    """`b=v,b=v` with beats x beat_k and values x val_k (exact rationals)."""
    parts = []
    for item in value.replace("\n", "").split(","):
        item = item.strip()
        if not item:
            continue
        b, v = item.split("=", 1)
        nb = Fraction(b.strip()) * beat_k
        nv = Fraction(v.strip()) * val_k
        parts.append(f"{float(nb):.3f}={float(nv):.3f}")
    return ",".join(parts)


def remeasure(body: str) -> str:
    """Each measure -> two measures of half the rows; odd R doubled with empty rows."""
    measures = [m.strip() for m in body.split(",")]
    out: list[str] = []
    for m in measures:
        rows = [r.strip() for r in m.splitlines() if r.strip()]
        if not rows:
            continue
        width = len(rows[0])
        if len(rows) % 2:
            doubled = []
            for r in rows:
                doubled.append(r)
                doubled.append("0" * width)
            rows = doubled
        half = len(rows) // 2
        out.append("\n".join(rows[:half]))
        out.append("\n".join(rows[half:]))
    return "\n,\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("song", help="song folder containing a .sm")
    ap.add_argument("--out", help="output folder (default: sibling '<Song> (split test)')")
    ap.add_argument("--stepstype", default="dance-single")
    ap.add_argument("--difficulty", default="Hard", help="StepMania name (Hard = DDR Expert)")
    ap.add_argument("--factor", type=int, default=2, help="BPM multiplier (only 2 is re-measured exactly)")
    args = ap.parse_args()
    if args.factor != 2:
        print("error: only --factor 2 is supported (one measure -> two)", file=sys.stderr)
        return 2

    src = Path(args.song).expanduser()
    sms = sorted(p for p in src.iterdir() if p.suffix.lower() == ".sm")
    if not sms:
        print(f"error: no .sm in {tilde(src)}", file=sys.stderr)
        return 2
    sm = sms[0]
    text = sm.read_bytes().decode("utf-8", errors="replace").replace("\r\n", "\n")
    tags = parse_msd(text)

    header: dict[str, str] = {}
    charts: list[list[str]] = []
    for t in tags:
        name = t[0].strip().upper()
        if name == "NOTES":
            # stepstype, description, difficulty, meter, radar, notedata
            if len(t) >= 7:
                charts.append([p.strip() for p in t[1:6]] + [":".join(t[6:])])
        elif len(t) >= 2 and name not in header:
            header[name] = ":".join(t[1:]).strip()

    title = header.get("TITLE", src.name)
    out = Path(args.out).expanduser() if args.out else src.with_name(src.name + SUFFIX)
    out.mkdir(parents=True, exist_ok=True)

    lines = ["#VERSION:0.83;", f"#TITLE:{title}{SUFFIX};"]
    for key in HEADER_ORDER:
        if key == "TITLE" or key not in header:
            continue
        lines.append(f"#{key}:{header[key]};")

    song_bpms = header.get("BPMS", "")
    song_offset = header.get("OFFSET", "0.000")
    k = Fraction(args.factor)
    converted = 0
    kept = 0
    for stepstype, description, difficulty, meter, _radar, body in charts:
        if not stepstype.lower().startswith("dance-"):
            continue
        block = [
            "#NOTEDATA:;",
            f"#STEPSTYPE:{stepstype};",
            f"#DESCRIPTION:{description};",
            f"#DIFFICULTY:{difficulty};",
            f"#METER:{meter};",
        ]
        is_target = (
            stepstype.lower() == args.stepstype.lower()
            and difficulty.lower() == args.difficulty.lower()
        )
        if is_target:
            block.append(f"#OFFSET:{song_offset};")
            block.append(f"#BPMS:{scale_pairs(song_bpms, k, k)};")
            for key in ("STOPS", "DELAYS"):
                if header.get(key):
                    block.append(f"#{key}:{scale_pairs(header[key], k, Fraction(1))};")
            for key in ("WARPS", "FAKES"):
                if header.get(key):
                    block.append(f"#{key}:{scale_pairs(header[key], k, k)};")
            body = remeasure(body)
            converted += 1
        else:
            body = "\n,\n".join(
                "\n".join(r.strip() for r in m.splitlines() if r.strip())
                for m in body.split(",")
                if m.strip()
            )
            kept += 1
        block.append("#NOTES:")
        block.append(body)
        block.append(";")
        lines.extend(block)

    if converted == 0:
        print(
            f"error: no {args.stepstype} {args.difficulty} chart in {tilde(sm)}",
            file=sys.stderr,
        )
        return 1

    ssc = out / (sm.stem + ".ssc")
    ssc.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    copied = []
    for p in sorted(src.iterdir()):
        if p.is_file() and p.suffix.lower() not in SIMFILE_EXTS:
            shutil.copy2(p, out / p.name)
            copied.append(p.name)

    print(f"source : {tilde(sm)}")
    print(f"output : {tilde(ssc)}")
    print(f"charts : {kept} on song timing, {converted} re-measured at {args.factor}x BPM "
          f"({args.stepstype} {args.difficulty}; song #BPMS {song_bpms} -> {scale_pairs(song_bpms, k, k)})")
    print(f"copied : {', '.join(copied) if copied else '(nothing)'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
