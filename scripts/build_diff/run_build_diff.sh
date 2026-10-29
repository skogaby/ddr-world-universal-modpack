#!/usr/bin/env bash
# Cross-build gamemdx diff: which functions changed between two builds, ignoring
# relocation noise (RIP-relative operands, absolute addresses, branch targets).
#
# Pipeline (all outputs land in a scratch dir OUTSIDE the repo — they are
# binary-derived and must never be committed):
#   1. Ghidra headless (-process, -noanalysis) runs GhidraExportInventory.java
#      over each build's program in the Ghidra project → inventory_<build>.json
#      (functions, bodies, callees). The programs must already be analysed in
#      the GUI; the GUI must be CLOSED (project lock).
#   2. pescan (from the sibling dance-dance-rustolution-world repo, built into
#      the scratch dir — that repo is never modified) fingerprints every
#      function position-independently → pescan_<build>.json
#   3. scripts/validate_signatures.sh --json over both DLLs → sig.json
#      (the modpack's real AOB resolver; names the functions and gives
#      build_diff.py its ground truth)
#   4. build_diff.py matches old→new and labels pairs
#   5. report_changes.py renders the Markdown report with names + sigs
#
# Usage:
#   scripts/build_diff/run_build_diff.sh <old-build> <new-build> [<modules-dir>]
#     e.g. scripts/build_diff/run_build_diff.sh 20260915 20261027 ~/Desktop/ddr_modules
#
# Env:
#   GHIDRA_HOME        default /opt/homebrew/opt/ghidra/libexec
#   GHIDRA_PROJECT     default ~/Projects/DDRWorld_Ghidra  (the .gpr sans extension)
#   RUSTOLUTION_DIR    default <repo>/../dance-dance-rustolution-world
#   PESCAN             prebuilt pescan binary (skips the cargo build)
#   BUILD_DIFF_WORK    scratch dir; default ${TMPDIR:-/tmp}/ddr_build_diff
#   SKIP_EXPORT=1      reuse existing inventory_*.json in the scratch dir
#   SKIP_SWEEP=1       skip the signature sweep (no names / no validation)
#
# Programs are expected to be named gamemdx_<build>.dll in the project and the
# DLLs gamemdx_<build>.dll in <modules-dir> (default ~/Desktop/ddr_modules); a
# libafp-win64*.dll must sit alongside them for the sweep's IAT derivations.

set -euo pipefail
cd "$(dirname "$0")/../.."
REPO_ROOT="$(pwd)"
HERE="$REPO_ROOT/scripts/build_diff"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[build_diff] $*" >&2; }

