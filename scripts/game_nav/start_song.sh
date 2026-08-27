#!/bin/bash
# start_song.sh — start the currently selected song from song select.
# Precondition:  at SELECT MUSIC (song select) with a song highlighted
#                (default: first song, e.g. "Ace out"). Options overlay CLOSED.
# Postcondition: in GAME PLAY (gameplay of that song, last-used difficulty).
#                If the Autoplay mod is enabled the song plays hands-free.
# Usage: start_song.sh [--host HOST] [--port PORT] [--password PASS]
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

echo "Entering song (difficulty select)..."
press "P1 Start"
sleep 2   # -> PLAYER1 DIFFICULTY carousel

echo "Confirming difficulty..."
press "P1 Start"
sleep 5   # -> "Tips" / song intro splash

echo "Waiting for gameplay to begin..."
sleep 10  # intro splash -> GAME PLAY

shot="/tmp/ddr_gameplay_$$.jpg"
screenshot "$shot"
if is_title_screen "$shot"; then
  echo "ERROR: at title screen — start_song failed. See $shot" >&2
  exit 1
fi
echo "Should be in gameplay now. Verification screenshot: $shot"
