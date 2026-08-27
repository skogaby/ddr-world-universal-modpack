#!/usr/bin/env bash
set -euo pipefail
# Windows 7 compatible build. The default x86_64-pc-windows-msvc target imports
# ProcessPrng from bcryptprimitives.dll, which doesn't exist on Win7 and causes
# the loader to reject the DLL with "The procedure entry point ProcessPrng could
# not be located". The x86_64-win7-windows-msvc target uses RtlGenRandom instead.
# It's a tier 3 target, so std must be built from source via -Z build-std.
cargo xwin build \
    --release \
    --target x86_64-win7-windows-msvc \
    -Z build-std=std,panic_abort \
    "$@"
echo "Output: target/x86_64-win7-windows-msvc/release/ddr_world_hook.dll"
