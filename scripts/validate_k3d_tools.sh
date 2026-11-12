#!/usr/bin/env bash
# Offline validation for the K3D dancer tools (DDR ULTRAMIX / Dancing Stage Unleashed on Xbox, DDR
# Universe on Xbox 360): scripts/ultramix_k3d_dump.py and the disc reader scripts/xdvdfs_extract.py.
# Formats and RE: docs/dancing_stage_unleashed_dancers_port_feasibility.md.
#
# Usage:
#   ./scripts/validate_k3d_tools.sh [extracted-x_data-dir ...]
#
# Always runs the host tests (scripts/test_k3d_formats.py: `.ddm` revision 0x112 with one and many
# palette groups, odd index padding, zero-normal repair, the Universe parent table, XDVDFS listing
# for a plain XISO and a partition-offset image). With extracted dirs
# (scripts/extract_ultramix_data.py output) it adds a `survey`: every `srdd` model parsed with its
# weights and parent table checked, every `.ani` clip parsed.
#
# Requires: python3 + numpy. Writes nothing into the repo.

set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

python3 -c "import numpy" 2>/dev/null || { echo "error: numpy is required" >&2; exit 1; }
note "running host tests (scripts/test_k3d_formats.py)"
(cd scripts && PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q test_k3d_formats)

if [[ $# -gt 0 ]]; then
  note "surveying $*"
  PYTHONDONTWRITEBYTECODE=1 python3 scripts/ultramix_k3d_dump.py survey "$@"
fi
note "OK"
