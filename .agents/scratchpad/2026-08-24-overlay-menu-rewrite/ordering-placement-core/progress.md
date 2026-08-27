# Progress — ordering-placement-core (Step 5 task-01)

## Checklist

- [x] Harness script (`scripts/validate_custom_options.sh`; mount checkpoint
      against the pre-rework ordering.rs passed 9/9 — log-stub macro approach
      proven)
- [x] ordering.rs rework (OptionMenuSetting, set_configured_settings,
      placement_override + placement_override_for, order semantics preserved,
      9 tests ported + 6 placement tests added — 15/15)
- [x] config.rs schema swap (row_order deleted; OptionMenuSettingConfig +
      option_menu_settings added)
- [x] mod.rs init read replacement (config structs → ordering type)
- [x] Doc/string sweep (api.rs UiKind::Header, builder_hook comments,
      decorative_option_headers docs + description + enable log, mods/mod.rs
      doc; only intentional "retired/legacy" mentions remain — grep-verified)
- [x] mod-config.json migration — scripted (41 entries, order + p1/p2 +
      all sections preserved, diff purely the array conversion); the
      CABINET's live config migrated with the same script (p1 19 / p2 13
      value keys preserved; backup at /tmp/mod-config.json.pre-migration.bak)
- [x] Gates: validate_custom_options.sh 15/15 + validate_mod_menu.sh 23/23 →
      cargo check 0 warnings → cargo fmt → ./build.sh clean
- [x] Boot regression: deployed DLL + migrated cabinet config; header mod
      logs 4/4 with the new key name; NO unknown-id warning (all 41 ids
      matched); only the 6 pre-existing unrelated WARNs; chrome unaffected.

## Log

- 2026-08-24: setup + artifacts; harness-first checkpoint (9/9 on old code).
- 2026-08-24: rework green 15/15; config/mod/sweep/migrations; gates green;
  cabinet boot regression clean.

## TDD cycles

1. Harness vs unmodified ordering.rs (mount + stub-macro proof, 9/9).
2. Rework with ported tests + new placement matrix — red was the new
   placement API absence (compile), green 15/15 in one pass.

## Deviations

- None from the task spec. The cabinet's live mod-config.json was migrated
  in addition to the repo copy (task named only the shipped file; leaving the
  cabinet unmigrated would have silently dropped the operator's ordering +
  all four headers on next boot).

Status: Complete (uncommitted — maintainer commits manually)
