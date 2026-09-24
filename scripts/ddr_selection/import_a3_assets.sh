#!/usr/bin/env bash
# DDR SELECTION — copy the DDR A3 files the feature needs into a DDR World
# install's LayeredFS mod folder.
#
# Usage:
#   import_a3_assets.sh <A3 install> [<World install>]
#
# Each path is the folder that CONTAINS `data/`. <World install> defaults to
# the folder above this script's folder (the release ships the script in
# <game>/ddr_selection_import/). Files go to
#   <World install>/data_mods/ddr_selection_a3/<path relative to data/>
# — nothing under World's own data/ is touched.
#
# a3_assets.manifest (next to this script) lists the files: `always` entries
# are copied every run (today only arc/bm2d/dance_combo0005_v0.arc, whose
# World copy is blanked); `missing` entries only when World has no copy of its
# own (a stock World install has them all). The repository never contains A3
# data — only this script and the manifest.

set -euo pipefail

usage() {
  echo "usage: $0 <A3 install> [<World install>]  (each = the folder that contains data/)" >&2
  exit 2
}
[[ $# -ge 1 && $# -le 2 ]] || usage

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
A3_ROOT="${1%/}"
WORLD_ROOT="${2:-$SCRIPT_DIR/..}"
WORLD_ROOT="${WORLD_ROOT%/}"
MANIFEST="$SCRIPT_DIR/a3_assets.manifest"

[[ -r "$MANIFEST" ]] || { echo "error: a3_assets.manifest not found next to the script" >&2; exit 2; }
[[ -d "$A3_ROOT/data" ]] || { echo "error: '$A3_ROOT' has no data/ folder (A3 install)" >&2; exit 2; }
[[ -d "$WORLD_ROOT/data" ]] || { echo "error: '$WORLD_ROOT' has no data/ folder (World install)" >&2; exit 2; }

DEST="$WORLD_ROOT/data_mods/ddr_selection_a3"
copied=0
failed=0
while read -r mode rel; do
  [[ -z "${mode:-}" || "$mode" == \#* ]] && continue
  if [[ "$mode" == "missing" && -f "$WORLD_ROOT/data/$rel" ]]; then
    continue
  fi
  if [[ ! -f "$A3_ROOT/data/$rel" ]]; then
    echo "  MISSING IN A3  $rel"
    failed=$((failed + 1))
    continue
  fi
  mkdir -p "$(dirname "$DEST/$rel")"
  cp "$A3_ROOT/data/$rel" "$DEST/$rel"
  echo "  copied  $rel"
  copied=$((copied + 1))
done < "$MANIFEST"

echo "done: $copied file(s) copied to data_mods/ddr_selection_a3/, $failed missing from the A3 install"
[[ $failed -eq 0 ]]
