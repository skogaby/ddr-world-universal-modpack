#!/usr/bin/env bash
# Offline validation for the Background Movie Sync pure layers
# (`src/services/movie_sync.rs` — position mapping + validity gate;
# grows as later steps add pure modules).
#
# Usage:
#   ./scripts/validate_movie_sync.sh
#
# Why a harness: plain `cargo test` cannot run on non-x86 hosts (the `retour`
# dependency only compiles for x86/x86_64), so — like the sibling
# validate_*.sh scripts — this builds a throwaway HOST cargo crate in a temp
# directory that mounts the module via `#[path]` and runs its `#[cfg(test)]`
# suite there (all engine-facing code in the module is `#[cfg(windows)]`-gated
# and compiles out on the host).
#
# Requires: cargo (host toolchain). Nothing else. Validation only — writes
# nothing into the repository.

set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "[*] $*"; }

SRC="$REPO_ROOT/src/services/movie_sync.rs"
[[ -r "$SRC" ]] || die "module source missing: src/services/movie_sync.rs"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
note "harness dir: $TMP"

cat >"$TMP/Cargo.toml" <<EOF
[package]
name = "movie-sync-harness"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"
EOF

mkdir -p "$TMP/src"
cat >"$TMP/src/lib.rs" <<EOF
#[path = "$SRC"]
mod movie_sync;
EOF

note "running movie_sync host tests"
(cd "$TMP" && cargo test --quiet)
note "OK"
