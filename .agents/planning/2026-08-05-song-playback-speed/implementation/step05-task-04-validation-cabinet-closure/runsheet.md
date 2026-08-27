# Song Playback Speed — Cabinet Run Sheet (merged Steps 5+6, task 04)

**Build:** `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`
md5 `fce4d53859d40e66173fa600d1296026` (2026-08-08; 156 host tests, all five gates green).
The cabinet currently carries the old Step-4 DLL (md5 `de04293a...`).

**One session where possible.** Each leg lists: action → expected log evidence →
pass criterion. Log at `$DDR_WORLD_INSTALL/log.txt` (resets per boot — copy it off
between boots if legs are split).

---

## Staging (before boot)

1. Copy the new DLL over `$DDR_WORLD_INSTALL/ddr_world_hook.dll`
   (keep the existing `ddr_world_hook.dll.step3-backup`; optionally add a
   `.step4-backup` of the old one).
2. `mod-config.json` cleanup on the cabinet:
   - DELETE the whole `song_playback_speed.diagnostic` block (retired). Keep/add
     `"song_playback_speed": { "cache_limit_gib": 10 }`.
   - Keep `layeredfs.developer_mode: true` for this session (leg j needs it for
     `DDR_SONG_RATE_FAULT`; also produces the retired-key INFO if the diagnostic
     block is left in — either is fine, but deleting is the cleanup).
   - `mods["song-playback-speed"]: true`; autoplay OFF both sides.
3. DELETE `data_mods/_diag/abdt-75.xwb` (Step-4 leftover).
4. Cache directory `data_mods/_cache/song_playback_speed/`: leave in place
   (recovery handles the stale `dbea619e...` entry) — OR delete it entirely to make
   leg (a) a guaranteed cold build. **Deleting is recommended** so U1 is observed on
   this build. Record the choice.
5. Boot. Expect boot lines:
   - `song_rate: Song-rate identity transaction ready` (or the equivalent readiness line)
   - `song_rate: lifecycle scene callback registered (identity_ready=true, cache limit 10 GiB)`
   - If the diagnostic block was left in: `song_rate: 'song_playback_speed.diagnostic' is retired and ignored — ...`

## Leg a — cold 75 % (U1: the on-demand cold build, live)

Card in (P1). On the MODS tab set **SONG SPEED = 75** (verify the row renders the
number; granular step 5, hold-to-repeat). Pick a song NOT in the cache (any song if
the cache was deleted). **Watch the stage-loading screen**: note pause length,
whether the loading animation keeps moving (render continuity), no watchdog/crash.

Expected log sequence:
```
song_rate: generation N armed (75%, mask 0b01, stage 0) — movie tentatively suppressed, clock identity
song_rate: generation N open-redirect 'sound/win/dance/<code>.xwb' -> './data_mods/_cache/song_playback_speed/<key>.xwb' (rate <a>/<b>, thread <tid>, build wall <W> ms)
song_rate: generation N exposed '...' (..., convert status 1)
song_rate: generation N committed (75%, rate <a>/<b>, q31 ...)
song_rate: gameplay started with committed generation N (75%, ...)   [may be absent — known, LOW]
song_rate: generation N completed at gameplay exit — identity reset first, movie contributor cleared
score_guard: ... savekind=2 rate-tainted stage save SUPPRESSED (generation=N, stage=0)
```
PASS: music at 75 %, pitch-correct, arrows in sync; the `open-redirect` line's
`build wall <W> ms` is the cold-build cost (record it — this is the req-24/U1
evidence, alongside your loading-screen observation); W < 30000; no crash.

## Leg b — warm replay

Play the SAME song at 75 % again. PASS: near-instant load; same `open-redirect`
line with `build wall` near 0 ms (warm hit); same rate committed; suppressed save.

## Leg c — second cold build (worker reuse)

Different song, still 75 %. PASS: same sequence as (a) with a new cache key —
proves the generation worker survives across songs.

