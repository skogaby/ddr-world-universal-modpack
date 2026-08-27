#!/bin/bash
# launch.sh — start DDR World under CrossOver/Wine (idempotent).
# Precondition:  none (game may or may not be running; only one instance allowed).
# Postcondition: SpiceAPI responding AND game at the title/attract screen.
# Usage: launch.sh [--host HOST] [--port PORT] [--password PASS]
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

if game_running; then
  echo "Game already running (SpiceAPI responding on $HOST:$PORT)."
  shot="/tmp/ddr_launch_state_$$.jpg"
  if screenshot "$shot" 2>/dev/null; then
    if is_title_screen "$shot"; then
      echo "Current state: title/attract screen."
    else
      echo "Current state: NOT the title screen (in-session or attract demo). Screenshot: $shot"
    fi
  fi
  exit 0
fi

echo "Launching DDR World..."
LOG="/tmp/ddr_run.log"
APIPASS_ARGS=()
[ -n "$PASSWORD" ] && APIPASS_ARGS=(-apipass "$PASSWORD")
cd "$DDR_DIR"
# -icmphook is VITAL under Wine/CrossOver: it fakes the AVS ICMP keepalive
# (Wine cannot create the raw ICMP socket). Without it the network status
# sticks at CHECKING and PASELI is NOT AVAILABLE — login/payment impossible.
# -audiohookdisable is equally vital when non_native_os_support.movie_mode is
# "fallback" (real DirectShow graphs): spice2x's audio hooks wrap the
# MMDevice/IAudioClient objects Wine's builtin winmm consumes during quartz's
# audio-renderer enumeration, crashing the first movie-graph build (stack:
# winmm←devenum←quartz). Game audio is WASAPI and unaffected. Matches the
# maintainer's run_ddr alias.
nohup "$WINE_BIN" --bottle bemani --wait-children \
  'C:\ddr_world\contents\spice64.exe' \
  -runas user -w -p 01201000026FDC3AB849 \
  -url http://127.0.0.1:5720 -api "$PORT" "${APIPASS_ARGS[@]}" \
  -icmphook -K ddr_world_hook.dll -audiohookdisable > "$LOG" 2>&1 &
echo "Launched (pid $!). Wine output: $LOG"

echo "Waiting for SpiceAPI..."
wait_for_api 240
echo "SpiceAPI up. Waiting for title/attract screen (boot takes ~60-90s)..."
wait_for_title 240
echo "Game is at the title/attract screen."
