#!/usr/bin/env bash
# Package tools/blender_ddr_addon as an installable Blender extension zip.
# The add-on's format code is the repo's scripts/ktmdl_dump.py + scripts/anm_dump.py;
# this copies them into the package's vendor/ directory so the zip is self-contained.
#
# usage: scripts/build_blender_addon.sh [out-dir]     (default: release/)
# install: Blender > Edit > Preferences > Get Extensions > (v) > Install from Disk...
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$HERE/release}"
SRC="$HERE/tools/blender_ddr_addon"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$SRC/blender_manifest.toml")"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/blender_ddr_addon/vendor" "$OUT"
cp "$SRC"/*.py "$SRC/blender_manifest.toml" "$SRC/README.md" "$STAGE/blender_ddr_addon/"
cp "$HERE/scripts/ktmdl_dump.py" "$HERE/scripts/anm_dump.py" "$STAGE/blender_ddr_addon/vendor/"
ZIP="$OUT/blender_ddr_addon-$VERSION.zip"
rm -f "$ZIP"
(cd "$STAGE" && zip -qr "$ZIP" blender_ddr_addon -x '*/__pycache__/*' '*/tests/*')
echo "wrote ${ZIP/#$HOME/~}"
