# Progress — Training loop / marker / timeline revisions

Updated: 2026-09-04
Status: Complete (uncommitted — maintainer commits manually); cabinet-validated 2026-09-04
NEXT ACTION: none — maintainer commits the working tree when ready.
Resume protocol: `implementation/plan.md` (checklist) → `design/detailed-design.md` →
`research/orientation.md`.

## Done
- Step 1: `song_reset::run_in_song()` (first_anchored_frame + `+0x178 < min(chart_end_raw)`);
  driver `loop_step` initial compute switched to it (deviation: `min` over sides vs the old
  `find_map` first-side — stricter, matches the marker clamp). `section_math::{GestureKind,
  GestureVerdict, gesture_gate, decorations_visible}` + 4 tests. New
  `scripts/validate_training_mode.sh` (26 tests green).
- Step 2: `bounds::on_input_event` rewritten around the gate: classify → scene → verdict →
  dispatch. `DropPreSong` = log_debug; `DropLoopOff` = one toast/song (`LOOP_HINT_SHOWN`,
  cleared in `clear_session_state`).
- Step 3: LOOP registered first; START/END `ShowWhen::Equals{loop,1}`; `rows_engaged` = any
  loop row; `try_resolve_row_bounds` `!loop_on` ⇒ defaults; `refresh_pre_shift` loop-gated
  + refreshed from `on_loop_song_change`. Row descriptions now say "where the loop
  starts/ends" (label textures unchanged).
- Step 4: `overlay_update` veil + A/B behind `decorations_visible(loop_latched())`.
- Step 5: `mod-config.json` order loop→start→end→placement; README Training blurb; AGENTS.md
  training row; `docs/training_mode_research.md` addendum. `cargo check` ✓ `cargo fmt` ✓
  `./build.sh` ✓ (51.7 s) `validate_training_mode.sh` ✓.

## In flight
Nothing. All work is unstaged in the working tree.

## Deploy & test log
- 2026-09-04 — maintainer cabinet run: "everything is working as expected". Design §7
  checklist passed (READY-window presses inert, loop-only markers + hint toast, child
  rows, retained-but-ignored section, HUD decorations loop-gated, scrub unaffected).

### Cabinet checklist (design §7)
1. LOOP OFF, press 6 repeatedly during READY → no toast, no marker, song normal (the repro).
2. LOOP ON, press 4/6 during READY → nothing; after arrows start, 4/6 set markers.
3. LOOP OFF mid-song press 4 → ONE toast "Enable LOOP SONG to set markers", no marker; press
   6 again → no second toast; HUD = strip + cursor + readout only.
4. LOOP OFF mid-song 7/9 → scrub works, indicator flashes, cursor follows.
5. Options menu: LOOP SONG OFF hides START/END; ON shows them beneath (MODS tab + overlay).
6. START/END set with LOOP ON → toggle LOOP OFF → song plays whole from 0; toggle ON → section
   retained, loop grinds it.
7. Versus: both sides see the same LOOP state + child visibility.
8. Quick restart (1) mid-loop + restart-from-A still work.
Log lines to expect: `TrainingMode: pinpad <name> dropped -- LOOP SONG not latched this song
(hint shown once)` (INFO); pre-song drops are DEBUG only.

## Deviations & open questions
- `run_in_song` folds `min` over both sides' chart ends (driver used first-side). Intentional.
- A mid-song loop DISARM (driver refusal ladder) now also locks out 4/5/6 with the hint toast —
  consistent with "no loop ⇒ no markers" (design D3), but the wording could confuse a player
  whose LOOP row IS on. Watch for it in logs (`loop disarmed` INFO precedes the hint).
- Soft-lock exact link (orientation U1) never pinned; the gate removes every candidate.

## Key facts for a cold resume
- Predicate: `src/services/song_reset/mod.rs::run_in_song`. Gate math: `section_math.rs` tail.
- Latch semantics: `LOOP_LATCHED` set at resolution, dropped by `on_loop_disarmed`.
- Retained-but-ignored readers: `bounds::on_scene_change`, `bounds::try_resolve_row_bounds`,
  `mod.rs::refresh_pre_shift`.
