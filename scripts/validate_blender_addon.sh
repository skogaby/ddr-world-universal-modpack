#!/usr/bin/env bash
# Headless smoke test of the Blender add-on in tools/blender_ddr_addon against real
# DDR A3 data (unpacked arcs — see docs/3d_model_format_research.md §1 for the layout).
#
# usage: scripts/validate_blender_addon.sh <unpacked-data-root> [out.blend]
#   <unpacked-data-root> must contain chara/pl_emi00/, chara/pl_rinon00(_*)/, chara/mc_female/
#   and camera/music/*. The character test also wants chara_resources.rlist: set DDR_3D_RLIST,
#   or unpack startup.arc to <unpacked-data-root>/../startup/ (default lookup).
#   Set BLENDER to the Blender binary if it is not on PATH / in /Applications.
#   Export round-trip files land in DDR_3D_OUT_DIR (default <data-root>/../blender_export_test).
set -euo pipefail

DATA="${1:?usage: $0 <unpacked-data-root> [out.blend]}"
OUT="${2:-}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"

if [ -z "${BLENDER:-}" ]; then
  if command -v blender >/dev/null 2>&1; then
    BLENDER=blender
  elif [ -x /Applications/Blender.app/Contents/MacOS/Blender ]; then
    BLENDER=/Applications/Blender.app/Contents/MacOS/Blender
  else
    echo "Blender not found; set BLENDER=/path/to/blender" >&2
    exit 2
  fi
fi

export DDR_3D_DATA="$DATA"
[ -n "$OUT" ] && export DDR_3D_OUT_BLEND="$OUT"
rc_total=0
for test in smoke_test character_test synthetic_test; do
  echo "== $test"
  set +e
  "$BLENDER" --background --factory-startup --python-exit-code 1 \
    --python "$HERE/tools/blender_ddr_addon/tests/$test.py" 2>&1 | grep -vE '^(Blender [0-9]|Read prefs|$)'
  rc=${PIPESTATUS[0]}
  set -e
  if [ "$rc" -ne 0 ]; then echo "$test: FAIL (rc=$rc)" >&2; rc_total=1; fi
done
if [ "$rc_total" -eq 0 ]; then echo "validate_blender_addon: PASS"; else echo "validate_blender_addon: FAIL" >&2; exit 1; fi
