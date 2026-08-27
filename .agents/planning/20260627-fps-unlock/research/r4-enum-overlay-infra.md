# R4 — Adding a real `Enum` RowKind to the mod_menu overlay

**Status: codebase survey complete (not RE — this is local Rust).** Scopes the "Part I"
reusable overlay work: add `RowKind::Enum` alongside `Boolean`/`Scalar` in
`src/mods/mod_menu.rs` (990 lines). The existing `Scalar` row (added by the timing-offsets
feature) is the structural template.

## Current row model (relevant excerpts)

```rust
pub enum RowKind {                                   // mod_menu.rs:59
    Boolean { value: bool },
    Scalar  { value: i32, min: i32, max: i32, step_fine: i32, step_coarse: i32 },
}

pub struct MenuRow {                                 // :77
    pub key: String, pub label: String, pub hint: String,
    pub indent: u8,
    pub kind: RowKind,
    pub visible_when: Option<(String, i32)>,         // parent row key + required value
    pub on_change: Option<RowChangeCallback>,        // Arc<dyn Fn(i32) + Send + Sync>
}

pub struct ScalarRowSpec { key,label,hint,parent_row_key,min,max,step_fine,step_coarse,
                           initial, on_change }      // :166  (public registration API)
```

Key existing facts:
- `RowChangeCallback` is `Arc<dyn Fn(i32)+Send+Sync>` — **already i32-valued**, so an Enum
  row can reuse it verbatim by passing the **selected raw value** (the FPS number), which
  is exactly what FPS wants (idea-honing Q5: store raw value, not index).
- `visible_when: Option<(String,i32)>` + `parent_row_key` already gives us "hide the enum
  row when the master toggle is OFF" (Q8.4) for free — same mechanism timing-offsets uses.
- Mod-contributed rows live in `contributed_rows`, registered via `register_scalar_row`,
  removed via `remove_rows_for(keys)` (:216). We add a parallel `register_enum_row`.

## Every site that `match`es on `RowKind` (must handle the new variant)

The `match &row.kind` / `match row.kind` blocks are **exhaustive (no `_` arm)**, so adding
`Enum` makes the compiler flag exactly these sites — a clean, bounded change:

| Site | Fn | What to add for `Enum` |
|---|---|---|
| `:229` | `row_value` | return the current selected **value** (for `visible_when` parent checks; an enum is unlikely to be a parent, but keep it total) |
| `:295` | `clone_row` | clone the `Enum { index, values, labels }` payload |
| `:573` | `activate_selected` | Left/Right = **cycle index** (`idx ± 1`), resolve new value, fire `on_change(value)`, mirror into row. (Start-held coarse is N/A for enum; default: ignore, or jump to first/last — decide in design.) |
| `:665` | `selected_is_scalar` (repeat gate) | generalize so **Enum also auto-repeats** on hold (nice for a 6+ entry list) |
| `:807` | `set_row_value_and_refresh` writer | set the enum's selected value/index |
| `:920` | `refresh_slots` (render) | value column shows the **label** for the current entry (e.g. `"144fps"`), white text |

Plus a new public API (`register_enum_row` + `EnumRowSpec`) mirroring `register_scalar_row`
(:188), and `row_value` totality.

## Recommended `Enum` shape

```rust
RowKind::Enum {
    index: usize,            // selected position in `values`
    values: Vec<i32>,        // raw values (e.g. [60,120,144,165,240,360]) — sorted asc
    labels: Vec<String>,     // display per entry (e.g. ["60fps","120fps",...]) parallel to values
}
```
- `on_change` is fired with `values[index]` (the **raw FPS value**), matching Q5.
- The mod owns normalization (sort/dedupe/auto-add-selected) before registering — keep the
  overlay dumb: it just cycles a prebuilt parallel `values`/`labels`.
- **Cycle behavior: recommend clamp at ends** (Left at index 0 = no-op; Right at last =
  no-op) to mirror `Scalar`'s clamp-at-bound semantics — no surprise wrap. Confirm in design.
- `EnumRowSpec { key, label, hint, parent_row_key, values, labels, initial_value,
  on_change }` — `initial_value` is a raw value; the mod resolves it to an index (auto-add
  if missing per Q5.2) before constructing the row.

## Hold-to-repeat

The repeat thread (`start_repeat_thread` :682) re-fires `activate_selected` while a
direction is held, gated by `selected_is_scalar` (:665). Generalize that gate to "scalar
**or** enum" so the enum cycles on hold. Trivial.

## Effort estimate

**Small–medium, low-risk.** ~6 match arms + 1 new spec struct + 1 new `register_*` fn +
repeat-gate tweak. No new threads, no FFI, no allocator concerns — pure overlay-state Rust.
Exhaustive matches make it self-checking (compiler points at every site). Same *shape* and
scale of change the timing-offsets feature made to add `Scalar`, so there's a proven path.
Reusable by any future mod wanting a labeled pick-list.

## Degradation (idea-honing Q7 tier 2)

The enum row is the **optional** tier: if `register_enum_row` can't run / overlay infra is
unavailable, the FPS mod still applies `selected` from config. The Enum work is additive
and can't break the load-bearing apply path.
