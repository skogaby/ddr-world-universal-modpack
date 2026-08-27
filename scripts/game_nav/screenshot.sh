#!/bin/bash
# screenshot.sh — capture a screenshot of the running game.
# Precondition:  game running (SpiceAPI responding).
# Postcondition: JPEG written to the given path (default: timestamped in /tmp).
# Usage: screenshot.sh [--host HOST] [--port PORT] [--password PASS] [OUTPUT_PATH]
set -e
source "$(dirname "$0")/lib.sh"
parse_common_flags "$@"

OUT="${REST_ARGS[0]:-/tmp/ddr_screenshot_$(date +%Y%m%d_%H%M%S).jpg}"
screenshot "$OUT"
echo "$OUT"
