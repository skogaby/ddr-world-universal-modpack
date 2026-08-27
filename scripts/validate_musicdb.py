#!/usr/bin/env python3
"""
validate_musicdb — cross-check musicdb.xml difficulty slots against SSQ contents.

For every <music> entry in `data/arc/startup.arc!data/gamedata/musicdb.xml`,
verify that each non-zero `diffLv` slot has a matching type-3 step chunk in
the song's SSQ file(s) under `data/mdb_apx/ssq/`.

Slot → chart code mapping (matches DDR World's SsqReader vtable[1]
dispatcher; see docs/ssq_format.md §5.1):

    Index  Param2   Chart
    0      0x0414   Single Beginner
    1      0x0114   Single Basic
    2      0x0214   Single Difficult
    3      0x0314   Single Expert
    4      0x0614   Single Challenge
    5      0x0418   Double Beginner
    6      0x0118   Double Basic
    7      0x0218   Double Difficult
    8      0x0318   Double Expert
    9      0x0618   Double Challenge

For songs whose charts use per-level gimmicks (different BPM curves etc.),
the data lives in split files `<basename>_<level>.ssq` where level is
1..5 = Beginner, Basic, Difficult, Expert, Challenge. Both the unsplit
`<basename>.ssq` and the level-specific `<basename>_<level>.ssq` are
checked — a chart is considered present if any candidate file contains
a type-3 chunk whose `param2` matches the expected code.

Usage:
    validate_musicdb.py <ddr-install-dir> [--strict] [--csv out.csv]

Exit code is 1 when any chart is missing (or, with --strict, when any
extra chart not declared in musicdb.xml is found in an SSQ).
"""

import argparse
import struct
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Set, Tuple

# Reuse the existing project ARC parser/decompressor.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from arc_tool import ARC_MAGIC, KonamiLz77, _read_cstring  # type: ignore


# --- Slot table -----------------------------------------------------------

# (chart code, level 1..5, mode label) per diffLv index.
SLOTS: List[Tuple[int, int, str]] = [
    (0x0414, 1, "Single Beginner"),
    (0x0114, 2, "Single Basic"),
    (0x0214, 3, "Single Difficult"),
    (0x0314, 4, "Single Expert"),
    (0x0614, 5, "Single Challenge"),
    (0x0418, 1, "Double Beginner"),
    (0x0118, 2, "Double Basic"),
    (0x0218, 3, "Double Difficult"),
    (0x0318, 4, "Double Expert"),
    (0x0618, 5, "Double Challenge"),
]


# --- ARC + musicdb extraction --------------------------------------------


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


# --- musicdb parsing ------------------------------------------------------


@dataclass
class MusicEntry:
    mcode: int
    basename: str
    title: str
    diff_lv: List[int]  # 10 entries, raw u8 values from XML


def parse_musicdb(xml_bytes: bytes) -> List[MusicEntry]:
    root = ET.fromstring(xml_bytes)
    entries: List[MusicEntry] = []
    for music in root.findall("music"):
        diff_text = (music.findtext("diffLv") or "").strip()
        if not diff_text:
            continue  # skip entries with no diffLv (shouldn't normally happen)
        diff_lv = [int(x) for x in diff_text.split()]
        if len(diff_lv) != 10:
            print(
                f"warning: <music> mcode={music.findtext('mcode')!r} "
                f"basename={music.findtext('basename')!r}: "
                f"diffLv has {len(diff_lv)} entries (expected 10), skipping",
                file=sys.stderr,
            )
            continue
        entries.append(
            MusicEntry(
                mcode=int(music.findtext("mcode") or "0"),
                basename=(music.findtext("basename") or "").strip(),
                title=(music.findtext("title") or "").strip(),
                diff_lv=diff_lv,
            )
        )
    return entries


# --- SSQ parsing ----------------------------------------------------------


