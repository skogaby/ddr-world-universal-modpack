# Progress — Step 1: pure resolution model + config section

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `plan.rs` types + `parse_dims` / `resolve_render` / `classify_aspect` / `compute(_gated)` / `present_mode` / `scissor_scale`
- [x] 16 host tests (T1–T22 grouped) green via `scripts/validate_custom_resolution.sh`
- [x] `ResolutionConfig` + `ConfigFile.resolution` + both fallback literals
- [x] `mods/mod.rs` registers `custom_resolution`
- [x] `cargo check --target x86_64-pc-windows-msvc` clean

## Cycles
1. Wrote `plan.rs` (tests + implementation in one pass — the design fixed every
   signature and rule, and the harness did not exist yet to observe a red state);
   harness run → 16 passed. Deviation from strict red→green noted below.
2. Config section + module wiring → `cargo check` clean (16 s).

## Deviations
- Tests and implementation for `plan.rs` were authored together rather than red-first
  (module is a direct transcription of design §4.1). All later steps run the harness
  red-first now that it exists.
- `Gates` / `compute_gated` introduced here (plan puts the constants in Step 3) so the
  gating rules are host-tested from the start; `GATES` ships `{false,false}`.
- 4:3 `coerced_render` is `true` whenever the resolved render spec ≠ 1280×720 — including
  the default `"output"` (which resolves to 640×480). The INFO in Step 3 must word this as
  "4:3 output always renders at 1280x720" rather than blaming the operator.

## Validation
- `scripts/validate_custom_resolution.sh`: 16 passed, 0 failed.
- `cargo check --target x86_64-pc-windows-msvc`: clean.
