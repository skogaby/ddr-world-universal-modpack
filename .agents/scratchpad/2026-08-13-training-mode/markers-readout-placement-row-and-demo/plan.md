# Plan: markers, readout, placement row, backend + Step-6 demo (task-03)

Status: Approved 2026-08-14 (verified upstream approval — same chain as
tasks 01/02; visual plan + veil amendment maintainer-approved in-session;
auto mode per the code-assist sop)

## Order of work

### 1. Marker asset + placement plumbing (DLL)

- Generate `data_mods/training_mode/tex/training_marker.png` (4×4,
  outline-baked; committed asset) via a transient script.
- strip_hud: `set_placement_left(bool)` per-side atomics + a per-song
  latch at GAMEPLAY entry (entered side via stage_records, P1 fallback);
  `strip_origin()` helper (x from placement + width, y = centered) used
  by poll_resolve AND the overlay.
- mod.rs: register `training_progress_pos` after LOOP SONG
  (`RegisterSpec::enum_values` LEFT=1/RIGHT=0, default RIGHT,
  `PersistMode` default Full) + on_change → strip_hud. Registration
  failure ⇒ WARN + default RIGHT (the row is optional chrome).

### 2. Overlay widgets + pump extension (DLL)

- strip_hud activate(): request-load the marker asset (once,
  process-lifetime) + create the readout/overlay widgets lazily on
  first use (render thread, hidden).
- Pump gains `overlay_update()`: per frame —
  - visible gate (same as the strip: session-active && GAMEPLAY; strip
    ABSENT is fine — fallback track at low alpha when the strip texture
    isn't resolved, marker asset willing);
  - cursor y from `current_raw_music_count()` → layout.y_for_ms;
  - A/B from bounds accessors (poll — gestures move them);
  - veil iff `a.is_some() || b.is_some()` spanning
    `[a.unwrap_or(0) … b.unwrap_or(chart_end)]` (maintainer amendment —
    LOOP plays no role);
  - readout text `format_mss(now) / format_mss(chart_end)` updated only
    when the displayed second changes (cached last string).
  - All positions through the current song's layout (reverse-aware) +
    the latched placement origin. ≤5 widget position writes/frame.
- The layout/chart_end/origin live in a small `Mutex<OverlayGeometry>`
  published by the snapshot (chart_end, reverse, columns) + poll_resolve
  (final rect) — the pump reads it, never engine state directly except
  the music count + bounds atomics.

### 3. Option textures

- gen_option_labels.py: add `timeline_placement` label ("TIMELINE
  PLACEMENT") + `seop_op_` ribbons LEFT/RIGHT (check stock first — the
  game may already ship seop_op_left/right) + a preview explainer.
  Run the script; list outputs for the maintainer's deploy.

### 4. Backend (bemani-buddy — SEPARATE repo, maintainer commits)

- `migrations/015_ddr_world_training_progress_pos.sql`: nullable INT,
  no default (012–014 convention).
- playdata.rs: `mod_training_progress_pos` ↔
  `opt_mod_training_progress_pos` at every touch point the
  preserve-pitch field has (struct, parse, save, defaults, tests
  following the 014 pattern).

### 5. Gates + demo

- Harness (expected: unchanged 300 unless pure math lands) → check →
  fmt → build.sh.
- Step-6 demo checklist (closes the plan step; maintainer-run):
  1. Strip shows the real chart at correct positions/colors (bar mode).
  2. Cursor tracks play; jumps correctly on loop + restart-from-A.
  3. A/B markers appear on gesture; triple-5 clears; the veil shades the
     active section whenever a proper sub-section exists (rows or
     gestures), loop on or off; no veil on whole-song play.
  4. TIMELINE PLACEMENT LEFT moves the whole ensemble to the left edge
     next song; value persists across card-out/in
     (server-verified opt_mod_training_progress_pos).
  5. Mid-song first gesture brings the HUD up.
  6. HUD-failure song plays clean (fault leg — already proven for the
     strip; the overlay adds the marker-asset-missing rung: readout
     only).
  7. Reverse scroll: strip + cursor + markers all run bottom-to-top.

## Test scenarios (host — only new pure math)

The veil span/predicate lands as a small pure helper in strip_synth
(layout-adjacent): `section_veil(a, b, chart_end) -> Option<(i32, i32)>`
— None when no marker (whole song), span clamped/ordered otherwise.
Failure-first tests: no markers ⇒ None; A only ⇒ [a, end]; B only ⇒
[0, b]; both ⇒ [a, b]; degenerate (a ≥ b) ⇒ ordered/None defensively.

## Risks

- Widget/text z-order between our own widgets (creation order) — verify
  on the demo; worst case reorder creation.
- The readout under the strip may collide with stage UI at some scenes'
  bottom edge — demo-tunable constant.
- Placement row wire name `mod_training_progress_pos` must match the
  backend column exactly (single source: the option id).
