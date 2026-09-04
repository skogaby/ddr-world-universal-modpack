# Orientation — Training loop / marker / timeline revisions

Read on 2026-09-04 against the shipped `training-mode` mod (v1 Section
Practice, `.agents/planning/2026-08-13-training-mode/`).

## 1. The surfaces this revision touches

| Surface | Where | Notes |
|---|---|---|
| Marker gestures 4/5/6 + scrub 7/9 | `src/mods/training_mode/bounds.rs::on_input_event` (l.959) | Gates today: `Pressed`, `GESTURES_ACTIVE`, `scene == GAMEPLAY`. **Nothing checks that the run has started.** |
| Marker set | `bounds.rs::set_marker` (l.908) | Reads `song_reset::current_raw_music_count()` (GamePlayActor `+0x178`), quantizes, clamps to `chart_end − 1000`, refuses `≤ 0`. A B-set immediately runs `apply_end_policy()`. |
| End policy | `section_math::end_policy` (l.163) + `bounds::apply_end_policy` (l.694) | LOOP latched ⇒ `ArmLoop` (raise `+0x94`); LOOP OFF + B>0 ⇒ `WriteThresholds` (early natural end — the v1 "play a section once" feature); else `Natural`. |
| Option rows | `training_mode/mod.rs::register_bound_rows` (l.339) | Registration order today: START, END, LOOP, PLACEMENT. All `PersistMode::Session` except PLACEMENT (`Full`). `ShowWhen::Always` everywhere. |
| Row-derived resolution | `bounds.rs::on_scene_change` (l.1022) + `try_resolve_row_bounds` (l.585) | `RESOLUTION_PENDING` set at GAMEPLAY entry iff `start>0 ‖ end<seeded ‖ loop`. Resolution latches LOOP (`LOOP_LATCHED`), then resolves START/END into `ROW_A/ROW_B` → live `A_MS/B_MS` **regardless of the loop row**. |
| Bind-time pre-shift (silent skip-first start) | `mod.rs::refresh_pre_shift` (l.293) / `pre_shift_side` | Reads `bounds::row_start_time(side)` **regardless of the loop row**. Refreshed on START edits + scene 25/26 entries. |
| Timeline HUD | `training_mode/strip_hud.rs::overlay_update` (l.1158) | Draws: fallback track, **veil** (`strip_synth::section_veil` — "ALWAYS shade the active region", whole strip when no markers), **A/B lines** (always rendered, fall back to strip edges), **cursor**, readout. Visibility = latched TIMELINE PLACEMENT ≠ OFF. |
| Loop driver | `training_mode/driver.rs::loop_step` (l.335) | Already waits for `song_reset::first_anchored_frame()` + a count-credibility check before its initial bound compute. |
| Mod-menu ordering | `mod-config.json` → `option_menu_settings` (l.196–214) | Shipped order: `training_loop_song`, `training_progress_pos`, `training_start_time`, `training_end_time`. |

## 2. Finding: the READY window is already characterized (and the pre-anchor count is garbage)

`driver.rs` l.339–364 (cabinet finding 2026-08-14): until the game's own
`0x1044 {now}` anchor lands at GamePlayActor `+0x160` (DPS state 6), the
`+0x178` music count **"reads as the raw frame tick (minutes-since-boot
scale)"**, and even on the first anchored frame it can hold the stale
pre-anchor tick for one frame. The loop driver defends against this with
two predicates; the gesture surface does not.

So during the READY banner, a press of **6** takes `set_marker('B')` with a
bogus `current`:

