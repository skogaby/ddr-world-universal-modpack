#!/usr/bin/env python3
"""Corrupt a materialised injected SSQ for the Step 11 quarantine demo.

Default: re-tag every step chunk (kind 3) as an unused kind, so every
declared slot reads as "no step chunk" — the chunk list stays walkable,
nothing is resized, and the game's stock analysis would return zero steps
(ME1529 without the quarantine). `--zero-rows` instead zeroes the step
bytes of every step chunk (rows present but empty).

The original is kept next to the file as `<name>.orig`; `--restore` puts it
back. Writing the file changes its mtime, so fast-bootup re-analyses (and
pre-flights) it on the next boot; the StepMania index reuses it because it
only checks that the artefact exists.

Usage:
  corrupt_ssq.py "$DDR_WORLD_INSTALL/data_mods/_cache/stepmania/ssq/zab12c.ssq"
  corrupt_ssq.py --zero-rows <ssq>
  corrupt_ssq.py --restore <ssq>
"""

import argparse
import os
import shutil
import struct
import sys

HEADER = 12
STEP_KIND = 3
DEAD_KIND = 0x0F


def chunks(data):
    off = 0
    while off + 4 <= len(data):
        (length,) = struct.unpack_from("<I", data, off)
        if length == 0:
            return
        if length < HEADER or off + length > len(data):
            raise SystemExit(f"chunk list not walkable at byte {off}")
        kind, param2, param3 = struct.unpack_from("<HHH", data, off + 4)
        yield off, length, kind, param2, param3
        off += length


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("ssq")
    ap.add_argument("--zero-rows", action="store_true", help="zero step bytes instead of re-tagging")
    ap.add_argument("--restore", action="store_true", help="restore <ssq>.orig")
    args = ap.parse_args()

    orig = args.ssq + ".orig"
    if args.restore:
        if not os.path.exists(orig):
            raise SystemExit(f"no backup at {orig}")
        shutil.move(orig, args.ssq)
        print(f"restored {args.ssq}")
        return

    data = bytearray(open(args.ssq, "rb").read())
    if not os.path.exists(orig):
        shutil.copy2(args.ssq, orig)

    touched = 0
    for off, length, kind, param2, count in list(chunks(data)):
        if kind != STEP_KIND:
            continue
        if args.zero_rows:
            start = off + HEADER + 4 * count
            data[start : start + count] = bytes(count)
        else:
            struct.pack_into("<H", data, off + 4, DEAD_KIND)
        touched += 1
        print(f"  step chunk slot code 0x{param2:04X}: {count} rows {'zeroed' if args.zero_rows else 're-tagged'}")
    if touched == 0:
        raise SystemExit("no step chunks found (already corrupted?)")
    open(args.ssq, "wb").write(data)
    print(f"corrupted {touched} step chunk(s) in {args.ssq} (backup: {os.path.basename(orig)})")


if __name__ == "__main__":
    sys.exit(main())