[[ $# -ge 2 ]] || die "usage: $0 <old-build> <new-build> [<modules-dir>]"
OLD="$1"; NEW="$2"
MODULES="${3:-$HOME/Desktop/ddr_modules}"; MODULES="${MODULES/#\~/$HOME}"
[[ -d "$MODULES" ]] || die "modules dir not found: $MODULES"

GHIDRA_HOME="${GHIDRA_HOME:-/opt/homebrew/opt/ghidra/libexec}"
ANALYZE="$GHIDRA_HOME/support/analyzeHeadless"
[[ -x "$ANALYZE" ]] || die "analyzeHeadless not found at $ANALYZE (set GHIDRA_HOME)"
PROJECT="${GHIDRA_PROJECT:-$HOME/Projects/DDRWorld_Ghidra}"; PROJECT="${PROJECT/#\~/$HOME}"
[[ -f "$PROJECT.gpr" ]] || die "ghidra project not found: $PROJECT.gpr (set GHIDRA_PROJECT)"
RUSTOLUTION_DIR="${RUSTOLUTION_DIR:-$REPO_ROOT/../dance-dance-rustolution-world}"

WORK="${BUILD_DIFF_WORK:-${TMPDIR:-/tmp}/ddr_build_diff}"
mkdir -p "$WORK"
note "scratch dir: $WORK"

for b in "$OLD" "$NEW"; do
  [[ -f "$MODULES/gamemdx_$b.dll" ]] || die "missing $MODULES/gamemdx_$b.dll"
done

# ── 1. Ghidra inventories ───────────────────────────────────────────────────
if pgrep -f 'ghidra.GhidraRun' >/dev/null 2>&1; then
  die "the Ghidra GUI is running — close it first (headless needs the project lock)"
fi
for b in "$OLD" "$NEW"; do
  inv="$WORK/inventory_gamemdx_$b.json"
  if [[ "${SKIP_EXPORT:-0}" == 1 && -s "$inv" ]]; then note "reusing $inv"; continue; fi
  note "exporting inventory for gamemdx_$b.dll …"
  "$ANALYZE" "$(dirname "$PROJECT")" "$(basename "$PROJECT")" \
      -process "gamemdx_$b.dll" -noanalysis \
      -scriptPath "$HERE" -postScript GhidraExportInventory.java "$inv" \
      > "$WORK/export_$b.log" 2>&1 || { tail -20 "$WORK/export_$b.log" >&2; die "headless export failed for $b"; }
  [[ -s "$inv" ]] || die "export produced no $inv (see $WORK/export_$b.log)"
done

# ── 2. pescan ───────────────────────────────────────────────────────────────
if [[ -z "${PESCAN:-}" ]]; then
  [[ -d "$RUSTOLUTION_DIR/crates/tools/pescan" ]] || die "pescan crate not found under $RUSTOLUTION_DIR (set RUSTOLUTION_DIR or PESCAN)"
  note "building pescan (target dir inside the scratch dir; the sibling repo is not modified) …"
  ( cd "$RUSTOLUTION_DIR" && CARGO_TARGET_DIR="$WORK/rustolution_target" cargo build -q --release -p rustolution-pescan )
  PESCAN="$(find "$WORK/rustolution_target" -type f -name pescan -path '*release*' | head -1)"
  [[ -x "$PESCAN" ]] || die "pescan binary not found after build"
fi
for b in "$OLD" "$NEW"; do
  note "pescan gamemdx_$b.dll …"
  "$PESCAN" --dll "$MODULES/gamemdx_$b.dll" --inventory "$WORK/inventory_gamemdx_$b.json" \
      --out "$WORK/pescan_gamemdx_$b.json" 2>&1 | tail -1 >&2
done

# ── 3. signature sweep (names + ground truth) ───────────────────────────────
SIG_ARGS=()
if [[ "${SKIP_SWEEP:-0}" != 1 ]]; then
  SIG_IN="$WORK/sig_in_${OLD}_${NEW}"; rm -rf "$SIG_IN"; mkdir -p "$SIG_IN"
  ln -sf "$MODULES/gamemdx_$OLD.dll" "$MODULES/gamemdx_$NEW.dll" "$SIG_IN/"
  afp="$(ls "$MODULES"/libafp-win64*.dll 2>/dev/null | head -1 || true)"
  [[ -n "$afp" ]] && ln -sf "$afp" "$SIG_IN/" || note "WARN no libafp-win64*.dll in $MODULES — IAT derivations will miss"
  SIG="$WORK/sig_${OLD}_${NEW}.json"
  note "signature sweep …"
  ./scripts/validate_signatures.sh "$SIG_IN" --json "$SIG" > "$WORK/sweep_${OLD}_${NEW}.log" 2>&1 || \
      note "sweep reported cross-build gaps (see $WORK/sweep_${OLD}_${NEW}.log) — that IS a finding"
  grep -A3 '^RESULT' "$WORK/sweep_${OLD}_${NEW}.log" >&2 || true
  [[ -s "$SIG" ]] && SIG_ARGS=(--sig "$SIG")
fi

# ── 4 + 5. match and report ─────────────────────────────────────────────────
DIFF="$WORK/diff_${OLD}__${NEW}.json"
TRUTH=(); [[ ${#SIG_ARGS[@]} -gt 0 ]] && TRUTH=(--truth "${SIG_ARGS[1]}" --old-build "$OLD" --new-build "$NEW")
python3 "$HERE/build_diff.py" \
    --old-inv "$WORK/inventory_gamemdx_$OLD.json" --old-pescan "$WORK/pescan_gamemdx_$OLD.json" \
    --new-inv "$WORK/inventory_gamemdx_$NEW.json" --new-pescan "$WORK/pescan_gamemdx_$NEW.json" \
    "${TRUTH[@]}" --out "$DIFF"
REPORT="$WORK/report_${OLD}__${NEW}.md"
python3 "$HERE/report_changes.py" --diff "$DIFF" \
    --old-inv "$WORK/inventory_gamemdx_$OLD.json" --new-inv "$WORK/inventory_gamemdx_$NEW.json" \
    --old-pescan "$WORK/pescan_gamemdx_$OLD.json" --new-pescan "$WORK/pescan_gamemdx_$NEW.json" \
    "${SIG_ARGS[@]}" --out "$REPORT"
note "done. Next: scripts/build_diff/asm_diff.py --diff $DIFF --old $OLD --new $NEW  (normalised disassembly diff of the 'changed' pairs)"
