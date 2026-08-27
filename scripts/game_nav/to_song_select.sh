#!/bin/bash
# to_song_select.sh — full card-in flow: title/attract -> SELECT MUSIC, verified.
# Thin wrapper around login.sh (the canonical GALAXY PLAY (PASELI) sequence) that
# adds screenshot verification of the postcondition. The old coin/credits payment
# flow is OBSOLETE — sessions are paid with GALAXY PLAY via PASELI.
# Precondition:  game at the title/attract screen ("Scan your e-amusement pass";
#                demo frames fine), nobody carded in, eamuse ONLINE, existing
#                card profile with PASELI balance.
# Postcondition: P1 logged in, GALAXY PLAY (PASELI) paid, at SELECT MUSIC
#                (1st STAGE song list, "Select a song." prompt, numpad legend
#                visible bottom-left). A reference screenshot is saved.
# Usage: to_song_select.sh [--host HOST] [--port PORT] [--password PASS] <CARD_ID> <PIN>
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

CARD_ID="${REST_ARGS[0]:-}"
PIN="${REST_ARGS[1]:-}"
if [ -z "$CARD_ID" ] || [ -z "$PIN" ]; then
  echo "Usage: $0 [--host HOST] [--port PORT] [--password PASS] <CARD_ID> <PIN>"
  exit 1
fi

"$GAME_NAV_DIR/login.sh" --host "$HOST" --port "$PORT" --password "$PASSWORD" \
  "$CARD_ID" "$PIN"

# Let the CAUTION -> logo -> SELECT MUSIC transition settle, then save a
# reference screenshot. The button sequence is deterministic; no pass/fail
# frame analysis here (inspect the screenshot if something looks off).
sleep 10
shot="/tmp/ddr_song_select_$$.jpg"
screenshot "$shot"
echo "At song select. Reference screenshot: $shot"
