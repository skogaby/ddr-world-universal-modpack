# Option Row Marker Render Research

Reverse engineering of the **value-selector position marker** rendered at the
bottom of each option row in DDR World's player-options menu — the thin green
bar (`choice_usr/scroll_usr` + `move_usr`) whose width encodes "how many values
this option has" and whose horizontal position encodes "which value is
currently selected". Also covers the `tri_l_usr`/`tri_r_usr` cycling arrows
driven by the same render, and the cause of the unwanted sweep animation on
mod-injected rows.

**Game binary**: `gamemdx.dll` (MDX-003_20260526)
**Ghidra base**: `0x180000000`
**Runtime base (this session)**: `0x7FFE0C4A0000` (so `runtime = ghidra + 0x7FFC8C4A0000`)
**Tools**: Ghidra (static) + Cheat Engine (live validation on the options screen)
**Status**: RESOLVED. Render function, marker geometry, the `+0x110` abort gate,
and the mod-row divergence all confirmed against the live `20260526` binary.

> **Version note**: this supersedes the stale `FUN_*` addresses in
> `custom_player_options_research.md`, which was built against `20260324`. The
> render-slot structure (primary vtable slot 7, shared across all
> `OptionElement<KIND>` specializations) and the marker geometry are stable;
> only the concrete addresses moved.

---

## Summary

Each option row's render (primary vtable **slot 7**) is responsible for the
value marker. Our mod-injected rows override slot 7 with a custom renderer
(position-pinning + texture binding only), so they get **no marker** and their
marker clip **free-runs its intro sweep animation** every time the row becomes
visible (tab open / scroll-in).

The native render can't simply be called on a mod row: it has an early
**abort gate** on `row+0x110`, a field the native *builder* (not the ctor)
populates with a per-instance BM2D-layer-capturing lambda closure that
mod rows deliberately skip. But the marker block itself does **not** read
`+0x110` — it is fully data-driven from the value count and current index,
both of which the mod framework already owns. So the fix is to **replicate the
marker block** in our slot-7 override using `bm2d_api` primitives we already
have, driven from the registry's `index`/`count`.

---

## Render Function (primary vtable slot 7)

| Symbol | Address | Basis |
|---|---|---|
| `option_row_render` (slot 7) | `FUN_1801754c0` (ghidra) / `0x7FFE0C6154C0` (runtime this session) | Sole referencer of the `"choice_usr/scroll_usr/move_usr"` string at `0x180375fe0`; confirmed at slot 7 of a live row's primary vtable |

**Confirmed shared across specializations**: `FUN_1801754c0` is referenced from
~20 `OptionElement<KIND>` primary vtables (the `0x18037xxxx` data xrefs), each at
slot index 7 (`vtable+0x38`). A live SpeedType row sampled on the options screen
had primary vtable ghidra `0x180379728`; its slot 7 = `0x1801754c0`. ✓

### Control-flow skeleton (decompiled)

```c
void option_row_render(longlong this) {              // RCX = OptionElement<KIND>*
    FUN_18017fd40(*(this + 0x1f8));                  // value-list housekeeping
    // reactive-stream pumps (only if +0x118 sub-MC present):
    if (*(this+0x118) && (*vtable_of(*(this+0x270)))() , *(this+0x118))
        && (*vtable_of(*(this+0x208)))() , *(this+0x118))
        (*vtable_of(*(this+0x2d0)))();

    // position-pin the sub-MC at this+0x118 to row pos (this+0x88/+0x90)
    // plus accumulated ancestor offset from the parent chain at this+0x60:
    plVar11 = *(this+0x118);
    if (plVar11) {
        x = *(double*)(this+0x88); y = *(double*)(this+0x90);
        if (*(this+0x60)) { (ax,ay)=FUN_180045510(*(this+0x60)); x+=ax; y+=ay; }
        local = pack_int2(x,y);
        (*vtable_of(plVar11)[6])(plVar11, &local);   // afp layer set position
    }

    // tick label/value TextLayers (scalar value digits):
    if (*(this+0x120)) (*vtable_of(*(this+0x120))[0])();
    if (*(this+0x130)) (*vtable_of(*(this+0x130))[0])();

    // ─── ABORT GATE ───────────────────────────────────────────────
    if (*(this+0x110) == 0) FUN_180278b94();         // does-not-return
    lVar5 = (*vtable_of(*(this+0x110))[1])();         // AFP ctx from +0x110 closure
    if (lVar5 && (p = FUN_18025e8b0(lVar5,"option_1p_usr/dummy_item_usr",...)))
        { ... re-anchor the sub-MC under dummy_item_usr ... }   // uses +0x110 only

    lVar5 = *(this+0x118);
    if (lVar5 == 0) return;

    // ─── MARKER BLOCK (does NOT touch +0x110) ─────────────────────
    optiontab = *(this+0x1f8);
    cur_index = FUN_18017e8a0(optiontab);            // current ENABLED index
    count = 0;                                       // count ENABLED entries
    for (e in optiontab.vector[0x40..0x50])          // stride 0x10
        if (*(char*)e->ptr != 0) count++;

    // bar width fraction:
    if (count > 1) width = max(0.2 /*DAT_18038f358*/, 1.0 /*DAT_18038f1c0*/ / count);
    else           width = 100.0 /*DAT_18038eb20*/;  // (raw, see note)

    // bar position fraction (eased):
    if (count < 2) pos = 0.0;
    else pos = (1.0 - width) * cur_index / (count - 1);
    pos = (pos - this->anim /*+0x140*/) * 0.5 /*DAT_18038eb88*/ + this->anim;
    this->anim = pos;                                // store eased value back
    pos_px = trunc(pos * 100.0 + 0.5);               // FUN_180289f28 = trunc; round-to-nearest

    // push to BM2D:
    mc = afp_mc_refer(*(lVar5+8), "choice_usr/scroll_usr");
    if (valid) afp_mc_op(mc, 0x0f04, pos_px);                       // position
    mc = afp_mc_refer(*(this+0x118 +8), "choice_usr/scroll_usr/move_usr");
    if (valid) afp_mc_op(mc, 0x0f04, trunc(width * 100.0));         // width

    // cycling-arrow visibility:
    for (l in children "choice_usr/tri_l_usr")  { set_param(l,0x1007, cur_index != 0);      set_param(l,0x101e,1); }
    for (r in children "choice_usr/tri_r_usr")  { set_param(r,0x1007, cur_index != count-1); set_param(r,0x101e,1); }
}
```

