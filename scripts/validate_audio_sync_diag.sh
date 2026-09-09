#!/usr/bin/env bash
# Mount the actual dependency-free diagnostic model, without retour/Windows.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
printf '[package]\nname="audio-sync-diag-validate"\nversion="0.0.0"\nedition="2021"\n[lib]\npath="lib.rs"\n[workspace]\n' > "$TMP/Cargo.toml"
printf '#![allow(dead_code)]\n#[path="%s/src/services/audio_sync_diag/model.rs"]\nmod model;\n#[path="%s/src/services/audio_sync_diag/spans.rs"]\nmod spans;\n' "$ROOT" "$ROOT" > "$TMP/lib.rs"
printf 'pub mod core { #[path="%s/src/core/frame_pump.rs"] pub mod frame_pump; }\n' "$ROOT" >> "$TMP/lib.rs"
cargo test --manifest-path "$TMP/Cargo.toml" --quiet
