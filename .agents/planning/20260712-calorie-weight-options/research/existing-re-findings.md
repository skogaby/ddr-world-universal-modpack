# Research Notes

The reverse-engineering research for this feature was completed **before** this
PDD session and lives in the repo's main RE-docs tree:

## Primary source

- [`docs/calorie_weight_profile_research.md`](../../../docs/calorie_weight_profile_research.md)
  — the authoritative research doc. Covers:
  - **Storage**: `weight` = `PlayerWork + 0x24` (s32), `is_disp_weight` =
    `PlayerWork + 0x28` (u8/bool), reachable via the same
    `player_work_table[side] → *wrapper = PlayerWork` chain the mod already
    resolves for customize.
  - **Wire format**: ess.dll `<common>` block — `weight` (kbin s32) and
    `is_disp_weight` (kbin bool); parsed by `sys_playerdata_load_receiver`.
  - **Reflect**: `ark::network::ReflectPlayerWork` (20260616 `FUN_180014850`,
    20260324 `FUN_180013c80`) copies staging → PlayerWork; offsets +0x24/+0x28/+0x30
    verified byte-identical across both builds.
  - **Consumer / proof**: calorie calc `FUN_180053430` reads `PlayerWork+0x24`.
  - **Calorie formula (§3.1)**: full `CalcCalorieActor` vtable breakdown +
    accumulation formula.
  - **Cross-version notes** and a **signature basis** for deriving `+0x24` at
    runtime from the calorie calc.

## Related prior research

- [`docs/player_customization_system_research.md`](../../../docs/player_customization_system_research.md)
  — the cosmetic `<customize>` subsystem this feature mirrors (pattern reference).

## Open research item (dynamic)

- **Weight unit / unset-default anomaly** — the calc's unset branch stores
  `F=60.0` vs `weight/100` for a set weight (~100× gap). Round-trip is
  unit-agnostic for the mod, but the option's display range/label should be
  calibrated from an observed value. A Cheat-Engine check (set a known kg via web
  UI, read `PlayerWork+0x24`) will settle it. Tracked as a requirements/impl
  decision, not a blocker.
