#!/usr/bin/env bash
# Offline Multiplayer Bot simulator — runs the DLL's real skill/planner code
# against a directory of DDR World SSQ charts through a model of the game's
# judge, gauge and scoring, and writes a self-contained HTML report with
# per-level overviews and synthesized scorecards (tools/bot_sim/).
#
# Usage:
#   ./scripts/bot_sim.sh <ssq-dir> [options]          # see --help
#   ./scripts/bot_sim.sh "$DDR_WORLD_INSTALL/data/mdb_apx/ssq" --seeds 3 --out /tmp/bot_sim.html
#
# Repeatable and offline: the native crate is built once (release) and
# cached; a full 1,586-file corpus × 5 difficulties × 10 levels × 1 seed
# runs in well under a minute on a laptop. Nothing is written into the
# repository (the report path defaults to ./bot_sim_report.html — gitignored).
set -euo pipefail
cd "$(dirname "$0")/.."
exec cargo run --quiet --release --manifest-path tools/bot_sim/Cargo.toml -- "$@"
