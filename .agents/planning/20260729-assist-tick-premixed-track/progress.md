# Progress — Assist Tick: Pre-Mixed Tick Track

**Updated:** 2026-07-29
**Status:** FEATURE CODE-COMPLETE (all 5 steps). Remaining: the maintainer's final full-session regression pass (plan Step 5 "Tests"), then commit.

**NEXT ACTION:** deploy the final build and run the closing regression matrix: a full session
across several songs — natural finish, quick restart, quick fail, exit during lead-in — plus
versus (P2-only and both) and doubles; JUDGMENT TIMING at a real value (e.g. ±15) behaves
sanely; logs show one registration for the whole session, per-song build/synthesis/commit
lines, stop on every exit, no WARNs beyond expected ones. Then the maintainer commits (the
agent does not). Optional calibration pass: raise `sound_offset` toward the CrossOver chain's
real latency and confirm game feel and claps converge together.

**Resume protocol:** read `design/detailed-design.md` (Approved) → this file →
`implementation/plan.md` checklist. `idea-honing.md` holds the 11 accepted decisions + the
timing-model rationale; `research/` holds the RE record. The RE spike named + plate-commented key
functions in the shared Ghidra project (`xactengine2_10.dll`, `gamemdx_*`).

---

## Done

- **Planning (2026-07-29):** RE spike into `xactengine2_10.dll` proved no sample-accurate / no
  future-scheduled cue-start primitive (packet-grid, notify-thread pump) ⇒ pre-mix is the only
  jitter-eliminating design. Offset chain resolved via `gamemdx` `FUN_18005f100`: the clap must
  target the judgement moment = `t_i + JUDGMENT_TIMING − SOUND_OFFSET − m0`. Register D1–D11
  accepted; design + plan approved.
- **Step 1 (2026-07-29): synthesis module + clap asset — implemented and offline-validated.**
  - New `src/services/se_bank_synth/`: `adpcm.rs` (mono MS-ADPCM encoder, byte-faithful port),
    `xsb.rs` (SE-profile writer, byte-faithful port), `xwb.rs` (fixed-header one-entry writer
    exposing the rewritable sample segment), `containers.rs` (public API:
    `build_tick_containers()` / `synthesize_track()` + `TICK_RATE_HZ`/`TICK_CAPACITY_MS`/
    `BANK_NAME` constants — **zero crate deps**, so the format layer compiles stand-alone on a
    host for validation), `mod.rs` (re-exports + `load_clap_pcm()` + the dev-gated debug dump).
    Registered in `src/services/mod.rs`; dump call wired into `src/lib.rs` after LayeredFS init
    (spawns its own background thread; no-op outside `layeredfs.developer_mode`; writes
    `data_mods/_cache/assist_tick_synth/tick.{xsb,xwb}` with a 40×500 ms test pattern).
  - Clap asset committed: `data_mods/assist_tick/clap_44k_mono.pcm` (9,423 samples — exactly the
    documented clap length), converted from the shipped `source/clap.ogg` via
    `ffmpeg -i clap.ogg -ac 1 -ar 44100 -f s16le`.
  - Fixed-shape numbers (from `TICK_CAPACITY_MS = 300_000`): 103,360 blocks → declared duration
    13,230,080 samples; `sample_seg_offset = 0xEC (236)`; `sample_seg_len = 7,235,200 B`; XWB
    total 7,235,436 B; XSB 262 B.
  - Offline validation: **`scripts/validate_se_bank_synth.sh`** (kept, repo-committed) builds a
    throwaway host harness compiling the format submodules alongside the sibling
    `ddr-chart-tools` as a path dep. ALL 36 CHECKS PASSED: XSB **byte-identical** to sibling
    `write_se("asti")`; XWB parses with the sibling parser and matches every engine-validator
    rule (flags 0x00090000, header_version 42, rigid segment layout, whole blocks,
    duration/loop rules); segment-4 descriptor == `sample_seg_offset/len` == EOF; encoder
    output **byte-identical** to sibling `adpcm::encode` for the same mixed buffer; all 40
    test-pattern onsets **sample-exact** (cross-correlation peak at lag 0 through the sibling
    decoder); tail + pristine segment decode to exact silence; clip/drop bookkeeping correct;
    synthesis deterministic.
  - Gates: `cargo check` clean → `cargo fmt` (whole crate) → `./build.sh` clean (0 warnings).
  - The shipped per-tick mod is untouched and still fully functional; old
    `banks/tick.{xwb,xsb}` + `build_assist_tick_bank.sh` deliberately kept until Step 5.
