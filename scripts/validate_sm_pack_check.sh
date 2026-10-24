#!/usr/bin/env bash
# Offline validation for `tools/sm_pack_check/` — the StepMania → DDR dry-run
# converter (plan `.agents/planning/2026-10-20-stepmania-pack-loader/
# implementation/plan.md` Step 5, task 03).
#
# Usage:
#   ./scripts/validate_sm_pack_check.sh                 # cargo test + a smoke conversion
#   ./scripts/validate_sm_pack_check.sh <song-or-pack>  # also convert that folder into a temp dir
#
# What it does:
#   1. `cargo test` in the tool crate — its own tests plus every `#[cfg(test)]`
#      suite of the DLL modules it mounts (`core::sm`, `core::ssq`,
#      `core::audio`, `core::xact`);
#   2. builds the release binary and converts a synthesized one-chart song
#      (SSC + 16-bit WAV written into a temp dir) with `--lead-in auto`,
#      asserting the three artefacts appear and the verdict says OK;
#   3. auto-sync parity: when `$DDR_WORLD_INSTALL/stepmania/Songs/Stamina RPG 5`
#      and the sibling CLI (`../ddr-chart-tools/target/{release,debug}/
#      ddr-chart-tools`) exist, measures >= 5 songs (`SM_SYNC_PARITY_SONGS`,
#      default 6) with `sm-pack-check --auto-sync` (lead-in auto AND a forced
#      3 s lead-in) and with the CLI's `--auto-sync report`, and asserts
#      |correction_ms difference| <= 1 ms (SKIP otherwise);
#   4. optionally converts the folder given on the command line (prints the
#      tool's report; nothing is kept).
#
# Requires: cargo (host toolchain). No Windows toolchain; the game install
# is only READ (leg 3).
# Validation only — writes nothing into the repository.

set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"
TOOL="$REPO_ROOT/tools/sm_pack_check"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[*] $*"; }

[[ -f "$TOOL/Cargo.toml" ]] || die "tool crate missing: tools/sm_pack_check"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

note "leg 1: cargo test in tools/sm_pack_check (mounted DLL suites included)"
(cd "$TOOL" && cargo test --quiet 2>&1 | grep -v '^$' | grep -v 'running 0 tests')
echo "PASS  tool crate tests"

note "leg 2: release build + smoke conversion of a synthesized song"
(cd "$TOOL" && cargo build --quiet --release)
BIN="$TOOL/target/release/sm-pack-check"
SONG="$TMP/Pack/Smoke Song"
mkdir -p "$SONG"
cat >"$SONG/smoke.ssc" <<'SSC'
#VERSION:0.83;
#TITLE:Smoke;
#ARTIST:harness;
#MUSIC:smoke.wav;
#OFFSET:-0.100;
#SAMPLESTART:1.0;
#SAMPLELENGTH:6.0;
#BPMS:0.000=150.000;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Hard;
#METER:9;
#NOTES:
1000
0100
0010
0001
,
1100
0000
0011
0000
;
SSC
# 10 s of 16-bit stereo 44.1 kHz: a 440 Hz tone, written with python.
python3 - "$SONG/smoke.wav" <<'PY'
import math, struct, sys
rate, secs = 44100, 10
n = rate * secs
data = bytearray()
for i in range(n):
    s = int(8000 * math.sin(2 * math.pi * 440 * i / rate))
    data += struct.pack('<hh', s, s)
hdr = b'RIFF' + struct.pack('<I', 36 + len(data)) + b'WAVE' + b'fmt ' + struct.pack('<IHHIIHH', 16, 1, 2, rate, rate * 4, 4, 16) + b'data' + struct.pack('<I', len(data))
open(sys.argv[1], 'wb').write(hdr + data)
PY
OUT="$TMP/out"
"$BIN" "$SONG" --out "$OUT" --lead-in auto | tee "$TMP/report.txt"
grep -q "verdict: OK — 1 chart(s) kept" "$TMP/report.txt" || die "smoke verdict not OK"
CODE="$(sed -n 's/^   \([a-z0-9_]*\)\.xsb: .*/\1/p' "$TMP/report.txt" | head -1)"
[[ -n "$CODE" ]] || die "could not read the basename from the report"
for ext in ssq xsb xwb; do
  [[ -s "$OUT/$CODE.$ext" ]] || die "missing artefact $CODE.$ext"
done
# A beat-0 first note at 150 BPM with #OFFSET −0.1 needs ≥ 1.9 s of lead-in.
grep -Eq "lead-in: 19[0-9][0-9] ms" "$TMP/report.txt" || die "expected a ~1.9 s auto lead-in"
echo "PASS  smoke conversion ($CODE.{ssq,xsb,xwb})"

