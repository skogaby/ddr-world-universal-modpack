# Design — Deterministic Audio Clock (DAC-authority gameplay clock)

Updated: 2026-09-09 (rev 2 — maintainer decisions folded in: dedicated top-level mod, default OFF, no score impact, assist-tick alignment included)
Status: APPROVED IN PRINCIPLE by the maintainer (path "(1) go straight for the clock fix"); ready for code-tasking. Supersedes the
unapproved four-minute estimator in `context.md`/`plan.md`.
Research authority: `docs/audio_clock_research.md` (engine mixer, libavs tick,
visual pipeline), `docs/audio_sync_diagnostics_v2.md`, `docs/frame_scheduling.md`.

## 0. Placement and ownership (maintainer decision)

Core-engine timing fixes live in ONE dedicated, toggleable top-level mod so they
can be isolated, reviewed and switched off as a unit:

```
src/mods/gameplay_timing_fixes/        id "gameplay-timing-fixes", display "GAMEPLAY TIMING FIXES"
  mod.rs        Mod trait: init (boot gate → asks the service to install its engine/game seams), enable/disable
                (= arm permission), config section `gameplay_timing_fixes`, per-arm/disarm INFO, WARN latches
  tick_align.rs assist-tick alignment glue (§10): consumes audio_clock's aux-voice onset API + assist_tick's
                candidate-track API; no engine access of its own
src/services/audio_clock/              the game-system integration (hooks + publication), policy-free
  mod.rs        lifecycle, seqlock publication, consumer API (`corrected_rbx`, `song_onset`, `register_aux_voice`,
                `consumed_bytes`), install gate `install_if_enabled(cfg)`
  fit.rs        PURE sliding-window LSQ (host-tested)
  onset.rs      PURE arm state machine + sanity gates (host-tested)
  engine.rs     render-thread observers (shared 0x435A50 dispatcher, 0x43CAC0 produce hook, voice-start identity)
  game.rs       input-tick (T,QPC) pairing detour, song_reset offset subscription, scene gating
```

Rules: the service installs its seams at boot ONLY when the mod is enabled in
`mod-config.json` (`mods["gameplay-timing-fixes"]`, default **false** for the
first cabinet build); a live disable is a passthrough flag (never uninstall a
mixer hook). `song_rate::clock_patch` stays the sole owner of the playhead
redirect and gains a single call-out; `audio_sync_diag` stays a separate,
config-gated diagnostic and shares the cursor seam through the new dispatcher.
No score/taint interaction (maintainer decision: a 100 %-rate synchronisation
correction with stock windows has no score impact).

## 1. Goal

Make the gameplay music count follow the sound device's actual playback of the
song — so that (a) the play-to-play onset error (stock: uniform ±5 ms, §3 of the
research doc) is eliminated and (b) the in-song drift between the game tick and
the DAC (crystal ppm) is eliminated — without changing judgement windows, score
math, the input timestamp contract, or the nominal song-rate semantics. Must be
ready for the first song of a session, need no per-song qualification, and fail
open to stock behaviour.

Non-goals (follow-ups): pinning D3D flip-queue depth; anything about the
32-bit build. The assist-tick cue alignment IS in scope (§10, maintainer
decision: timing-sensitive players train against the ticks).

## 2. Model

Per mix pass `k` on the engine's render thread we observe (all from the DS
backend object, at the existing `0x435A50` cursor-read seam):

- `t_k` — QPC bracketing the `GetCurrentPosition` call (midpoint of pre/post);
- `P_k` — accumulated play-cursor frames (`+0xD0 / blockalign`);
- `Wc_k` — accumulated write-cursor frames (`+0xC8`);
- `W_k` — frames written so far (`+0xC0`, read BEFORE this pass's Commit);
- `Hz`, `blockalign` — from `fmt = *(DS+0x80)`.

Derived per pass: `lead_k = W_k − Wc_k` (frames), `margin_k = Wc_k − P_k`.

Song onset: `F0 = W_k0` read inside the first `0x43CAC0` produce for the song's
node (`node+0x5F8` 0 → >0). Exact.

DAC frame → wall time: `P̂(t) = a + b·(t − t_ref)`, a line fitted to the last
`N` passes' `(t_k, P_k)` (default window ≈ 10 s). Only the line's PHASE at
`t_frame` matters: `t_frame` is always within one pass (≤10 ms + one frame) of
the newest observation, so even a 100 ppm slope error is < 2 µs. The fit exists
solely to average the cursor's reporting staircase (11.6 ms steps on CrossOver;
Win7 unmeasured). A `raw` mode (`P̂(t) = P_k + (t − t_k)·Hz`) is provided for
platforms whose cursor is smooth.

