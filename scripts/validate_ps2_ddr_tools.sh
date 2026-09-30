#!/usr/bin/env bash
# Offline validation for the PS2 DDR asset extractor, scripts/extract_ps2_ddr_data.py.
# Formats and RE: docs/ps2_ddr_filedata_research.md.
#
# Usage:
#   ./scripts/validate_ps2_ddr_tools.sh [extracted-dir]...
#
# Always runs the host tests (scripts/test_ps2_ddr_formats.py): Bemani LZ, TOC reading and
# scanning, the container parsers (including random data being left alone), TCB and Svag
# decoding. Each extracted-dir argument (the out_dir of `extract_ps2_ddr_data.py extract
# ... --unpack`) adds a survey: every System 573-format dancer mesh (.cmd) is parsed and every
# motion clip (.cmm, PS2 key-block layout) sampled with scripts/sys573_dancer_dump.py, and the
# rig tables copied from the ELF are checked.
#
# Requires: python3 (+ numpy for the survey). Writes nothing into the repo.

set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

note "running host tests (scripts/test_ps2_ddr_formats.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q test_ps2_ddr_formats)

for dir in "$@"; do
  note "surveying ${dir/#$HOME/~}"
  PYTHONDONTWRITEBYTECODE=1 python3 - "$dir" <<'EOF'
import glob, os, struct, sys
sys.path.insert(0, "scripts")
import sys573_dancer_dump as D

out = sys.argv[1]
meshes = sorted(glob.glob(os.path.join(out, "unpacked", "**", "*.cmd"), recursive=True))
bad = []
for path in meshes:
    try:
        objects = D.parse_cmd(open(path, "rb").read())
        if len(objects) != 28:
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
print(f"    {len(meshes)} dancer meshes, {len(motions)} motion sets ({clips} clips sampled), {len(bad)} problems")
for path, why in bad[:20]:
    print(f"    {os.path.relpath(path, out)}: {why}")
sys.exit(1 if bad else 0)
EOF
done
note "OK"
