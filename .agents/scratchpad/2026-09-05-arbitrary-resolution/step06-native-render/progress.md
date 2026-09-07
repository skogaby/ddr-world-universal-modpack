# Progress — Step 6: native render path (Tier B)

Status: Complete — cabinet checkpoint #3 PASSED 2026-09-07 (uncommitted; maintainer commits manually)

## Checklist
- [x] `patches.rs::apply_render_set(sigs, plan)` — group 3 (hoisted `R15D`/`ESI` + the 3/1/2 RT-struct dim
      stores via `sites::rt_dim_sites`, `is_complete()` required), group 4 (`sites::viewport_pairs`, 5 wide + 1
      square required; square pair → `(render.w, render.w)`), group 5 (`sites::letterbox_sites` x1/y1 → render).
      Empty set when the render is stock; every site stock-verified; `apply_all` rolls back on any miss.
      22 imm32 writes at 1080p/4K (2 hoist + 6 RT + 12 viewport + 2 letterbox — minus any site whose stock value
      already equals the target).
- [x] `scissor.rs` — `GenericDetour<unsafe extern "C" fn(*mut *mut u8, *mut u8)>` on `scissor_handler`;
      body: `ctx = *walker` (+0x00 offset {ox/rt_w, oy/rt_h}, +0x10 scale {1/cw, 1/ch}), `gd = walker[1]`
      (+0x144/+0x146 u16 viewport w/h), record +4 enable / +6..+0xE rect; `plan::scissor_scale` → write rect,
      call original, RESTORE the 8 bytes. `catch_unwind` around the rewrite; passthrough on disabled record /
      zero viewport / non-finite or non-positive scale (one WARN) / identity result. First 3 rescales logged.
- [x] `mod.rs` — RENDER set + scissor install right after the OUTPUT set (before any detour), all-or-nothing:
      RENDER-set miss ⇒ OUTPUT rolled back; scissor install miss ⇒ RENDER + OUTPUT rolled back; present/letterbox
      failures now roll back both sets. `render_set` kept on the mod.
- [x] `plan::GATES.native_render = true`; T17 asserts `GATES == ALL_ON`.
- [x] Gates: `cargo check` · `cargo fmt` · harness 24/24 · `validate_signatures.sh` ALL GREEN · `./build.sh` clean.
- [x] Offline site check (python over the four `~/Desktop/ddr_modules` builds): hoist R15D/ESI at +9/+0x2C, RT
      stores wide @0xd05/0xe55/0xf0d, square @0xc85, height-only @0xbdd/0xdbf (PRESENT first), 5+1 viewport
      pairs, letterbox y1 @0xdd — identical on 20250805/20260224/20260721/20260825.
- [x] Ghidra (20260825): scissor handler `FUN_1802692e0` decompile matches the record layout; tag-0x07
      `FUN_180268ea0` reads `[RCX+0x146]/[RCX+0x144]` with `RCX = walker[1]` and stores `[ctx+0x00]`/`[ctx+0x10]`.
- [x] CABINET CHECKPOINT #3 — 1080p + 4K native, visually clean (feature `progress.md` has the log analysis)
- [x] Follow-ups from the log: `present_depth_release/addref` derivation accepts the PATCHED PRESENT height
      (`C7 40 16 ?? ?? 00 00`); `enable()`-time `boot state` summary (spice2x debughook attach race lost every
      early_apply line in `log_4k.txt`); `letterbox::installed()` / `logical_screen::installed()` accessors.

## Deviations
- `apply_render_set` takes `(sigs, plan)` — no anchor is needed (all three groups hang off linear AOB hits).
- Failure policy: any RENDER-set / scissor failure rolls back EVERYTHING (stock boot) rather than degrading to
  Tier A — the plan's present policy was computed for render == output and a Tier-A fallback would need
  ForceLetterbox re-planned mid-flight. Documented in the `mod.rs` comment.
- Hot path: the scissor body does null checks only (no `VirtualQuery` per record) — every pointer it reads is
  one the AOB-pinned prologue dereferences unconditionally right after.
