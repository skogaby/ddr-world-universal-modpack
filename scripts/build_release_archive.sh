#!/usr/bin/env bash
set -euo pipefail
# Build a GitHub release archive containing the Windows 7 build of the DLL
# plus the runtime data files an operator needs.
#
# Output: ddr-world-universal-modpack-YYYYMMDD.zip (in the repo root)

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

DLL="target/x86_64-win7-windows-msvc/release/ddr_world_hook.dll"
ARCHIVE="ddr-world-universal-modpack-$(date +%Y%m%d).zip"

echo "==> Building Windows 7 DLL"
# Windows 7 compatible build (inlined from build_win7.sh). The default
# x86_64-pc-windows-msvc target imports ProcessPrng from bcryptprimitives.dll,
# which doesn't exist on Win7 and causes the loader to reject the DLL with
# "The procedure entry point ProcessPrng could not be located". The
# x86_64-win7-windows-msvc target uses RtlGenRandom instead. It's a tier 3
# target, so std must be built from source via -Z build-std.
cargo xwin build \
    --release \
    --target x86_64-win7-windows-msvc \
    -Z build-std=std,panic_abort

if [[ ! -f "$DLL" ]]; then
    echo "ERROR: expected build output not found: $DLL" >&2
    exit 1
fi

echo "==> Staging release contents"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp "$DLL" "$STAGE/"
cp mod-config.json judgement_offsets.csv README.md "$STAGE/"

# data_mods/ minus runtime-generated artifacts (never ship these):
#   _cache/    - runtime caches (step-data, shader synthesis)
#   *_ifs/     - enable-time generated IFS dirs (s_marvelous etc.)
#   *.arc      - generated ARC packages
rsync -a \
    --exclude '.DS_Store' \
    data_mods "$STAGE/"

echo "==> Creating $ARCHIVE"
rm -f "$ARCHIVE"
(cd "$STAGE" && zip -r -q "$REPO_ROOT/$ARCHIVE" .)

echo "Done: $REPO_ROOT/$ARCHIVE"
unzip -l "$ARCHIVE" | tail -3
