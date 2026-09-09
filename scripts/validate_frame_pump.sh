#!/usr/bin/env bash
# Run the actual dependency-free frame scheduler on the host, without retour.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
rustc --edition=2021 --test "$ROOT/src/core/frame_pump.rs" -o "$TMP/frame-pump-tests"
"$TMP/frame-pump-tests"
