# Plan — task-01 state-module

Status: Approved 2026-08-29 (auto mode — verified upstream approval chain
stands in per code-assist Step 3; see context.md)

## Test scenarios (pure core, written first)

1. window edge inclusive: grade 0, ms = +12 / −12, window 12, combo 2 ⇒
   count++ each, bit stays clean.
2. loose marvelous: grade 0, ms = ±13, window 12, combo 5 ⇒ no count, bit set.
3. combo restart resets before classify: degraded bit, then event
   (combo=1, grade 0, ms=3) ⇒ bit clean AND count++.
4. O.K. neutral: clean bit, grade 6, ms None, combo 7 ⇒ nothing changes;
   degraded bit stays degraded.
5. grades 1..=3 degrade: grade 2 at combo 4 ⇒ bit set. grade 4 at combo 0/1
   boundary: grade 4 event itself doesn't clear count; next combo-1 tight
   marvelous ⇒ clean + count++.
6. grade 0 with ms None (defensive) ⇒ no change.
7. clamp_window: 0→1, 1→1, 12→12, 17→17, 18→17, −5→1, i32::MIN/MAX safe.
8. i32::MIN ms doesn't panic (unsigned_abs) ⇒ degrades (way outside window).
9. wrapper sequence test (single #[test], statics are shared): disarmed ⇒
   on_judge_event inert; arm(0,12) ⇒ is_armed(0) && !is_armed(1); events
   flow; reset_song_state clears counts/bit but keeps armed window;
   disarm_all ⇒ inert again.

## Implementation shape

- `src/mods/s_marvelous/state.rs` (std-only):
  - consts `DEFAULT_WINDOW_MS=12`, `MIN=1`, `MAX=17`; `clamp_window`.
  - `SideState { smarv_count: u32, combo_has_loose_marv: bool }` +
    `apply_event(&mut SideState, grade: u32, ms: Option<i32>, combo: i32,
    window_ms: i32)` — the single implementation of the semantics.
  - statics `WINDOW_MS: [AtomicI32;2]` (0 = disarmed), `SMARV_COUNT:
    [AtomicU32;2]`, `COMBO_HAS_LOOSE_MARV: [AtomicBool;2]`.
  - `on_judge_event`: guard `is_armed`, load fields → `apply_event` → store
    (single-writer game thread; relaxed everywhere).
  - `#[cfg(test)]` suite per scenarios above.
- `src/mods/s_marvelous/mod.rs`: shell — module doc + `pub mod state;`.
- `src/mods/mod.rs`: add `pub mod s_marvelous;` (alphabetical).
- `scripts/validate_s_marvelous.sh`: clone of validate_auto_calibration.sh
  shape mounting `src/mods/s_marvelous/state.rs` as `s_marvelous_state`.

## Risks
- Statics shared across parallel tests → one sequential wrapper test.
- Harness mountability → zero `crate::` imports in state.rs (checked by the
  harness itself compiling).
