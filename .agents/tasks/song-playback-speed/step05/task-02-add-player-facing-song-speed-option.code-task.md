# Task: Add the Player-Facing SONG SPEED Option (DLL Side)

## Description
Add `SongPlaybackSpeedMod` with the `song_speed` custom-option row — a SCALAR row from 25 to 175 (percent), granular step 5, coarse step 10, default 100 — plus per-side desired-rate atomics, scene-26 eligibility resolution, and the strict row-availability framework additions, making the on-demand runtime from Task 1 player-selectable per e-amusement profile. The mod supplies only desired policy and arm requests; the shared song-rate service remains the sole authority for what actually commits.

## Background
Task 1 generalized the runtime to arm any song at a request-supplied rate but left no production rate source. This task adds the product policy: one row registered once in `init` before the atlas flush, per-side persistence (`PersistMode::Full` — network `mod_song_speed` wire field plus the framework's offline JSON cache), and scene-26 eligibility in which exactly one entered player side selects the shared rate for both audio and clock. Course mode, zero or two entered sides, local versus, missing session pointers, missing services, and the matching/BPL/special scene chains all resolve to 100%.

Maintainer-approved design change (2026-08-07, superseding the design's 75/100/125 enum): the row is a SCALAR option in the style of the arrow/overlay scaling rows — see `src/mods/playfield_styling/mod.rs::register_rows` (`RegisterSpec::scalar(id, min, max, step_fine, ScalarFormat::Integer).step_coarse(...).default_value(...)`). Range 25..=175, `step_fine` 5, `step_coarse` 10, default 100, integer display. Scalar rows render their value numerically, so only the item label texture is needed (no per-value textures).

The existing `lifecycle::classify_scene26` already implements the session-eligibility legs (course field, exactly-one-entered-side, stage index, services-ready) — this task connects the entered side's desired-rate atomic as the percent source and keeps the fail-closed semantics. Precedents in this repo: `src/mods/playfield_styling/mod.rs` (scalar rows + per-side atomics) and `src/mods/player_perspective/mod.rs` (`PersistMode::Full` + gameplay latch).

Two new custom-options framework APIs come from the design: `set_option_available(id, bool)` (registration and persistence stay stable while unavailable rows are omitted by the builder; visibility changes on the next form rebuild, never mutating an open form) and a strict `row_injection_available()` predicate (row allocator + builder + tab filter + required assets, defaulting false until full initialization).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md` (Detailed Requirements: User Behavior + Eligibility; `SongPlaybackSpeedMod`; Configuration and Initialization; Architecture Overview)
- Plan Step 6: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`

**Additional References (if relevant to this task):**
- `src/services/custom_options/mod.rs` (+ `registry.rs`, `builder_hook.rs`, `rows.rs`, `asset_gen.rs`)
- `src/services/custom_options_persistence.rs` (PersistMode wiring, load transform, JSON cache)
- `src/mods/playfield_styling/mod.rs` (scalar-row + per-side atomics precedent) and `src/mods/player_perspective/mod.rs` (`PersistMode::Full` precedent)
- `src/services/custom_options/api.rs` (`RegisterSpec::scalar`, `ScalarFormat`, `step_coarse`)
- `src/services/song_rate/lifecycle.rs` (`classify_scene26`, `ArmRequest`), `src/services/song_rate/runtime.rs` (scene callback, arm sink)
- `src/lib.rs` (init ordering: mod registration before atlas flush)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Create `src/mods/song_playback_speed.rs` implementing the `Mod` trait; register it in `src/lib.rs` (and `src/mods/mod.rs`) at the design's init position — after the song-rate service/clock/wave hooks, before the option-atlas flush.
2. Register the `song_speed` SCALAR row exactly once during `init`: range 25..=175, `step_fine` 5, `step_coarse` 10, `ScalarFormat::Integer`, default 100, `PersistMode::Full`, and a load transform that normalizes persisted values — clamp to 25..=175, snap to the nearest multiple of 5, and map unknown/absent to 100 (the server intentionally stores NULL until a hooked client first saves — the 100 default lives entirely client-side).
3. Add the item label texture via the existing seop generator pattern (`seop_item_song_speed`), generated/registered before the one-time atlas flush; scalar values render numerically so no per-value textures exist.
4. Option callbacks perform no I/O and call no game API: they only normalize and store per-side desired-rate atomics.
5. Add the custom-options framework APIs from the design: `set_option_available(id, bool)` and the strict `row_injection_available()` predicate. The mod registers unconditionally when static capability resolution succeeds and uses availability to hide the row while disabled or while runtime readiness (song-rate service, clock patch, wave hooks, LayeredFS conversion/source-read readiness, score-guard full sanitization) is incomplete. Re-enable changes availability and behavior only; it never re-registers.
6. Scene-26 arming: the mod's (or runtime's) permanent scene callback resolves eligibility and calls the nonblocking arm API with the entered side's desired rate; 100% arms identity. The callback may read atomics/raw session state only — no mutex, no waiting, no I/O, no game function calls, no scene-manager re-entry; a busy arm fails closed to identity with a bounded warning.
7. Next-song-only semantics: option edits during gameplay affect the next scene-26 arm; mid-song edits never touch the active generation. Disabling the mod sets future policy disabled and the row unavailable but keeps the permanent callback and current-attempt state until a definitive lifecycle boundary.
8. The mod reports inactive when the shared service cannot guarantee audio/clock/score/movie integration (readiness predicate false).
9. Update `mod-config.json`'s example surface (mods map entry; `custom_options.row_order` example including `song_speed`) per the design's deliverables list.
10. Host tests (same task): registration-before-flush ordering; boot-disabled then enabled availability flip without re-registration; load-transform normalization (out-of-range clamp, non-multiple-of-5 snap, unknown/absent -> 100); P1/P2 isolation of desired-rate atomics; eligibility matrix (solo P1, solo P2, P2-doubles, local versus, course, zero sides, missing pointers/services) resolving arm-vs-identity; mid-song edit isolation; disable-mid-song retention. Register any new test FILES in both the validator's file list and its generated harness mods.
11. Run all five host gates. Do NOT deploy — cabinet validation is concentrated in Task 4.

## Dependencies
- Task 1 (rate-generic arm model and on-demand runtime) — the arm API this mod calls.
- Existing custom-options framework (registry, builder, persistence, atlas) and seop texture generator.

## Implementation Approach
1. Write failing host tests for the eligibility matrix, load transform, and availability semantics first.
2. Add the framework APIs (`set_option_available`, `row_injection_available`) with tests, without changing existing rows' behavior.
3. Implement the mod: registration, textures, callbacks-to-atomics, readiness gating.
4. Connect scene-26 arming to the per-side desired rates; verify identity arming for every ineligible leg.
5. Run the full gate set; update the canonical planning-dir `progress.md`.

## Acceptance Criteria

1. **Row Registration and Availability**
   - Given a boot with the mod disabled, then enabled at runtime
   - When the options form is rebuilt
   - Then the row was registered exactly once before the atlas flush, is hidden while disabled or not ready, appears after enable without re-registration, and an already-open form is never mutated

2. **Per-Side Policy Selection**
   - Given P1 and P2 profiles carrying different persisted `song_speed` values (including out-of-range, non-multiple-of-5, and unknown/absent values)
   - When profiles load and scene 26 arms
   - Then each side's desired rate normalizes correctly (clamp/snap; unknown/absent -> 100), the entered side's value selects the shared rate, and the other side's value is untouched

3. **Eligibility Fail-Closed Matrix**
   - Given course mode, local versus, zero entered sides, two entered sides, missing session pointers, missing services, and alternate scene chains
   - When scene-26 arming resolves
   - Then every such case arms identity (100%) and only ordinary solo/doubles with exactly one entered side arms the desired rate

4. **Next-Song-Only and Disable Semantics**
   - Given an option edit or mod disable during active gameplay
   - When the current song continues and the next song is selected
   - Then the active generation is unaffected, the next arm reflects the new policy, and disable preserves the permanent callback and attempt state to its lifecycle boundary

5. **Gates**
   - Given the completed implementation
   - When the five host gates run
   - Then all pass, no deployment has occurred, and the option callback provably performs no I/O or game-API calls

## Metadata
- **Complexity**: Medium
- **Labels**: rust, custom-options, mod, ui-policy, persistence, eligibility, host-only, step-6
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-07
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Steps 5+6 (merged delivery, maintainer-approved 2026-08-07) — Step 6: Add player-facing policy, persistence, and backend support
