#!/bin/bash
# open_options.sh — open the player OPTIONS overlay from song select.
# Uses the numpad shortcut: keypad key 9 = "OPTION" (see the on-screen
# "Numpad operation information" legend at song select). Keypad 9 TOGGLES the
# panel — run this script again (or send keypad 9) to close it.
# Precondition:  at SELECT MUSIC (song select). Also works at the difficulty
#                confirm step (it shows the same OPTION legend).
# Postcondition: "P1 Option" overlay open on the Basic tab (star icon):
#                SCROLL SPEED / DANCE GAUGE / ARROW PLACEMENT rows, with more
#                tabs (arrow skin, screen filter, combo display, ... , MODS)
#                selectable via P1 Menu Left/Right; values via Menu Up/Down.
# Usage: open_options.sh [--host HOST] [--port PORT] [--password PASS] [PLAYER]
#        PLAYER: 0 = P1 keypad (default), 1 = P2 keypad
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

PLAYER="${REST_ARGS[0]:-0}"

echo "Opening P$((PLAYER + 1)) options overlay (keypad 9)..."
cli keypads write "$PLAYER" "9" >/dev/null
sleep 3

shot="/tmp/ddr_options_$$.jpg"
screenshot "$shot"
echo "Options overlay should now be open. Verification screenshot: $shot"