def ssq_chart_codes(ssq_path: Path) -> Set[int]:
    """Return the set of param2 values for every type-3 chunk in the SSQ."""
    data = ssq_path.read_bytes()
    codes: Set[int] = set()
    i = 0
    n = len(data)
    while i + 12 <= n:
        length, ctype, param2, _p3, _p4 = struct.unpack_from("<IHHHH", data, i)
        if length == 0 or param2 == 0xFFFF:
            break
        if length < 12 or i + length > n:
            break  # malformed; bail rather than infinite-loop
        if ctype == 3:
            codes.add(param2)
        i += length
    return codes


# --- Validation -----------------------------------------------------------


@dataclass
class SongResult:
    entry: MusicEntry
    missing: List[Tuple[int, int, str]] = field(default_factory=list)  # (slot, code, label)
    extra: List[Tuple[int, str, Path]] = field(default_factory=list)   # (code, label, file)
    candidate_files: List[Path] = field(default_factory=list)          # files actually checked


def validate(
    entries: Iterable[MusicEntry],
    ssq_dir: Path,
    strict: bool,
) -> Tuple[List[SongResult], int, int]:
    """Returns (results, total_expected_charts, total_missing_charts)."""
    results: List[SongResult] = []
    total_expected = 0
    total_missing = 0

    for entry in entries:
        result = SongResult(entry=entry)

        # Build the union of chart codes across all candidate SSQ files.
        # Track each candidate so a missing slot's error message can mention
        # exactly which files were searched.
        unsplit = ssq_dir / f"{entry.basename}.ssq"
        present_codes: Dict[int, Path] = {}  # code → first file we saw it in
        if unsplit.is_file():
            result.candidate_files.append(unsplit)
            for code in ssq_chart_codes(unsplit):
                present_codes.setdefault(code, unsplit)

        # Per-level files (`_1.ssq`..`_5.ssq`). Only check the ones that exist.
        for level in range(1, 6):
            split = ssq_dir / f"{entry.basename}_{level}.ssq"
            if split.is_file():
                result.candidate_files.append(split)
                for code in ssq_chart_codes(split):
                    present_codes.setdefault(code, split)

        # No SSQ at all? That's a complete miss for every non-zero slot.
        if not result.candidate_files:
            for slot, (code, _level, label) in enumerate(SLOTS):
                if entry.diff_lv[slot] != 0:
                    result.missing.append((slot, code, label))
                    total_missing += 1
                    total_expected += 1
            results.append(result)
            continue

        # Per-slot validation.
        declared_codes: Set[int] = set()
        for slot, (code, _level, label) in enumerate(SLOTS):
            if entry.diff_lv[slot] == 0:
                continue
            total_expected += 1
            declared_codes.add(code)
            if code not in present_codes:
                result.missing.append((slot, code, label))
                total_missing += 1

        # Strict: any chunk in the SSQs not declared in diffLv is "extra".
        if strict:
            for code, file in sorted(present_codes.items()):
                if code not in declared_codes:
                    label = next(
                        (lbl for c, _l, lbl in SLOTS if c == code),
                        f"unknown(0x{code:04X})",
                    )
                    result.extra.append((code, label, file))

        results.append(result)

    return results, total_expected, total_missing


# --- Output ---------------------------------------------------------------


