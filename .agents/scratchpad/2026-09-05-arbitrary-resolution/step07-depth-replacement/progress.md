# Progress — Step 7: PRESENT depth replacement (render < output)

Status: Complete — cabinet checkpoint #4 PASSED 2026-09-07 (uncommitted; maintainer commits manually)

## Checklist
- [x] `present.rs::replace_depth(rt10, out)` — runs inside `fixup()` when `PresentDepth::CreateOutputSized`:
      `new = surface_create(out.w, out.h, 0x4b, msaa 0, &{u32 0, u8 0})` (the ctor's exact 5-arg shape, 20260825
      `FUN_180250950(u16,u16,fmt,msaa,opts*)`), then the engine's refcount idiom byte-for-byte
      (`if old { slot=0; release(old) } slot=new; addref(new)` — ctor @ 1801f1df3..1e17 binding `render_depth`).
      Missing derivation or create→0 ⇒ depth NULLED + WARN (PRESENT is a textured quad, Z off).
- [x] Anchors threaded through `install`: `surface_create` / `present_depth_release` / `present_depth_addref` atomics.
- [x] AFP projection redirect half of the original Step 7 = already delivered by Step 4's `logical_screen` (the 4 AFP
      loads read the render block) — plan text updated, no `afp_projection.rs`.
- [x] Gates: `cargo check` · `cargo fmt` · harness 24/24 · `./build.sh` clean (no signature change since the sweep;
      the loosened `present_depth_*` derivation was verified offline on all four builds in Step 6's follow-up).
- [x] CABINET CHECKPOINT #4 — runs G/H/I clean (feature `progress.md`)

## RE facts confirmed this step (20260825, surface ctor `FUN_1801f10e0`)
- Surface ids: `+0xdc` OFFSCREEN1 colour (0x500², fmt 0x15), `+0xb0/+0xb8/+0xc8` depths (fmt 0x4b), `+0xb4/+0xc0/+0xd0`
  colours (fmt 0x16), `+0xc4/+0xcc` RENDER msaa pair, `+0xd4` `D24R`, `+0xec` DISPLAY colour (screen w/h, fmt 0x16).
- PRESENT rt (`[surfaces+0x80]`): colour = `+0xc0` render_color (or `+0xec` when AA config == 3 — our plan forces 0),
  depth = `+0xc8` render_depth. `release` = `FUN_180250be0(id)`, `addref` = `FUN_180250b30(id)`, `bind_colour` =
  `FUN_180252c20(rt, id)`.
