#!/usr/bin/env bash
set -euo pipefail
# Windows 7 compatible build of everything that ships as a binary: the hook
# DLL, the auto-updater and the StepMania prep tool.
#
# The default x86_64-pc-windows-msvc target imports ProcessPrng from
# bcryptprimitives.dll, which doesn't exist on Win7 and causes the loader to
# reject the binary with "The procedure entry point ProcessPrng could not be
# located". The x86_64-win7-windows-msvc target uses RtlGenRandom instead. It's
# a tier 3 target, so std must be built from source via -Z build-std.
#
# Extra arguments are passed to every cargo invocation (e.g. --verbose).
# Set ONLY=dll|updater|prep to build a single component.
cd "$(dirname "${BASH_SOURCE[0]}")"

TARGET="x86_64-win7-windows-msvc"
ONLY="${ONLY:-}"

build() {
    local name="$1" dir="$2" out="$3"
    shift 3
    if [[ -n "$ONLY" && "$ONLY" != "$name" ]]; then
        return
    fi
    echo "==> Building Windows 7 $name"
    (cd "$dir" && cargo xwin build \
        --release \
        --target "$TARGET" \
        -Z build-std=std,panic_abort \
        "$@")
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
