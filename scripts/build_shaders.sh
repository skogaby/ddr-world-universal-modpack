#!/usr/bin/env bash
# Build the mod's shader BLOBS: HLSL -> d3dbc, committed at
# data_mods/shader_fixes/blobs/.
#
# Usage:
#   ./scripts/build_shaders.sh            # build all blobs in the manifest
#   ./scripts/build_shaders.sh --check    # toolchain smoke test only
#   ./scripts/build_shaders.sh --vkd3d    # force the Docker/vkd3d fallback
#
# The .gsp containers themselves are SYNTHESIZED AT RUNTIME by the hook DLL
# (services/avs_layeredfs/shader_synthesis.rs) from the game's own stock
# shader.arc blobs plus these committed mod blobs — no Konami bytecode is
# committed to the repo, and program 0 of every touched container uses the
# game's own stock VS (bit-identical stock behavior with anti-aliasing off).
# scripts/gsp_pack.py remains the offline inspect/selftest/dev-packing tool.
#
# ── Compiler golden path: fxc (tools/fxc/, under wine) ──────────────────
# Microsoft fxc 9.29.952.3111 — the exact compiler lineage the game's stock
# shaders were built with — is checked into the repo at tools/fxc/ and runs
# under the CrossOver bottle. Its SM3 codegen is ~4.4x smaller than
# vkd3d-compiler's for our pixel shaders (see
# .agents/planning/20260721-player-perspective-hallway/research/fxc-performance.md).
#
# Fallback: the Docker/vkd3d path (--vkd3d flag, or automatic when no wine
# binary is found). vkd3d output is functionally identical but unoptimized
# (~4-7x the instruction count) — fine for development, not for committing.
#
# Neither toolchain is needed for normal DLL builds/deploys — the .d3dbc
# blob outputs are committed.
# ─────────────────────────────────────────────────────────────────────────

set -euo pipefail
cd "$(dirname "$0")/.."

# Reproducibility pins: committed blob hashes were produced with these
# compiler versions. On drift, either match the pinned version or
# intentionally re-bless the committed artifacts (rebuild + review + commit).
FXC_VERSION_PIN="9.29.952.3111"
VKD3D_VERSION_PIN="1.14"

FXC_EXE=tools/fxc/fxc.exe
WINE="${WINE:-/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin/wine}"
WINE_BOTTLE="${WINE_BOTTLE:-bemani}"
IMAGE=ddr-shader-build

SRC_DIR=shaders/src
OUT_DIR=data_mods/shader_fixes/blobs

# Blob manifest: "<hlsl name>:<profile>:<entry point>:<output blob>".
# - the two AA pixel shaders (program 0's PS when ANTI-ALIASING is on;
#   always the PS of the arrow perspective program)
# - the two perspective vertex shaders (program 1 of arrow/default)
# The stock-replica vs_main entries are RETIRED: program 0 always uses the
# game's own stock VS blob, sliced from shader.arc at synthesis time.
BLOBS=(
  "gs_screencommand_arrow:ps_3_0:ps_main:gs_screencommand_arrow.ps.d3dbc"
  "gs_screencommand_arrow:vs_3_0:vs_persp_main:gs_screencommand_arrow.vs_persp.d3dbc"
  "gs_screencommand_judge:ps_3_0:ps_main:gs_screencommand_judge.ps.d3dbc"
  "gs_screencommand_default:vs_3_0:vs_persp_main:gs_screencommand_default.vs_persp.d3dbc"
  # Mod-menu animated backgrounds (overlay-menu rewrite Step 8; Shadertoy
  # theme pack 2026-08-25): one shared passthrough VS + one PS per
  # shader-backed theme, appended to the default container by
  # shader_synthesis. ORDER MATTERS: it must match ThemeProgram::slot()
  # (theme.rs) and THEME_BLOBS (shader_synthesis.rs).
  "themes/theme_common:vs_3_0:vs_theme_main:theme_passthrough.vs.d3dbc"
  "themes/theme_bubbles:ps_3_0:ps_main:theme_bubbles.ps.d3dbc"
  "themes/theme_terminal:ps_3_0:ps_main:theme_terminal.ps.d3dbc"
  "themes/theme_waveform:ps_3_0:ps_main:theme_waveform.ps.d3dbc"
  "themes/theme_spectrum:ps_3_0:ps_main:theme_spectrum.ps.d3dbc"
  "themes/theme_tunnel:ps_3_0:ps_main:theme_tunnel.ps.d3dbc"
  "themes/theme_xmb:ps_3_0:ps_main:theme_xmb.ps.d3dbc"
  "themes/theme_squares:ps_3_0:ps_main:theme_squares.ps.d3dbc"
  "themes/theme_card_swirl:ps_3_0:ps_main:theme_card_swirl.ps.d3dbc"
  "themes/theme_blobs:ps_3_0:ps_main:theme_blobs.ps.d3dbc"
  "themes/theme_ps2:ps_3_0:ps_main:theme_ps2.ps.d3dbc"
  "themes/theme_prime_cube:ps_3_0:ps_main:theme_prime_cube.ps.d3dbc"
)