Gameplay clock (replaces `T − A`, i.e. the `RBX + S` term at the clock-patch
site):

```
E(t_frame) = (P̂(t_frame) − F0) / Hz · 1000  +  C  +  content_offset_wall_ms
C          = 5 ms + mean_k(lead_k)/Hz·1000 + mean_k(margin_k)/Hz·1000 + latency_bias_ms
mc         = round(E − S + J)                    (then × Q31 by song_rate's existing stub logic)
```

`C` reproduces the MEAN stock latency (phase mean 5 ms + lead + DS margin, all
session-measured) so existing SOUND_OFFSET calibrations remain valid on
average; the constant device latency beyond the cursor stays inside SOUND_OFFSET
exactly as today. `content_offset_wall_ms` is 0 for a normal start and
`wall(T_q)` for a training seek (supplied by `song_reset`, which already
composes the stock anchor the same way).

Input: **no transformation.** `age = T − Pₛ` stays in the game-tick domain; the
tick-vs-DAC rate mismatch over a ≤250 ms age is microseconds. `event = mc − age`
is otherwise unchanged (AutoFootPanel synthetic ages untouched).

`t_frame` = the QPC paired with this frame's `T` (post-original detour on the
input-manager tick whose last instruction stores `T`; ~100 ns skew). The
correction verifies the pair's `T` equals the `T` the stub is computing from
(`*frame_tick_global + 0x1268`) and passes through otherwise.

## 3. Components

Module layout is fixed by §0 (`src/mods/gameplay_timing_fixes/` = policy +
toggle + tick-alignment glue; `src/services/audio_clock/` = seams + fit +
publication). Consumer API exposed by the service:

```
corrected_rbx(actor, rbx) -> i32          // called from the clock-patch stub; identity passthrough when not armed
song_onset() -> Option<Onset>             // {F0_song, generation, t_k0, P_k0}
register_aux_voice(bank_ptr) / aux_onset(bank_ptr) -> Option<Onset>   // assist tick (§10)
consumed_bytes(node) -> Option<u64>       // node+0x5F8 snapshot for safe in-place rewrites (§10)
```

Hook inventory (one detour per target — shared dispatchers where a target is
already detoured by `audio_sync_diag`):

| Target | Kind | Notes |
|---|---|---|
| engine `0x435A50` cursor read | existing diag detour → **shared dispatcher** (diag + clock subscribers) | post-original: publish `(t_k, P_k, Wc_k, W_k, Hz, ba, DS ptr)`; feed fit + running means; detect resets (P decreases / `Wc>W` clamp / gap > 200 ms) |
| engine `0x43CAC0` source-node produce | NEW detour | pre: if `node == pending_node` read `+0x5F8` and `DS+0xC0`; post: if pre==0 && `+0x5F8`>0 → publish `F0` |
| engine `0x25ED0` streaming submission | existing diag detour | extend the binding with `node = *(*(V+0x28) + 8)` (both pointers `is_readable`-probed) |
| engine wrapper Start `0x1E1F0` (or the in-memory wave-start seam feeding it) | NEW detour (pre-original) | identity for IN-MEMORY waves (the assist-tick bank) — streaming waves never pass through `0x25ED0`'s siblings; resolves `V → cue/bank` via the diag's reciprocal chain and records `node` for registered aux banks |
| engine sound-stop / cue-destroy / bank-unregister | existing diag hooks | disarm on the armed cue's stop/destroy |
| game input tick (`FUN_1800231F0`-shaped; AOB on the `Ordinal_45` call + `[reg+0x1268]` store tail) | NEW detour, post-original | publish `(T, QPC)` seqlock |
| game clock-patch site | existing `song_rate::clock_patch` stub (sole owner) | stub grows a call to `audio_clock::corrected_rbx(rdi=actor, rbx)` BEFORE the Q31 multiply; default path returns `rbx` unchanged; ABI: preserve all live registers/flags (the stub already saves rax/rcx/rdx — extend the save set for a Rust call, 16-byte stack alignment, shadow space) |
| `song_reset` | API subscription | `content_offset_wall_ms` for the next voice instance (0 on restart, `wall(T_q)` on seek); clears on natural start |

All engine hooks install in the factory-return / pre-Initialize window with the
same AMD64 PE identity + consumed-code attestation discipline as the diagnostics
(never late-patch a running mixer). Missing any engine seam ⇒ clock unavailable
(stock), one WARN.

