# Task: S-Marvelous state module (pure core + atomics)

## Description
Create `src/mods/s_marvelous/state.rs`: the per-side live state for the
S-Marvelous judgement feature — S-Marv counters and the "combo is all
S-Marvelous" tracking bit — built as a pure, host-testable transition core
wrapped in lock-free atomics for the hot path.

## Background
S-Marvelous is a presentation-layer judgement: a stock Marvelous (grade 0)
whose signed ms delta satisfies `|ms| <= window` (default 12, inclusive).
Every display surface in later plan steps reads this module's state. The
classification events arrive from the `judge_submit` detour hot path (game
thread, every judgement), so the armed-path cost must be a handful of relaxed
atomic ops and the disarmed cost one load; no locks, no allocation, no panics
(see AGENTS.md "Rust Quality Rules" #1/#4).

Combo-bit semantics (design §4.3): the bit tracks "the current combo contains
no Marvelous looser than the window". It resets to clean when a combo
(re)starts — detected by observing the actor's combo counter `<= 1` at event
time, reset applied BEFORE classifying that same event. Grade 0 with
`|ms| <= window` increments the S-Marv counter; grade 0 with `|ms| > window`
degrades the bit; grades 1..=3 degrade it (worst-tier parity with stock);
grades 4/5 break the combo (bit self-corrects at the next combo start);
grade 6 (freeze O.K.) is neutral — stock maps O.K. to Marvelous tier and it
carries no ms delta.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-29-s-marvelous-judgement/design/detailed-design.md (§4.3, §5.1)

**Additional References (if relevant to this task):**
- docs/s_marvelous_judgement_research.md §1.3, §2 (event semantics, delta source)
- scripts/validate_auto_calibration.sh (the temp-crate host-test harness pattern to copy)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New module `src/mods/s_marvelous/state.rs` (create `src/mods/s_marvelous/mod.rs` as a minimal module shell if needed for compilation; the full Mod impl is task 03).
2. A **pure core** (no statics, no atomics) implementing the transition:
   e.g. `struct SideState { smarv_count: u32, combo_has_loose_marv: bool }`
   plus `fn apply_event(state: &mut SideState, grade: u32, ms: Option<i32>, combo: i32, window_ms: i32)`. The pure core must have zero dependencies on the DLL crate (mountable by the validation harness).
3. An atomics wrapper around the pure semantics for hot-path use:
   - `WINDOW_MS: [AtomicI32; 2]` (0 = disarmed — doubles as the armed flag)
   - `SMARV_COUNT: [AtomicU32; 2]`
   - `COMBO_HAS_LOOSE_MARV: [AtomicBool; 2]`
   - Public API per design §4.3: `arm(side, window_ms)`, `disarm_all()`, `reset_song_state()`, `smarv_count(side) -> u32`, `combo_is_all_smarv(side) -> bool`, `is_armed(side) -> bool`, and the hot-path entry `on_judge_event(side: usize, grade_index: u32, ms: Option<i32>, combo: i32)` (relaxed orderings; side masked `& 1`).
4. Window clamp helper `pub fn clamp_window(ms: i32) -> i32` (1..=17, default 12 handled by the caller).
5. New `scripts/validate_s_marvelous.sh` cloned from the validate_auto_calibration.sh shape, mounting `src/mods/s_marvelous/state.rs` (plain `cargo test` cannot compile `retour` on non-x86 hosts).

## Dependencies
- None on other tasks (first task of the step).

## Implementation Approach
1. Write the pure core + `#[cfg(test)]` suite first (TDD), keeping it free of crate imports.
2. Wrap in the atomic statics; hot-path fn delegates to the pure transition logic (inline, branch-light).
3. Add the validation script; run it.

## Acceptance Criteria

1. **Window edge is inclusive**
   - Given an armed side with window 12
   - When a grade-0 event with ms = +12 (and one with −12) arrives
   - Then the S-Marv count increments and the combo bit stays clean

2. **Loose Marvelous degrades**
   - Given an armed side mid-combo (combo > 1)
   - When a grade-0 event with |ms| = 13 arrives
   - Then the count does not increment and `combo_is_all_smarv` becomes false

3. **Combo restart resets the bit before classifying**
   - Given a side whose bit is degraded
   - When an event arrives with combo == 1 and grade 0, |ms| <= window
   - Then the bit is clean after the event and the count increments

4. **O.K. neutrality**
   - Given an armed side with a clean bit
   - When a grade-6 event (ms = None) arrives at any combo value > 1
   - Then neither the count nor the bit changes

5. **Grades 1–3 degrade; 4/5 leave the bit to the next combo start**
   - Given an armed clean side
   - When a grade-2 event arrives, then a grade-4 event, then a combo-1 grade-0 tight event
   - Then the bit is degraded after the first, and clean with count incremented after the third

6. **Disarmed is inert**
   - Given `WINDOW_MS[side] == 0`
   - When any event arrives
   - Then no state changes (and the wrapper's guard is a single relaxed load)

7. **Host validation runs**
   - Given the repo on a non-x86 host
   - When `./scripts/validate_s_marvelous.sh` runs
   - Then the pure-core test suite passes in the temp harness

## Metadata
- **Complexity**: Low
- **Labels**: s-marvelous, state, pure-module, host-tests
- **Required Skills**: Rust, atomics, repo host-test harness pattern
- **Generated By**: code-task-generator 2026-08-29
- **Source Plan**: .agents/planning/2026-08-29-s-marvelous-judgement/implementation/plan.md
- **Plan Step**: Step 1
