# Progress — task-03 mod-lifecycle

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] state.rs extended: `marv_total` (pure core field + MARV_TOTAL static +
  accessor + reset), all tests updated — 10/10 pass (logs/validate.log)
- [x] config.rs: `SMarvelousConfig { window_ms }` + `s_marvelous` field in
  ConfigFile (+ both default-literal sites)
- [x] src/mods/s_marvelous/mod.rs: SMarvelousMod — id `s-marvelous`, name
  "S-Marvelous Judgement (12ms)", required_signatures ["judge_submit"],
  init → data_feed::install (fail ⇒ inert + WARN, is_active reports it),
  enable → window read/clamp + scene callback (arm both sides at 28 entry
  after reset; per-song log + disarm at 28 exit) + song_reset subscription,
  disable → flags/disarm/unregister. ACTIVE gate on all callback bodies.
- [x] lib.rs registration (mods_to_register; NOT in DEFAULT_OFF_MODS ⇒
  default ON)
- [x] Gates: cargo check (msvc) clean, ./scripts/validate_s_marvelous.sh
  10/10, cargo fmt clean, ./build.sh clean (logs/)

## Deviations
- Per-song log emits at GAMEPLAY exit only (not at song_reset) — a reset
  discards the aborted attempt silently, matching the PUS buffer-reset
  precedent. Task text implied the same; noted for completeness.
- Commit step skipped per repo AGENTS.md git rules.

## Step 1 sibling status
- task-01 state-module: Complete
- task-02 data-feed-tap: Complete
- task-03 mod-lifecycle: Complete (this)
→ Step 1 checklist item ticked in the source plan. Cabinet demo (the plan
step's Demo) is the maintainer's validation gate before Step 2 tasks are
generated.
