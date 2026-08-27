# Context: driver-and-silent-skip-first-start (Step 3, task-03)

Task: `.agents/tasks/2026-08-13-training-mode/step03/task-03-driver-and-silent-skip-first-start.code-task.md`
Approval chain verified (same as tasks 01/02). Auto mode. Baseline: 229/229.

## Key exploration facts

- **`perform_seek` anatomy** (song_reset/mod.rs:1361): `plan_side_rebuilds`
  → trio ptr checks → frame-tick read → `MSG_PRE_START_ARM` (0x1043)
  broadcast → `seek::anchor_tick(now, delay_wall, t_q, rate)` →
  `MSG_TIMING_ANCHOR` (0x1044) broadcast → per-side trio + R14 writes →
  `reset_side_state` (accumulator/gauge/HUD — the block the ADJUST skips).
  The factored core = everything before `reset_side_state`.
- **0x1043 stays in the shared core**: research §5.4's transaction tail is
  "0x1043 + 0x1044 {now − wall(T_q)}" (docs line 350) and 0x1043 merely
  re-arms the start/input protocol the natural start ran one frame earlier
  — proven safe by the shipped reset; keeping it keeps `perform_seek`
  byte-identical AND the adjust on the proven block.
- **`seek::anchor_tick(now, delay, t_q, snapshot)` = `now + delay −
  wall(t_q)`** — exactly the design's adjust anchor with `delay` =
  lead_wall_ms.
- **Anchor field**: GamePlayActor `+0x160` (i64 tick; research §2.2/§6) —
  set by the game's own 0x1044; nonzero = anchored. First-anchored-frame
  predicate = live DPS step 7 + all actors step 4 + all anchors ≠ 0.
- **`Binding::ms_to_blocks`** (binding.rs:1233): `frames = ms ·
  main_entry.sample_rate / 1000` — the MAIN entry's SERVED-stream grid ⇒
  the values passed to `set_initial_content_mapping_ms` are WALL-domain ms
  (confirmed; the stretch preserves sample_rate, only frame count changes).
- **Wall conversion at arm time**: only the DESIRED percent exists (the
  committed exact ratio is block-quantized later); `wall = content ·
  100 / percent` (Step-2 demo numerics: 175 % A=44861 → ≈25 635 wall ✓).
  The sub-block epsilon vs the committed ratio is ABSORBED by deriving the
  adjust's t_q from the LIVE mapping read-back (below), so clock/audio/
  claps stay mutually exact.
- **Adjust inputs derive from the live binding**: the driver reads
  `runtime::active_content_mapping()` — `None` ⇒ no binding (ladder: song
  plays from 0; a fallback seek would refuse anyway — it needs the same
  binding); `(0, _)` ⇒ pre-shift missed with a live binding ⇒ WARN +
  stop/replay fallback; `(shift, lead)` ⇒ wall ms via a new pure
  `seek::blocks_to_wall_ms` + `seek::content_ms(wall, snapshot)` → t_q,
  lead_wall → `song_reset::adjust_run_to(t_q, lead_wall)`.
- **Scene ids**: SONG_SELECT=25, SONG_TO_STAGE_INTERSTITIAL=26,
  STAGE_INDICATOR=27, GAMEPLAY=28. The bank create fires during 26/27
  loading; refresh points for the sticky pre-shift: the skip row's
  on_change (modal edits happen AT 25), scene entry to 25, scene entry to
  26 (last-chance, also catches SONG SPEED edits changing the wall
  conversion).
- **Entered side at select**: `stage_records::player_work(side)+0x4`
  (quick_logout's PLAYER_WORK_ENTERED_OFFSET precedent). Exactly one
  entered ⇒ that side; both ⇒ clear (versus is ineligible — the classifier
  fails it closed, no binding would consume the mapping anyway); none
  detectable ⇒ P1-preferring side with a nonzero skip (assist_tick's
  "P1 or the only enabled side" class).
- **`runtime::desired_percent(side)`** exists (getter).
- **Driver arming**: bounds::on_scene_change is the mod's single scene
  callback — it calls driver entry/exit hooks. Arm iff rows-resolution
  pending OR mapping shift > 0 (both imply the mod is enabled: pending is
  gated on GESTURES_ACTIVE, the mapping is cleared on disable). Zero rows
  + no shift ⇒ driver never arms (task req 5's zero footprint).
- **DPS step at first anchored frame**: the 0x1044 handler re-enters actor
  step 4; DPS advances 6→7 around the same frames. The DETECTION predicate
  includes DPS step 7 so the one-shot adjust never fires into a
  transiently-refusing gate (the design's 1–2 silent frames tolerance;
  miss processing needs mc ≥ 160 ms — far beyond the detection window).

## Requirements checklist

1. Pre-shift arming: `set_initial_content_mapping_ms(wall(a_row), 2500)`
   kept current at scene 25/26 boundaries + row edits; cleared on skip 0 /
   disable / ineligible.
2. `driver.rs`: render-thread self-requeueing, generation-tokened,
   detects first anchored frame, fires the adjust ONCE per song.
3. `song_reset::adjust_run_to(t_ms, lead_wall_ms) -> bool` — factored
   anchor+trio+neutralization, seek-identical gates, ends in
   `notify_subscribers(t_q)`; `perform_seek` behavior-identical.
4. Fallback ladder: gates fail / pre-shift missed ⇒ one WARN + stop/replay
   `request_reset(a_ms, TRAINING_LEAD_MS, Zero, None)`; that refusing too
   ⇒ song plays from 0 (nothing broken).
5. Zero footprint at skip 0 / mod disabled.
6. Host tests: `blocks_to_wall_ms` (seek_tests, mounted) +
   `pre_shift_wall_ms` (section_math, mounted).

## Files to touch

- `src/services/song_reset/seek.rs` (+seek_tests.rs) — blocks_to_wall_ms
- `src/services/song_reset/mod.rs` — perform_adjust factor-out,
  adjust_run_to, first_anchored_frame, GPA_ANCHOR_OFFSET
- `src/mods/training_mode/section_math.rs` — pre_shift_wall_ms (+tests)
- `src/mods/training_mode/driver.rs` (new)
- `src/mods/training_mode/mod.rs` — pre-shift refresh + driver module +
  row-callback hook + disable clears
- `src/mods/training_mode/bounds.rs` — scene callback calls driver hooks +
  pre-shift refresh at 25/26
