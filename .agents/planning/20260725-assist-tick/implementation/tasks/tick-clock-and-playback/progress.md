# Progress — Task 03: tick clock + playback

- [x] `SongState` extended per design §5.2: `cursor`, `last_music_count` (`i32::MIN` sentinel),
      `last_delta`, `tick_offset_ms`, `ticks_logged`, `lead_logged`
- [x] Judge-callback clock per design §4.2: identity check vs latched side → rewind guard
      (`partition_point` re-seek, logged) → delta update → adaptive lead → cursor advance past
      every due timestamp → exactly one `play_cue(handle, c"asti", 0.0)` (FR-4, FR-6)
- [x] `adaptive_lead()` — the one named place, §4.2.3 rationale in its doc comment; half the
      observed frame delta clamped to 2..=34 ms, fallback 8 ms
- [x] Play decision computed under the `SONG` lock, game call made after release (design §5.2)
- [x] Measurement logging: first 10 ticks per song (scheduled/actual/delta) + once-per-song
      frame-delta/lead/offset line; nothing per frame after
- [x] **Step 2 scaffolding deleted**: `mod demo` block + `demo::install();` call — the diff of
      `services/game_audio.rs` contains no additions, only the removal (AC8)
- [x] **Operator latency offset** (maintainer-directed amendment, 2026-07-26): `assist_tick`
      config section (`offset_ms: i32`, default 0, positive = earlier) in `src/mods/config.rs`
      (struct + `ConfigFile` field + both fallback literals), latched once per song in
      `rebuild_for`, applied as a third horizon term. Design amended in place (§4.2.3, §5.3,
      Appendix C row 1)
- [x] Gates: `cargo check` 0, `cargo fmt` clean, `./build.sh` 0; installed (sha256 match)

## Deviations

1. **Per-tick measurement lines at info, not debug.** The task specified debug level, but the
   logger boots at `Info` and nothing in the tree ever calls `set_log_level` — debug lines can
   never reach `log.txt`, and reading these numbers out of `log.txt` IS this step's verification.
   Bounded (first 10 per song); Step 6's diagnostic pass demotes or deletes them (plan step 6
   item 2). Recorded at the log site.
2. **Latency knob added mid-step** (maintainer-directed, so an approved design change, not an
   agent deviation): the first listening pass heard claps 100–200 ms late while the log showed
   ±6 ms of schedule — the residual is the audio chain's trigger-to-audible latency (XACT
   once-per-frame submit + DirectSound mixing buffer under CrossOver/Wine), which the half-frame
   lead cannot see. Appendix C row 1 promoted early (config half only; overlay row stays
   deferred). Initial theory blamed the config's `sound_offset: 981` — wrong, the maintainer
   pointed out `timing-offsets` is disabled so that value never applies.

## Verification record

Boot 1 (offset absent → 0): mod registered/enabled, bank pair loaded (5416+262 B); song build
`side=0 results=438 kept=437 first=[8888, 9110, …]` strictly increasing; registration once, slot 4,
both HRESULTs 0; cue `asti` → index 0; first-10 tick deltas `[1,4,-2,3,6,0,6,1,-5,1]` (centred ~0,
spread within half a frame); second song rebuilt once with **no** second registration; gameplay
entry/exit lines once each. **Maintainer heard claps 100–200 ms late** → knob.

Boot 2 (`offset_ms: 150` seeded in the installed `mod-config.json`): clock line reports
`operator offset 150 ms`; tick deltas `[-141,-146,-152,-147,-144,-150,-144,-150,-155,-150]`
(mean ≈ −148, spread ±5) — firing early by the offset as designed. **Maintainer confirmed the
claps are on-sync ("more or less on-sync … the right fix").** Chart-driven-through-misses is
implicitly exercised: the scripted play is input-less, i.e. all misses, and the claps kept time.
No crash records; the only ERROR line is the crash handler's install banner.

Commit deliberately not made (maintainer owns commits).

Status: Complete
