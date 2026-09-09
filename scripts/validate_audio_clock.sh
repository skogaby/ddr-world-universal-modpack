#!/usr/bin/env bash
# Host tests for the deterministic audio clock's PURE layers (fit + arm/gate
# state machine), mounted from the real sources without retour/Windows.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
printf '[package]\nname="audio-clock-validate"\nversion="0.0.0"\nedition="2021"\n[lib]\npath="lib.rs"\n[workspace]\n' > "$TMP/Cargo.toml"
printf '#![allow(dead_code)]\n#[path="%s/src/services/audio_clock/fit.rs"]\npub mod fit;\n#[path="%s/src/services/audio_clock/onset.rs"]\npub mod onset;\n#[path="%s/src/services/audio_clock/seqpub.rs"]\npub mod seqpub;\n' "$ROOT" "$ROOT" "$ROOT" > "$TMP/lib.rs"
cargo test --manifest-path "$TMP/Cargo.toml" --quiet
