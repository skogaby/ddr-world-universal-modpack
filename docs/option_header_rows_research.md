# Option Group-Header Rows — Non-Selectable Info Rows in the Options Menu

RE record (2026-08-13) for rendering **non-selectable, display-only rows** in
the player options menu — the mechanism behind the native gray
"scroll speed MIN~CORE~MAX" row that sits below the SCROLL SPEED / MULTIPLIER
rows and is skipped by cursor navigation. Motivation: mod-authored **group
header rows** on the MODS tab (e.g. a TRAINING group heading above assist
tick / song speed / training options) without abusing parent-child rows.

**Game binary**: `gamemdx.dll` (MDX-003_**20260324** — same build as
`docs/options_scroll_research.md`; structure is build-stable, see
Cross-Version Notes)
**Ghidra base**: `0x180000000`
**Prerequisites**: `docs/options_scroll_research.md` (container/row layout),
`docs/option_row_marker_render.md` (slot-7 render, mod-row render ownership),
`src/services/custom_options/rows.rs` (the donor-clone row factory).

---

## Summary

The gray row is a **distinct native row class, `OptionHispeed`** (0x288
bytes, ctor `FUN_180160ce0`), registered into the **same flat row vector**
as every ordinary `OptionElement<KIND>` row (0x330 bytes) via the same
register helper the modpack already calls (`FUN_180168c70`,
`option_tab_register`). It renders and occupies a normal layout slot; the
cursor skips it because of ONE thing: its **selectability interface** (the
MI base at `row+0x28`) has **slot-0 hardcoded to `return 0`**.

Two fully independent bits govern a row:

| Bit | Where | Meaning | Gray row |
|---|---|---|---|
| `row+0xB8` active byte | main object | laid out + rendered (position packed by the layout engine, occupies a slot) | `1` |
| `+0x28` interface slot-0 | secondary vtable | **focusable/selectable** — tested by every cursor path | returns `0` |

A mod **header row** is therefore: a normal framework row whose `+0x28`
vtable is swapped for a mod-owned 2-slot table `{return 0, no-op}`, with a
label-only render. No other engine cooperation needed.

---

## 1. The `+0x28` selectability interface

Every options row embeds a 2-slot MI interface at `+0x28` (the third qword
is the next base's RTTI COL pointer — the table ends at 2 slots on both
classes):

| Slot | Signature | Ordinary row (`OptionElement<ArrowColor>` donor, vtable `0x180377300`) | Gray row (`OptionHispeed`, vtable `0x1803740d0`) |
|---|---|---|---|
| 0 | `bool isSelectable(this)` | `FUN_1801f3490` — **hardcoded `return 1`** | `FUN_1801ab270` — **hardcoded `return 0`** |
| 1 | `void onFocusChanged(this, bool focused)` | `FUN_18016fe10` — recolors the label TextLayer (`subobj+0x108` RGBA block), sets the row clip's frame label `loop_select`/`loop_off` (the green focus highlight) on the row MC and `choice_usr`, toggles `invalid_usr` visibility, tail-calls an adjacent base | `FUN_180153750` — **empty stub** |

`this` for both slots is the **subobject** (`row + 0x28`); the donor's
slot-1 reaches row fields via subobject-relative offsets (`+0xF0` = row
`+0x118` sub-MC, `+0x108` = row `+0x130` value TextLayer).

It is a pure **vtable difference** — there is no per-instance "focusable"
field to flip.

## 2. Every cursor path tests slot-0 (verified by decompilation)

All three navigation paths use the identical two-part predicate —
`(*(vt@+0x28)[0])(row+0x28) != 0 && *(row+0xB8) != 0`:

- `FUN_180049a40` / `FUN_180049b60` — first/last selectable index (forward /
  reverse walk). Already documented in `docs/options_scroll_research.md`;
  the slot-0 call is the previously-unexplained "secondary vtable slot 0
  returned non-zero" condition.
- `FUN_18004a3c0(container, dir)` — the directional step scan used by the
  step-focus entrypoints (`FUN_1800495a0` etc.): advances `focus_index`
  (`container+0x168`) by `dir` per iteration, **silently skipping** rows
  failing the predicate, with clamp/wrap handling (`container+0x12C`).
- Initial focus placement on tab open runs through the same scan, so a
  header row **as the first row of a tab** is skipped correctly — focus
  lands on the first selectable row.

The layout engine (`FUN_18004a720`) does NOT consult slot-0 — it packs every
`+0xB8=1` row. That is exactly why the gray row occupies a slot and renders
while being invisible to the cursor.

## 3. How the native gray row is built (for reference)

In the 21-row OptionForm builder (`FUN_180163970`, resolved by the framework
as `row_builder_fn`), immediately after the "Hispeed" (MULTIPLIER) row:

1. `operator new(0x288)` + `FUN_180160ce0` (OptionHispeed ctor) — writes the
   three vtables (primary `0x180374088`, `+0x28` `0x1803740d0`, `+0xC0`
   `0x1803740e8`) and the same `"option_item"` layout-metrics key (`0x19`)
   ordinary rows use (⇒ standard row-slot height).
2. Registered via the same `FUN_180168c70` push into `container+0x68`.
3. Tab tags `"Page1"` + name `"Speed"`; wired with two `ReactiveAction`
   lambdas (a `double` — the live speed value feeding the MIN~CORE~MAX
   readout — and a `bool`) plus the player side at `+0x140`.
