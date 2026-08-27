#!/bin/bash
# Build and deploy the hook DLL to the target machine
set -e

cd "$(dirname "$0")/.."

HOST=$(cat /tmp/sshhost)
USER=$(cat /tmp/sshuser)
PASS=$(cat /tmp/sshpass)
SSH="sshpass -p $PASS ssh -o StrictHostKeyChecking=no $USER@$HOST"

echo "=== Building hook DLL ==="
set -euo pipefail
cargo xwin build --release --target x86_64-pc-windows-msvc "$@"
echo "Output: target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll"

echo "=== Copying hook DLL to target machine ==="
sshpass -p "$PASS" scp -o StrictHostKeyChecking=no target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll "$USER@$HOST:C:/Users/$USER/Desktop/ddr_world_hook.dll"

echo "=== Done ($?) ==="
