#!/usr/bin/env bash
# Offline validation for the Background Dancers feature's pure layers.
#
# Usage:
#   ./scripts/validate_background_dancers.sh
#
# Why a harness: plain `cargo test` cannot run on non-x86 hosts (the `retour`
# dependency only compiles for x86/x86_64), so — like
# validate_two_player_bpl.sh — this builds a throwaway HOST cargo crate in a
# temp directory that mounts the feature's dependency-free modules via
# `#[path]` and runs their `#[cfg(test)]` suites there.
#
# Mounted modules grow with the plan steps:
#   Step 2  services/scene3d/pure.rs               (arc path resolution, FNV-1 model hash)
#   Step 3  services/scene3d/render_item_layout.rs (item/record/material layout + trailing math)
#           services/scene3d/node_layout.rs        (SceneNode offset_of pins, visible push)
#   Step 7  services/scene3d/frame_board.rs      (seqlocked pose channel game thread → visit(2))
#           mods/background_dancers/{director_math,clock,tempo}.rs (placement/clip-frame math, the song clock latch, the tempo-map dance clock)
#   Step 5  core/anm/mod.rs                        (ANM/CAMANM/B2IT/MRL0/KTMDL codecs — a DIRECTORY
#                                                    module: mount mod.rs, its children resolve beside it)
#   Step 6  mods/background_dancers/{selection,schedule}.rs (picks, playlists, dance/camera timelines;
#                                                    schedule reaches selection::Rng via `super::` — both
#                                                    mount at the crate root, as in the DLL's mod dir)
#   2026-09-21 mods/background_dancers/outline.rs   (scene-outline plan; since 2026-09-28 DSU's one black hull)
#   2026-09-21 services/scene3d/camera_math.rs      (the engine's LookAtRH view + D3D off-centre projection, row-vector)
#   2026-09-21 services/scene3d/viewport_pass_layout.rs (ClearViewport repr(C) pins, gd Clear record bytes, canvas→RT rect, prio/bit tables)
#   2026-09-21 mods/background_dancers/catalog.rs   (option-row catalog: sorted keys → ≤15-byte labels, load clamp;
#                                                    reaches selection::{StageCandidate,DancerCandidate} via `super::`)
#   2026-09-21 mods/background_dancers/pick.rs      (the Pick — optional stage / empty dancers, arc lists, summary line,
#                                                    ParseOptions; reaches selection via `super::`)
#   2026-09-21 mods/background_dancers/instance_plan.rs (the instance table: build order, slot base/budget, pass-mask
#                                                    override, hull twins; reaches selection::SHADOW_MODEL via `super::`)
#   2026-09-21 mods/background_dancers/preview/layout.rs (preview box geometry, per-side constants, crop / dancer /
#                                                    fallback frustum extents — std only)
#   2026-09-21 mods/background_dancers/preview/state.rs (the per-side focus / wanted / 150 ms settle / live machine)
#   2026-09-22 mods/background_dancers/custom_content.rs (custom dancers/stages from data_mods: arc-name classification,
#                                                    folder/key labels, text-rlist grammar, defaults, the planner;
#                                                    reaches selection + catalog via `super::`)
#   2026-09-30 mods/background_dancers/sources.rs (custom-content SOURCES: slug / label / implicit CUSTOM,
#                                                    directory roles, option-row ids; reaches custom_content via `super::`)
#   2026-10-04 mods/background_dancers/scan_index.rs (the custom-models scan index codec: packed-folder
#                                                    fingerprints, ready-arc members, sidecar rows keyed by
#                                                    listing stamps; escaping, malformed-record isolation — std only)
#   2026-09-30 mods/background_dancers/options_logic.rs (the source / model rows' pure decisions: row table,
#                                                    Request mapping, preview key, bounds, labels; reaches catalog + sources)
#   2026-09-22 mods/background_dancers/movie_mode.rs (Background Movies: OFF / THUMBNAIL / STAGE SCREENS / FULLSCREEN — movie-size
#                                                    override table, fullscreen-backdrop classification, scene mask;
#                                                    2026-09-23: degrade/window-mode rules, the `offscreen1.dds`
#                                                    has-screens test, the fit window, the checked imm8 write)
#   2026-09-22 mods/background_dancers/movie_camera.rs (the MOVIE camera set: `_1p`/`_2p` tags, `_non` cut-aways,
#                                                    dancer-count filter + fallback, folder listing)
#   2026-10-05 mods/background_dancers/flight_fx.rs (the zan CzanEff particle runtime for the HOTTEST PARTY
#                                                    flight effects: TEB parser + simulator, World bone helpers;
#                                                    tests on a synthetic TEB built in the test — std only)
#
# Fixtures: the core/anm suite replays `tests/fixtures/anm/*.json` (generated by
# scripts/gen_anm_fixtures.py from the Python reference codecs) against the REAL
# stock arcs. That leg needs `DDR_WORLD_INSTALL` to point at a World install; it
# SKIPS with a note otherwise (the synthetic tests always run).
#
# Requires: cargo (host toolchain). No Windows toolchain.
# Validation only — writes nothing into the repository.

