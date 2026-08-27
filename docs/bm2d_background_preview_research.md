# BM2D Data Manager & On-Demand Background Packages — RE Notes

**Date:** 2026-07-09
**Builds:** `gamemdx_20260526.dll` (primary; image base `0x180000000`) and
`gamemdx_20260324.dll` (cross-check). All addresses file-relative to the image
base. Cabinet-validated on `MDX:J:F:A:2026052600` (DLL probes + a Cheat Engine
live session + the shipped `bg_preview_overlay`).

Feature context: animated background previews for the WebUI Options BACKGROUND
rows (`src/mods/webui_options/bg_preview_overlay.rs`, `services/bm2d_package.rs`,
the AFP-layer wrapper set in `services/bm2d_api.rs`). Full development history:
`.agents/planning/20260708-background-preview-overlay/`.

## The on-demand loader: `bm2d::data::(anonymous namespace)::Manager`

The game loads BM2D packages (IFS-inside-ARC, e.g.
`data/arc/custom/background/background_0001.arc`) through a name-keyed manager
in gamemdx — NOT through `FileManager::Load`:

| Role | 20260526 | 20260324 | Signature |
|---|---|---|---|
| request_load | `FUN_1801abbb0` | `0x1801aa3a0` | `bool f(const char* dir /*RCX, "custom/background"*/, const char* name /*RDX, "background_0001"*/, u32 flag /*R8D, game passes 0*/)` |
| is_ready | `FUN_1801ab7f0` | `0x1801a9fe0` | `bool f(const char* name)` — true once entry exists AND package created |
| release | `FUN_1801abe40` | `0x1801aa630` | `void f(const char* name)` — erases the entry **synchronously**, defers the destroy (below). Only dynamic entries (index ≥ 72) are erased; the 72 permanent common entries are protected |
| registry global | `DAT_1806f1d68` | `DAT_1806ebce8` | ptr → heap obj: `[0]`=begin, `[8]`=end, `[0x10]`=cap, `[0x24]`=dirty byte. Entry stride 0x40: +0x00 MSVC `std::string` name, +0x28 arc idx, +0x2c **FNV hash of `<name>.ifs`** (see below), +0x30 package ptr, +0x38 lang flag |
| lookup | `FUN_1801ac5e0` | `FUN_1801aadd0` | `Entry* f(Entry* begin, Entry* end, const char* name)` — returns `end` if missing |
| Manager::Update | `FUN_1801ab910` | — | pumped EVERY frame by the engine main update (`FUN_180002fe0`, gated on registry != 0); creates packages once ALL queued arcs finish loading |

AOB signatures for the three functions (verified unique on both builds) are in
`src/core/signatures.rs` (`bm2d_data_request_load` / `bm2d_data_is_ready` /
`bm2d_data_release`); the registry global and lookup fn are derived from the
`is_ready` anchor (RIP disp32 at +9, `CALL rel32` at +26). The entry's
package-pointer offset is the disp8 of `is_ready`'s final instruction
(`MOV RCX,[RAX+disp8]`, pattern position +39, wildcarded) — read from the
matched bytes at init so a layout change can't silently desync.

### Arc name variant resolution + the inner-IFS FNV hash

`request_load` → `FUN_1801abfd0` → `FUN_1801ab3e0` tries arc-name variants in
order `%s_v3`, `%s_v0`, `%s_lite` (machine-type-gated via `(*DAT_1806f1330)()`),
`%s` — composing `data/arc/<dir>/<variant>.arc` and stat-ing it. The stat is
**`avs_fs_lstat`** (import `Ordinal_100` on 20260526 = `XCnbrep7000063` on
20260324) and the subsequent open goes through `avs_fs_open` — both LayeredFS-
hooked, which is what makes serving generated arcs from `data_mods/` work.
This also explains `background_0009_lite.arc`: always request the base name;
the manager resolves the variant.

**Critical:** after resolving the variant, `FUN_1801ab3e0` formats
`"<resolved_arc_name>.ifs"` and FNV-1a-hashes it (basis `0x811c9dc5`, prime
`0x1000193`), storing the hash at entry+0x2c. `Manager::Update` later locates
the arc's **inner IFS data object by that hash** (`FUN_180202a30`, map at
`DAT_1806f1f60+0xf8`) and writes `*(obj+0x308) = 1` **with no null check** —
so an arc whose inner IFS basename doesn't match the arc name makes the lookup
miss and Update null-derefs (~AV at 0x308). Any generated/copied arc served
under a new name MUST have its inner IFS renamed to `<new_arc_name>.ifs`
(`core/arc.rs::rewrite_paths` does this without touching the compressed
payload). The afpu package id used for layer creation lives at pkg+0x314 (u32).

### Deferred destroy — the crash class

