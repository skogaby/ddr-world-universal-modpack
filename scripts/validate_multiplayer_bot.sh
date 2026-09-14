#!/usr/bin/env bash
# Offline validation for the Multiplayer Bot's pure layer.
#
# The simulator crate `tools/bot_sim/` mounts the DLL's REAL pure modules via
# `#[path]` (`services/foot_panel_swap/layout.rs`,
# `mods/multiplayer_bot/{eligibility,skill,planner}.rs`, `core/ssq/*`), so
# `cargo test` there runs their `#[cfg(test)]` suites AND the tool's own
# judge-model / gauge / scoring / chart-parser tests — no throwaway temp crate
# needed (plain `cargo test` on the DLL crate cannot compile `retour` on
# non-x86 hosts).
#
# Usage:
#   ./scripts/validate_multiplayer_bot.sh              # all host tests
#   ./scripts/validate_multiplayer_bot.sh --report <ssq-dir> [bot_sim options]
#                                                      # + a full simulation report
set -euo pipefail
cd "$(dirname "$0")/.."

note() { echo "[*] $*"; }

note "running pure-module + simulator tests (tools/bot_sim)"
cargo test --quiet --manifest-path tools/bot_sim/Cargo.toml
note "OK"

if [[ "${1:-}" == "--report" ]]; then
  shift
  [[ $# -ge 1 ]] || { echo "error: --report needs <ssq-dir>" >&2; exit 2; }
  note "simulating $1"
  exec ./scripts/bot_sim.sh "$@"
fi