## 4. Lifecycle

```
boot            fit warms from the first pass (attract audio is continuous); READY when N ≥ 256 passes (~2.5 s)
song prepare    diag binding identifies the DPS song cue (slot-5 dance bank) → Pending(node) at streaming submission
first produce   F0 latched (render thread) → game thread sees Armed on its next frame
sanity gate     delta = E(t_frame) − (T − A) must be within ±50 ms else refuse (WARN once, stay stock)
armed           every onUpdate: rbx' = round(E(t_frame)) − S ; song_rate Q31 applies after
disarm          armed cue stops/destroys; scene leaves the play scenes; fit invalid; pass observations stale (>150 ms:
                render thread stalled → stock until passes resume, then re-arm with the same F0)
restart/seek    song_reset stop→replay creates a NEW voice: Pending again; offset from song_reset; new F0
```

Switching from stock to armed happens ≤2 frames after the anchor, ~450 ms
before chart time zero; the one-time step equals this play's stock onset error
(≤ ~±5–10 ms) and is not judged. No slewing.

## 5. Failure modes / fail-open

- Engine seams unresolved, PE identity mismatch, factory window missed → clock never arms; stock.
- Fit not READY / discontinuity → disarm; stock.
- Node identity chain unreadable (`is_readable` fails) → never Pending; stock.
- `|delta| > 50 ms` at arm → refuse; stock. (Catches wrong-voice, seek-offset mismatch, rate confusion.)
- Render-thread stall → stock for the stall; re-arm on resume (same F0).
- Any panic in a hook → `catch_unwind`, permanent disarm, WARN.

Nothing here alters windows, score math, saves, or the nominal rate
publication. **No score taint** (maintainer decision): this is a
synchronisation correction of the same nature as SOUND_OFFSET calibration
(100 % rate, stock windows).

## 6. Configuration (`mod-config.json`)

```json
"mods": { "gameplay-timing-fixes": false },   // first cabinet build ships OFF; tester flips ON
"gameplay_timing_fixes": {
  "audio_clock": {
    "mode": "fit",            // "fit" (default) | "raw"
    "window_seconds": 10,     // fit history (2..60)
    "latency_bias_ms": 0      // operator tweak added to C
  },
  "assist_tick_alignment": true   // §10; ignored when the assist-tick mod is off
}
```

Boot-only (the engine seams need the pre-Initialize window). Overlay row optional later.

## 7. Diagnostics (same build)

- v2 CSV gains `onset` events: `F0, W_k0, P_k0, Wc_k0, t_k0, lead, margin, delta_vs_stock_ms, fit_n, fit_resid_sd_ms`.
- One INFO per arm: `audio_clock: armed gen=… F0=… delta_vs_stock=+3.2 ms C=55.1 ms fit(n=1024, sd=0.31 ms, slope=44100.9 f/s)`.
- One INFO per disarm with reason. WARN once per class on refusals.
- The tester's first run therefore yields: stock onset spread across plays (from
  `delta_vs_stock`), Win7 cursor granularity (`fit_resid_sd`, cursor gcd), and
  confirmation the clock armed on every song.

## 8. Interactions

| Feature | Effect |
|---|---|
| song_playback_speed | unchanged: the Q31 multiply still follows; `E` is wall-domain like `T − A` |
| song_reset (restart/seek/loop) | supplies `content_offset_wall_ms`; each replay is a new voice → new `F0` |
| training_mode, movie_sync, PUS, real_speed, s_marvelous, per-song offsets | consume `mc` / `J` → automatically corrected |
| auto-calibration | still valid; it will now converge on a stable value instead of chasing the onset jitter |
| assist_tick | aligned to the song's pass grid via §10 (in scope) |
| audio_sync_diag | shares the cursor seam via the new dispatcher; no second detour |
| Wine `fallback` movie mode | unaffected (movie clock proxy consumes `mc`) |

## 9. Validation

- Host: `fit.rs` (staircase synthetic → phase error bound, slope recovery, reset/gap handling, re-centering), `onset.rs` (state machine, sanity gate, seek offset), stub-layout unit test for the extended clock stub (bytes, alignment, rel32), engine-site fingerprints on the real `xactengine2_10.dll` (extend `scripts/validate_xact_diagnostics.sh`), input-tick AOB on all four game builds + `shape_diff`, candidate-track byte identity for the assist-tick swap (`scripts/validate_se_bank_synth.sh`).
- Build gates: `cargo check` (windows target), `cargo fmt`, `./build.sh`, `./build_win7.sh`, `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`.
- Cabinet: (1) CrossOver smoke — arm on every song, delta distribution, no stutter, tick swap logged; (2) Win7 tester run with the mod + diagnostics ON — the §7 evidence; then flip default ON.

