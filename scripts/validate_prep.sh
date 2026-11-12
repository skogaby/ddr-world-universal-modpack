#!/usr/bin/env bash
# Offline validation for `prep/` — the StepMania prep tool
# (`ddr_world_hook_prep.exe`), which mounts the hook DLL's pure StepMania
# pipeline from `src/`. Plan: `.agents/planning/2026-10-24-stepmania-prep-tool/`.
#
# Usage:
#   ./scripts/validate_prep.sh                 # cargo test + a smoke dry-run conversion
#   ./scripts/validate_prep.sh <song-or-pack>  # also dry-run that folder into a temp dir
#
# Legs:
#   1. `cargo test` in prep/ (the mounted DLL suites run too — the tool must
#      compile exactly the code the hook ships);
#   2. release build + `--dry-run` of a synthesized .ssc/.wav song: verdict
#      OK, the three artefacts exist, the auto lead-in lands where the rule
#      says (~1.9 s for a beat-0 note at 150 BPM with #OFFSET -0.1);
#   3. the expanded XML arena size is defined once (`ceiling.rs`, mounted
#      here) and imported by the DLL patch (`song_limit_expansion.rs`), so
#      the tool's ceiling report and the game agree;
#   4. (optional) dry-run the folder given on the command line.
#
# Requires: cargo (host toolchain), python3. Writes nothing into the repository.

set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"
TOOL="$REPO_ROOT/prep"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[*] $*"; }

[[ -f "$TOOL/Cargo.toml" ]] || die "tool crate missing: prep/"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

note "leg 1: cargo test in prep/ (mounted DLL suites included)"
(cd "$TOOL" && cargo test --quiet 2>&1 | grep -v '^$' | grep -v 'running 0 tests' | grep -v 'trim-paths' || true)
(cd "$TOOL" && cargo test --quiet >/dev/null 2>&1) || die "prep tests failed"
echo "PASS  tool crate tests"

note "leg 2: release build + smoke --dry-run of a synthesized song"
(cd "$TOOL" && cargo build --quiet --release 2>&1 | grep -v 'trim-paths' || true)
BIN="$TOOL/target/release/ddr_world_hook_prep"
[[ -x "$BIN" ]] || die "release binary missing: prep/target/release/ddr_world_hook_prep"
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
# 10 s of 16-bit stereo 44.1 kHz: a 440 Hz tone.
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
"$BIN" --dry-run "$SONG" --out "$OUT" --lead-in auto | tee "$TMP/report.txt"
grep -q "verdict: OK — 1 chart(s) kept" "$TMP/report.txt" || die "smoke verdict not OK"
CODE="$(sed -n 's/^   \([a-z0-9_]*\)\.xsb: .*/\1/p' "$TMP/report.txt" | head -1)"
[[ -n "$CODE" ]] || die "could not read the basename from the report"
for ext in ssq xsb xwb; do
  [[ -s "$OUT/$CODE.$ext" ]] || die "missing artefact $CODE.$ext"
done
grep -Eq "lead-in: 19[0-9][0-9] ms" "$TMP/report.txt" || die "expected a ~1.9 s auto lead-in"
echo "PASS  smoke dry-run ($CODE.{ssq,xsb,xwb})"

note "leg 3: the expanded XML arena size is one definition shared by the DLL patch and the ceiling"
CEIL_ARENA="$(grep -Eo 'pub const EXPANDED_ARENA_BYTES: usize = 0x[0-9A-Fa-f_]+' src/services/stepmania/ceiling.rs | grep -Eo '0x[0-9A-Fa-f_]+' || true)"
[[ -n "$CEIL_ARENA" ]] || die "ceiling.rs does not define EXPANDED_ARENA_BYTES"
grep -q 'ceiling::EXPANDED_ARENA_BYTES' src/mods/song_limit_expansion.rs || die "song_limit_expansion.rs does not import ceiling::EXPANDED_ARENA_BYTES"
grep -Eq 'EXPANDED_ARENA_BYTES: u32 = 0x' src/mods/song_limit_expansion.rs && die "song_limit_expansion.rs carries its own arena literal"
echo "PASS  expanded arena $CEIL_ARENA defined once (ceiling.rs), imported by the patch"

if [[ $# -ge 1 ]]; then
  note "leg 4: --dry-run of $1"
  "$BIN" --dry-run "$1" --out "$TMP/user_out" --lead-in auto | tail -5
  echo "PASS  user folder dry-run (artefacts in a temp dir, now removed)"
fi

echo "ALL CHECKS PASSED"