### Marker geometry constants

| Const | Ghidra addr | Value | Role |
|---|---|---|---|
| total track | `DAT_18038f1c0` | `1.0` | full normalized bar track |
| min bar width | `DAT_18038f358` | `0.2` | floor on width fraction |
| px scale | `DAT_18038eb20` | `100.0` | normalized → `0x0f04` units (×100) |
| easing factor | `DAT_18038eb88` | `0.5` | per-frame lerp toward target position |
| round helper | `FUN_180289f28` | `trunc()` | `trunc(x*100 + 0.5)` = round-to-nearest |

**Resulting widths**: count=2 → `max(0.2, 0.5)` = **0.5** (half-width bar, matches
the ON/OFF stock rows); count=3 → 0.333; count≥5 → clamped at 0.2.

The position eases toward its target by 0.5/frame (`+0x140` is the stored eased
state), which is the slide-to-position animation seen when changing a value — it
is intended and lightweight, distinct from the unwanted intro sweep (see below).

> **Width note**: the `count <= 1` branch loads `DAT_18038eb20` (100.0) as the
> width fraction, which then gets `*100` → an absurd value. In practice every
> real option has ≥2 enabled values so this branch isn't hit; mod rows should
> mirror that by only driving the marker when `count >= 2`.

---

## libafp ordinal → export mapping (used by the marker block)

The render calls libafp by ordinal; these map to the named exports `bm2d_api`
already wraps:

| Ordinal in render | Export | `bm2d_api` wrapper | Used for |
|---|---|---|---|
| `Ordinal_103(mc, name)` | `afp_mc_refer` | `find_child(parent_mc_id, name)` | resolve `scroll_usr` / `move_usr` / `tri_*` child MC ids |
| `Ordinal_22(6, mc)` | (validity check) | — (wrappers return `None` on failure) | guards the bad-MC warning path |
| `Ordinal_114(mc, 0x0f04, v)` | `afp_mc_op` | `mc_op(mc_id, 0x0F04, v)` | set scroll position (and bar width) |
| `Ordinal_116(mc, p, v)` | `afp_mc_set_param` | `mc_set_param(mc_id, p, v)` | `0x1007` visibility, `0x101e` "dirty/apply" |
| `Ordinal_106(mc, 6)` | `afp_mc_traversal` | `mc_traversal(mc_id, 6)` | iterate sibling `tri_*` layers |

`afp_mc_op` op `0x0F04` = "set scroll position in pixels" (already documented in
`bm2d_api.rs`). The marker's "position" and "width" are both expressed as this
op against the `scroll_usr` and `move_usr` sub-clips respectively.

The child MC id is resolved from the sub-MC layer id stored at `row+0x118`,
read at offset `+0x08` (`*(u32*)(sub_mc + 8)`) — the same `mc_id` our existing
`render_enum`/`render_scalar` already extract for `option_usr`/`choice_usr`
binding.

