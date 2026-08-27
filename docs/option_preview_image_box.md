# Option Preview-Image Box Research

Reverse engineering of the **gray preview-image box** below the option rows in
DDR World's player-options menu — the panel that shows a `seop_image_*` texture
illustrating the currently-focused option's value (e.g.
`seop_image_lanecover_hidden`, `seop_image_stepzone_off`). Goal: enable the
preview box for mod-injected custom-option rows.

**Game binary**: `gamemdx.dll` (MDX-003_20260526)
**Ghidra base**: `0x180000000`
**Runtime base (this session)**: `0x7FFE04CF0000` (so `runtime = ghidra + 0x7FFC84CF0000`)
**Tools**: Ghidra (static) + Cheat Engine (live focus-change breakpoints)
**Status**: RESOLVED — full dispatch chain mapped end to end and validated live,
including the exact override site. The preview name is produced by **slot 0 of the
`IOptionElement` vtable (the 3rd MI base at `row+0xC0`)**, `FUN_18017a170`. Mod
implementation = override that one slot on our synthesized `+0xC0` vtable and
write the `seop_image_*` name directly (no detour, no value-model wiring). No open
RE items.

---

## Summary

The preview box is driven by a **`ReactiveAction<std::string>`-style observer**
embedded in the per-side options driver (the `option_Np_usr` form). On every
focus update the observer:

