#!/usr/bin/env bash
# Offline validation for the Wii Dancing Stage / DDR HOTTEST PARTY tools:
# scripts/extract_wii_ddr_data.py, scripts/hsf_dump.py (HOTTEST PARTY 1) and scripts/zan_dump.py
# (FuruFuru Party = HOTTEST PARTY 2, MUSIC FIT = HOTTEST PARTY 3). Formats and RE:
# docs/wii_ddr_hottest_party_research.md, docs/wii_ddr_hottest_party_2_3_research.md.
#
# Usage:
#   ./scripts/validate_wii_ddr_tools.sh [extracted-dir | zan-disc-dir]...
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
# Also always runs scripts/test_zan_formats.py (synthetic zan archives, ZMB / ZAB / cameras, the
# material modes, the UV-key and flip-book semantics, the choreography helpers). A zan-disc-dir
# argument (a dumped FuruFuru Party / MUSIC FIT disc: `extract_wii_ddr_data.py disc ...`, detected by
# its stage/ + sound/stream/ dirs) adds a zan survey instead: every `WII\0` archive parses
# (zan_dump.survey), every costume CHR<nn>0 with a head builds a rig of <= 64 joints and has a skin
# material and a main.dol skin tone (sys/*.dol), every stage's
# OBJSET_ layout node finds its prop, every multi-texture material is a well-formed flip-book and
# every UV-key set samples.
#
# Requires: python3 + numpy. Writes nothing into the repo. e.g.
#   ./scripts/validate_wii_ddr_tools.sh ~/"Desktop/DDR Wii ISOs/Furu Furu Party" \
#       ~/"Desktop/DDR Wii ISOs/Music Fit"

set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

note "running host tests (scripts/test_wii_ddr_formats.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -W ignore::ResourceWarning -m unittest -q test_wii_ddr_formats)
note "running host tests (scripts/test_zan_formats.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -W ignore::ResourceWarning -m unittest -q test_zan_formats)

zan_survey() {
  PYTHONDONTWRITEBYTECODE=1 python3 -W ignore::ResourceWarning - "$1" <<'PYEOF'
import glob, os, sys
sys.path.insert(0, "scripts")
import numpy as np
import zan_dump as Z

root = sys.argv[1]
counts, problems = Z.survey([root])
print("  archives: %s" % ", ".join("%d %s" % (n, k) for k, n in sorted(counts.items())))
# the costumes: a rig within the frame board's 64 bones. HOTTEST PARTY 4 / 5 (a dance/ dir) name
# them CHR<id><variant:02> and bake the skin into the textures: no skin material, no tone table
hp45 = os.path.isdir(os.path.join(root, "dance"))
mots = sorted(glob.glob(os.path.join(root, "dance", "DANCE_*_MOT_010.bin") if hp45 else
                        os.path.join(root, "motion", "MOT010_SSQ*.bin")))
keep = Z.parse_zab(Z.members(open(mots[0], "rb").read(), "zab")[0][2])["order"] if mots else []
rigs = 0
dols = sorted(glob.glob(os.path.join(root, "sys", "*.dol")))
tones = None
if not hp45:
    try:
        tones = Z.skin_tone_table(open(dols[0], "rb").read()) if dols else None
    except ValueError as e:
        problems.append((dols[0], repr(e)))
pattern = "CHR*00.bin" if hp45 else "CHR??0.bin"
for path in sorted(glob.glob(os.path.join(root, "sound", "stream", "character", pattern))):
    zm = Z.members(open(path, "rb").read(), "zmb")
    if len(zm) < 2:
        continue                                  # a Mii body: no head, not ported
    try:
        body = Z.parse_zmb(zm[0][2])
        n = len(Z.rig_joints(body, keep=keep))
        rigs += 1
        if n > 64:
            problems.append((path, "%d rig joints (> 64)" % n))
        if hp45:
            continue
        nn = int(os.path.basename(path)[3:5])
        if tones is not None and nn not in tones:
            problems.append((path, "no skin tone in main.dol"))
        if Z.skin_material(body) is None:
            problems.append((path, "no skin material (colour group 2)"))
    except (ValueError, IndexError, KeyError) as e:
        problems.append((path, repr(e)))
print("  costumes: %d rigs <= 64 joints, %s" % (
    rigs, "skin in the textures (HP4 / HP5)" if hp45 else "%s skin tones from main.dol" % (
        len(tones) if tones is not None else "no")))
# the stages: layout nodes, flip-books, UV keys
stages = sorted(p for p in glob.glob(os.path.join(root, "stage", "STG*.bin")) if Z.STAGE_FILE.match(os.path.basename(p)))
placed = flips = uvsets = orphans = 0
for path in stages:
    blob = open(path, "rb").read()
    try:
        src = Z.stage_sources(blob)
        inst = Z.stage_instances(src)
        placed += sum(len(v) for v in inst.values())
        col = src["col"][0] if src["col"] else None
        stems = [e["stem"].lower() for e in src["models"] if e["kind"] == "obj"]
        for nd in (col["nodes"] if col else []):
            if nd["name"].startswith("OBJSET_") and not any(nd["name"] in v for v in inst.values()):
                tail = Z.instance_key(nd["name"]).split("_")[-1].lower()
                if any(tail in st for st in stems):        # the prop is there but the rule missed it
                    problems.append((path, "%s places no prop" % nd["name"]))
                else:                                       # the disc dropped the prop (MUSIC FIT's screens)
                    orphans += 1
        zb = {n.rsplit(".", 1)[0]: b for _p, n, b in Z.members(blob, "zmb") if n}
        for e in src["models"]:
            m = e["model"]
            for mi, mt in enumerate(m["materials"]):
                if len(mt["textures"]) > 1:
                    fb = Z.flip_book(mt)
                    if fb is None or max(fb[0]) >= len(e["textures"]):
                        problems.append((path, "%s material %d: a malformed flip-book" % (e["stem"], mi)))
                    else:
                        flips += 1
                k = Z.material_uv_keys(m, mt)
                if k and e["stem"] in zb:
                    keys, flags = Z.uv_keys(zb[e["stem"]], *k)
                    v = Z.sample_uv(keys, flags, np.linspace(0.0, 30.0, 61))
                    if not np.isfinite(v).all():
                        problems.append((path, "%s material %d: UV keys sample to NaN" % (e["stem"], mi)))
                    uvsets += 1
    except (ValueError, IndexError, KeyError) as e:
        problems.append((path, repr(e)))
print("  stages: %d, %d props placed (%d layout nodes without a prop on the disc), %d flip-books, %d UV-key sets" % (
    len(stages), placed, orphans, flips, uvsets))
home = os.path.expanduser("~")
for path, err in problems:
    print("PROBLEM", path.replace(home, "~", 1), err)
print("  %d problem(s)" % len(problems))
sys.exit(1 if problems else 0)
PYEOF
}

for dir in "$@"; do
  if [ -d "$dir/stage" ] && [ -d "$dir/sound/stream" ]; then
    note "zan survey of ${dir/#$HOME/~}"
    zan_survey "$dir"
    continue
  fi
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
