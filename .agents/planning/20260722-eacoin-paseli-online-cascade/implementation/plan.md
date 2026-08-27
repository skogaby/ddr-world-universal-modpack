# Implementation Plan — EACoin/PASELI online cascade (sub-fix (c))

Feature dir: `.agents/planning/20260722-eacoin-paseli-online-cascade/`
Design: `design/detailed-design.md` · RE record: `rough-idea.md`

No unit-test harness (pack convention). Each step's "validation" is the build gate;
behavioral validation is the live CrossOver deploy in Step 4.

## Steps

- [ ] **Step 1 — libavs-ea3 module resolver.**
  Add `resolve_libavs_ea3_module()` to `src/core/module_resolver.rs` (mirrors
  `resolve_ark_module`; `LIBAVS_EA3_DLL_NAMES = ["libavs-win64-ea3.dll"]`, reuse private
  `resolve_module`). Gate: `cargo check` clean.

- [ ] **Step 2 — sub-fix (c) resolve in `init`.**
  In `src/mods/non_native_os_support.rs`: add the `Ea3GetStatusFn` type,
  `EA3_NETWORK_DOWN`/`EA3_ONLINE` consts, `EA3_GET_STATUS_SIG` AOB, `EA3_STATUS_HOOK`
  static + `EA3_STATUS_ONE_SHOT` latch, and `ea3_status_target` struct field. In `init`,
  resolve the libavs-ea3 module and `scanner::scan_pattern` the signature; `transmute` +
  store the target; log resolved addr or a self-disable warning. Gate: `cargo check`.

- [ ] **Step 3 — install/teardown + status reporting (core payoff).**
  Add `ea3_status_detour` (call original, promote 1→3, one-shot log, panic-guarded).
  In `enable`, `install_hook(addr_of_mut!(EA3_STATUS_HOOK), target, ea3_status_detour,
  "ea3-status")` and count it. Extend `remove_hooks()` to tear down `EA3_STATUS_HOOK` +
  reset the latch. Union `EA3_STATUS_HOOK.is_some()` into `is_active()`. Update the mod's
  module-doc header to describe sub-fix (c). Gate: `cargo check` → `cargo fmt` (whole
  crate) → `./build.sh` clean.

- [ ] **Step 4 — docs + live validation.**
  README (Non-Native OS Support row: add the PASELI/EACoin sub-fix) and AGENTS.md
  (Key Entry Points row for `non_native_os_support` — note sub-fix (c) + this planning
  dir). Deploy via `./scripts/deploy.sh`; confirm the Step-1/2 acceptance in
  `design/detailed-design.md` Testing (PASELI available at entry; one-shot log; then a real
  consume). Record results in `progress.md`.

## Notes

- Steps 1–3 may land in one edit pass (small, self-contained); each increment is distinct
  in the code and verified by the build gate. TDD adapted to build-gates (no unit harness;
  behavioral test = live CrossOver deploy, Step 4).
- Keep sub-fix (a) as-is; do NOT consolidate it into (c) yet (defer until (c) is validated).
- If a live consume fails, pivot the hook to the deeper `XEyy2igh00001b` network-up reader
  (force the out-param) per rough-idea Open Questions — same mod, same idioms.
