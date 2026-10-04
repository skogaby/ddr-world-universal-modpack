#!/usr/bin/env bash
# Offline validation for the Wii Dancing Stage / DDR HOTTEST PARTY tools:
# scripts/extract_wii_ddr_data.py and scripts/hsf_dump.py. Formats and RE:
# docs/wii_ddr_hottest_party_research.md.
#
# Usage:
#   ./scripts/validate_wii_ddr_tools.sh [extracted-dir]...
#
# Always runs the host tests (scripts/test_wii_ddr_formats.py): the Hudson codecs (LZ, slide,
# RLE, zlib packs; random data rejected), GX textures (I4 .. RGBA8, CMPR, C8 + palette), sprites,
# TPL, U8, Nintendo LZ10, DSP-ADPCM and RWSD waves, the message banks, the main.dol / REL tables,
# and a synthetic HSFV037 scene (objects, envelopes, the GX triangle order, the curve rules incl.
# MayaConverter's pre-roll keys, the World-space clip round trip). Each extracted-dir argument (the
# out_dir of `extract_wii_ddr_data.py extract ...`) adds a survey: every .hsf under data/ is parsed,
# its bitmaps decoded, its envelope meshes skinned at rest and its motions sampled; every .spr is
# parsed and its bitmaps decoded; the 55 SSQ charts are walked; the dol/ tables are checked.
#
# Requires: python3 + numpy. Writes nothing into the repo.

set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

note "running host tests (scripts/test_wii_ddr_formats.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -W ignore::ResourceWarning -m unittest -q test_wii_ddr_formats)

for dir in "$@"; do
  note "surveying ${dir/#$HOME/~}"
  PYTHONDONTWRITEBYTECODE=1 python3 -W ignore::ResourceWarning - "$dir" <<'EOF'
import csv, glob, os, sys
sys.path.insert(0, "scripts")
import extract_wii_ddr_data as W
import hsf_dump as H

out = sys.argv[1]
data = os.path.join(out, "data")
files, problems = H.survey([data])
print("  HSF: %d files" % files)
sprites = sorted(glob.glob(os.path.join(data, "**", "*.spr"), recursive=True))
bitmaps = 0
for path in sprites:
    blob = open(path, "rb").read()
    try:
        spr = W.parse_sprite(blob)
        for b in spr["bitmaps"]:
            W.sprite_bitmap_rgba(blob, b)
            bitmaps += 1
    except (ValueError, IndexError, KeyError) as e:
        problems.append((path, repr(e)))
print("  sprites: %d files, %d bitmaps" % (len(sprites), bitmaps))
charts = sorted(glob.glob(os.path.join(data, "c_000_*", "000.ssq")))
for path in charts:
    if not W.is_ssq(open(path, "rb").read()):
        problems.append((path, "not a walkable SSQ"))
print("  SSQ charts: %d" % len(charts))
dol = os.path.join(out, "dol")
for name, minimum in (("data_dirs.csv", 190), ("songs.csv", 50), ("dance_clip_bars.csv", 250)):
    path = os.path.join(dol, name)
    if not os.path.exists(path):
        problems.append((path, "missing"))
        continue
    rows = list(csv.DictReader(open(path)))
    if len(rows) < minimum:
        problems.append((path, "%d rows (< %d)" % (len(rows), minimum)))
    print("  dol/%s: %d rows" % (name, len(rows)))
home = os.path.expanduser("~")
for path, err in problems:
    print("PROBLEM", path.replace(home, "~", 1), err)
print("  %d problem(s)" % len(problems))
sys.exit(1 if problems else 0)
EOF
done
note "done"