`release(name)` erases the registry entry synchronously (`is_ready` → false
immediately) but the actual `afpu_destroy_package_data` runs LATER, from the
engine's update in `gameMain`. Consequences (all cabinet-observed as crashes,
2026-07-09):

- A layer must be destroyed BEFORE its package is released — the engine asserts
  `F:afpu-package: destroy stream[..] is used at layer[..]` and crashes if the
  deferred destroy finds a live layer on the stream.
- Never re-request a name whose deferred destroy is pending and bind a new
  layer to the resulting package.
- **The registry is shared by name with the game.** The game's own backdrop
  manager releases the previously-applied `background_%04d` whenever the
  applied customize value changes — so previewing packages under the game's
  names is structurally unsafe (the game destroys them under your layer at its
  own cadence). The shipped fix: load previews under private alias names
  (`bgprev_background_%04d`) served via LayeredFS — entries the game never
  composes. See `bg_preview_overlay`'s module doc.

## Creating a layer from a package (validated recipe)

From `FUN_18003e060` (create-background-layer) / `FUN_18025e480` (CreateLayer),
identical on 20260324 (`FUN_18003e760`/`FUN_18021b6a0`, imports named there):

```
entry    = lookup(registry.begin, registry.end, name); entry == end → not loaded
pkg      = entry[+0x30]                                // NULL until Update creates it
pkg_id   = *(u32*)(pkg + 0x314)
afpu_get_afp_info_at_package(&desc, pkg_id, "bg_root") // libafputils-win64.dll export!
    // desc: 0x28-byte out struct; +0x10 = name ptr, +0x18 = stream id; ret 0 = ok
layer_id = afp_layer_create_with_property(desc.stream_id, desc.name_ptr, 0, 0)  // libafp
afp_id_is_valid(5 /*AFP_LAYER*/, layer_id) >= 0        // else no layer exists
```

Post-create setup (all libafp named exports; defaults: group 0, prio 0,
identity transform = full-screen 1280×720, NOT playing):

- `afp_layer_set_attribute(id, 0x200, 0x200)` — **3-arg** `(id, mask, value)`.
  0x200 is the standard display setup the game applies to every layer it
  creates (NOT a one-shot/stop-at-end flag — looping clips keep it). Bit 1 =
  visibility.
- `afp_layer_set_priority(id, u16)` / `afp_layer_set_group(id, u16)` — display
  sorts ascending within a group: HIGHER priority = drawn later = on top.
- `afp_layer_set_matrix(id, &[f32;6]{a,b,c,d,tx,ty})` +
  `afp_layer_set_position(id, &{f32 x, f32 y})` — they COMPOSE (top-left
  anchor): scale via the matrix, place via position.
- `afp_layer_set_mask(id, x, y, w, h)` — screen-space hard crop.
- `afp_layer_play(id, rate_f32)` — rate is a FLOAT (XMM1): 1.0 = play (looping
  is the engine default), 0.0 = paused static frame. Play clears the
  fresh-layer play-gate (flags bit 3 at layer+0x8).
- Teardown: `afp_layer_do_destroy(5, id, 0)`.

## Render walk & z-order facts (CE session, 2026-07-09)

- The ENGINE renders/advances layers per group: `afp_do_render(delta, 2, group)`
  per group 0–5 in job order 0,4,5,1,2,3 every frame (per-group render jobs
  created at boot in `FUN_18002aa50`); `afp_do_display(2, group)` collects
  layers with attr bit0 set + flags bit4 clear, sorts ascending by priority
  (u16 at layer+0xC), draws.
- **NEVER call `afp_do_render` from hook context** — it asserts instantly in
  `afp_advance_play_data` (`afp-sys.c:4209`) and crashes. The engine renders
  mod-owned layers for free.
- The options modal lives in group 4 (root prio 99, row clips prio 100).
  Group 4 / priority 300 draws OVER the modal, un-darkened. Group 0 =
  song-select bg/characters. Non-BM2D rendering (BmpFont text, sprites) draws
  after all BM2D groups.
- Live layer struct (libafp, stride 0x280; array ptr at `libafp+0x244fd0`,
  count i16 at `+0x244fe0`): +0x00 attr, +0x04 refcount, +0x08 flags (bit3
  play-gate, bit4 no-display, 0x80000 first-frame latch, 0x2000
  pending-destroy), +0x0C prio i16, +0x0E group i16, +0x1C layer id u32, +0x2C
  play rate f32, +0x100 matrix44, +0x1A0 movie-work ptr. (Production code uses
  the exported setters, never these offsets.)

## libafp / libafputils resolution

Both DLLs export by NAME (gamemdx imports by ordinal, but names are present) —
resolve via `GetProcAddress`, never ordinals. Notable:
`afpu_get_afp_info_at_package` is a **libafputils-win64.dll** export (an older
note wrongly attributed it to libafp).
