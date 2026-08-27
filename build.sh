#!/usr/bin/env bash
set -euo pipefail
cargo xwin build --release --target x86_64-pc-windows-msvc "$@"
echo "Output: target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll"
