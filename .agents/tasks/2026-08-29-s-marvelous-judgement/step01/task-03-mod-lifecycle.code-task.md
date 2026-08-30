# Task: s-marvelous mod lifecycle, config, and per-song logging

## Description
Create the `SMarvelousMod` (`src/mods/s_marvelous/mod.rs`), register it, wire
the arm/disarm/reset lifecycle (scene callback + song_reset subscription),
read the `s_marvelous.window_ms` config key, and emit the per-song INFO log
that constitutes Step 1's cabinet demo.

## Background
Step 1 ships classification + logging only — no art, no AFP, no display. The
demo: cabinet log shows one line per side per song end
(`smarv=N marv_total=M side=S window=W`), autoplay yields
`smarv == marv_total`, and disabling the mod leaves only a relaxed load on the
hot path. Marvelous totals come from the GamePlayActor per-grade counter array
(`actor + 0x1A0`, slot 0) — but the actor is gone by scene exit; instead read
the per-side `ddr::player::Record` mirror or, simpler and already shipped,
count marvelous in the mod's own state: increment a second counter
`MARV_TOTAL[side]` for every grade-0 event in `state::on_judge_event` (extend
task-01's module — the pure core already sees every grade-0 event). The house
lifecycle patterns to follow: announcer_mute (simple mod shape),
playfield_styling (scene-entry latch), power_user_statistics `mod.rs`
(scene callback + song_reset pairing, data_feed::install from init).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-29-s-marvelous-judgement/design/detailed-design.md (§4.10, §5.3, §6)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-29-s-marvelous-judgement/research/orientation.md §2.4, §2.7, §2.9 (song_reset API, lifecycle patterns, Mod trait)
- src/mods/power_user_statistics/mod.rs (scene/song_reset wiring to mirror)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/mods/s_marvelous/mod.rs`: `SMarvelousMod` implementing the Mod trait —
   `id() = "s-marvelous"`, `name() = "S-Marvelous Judgement (12ms)"`,
   `required_signatures() = &["judge_submit"]`, `is_active()` reporting
   self-disable.
2. `init`: call `data_feed::install(ctx.signatures)` (idempotent); on failure
   mark self-disabled with one WARN (fail-open, FR13).
3. Config: add `s_marvelous: Option<SMarvelousConfig>` (with
   `window_ms: Option<i32>`) to `src/mods/config.rs` following the existing
   section pattern (e.g. `non_native_os_support`). Read at `enable`, clamp
   via `state::clamp_window` (1..=17, default 12), one INFO if clamped.
4. `enable`: register a `scene_manager::on_scene_change` callback:
   - `next == scene::GAMEPLAY (28)`: `state::reset_song_state()` then
     `state::arm(0, window)` + `state::arm(1, window)` (both sides; per-song
     latch — mid-song toggles apply next song, D26).
   - `prev == scene::GAMEPLAY`: emit the per-song INFO per side with a
     nonzero event count: `s_marvelous: song end side=S smarv=N marv_total=M
     window=W`, then `state::disarm_all()`.
   - Gate all callback work on the mod's own ACTIVE flag (snapshot-dispatch
     rule: a removed callback may fire once more).
5. `enable`: register `song_reset::on_song_reset(|_t| state::reset_song_state())`
   (in-place restarts / training scrubs never leave scene 28); guard on
   `song_reset::is_available()`.
6. `disable`: disarm, unregister both callbacks, flip the ACTIVE flag.
7. Register the mod in `src/lib.rs` (`mods_to_register`) and declare the
   module in `src/mods/mod.rs`. Default ON (do NOT add to `DEFAULT_OFF_MODS`).
8. Extend `state.rs` with `MARV_TOTAL[2]` (grade-0 event counter, reset with
   the rest) + accessor, updating the task-01 pure core and its tests.
9. No panics in any callback; readiness gates before handoff:
   `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt` (whole crate),
   `./build.sh`, `./scripts/validate_s_marvelous.sh`.

## Dependencies
- task-01 (state module), task-02 (tap block) — this task makes them live.

## Implementation Approach
1. Extend state.rs (marv_total) + tests; update the validation script run.
2. Config struct + mod.rs lifecycle; register in lib.rs.
3. Full readiness gates; hand the build to the maintainer for the cabinet
   deploy (Step 1 demo). Do NOT commit (maintainer commits manually).

## Acceptance Criteria

1. **Registration + gating**
   - Given `judge_submit` resolves
   - When the DLL boots with the mod enabled in `mods`
   - Then the mod registers, arms at GAMEPLAY entry with the configured
     window, and the boot log shows the mod enabled

2. **Per-song log**
   - Given a song is played with judgements on side 0
   - When the scene leaves GAMEPLAY
   - Then exactly one INFO line for side 0 reports smarv/marv_total/window,
     and no line is emitted for a side with zero events

3. **Reset discipline**
   - Given a quick restart or training scrub mid-song
   - When the reset completes
   - Then counters restart from zero (song_reset path; scene 28 never exited)

4. **Config clamp**
   - Given `s_marvelous.window_ms: 25` in mod-config.json
   - When the mod enables
   - Then the effective window is 17 and one INFO notes the clamp; absent key
     ⇒ 12

5. **Disabled ⇒ inert**
   - Given `mods["s-marvelous"] = false`
   - When songs are played
   - Then no arming occurs, no logs are emitted, and the hot path pays one
     relaxed load per judgement

6. **Readiness gates**
   - Given the completed change set
   - When `cargo check` (msvc target), `cargo fmt`, `./build.sh`, and
     `./scripts/validate_s_marvelous.sh` run
   - Then all pass clean

## Metadata
- **Complexity**: Medium
- **Labels**: s-marvelous, lifecycle, config, scene-manager, song-reset
- **Required Skills**: Rust, repo Mod-trait + callback lifecycle patterns
- **Generated By**: code-task-generator 2026-08-29
- **Source Plan**: .agents/planning/2026-08-29-s-marvelous-judgement/implementation/plan.md
- **Plan Step**: Step 1
