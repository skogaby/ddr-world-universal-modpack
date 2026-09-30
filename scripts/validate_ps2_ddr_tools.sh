#!/usr/bin/env bash
# Offline validation for the PS2 DDR asset extractor, scripts/extract_ps2_ddr_data.py.
# Formats and RE: docs/ps2_ddr_filedata_research.md.
#
# Usage:
#   ./scripts/validate_ps2_ddr_tools.sh [extracted-dir]...
#
# Always runs the host tests (scripts/test_ps2_ddr_formats.py): Bemani LZ, TOC and DAT table
# reading and scanning, the container parsers (including random data being left alone), TCB,
# TIM2, TGCD, Svag and VIG decoding, and extraction end to end on synthetic discs; and
# scripts/test_tzm_dump.py: the SuperNova-engine .TZM model packs (container, 8/4 bpp
# textures, MODEL nodes / meshes / strips / skinning, MOTION records incl. the XSI fcurves and
# cameras, the World-space math) on synthetic packs; and scripts/test_anm_dump.py: the World
# `.sanm` material-animation writer round trip. Each extracted-dir argument (the out_dir of
# `extract_ps2_ddr_data.py extract ... --unpack`) adds a survey: every System 573-format
# dancer mesh (.cmd) is parsed and every motion clip (.cmm, PS2 key-block layout) sampled with
# scripts/sys573_dancer_dump.py, the rig tables copied from the ELF are checked, every .TZM
# under files/ is parsed with scripts/tzm_dump.py (textures decoded, models skinned at rest,
# motion records read; the IMAGE/test/ developer packs are skipped), and (DAT games) every
# file's checksum result in manifest.csv.
#
# Requires: python3 (+ numpy for the survey). Writes nothing into the repo.

set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

note "running host tests (scripts/test_ps2_ddr_formats.py, scripts/test_tzm_dump.py, scripts/test_anm_dump.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q test_ps2_ddr_formats test_tzm_dump test_anm_dump)

for dir in "$@"; do
  note "surveying ${dir/#$HOME/~}"
  PYTHONDONTWRITEBYTECODE=1 python3 - "$dir" <<'EOF'
import csv, glob, os, struct, sys
sys.path.insert(0, "scripts")
import sys573_dancer_dump as D

out = sys.argv[1]
meshes = sorted(glob.glob(os.path.join(out, "unpacked", "**", "*.cmd"), recursive=True))
bad = []
for path in meshes:
    try:
        objects = D.parse_cmd(open(path, "rb").read())
        if len(objects) not in (20, 28):  # chara20.lst / chara.lst
            bad.append((path, f"{len(objects)} objects"))
    except (ValueError, IndexError) as e:
        bad.append((path, str(e)))
motions = glob.glob(os.path.join(out, "unpacked", "**", "*.cmm"), recursive=True)
rest = [[0, 0, 0]] * 16
clips = 0
for path in motions:
    try:
        for clip in D.parse_cmm(open(path, "rb").read()).values():
            for t in (0, 700, 1920):
                D.local_pose(clip, t, rest)
            clips += 1
    except (ValueError, IndexError, struct.error) as e:
        bad.append((path, str(e)))
rig = os.path.join(out, "elf")
if os.path.isdir(rig):
    lst = D.parse_lst(open(os.path.join(rig, "chara.lst"), "rb").read())
    pos = D.parse_pos(open(os.path.join(rig, "chara.pos"), "rb").read())
    if len(lst) != 28 or len(pos) != 17 or [j for j, _ in lst[:13]] != list(range(13)):
        bad.append((rig, "unexpected chara.lst / chara.pos shape"))
tzms = sorted(glob.glob(os.path.join(out, "files", "**", "*.[Tt][Zz][Mm]"), recursive=True))
tzms = [p for p in tzms if "test" not in os.path.relpath(p, out).split(os.sep)[:-1]]
tzm_models = tzm_records = 0
if tzms:
    import tzm_dump as Z
    _n, tzm_models, _meshes, tzm_records, _tracks, tzm_bad = Z.survey(tzms)
    bad += tzm_bad
checked = 0
with open(os.path.join(out, "manifest.csv"), newline="") as fh:
    for row in csv.DictReader(fh):
        if row.get("checksum"):  # DAT games: the table's byte sum, verified at extraction
            checked += 1
            if row["checksum"] != "ok":
                bad.append((os.path.join(out, row["file"] or "manifest.csv"), "byte sum differs from the table"))
print(f"    {len(meshes)} dancer meshes, {len(motions)} motion sets ({clips} clips sampled),"
      f" {len(tzms)} TZM packs ({tzm_models} models, {tzm_records} motion records),"
      f" {checked} checksums, {len(bad)} problems")
for path, why in bad[:20]:
    print(f"    {os.path.relpath(path, out)}: {why}")
sys.exit(1 if bad else 0)
EOF
done
note "OK"
