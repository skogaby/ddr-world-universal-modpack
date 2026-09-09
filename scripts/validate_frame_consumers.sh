#!/usr/bin/env bash
# Host tests for the actual pure helper used by deferred render consumers.
# No engine stubs, Windows dependencies, game assets, or full-crate build.
set -euo pipefail

REPO_ROOT="$(dirname "$0")/.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

rustc --edition=2021 --test "$REPO_ROOT/src/core/deferred_work.rs" \
    -o "$TMP/frame-consumers-tests"
"$TMP/frame-consumers-tests" "$@"
