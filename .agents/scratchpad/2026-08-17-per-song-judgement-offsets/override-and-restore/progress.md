# Progress: override-and-restore

Updated: 2026-08-18
Status: Complete (uncommitted — maintainer commits manually); cabinet demo pending

- New override_hook.rs (module named override_hook — `override` is a Rust
  keyword): PENDING/ACTIVE/STOCK_CACHE per-side atomics + LOCKED_CODE.
  Lifecycle: latch at 26-entry (ui::current_code) → arm at 28-entry
  (side_entered + event_mode belt-and-braces + store.arm_decision) → write
  at first judge dispatch (real_speed idiom; per-actor course veto via
  actor+0x08→DPS+0x98 course_max_stage>1 — actors don't exist at 28-entry,
  so the course check lives at judge time) → restore on prev==28 (before
  any other transition handling) → SONG_SELECT sweep (restore+WARN).
- Sanity: stock read outside ±100 ⇒ refuse + warn-once. Restore failure
  leaves ACTIVE set on purpose so the trampoline layer still fires.
- Trampoline layer: `leaked_stock(side)` probe + new
  `custom_options_persistence::replace_option_s32` (find 162 → remove 164 →
  get-ctx 175 → add 163 type 6; the add needs the DERIVED ctx from ordinal
  175, not kbin_ctx — matched emit_network_children exactly). Called
  post-original.call in save_sender_trampoline; failure logs log_error.
- Mod lifecycle: init stashes player_option_table; enable registers judge +
  scene callbacks (idempotent, gated on is_active); disable restores any
  live override immediately.
- All service callback dispatchers verified panic-contained (scene_manager /
  judge_hook / input_manager).
- Validation: check clean, harness 23/23, release build clean.
- Cabinet validation COMPLETE across deploys #3–#3d: efficacy, profile
  purity (server opt_timing_music) across natural/quick-restart/quick-fail/
  in-place exits, Training Mode FF/RW, full Dan course per-stage
  application, assist-tick integration. Post-#3 evolution (all cabinet-
  driven): D21 removed the course veto; identity = SSQ-open observer +
  dance-bank-create observer + wheel latch (last writer wins); lazy value
  resolution at first judge; Priority::Early (assist_tick reads
  Option+0x24 on the same dispatch at Normal).

Status: Complete (uncommitted — maintainer commits manually)
