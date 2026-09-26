# Progress — theme-policy-and-trigger

- [x] Failing tests first (trigger rows / labels / dev knob; policy theme table, `tex_number`,
      `engine_skin`, `skin_name` ≡ row labels, theme names per base, adapter gating, Stock for the
      unadapted bases, era/theme row separation, invariants; marker roots; song-info mode)
- [x] Red confirmed: harness failed to compile on the missing API (23 errors: `Naming`, `ThemeArc`,
      `theme`, `is_era`, `tex_number`, `engine_skin`, `ERA_MAX`, `Decision::Legacy::naming`)
- [x] Implementation: `trigger.rs` (ROW_MAX 9, labels, dev knob via `policy::SKIN_MAX`),
      `policy.rs` (themes, naming, u16 masks, theme rows for the eight Step-1 bases),
      `marker_keys::root_name`, `song_info_logic::mode_for_skin`
- [x] Existing tests migrated from `fixed_arc` to `Naming` (era behaviour unchanged)
- [x] Green: `scripts/validate_ddr_selection.sh` 151 passed (baseline 142)

## Deviations
- `skin_name` now returns the row labels (the logs read `2013-2014` instead of `2013-A`), per D18.

Status: Complete (uncommitted — maintainer commits manually)
