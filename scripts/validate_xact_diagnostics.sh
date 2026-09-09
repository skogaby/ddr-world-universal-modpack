#!/usr/bin/env bash
# Host tests mount the actual XACT diagnostic models and shared scanner.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export XACT_DIAG_BINARY="${XACT_DIAG_BINARY:-${DDR_WORLD_INSTALL:?set DDR_WORLD_INSTALL}/com/xactengine2_10.dll}"
export XACT_DIAG_GAME_DIR="${XACT_DIAG_GAME_DIR:-$HOME/Desktop/ddr_modules}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
printf '[package]\nname="xact-diagnostics-validate"\nversion="0.0.0"\nedition="2021"\n[lib]\npath="lib.rs"\n[dependencies]\naho-corasick="1"\n[workspace]\n' > "$TMP/Cargo.toml"
printf '#![allow(dead_code,unused_imports,unused_variables)]\n#[macro_export] macro_rules! log_warn { ($($arg:tt)*) => {}; }\npub mod core { pub mod profiling { pub fn record_scan_pattern(_: &str, _: std::time::Duration) {} pub fn record_scan_pattern_all(_: &str, _: std::time::Duration) {} pub fn record_scan_batch(_: usize, _: usize, _: std::time::Duration) {} } #[path="%s/src/core/scanner.rs"] pub mod scanner; }\n#[path="%s/src/services/audio_sync_diag/xact_model.rs"] mod xact_model;\n#[path="%s/src/services/audio_sync_diag/xact_sites.rs"] mod xact_sites;\n' "$ROOT" "$ROOT" "$ROOT" > "$TMP/lib.rs"
cargo test --manifest-path "$TMP/Cargo.toml" -- --nocapture
