# Task: Dynamic markers, readout, TIMELINE PLACEMENT row, backend + Step-6 demo

## Description
The interactive layer over task-02's static strip: the per-frame
current-time cursor, A/B markers, loop-window shading, and the
`m:ss / m:ss` readout; the per-player TIMELINE PLACEMENT enum row
(LEFT/RIGHT, default RIGHT, `PersistMode::Full`); the bemani-buddy
backend column; and the Step-6 demo that closes the plan step.

## Background
- **Markers**: ≤4 small tinted ImageWidgets (a static white marker PNG
  under `data_mods/training_mode/tex/`, mine-texture pipeline — or the
  stock-asset route if one is already resident; implementer's judgment,
  document the choice). Repositioned per frame by a generation-tokened
  self-requeueing render loop (the mod's `toast.rs` pattern — the
  strip's driver may host it): cursor y from
  `song_reset::current_raw_music_count()` through task-01's layout math;
  A/B from `bounds::active_section_start()` / `bounds::section_end()`
  (poll per frame — gestures move them mid-song); the loop window shades
  `[a, fire bound]` while `bounds::loop_latched()`. Seeks/loops need no
  subscription — per-frame polling of the live count follows every jump.
- **Readout**: one TextWidget `m:ss / m:ss` (current/total) beside the
  strip, updated when the displayed second changes (not every frame —
  text re-layout is the only non-constant cost here).
- **Visibility**: strip + markers + readout show only during
  `bounds::training_session_active()` at GAMEPLAY; a session activating
  MID-SONG (first gesture) must bring the HUD up (the per-frame loop
  handles it); everything hides at song end/exit.
- **Fail-open ladder tie-in** (design §6 / R7 amendment): strip absent
  (task-02 failure) ⇒ markers + readout still run on a plain background
  track (the same marker asset stretched, low alpha); marker asset also
  missing ⇒ readout only; nothing blocks the session.
- **Placement row**: `training_progress_pos` "TIMELINE PLACEMENT",
  enum LEFT(1)/RIGHT(0, default), `PersistMode::Full` → wire
  `mod_training_progress_pos` (design R11 + the R7 amendment — the ONE
  training row that persists with the profile). Registered after LOOP
  SONG (player_perspective's `RegisterSpec::enum_values` precedent);
  per-side atomics; the shared HUD reads the ENTERED side's value
  (`stage_records::side_entered`, P1 fallback) each frame — placement
  edits apply at the next song without restart. Label + value-chip +
  preview textures via `scripts/gen_option_labels.py` (maintainer
  deploys to the cabinet's custom_options tex dir; do NOT add any
  `seop_op_item_*.png` — AGENTS.md warning).
- **Backend**: bemani-buddy migration `015_ddr_world_training_progress_pos.sql`
  (`opt_mod_training_progress_pos INT NULL DEFAULT NULL` on the profile
  table, following 012–014's shape) + the `playdata.rs` verbatim
  save/load applier (the assist-tick-volume precedent). Separate repo —
  separate commit handling per the multi-package convention; the DLL
  side works (save-and-load) once the server stores the field.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (R7 amendment, R11, §6)

**Additional References (if relevant to this task):**
- docs/chart_strip_hud_research.md (§2 overlays, §6 scope decisions)
- src/mods/training_mode/toast.rs (generation-tokened render loop)
- src/mods/player_perspective/mod.rs (`register_rows` enum precedent)
- src/mods/training_mode/mod.rs (row registration + entered-side pattern)
- scripts/gen_option_labels.py (texture generation)
- ../bemani-buddy migrations 012–014 + crates/game-server/src/handlers/ddr_world/playdata.rs (backend precedent — sibling checkout)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Marker/readout render loop (panic-free, per-frame work = ≤4 widget
   position writes + occasional text update), gated on session-active +
   GAMEPLAY, generation-tokened per song.
2. Placement row registered with the training rows (Session rows'
   degradation pattern: registration failure ⇒ default RIGHT, one WARN);
   default-value seeding on re-enable like the bound rows.
3. Strip + markers + readout all honor the placement side (mirrored x
   layout; the strip widget from task-02 takes its x from this row too).
4. Textures generated + listed for the maintainer's deploy.
5. Backend migration + applier in the sibling bemani-buddy checkout,
   verbatim storage, following the shipped naming exactly.
6. Host tests: only what task-01's layout module doesn't already cover
   (e.g. cursor-fraction edge cases if new math appears); the full suite
   stays green.
7. Full readiness gates (harness → check → fmt → build.sh) + the Step-6
   demo below.

## Dependencies
- task-01 (layout math), task-02 (strip widget + pipeline).
- Steps 3–5 shipped (bounds accessors, session predicate, side_entered).

## Implementation Approach
1. Markers + readout loop over task-02's strip; fail-open ladder.
2. Placement row + textures + entered-side plumbing.
3. Backend migration + applier.
4. Gates; run the demo with the maintainer.

## Acceptance Criteria

1. **Cursor + readout track play**
   - Given a training-active song
   - When it plays, seeks (restart-from-A), and loops
   - Then the cursor tracks the music count and jumps correctly on every
     seek/loop, and the readout shows current/total m:ss
2. **A/B markers live**
   - Given triple-4 / triple-6 mid-song
   - When each gesture fires
   - Then the corresponding marker appears at the marked position;
     triple-5 clears them; the loop window shades while LOOP is latched
3. **Placement row**
   - Given TIMELINE PLACEMENT edited at song select (either side in a
     solo session)
   - When the next song starts
   - Then the whole HUD renders on the chosen edge, and the value
     persists with the profile (card-out/in round-trip, server-verified
     `opt_mod_training_progress_pos`)
4. **Mid-song activation + fail-open**
   - Given an untouched song where the first gesture fires mid-song
   - When the session activates
   - Then the HUD appears mid-song; and with the strip forced absent the
     markers/readout still run (one WARN, session unaffected)
5. **Step-6 demo (closes the plan step)**
   - Given the full checklist: real chart in the player's noteskin at
     correct positions/colors; cursor tracks; A/B on gesture; loop jump;
     placement left/right; HUD-failure song plays clean
   - When run on the cabinet
   - Then all legs pass (maintainer-verified; plan Step 6 ticks)

## Metadata
- **Complexity**: Medium
- **Labels**: training-mode, hud, custom-options, backend, cabinet-demo
- **Required Skills**: Rust, the custom_options row framework, the widget render-loop pattern, bemani-buddy migrations
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 6: Chart-strip timeline HUD + placement row
