#!/usr/bin/env bash
# Offline validation for the System 573 (DDR 3rdMIX PLUS .. EXTREME) asset tools:
# scripts/extract_sys573_data.py, scripts/sys573_dancer_dump.py, scripts/sys573_video.py.
# Formats and RE: docs/sys573_dancers_research.md.
#
# Usage:
#   ./scripts/validate_sys573_tools.sh [extracted-mix-dir]...
#
# Always runs the host tests (scripts/test_sys573_formats.py: name hash + layout solver,
# LZ / cipher, .cmm key sampling, PSX rotation order, draw selection, glTF quaternions,
# MDEC VLC + DC decode). Each extracted-mix-dir argument (the --out of
# `extract_sys573_data.py extract`) adds a `survey`: every dancer model parsed and
# index-checked, every clip of every .cmm sampled.
#
# Requires: python3 + numpy (+ Pillow for the texture paths). Writes nothing into the repo.

set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

python3 -c "import numpy" 2>/dev/null || { echo "error: numpy is required" >&2; exit 1; }
note "running host tests (scripts/test_sys573_formats.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q test_sys573_formats)

if [[ $# -gt 0 ]]; then
  note "surveying extracted mixes"
  PYTHONDONTWRITEBYTECODE=1 python3 scripts/sys573_dancer_dump.py survey "$@"
fi
note "OK"
