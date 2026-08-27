#!/usr/bin/env python3
"""
Bulk-unpack DDR asset archives.

Walks an input directory tree, extracts every ``*.arc`` file into a parallel
structure under the output directory, then unpacks any ``*.ifs`` files that
appear inside the extracted ARC contents using ``ifstools``.

Usage:
    python3 unpack_all.py <input_dir> -o <output_dir>
    python3 unpack_all.py <input_dir> -o <output_dir> --keep-ifs --verbose
    python3 unpack_all.py <input_dir> -o <output_dir> --no-ifs

Requires:
    - scripts/unpack_arc.py (next to this script)
    - ifstools on PATH (pip install ifstools)
"""
from __future__ import annotations

import argparse
import contextlib
import io
import shutil
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
from unpack_arc import ARC, ARCError  # noqa: E402


def extract_arc(arc_path: Path, dest: Path, verbose: bool) -> tuple[int, int]:
    """Extract all files from an ARC archive into ``dest``.

    Returns (files_ok, files_failed).
    """
    arc_data = arc_path.read_bytes()

    sink = None if verbose else io.StringIO()
    redirect = contextlib.redirect_stdout(sink) if sink is not None else contextlib.nullcontext()

    with redirect:
        arc = ARC(arc_data, decompress=True)
        dest.mkdir(parents=True, exist_ok=True)

        ok = 0
        failed = 0
        for name in arc.list_files():
            try:
                data = arc.get_file(name)
                if data is None:
                    failed += 1
                    continue
                out = dest / name
                out.parent.mkdir(parents=True, exist_ok=True)
                out.write_bytes(data)
                ok += 1
            except Exception as exc:
                failed += 1
                if verbose:
                    print(f"  [warn] could not extract {name}: {exc}")
    return ok, failed


def extract_ifs(ifs_path: Path, verbose: bool) -> bool:
    cmd = ["ifstools", "-y", "-o", str(ifs_path.parent), str(ifs_path)]
    if not verbose:
        cmd.insert(1, "-s")
    result = subprocess.run(cmd, capture_output=not verbose, text=True)
    if result.returncode != 0:
        if not verbose:
            err = (result.stderr or result.stdout or "").strip()
            print(f"  [ifs-fail] {ifs_path}: {err}")
        return False
    return True


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Recursively extract ARC files in a directory, then unpack any IFS files within.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("input", type=Path, help="Input directory to search for ARC files")
    parser.add_argument("-o", "--output", type=Path, required=True,
                        help="Output directory for extracted contents")
    parser.add_argument("--no-ifs", action="store_true",
                        help="Skip the IFS extraction pass")
    parser.add_argument("--keep-ifs", action="store_true",
                        help="Keep .ifs files after unpacking (default: remove once extracted)")
    parser.add_argument("-v", "--verbose", action="store_true",
                        help="Echo per-file output from unpack_arc and ifstools")
    args = parser.parse_args()

    if not args.input.is_dir():
        print(f"Error: input directory not found: {args.input}", file=sys.stderr)
        return 1

    if not args.no_ifs and shutil.which("ifstools") is None:
        print("Error: 'ifstools' is not on PATH. Install with: pip install ifstools", file=sys.stderr)
        print("       (or re-run with --no-ifs to skip IFS extraction)", file=sys.stderr)
        return 1

    args.output.mkdir(parents=True, exist_ok=True)

    # Pass 1: ARC files under the input tree.
    arc_files = sorted(args.input.rglob("*.arc"))
    print(f"Found {len(arc_files)} ARC file(s) under {args.input}")

    arc_ok = arc_fail = files_ok = files_fail = 0
    for arc_path in arc_files:
        rel = arc_path.relative_to(args.input)
        try:
            ok, failed = extract_arc(arc_path, args.output, args.verbose)
            files_ok += ok
            files_fail += failed
            arc_ok += 1
            suffix = f", {failed} failed" if failed else ""
            print(f"[arc] {rel} ({ok} files{suffix})")
        except ARCError as exc:
            arc_fail += 1
            print(f"[arc-fail] {rel}: {exc}")
        except Exception as exc:
            arc_fail += 1
            print(f"[arc-fail] {rel}: {type(exc).__name__}: {exc}")

    print(f"\nARC pass: {arc_ok} archive(s) extracted ({files_ok} files), "
          f"{arc_fail} archive(s) failed, {files_fail} files failed")

    if args.no_ifs:
        return 0 if arc_fail == 0 else 2

    # Pass 2: IFS files found anywhere in the output tree (from extracted ARCs
    # or pre-existing). Also catches IFS-in-IFS via ifstools' own recursion.
    ifs_files = sorted(args.output.rglob("*.ifs"))
    print(f"\nFound {len(ifs_files)} IFS file(s) under {args.output}")

    ifs_ok = ifs_fail = 0
    for ifs in ifs_files:
        rel = ifs.relative_to(args.output)
        print(f"[ifs] {rel}")
        if extract_ifs(ifs, args.verbose):
            ifs_ok += 1
            if not args.keep_ifs:
                try:
                    ifs.unlink()
                except OSError as exc:
                    print(f"  [warn] could not remove {rel}: {exc}")
        else:
            ifs_fail += 1

    print(f"\nIFS pass: {ifs_ok} unpacked, {ifs_fail} failed")
    return 0 if (arc_fail == 0 and ifs_fail == 0) else 2


if __name__ == "__main__":
    sys.exit(main())
