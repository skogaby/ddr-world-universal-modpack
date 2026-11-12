#!/usr/bin/env bash
set -euo pipefail
# Release build (plain x86_64-pc-windows-msvc, via cargo-xwin) of everything
# that ships as a binary: the hook DLL, the auto-updater and the StepMania
# prep tool. For Windows 7 compatible binaries use ./build_win7.sh.
#
# Extra arguments are passed to every cargo invocation (e.g. --verbose).
# Set ONLY=dll|updater|prep to build a single component.
cd "$(dirname "${BASH_SOURCE[0]}")"

TARGET="x86_64-pc-windows-msvc"
ONLY="${ONLY:-}"

build() {
    local name="$1" dir="$2" out="$3"
    shift 3
    if [[ -n "$ONLY" && "$ONLY" != "$name" ]]; then
        return
    fi
    echo "==> Building $name"
    (cd "$dir" && cargo xwin build --release --target "$TARGET" "$@")
    if [[ "$dir" == "." ]]; then
        echo "Output: $out"
    else
        echo "Output: $dir/$out"
    fi
}

EXTRA=("$@")
build dll     .       "target/$TARGET/release/ddr_world_hook.dll"         "${EXTRA[@]+"${EXTRA[@]}"}"
build updater updater "target/$TARGET/release/ddr_world_hook_updater.exe" "${EXTRA[@]+"${EXTRA[@]}"}"
build prep    prep    "target/$TARGET/release/ddr_world_hook_prep.exe"    "${EXTRA[@]+"${EXTRA[@]}"}"
