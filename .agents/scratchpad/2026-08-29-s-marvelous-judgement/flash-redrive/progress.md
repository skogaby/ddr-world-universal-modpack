# Progress — task-03 flash-redrive

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `capture.rs` shared-capture mode: `SHARED_CAPTURE` flag +
  `tracking_enabled()` (styling enabled OR shared consumer) on both hook
  bodies; styling APPLY still gated on the styling mod (shared-only mode
  tracks + binds with zero visual writes, judge anchor still recorded);
  `install_create`/`install_set_position` idempotent; `remove()` no-op
  while shared (styling's enable rollback can't break the consumer);
  `judge_wrapper_for_side` with live layer-id revalidation
- [x] `overlay_element_styling::ensure_capture_installed(signatures)` +
  `judge_clip(side)` pub API; player-array derivation deduped into
  `derive_player_array` (init + ensure share it)
- [x] `bm2d_api::mc_op_str` (string 3rd arg as u64; op 0xF09
  goto-label-by-string; no unwrap — poisoned-lock returns false)
- [x] `state::apply_event`/`on_judge_event` return `bool` (S-Marv
  classified); tests extended with return asserts
- [x] `src/mods/s_marvelous/flash.rs`: patch_applied gate → judge_clip →
  MC id guard → 0xF09 re-drive; one-shot WARN latches (reset at GAMEPLAY
  entry) + one-shot INFO on the first successful re-drive (cabinet-log
  confirmation)
- [x] Wiring: tap fires `flash::on_smarvelous` on classification; s_marvelous
  init requests the shared capture (best-effort, WARN on miss)
- [x] Gates: cargo check clean (0 warnings) · validate script 85/75 host
  tests + Legs A/B/C/D green · cargo fmt · ./build.sh clean

## Deviations
- `mc_op_str` follows the house mutex pattern but avoids `.unwrap()` on the
  lock (hook-path no-panic rule is stricter than the existing `mc_op`).
- Shared-only bind skips place/opacity but still records the judge anchor
  (cheap; keeps styling's late-enable behavior coherent).
- Commit step skipped per repo AGENTS.md git rules.

## Step 4 sibling status
- task-01 definition-aware-cloning: Complete
- task-02 dance-judge-patch: Complete
- task-03 flash-redrive: Complete (this)
→ Step 4 checklist item ticked. CABINET DEMO = the maintainer's gate.
