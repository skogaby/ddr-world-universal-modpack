# Implementation Plan — Training loop / marker / timeline revisions

Status: Approved 2026-09-04

Design: `design/detailed-design.md` (Approved 2026-09-04). Maintainer
pre-authorized moving straight through implementation in the same session;
validation is host tests + the design §7 cabinet checklist.

- [x] Step 1: Shared `run_in_song()` predicate + pure gate math
- [x] Step 2: Gesture gates (READY + loop) with hint toast
- [x] Step 3: Row hierarchy + retained-but-ignored readers
- [x] Step 4: Timeline HUD decorations keyed off the loop latch
- [x] Step 5: Config order, docs, readiness gates

---

## Step 1: Shared `run_in_song()` predicate + pure gate math

**Objective.** Land the one definition of "the run is live and its count is
trustworthy" and the pure decision helpers everything else consumes.

**Guidance.** In `src/services/song_reset/mod.rs` add `pub fn run_in_song()`
composing `first_anchored_frame()` + `current_raw_music_count()` +
`min(chart_end_raw)` per design §4.1; note on `first_anchored_frame`'s doc that
it is a state predicate. In `src/mods/training_mode/driver.rs::loop_step`
replace the inline `first_anchored_frame` + credibility match with
`song_reset::run_in_song()`. In `section_math.rs` add `GestureKind`,
`GestureVerdict`, `gesture_gate`, `decorations_visible` (design §4.2).

**Tests.** `section_math` unit tests: full `gesture_gate` truth table (pre-song
precedence over loop-off; scrub ignores loop), `decorations_visible`. New
`scripts/validate_training_mode.sh` mounting `section_math.rs`
(`validate_auto_calibration.sh` pattern). `cargo check`.

**Integration.** Driver behavior is unchanged by construction (same three
reads). Nothing consumes the new gate fns yet.

**Demo.** `./scripts/validate_training_mode.sh` green; a LOOP ON song still
grinds exactly as before.

## Step 2: Gesture gates (READY + loop) with hint toast

**Objective.** Close the soft-lock and make 4/5/6 loop-only.

**Guidance.** `bounds::on_input_event`: after the scene check, evaluate
`gesture_gate(kind, song_reset::run_in_song(), loop_latched())` for both the
scrub arm and the marker arm (design §4.3). Add `LOOP_HINT_SHOWN` (cleared in
`clear_session_state`); `DropLoopOff` flashes the toast once per song;
`DropPreSong` is `log_debug` only.

**Tests.** Covered by Step 1's pure tests; the callback is a thin dispatcher.
`cargo check`.

**Integration.** Uses Step 1's predicate and gate. `set_marker`/`scrub`
bodies untouched.

**Demo.** Cabinet: press 6 during READY (LOOP OFF) → nothing, song plays
normally; LOOP OFF mid-song press 4 → one toast; 7/9 still scrub.

## Step 3: Row hierarchy + retained-but-ignored readers

**Objective.** START/END become LOOP SONG children and are ignored while LOOP
is off everywhere they are read.

**Guidance.** `mod.rs::register_bound_rows`: register LOOP first, START/END
with `ShowWhen::Equals { training_loop_song, 1 }`; `on_loop_song_change` calls
`refresh_pre_shift()`; `refresh_pre_shift` requires the governing side's loop
row. `bounds::on_scene_change`: `rows_engaged` = any side's loop row.
`bounds::try_resolve_row_bounds`: `!loop_on` ⇒ resolve as defaults (design
§4.5). Update module docs describing the retired LOOP-OFF early end.

**Tests.** `cargo check`; existing `custom_options` registry tests already
cover parent-first validation. Cabinet items 5–6 of design §7.

**Integration.** Builds on Step 2 (gestures already loop-gated, so a retained
END can no longer be reached by any path).

**Demo.** Options menu shows START/END only under LOOP SONG = ON; a LOOP OFF
song with retained START>0 plays whole from 0.

## Step 4: Timeline HUD decorations keyed off the loop latch

**Objective.** Veil + A/B lines only for looping songs; cursor/readout/strip
always.

**Guidance.** `strip_hud::overlay_update`: `decorations_visible(loop_latched())`
gates the veil block and passes `None` to `place_line` for A/B when false
(design §4.6). Update the "ALWAYS shade" comment.

**Tests.** `decorations_visible` (Step 1). `cargo check`. Cabinet item 3.

**Integration.** Reads the same latch the gestures use (Step 2).

**Demo.** LOOP OFF song: strip + yellow cursor + readout only. LOOP ON:
unchanged v1 rendering.

## Step 5: Config order, docs, readiness gates

**Objective.** Ship-ready.

**Guidance.** Reorder the four training ids in `mod-config.json`
`option_menu_settings`. Update `README.md` (Training Mode blurb), `AGENTS.md`
training rows (retired LOOP-OFF early end; READY gate; children), and
`docs/training_mode_research.md` (addendum). Run `cargo check` → `cargo fmt`
→ `./build.sh` → `./scripts/validate_training_mode.sh`. Write `summary.md` and
`progress.md` with the cabinet checklist.

**Tests.** All host suites above green; build clean.

**Integration.** Final; no code behavior change.

**Demo.** Fresh build + config ready for the maintainer's cabinet run through
the design §7 checklist.
