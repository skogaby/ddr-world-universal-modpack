# Progress — Step 2: signatures, derivations, site finders

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `src/mods/custom_resolution/sites.rs` (pure imm-site finders) + 7 fixture tests
- [x] 5 AOBs in `src/core/signatures.rs` (`display_backbuffer_dims`, `render_surface_hoist`,
      `list_viewport_table`, `letterbox_rect_fn`, `scissor_handler`)
- [x] `derive_custom_resolution` → `aa_config_imm`, `graphics_init`, `render_surfaces_global`,
      `screen_w_global`, `screen_h_global`, `surface_create`, `present_depth_release`,
      `present_depth_addref`
- [x] harness: 23 passed (plan 16 + sites 7)
- [x] `cargo check` clean
- [x] `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN — all 13 names on
      20250805/20260224/20260721/20260825; `shape_diff.py` identical through 0x140 (all five)
      and 0x1200 (`render_surface_hoist`, `list_viewport_table`)

## Cycles
1. `sites.rs` tests red (2 fixture/logic failures caught: the AA test mutated the wrong byte;
   the letterbox scan window 0xE0 was smaller than the real `[RBX+0x298]` store at fn+0xDD+10 —
   window raised to `LETTERBOX_SCAN_LEN = 0x100`) → green.
2. Signatures + derivation compiled first try; sweep flagged `letterbox_rect_fn` as
   never-resolving — my prologue transcribed `MOV R11D,EDX`/`MOV R10D,EAX` as `41 8B D2`/`41 8B D0`
   instead of `44 8B DA`/`44 8B D0` (read from the live bytes) → fixed → ALL GREEN.

## Deviations
- `list_viewport_table` AOB re-anchored on the preceding `MOVAPS XMM13,[rip]` so it hits
  exactly once (the design's bare store-pair pattern hit twice).
- `aa_config_imm` is published as a plain in-module address (not a `publish_value`
  pseudo-address) since it IS an address.
- `letterbox_rect_fn` prologue AOB corrected (see cycle 2); documented offsets unchanged.

## Resolved addresses (RVA) — 20260825 / 20260721 / 20260224 / 20250805
graphics_init 1f2c30/1f2420/1de900/1da730 · letterbox_rect_fn 1f5010/1f4820/1e0ba0/1dcb70 ·
scissor_handler 2692e0/260ef0/24cb60/220050 · render_surfaces_global 6f2ef0/6f2ee8/6c9a98/6b5c78 ·
screen_w_global 6f1520/6f1520/6c80d0/6b42d0 · surface_create 250950/226c70/23bab0/20ee90 ·
present_depth_release 250be0/226f00/23bd40/20f120 · present_depth_addref 250b30/226e50/23bc90/20f070
