#!/bin/bash
# shutdown.sh — cleanly stop DDR World.
# Precondition:  none (no-op if the game is not running).
# Postcondition: spice64/wine game process stopped; SpiceAPI no longer responding.
# Usage: shutdown.sh [--host HOST] [--port PORT] [--password PASS]
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

if ! game_running; then
  echo "Game not running (SpiceAPI not responding). Nothing to do."
  # Clean up any orphaned processes just in case.
  pkill -f spice64 2>/dev/null || true
  exit 0
fi

echo "Sending control exit..."
cli control exit >/dev/null 2>&1 || true

for i in $(seq 1 12); do
  sleep 2
  if ! game_running; then
    echo "Game exited cleanly."
    # wine children can linger a moment; nudge them.
    sleep 2
    pkill -f spice64 2>/dev/null || true
    exit 0
  fi
done

echo "control exit did not stop the game; falling back to pkill."
pkill -f spice64 || true
sleep 3
if game_running; then
  echo "ERROR: game still responding after pkill." >&2
  exit 1
fi
echo "Game stopped."