## Leg d — live 125 % (U2: first above-identity rate)

Set **SONG SPEED = 125** (coarse step: Start+Left/Right jumps by 10). Play.
PASS: pitch-correct FASTER audio, arrows in sync; committed rate shows an
above-1 ratio (source > output, e.g. `rate <a>/<b>` with a > b); suppressed save.

## Leg e — extreme rate observation (U1 worst case)

Set **SONG SPEED = 50** (or lower; 25 = worst case). Pick a LONG song if possible.
PASS (either outcome):
- Build completes: record `build wall` ms vs the 30 s deadline + loading behavior; OR
- Admission refusal (memory/duration on a long song): ONE bounded
  `song_rate: ... refused ... (thread ..., wall ... ms)` WARN, song plays at
  literal stock 100 %, no crash, next song behaves normally. This is an ACCEPTED
  documented outcome — record which one occurred.

## Leg f — literal stock 100 %

Set **SONG SPEED = 100**. Play. PASS: NO open-redirect/exposed/committed lines at
all; bank timeline (if enabled) shows `path=Stock`; normal score save:
`score_guard: ... savekind=2 save allowed (stage_taint=false, ...)`.
(logout_taint will be true from earlier legs — per-stage saves gate on stage_taint
only, so the clean song still saves. Same as the Step-4 matrix.)

## Leg g — static custom-song LayeredFS source

Pick a song whose `.xwb` is a static LayeredFS replacement (any custom song under
`data_mods/.../sound/win/dance/`). Set a non-100 rate and play. PASS: generation
composes FROM the replacement bytes (open-redirect fires; the audible slowed audio
is the custom song's, not stock).

## Leg h — policy fail-closed spot checks

- Start a LOCAL VERSUS session (both pads) with a non-100 SONG SPEED set:
  PASS: `song_rate: scene 26 resolved to identity (...)` — no arm, stock audio.
- Enter a COURSE/Dan chain (if available): same identity resolution.

## Leg i — persistence round-trip (+ backend)

1. In the UI: verify stepping (Left/Right = ±5, Start+Left/Right = ±10) and the
   clamps (can't go below 25 or above 175).
2. Leave SONG SPEED at a distinctive value (e.g. 85). Card out.
   PASS: `P1 logout save SANITISED — scores stripped (Removed), profile forwarded`
   (session had rate-tainted songs).
3. Card back in. PASS: the row shows 85 (restored via the load echo).
4. Backend DB check: `SELECT ddr_code, opt_mod_song_speed FROM ddr_world_profiles ...`
   shows 85 for this profile. NO score rows for any rate-played song; the 100 %
   leg-f song's score IS present.

## Leg j — injected early failure (fail-open fallback)

Reboot with env `DDR_SONG_RATE_FAULT=source-read` (dev mode must be on; spice2x
launch env). Expect boot WARN `song_rate: FAULT INJECTION ACTIVE (...)`.
Set 75 and play. PASS: one bounded refusal WARN
(`... open redirect: SourceRead (thread ..., wall ... ms)`), song plays stock
100 % at identity clock, NO score suppression for it (no taint from an early
failure)... note: the ARM still suppresses the movie tentatively — accepted design.
Next song (same boot) also refuses the same way (boot-only selector); reboot
WITHOUT the env var afterwards and confirm normal 75 % operation returns.

Optional: a cache-eviction observation (fill past `cache_limit_gib` with many
long-song rates) if practical — not required.

## After the session

- Copy `log.txt` off the cabinet for the evidence record.
- Report per-leg outcomes (esp. a/e loading-screen behavior + `build wall` values,
  and the leg-i backend query results).
- On a full pass I tick plan Steps 5+6 and update the canonical progress.md.
- Remember to turn `layeredfs.developer_mode` back off and unset
  `DDR_SONG_RATE_FAULT` for normal operation.