def report(
    results: List[SongResult],
    total_expected: int,
    total_missing: int,
    csv_path: Optional[Path],
    strict: bool,
) -> int:
    songs_with_missing = [r for r in results if r.missing]
    songs_no_ssq = [r for r in results if not r.candidate_files]

    print(f"Songs in musicdb.xml      : {len(results)}")
    print(f"Charts declared (non-zero): {total_expected}")
    print(f"Charts missing            : {total_missing}")
    if strict:
        total_extra = sum(len(r.extra) for r in results)
        print(f"Charts in SSQ but not in musicdb.xml: {total_extra}")
    print(f"Songs with at least one missing chart: {len(songs_with_missing)}")
    print(f"Songs with no SSQ file at all        : {len(songs_no_ssq)}")
    print()

    for r in songs_with_missing:
        files = ", ".join(p.name for p in r.candidate_files) or "(none)"
        print(
            f"[MISS] {r.entry.basename!r} (mcode={r.entry.mcode}, "
            f"title={r.entry.title!r}) — files checked: {files}"
        )
        for slot, code, label in r.missing:
            level = r.entry.diff_lv[slot]
            print(f"        slot {slot} ({label:<17}) lv={level:<3} code=0x{code:04X}")
        if strict and r.extra:
            for code, label, file in r.extra:
                print(
                    f"        [extra in SSQ] {label:<17} code=0x{code:04X} "
                    f"in {file.name}"
                )

    if strict:
        for r in results:
            if r.extra and not r.missing:
                print(
                    f"[EXTRA] {r.entry.basename!r} (mcode={r.entry.mcode}) — "
                    f"SSQ has charts not declared in musicdb.xml"
                )
                for code, label, file in r.extra:
                    print(f"        {label:<17} code=0x{code:04X} in {file.name}")

    if csv_path is not None:
        write_csv(csv_path, results, strict)
        print(f"\nWrote CSV report → {csv_path}")

    if total_missing > 0:
        return 1
    if strict and any(r.extra for r in results):
        return 1
    return 0


def write_csv(path: Path, results: List[SongResult], strict: bool) -> None:
    import csv

    with path.open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(
            [
                "mcode",
                "basename",
                "title",
                "kind",
                "slot_index",
                "chart_label",
                "chart_code",
                "diff_lv",
                "files_checked",
            ]
        )
        for r in results:
            files_checked = ";".join(p.name for p in r.candidate_files)
            for slot, code, label in r.missing:
                w.writerow(
                    [
                        r.entry.mcode,
                        r.entry.basename,
                        r.entry.title,
                        "missing",
                        slot,
                        label,
                        f"0x{code:04X}",
                        r.entry.diff_lv[slot],
                        files_checked,
                    ]
                )
            if strict:
                for code, label, file in r.extra:
                    w.writerow(
                        [
                            r.entry.mcode,
                            r.entry.basename,
                            r.entry.title,
                            "extra",
                            "",
                            label,
                            f"0x{code:04X}",
                            "",
                            file.name,
                        ]
                    )


# --- CLI -----------------------------------------------------------------


def main() -> int:
    p = argparse.ArgumentParser(
        description=(
            "Validate musicdb.xml difficulty slots against the SSQ files "
            "in a DDR World installation."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Pass the `contents` directory (the one with `data/`, `prop/`, "
            "`spice64.exe`, etc.). The script reads "
            "`data/arc/startup.arc!data/gamedata/musicdb.xml` and the SSQs "
            "under `data/mdb_apx/ssq/`."
        ),
    )
    p.add_argument("install_dir", type=Path, help="DDR World install root (the `contents` dir)")
    p.add_argument(
        "--strict",
        action="store_true",
        help="Also flag charts present in SSQ files that aren't declared in musicdb.xml.",
    )
    p.add_argument(
        "--csv",
        type=Path,
        default=None,
        help="Write a per-issue CSV report to this path.",
    )
    args = p.parse_args()

    install: Path = args.install_dir
    startup_arc = install / "data" / "arc" / "startup.arc"
    ssq_dir = install / "data" / "mdb_apx" / "ssq"

    if not startup_arc.is_file():
        print(f"error: {startup_arc} not found", file=sys.stderr)
        return 2
    if not ssq_dir.is_dir():
        print(f"error: {ssq_dir} not found", file=sys.stderr)
        return 2

    print(f"musicdb source: {startup_arc}")
    print(f"ssq dir       : {ssq_dir}")
    print()

    xml_bytes = extract_musicdb(startup_arc)
    entries = parse_musicdb(xml_bytes)
    results, total_expected, total_missing = validate(entries, ssq_dir, args.strict)
    return report(results, total_expected, total_missing, args.csv, args.strict)


if __name__ == "__main__":
    raise SystemExit(main())
