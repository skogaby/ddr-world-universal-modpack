# Progress — task-03 training-mode skeleton + demo knob

## Checklist

- [x] Test: bind-time initial mapping lands before publication + exact
  ms→block floor conversion (wavebank_hook_tests, +1)
- [x] Impl (task-02 addendum): `Binding::ms_to_blocks`,
  `BindContext.initial_mapping_ms` applied pre-publication in
  `bind_for_create`, `runtime::set_initial_content_mapping_ms` /
  `initial_content_mapping_ms` + create_hook wiring
- [x] Impl: `src/mods/training_mode/mod.rs` (id `training-mode`,
  `TRAINING_LEAD_MS = 2500`, integration_ready gate + honest `is_active`,
  standing `set_training_arm` request, TEMPORARY `DDR_TRAINING_TEST_SHIFT_MS`
  knob — marked for Step-2 removal)
- [x] Impl: registration (`src/mods/mod.rs` + `src/lib.rs` mods_to_register,
  after song_playback_speed — services init before step-7 registration)
- [x] Validate: harness **197 passed / 0 failed** (196 + 1 new)
- [x] Validate: `cargo check --target x86_64-pc-windows-msvc` clean,
  `cargo fmt`, `./build.sh` release DLL built
- [ ] Cabinet demo (maintainer deploys via `./scripts/deploy.sh`) — see below
- [x] Close (no commit — maintainer handles git)

## Cabinet demo checklist (Step-1 exit criteria)

- (a) mod enabled (default), ordinary solo 100 % song, knob absent:
  logs show `TrainingMode: enabled`, `song_rate: generation N armed (100%,
  …)`, `generation N committed (100%, rate X/X, q31 2147483648)`; song
  sounds/plays byte-identical; preview normal; score submits (no taint).
- (b) launch with `DDR_TRAINING_TEST_SHIFT_MS=60000`: song starts with
  ~2.5 s silence, then content at ~1:00; the true beginning is never
  audible; notes/clock NOT adjusted (expected — Step 2/3 work).
- (c) `mods["training-mode"] = false`: no arm lines, ordinary 100 % plays
  literally stock (zero footprint).
- Also worth confirming: versus/course sessions with the mod enabled log
  `scene 26 resolved to identity (LocalVersus/CourseMode)` and play stock.

## TDD cycle

1. Wrote the bind-time-mapping composition test first (compile-fail on the
   missing `initial_mapping_ms` field).
2. Implemented the addendum + skeleton + registration; suite green first
   run (197/197).

## Deviations

- **Task-02 addendum implemented here:** the bind-time pre-shift plumbing
  (task-02 req 3 deferred it as "plumbed but driven by Step 3"); task-03's
  demo requires it NOW — the mapping must be live before bank prepare's
  buffering reads, which happen inside the create call (no post-publication
  call can win that race). `BindContext.initial_mapping_ms` (ms domain,
  converted at bind time where the format is known).
- Standing-request eligibility model (see context.md): the mod sets
  `set_training_arm(true)` at enable / false at disable; the scene-26
  classifier applies the eligibility gates (identical observable behavior
  to the task's per-scene latch wording, without duplicating the predicate).
- Known cosmetic: any arm enables the diagnostic bank-event timeline → a
  few INFO lines per song while the mod is enabled. Accepted for Step 1.

## Files changed

- `src/mods/training_mode/mod.rs` (new), `src/mods/mod.rs`, `src/lib.rs`
- `src/services/song_rate/binding.rs` (`ms_to_blocks`),
  `wavebank_hook.rs` (BindContext field + pre-publication apply + hook
  wiring), `runtime.rs` (initial-mapping atomics)
- `src/services/song_rate/wavebank_hook_tests.rs` (+1)

Status: Complete (uncommitted — maintainer handles git; cabinet demo pending)
