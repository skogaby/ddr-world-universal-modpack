#!/usr/bin/env bash
# Offline validation for the DDR for Windows (KCEA 2002) dancer tools: scripts/ddrpc_dancer_dump.py.
# Formats and RE: docs/ddr_pc_dancers_research.md.
#
# Usage:
#   ./scripts/validate_ddrpc_tools.sh [game-dir [character_dll-dir]]
#
# Always runs the host tests (scripts/test_ddrpc_formats.py: BMP decode + colour key, D3D -> World
# space, rig order, clip timing / recentre / retarget, .anm spec, atlas packing). With a game dir
# (DanceDanceRevolution.exe + data.bin) it adds a `survey`: every character model parsed and
# index-checked, every routine's matrices checked for orthonormality.
#
# Requires: python3 + numpy. Writes nothing into the repo.

set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

python3 -c "import numpy" 2>/dev/null || { echo "error: numpy is required" >&2; exit 1; }
note "running host tests (scripts/test_ddrpc_formats.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q test_ddrpc_formats)

if [[ $# -gt 0 ]]; then
  note "surveying $1"
  PYTHONDONTWRITEBYTECODE=1 python3 scripts/ddrpc_dancer_dump.py survey "$@"
fi
note "OK"