die() { echo "error: $*" >&2; exit 1; }

# ── Compiler selection ───────────────────────────────────────────────────
BACKEND=fxc
if [[ "${1:-}" == "--vkd3d" ]]; then
  BACKEND=vkd3d
  shift || true
elif [[ ! -x "$WINE" ]]; then
  echo "[*] no wine at $WINE — falling back to Docker/vkd3d" >&2
  BACKEND=vkd3d
fi

if [[ "$BACKEND" == "fxc" ]]; then
  [[ -f "$FXC_EXE" ]] || die "missing $FXC_EXE (repo-committed fxc binaries)"
  # Version pin check (banner line 1). NB: fxc rejects leading-/ POSIX paths
  # as options — all paths handed to it below must be repo-relative.
  # (|| true: head -1 SIGPIPEs wine, which set -o pipefail would turn fatal.)
  ver=$("$WINE" --bottle "$WINE_BOTTLE" "$FXC_EXE" '/?' 2>/dev/null \
        | sed -n 's/.*Shader Compiler \([0-9.]*\).*/\1/p' | head -1 || true)
  [[ "$ver" == "$FXC_VERSION_PIN" ]] \
    || die "fxc version drift: got '$ver', pinned '$FXC_VERSION_PIN'"
  compile() { # compile <profile> <entry> <hlsl> <out>
    "$WINE" --bottle "$WINE_BOTTLE" "$FXC_EXE" /nologo \
        /T "$1" /E "$2" /Fo "$4" "$3" 2>/dev/null | grep -v '^msync:' || true
    [[ -s "$4" ]] || die "fxc produced no output for $3 ($2)"
  }
else
  command -v docker >/dev/null || die "docker not found (required for the vkd3d fallback)"
  docker info >/dev/null 2>&1 || die "docker daemon not running"
  if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "[*] building toolchain image '$IMAGE'..."
    docker build -t "$IMAGE" shaders/
  fi
  ver=$(docker run --rm "$IMAGE" vkd3d-compiler --version | sed -n 's/.*version \([0-9.]*\).*/\1/p' | head -1)
  [[ "$ver" == "$VKD3D_VERSION_PIN" ]] \
    || die "vkd3d-compiler version drift: got '$ver', pinned '$VKD3D_VERSION_PIN'"
  compile() { # compile <profile> <entry> <hlsl> <out>
    docker run --rm -v "$PWD":/work -w /work "$IMAGE" \
      vkd3d-compiler -x hlsl -b d3dbc -e "$2" --profile "$1" "$3" -o "$4"
  }
fi
echo "[*] compiler backend: $BACKEND $ver"

if [[ "${1:-}" == "--check" ]]; then
  echo "[*] toolchain check: compiling a trivial ps_3_0..."
  printf 'float4 ps_main() : COLOR { return float4(1,0,0,1); }\n' > .build_shaders_check.hlsl
  compile ps_3_0 ps_main .build_shaders_check.hlsl .build_shaders_check.d3dbc
  python3 - <<'EOF'
import struct
tok = struct.unpack_from("<I", open(".build_shaders_check.d3dbc","rb").read(), 0)[0]
assert tok >> 16 == 0xFFFF and (tok >> 8) & 0xFF == 3, f"not ps_3_0: 0x{tok:08X}"
print("[*] round-trip token check OK (ps_3_0)")
EOF
  rm -f .build_shaders_check.*
  python3 scripts/gsp_pack.py selftest
  echo "[+] toolchain OK ($BACKEND $ver, gsp_pack selftest passed)"
  exit 0
fi

mkdir -p "$OUT_DIR"

for spec in "${BLOBS[@]}"; do
  IFS=':' read -r name profile entry out <<< "$spec"
  hlsl="$SRC_DIR/$name.hlsl"
  [[ -f "$hlsl" ]] || die "missing $hlsl"
  echo "[*] $out  ($name.hlsl $entry/$profile)"
  compile "$profile" "$entry" "$hlsl" "$OUT_DIR/$out"
done

echo
echo "[+] blob stats:"
python3 - "$OUT_DIR" <<'EOF'
import sys, os, hashlib
sys.path.insert(0, "scripts")
from gsp_pack import sm3_instr_count, blob_kind
d = sys.argv[1]
for f in sorted(os.listdir(d)):
    if not f.endswith(".d3dbc"):
        continue
    blob = open(os.path.join(d, f), "rb").read()
    n, tex = sm3_instr_count(blob)
    extra = f", {tex} texld" if blob_kind(blob).startswith("ps_") else ""
    print(f"  {f}: {len(blob)} B {blob_kind(blob)} ({n} instr{extra}) sha256={hashlib.sha256(blob).hexdigest()[:16]}")
EOF