> **CRITICAL — use `layer_find_child`, not `find_child`.** The `mc_id` at
> `*(sub_mc+0x08)` is a **type-1 layer id**, so child paths must be resolved with
> `afp_layer_mc_refer` (`bm2d_api::layer_find_child`), the same call the working
> `option_usr`/`choice_usr` binds use. Resolving with `afp_mc_refer`
> (`bm2d_api::find_child`, the type-4 MC-id namespace) returns **-1** for these
> paths and the marker silently no-ops. Verified live via Cheat Engine: an
> `afp_mc_refer("choice_usr/scroll_usr", ...)` from our render returned RAX=-1,
> while `afp_layer_mc_refer` on the same id resolves. The native render also
> uses the layer-id form (`Ordinal_103` against `*(sub_mc+8)`). The first
> implementation pass used `find_child` and produced exactly the "no marker, no
> error" symptom.

---

## The `+0x110` Abort Gate (why we can't call the original)

`row+0x110` gates the entire back half of the render: `if (*(+0x110)==0)
FUN_180278b94()` — a does-not-return abort. On mod-injected rows `+0x110` is
**null**, so calling the native render would abort immediately.

### What `+0x110` is (live-validated)

On a live native row (`this = 0x140076E0`):
- `*(this+0x110)` = `0x140077D8` = **`this + 0xF8`** (a self-pointer to an
  embedded subobject).
- RTTI of `this+0xF8`:
  `std::tr1::_Impl_no_alloc0<_Callable_obj<sequence::selectmusic::\`anonymous namespace\'::<lambda79>,0>, BM2D::CLayer*>`
  — i.e. a **lambda closure capturing the row's `BM2D::CLayer*`**.

### Why mod rows lack it

The SpeedType ctor `FUN_18016f2d0` (one of the donor ctors the framework calls)
**explicitly writes `param_1[0x22] = 0`** (byte offset `0x110`). So `+0x110` is
zero right after construction on native rows too — it is wired up **later by the
row builder/registration path** to point at the `+0xF8` lambda closure, which
the builder constructs against the row's live BM2D layer.

Our `custom_options` rows are donor-vtable clones that skip the native builder's
per-instance wiring (this is the documented reason slot 6 `onCreate` is a no-op
and slot 7 is overridden — see `rows.rs` and `.agents/learnings/learnings.md`
→ "Inherited donor slots can depend on unwired per-instance state"). So `+0x110`
stays null and the original render is unusable on our rows.

Reconstructing the `+0xF8` closure (a `BM2D::CLayer*`-capturing
`_Impl_no_alloc0` lambda) just to satisfy the abort gate is exactly the
per-instance-state trap that bit prior sessions, and it would *also* pull in the
`dummy_item_usr` re-anchor block we don't want. **Not worth it** — the marker
block we actually want doesn't use `+0x110`.

### What the marker block actually needs

Only three fields, all of which mod rows have or can supply:

| Field | Native source | Mod-row source |
|---|---|---|
| `+0x1F8` value-list | OptionTab (count + current index) | **registry**: `allowed_values.len()` (count) and index of current value |
| `+0x118` sub-MC layer id | set by visibility handler | already read by `render_enum`/`render_scalar` (`*(u32*)(sub_mc+8)`) |
| `+0x140` anim state | eased per frame | a mod-owned `f64` field (can live in `RowSlot`, or reuse `row+0x140` directly since the donor ctor zeroed it) |

Driving the marker from the **registry index/count** (rather than reading the
native OptionTab at `+0x1F8`) is cleaner and avoids depending on a field the
donor ctor may leave in a non-native state.

**The marker applies to BOTH enum and scalar rows.** This is slot 7 shared
across all `OptionElement<KIND>` including `OptionElement<int>` — a live capture
of the stock SCROLL SPEED row (value "490") shows the same `scroll_usr` bar
under the digits, confirmed against the value-list at `+0x1F8`. The marker
encodes the value's position within its discrete enumerated range, so it is
*not* enum-only.

- **Enum rows**: count = `allowed_values.len()`, index = position of the current
  value in `allowed_values`.
- **Scalar rows**: the native value-list enumerates the range at the step
  granularity, so count = `(max - min) / step_fine + 1` and index =
  `round((current - min) / step_fine)`. The min-width clamp (0.2) keeps the bar
  usable even for a wide range (e.g. 1–999): it caps at a 20%-wide bar that
  slides, exactly as the native render does.

---

## The Sweep Animation (root cause)

The unwanted animation — every row's marker sweeping in on tab-open / scroll-in
— is the marker clip (`choice_usr/scroll_usr` and/or `move_usr`) **free-running
its own AFP timeline** because nothing pins it to a static frame.