set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[*] $*"; }

# module name -> repo-relative source path. Names must be unique. Only
# dependency-free modules can mount here (engine-facing files stay
# cabinet-validated).
declare -a MODULE_NAMES=(scene3d_pure scene3d_render_item_layout scene3d_node_layout scene3d_frame_board scene3d_camera_math scene3d_viewport_pass_layout anm selection schedule director_math clock tempo outline catalog pick instance_plan preview_layout preview_state custom_content sources scan_index options_logic movie_mode movie_camera flight_fx)
declare -a MODULE_PATHS=(
  "src/services/scene3d/pure.rs"
  "src/services/scene3d/render_item_layout.rs"
  "src/services/scene3d/node_layout.rs"
  "src/services/scene3d/frame_board.rs"
  "src/services/scene3d/camera_math.rs"
  "src/services/scene3d/viewport_pass_layout.rs"
  "src/core/anm/mod.rs"
  "src/mods/background_dancers/selection.rs"
  "src/mods/background_dancers/schedule.rs"
  "src/mods/background_dancers/director_math.rs"
  "src/mods/background_dancers/clock.rs"
  "src/mods/background_dancers/tempo.rs"
  "src/mods/background_dancers/outline.rs"
  "src/mods/background_dancers/catalog.rs"
  "src/mods/background_dancers/pick.rs"
  "src/mods/background_dancers/instance_plan.rs"
  "src/mods/background_dancers/preview/layout.rs"
  "src/mods/background_dancers/preview/state.rs"
  "src/mods/background_dancers/custom_content.rs"
  "src/mods/background_dancers/sources.rs"
  "src/mods/background_dancers/scan_index.rs"
  "src/mods/background_dancers/options_logic.rs"
  "src/mods/background_dancers/movie_mode.rs"
  "src/mods/background_dancers/movie_camera.rs"
  "src/mods/background_dancers/flight_fx.rs"
)

for p in "${MODULE_PATHS[@]}"; do
  [[ -r "$REPO_ROOT/$p" ]] || die "module source missing: $p"
done

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
note "harness dir: $TMP"

cat >"$TMP/Cargo.toml" <<EOF
[package]
name = "background-dancers-validate"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

# Test-only: the core/anm fixture suite reads the Python-generated JSON.
[dependencies]
serde_json = "1"

[workspace]
EOF

mkdir -p "$TMP/src"
{
  echo "//! Throwaway offline-validation harness — generated by"
  echo "//! scripts/validate_background_dancers.sh; never committed."
  echo "#![allow(dead_code)]"
  for i in "${!MODULE_NAMES[@]}"; do
    echo "#[path = \"$REPO_ROOT/${MODULE_PATHS[$i]}\"]"
    echo "pub mod ${MODULE_NAMES[$i]};"
  done
} >"$TMP/src/lib.rs"

export ANM_FIXTURE_DIR="$REPO_ROOT/tests/fixtures/anm"
# The shipped movie camera set (core/anm fixtures.rs parses + frames every clip).
export MOVIE_CAMERA_DIR="$REPO_ROOT/data_mods/background_dancers/movie_camera"
if [[ -n "${DDR_WORLD_INSTALL:-}" && -d "${DDR_WORLD_INSTALL}/data/arc" ]]; then
  note "core/anm fixture leg: ON (DDR_WORLD_INSTALL set, fixtures in tests/fixtures/anm)"
else
  note "core/anm fixture leg: SKIPPED (DDR_WORLD_INSTALL unset or has no data/arc) -- synthetic tests only"
fi

note "running pure module tests (${MODULE_NAMES[*]})"
(cd "$TMP" && cargo test --quiet)

# The release-time model packer mirrors custom_content's folder rules in
# Python (scripts/pack_custom_models.py): pin it to the same cases.
note "running scripts/test_pack_custom_models.py (release packer parity)"
(cd "$REPO_ROOT/scripts" && PYTHONDONTWRITEBYTECODE=1 python3 -m unittest -q test_pack_custom_models)

# The in-place COLOR0 sRGB undo for shipped models whose source is not at hand.
note "running scripts/test_fix_vertex_colour_srgb.py (COLOR0 patch tool)"
(cd "$REPO_ROOT/scripts" && PYTHONDONTWRITEBYTECODE=1 python3 -W ignore::ResourceWarning -m unittest -q test_fix_vertex_colour_srgb)
note "OK"