note "leg 3: auto-sync parity vs the ddr-chart-tools CLI (--auto-sync report) on Stamina RPG 5"
PARITY_PACK="${DDR_WORLD_INSTALL:-}/stepmania/Songs/Stamina RPG 5"
CLI=""
for cand in "$REPO_ROOT/../ddr-chart-tools/target/release/ddr-chart-tools" "$REPO_ROOT/../ddr-chart-tools/target/debug/ddr-chart-tools"; do
  if [[ -x "$cand" ]]; then CLI="$cand"; break; fi
done
if [[ -z "${DDR_WORLD_INSTALL:-}" || ! -d "$PARITY_PACK" ]]; then
  echo "SKIP  auto-sync parity: \$DDR_WORLD_INSTALL/stepmania/Songs/Stamina RPG 5 not present"
elif [[ -z "$CLI" ]]; then
  echo "SKIP  auto-sync parity: ../ddr-chart-tools/target/{release,debug}/ddr-chart-tools not built"
else
  PARITY_N="${SM_SYNC_PARITY_SONGS:-6}"
  checked=0
  worst="0"
  printf '      %-44s %9s %9s %9s %9s\n' "song" "cli" "ours" "ours+3s" "max|d|"
  while IFS= read -r -d '' dir; do
    [[ $checked -ge $PARITY_N ]] && break
    sims=("$dir"/*.sm)
    oggs=("$dir"/*.ogg)
    [[ ${#sims[@]} -eq 1 && -f "${sims[0]}" && ${#oggs[@]} -eq 1 && -f "${oggs[0]}" ]] || continue
    name="$(basename "$dir")"
    cli_out="$("$CLI" --from-format SM5 --to-format DDR --chartfile "${sims[0]}" --audiofile "${oggs[0]}" \
      --output-dir "$TMP/cli_$checked" --song-code parity --overwrite --auto-sync report 2>&1 || true)"
    cli_c="$(sed -n 's/.*auto_sync=.* correction_ms=\(-\{0,1\}[0-9.]*\).*/\1/p' <<<"$cli_out" | head -1)"
    ours_out="$("$BIN" "$dir" --out "$TMP/ours_$checked" --lead-in auto --auto-sync 2>&1 || true)"
    ours_c="$(sed -n 's/.*auto-sync [a-z0-9_]*\.ssq:.* correction_ms=\(-\{0,1\}[0-9.]*\).*/\1/p' <<<"$ours_out" | head -1)"
    # The same song with a 3 s lead-in: the time-base subtraction must hold.
    lead_out="$("$BIN" "$dir" --out "$TMP/lead_$checked" --lead-in 3000 --auto-sync 2>&1 || true)"
    lead_c="$(sed -n 's/.*auto-sync [a-z0-9_]*\.ssq:.* correction_ms=\(-\{0,1\}[0-9.]*\).*/\1/p' <<<"$lead_out" | head -1)"
    if [[ -z "$cli_c" || -z "$ours_c" || -z "$lead_c" ]]; then
      echo "      $name: no correction measured (cli='$cli_c' ours='$ours_c' lead='$lead_c') -- skipped"
      continue
    fi
    d="$(python3 -c "import sys; c,o,l=map(float,sys.argv[1:]); print(f'{max(abs(o-c),abs(l-c)):.2f}')" "$cli_c" "$ours_c" "$lead_c")"
    printf '      %-44s %9s %9s %9s %9s\n' "${name:0:44}" "$cli_c" "$ours_c" "$lead_c" "$d"
    worst="$(python3 -c "import sys; print(max(float(sys.argv[1]), float(sys.argv[2])))" "$worst" "$d")"
    checked=$((checked + 1))
  done < <(find "$PARITY_PACK" -mindepth 1 -maxdepth 1 -type d -print0 | sort -z)
  [[ $checked -ge 5 ]] || die "auto-sync parity: only $checked song(s) measurable (need >= 5)"
  python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) <= 1.0 else 1)" "$worst" \
    || die "auto-sync parity: |correction delta| $worst ms > 1 ms"
  echo "PASS  auto-sync parity: $checked song(s), max |ours - cli| = $worst ms (auto and 3 s lead-in)"
fi

if [[ $# -ge 1 ]]; then
  note "leg 4: converting $1"
  "$BIN" "$1" --out "$TMP/user_out" --lead-in auto
  echo "PASS  user folder converted"
fi

echo "ALL CHECKS PASSED"
