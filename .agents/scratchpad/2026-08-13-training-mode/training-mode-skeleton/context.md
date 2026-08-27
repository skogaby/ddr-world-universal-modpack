# Context — task-03 training-mode skeleton + demo knob

Task file: `.agents/tasks/2026-08-13-training-mode/step01/task-03-training-mode-skeleton.code-task.md`
Approval chain: same as tasks 01/02 — auto mode proceeds.
Depends on task-02 (complete).

## Requirements

1. `src/mods/training_mode/mod.rs`: Mod impl, id `training-mode`, default
   enabled, registered in lib.rs after song_rate/song_reset services are up.
2. On enable + scene-26 eligibility: `set_training_arm(true)`; cleared on
   disable and on ineligible sessions.
3. TEMPORARY demo knob `DDR_TRAINING_TEST_SHIFT_MS` (read once; absent ⇒ no
   mapping): bind-time `set_content_mapping(B(shift_ms), B(2500))`.
4. Mod disabled ⇒ zero footprint.
5. Logging per repo convention.

## Key findings

- **Gap discovered (task-02 addendum needed):** the demo's "true beginning
  never audible" requires the mapping IN PLACE before the engine's bank
  prepare buffers the first packets — those reads happen inside the create
  call, before any mod callback can run. A post-publication
  `runtime::set_content_mapping` call would lose the race by design.
  Task-02's requirement 3 anticipated this ("may carry an initial mapping —
  bind-time pre-shift parameter, plumbed but driven by Step 3"); I deferred
  it there and am implementing it here: `BindContext.initial_mapping_ms`
  applied between `prepare_binding` and registry publication, fed by
  runtime atomics (`set_initial_content_mapping_ms`) the mod writes.
- ms→block conversion (the design's `B(T)`) needs the bank's format — only
  known at bind time. `Binding::ms_to_blocks` (floor onto the main entry's
  block grid) hosts it where the format lives.
- Eligibility: `classify_scene26` (task-02) already applies the full
  ordinary-solo/doubles gate set to training arms — the mod keeps a
  STANDING request (set at enable, cleared at disable) instead of
  duplicating the session predicate; ineligible sessions resolve Identity
  inside the classifier. Observable behavior == the task's ACs.
- Mod gating: enable checks `song_rate::runtime::integration_ready()` —
  self-disables (is_active false) when the streaming integration is absent
  (otherwise every song would arm → EarlyFail → WARN noise).
- Registration: append to `mods_to_register` in lib.rs (services init
  before step-7 registration); add `pub mod training_mode;` to mods/mod.rs.
- Known cosmetic side effect: any arm (identity included) sets
  RATE_ARMED_THIS_BOOT → spawns the maintenance drain + enables the
  diagnostic bank-event timeline (a few INFO lines per song while the mod
  is enabled). Acceptable for Step 1; noted for later refinement.

## Decisions (auto mode)

- Standing-request model (above) — simpler and race-free vs. a per-scene
  callback duplicating the classifier's gates.
- Demo knob read once at ENABLE (not per-arm); sets the sticky initial
  mapping (`shift_ms`, `TRAINING_LEAD_MS = 2500`). Absent/unparseable ⇒ no
  mapping. Marked `// TEMPORARY (Step 1 demo — removed in Step 2)`.
- The initial-mapping cell is sticky until changed — Quick-Restart
  re-creates of the same song re-apply it (correct for the demo: a
  restarted song starts shifted too).