- **Step 2 (2026-07-29): `game_audio` rewritable tick bank + dev trigger — code complete;
  awaiting the maintainer's live proof.**
  - `src/services/game_audio.rs` additions: `TickBankRequest` / `TickBankHandle` (Copy, addresses
    as `usize` for Send), `register_tick_bank` (idempotent; own decline latch
    `TICK_REGISTER_DECLINED`, separate from the shipped path's; reuses the verified-engine +
    manager-sanity gates and the leak→`CreateInMemoryWaveBank`→`CreateSoundBank` sequence;
    captures the leaked XWB's sample-segment pointer), `rewrite_tick_wave`
    (`copy_nonoverlapping`, exact-length contract), `play_tick_track(seek_ms)` (direct
    `SoundBank::Play` vt+0x20, `timeOffset` = seek, `ppCue` NULL), `stop_cue` (direct
    `SoundBank::Stop` vt+0x28, flags=1 immediate). Vtable byte-offsets 0x20/0x28 re-verified
    against the engine binary's sound-bank vtable in Ghidra this session (slot 0 =
    `GetCueIndex` 0x423d00 matches the shipped code's vt+0x00; Play 0x423990; Stop 0x423b80;
    beware: the first "xrefs to Play" hits are `.pdata` RUNTIME_FUNCTION entries, the real
    vtable is at 0x402ea0).
  - **APPROVED DEVIATION (maintainer, 2026-07-29): the tick bank claims NO manager sound-bank
    slot** (design/plan said "reuse the slot-claim path"). Reasons, from the shipped feature's
    own exhaustive RE (`20260725-assist-tick/research/bank-slot-and-anchors.md`): the slots'
    only readers are the `se_play`/`se_prepare` façade, which the tick path never uses (it
    needs Play's `timeOffset`, which the façade hardwires to 0); slot 5 is the game's per-song
    bank slot, so mid-song registration (= any judge dispatch) with the shipped bank still in
    slot 4 would find NO free slot and the Step-2 demo could never sound; and a slot-less bank
    is invisible to the game's `file_id`-searching bank destroyer — strictly safer under the
    immortal-bank rule. The named-bank-count gate is likewise not consulted (it protects only
    the free-slot assumption).
  - Temporary dev trigger `src/mods/assist_tick_dev_trigger.rs` (own judge subscriber
    `Priority::Late` + scene callback; wholly independent of `AssistTickMod` so Step-3 removal
    is deletion): gated on new TEMPORARY config `assist_tick.dev_tick_bank` (default false) +
    `assist_tick.dev_seek_ms`; per song synthesizes a 20 s metronome on a background thread —
    spacing alternates 500/250 ms so a successful rewrite is audibly distinct — and commits on
    a judge frame (register once → paranoia stop → rewrite → play(seek)); stops on gameplay
    exit. lib.rs init call after `game_audio::init`.
  - Gates: `cargo check` clean → `cargo fmt` → `./build.sh` clean (0 warnings). NOT deployed yet.
- **Step 3 (2026-07-29): `mods/assist_tick.rs` reworked to the pre-mixed track — code
  complete; awaiting the maintainer's listening pass.**
  - Kept verbatim (D11): option row + persistence, scene wiring, enable latching,
    `build_tick_list` (FR-2 predicate + 4 ms coalesce), side selection (sibling walk + FR-5
    choice + degraded mode), once-per-song diagnostics shape.
  - Removed: the per-frame cursor / adaptive-lead / `play_cue` clock, `TICK_OFFSET_MS`, the
    overlay child row, the old on-disk bank load (`BANK_BYTES` / `read_bank_file`), the Step-2
    dev trigger (file + `mod.rs` line + `lib.rs` call + `dev_*` config keys).
  - `assist_tick.offset_ms` RETIRED: config field is now `Option<i32>`, parse-but-ignore; one
    INFO at enable when present. Nothing writes the section back.
  - New per-song state machine (`Phase`: Idle → AwaitAnchor → Building → Ready → Playing):
    first judge dispatch builds the list + reads `SOUND_OFFSET` (chosen actor `+0x16c`); the
    first CHOSEN-side dispatch latches `m0` and spawns background synthesis
    (`content_ms(i) = t_i − SOUND_OFFSET − m0`; per-song `generation` token invalidates stale
    threads); a later chosen-side dispatch commits: register-once (`ensure_tick_bank_registered`,
    containers built on demand) → paranoia stop → `rewrite_tick_wave(encoded, skip)` with
    `skip = shift_bytes_for_ms(mc − m0)` → play. The m0 epoch cancels in the shift, so claps
    land at judgement moments regardless of when the track starts. Rewind guard (REWIND_MS
    1000) re-anchors via the same commit path from the retained `Arc<Vec<u8>>` (no resynthesis)
    — FR-7 landed early (plan had it in Step 5) because the unified shift makes it ~free.
  - Failure paths (FR-6): synthesis panic / register / rewrite / play failure ⇒ song Idle +
    one latched WARN; scene exit/entry + mod-disable stop the cue (disable would otherwise
    leave a track playing for up to 300 s with the callbacks gone).
  - Gates: `cargo check` clean → `cargo fmt` → `./build.sh` clean (0 warnings).
- **Step 3 listening (2026-07-29): PASSED** — maintainer-validated: claps perfectly in sync
  with the music, misses ignored, solo sides OK, quick restart clean. **STEP 3 CLOSED.**
- **Step 4 (2026-07-29): JUDGMENT TIMING — code complete; awaiting the ±100 ms ear test.**
  - New signature `player_option_ctx_load` (`src/core/signatures.rs`): the per-side
    context-table load inside the per-frame count computation (`FUN_18005f100`-equivalent).
    Actor-field displacements literal, build-varying displacements wildcarded. Verified to
    match EXACTLY ONCE on 20260324 / 20260421 / 20260616 / 20260721 (Ghidra, this session).
  - New derivation `derive_player_option_table`: validates the anchor's `LEA R12` resolves
    to the MODULE BASE (fail-closed — refuses if a compiler change re-anchored R12), then
    `player_option_table = base + the MOV's disp32` (0x6EBE50 / 0x6F1EE0 / 0x6F2ED0 across
    verified builds).
  - Mod: `required_signatures = ["player_option_table"]` (FR-6/NFR-4 — derivation missing ⇒
    the mod never appears, no degraded mode); per-song read at build time:
    `Option(side) = *( *(table + side*8) ) + 0xE0`, `timing_music` at `+0x24`, sanity-clamped
    to ±100 (out-of-range or null chain ⇒ 0 for the song + one WARN, fail-soft per song).
    Formula now `content_ms = t_i + SIGN·JT − SOUND_OFFSET − m0` with
    `JUDGMENT_TIMING_SIGN = +1` (assumption 1 — the ear test's one-line flip). Build line
    logs `judgment_timing=`.
  - Gates: `cargo check` clean → `cargo fmt` → `./build.sh` clean.

- **Step 4 ear test (2026-07-29): PASSED — sign confirmed.** Via a temporary 10× amplifier
  (`JUDGMENT_TIMING_DEV_SCALE`, since removed): JT +100 → claps 1 s LATE, −100 → 1 s EARLY ⇒
  `JUDGMENT_TIMING_SIGN = +1` is correct (assumption 1 validated). Earlier in the step: the
  first deploy failed cleanly on a derivation off-by-one (TABLE_DISP 45 → 46, the fail-closed
  out-of-module gate caught it); fix verified offline against all three builds' raw bytes.
  **STEP 4 CLOSED.**
- **Step 5 (2026-07-29): hardening/diagnostics/docs/cleanup — complete.** Most hardening had
  already landed in Step 3 (unified block-shift = late-start + rewind; latched failure WARNs;
  stop-before-rewrite; per-song diagnostics). Step 5 added: the FR-8 truncation WARN (dropped
  ticks past the 300 s cap now log loudly, once per affected song); README rework (Assist Tick
  section + Included-Mods row — pre-mixed, judgement-aligned, no knob, PCM swap instructions;
  `assist_tick` config block dropped from the example); AGENTS.md (Key-Entry-Points row +
  config note rewritten for the new architecture); `docs/xact_audio_research.md` (dated §1
  amendment: packet-grid notify-thread starts, timeOffset-is-not-a-seek, rewrite-in-place,
  global name pairing, no-slot rationale; §4 vtable table + §5 generator references updated).
  Asset cleanup: deleted `data_mods/assist_tick/banks/tick.{xwb,xsb}` and
  `scripts/build_assist_tick_bank.sh` (superseded by `clap_44k_mono.pcm` + in-DLL synthesis;
  `source/clap.ogg` kept as the sample's source of truth). The dev debug dump stays behind
  `layeredfs.developer_mode` (cheap, useful). Gates + offline harness: all clean/passing.

## In flight

Nothing mid-edit. Feature code-complete; NOT committed (maintainer owns commits).

## Deploy & test log

| Date | What | Result |
|---|---|---|
| 2026-07-29 | Planning only — no build, no deploy | — |
| 2026-07-29 | Step 1: offline harness `scripts/validate_se_bank_synth.sh` (host) | ALL 36 CHECKS PASSED |
| 2026-07-29 | Step 1: `cargo check` / `cargo fmt` / `./build.sh` | all clean |
| — | Step 1 demo (dev-mode boot dumps `_cache/assist_tick_synth/tick.{xsb,xwb}`) | pending maintainer boot (optional — the same code paths are already validated offline) |
| 2026-07-29 | Step 2: `cargo check` / `cargo fmt` / `./build.sh` | all clean (0 warnings) |
| 2026-07-29 | Step 2 attempt 1 (dev_tick_bank=true, CrossOver) | **PARTIAL — bug found + fixed.** Heard ONE clap at song start, then silence. Log: engine ACCEPTED the 7.2 MB bank (`tick CreateInMemoryWaveBank hr=0`, `tick CreateSoundBank hr=0`), synthesis 694 ms, commit `rewrite -> true, play -> true` — the big-entry assumption is PROVEN. Root cause of the silence: **internal-name collision** — the shipped bank (also `asti`, registered unconditionally at the same first judge dispatch even with the option OFF) pairs by name globally, so the tick sound bank resolved wave 0 of the shipped 214 ms clap wave bank. Fix: tick bank renamed `astk` (`se_bank_synth::BANK_NAME` + trigger cue). Harness re-run: ALL 37 CHECKS PASSED (incl. byte-identity for both names); gates clean. |
| 2026-07-29 | Step 2 attempt 2 (dev_tick_bank=true, CrossOver; log_0.txt + log_5000.txt) | **4/5 PASSED, seek REFUTED.** Even 20 s metronome ✓; 500↔250 ms alternation across songs (**rewrite-in-place PROVEN** — no double-buffer fallback needed) ✓; ≥3 songs, one registration ✓; stop on exit, no stuck audio, no crash ✓. FAILED: `dev_seek_ms=5000` — pattern still started at 0. Ghidra trace: `timeOffset` only fast-forwards the cue EVENT timeline (`Wave_ComputeScheduledStartMs`), an already-due wave starts at sample 0 (`Wave_StartNow_NoSampleOffset`) — **no in-wave seek exists in this engine**. |
| 2026-07-29 | Block-shift replacement implemented (maintainer-approved): `rewrite_tick_wave(h, encoded, skip_bytes)` + `shift_bytes_for_ms` + `silence_block`; `play_tick_track` always timeOffset 0; design doc amended (dated note). Harness extended | ALL 41 CHECKS PASSED (incl. shifted-rewrite onsets land shift-exactly through the sibling decoder); `cargo check`/`fmt`/`./build.sh` clean (0 warnings) |
| — | Step 2 attempt 3 (shift test only: dev_seek_ms=5000 → metronome starts 5 s into the pattern; other 4 checks already passed) | **PASSED (2026-07-29).** Claps from song start for ~15 s (= 20 s pattern minus the 5 s shifted off the head — exactly the shift semantics). Log: `rewrite(shift 5000 ms = 120610 bytes)` = 1723 blocks = 5000.98 ms. **STEP 2 CLOSED** — every engine assumption proven live under CrossOver. |
| 2026-07-29 | Step 3: rework built; `cargo check` / `cargo fmt` / `./build.sh` | all clean (0 warnings) |
| 2026-07-29 | Step 3 listening (ASSIST TICK row ON; 16th-burst evenness A/B, misses, solo P1/P2, quick restart) | **PASSED** — "perfectly in-sync with the music"; quick restart validated. STEP 3 CLOSED |
| 2026-07-29 | Step 4: JT derivation + FR-3 term built; gates | all clean |
| 2026-07-29 | Step 4 deploy attempt 1 | **FAILED CLEANLY — off-by-one, fixed.** `player_option_table -- table displacement 0x6F2ED0CC is outside the module` → mod skipped (the fail-closed gate working as designed). Cause: `TABLE_DISP = 45` read the MOV's 0xCC SIB byte into the disp32; correct offset is **46** (opcode+ModRM+SIB span match+42..46). Fix verified offline against the raw pattern-hit bytes of all three builds (LEA→base ok; table RVA = 0x6EBE50 / 0x6F1EE0 / 0x6F2ED0); gates clean. Also triaged `SongLimitExpansion: init() skipped — early_apply already ran` — pre-existing SUCCESS-path trace (in every prior log incl. both Step-2 logs), not a regression. |
| 2026-07-29 | Step 4 ear test (10× amplifier build) | **PASSED** — +100 → 1 s late, −100 → 1 s early; sign +1 confirmed. Amplifier removed. STEP 4 CLOSED |
| 2026-07-29 | Step 5: FR-8 WARN, docs, asset cleanup; gates + offline harness | all clean, ALL CHECKS PASSED |
| — | Final full-session regression pass (several songs, restart/fail/exit, versus, doubles, real JT values) | **PENDING — the feature's last gate before commit** |

## Deviations & open questions

- JUDGMENT TIMING **sign** — one named constant, validate by ear (Step 4 ±100 ms test).
- **APPROVED (2026-07-29): `Play(timeOffset)` is NOT a seek — block-shift replaces it** (design
  amended with a dated note). `timeOffset` only fast-forwards the cue EVENT timeline; an
  already-due wave starts at sample 0 (no in-wave seek primitive exists). Replacement: the
  rewrite copies `encoded[skip_bytes..]` + silence-fills the tail (`rewrite_tick_wave(h,
  encoded, skip_bytes)`, `se_bank_synth::shift_bytes_for_ms`, 2.90 ms block granularity,
  ≤ 1.45 ms rounding). `skip = mc_now − m0` at commit UNIFIES normal start / late start /
  rewind re-anchor (past content shifts out of the track); `Play` always gets timeOffset 0.
  Claps whose moment passes before synthesis completes (~0.7 s of lead-in on this cabinet)
  are inherently un-clappable; can be shrunk later by encoding only through the last clap.
- **Rewrite-in-place race: RESOLVED (2026-07-29)** — attempt 2 proved alternating rewrites
  across ≥3 songs play the new content cleanly. The double-buffer fallback is retired.
- Reserved **config-only trim key** (no UI) if a residual fresh-start constant appears on the
  track voice (≈38 ms per-trigger asymmetry was measured on CrossOver for the *old* per-cue path;
  the track voice may differ).
- **APPROVED (2026-07-29): no manager slot for the tick bank** — see Step 2 notes above. Carries
  into Steps 3–5: `TickBankHandle` is the only route to the bank; `play_cue`/`se_play` are never
  used for it.
- **Tick bank internal name is `astk`, NOT `asti`** (cabinet-confirmed defect, 2026-07-29): the
  engine pairs sound↔wave banks by internal name GLOBALLY, so sharing the shipped bank's name
  cross-paired the two banks while they coexist (one 214 ms clap, then silence). Steps 3–5:
  when the shipped bank is deleted in Step 3, `astk` stays (renaming back buys nothing and
  risks re-collision on any install that boots an old DLL once).
- Step 1 layout deviation (recorded, benign): the plan sketched submodules
  `adpcm/xwb/xsb/mix.rs`; implemented as `adpcm/xwb/xsb/containers.rs` — the mixer lives in
  `containers.rs` with the public API so the whole pure layer has zero crate deps (what makes
  the host-side offline harness possible). API surface is exactly as designed.
- Step 1 validation harness: implemented as a Rust host harness generated by
  `scripts/validate_se_bank_synth.sh` (path-dep on the sibling repo) rather than replaying
  `build_assist_tick_bank.sh`'s shell checks — strictly stronger (byte-identity + decoded
  onset positions, not just header fields).
- Dev-trigger simplification (accepted, Step-2-only): a quick restart landing inside the
  ~100–300 ms synthesis window can commit the *previous* song's pattern spacing (no song
  token on the background thread). Harmless for the metronome test; the real Step-3 flow
  carries per-song identity.

## Key facts for a cold resume

- **Feature:** one pre-mixed tick track per song, played as a single XACT cue; replaces the
  shipped per-tick `se_play` clock. Same engine ⇒ same mixer clock as the music ⇒ zero
  within-song drift; only a constant per-song start offset remains.
- **Timing (FR-3):** `content_ms(i) = t_i + SIGN*JUDGMENT_TIMING − SOUND_OFFSET − m0`.
  `SOUND_OFFSET` = `GamePlayActor+0x16c` (per-song latch). `JUDGMENT_TIMING` = per-side
  `ddr::player::Option+0x24` (reached via `side_ctx[side]+0xe0`; needs a new RIP-decode
  derivation in `signatures.rs`). `m0` = raw `music_count` at track start. INPUT/RENDER/DISPLAY
  offsets are display-count-only — NOT consumed.
- **No knob** (D3/FR-4): overlay row removed; `assist_tick.offset_ms` parsed-but-ignored.
- **No fallback** (D5/FR-6): boot prereq failure ⇒ mod disabled; per-song failure ⇒ silent song
  + one WARN. The per-tick path is deleted, not kept.
- **Bank (D6/NFR-2):** one immortal in-memory ADPCM wave bank, entry declared at 300 s
  (7,235,200 B segment at offset 236), **no manager slot (approved deviation)**, internal
  name **`astk`** (not `asti` — collision with the shipped bank); per song rewrite ONLY the
  sample bytes (header immutable), after an immediate `SoundBank::Stop` (vt+0x28, flags=1).
  **Late start + rewind + normal start are ONE mechanism: block-shifted rewrite**
  (`rewrite_tick_wave(h, encoded, skip_bytes)`, skip = `mc_now − m0` block-rounded via
  `shift_bytes_for_ms`) — `Play` (vt+0x20) always gets timeOffset 0 (it is NOT a seek;
  live-refuted). Engine API surface lives in `services/game_audio`
  (`register_tick_bank` / `rewrite_tick_wave` / `play_tick_track` / `stop_cue`), all
  game-thread-only.
- **Synthesis (D10/NFR-1):** DONE (Step 1) — `services::se_bank_synth`, mix + MS-ADPCM encode
  pure-CPU/any-thread; clap ships as `data_mods/assist_tick/clap_44k_mono.pcm` (raw mono i16).
  Offline validation: `./scripts/validate_se_bank_synth.sh` (needs the sibling
  `ddr-chart-tools` checkout).
- **Unchanged from shipped mod:** FR-2 predicate, 4 ms coalesce, FR-5 side selection, per-song
  enable latch, the `assist_tick` option row.
- **Verification split:** agent = builds + offline container validation + log reading; maintainer
  = all listening/gameplay. No unit-test harness.