## 10. Assist-tick alignment (in scope — maintainer decision)

### 10.1 The stock-shaped defect

The tick track is ONE cue played through the same engine; the mod computes each
clap's position from the game clock at hand-off (`m0`) and, at commit, stops,
rewrites the wave shifted by the wall-converted `mc − m0` in whole ADPCM blocks
(2.9 ms granularity), and calls `Play`. That `Play` is a posted voice `Start`
like the song's, so the tick voice's sample 0 lands at the first frame of
whichever pass drains it. Both onsets sit on the 441-frame pass grid, hence
**`F0_tick − F0_song = 441·n` exactly** for some integer `n` — the tick track
is offset from the song by a whole number of 10 ms passes plus the fractional
constant the mod's arithmetic assumed. In stock the pass phase is random, so
tick-vs-song alignment carries the same ±5 ms play-to-play spread as the chart
(plus the ±1.45 ms block quantisation). Sensitive players training against the
ticks feel exactly this.

### 10.2 Fix: exact onset, candidate tracks, in-place swap

With the audio clock the game-clock reading at the tick's actual sample 0 is
exact: `mc_tick0 = (F0_tick − F0_song)/44.1·1000 + C − S + J_side`
(fractional ms). The only unknown before `Start` is `n`; after the tick voice's
first produce it is known to the sample.

1. **Synthesis (background thread, once per song as today)** produces the tick
   PCM mix once and encodes **K = 3 candidate tracks** whose byte 0 corresponds
   to `mc_tick0` for `n ∈ {n̂−1, n̂, n̂+1}` where `n̂` = the pass the commit
   expects to land on (predicted from the current pass cadence published by the
   service; any K that covers the observed spread is fine — the swap step
   detects a miss and WARNs). Encoding is ~3× today's cost, still inside the
   READY dwell; memory ~3× one track (a few MB).
2. **Commit (game thread, unchanged shape):** stop, write candidate `n̂`
   (no block-shift — the candidate already encodes the exact fractional
   alignment), `Play`.
3. **Swap (game thread, next frame after `aux_onset()` reports `F0_tick`):**
   compute actual `n`; if `n ≠ n̂`, `memcpy` candidate `n` over the bank buffer
   from `consumed_bytes(node) + SAFETY` (≥ 8 KiB ≈ 350 ms of mono ADPCM ahead
   of the decoder's read pointer) to the end. The engine reads the client-owned
   in-memory bank lazily (proven: per-song in-place rewrites already ship), so
   bytes ahead of the read pointer take effect when reached. The ≤ 350 ms of
   already-served track keeps the `n̂` alignment (≤ 10 ms off) — the tick cue
   starts during READY, so no real tick is affected; on a training scrub the
   first ≤ 0.35 s of ticks may be one pass off, logged.
4. Restart / seek / loop: the existing stop→rewrite→play path re-runs with the
   new `F0_song`/`F0_tick` (each replay is a new voice) — same three steps.

Fail-open ladder: audio clock not armed for the song ⇒ today's `m0` path
unchanged; `aux_onset` never arrives (identity miss) ⇒ keep candidate `n̂`
(≤ 10 ms off, same as today, one WARN); `n` outside the candidate set ⇒ WARN,
keep `n̂`. Song-rate ≠ 100 %: `content_to_wall` already rides the committed
`RateRatio`; `mc_tick0` is computed in wall ms then converted exactly as the
existing `tick_domain` arithmetic does. The `assist_tick` mod grows two small
APIs (`synthesize_candidates`, `swap_candidate_ahead`) consumed by
`gameplay_timing_fixes::tick_align`; with the new mod disabled, assist_tick is
byte-for-byte today's behaviour.

## 11. Decisions (resolved 2026-09-09)

1. `mods["gameplay-timing-fixes"]` default **false** for the first build; configurable; tester flips ON.
2. **No score impact.**
3. Assist-tick alignment **included** (§10).
4. Frame-pump review follow-ups (mod_menu `open()` availability gate, PUS raise `try_lock`, `topmost_ready()`, `init_factory` profiling tick, stale movie_sync comments) are fixed alongside, before the cabinet build.
