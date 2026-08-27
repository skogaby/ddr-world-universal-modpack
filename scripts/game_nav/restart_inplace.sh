#!/bin/bash
# restart_inplace.sh — soft-restart the game via the operator Test menu (GAME MODE).
# Much faster than killing/relaunching the wine process.
# Precondition:  game running (SpiceAPI responding). Works from ANY in-game state,
#                including mid-gameplay. NOTE: an in-progress credit/session is lost.
# Postcondition: game soft-rebooted and back at the title/attract screen.
# Usage: restart_inplace.sh [--host HOST] [--port PORT] [--password PASS]
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

if ! game_running; then
  echo "ERROR: game not running — use launch.sh instead." >&2
  exit 1
fi

echo "Opening operator Test menu..."
press "Test"
sleep 3

# MAIN MENU cursor starts on I/O CHECK; Menu Up wraps to the last item = GAME MODE.
echo "Selecting GAME MODE..."
press "P1 Menu Up"
sleep 1
press "P1 Start"

echo "Soft restart triggered. Waiting for boot -> title (~80-120s)..."
sleep 20   # screen goes black immediately; give the reboot a head start
wait_for_title 240
echo "Game is back at the title/attract screen."
