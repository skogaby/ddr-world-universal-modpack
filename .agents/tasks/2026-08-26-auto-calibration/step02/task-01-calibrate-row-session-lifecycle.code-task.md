# Task: Calibrate Row, Arm State, and Session Lifecycle (Consume-Only)

## Description
Restructure the timing-offsets mod into a directory and add the calibration
session skeleton: the "Calibrate next song?" overlay row at the top of the
mod's GLOBAL SETTINGS section, the arm/consume lifecycle (GAMEPLAY entry
guards, pulsing/refusal toasts, always-flip-OFF at song end), with the actual
measurement stubbed out (Step 3 fills it).

## Background
The design (§1–§2) places calibration inside the timing-offsets mod. This step
delivers everything user-visible about the arm lifecycle so it can be
cabinet-verified independently of the measurement math:

- Arm via the overlay row (in-memory only, never persisted, OFF at boot).
- At scene-28 (GAMEPLAY) entry with the arm ON: side census (exactly one
  entered side ⇒ `Collecting`; two ⇒ `ConsumeOnly` + 3 s "2P MODE DETECTED,
  CALIBRATION DISABLED"; zero/unreadable ⇒ `ConsumeOnly` + WARN, no toast) and
  rate guard (`song_rate` non-identity commit ⇒ `ConsumeOnly` + 3 s "SONG
  SPEED ACTIVE, CALIBRATION DISABLED").
- `Collecting` shows the pulsing "Calibrating..." toast.
- At GAMEPLAY exit (fires for every exit shape): dismiss the toast, log a
  placeholder for the apply, and flip the arm OFF (row re-registered with
  value 0 — `register_enum_row` replaces by key, so no new mod_menu API).
- `song_reset` subscription registered with a no-op body (Step 3 fills it).

Key codebase facts: contributed rows render in insertion order under the
owning mod's section while it is enabled (`src/mods/mod_menu/rows.rs`);
scene callbacks fire outside the manager lock, catch_unwind-wrapped, with
0-indexed `(prev, next)` (`src/services/scene_manager.rs`; GAMEPLAY = 28 in
`src/types/scenes.rs`); `stage_records::side_entered(side) -> Option<bool>`;
`song_rate::clock_patch::snapshot().is_non_identity_commit()`;
`song_reset::on_song_reset(cb) -> usize`.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-26-auto-calibration/design/detailed-design.md
  (§1 restructure + lifecycle wiring, §2 calibration.rs — state, row spec,
  scene callback, flip-OFF mechanism; Requirements 2–4; Error Handling table)

**Additional References (if relevant to this task):**
- src/mods/timing_offsets.rs — the file being restructured (row registration
  order lives in `register_overlay_rows`)
- src/mods/mod_menu/rows.rs — EnumRowSpec, idempotent re-registration,
  remove_rows_for
- src/mods/playfield_styling/mod.rs — the GAMEPLAY-entry latch precedent

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Restructure: `src/mods/timing_offsets.rs` → `src/mods/timing_offsets/mod.rs`
   via `git mv` (content edits limited to the calibration wiring below).
2. New `src/mods/timing_offsets/calibration.rs`:
   - `ARMED: AtomicBool` (default false) + `Session { Idle, Collecting { side: u8 }, ConsumeOnly }`
     in a `Mutex`, plus callback-handle storage for teardown.
   - `pub fn enable()` — registers the enum row, the scene callback, and the
     song_reset callback. Refuses (one WARN, no row) if `scene_manager` or
     `song_reset` is unavailable.
   - `pub fn disable()` — unregisters callbacks, removes the row
     (`remove_rows_for(&["timing_calibrate_next"])`), disarms, dismisses the
     toast if a session was live, resets `Session::Idle`.
   - Row spec per the design §2 (key `timing_calibrate_next`, label
     "Calibrate next song?", values [0,1] / OFF/ON, initial 0,
     `parent_row_key: Some("timing-offsets")`, `on_change` stores `ARMED`).
   - Flip-OFF helper: set `ARMED=false` + re-register the identical spec with
     `initial_value: 0`.
   - Scene callback per the design §2 (entry census/rate guards → session +
     toasts; exit → dismiss + placeholder apply log + flip OFF for any
     non-idle session). Entered-side census factored as a pure function
     `census(p1: Option<bool>, p2: Option<bool>) -> CensusOutcome` for host
     tests.
   - Toast texts exactly: pulsing `Calibrating...`; refusals (3000 ms holds)
     `2P MODE DETECTED, CALIBRATION DISABLED`, `SONG SPEED ACTIVE, CALIBRATION
     DISABLED`.
   - No panics/unwraps in callbacks; no work on the judge hot path (this step
     touches none).
3. `mod.rs` wiring: `mod calibration;` + `calibration::enable()` at the end of
   the mod's `enable()` (after `register_overlay_rows()`), and
   `calibration::disable()` in `disable()`. The calibrate row must be
   registered BEFORE the four offset scalar rows (top of the section) — move
   the calibration row registration into/ahead of `register_overlay_rows()`
   accordingly.
4. The census pure function lives so the validation harness can mount it
   (either in `calibration.rs` if dependency-free is impractical, then in a
   small `census` section of the Step 3 `compute.rs`; simplest: a
   dependency-free `src/mods/timing_offsets/compute.rs` started now holding
   `census` + tests, which Step 3 extends with the decision core — the
   harness already probes for that path).
5. No persistence: the row value is never written to `mod-config.json`
   (untouched `persist_all`).

## Dependencies
- Step 1 (toast service): `show_pulsing`, `flash_with_hold`, `dismiss`.

## Implementation Approach
1. `git mv src/mods/timing_offsets.rs src/mods/timing_offsets/mod.rs`.
2. Create `src/mods/timing_offsets/compute.rs` with `CensusOutcome` +
   `census()` + `#[cfg(test)]` tests (TDD first; harness auto-mounts it).
3. Create `calibration.rs`; wire `mod.rs`.
4. `./scripts/validate_auto_calibration.sh`, `cargo check`, `./build.sh`.

## Acceptance Criteria

1. **Row placement and behavior**
   - Given the timing-offsets mod enabled and the overlay menu open on GLOBAL
     SETTINGS
   - When the timing-offsets section renders
   - Then "Calibrate next song?" is the first row of the section (above SOUND
     OFFSET), toggles OFF/ON, and defaults OFF at every boot

2. **1P arm collects (skeleton)**
   - Given the row ON and exactly one entered player at 100 % rate
   - When gameplay starts
   - Then the pulsing "Calibrating..." toast breathes for the whole song, and
     at song end it disappears and the row reads OFF

3. **2P refusal**
   - Given the row ON and two entered players
   - When gameplay starts
   - Then a 3 s "2P MODE DETECTED, CALIBRATION DISABLED" toast shows, no
     pulsing toast appears, and the row reads OFF after the song

4. **Rate refusal**
   - Given the row ON and a side playing at song speed ≠ 100 %
   - When gameplay starts
   - Then a 3 s "SONG SPEED ACTIVE, CALIBRATION DISABLED" toast shows and the
     row reads OFF after the song

5. **Unplayed arm survives**
   - Given the row ON
   - When the player backs out to attract without playing a song
   - Then the row still reads ON

6. **Census unit tests (host)**
   - Given all combinations of `(Option<bool>, Option<bool>)`
   - When `census` is evaluated
   - Then exactly-one-true yields that side, two-true yields the 2P refusal,
     and zero-true/any-None yields the silent refusal

7. **Regression**
   - Given the restructure
   - When `cargo check --target x86_64-pc-windows-msvc` and the validation
     script run
   - Then both pass and the four offset rows still register and persist
     exactly as before

## Metadata
- **Complexity**: Medium
- **Labels**: rust, timing-offsets, overlay-menu, scene-lifecycle
- **Required Skills**: Rust, this codebase's scene-callback and overlay-row patterns
- **Generated By**: code-task-generator 2026-08-26
- **Source Plan**: .agents/planning/2026-08-26-auto-calibration/implementation/plan.md
- **Plan Step**: Step 2: Calibrate row, arm state, and session lifecycle (consume-only)
