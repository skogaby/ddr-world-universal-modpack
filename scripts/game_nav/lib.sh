# shellcheck shell=bash
# lib.sh — shared helpers for DDR World game navigation scripts.
# Source this from the sibling scripts; do not execute directly.
#
# Provides:
#   parse_common_flags "$@"   -> consumes --host/--port/--password, leaves rest in REST_ARGS
#   cli <args...>             -> spice2x-cli with host/port/password applied
#   press <button> [hold]     -> press+release a button (default hold 0.1s)
#   screenshot <path>         -> capture a JPEG to <path>
#   avg_color <jpg>           -> echoes "R G B" (sparse-sampled average)
#   is_title_screen <jpg>     -> exit 0 if the frame looks like the green title/attract screen
#   wait_for_api [timeout_s]  -> block until SpiceAPI responds (default 300s)
#   wait_for_title [timeout_s]-> block until the title/attract screen is on screen (default 300s)
#   game_running              -> exit 0 if SpiceAPI responds right now

GAME_NAV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPICE_CLI="$GAME_NAV_DIR/../../spice2x-cli/spice2x-cli"

HOST="localhost"
PORT="1337"
PASSWORD=""

# DDR World install dir + launch command (CrossOver/Wine). Quoted: paths have spaces.
# Both are overridable by environment variable; the defaults assume a stock
# CrossOver install with the game in a bottle named "bemani". Note the defaults
# use "$HOME" rather than "~" on purpose -- tilde does NOT expand inside quotes,
# so a quoted "~/..." would be treated as a literal directory named "~".
DDR_DIR="${DDR_WORLD_INSTALL:-$HOME/Library/Application Support/CrossOver/Bottles/bemani/drive_c/ddr_world/contents}"
WINE_BIN="${WINE_BIN:-/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin/wine}"

parse_common_flags() {
  REST_ARGS=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --host) HOST="$2"; shift 2;;
      --port) PORT="$2"; shift 2;;
      --password) PASSWORD="$2"; shift 2;;
      *) REST_ARGS+=("$1"); shift;;
    esac
  done
}

cli() {
  "$SPICE_CLI" --host "$HOST" --port "$PORT" --password "$PASSWORD" "$@"
}

# press <button name> [hold seconds]
press() {
  local btn="$1" hold="${2:-0.1}"
  cli buttons write "$btn=1.0" >/dev/null
  sleep "$hold"
  cli buttons write-reset "$btn" >/dev/null
}

# screenshot <output path>
screenshot() {
  cli capture get-jpg --output-path "$1" >/dev/null
}

game_running() {
  cli info launcher >/dev/null 2>&1
}

# avg_color <jpg> -> "R G B" averaged over a sparse pixel grid (via sips->bmp, no PIL needed)
avg_color() {
  python3 - "$1" <<'PYEOF'
import struct, subprocess, sys, tempfile, os
jpg = sys.argv[1]
tmp = tempfile.mktemp(suffix=".bmp")
subprocess.run(["sips","-s","format","bmp",jpg,"--out",tmp],capture_output=True,check=True)
with open(tmp,"rb") as f: d=f.read()
os.unlink(tmp)
off = struct.unpack("<I", d[10:14])[0]
w, h = struct.unpack("<ii", d[18:26])
bpp = struct.unpack("<H", d[28:30])[0]
step = bpp//8
row = ((w*step+3)//4)*4
tr=tg=tb=n=0
for y in range(0, abs(h), 20):
    base = off + y*row
    for x in range(0, w, 20):
        p = base + x*step
        b,g,r = d[p], d[p+1], d[p+2]
        tr+=r; tg+=g; tb+=b; n+=1
print(f"{tr//n} {tg//n} {tb//n}")
PYEOF
}

# strip_color <jpg> <x0> <y0> <x1> <y1> -> "R G B" average of a pixel region
strip_color() {
  python3 - "$1" "$2" "$3" "$4" "$5" <<'PYEOF'
import struct, subprocess, sys, tempfile, os
jpg = sys.argv[1]
x0,y0,x1,y1 = (int(v) for v in sys.argv[2:6])
tmp = tempfile.mktemp(suffix=".bmp")
subprocess.run(["sips","-s","format","bmp",jpg,"--out",tmp],capture_output=True,check=True)
with open(tmp,"rb") as f: d=f.read()
os.unlink(tmp)
off = struct.unpack("<I", d[10:14])[0]
w, h = struct.unpack("<ii", d[18:26])
bpp = struct.unpack("<H", d[28:30])[0]
step = bpp//8
row = ((w*step+3)//4)*4
flip = h > 0
H = abs(h)
tr=tg=tb=n=0
for y in range(y0, min(y1,H), 4):
    yy = (H-1-y) if flip else y
    base = off + yy*row
    for x in range(x0, min(x1,w), 4):
        p = base + x*step
        b,g,r = d[p], d[p+1], d[p+2]
        tr+=r; tg+=g; tb+=b; n+=1
print(f"{tr//n} {tg//n} {tb//n}")
PYEOF
}

# is_title_screen <jpg> -> true for either title/attract variant:
#  - normal: frame dominated by DDR World green (~ R29 G200 B157)
#  - eco mode: near-black frame with the green "Scan your e-amusement pass"
#    prompt strip at y~620-665 (the game drops to eco after idling)
# Boot is pure black (prompt strip too); attract demo frames are neutral/colorful.
is_title_screen() {
  local rgb r g b
  rgb=$(avg_color "$1") || return 1
  r=$(echo "$rgb" | awk '{print $1}')
  g=$(echo "$rgb" | awk '{print $2}')
  b=$(echo "$rgb" | awk '{print $3}')
  # normal green title
  if [ "$g" -gt 170 ] && [ $((g - r)) -gt 100 ] && [ "$b" -gt 120 ]; then
    return 0
  fi
  # eco-mode title: dark frame + green prompt text strip
  if [ "$g" -lt 60 ] && [ "$r" -lt 60 ] && [ "$b" -lt 60 ]; then
    local srgb sr sg sb
    srgb=$(strip_color "$1" 350 620 930 665) || return 1
    sr=$(echo "$srgb" | awk '{print $1}')
    sg=$(echo "$srgb" | awk '{print $2}')
    sb=$(echo "$srgb" | awk '{print $3}')
    if [ "$sg" -gt 25 ] && [ "$sg" -gt $((sr + 10)) ] && [ "$sg" -gt $((sb + 10)) ]; then
      return 0
    fi
  fi
  return 1
}

# wait_for_api [timeout seconds]
wait_for_api() {
  local timeout="${1:-300}" waited=0
  while ! game_running; do
    sleep 5; waited=$((waited + 5))
    if [ "$waited" -ge "$timeout" ]; then
      echo "ERROR: SpiceAPI did not respond within ${timeout}s" >&2
      return 1
    fi
  done
  return 0
}

# wait_for_title [timeout seconds] — polls screenshots until the title/attract green
# screen is visible. The attract loop cycles title -> demo -> title, so a title frame
# recurs every ~60-70s even if we start mid-demo.
wait_for_title() {
  local timeout="${1:-300}" waited=0
  local shot="/tmp/game_nav_title_poll_$$.jpg"
  while true; do
    if screenshot "$shot" 2>/dev/null && is_title_screen "$shot"; then
      rm -f "$shot"
      return 0
    fi
    sleep 5; waited=$((waited + 5))
    if [ "$waited" -ge "$timeout" ]; then
      rm -f "$shot"
      echo "ERROR: title screen not reached within ${timeout}s" >&2
      return 1
    fi
  done
}
