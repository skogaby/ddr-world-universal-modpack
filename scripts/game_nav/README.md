# game_nav — scripted navigation for DDR World under CrossOver/Wine

Shell scripts that drive a locally running DanceDanceRevolution World cabinet
(spice2x under CrossOver/Wine) via the SpiceAPI, for automated testing of the
hook DLL. They launch the game, log a player in, pay a session with GALAXY
PLAY (PASELI), reach song select / options / gameplay, and soft-restart or
shut the game down — screenshotting along the way for verification.

All scripts: bash, `set -e`, source `lib.sh`, accept
`[--host HOST] [--port PORT] [--password PASS]` before positional args, and
carry a comment header documenting precondition → postcondition.

```bash
# Typical session
./scripts/game_nav/launch.sh          --host 127.0.0.1 --port 1337 --password lolhax
./scripts/game_nav/to_song_select.sh  --host 127.0.0.1 --port 1337 --password lolhax E0040159F4D19F72 1337
./scripts/game_nav/open_options.sh    --host 127.0.0.1 --port 1337 --password lolhax
./scripts/game_nav/open_options.sh    --host 127.0.0.1 --port 1337 --password lolhax   # toggle closed
./scripts/game_nav/start_song.sh      --host 127.0.0.1 --port 1337 --password lolhax
./scripts/game_nav/restart_inplace.sh --host 127.0.0.1 --port 1337 --password lolhax   # fast reset to title
./scripts/game_nav/shutdown.sh        --host 127.0.0.1 --port 1337 --password lolhax
```

## Standard test credentials

| | |
|---|---|
| Card ID | `E0040159F4D19F72` |
| PIN | `1337` |

This card has an EXISTING profile (dancer `AAAAAAAA`) with a PASELI balance.
**Always use this card.** A different/new card diverts into the first-time
registration flow (name entry, etc.), which desyncs every scripted sequence
and has no PASELI to pay with.

## State map

```
boot (black) ──~2-4 min──> title/attract loop
                             │  (title screen <-> DEMONSTRATION songs, ~60-70s cycle;
                             │   card reader live throughout)
                             │ card insert
                             ▼
                    SELECT LANGUAGE (ENGLISH pre-highlighted; brief)
                             │ P1 Start
                             ▼
                    Confirm e-amusement pass — PIN entry
                             │ keypad PIN (auto-submits at 4 digits)
                             ▼
                    PLAYER1 PROFILE splash (name/stars/rank/league)
                             │ P1 Start x4 (profile -> SELECT MODE, solo)
                             ▼
                    PLAYER1 Payment  ── cursor starts on [Back]
                             │ Menu Down x3:
                             │   Back -> PREMIUM PLAY with PASELI (130)
                             │        -> NORMAL PLAY with PASELI (100)
                             │        -> GALAXY PLAY with PASELI (360)   <- select this
                             │ (4th row: NORMAL PLAY with CREDITS — coin path, OBSOLETE)
                             │ P1 Start (confirm) + P1 Start (skip CAUTION splash)
                             ▼
                    SELECT MUSIC (song select; bottom bar shows PASELI: ******)
                       │ keypad 9              │ P1 Start x2
                       ▼                       ▼
                 P1 Option overlay      difficulty confirm -> GAME PLAY
```