4. No `OptionItem` value list is populated — it is display-only; its own
   render draws the readout from the reactive value.

A mod header row needs **none** of the reactive wiring — only the class
shape (non-selectable + display-only render).

## 4. Implementation strategy for mod header rows

The framework (`src/services/custom_options/rows.rs`) already synthesizes
per-row vtables for the primary (`+0x00`, 8 slots, slot 4/6/7 overridden)
and IOptionElement (`+0xC0`, 8 slots, slot 0 overridden) tables. Header rows
extend the same pattern to `+0x28`:

1. **Allocate + donor-ctor** exactly as today (the ArrowColor donor clone).
2. **Swap `row+0x28`** to a mod-owned 2-slot vtable:
   - slot 0 → mod stub `return 0` (non-selectable);
   - slot 1 → mod no-op (never called once unfocusable, but stubbed so no
     donor state-dependency can ever fire).
   Requires **no new signatures and no donor derivation** — both slots are
   mod code; the table is `memory::alloc_zeroed` + leak, like the existing
   synthesized tables.
3. **Label-only slot-7 render**: the framework's render override already
   owns everything drawn per row; a `RowKind::Header` draws the group-label
   texture and skips value box / marker / tri arrows entirely (they simply
   aren't drawn — same as the current no-marker state of mod rows, minus
   the value texture).
4. **No value list**: mod rows never use the native `+0x1F8` OptionTab list
   anyway (registry-driven); a header registers with no values and no
   persistence (`PersistMode` n/a — it holds no state).
5. **Preview box**: the `+0xC0` slot-0 override returns an empty (or
   header-specific) `seop_image_*` name.
6. **Paging/scroll**: headers participate as ordinary rows (`+0xB8` via
   `filter_hook` + the scroll driver's window mask) — a header scrolls with
   its group. Ordering via the existing registration order / `row_order`
   config.

Edge behaviors, all engine-native and already correct:
- Header first-in-tab: initial focus skips it (§2).
- Header last visible in the scroll window: cursor scan skips it into the
  next window (scroll driver treats it as any hidden/unselectable row —
  same as today's `+0xB8=0` handling).
- The `+0x110` abort gate (`docs/option_row_marker_render.md`) is
  irrelevant: header rows never call the native render.

## 5. Row dimensions — per-row height IS engine-controllable

The grid layout engine (`FUN_18004a720`) advances its packing accumulator
per visible row as:

```
accum[orient] += row(+0xA0/+0xA8)[orient] + container_spacing(+0xD0/+0xD8)[orient]
```

(`FUN_18004c170` is an orientation-component selector; the options menu is
vertical ⇒ the advance reads **`row+0xA8`**, the per-row y-extent.) The
extent is loaded from **each row object individually** — it is the value
the ctor fetched via the layout-metrics lookup `FUN_180046010(0x19,
"option_item")` into `+0xA0/+0xA8`, not a container constant.

Consequences for header rows:

- **Half-height / slimmer header**: after the donor ctor, overwrite
  `row+0xA8` with the desired slot height (e.g. half the stock metrics
  value read back from the field itself — no new lookups needed). The
  engine packs subsequent rows correspondingly closer. Per-row, no effect
  on other rows.
- **Full-width header**: the x-extent (`+0xA0`) does not drive packing in
  the vertical orientation — cross-axis position is not advanced — so the
  header's visual width is entirely the mod render's art choice (the
  native gray row only *looks* ~70 % wide because of its own clip art).
- The scroll driver's window math is row-count based (`+0xB8` masking),
  not pixel based, so a short header consumes a window slot like any row —
  acceptable; revisit only if slim headers make the 7-row window visually
  uneven.

## 6. Cross-version notes

- Addresses in this doc are **20260324**. The structure — 2-slot interface
  at `+0x28`, hardcoded `return 0/1` slot-0 bodies, the two-part cursor
  predicate, `OptionHispeed` at 0x288 — is class layout, stable across the
  supported builds (same stability class as the `+0xB8`/`+0x168` offsets
  already relied on; `docs/options_scroll_research.md` Cross-Version Notes).
- The implementation needs **zero new AOB signatures**: the swapped `+0x28`
  table is entirely mod-owned, and row allocation rides the already-resolved
  donor ctor / register helper.
- If a future build ever adds a third slot to the `+0x28` interface, the
  no-op stubbing policy fails loud in testing (cursor behavior), not silent
  corruption — the table is only reachable through the two known call
  sites.

## 7. Gotchas

- **Do not reuse the native `return 0` / no-op function addresses** (e.g.
  pointing our table at `FUN_1801ab270`) — that saves nothing and adds two
  address dependencies; mod stubs are free.
- **`+0xB8` masking still owns visibility.** A header row hidden by the
  scroll window must be masked exactly like any other row; slot-0 has no
  effect on layout/render.
- **Focus-clear on the previously-focused row** goes through slot-1
  (`onFocusChanged(false)`) — headers can never be the previously-focused
  row, but the no-op stub makes this unconditionally safe anyway.
- The gray row uses the standard `"option_item"` metrics key, so it is a
  full-height slot — but §5 shows the y-extent at `row+0xA8` is a per-row
  layout input: overwrite it after the ctor for a genuinely slimmer slot.
  Combine with header art sized to the reduced slot.
