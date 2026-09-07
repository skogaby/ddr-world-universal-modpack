# Progress — Step 3: mod skeleton, OUTPUT set, graphics_init detour + PRESENT fixup

Status: Implemented — awaiting the first cabinet checkpoint (uncommitted; maintainer commits manually)

## Checklist
- [x] `src/core/signatures.rs`: `CustomResolutionAnchors` + `SignatureStore::custom_resolution_anchors()`
      (usable at `early_apply`, before `resolve_derived`); `derive_custom_resolution` now publishes those anchors
- [x] `custom_resolution/patches.rs` — OUTPUT set (4 back-buffer imms → output; AA imm 3→0), `apply_checked_patch`,
      atomic with rollback
- [x] `custom_resolution/present.rs` — `graphics_init` detour: pre = display-struct diagnostic, post = PRESENT rt
      `u16 w/h` render→output (layout-gated), screen-global R14 WARN; depth left stock (Step 7 replaces it)
- [x] `custom_resolution/display_modes.rs` — `EnumDisplaySettingsW` fail-safe (fits-in-desktop OR exact mode, Hz
      when FPS Unlock ≠ 60)
- [x] `custom_resolution/mod.rs` — `CustomResolutionMod` (`early_apply` flow per design §4.2; `is_active` truthful)
- [x] `src/lib.rs` registration (after `FpsUnlockMod`, before `resolve_derived`)
- [x] `mod-config.json`: `mods["custom-resolution"] = false` + `resolution` section (stock defaults)
- [x] Gates: `cargo fmt` · harness 23/23 · `validate_signatures.sh` ALL GREEN · `./build.sh` clean (1m09s)
- [ ] CABINET CHECKPOINT (maintainer): SD 640×480 in the CrossOver window — see feature `progress.md`

## Cycles
1. Anchors refactor (derivation → accessor) so `early_apply` can consume them: `cargo check` clean.
2. patches/present/display_modes/mod written against the design; `cargo check` clean first try; fmt; build.

## Deviations
- D16 refinement carried through: no HD-flag byte patch; both selector branches receive the output dims.
- `present.rs` logs (rather than implements) the `CreateOutputSized` depth policy — unreachable while
  `GATES` refuses 16:9 render ≠ output; Step 7 fills it in.
- `mod.rs::init/enable/disable` are logging-only until Step 4 adds the overlay rows.

## Review notes
- `output_set` is retained on the mod struct only for the in-`early_apply` rollback path; never read afterwards
  (crate-wide `dead_code` allow).
- `display_modes::validate` is intentionally permissive under `-w`: any size ≤ desktop passes.