Payment is **GALAXY PLAY with PASELI**. The coin/credits flow that earlier
revisions of `to_song_select.sh` used (coin insert + "NORMAL PLAY with
CREDITS") is obsolete and has been removed.

## Scripts

### `lib.sh`
Shared helpers — source it, don't run it. `parse_common_flags`, `cli`,
`press <button> [hold]`, `screenshot <path>`, `avg_color`, `strip_color`,
`is_title_screen`, `game_running`, `wait_for_api`, `wait_for_title`. Also
holds `DDR_DIR` (game install) and `WINE_BIN` (CrossOver wine).

### `launch.sh`
- **Pre:** none (idempotent; one instance max). **Post:** SpiceAPI up, game at title/attract.
- If the game is already running it reports the current state and exits 0.
- Cold start runs wine with the full spice64 argument set. **`-icmphook` is
  vital**: it fakes the AVS ICMP keepalive that Wine cannot create (raw ICMP
  socket). Without it the game boots but network sticks at CHECKING and
  PASELI is NOT AVAILABLE — login/payment impossible.
- Boot to title takes ~2–4 minutes; the script polls (`wait_for_api` +
  `wait_for_title`), no manual waiting needed.

### `screenshot.sh [OUTPUT_PATH]`
- **Pre:** game running. **Post:** JPEG at OUTPUT_PATH (default timestamped in /tmp), path echoed.

### `login.sh <CARD_ID> <PIN>`
- **Pre:** title/attract (demo frames fine), nobody carded in, eamuse ONLINE,
  existing card profile. **Post:** P1 logged in, GALAXY PLAY (PASELI) paid,
  at SELECT MUSIC.
- The canonical, deterministic card-in sequence (fixed sleeps, no frame
  analysis): card insert → Start (language) → PIN → Start ×4 (profile/mode)
  → Menu Down ×3 + Start ×2 (GALAXY PLAY payment + CAUTION skip).
- **Do not add screens/steps here without re-validating on the cabinet** —
  every downstream flow builds on this exact sequence.

### `to_song_select.sh <CARD_ID> <PIN>`
- Thin wrapper: runs `login.sh` with the same args, waits for the
  CAUTION→logo→SELECT MUSIC transition to settle, saves a reference
  screenshot. Same pre/post as `login.sh`.

### `open_options.sh [PLAYER]`
- **Pre:** at SELECT MUSIC (or difficulty confirm). **Post:** "P1 Option"
  overlay open on the Basic tab (SCROLL SPEED / DANCE GAUGE / ARROW
  PLACEMENT; more tabs incl. MODS via Menu Left/Right).
- Keypad key 9 = OPTION (see the on-screen numpad legend). It **toggles** —
  run again to close. PLAYER 0 = P1 keypad (default), 1 = P2.

### `start_song.sh`
- **Pre:** at SELECT MUSIC, a song highlighted, options overlay CLOSED.
  **Post:** in GAME PLAY (selected song, last-used difficulty).
- Start ×2 (song → difficulty confirm), then waits out the intro splash.

### `restart_inplace.sh`
- **Pre:** game running (any state, even mid-gameplay; in-progress session is
  lost). **Post:** soft-rebooted, back at title/attract (~80–120s).
- Much faster than shutdown + launch — prefer it between test iterations.
- Mechanism: `Test` button → operator menu (cursor on I/O CHECK) →
  `P1 Menu Up` (wraps to last item, GAME MODE) → `P1 Start`.

### `shutdown.sh`
- **Pre:** none (no-op if not running). **Post:** spice64/wine stopped,
  SpiceAPI dead. Tries `control exit` first, falls back to `pkill -f spice64`.

## Button inventory (`spice2x-cli buttons read`)

```
Service   Test   Coin Mech
P1 Start  P1 Panel Up  P1 Panel Down  P1 Panel Left  P1 Panel Right
          P1 Menu Up   P1 Menu Down   P1 Menu Left   P1 Menu Right
P2 Start  P2 Panel Up  P2 Panel Down  P2 Panel Left  P2 Panel Right
          P2 Menu Up   P2 Menu Down   P2 Menu Left   P2 Menu Right
```

Panel = dance pad arrows (gameplay); Menu = cabinet menu buttons (UI
navigation). Keypads are separate: `cli keypads write <0|1> "<digits>"`
(PIN entry, numpad shortcuts like 9=OPTION at song select).

## Timing quirks

- **Attract cycle:** title screen and DEMONSTRATION gameplay alternate
  (~60–70s per cycle). `is_title_screen` only matches the green title frame,
  so a "not at title" report may just mean mid-demo — `wait_for_title` rides
  the cycle out. Card insert works during demos too.
- **Eco mode:** after idling, the title dims to a near-black frame with only
  the green "Scan your e-amusement pass" strip. `is_title_screen` has a
  special case for it (dark frame + green strip at y≈620–665).
- **Boot:** pure black for a long time; SpiceAPI comes up well before the
  title (~2–4 min total). Green transition splashes (CAUTION, logo) look
  like the title to naive average-color checks — beware in new checks.
- **Post-boot network:** the bottom bar goes CHECKING → ONLINE within ~a
  minute of reaching the title. Don't card in while CHECKING.

## Troubleshooting

- **Network stuck at CHECKING / "PASELI cannot be used":** the launch was
  missing `-icmphook` (see `launch.sh`), or the bemani-buddy eamuse server
  (127.0.0.1:5720) is down. Relaunch with the flag; don't touch the server.
- **Hung/crashed:** `pkill -f spice64`, then `launch.sh`.
- **Game log:** `<install>/log.txt` (install dir in `lib.sh`; path has spaces
  — quote it). Wine/launcher output: `/tmp/ddr_run.log`.
- **Sequence desynced mid-flow** (wrong screen for the next press):
  `restart_inplace.sh` and start over from title.
- **SpiceAPI dead but process alive:** the API notification toasts in-game
  show connect/disconnect spam; if `cli info launcher` fails repeatedly,
  treat as hung.