1. asks the **currently-focused row** for its preview-image name (a per-kind
   getter, dispatched as a reactive lambda bound to the row's value model),
2. dirty-checks it against the last name,
3. if changed, binds that texture onto the `<root>/image_usr` AFP clip.

**Per-value vs. fixed image is decided entirely by the per-kind getter**: enum
kinds index a value→name string table (`seop_image_lanecover_%s`); scalar kinds
(`OptionElement<int>`: scroll speed, hispeed, lane visibility, …) return a single
literal name with no value substitution. See
[`option_row_marker_render.md`](./option_row_marker_render.md) for the sibling
marker investigation; this doc is the preview-box analog.

**Key consequence for mods — RESOLVED to a clean row vtable override.** The
preview-name producer bottoms out at a **virtual method our rows already carry**:
the observer fetches the focused row, `__RTDynamicCast`s it to `IOptionElement`,
and calls **slot 0 of the `IOptionElement` vtable** to build the name. On
`OptionElement<KIND>`, `IOptionElement` is the **third MI base at `row+0xC0`**
(vtable e.g. `0x18037a3c8`), and its slot 0 is `FUN_18017a170` — the getter that
returns the `seop_image_*` string. That function's FIRST check is
`if (row+0x110 == 0) return "";` — the exact value-model slot the native builder
populates and our donor-clone rows leave null (same field that gated the marker
render). **That is why mod rows show no preview: `FUN_18017a170` takes the
empty-string branch.**

So the implementation is the familiar pattern, NOT an observer intercept:
override **slot 0 of our synthesized `row+0xC0` (IOptionElement) vtable** to write
our own `seop_image_<id>[_<suffix>]` into the out-param `std::string`, computed
from the registry. No reactive wiring, no `onCreate`, no `+0x110` reconstruction —
the observer/binder/dirty-check all keep working unchanged because they only call
this one virtual. (The reactive `OptionItem` lambda path described below is how
the NATIVE getter reaches the value — we bypass it by writing the string
directly, the same "replicate, don't wire" approach as the marker.)

---

## Dispatch chain (live-validated)

```
focus change
  └─ FUN_18018ea20(driver)                      [per-side options update handler; fires for OUR rows too]
       └─ (*(driver+0x118))[0](driver+0x118)     [embedded ReactiveAction<string> observer, vtable 0x18035dc80]
            = FUN_180037160(observer)
                ├─ produce name  → FUN_18018e6d0(getterHolder)
                │    ├─ focused = FUN_18004a520(*(…)+0x230)          fetch focused row (Component*)
                │    ├─ io = __RTDynamicCast(focused, Component → IOptionElement)   [IOptionElement RTTI @ 0x1804a0e18]
                │    └─ (*io.vtable[0])(io, &name)                   ★ IOptionElement SLOT 0 = preview-name getter
                │         = FUN_18017a170(row+0xC0, &name)           [io = row+0xC0, the 3rd MI base]
                │             if (row+0x110 == 0) name = ""          ← MOD ROWS HIT THIS (value-model null)
                │             else  name = per-kind builder(value)   via row+0xf8 / row+0x138 → seop_image_*
                │                   • enum:   FUN_18016d500  sprintf("seop_image_lanecover_%s", table[value])
                │                   • scalar: FUN_180185190  strcpy("seop_image_scroll_speed")  [value ignored]
                ├─ FUN_18003b560(observer+0x58, &name)          DIRTY CHECK vs cached last name
                └─ if changed: (*(observer+0x30))[1](observer+0x30, &name)   BIND texture
                     = FUN_18018e760(binder_sink, &name)
                         └─ Ordinal_103(<root>, "%s/image_usr") + Ordinal_112(clip, name)  load bitmap
```

### Live evidence

- **Focus handler fires for mod rows.** Breakpoint on `FUN_18018ea20` hit 1006×
  while scrolling the MODS tab; `driver` (RCX) = the single per-side driver
  instance for every row (stock and mod). So the preview path runs for our rows
  already — it just produces an empty name.
- **Name getter is `IOptionElement` slot 0, at `row+0xC0`.** Breakpoint at the
  post-`__RTDynamicCast` site (`0x18018e710`): RAX (the cast result = the
  `IOptionElement` subobject) = focused-row-base + `0xC0`, and `[RAX]` = the
  row's `+0xC0` vtable. Its slot 0 decompiles to `FUN_18017a170`, whose first
  branch is `if (row+0x110 == 0) return ""` and whose else-branch runs the
  per-kind `seop_image_*` builder against the value model. The cast target RTTI
  descriptor at `0x1804a0e18` = `.?AVIOptionElement@selectmusic@sequence@@`,
  confirming the interface. (Earlier I mis-described the producer as a reactive
  lambda with "no row slot to override"; the deeper trace shows it IS a row
  vtable slot — `IOptionElement[0]` — invoked by the observer. The reactive
  `_Impl_no_alloc1<…OptionItem<KIND>>` callable is how the NATIVE getter reaches
  the value internally, not the entry point we override.)
- **`+0x110` is the discriminator.** `FUN_18017a170` returns `""` when
  `row+0x110 == 0` — the same value-model self-pointer slot the native builder
  populates and our donor-clone rows leave null (cf.
  `option_row_marker_render.md`, where `+0x110` gated the marker block).
- **Binder confirmed.** `FUN_18018e760(sink, &name)` builds `"%s/image_usr"`,
  resolves the clip via `Ordinal_103` (`afp_mc_refer`) and binds the named
  texture via `Ordinal_112`. It receives the already-computed `std::string`
  name as its second argument.

---

## Per-kind getter: value-name table vs. fixed literal

The `seop_image_*` format strings live at `0x180376a48..0x180376cc8`. Two shapes:

**Enum / boolean kinds → per-value** (format string with `_%s`, indexed by value):

| Kind | Format string | Builder |
|---|---|---|
| LaneCover | `seop_image_lanecover_%s` | `FUN_18016d500` (indexes `PTR_DAT_1804b33f0[ value ]`) |
| Gauge | `seop_image_gauge_%s` | — |
| Stepzone | `seop_image_stepzone_%s` | — |
| ArrowDesign | `seop_image_design_%s` | — |
| ArrowColor | `seop_image_color_%s` | — |
| … (≈18 total `_%s` kinds) | | |

**Scalar / numeric kinds → one fixed image** (literal copy, value ignored):

| Kind | Literal name | Builder |
|---|---|---|
| ScrollSpeed | `seop_image_scroll_speed` | `FUN_180185190` (`strcpy`, no `%s`) |
| Hispeed | `seop_image_highspeed` | `FUN_180185390` (`strcpy`) |
| LaneVisibility | `seop_image_lane_visibility` | `FUN_18016d6f0` (`strcpy`) |
| (Display timing) | `seop_image_display` | — |
| (Judge timing) | `seop_image_judge_timing` | — |

So scalar previews are static by design (a continuous range has no value-name
table). Per-value previews on a scalar are achievable for a mod (we control the
name we feed) but are not native behavior.

---

## Implementation approach for mod rows

**Override `IOptionElement` vtable slot 0 on our synthesized rows** — the same
slot-synthesis pattern already used for primary slots 4/6/7 (`rows.rs`), just on
a different MI vtable. No detour, no observer intercept, no focused-row tracking
(the engine already calls this virtual only on the focused row), no value-model
reconstruction.

Mechanics, mirroring the existing vtable work in `rows.rs`:

1. **Find the `IOptionElement` vtable on our rows.** It is the **third MI base at
   `row+0xC0`** (offset confirmed live). Our rows clone the donor's MI vtables, so
   `row+0xC0` currently points at the donor kind's `IOptionElement` vtable. We
   already synthesize the primary vtable (`build_mod_vtable` /
   `build_mod_vtable_scalar`); add a parallel synthesis for the `+0xC0` vtable:
   `alloc_zeroed`, copy the donor's slots + the `[-1]` COL pointer (same RTTI
   discipline as the primary — `__RTDynamicCast` reads `[-1]`), override slot 0,
   and write the new vtable pointer to `row+0xC0`.

