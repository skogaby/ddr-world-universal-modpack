# Progress — registration-surface (Step 5 task-02)

## Checklist

- [x] api.rs: MenuPlacement (+Default both-true) + spec fields (4 ctor sites)
      + 5 builders + EnumValue.display_label/with_display/display_label
      builder + bool_toggle OFF/ON labels + prettify_id/prettify_texture_suffix
      + format_scalar_value moved in (pub(crate)) + format_scalar_value_utf8
      (SJIS ± → "±") + 8 in-file tests (red: 3 helper tests failed at todo!();
      green: 24/24 harness)
- [x] rows.rs fn removal + import; scalar_format_tests.rs → super::api
      (byte pins untouched); unused ScalarFormat import dropped
- [x] registry.rs: RegisteredOption {menus, display_name, description} +
      try_register copy-through + header-validation comment (display strings
      + placement explicitly ALLOWED on headers — stateless)
- [x] builder_hook: snapshot filter drops resolved-!in_game rows
      (ordering::placement_override_for wins over opt.menus.in_game;
      OnceCell read is lock-free — safe inside the STATE lock)
- [x] mod.rs: register_label_for skipped when resolved in_game == false
      (one INFO; menus captured before the spec move); MenuPlacement added
      to the pub use surface
- [x] Harness MODULES += api.rs (24/24)
- [x] Gates: check 0 warnings → fmt → build clean
- [x] Boot probes: `"in_game": false` on timing_stats ⇒
      "label texture skipped" INFO logged, mod still enabled (values/handles
      intact); probe reverted; steady-state boot back to the 6 pre-existing
      WARNs.

## Findings (pre-existing, out of scope)

- Atlas-REBUILD boots (any registered-texture-set change) emit 6 WARNs:
  atlas_cloner can't find seop_op_left/right PNGs — training_mode's
  progress-pos row references stock-atlas chips missing from
  asset_gen::STOCK_RIBBONS, so the cloner conservatively attempts an
  injection clone. Harmless (chips render from the game's stock atlas);
  candidate Step 9 polish: add seop_op_left/right to STOCK_RIBBONS.

## Deviations

- None from the task spec.

Status: Complete (uncommitted — maintainer commits manually)
