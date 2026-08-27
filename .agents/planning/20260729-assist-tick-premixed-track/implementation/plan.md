# Implementation Plan — Assist Tick: Pre-Mixed Tick Track

**Status: Approved 2026-07-29 (maintainer)**

Decomposition of `design/detailed-design.md` (Approved 2026-07-29). Each step leaves the system
building and demonstrable. Risk-ordered: the steps most likely to invalidate the design (does the
engine accept a long in-memory ADPCM entry? does rewrite-in-place + seek behave?) land first,
behind a temporary dev trigger, before the mod's clock is touched.

Verification split (repo norm): the agent runs `cargo check`/`cargo fmt`/`./build.sh` and offline
container validation and reads `log.txt` out of the local install; the **maintainer runs every
listening/gameplay test**. There is no unit-test harness; "tests" below are offline byte/log
checks plus the maintainer listening matrix.

## Checklist

- [x] Step 1: Synthesis module (`se_bank_synth`) + clap PCM asset — pure CPU, offline-validated
- [x] Step 2: `game_audio` rewritable tick bank (register / rewrite / play-with-seek / stop) behind a dev trigger *(done — seek replaced by the approved block-shift mechanism; all engine assumptions proven live 2026-07-29)*
- [x] Step 3: Rework `mods/assist_tick` to the pre-mixed track (SOUND_OFFSET only), retire the clock/knob *(done 2026-07-29 — maintainer-validated: even claps in sync with the music, misses ignored, quick restart clean; rewind re-anchor landed here too)*
- [x] Step 4: JUDGMENT TIMING — per-side `Option` base derivation + FR-3 offset term + sign validation *(done 2026-07-29 — sign ear-validated via a temporary 10× amplifier: +100 → 1 s late, −100 → 1 s early; `JUDGMENT_TIMING_SIGN = +1` confirmed, amplifier removed)*
- [x] Step 5: Lifecycle hardening (rewind/late-start seek, capacity, failure paths), diagnostics, docs, asset cleanup *(done 2026-07-29 — most hardening had landed in Step 3 via the unified block-shift; Step 5 added the FR-8 truncation WARN, README/AGENTS/xact-research doc updates, and removal of `banks/tick.{xwb,xsb}` + `build_assist_tick_bank.sh`. Final full-session regression pass: maintainer)*

---

## Step 1: Synthesis module (`se_bank_synth`) + clap PCM asset

**Objective.** Stand up the pure-CPU format layer with no game dependency: MS-ADPCM encoder,
fixed-header XWB writer (+ sample-segment rewrite math), one-cue SE-profile XSB writer, and clap
mixing. Introduce the raw-PCM clap asset. This is the foundation both later engine and mod work
build on, and it is fully validatable offline.

**Implementation guidance.**
- New module `src/services/se_bank_synth/` (`mod.rs` + submodules as convenient: `adpcm.rs`,
  `xwb.rs`, `xsb.rs`, `mix.rs`). Register `pub mod se_bank_synth;` in `src/services/mod.rs`.
- Port `adpcm::encode`, the XWB writer, and `xsb::write_se` from the sibling `ddr-chart-tools`
  (offline-proven by the shipped feature). Reduce the writers to the fixed shape: one ADPCM mono
  entry declared at `TICK_CAPACITY_MS = 300_000`, `TICK_RATE_HZ = 44_100`; internal bank/cue name
  `"asti"`; mix category 6, no RPC curve, wave index 0.
- Public API exactly as design §"Components 2": `build_tick_containers() -> TickContainers`
  (`xsb_bytes`, `xwb_bytes`, `sample_seg_offset`, `sample_seg_len`) and
  `synthesize_track(clap_pcm: &[i16], content_ms: &[i32]) -> SynthResult` (saturating i32→i16 mix,
  clip `content_ms < 0` to 0 with a count, drop `≥ TICK_CAPACITY_MS` with a count, encode to
  exactly `sample_seg_len` bytes with an encoded-silence tail).