On native rows, slot 7 writes the marker's position/width via `afp_mc_op(...,
0x0f04, ...)` **every frame**, which holds the clip at the computed static
position. Our override never touches these sub-clips, so the clip plays its
intro tween unchecked each time it's (re)instantiated.

**Expectation**: once our slot-7 override drives `scroll_usr`/`move_usr` with the
computed static position every frame (the marker fix), the sweep should stop —
same root cause, same fix. To be confirmed on deploy; if a residual intro tween
persists, the fallback is an explicit `afp_mc_op`/`mc_set_param` "goto frame +
stop" on the clip (selector TBD), but the per-frame position write is expected
to be sufficient (it is what the native path relies on).

---

## Key Addresses (file-relative to `gamemdx.dll` `20260526`)

| Symbol | Address | Notes |
|---|---|---|
| `option_row_render` (slot 7) | `FUN_1801754c0` | The render; replicate its marker block |
| current-enabled-index getter | `FUN_18017e8a0` | counts enabled entries up to the live cursor |
| value-list housekeeping | `FUN_18017fd40` | called at render top on `+0x1F8` |
| ancestor-offset accumulator | `FUN_180045510` | parent-chain x/y sum (we already mirror this) |
| abort handler | `FUN_180278b94` | does-not-return; fired when `+0x110==0` |
| `trunc()` | `FUN_180289f28` | `trunc(x*100+0.5)` round-to-nearest |
| marker string | `"choice_usr/scroll_usr/move_usr"` @ `0x180375fe0` | unique; anchors the render |
| SpeedType `OptionElement` ctor | `FUN_18016f2d0` | writes `+0x110 = 0` (proof it's builder-populated) |
| sample row primary vtable (SpeedType) | `0x180379728` | slot 7 = `0x1801754c0` ✓ |

### Stable anchor for the render

The most version-robust way to locate `option_row_render` is the marker string
`"choice_usr/scroll_usr/move_usr"` — a unique literal with a single referencing
function (verified: exactly one xref function, two data refs from within it).
Alternatively, derive it as slot 7 (`vtable+0x38`) of any `OptionElement<KIND>`
primary vtable (the framework already RTTI-derives those vtables for the donor
ctor). The mod does **not** need the render's address to *call* it — only the
field offsets (`+0x118`, `+0x140`) and the marker geometry constants, all
recorded above — so no new signature is strictly required.

---

## Struct offsets (OptionElement<KIND>, this binary)

| Offset | Field | Used by marker? |
|---|---|---|
| `+0x00` | primary vtable (slot 7 = render) | — |
| `+0x60` | parent component (ancestor x/y chain) | position pin |
| `+0x88` / `+0x90` | row position x/y (double) | position pin |
| `+0xF8` | embedded `_Impl_no_alloc0<lambda79, BM2D::CLayer*>` closure | (target of +0x110) |
| `+0x110` | self-ptr → `+0xF8` (abort gate); **null on mod rows** | gate only — NOT the marker block |
| `+0x118` | sub-MC layer id container (`mc_id` at `+0x08`) | YES |
| `+0x140` | eased marker anim state (double) | YES |
| `+0x1F8` | OptionTab value-list (count + current index) | native marker source |

---

## Cross-Version Notes

- Addresses above are for `20260526` and differ from
  `custom_player_options_research.md` (`20260324`). The **structure** is stable:
  render at primary slot 7, shared across all `OptionElement<KIND>`; marker
  geometry constants (`1.0`/`0.2`/`100.0`/`0.5`) and the `0x0f04` op are
  format-level, not version-specific.
- The mod implementation should not hardcode the render address; it replicates
  the marker block from registry state. The only binary-derived inputs are the
  field offsets (`+0x118`, `+0x140`) — already used by the existing render
  override — and the geometry constants, which are documented here.
- Verify on the second supported build before shipping: confirm slot 7 still
  references the marker string and that `+0x118`/`+0x140` offsets are unchanged.

---

## Gotchas

- **Don't try to call the native render on mod rows** — the `+0x110==0` abort is
  unconditional and reconstructing the `+0xF8` `BM2D::CLayer*` lambda closure is
  the per-instance-state trap. Replicate the small marker block instead.
- **Marker applies to enum AND scalar rows.** Both go through this shared slot-7
  render; scalar rows show the bar in addition to their numeric TextLayer (the
  stock SCROLL SPEED row is the proof). Drive it for both — derive (index,
  count) from `allowed_values` for enum and from `{min, max, step_fine}` for
  scalar.
- **Only drive when `count >= 2`.** The native `count<=1` branch loads a
  nonsense width (100.0); real options always have ≥2 values. Guard accordingly.
- **`0x101e` after `0x1007`.** The native path always pairs the visibility write
  (`0x1007`) with a `0x101e` write (apply/dirty) — mirror both, as our existing
  `mc_set_param` binding code already does for `option_usr`.
- **Round-to-nearest, not truncate.** Position/width px are `trunc(frac*100 +
  0.5)`. Plain truncation will be off-by-one at boundaries.
