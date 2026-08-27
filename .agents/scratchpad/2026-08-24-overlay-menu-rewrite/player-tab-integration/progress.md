# Progress — player-tab-integration (Step 6 task-02)

## Checklist

- [x] mod.rs — player_side/player_editable/framework_unavailable state +
      selector/banner/banner-backing widget fields; gesture-side capture
      (the completing NUM_0 press's `event.player`); observer subscription +
      scene-change callback at enable (torn down at disable);
      `schedule_coalesced_refresh` (REFRESH_PENDING latch; closure clears
      the latch FIRST so bursts coalesce without losing a trailing event)
- [x] tabs.rs — `editable_sides_now()` (side_entered fail-closed + attract
      band via `ATTRACT_SCENE_MIN/MAX`; scene unknown ⇒ gated);
      `build_player_rows` (framework flag, `resolve_selected_side`
      normalization, `overlay_snapshot(side)` → MirroredRowSnap convert →
      `build_player_tab`); rebuild restructure (player rows precomputed);
      clamp via `render::navigator_for`
- [x] render.rs — `visible_rows(tab)` (PLAYER = 11) / `list_start_y(tab)` /
      `navigator_for`; per-refresh geometry (slots, header bars, cursor,
      selection bar, scrollbar track+thumb all repositioned per tab);
      selector line (`CONFIGURING:  < PLAYER n >` Free / no-arrows
      Locked/AllGated; accent when pinned-focused; selection bar + cursor
      track the pinned focus; footer hint while focused); banner
      (`NO ACTIVE SESSION` / `OPTIONS FRAMEWORK UNAVAILABLE`) over a dark
      strip backing; Scalar values render `formatted` verbatim when present;
      budgets TEXT +2 / CHROME +1
- [x] input.rs — `compute_new_value` factored (Contributed + Mirrored share
      it incl. coarse); Mirrored arm (side read under lock, apply marshaled
      with the gate re-checked INSIDE the closure → set_value; refused ⇒
      one INFO + coalesced repaint); `try_switch_player_side` (pinned focus
      consumes LEFT/RIGHT: Free ⇒ LEFT=P1/RIGHT=P2 among editable; Locked/
      AllGated ⇒ consumed no-op); navigator_for in navigate/selected_row
- [x] Gates: validate_mod_menu 30/30 + validate_custom_options 40/40 →
      check 0 warnings → fmt → build clean
- [x] Autonomous attract walkthrough (keypad `000` injection, tab ×2,
      UP onto selector, RIGHT no-op, DOWN, close): open/close logged, zero
      panics, 6 pre-existing WARNs; screenshots archived in this directory
      (ddr_s6_player_tab/selector/nav.jpg) — visuals are the maintainer's
- [x] Maintainer demo (2026-08-25): mirror AND reverse mirror correct;
      selector correct for one- and two-side sessions; session gating
      correct; round-trip persistence correct — ALL VALIDATED

## Incidents & fixes during validation

1. **Login-flow crash (NOT a Step 6 bug):** scripted login crashed in
   winmm←devenum←quartz←gamemdx — the documented Wine movie-graph crash;
   `scripts/game_nav/launch.sh` was missing `-audiohookdisable` (required
   by the cabinet's `movie_mode: "fallback"`). launch.sh fixed to match the
   run_ddr alias (comment added).
2. **Song-select lockup (pre-existing Training Mode defect, exposed):**
   maintainer's session wedged after ~2 min at song select; log showed
   66,819 "bounds seeded" lines — the highlight watcher's chart-length and
   audio-publication seeders alternately re-stamping ROWS_DIGEST when the
   two publications disagreed persistently (chart='amab'/116 s vs audio
   stuck at 141 s), each seed doing 2 bound rewrites + 4 row writes + a
   pre-shift refresh, per frame. Zero occurrences in the step3-era log
   ONLY because the skew condition hadn't been hit — the loop shape
   predates Step 5/6 (observer wiring added ~3 atomic ops per event, menu
   was closed during the storm). FIX (driver.rs::select_step): the audio
   seeder is now a fallback ONLY when NO chart publication exists at all
   (`chart.is_none()`) — never a competitor; a mismatched chart converges
   via the wheel poll's re-request. Post-fix live session: seeds
   proportional to selections (83 over a long scroll session), exactly 1
   audio-fallback seed (first entry), 0 panics, log size normal.

## Deviations

- launch.sh + training_mode/driver.rs fixes above (outside the task's file
  list; both validation-blocking, both surgical).

Status: Complete (uncommitted — maintainer commits manually)