2. **Slot-0 override signature.** `unsafe extern "C" fn(this: *mut u8, out: *mut u8)
   -> *mut u8` where `this` = the `IOptionElement` subobject (`= row + 0xC0`;
   recover the row base by subtracting `0xC0`, then map via `ROWS` like
   `render_enum`/`render_scalar` do with their `this`), and `out` = a
   pre-positioned MSVC `std::string` (SSO) the caller owns. Write the
   `seop_image_<…>` text into `out` using the same `string::assign` SSO primitive
   the row code already uses (`prime_sso_string`), and `return out`.

3. **Name to write (value-aware).** From the registry value for `(row, side)`,
   via `RegisteredOption::preview_image_name_for_value(value)`:
   - **Scalar / boolean / keyless enum value → `seop_image_<id>`** (the base
     name, value ignored — native scalar parity).
   - **Enum value carrying a `preview_key` → `seop_image_<id>_<key>`**. The key
     is the optional `EnumValue::preview_key` field; values without one fall
     back to the base. So one enum option can show a different preview per
     value (mirroring Konami's `seop_image_lanecover_hidden`-style naming).

Because we write the string directly, the `row+0x110` empty-string branch is
moot — the override never consults the value model, so the null `+0x110` that
blanks native-getter output on our rows is irrelevant.

**Texture delivery — atlas-clone injection, NOT bare PNGs.** `seop_image_*` names
aren't in the stock texturelist, so a loose PNG in `data_mods/` won't resolve
(the donor-clone-vs-auto-inject trap from `.agents/learnings/learnings.md`).
They ride the SAME lang_eng atlas pass as the row labels (`asset_gen.rs`):
`register_option` calls `register_preview_images(option.preview_image_names())`
— the base name plus every per-value `seop_image_<id>_<key>` — and the single
`flush_label_atlas()` at init clones each off the `seop_image_scroll_speed`
donor's slot into `select_music_option_lang_eng_v3.ifs`. The mod ships
`seop_image_<id>[_<key>].png` at
`data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`; a missing
PNG → blank box, not an error (see the empty-string contract below). The native
binder's `Ordinal_112` then loads the
name. The observer's dirty-check (`observer+0x58` cached name) means the bind
only re-fires when the name changes, so per-value switching is cheap.

**Empty-string-hides-the-box contract (load-bearing).** The binder
(`FUN_18018e760`) sets the `image_usr` layer's VISIBILITY from the name length:
`visible = (name_length != 0)`, via `FUN_18025e760(layer, clip, visible)`, BEFORE
binding the texture. So an **empty name hides the box**; that's how native rows
with no preview show nothing. A non-empty name that fails to bind (no such
texture) does NOT clear the clip — it leaves whatever was last bound, i.e. the
previously-focused row's image. Therefore the getter MUST return `""` for a value
with no shipped/injected preview, not a plausible-but-unresolvable name.
`rows.rs` enforces this: `asset_gen::preview_is_available(name)` is consulted (the
set of preview names whose PNG actually existed at `flush_label_atlas` time); if
the name isn't available the getter writes `""` and the box hides. Returning the
name unconditionally is the bug "focusing a no-preview row keeps the last image
up."

### Scaling preview injection (multi-atlas) + runtime-count enums

Preview images are large (~368x172). The atlas cloner groups all specs sharing a
donor into ONE cloned atlas capped at 4096x4096 (`MAX_ATLAS_SIDE`), which holds
only **~250** preview images. Options like CHARACTER discover hundreds of assets
at runtime, so a single shared preview atlas overflows (the cloner logs "no room
to pack — skipping" and those previews silently get no texture → blank box via
the availability gate above).

`asset_gen::rebuild_lang_eng_atlas` therefore **chunks preview images** into
groups of `PREVIEW_CHUNK` (128), each cloned into its own atlas under a distinct
prefix (`copt_prev_<chunk>_*`). It uses the `generate_cloned_atlases_xml` +
`write_merged_texturelist` aggregation pattern: one fragment per atlas set (base
labels/ribbons + each preview chunk), concatenated, written once. Distinct
prefixes → distinct cache-blob MD5s → no collision (per the learnings-doc
shared-cache rule). Labels and ribbons stay in the single base atlas
(`copt_mods_lang_eng`) — they're small and few (~4700 ribbons fit one atlas).

Per-atlas note: the cloner only GROWS the atlas from the donor's native size
(typically 2048²) up to 4096²; it never shrinks. So even a small chunk allocates
a ≥2048² (≥16 MB) atlas at boot. Boot cost scales with the number of chunks; for
the CHARACTER POC (1–2 chunks) it's negligible, but a dozen indexed options with
hundreds of assets each is many atlases — revisit chunk size / lazy injection if
that materializes.

**Runtime-count categories (`webui_options`) render as scalar rows.** All
`webui_options` categories use the same index-based value model
(`value` = index into `asset_ids`), so persistence/apply are mode-agnostic.
Runtime-count categories (CHARACTER, APPEAL BOARD, BACKGROUND ×2, LANE ×2,
LANE COVER ×2) are `Scalar`: the value renders through the game's native digit
text path as the 1-based position ("1".."N",
`ScalarFormat::OffsetInteger { display_offset: 1 }` — display-only; the stored
value stays the 0-based index), needing **no per-value ribbon or preview
textures**; the preview box shows the single base `seop_image_<id>` chrome with
the live art overlaid by `preview_overlay`. (Historical: these were briefly
`EnumIndexed` rows keyed `item_<NNN>` over a shared 150-chip
`seop_op_item_<NNN>` ribbon set; that mode was retired 2026-08-11 because the
150 injected chips dominated the CAUTION-screen load — see
`docs/scene_load_analysis.md`.)

### Caveats to carry into implementation

- **Verify the slot is inheritable-then-overridable, not dependent on unwired
  state for OTHER slots of the `+0xC0` vtable.** We only override slot 0; the rest
  of that vtable is inherited from the donor. If any other `+0xC0` slot the engine
  calls reads value-model/`onCreate` state our rows lack, it could misbehave (cf.
  the donor-slot hazard in `.agents/learnings/learnings.md`). Slot 0 is the only
  one the preview path calls; audit the others before shipping.
- **Out-param `std::string` ownership.** The caller (`FUN_18018e6d0`) passes a
  caller-owned `std::string` and the native slot 0 fills it (SSO or heap-promoted
  via the game allocator). Use the existing `prime_sso_string` path so any heap
  promotion uses the game's allocator, not Rust's. Most `seop_image_*` names are
  > 15 chars, so they WILL heap-promote — make sure the assign goes through the
  game's `string::assign` (already wired in `rows.rs`).
- **`row+0xC0` is the third MI base** — when computing the row base inside the
  override, subtract exactly `0xC0`; do not assume it equals the primary `this`.

---

## Key Addresses (file-relative to `gamemdx.dll` 20260526)

| Symbol | Address | Role |
|---|---|---|
| **`IOptionElement` slot-0 preview-name getter** | `FUN_18017a170` | **THE override target.** `(this=row+0xC0, &out_string)`; `if (row+0x110==0) ""` else per-kind `seop_image_*` |
| IOptionElement RTTI type descriptor | `0x1804a0e18` | `.?AVIOptionElement@selectmusic@sequence@@` (cast target) |
| name producer (calls slot 0) | `FUN_18018e6d0` | fetches focused row, `__RTDynamicCast`→IOptionElement, calls `vtable[0]`; post-cast site `0x18018e710` |
| focused-element getter | `FUN_18004a520` | returns the focused row (Component*) from the row container |
| per-side options update handler | `FUN_18018ea20` | fires on focus change (mod rows included) |
| preview observer dispatcher | `FUN_180037160` | produce → dirty-check → bind; observer = `driver+0x118`, vtable `0x18035dc80` |
| texture binder | `FUN_18018e760` | `(sink, &std::string name)` → loads onto `%s/image_usr` |
| binder vtable thunk | `FUN_180190030` | wraps the binder; vtable `0x18037d328` |
| enum getter (lanecover) | `FUN_18016d500` | `seop_image_lanecover_%s`, value-indexed |
| scalar getter (scroll speed) | `FUN_180185190` | literal `seop_image_scroll_speed` |
| scalar getter (hispeed) | `FUN_180185390` | literal `seop_image_highspeed` |
| `seop_image_*` format strings | `0x180376a48..0x180376cc8` | 24 entries; `_%s` = per-value, bare = fixed |
| `%s/image_usr` | `0x18037cfe8` | preview clip path |
| sample row primary vtable (Visibility) | `0x18037a368` | IOptionElement (3rd MI) vtable = `0x18037a3c8`, slot 0 = `FUN_18017a170` |

Live measurement that pinned `IOptionElement = row+0xC0`: at the post-cast site,
`RAX (cast result) = focused-row-base + 0xC0`, and `[RAX]` was the row's `+0xC0`
vtable whose slot 0 = `FUN_18017a170`.

### Observer object layout (`driver+0x118`, partial)

| Offset (from observer) | Field | Notes |
|---|---|---|
| `+0x00` | vtable `0x18035dc80` | slot 0 = `FUN_180037160` dispatcher |
| `+0x10` | byte | force/dirty flag (forces re-bind even if name unchanged) |
| `+0x30` | ptr → binder sink | slot 1 = `FUN_18018e760` |
| `+0x38` | getter holder | `+0x18` → callable; slot 1 = per-kind preview-name getter |
| `+0x50` | (within getter holder) | the rebindable per-row getter callable |
| `+0x58` | `std::string` | cached last-emitted name (dirty-check target) |

(Observer offsets are from the embedded sub-object at `driver+0x118`; the driver
itself is the `option_Np_usr` form instance, one per active player side.)

---

## Cross-Version Notes

- Addresses are for `20260526`. The architecture — a `ReactiveAction<string>`
  observer that pulls a per-kind getter on focus change and binds to
  `%s/image_usr` — is structural and matches the reactive option pipeline
  documented in `custom_player_options_research.md`; concrete `FUN_*` will move.
- Stable anchors: the `%s/image_usr` string (`0x18037cfe8`, one referencing
  cluster) for the binder/handler, and the `seop_image_*` format strings for the
  per-kind getters. The observer dispatcher is reachable as slot 0 of the vtable
  stored at `driver+0x118`.

## Gotchas

- **The preview name IS a row vtable slot — `IOptionElement` slot 0 at
  `row+0xC0`** (`FUN_18017a170`). Override that on our synthesized `+0xC0` vtable;
  do NOT detour the observer/binder (an earlier draft of this doc wrongly
  concluded "no row slot" — superseded). The native getter internally pulls from
  a reactive `OptionItem` lambda, but the entry point we override is the plain
  virtual the observer calls.
- **`row+0x110 == 0` ⇒ native getter returns `""`.** That's why mod rows are
  blank today (the donor slot-0 runs and short-circuits on our null value-model
  slot). Our override sidesteps it by writing the string directly — don't try to
  populate `+0x110`.
- **`row+0xC0` ≠ primary `this`.** Inside the slot-0 override, `this` is the
  IOptionElement subobject; subtract `0xC0` to get the row base before `ROWS`
  lookup.
- **Audit the other `+0xC0` slots.** We override only slot 0; the rest are
  inherited from the donor. Confirm none the engine calls depend on unwired
  value-model state (donor-slot hazard, `.agents/learnings/learnings.md`).
- **Out-param string heap-promotes.** `seop_image_*` names exceed SSO (15 ch), so
  fill the out `std::string` via the game's `string::assign` (`prime_sso_string`
  in `rows.rs`), not Rust, to keep allocator hygiene.
- **Scalar = fixed image natively.** For scalar parity return a constant
  `seop_image_<id>`; per-value scalar previews are non-native (compute a suffix if
  ever wanted).
- **Texture must be atlas-injected via LayeredFS**, same as the row/label
  textures; the binder loads by name through the normal BM2D path.
