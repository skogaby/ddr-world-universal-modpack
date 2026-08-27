#!/bin/bash
# login.sh — canonical card-in flow: title/attract -> paid session -> SELECT MUSIC.
# Payment is GALAXY PLAY with PASELI (NOT coins/credits — coin flow is obsolete).
#
# Screen sequence (existing profile; validated on MDX:J:F:A:2026072100):
#   title/attract --card insert--> SELECT LANGUAGE (ENGLISH pre-highlighted;
#   brief — confirm with Start) --Start--> Confirm e-amusement pass (PIN entry)
#   --PIN--> PLAYER1 PROFILE splash --Start x4--> SELECT MODE (solo) ->
#   PLAYER1 Payment (cursor starts on "Back")
#   --Menu Down x3--> GALAXY PLAY with PASELI --Start x2--> CAUTION splash
#   -> SELECT MUSIC.
#
# Precondition:  game at the title/attract screen (demo frames are fine — the
#                card reader is live throughout the attract loop), nobody carded
#                in, eamuse server ONLINE, card profile EXISTS (a new card would
#                divert into first-time registration and desync this sequence).
# Postcondition: P1 logged in, GALAXY PLAY (PASELI) paid, at SELECT MUSIC
#                (1st STAGE song list). PASELI shows ****** in the bottom bar.
# Usage: login.sh [--host HOST] [--port PORT] [--password PASS] <CARD_ID> <PIN>
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

CARD_ID="${REST_ARGS[0]:-}"
PIN="${REST_ARGS[1]:-}"

if [ -z "$CARD_ID" ] || [ -z "$PIN" ]; then
  echo "Usage: $0 [--host HOST] [--port PORT] [--password PASS] <CARD_ID> <PIN>"
  exit 1
fi

echo "Inserting card..."
cli card insert 0 "$CARD_ID" >/dev/null
sleep 4   # -> SELECT LANGUAGE (ENGLISH pre-highlighted)

echo "Confirming language (ENGLISH)..."
press "P1 Start"
sleep 5   # -> Confirm e-amusement pass (PIN entry)

echo "Entering PIN: ****"
cli keypads write 0 "$PIN" >/dev/null
sleep 6   # PIN auto-submits at 4 digits -> PLAYER1 PROFILE splash

echo "Advancing through profile/mode select (solo)..."
press "P1 Start"; sleep 2.0
press "P1 Start"; sleep 2.0
press "P1 Start" 1.0; sleep 2.0   # long-press: start playing solo
press "P1 Start"; sleep 2.0      # -> PLAYER1 Payment (cursor on "Back")

echo "Selecting GALAXY PLAY with PASELI..."
press "P1 Menu Down"; sleep 0.1   # Back -> PREMIUM PLAY with PASELI
press "P1 Menu Down"; sleep 0.1   # -> NORMAL PLAY with PASELI
press "P1 Menu Down"; sleep 0.1   # -> GALAXY PLAY with PASELI
press "P1 Start"; sleep 1.0       # confirm payment
press "P1 Start"                  # skip CAUTION splash -> SELECT MUSIC

echo "Login sequence complete."