- if the tick is inside the sanity window (`−60 000..=3 600 000` ms —
  `song_reset::current_raw_music_count`), it survives, clamps to
  `chart_end − 1000` (when the CMA is already up) or passes through
  unclamped (when it isn't), and lands in `B_MS`;
- LOOP OFF ⇒ `apply_end_policy` ⇒ `write_end_thresholds(b)` on a run whose
  DPS is still in its pre-song init states (0..=6).

Which exact link soft-locks is not provable statically (candidates: an end
cascade fired into a pre-song DPS — the same class as the quick-fail
pre-song soft-lock fixed 2026-08-31; a garbage raw threshold the song can
never reach; a stash of not-yet-initialised stock thresholds). It does not
need to be: **every candidate is upstream of the same missing gate** — the
gesture consumed a music count before the run had an anchor. The spice2x
log of the soft-locked session would carry
`TrainingMode: section end B set at N ms (press 6, …)`; `N` pins the link
if the maintainer wants the post-mortem.

### The detector already exists as a shared service predicate

`src/services/song_reset/mod.rs::first_anchored_frame()` (l.1087): live
DPS **at the in-song step (7)**, ≥1 GamePlayActor, every actor at its
in-song step **with a nonzero clock anchor**. Despite the name it is a
state predicate, not an edge: true for the whole in-song phase (an
in-place reset re-writes the anchor), false during pre-song init (READY)
and the song-end tail (DPS 8/9). It is exactly "READY is gone and the
playfield is live".

`quick_restart_or_fail.rs::dps_pre_song()` (l.668) detects the same
window from the other side (DPS step `< 7`) but is private to that mod and
weaker (no actor/anchor check). Both modules carry their own copies of the
DPS step constants (`DPS_STEP_BASE 0x68` / `DPS_STEP_INDEX 0x92` /
`DPS_STEP_IN_SONG 7`) — pre-existing duplication, out of scope here.

Recommendation: gate every training gesture on
`song_reset::first_anchored_frame()` **plus** the driver's
count-credibility check (`current < chart_end`) — or better, hoist that
pair into one `song_reset` predicate both the driver and the gestures call.

## 3. Finding: making START/END children of LOOP touches four consumers, not one

Hiding the rows is the `ShowWhen::Equals { parent_id: "training_loop_song",
value: 1 }` precedent (assist_tick → `assist_tick_volume`;
song_playback_speed → `preserve_pitch`/`sync_movie`). Framework rule
(`custom_options/api.rs` l.312): **the parent must be registered first**, so
LOOP moves ahead of START/END in `register_bound_rows`. Hidden rows keep
their registry values (the preserve_pitch behavior), so the safety
property cannot be "the rows are hidden" — it has to be "a hidden value is
ignored". Every reader of the row values must consult the loop row:

1. `bounds::on_scene_change` — `rows_engaged` must not arm the resolution
   on START/END alone.
2. `bounds::try_resolve_row_bounds` — must not turn START/END into
   `A_MS`/`B_MS` unless LOOP is on (this is also what retires the v1
   "LOOP OFF + section end = early natural end" behavior).
3. `mod.rs::refresh_pre_shift` — a retained START > 0 with LOOP OFF would
   otherwise still pre-shift the bank (the song would start mid-way with
   no loop). Also needs a refresh trigger from `on_loop_song_change`.
4. `strip_hud::overlay_update` — veil + A/B lines keyed off the loop
   state; cursor/readout/strip unconditional.

Live source of "loop on for THIS song": `bounds::loop_latched()` — latched
once per song at resolution, dropped by `on_loop_disarmed` (the driver's
refusal ladder). Gating gestures/HUD on the latch (not the raw row) makes
a mid-song disarm consistent: no loop ⇒ no markers, no veil.

## 4. Consequences worth surfacing

- `section_math::end_policy`'s `WriteThresholds` arm becomes unreachable
  from user input (B can only be set while looping; a disarm restores
  stock thresholds). Pure + host-tested; can stay as dead-defensive code
  or be removed.
- The 7/9 scrub is NOT loop-specific (pure timeline adjuster) but shares
  the garbage-count exposure pre-anchor. The READY gate should cover it;
  the loop gate should not (decision for the maintainer).
- The strip veil's "always shade the active region" 2026-08-15 amendment
  is reversed for LOOP OFF songs by this revision.
- Versus mirror (`MIRRORED_OPTIONS`) is unaffected — same four rows.
- Docs to update: `README.md` (hotkey legend alt text says "set loop
  start/end" already — consistent), `AGENTS.md` training rows,
  `docs/training_mode_research.md`, the v1 design's §4.1/§4.2 statements
  about LOOP OFF sections (leave the archived design; note supersession in
  the new design).
- Shipped `mod-config.json` `option_menu_settings` order should become
  loop → start → end → placement so the children sit under their parent
  (config-driven order; the framework does not auto-nest).
- No new signatures, detours, or offsets: everything rides existing
  `song_reset` predicates and the custom-options framework.

## 5. Unknowns

- U1 — Exact soft-lock link (see §2). Not blocking; a log line would
  settle it.
- U2 — Whether the maintainer wants a hint toast on a refused press
  ("Enable LOOP SONG to set markers" / nothing during READY) or silent
  drops.
- U3 — Whether toggling LOOP OFF should reset START/END to the seeded
  defaults or retain them hidden (preserve_pitch precedent retains).