- Clap asset: convert the shipped `data_mods/assist_tick/source/clap.ogg` to raw mono i16 LE
  44100 Hz, commit as `data_mods/assist_tick/clap_44k_mono.pcm`. Document the one-liner
  (`ffmpeg -i clap.ogg -ac 1 -ar 44100 -f s16le clap_44k_mono.pcm`) in a comment in the module or
  a short note under `scripts/`. Do NOT delete the old `banks/tick.{xwb,xsb}` or
  `build_assist_tick_bank.sh` yet (Step 5 cleanup, after the new path is proven).

**Tests (offline, agent).**
- Add a debug-gated dump: when `layeredfs.developer_mode` (or a dedicated diag flag) is set,
  `build_tick_containers()`/`synthesize_track()` write their outputs to
  `data_mods/_cache/assist_tick_synth/{tick.xsb,tick.xwb}`. Validate offline against the engine's
  validator rules (the shipped feature's replay checks) and byte-compare the XSB against the
  sibling `ddr-se-bank` output for the same parameters; assert `sample_seg_offset + sample_seg_len
  == xwb.len()` and that a known clap pattern lands at the expected block offsets in the dump
  (via `ddr-se-bank dump`).
- Cross-build sanity: encoder output for a fixed input is byte-reproducible.

**Integration.** No game calls, no mod wiring yet — the module compiles and is exercised only by
the debug dump. Nothing else references it.

**Demo.** With a dev build in developer mode, boot the game once: the cache dir contains a
`tick.xwb`/`tick.xsb` pair that passes the offline validator and whose dump shows claps at a
test pattern of timestamps — proving the synthesized containers are engine-legal before any
engine call exists.

---

## Step 2: `game_audio` rewritable tick bank behind a dev trigger

**Objective.** Prove the load-bearing engine assumptions live: the engine accepts a 300 s
in-memory ADPCM wave bank; the sample segment can be rewritten in place between songs; the cue
starts, stops, and honors a `timeOffset` seek. All behind a temporary trigger, before the mod's
real logic depends on any of it.

**Implementation guidance.**
- Extend `src/services/game_audio.rs` with the design §"Components 1" API: `TickBankHandle`,
  `register_tick_bank`, `rewrite_tick_wave`, `play_tick_track(seek_ms)`, `stop_cue`. All
  game-thread-only; all behind the existing engine-module presence gate; permanent failures latch
  one WARN (existing pattern).
- `register_tick_bank`: reuse the existing slot-claim + `CreateInMemoryWaveBank` +
  `CreateSoundBank` path; additionally capture the leaked XWB buffer pointer + the
  `sample_seg_offset/len` (from `build_tick_containers`) into `TickBankHandle`. Keep the immortal
  rule (write only the slot bank pointer; `file_id` stays −1).
- `play_tick_track`: dispatch `SoundBank::Play` (vt+0x20) directly with `timeOffset = seek_ms`,
  cue by name, centre pan implicit. `stop_cue`: `SoundBank::Stop` (vt+0x28) with flags = 1
  (immediate). `rewrite_tick_wave`: `copy_nonoverlapping` into the captured sample segment;
  assert `encoded.len() == sample_len`.
- Temporary dev trigger (removed in Step 3): on the shipped mod's first judge dispatch, once per
  song, synthesize a fixed test pattern (e.g. a clap every 500 ms for 20 s), register the bank on
  song 1, `rewrite_tick_wave` + `play_tick_track(0)` each song, `stop_cue` on gameplay exit.

**Tests (offline + maintainer).**
- Boot log (agent): `register_tick_bank` slot computed once, both `Create*` `hr=0`, `file_id`
  left −1; on song 2+, no second registration, one rewrite line, one play line.
- Maintainer: the 500 ms test pattern is audible and **even**; survives ≥ 3 song loads (immortal
  bank); `play_tick_track(seek_ms=5000)` starts the pattern 5 s in (seek works); `stop_cue` on
  exit silences it with no stuck audio or crash; a rewrite on the next song plays the new pattern
  (rewrite-in-place works). Confirm under CrossOver specifically.

**Integration.** `game_audio` gains the tick API; the shipped mod still runs its per-tick clock
unchanged — the dev trigger is additive and side-effect-free except for the test tone. If Step 2
disproves rewrite-in-place (race), switch to the double-buffer fallback (design assumption 3)
before Step 3.

**Demo.** In-game, each song plays an even 500 ms metronome from the mod-owned bank, seekable and
stoppable, surviving song changes — the entire engine contract the feature relies on, shown
working before the mod's timing logic is rebuilt on it.

---

## Step 3: Rework `mods/assist_tick` to the pre-mixed track (SOUND_OFFSET only)

**Objective.** Replace the per-frame `se_play` clock with the pre-mixed track driven by real
chart timestamps, delivering the core jitter fix end-to-end. Defer JUDGMENT TIMING to Step 4;
use `content_ms(i) = t_i − SOUND_OFFSET − m0` for now.

**Implementation guidance.**
- Keep verbatim: the `assist_tick` option row + persistence, scene wiring, enable latching,
  `build_tick_list` (FR-2 predicate + 4 ms coalesce), side selection (sibling walk + FR-5),
  degraded mode, the once-per-song build diagnostic shape.
- Remove: the per-frame cursor / adaptive-lead / `play_cue` clock, `TICK_OFFSET_MS`, the overlay
  child row (`register_overlay_row`, `set_offset_ms`, `remove_rows_for` for it), and the
  `assist_tick.offset_ms` read. Leave the config field parsed-but-ignored with a one-shot INFO
  ("legacy key ignored"). Remove the Step 2 dev trigger.
- New per-song state machine (design §"Components 3"): at the first judge dispatch, after
  `build_tick_list`, read `SOUND_OFFSET = *(i32*)(chosen_actor + 0x16c)`, latch `m0 = music_count`,
  compute `content_ms[]`, and hand off to a background synthesis job (NFR-1: mix + encode off the
  game thread). On a subsequent judge frame, when the result is ready: ensure the bank is
  registered (idempotent), `rewrite_tick_wave`, then `play_tick_track(seek_ms = max(0,
  music_count − m0))`. `stop_cue` on scene exit / GAMEPLAY re-entry / fail-out.
- Background job: spawn a thread (or reuse a small worker) that runs `synthesize_track`; publish
  the encoded bytes back through the `SONG` mutex as `phase = Ready(encoded)`; the game thread
  commits. Catch panics at the thread boundary → `phase = Idle` + one WARN.

**Tests (offline + maintainer).**
- Build line (agent): `results/kept/rej_*/coalesced` unchanged from the shipped mod for the same
  chart, plus `sound_offset`, `m0`, `clipped/dropped` counts and synthesis duration ms.
- Maintainer: the 16th-note burst chart — claps are now **metronomically even** (the acceptance
  test; A/B against the shipped build). Chart-driven-through-misses still holds. Solo P1 and solo
  P2 both tick.

**Integration.** The mod now depends on Step 1 (synth) and Step 2 (engine API); the old clock is
gone. JUDGMENT TIMING not yet applied, so a player with a non-zero JUDGMENT TIMING hears claps on
the objective chart beat (documented as the Step 4 gap). FR-6 init gating for existing prereqs
(clap asset, audio/judge/scene availability) lands here.

**Demo.** With `assist_tick` ON, playing a real chart produces sample-exact, jitter-free claps at
`t_i − SOUND_OFFSET`, synthesized per song, surviving restarts — the feature's core value, minus
the per-player judge offset.

---

## Step 4: JUDGMENT TIMING — per-side `Option` base derivation + FR-3 term

**Objective.** Complete FR-3: make the clap follow the tick side's JUDGMENT TIMING, so it lands
on the judgement moment rather than the objective chart beat.

**Implementation guidance.**
- Add a derivation in `src/core/signatures.rs` for the per-side context table base
  (`DAT_1806ebe50`-equivalent): RIP-decode from a matched instruction inside the per-frame count
  function (`FUN_18005f100` on 20260324; match by a stable AOB near the `side_ctx` load), same
  style as `derive_game_audio_addresses`. Expose a resolved address the mod can use to reach
  `side_ctx[side]` → `+0xe0` (`ddr::player::Option`) → `+0x24` (`timing_music`).
- In the mod, at song build, read `JUDGMENT_TIMING(chosen_side)` and extend the formula to
  `content_ms(i) = t_i + SIGN * JUDGMENT_TIMING − SOUND_OFFSET − m0`, where `SIGN` is one named
  constant (`+1` per assumption 1; the whole point of naming it is a one-line flip).
- FR-6/NFR-4: if the derivation fails, the mod fails `init()` (no degraded mode). Add it to the
  init prereq checks alongside the Step 3 set.
- Log the read `judgment_timing` value on the build line.

**Tests (maintainer).**
- Set P1 JUDGMENT TIMING to **+100 ms**: claps shift audibly by ~100 ms in the expected
  direction (later, per assumption 1). Set **−100 ms**: shift the other way. If the direction is
  backwards, flip `SIGN` and re-verify — this is the sign-validation gate.
- DISPLAY TIMING ±100 ms: **no** change to clap timing (proves we read the right field).
- JUDGMENT TIMING 0: identical to Step 3 behavior.

**Integration.** Adds the one new game-side derivation; missing ⇒ mod disabled. With it, FR-3 is
complete for solo and for the FR-5-chosen side in versus/doubles.

**Demo.** A player who has dialed in a JUDGMENT TIMING hears the clap at *their* judgement moment;
changing the option moves the clap by exactly that amount in the correct direction.

---

## Step 5: Lifecycle hardening, diagnostics, docs, asset cleanup

**Objective.** Cover the remaining edges, finalize diagnostics and docs, and remove the
superseded on-disk bank/build-script assets. Leaves the feature complete.

**Implementation guidance.**
- Rewind guard (FR-7): keep the shipped `REWIND_MS` drop detection, but re-anchor by
  `stop_cue` → `play_tick_track(seek_ms = music_count − m0)` (no rebuild). Late-start seek
  (result ready after the first note) already uses the same seek; confirm both share one code
  path.
- Capacity (FR-8): the `dropped` count from `synthesize_track` produces one WARN naming the count;
  `content_ms < 0` `clipped` count noted on the build line (clap still fires at 0 once).
- Per-song failure paths (FR-6): synthesis panic, `rewrite_tick_wave`/`play_tick_track`/`stop_cue`
  failure → song silent, one latched WARN, state cleared; never a crash on the game thread
  (preserve the judge-callback catch discipline).
- Stop-ordering paranoia: a `stop_cue` immediately before every `rewrite_tick_wave` (no-op in the
  normal next-song case; guarantees the buffer is quiescent — design assumption 3).
- Diagnostics: finalize the once-per-song build/synth/commit/stop lines (bounded; no per-tick
  lines).
- Docs: update `README.md` (the Assist Tick section + Included-Mods row — pre-mixed, judgement-
  aligned, no knob), `AGENTS.md` (the Key-Entry-Points row + config note — `offset_ms` retired,
  new `se_bank_synth` service, tick-bank rewrite mechanism), and add a dated amendment to
  `docs/xact_audio_research.md` §1 recording the notify-thread/packet-grid finding and the
  pre-mix rationale.
- Asset cleanup: remove `data_mods/assist_tick/banks/tick.{xwb,xsb}`, the
  `source/clap.ogg`-only build path, and `scripts/build_assist_tick_bank.sh` (superseded by the
  committed `clap_44k_mono.pcm` + in-DLL synthesis). Remove the debug-dump gating or leave it
  behind the diag flag (agent's call; keep if cheap).

**Tests (offline + maintainer).**
- Maintainer matrix: quick restart mid-song, quick fail, natural finish, exit during lead-in —
  no stuck audio, next song ticks; versus (P2-only; both) and doubles side rules unchanged; a
  >300 s chart (or a dev-lowered cap) shows the truncation WARN + silent tail, no crash;
  optional calibration pass (raise `sound_offset` toward the CrossOver chain latency; game-feel
  and claps converge).
- Log (agent): one registration for the whole session; rewrite only ever follows a stop; failure
  paths emit exactly one WARN each; crash log clean.

**Integration.** Final gates: `cargo check` clean → `cargo fmt` (whole crate) → `./build.sh`
clean. Feature complete.

**Demo.** A full session across many songs, restarts, fail-outs, versus and doubles, with
JUDGMENT TIMING changes — jitter-free, judgement-aligned claps throughout, one immortal bank,
no knob, clean logs; the docs describe the shipped behavior.
