# Filter Menu System — Deep Dive

Reverse-engineering reference for DDR World's song-select **FILTER** overlay: how every
filter menu (LEVEL, DIFFICULTY, VERSION, MUSIC TITLE, CLEAR RANK, CLEAR TYPE, FLARE RANK,
FLARE SKILL TARGET, BPM, GENRE, EVENT) is defined, laid out, rendered, evaluated and
persisted. Written as the "bones" document for a future config-driven filter system
(user-defined menus, row layouts and per-item filter values in `mod-config.json`); it
deliberately does **not** propose a schema.

**Primary build**: `gamemdx.dll` 20260915. **Cross-checked**: 20260721, 20250805 (see
[Cross-version notes](#11-cross-version-notes)). **Ghidra base** `0x180000000`; every
address below is file-relative and version-specific — a mod must derive them (AOB / RTTI /
string xref), never hardcode them.

**Related notes** (older, VERSION-centric, some conclusions corrected in
[section 12](#12-corrections-to-earlier-notes)):
[series_filter_internals.md](series_filter_internals.md),
[filter_ui_extension.md](filter_ui_extension.md),
[filter_scroll_research.md](filter_scroll_research.md),
[genre_filter_expansion_research.md](genre_filter_expansion_research.md),
[afp_system.md](afp_system.md), [folder_system_research.md](folder_system_research.md).
Existing consumers in the tree: `src/mods/series_expansion/` (legacy table
extension and the experimental enhanced layout, which follows strategy B of
[9.2](#92-two-implementation-strategies)), `src/services/series_filter_scroll.rs`,
`src/mods/improved_song_title_sorting/` (per-letter MUSIC TITLE, strategy B plus a
recreated row break). Findings added by the enhanced-layout work:
[section 15](#15-addenda-2026-09-27); by the title work: [section 16](#16-addenda-2026-09-27-per-letter-music-title-improved_song_title_sorting).

---

## Table of contents

1. [TL;DR](#1-tldr)
2. [Object model and lifecycle](#2-object-model-and-lifecycle)
3. [Question 1 — what decides a row's layout](#3-question-1--what-decides-a-rows-layout)
4. [Question 2 — how per-item filters are defined and applied](#4-question-2--how-per-item-filters-are-defined-and-applied)
5. [Per-category reference](#5-per-category-reference)
6. [Labels, textures and summary text](#6-labels-textures-and-summary-text)
7. [Selection state and persistence](#7-selection-state-and-persistence)
8. [Interaction behaviours](#8-interaction-behaviours)
9. [Implications for a fully configurable filter system](#9-implications-for-a-fully-configurable-filter-system)
10. [Struct layouts](#10-struct-layouts)
11. [Cross-version notes](#11-cross-version-notes)
12. [Corrections to earlier notes](#12-corrections-to-earlier-notes)
13. [Address reference (20260915)](#13-address-reference-20260915)
14. [Open questions](#14-open-questions)
15. [Addenda (2026-09-27)](#15-addenda-2026-09-27)
16. [Addenda (2026-09-27): per-letter MUSIC TITLE](#16-addenda-2026-09-27-per-letter-music-title-improved_song_title_sorting)

---

## 1. TL;DR

**Row layout (Q1).** There is no "row type". The item area is a generic
`sequence::GridPanel` doing a **wrapping flow layout** (think CSS `flex-wrap`) inside a
**216 × 266** box with **zero gap**, left-aligned. Every item is a `FilterButton` whose
cell size comes from one of five AFP templates, `filter_switch_base01..05`:

| Template | Cell (w × h) | Items that fit in 216 px | Used by |
|---|---|---|---|
| `filter_switch_base01` | 220 × 26 | 1 (overflows → always its own row) | DIFFICULTY, BPM, GENRE, EVENT, FLARE SKILL TARGET, FLARE "NO RANK" |
| `filter_switch_base02` | 108 × 26 | 2 | VERSION entries, CLEAR TYPE |
| `filter_switch_base03` | 72 × 26 | 3 | VERSION group tabs, CLEAR RANK |
| `filter_switch_base04` | 54 × 26 | 4 | MUSIC TITLE |
| `filter_switch_base05` | 42 × 26 | 5 | LEVEL, FLARE I–EX |

The template index is chosen **per button** by the category's builder function (a
per-category constant for most menus, a per-entry table field for DIFFICULTY / FLARE RANK /
CLEAR TYPE). Items are placed in **builder creation order**; a row ends when the next cell
would overflow 216 px. Mixed rows are just mixed template widths: VERSION emits three
72-px group tabs, then an invisible full-width 1-px **`FilterHeader`** (a forced line break),
then 108-px entries. FLARE RANK emits one 220-px "NO RANK" cell followed by 42-px cells.

**Per-item filters (Q2).** Each category has (a) a **static entry table** built once per
process inside the filter-init function, (b) a **builder** that turns table rows into
buttons, (c) a **predicate** lambda that tests a song/chart against the category's
selected entries, (d) a **summary** lambda that renders the song-select filter chip text,
and (e) a **persisted `u64` bitfield** in the player profile (`filtersort/<name>`).
Selection is keyed by *(category id, selection index)*; the selection index is also the
persisted bit number (so ≤ 64 entries per category). Semantics: **OR within a category,
AND across categories**, an empty category imposes no constraint. The grouped menus you
care about are **range tables**: MUSIC TITLE entry *i* matches title-class values
`[start_i, start_{i+1})`, VERSION entry *i* matches (mapped) series values
`[start_i, start_{i+1})`. Title classes are already per-letter (A=10 … Z=35), so splitting
"A,B,C" into A / B / C is a table + UI + texture change only. Kana lines (ア行 … ワ行) are
the finest the stock classifier produces; splitting further needs a classifier hook.

---

## 2. Object model and lifecycle

```
song-select setup (FUN_18010b2e0)
 └─ FUN_1801102f0 → FilterPanel::FilterPanel (FUN_180135ba0)          once per song-select scene (inferred)
      └─ filter init FUN_18011f600
           ├─ builds 11 static entry tables (once per process, guard bits DAT_181237450)
           ├─ registers 11 `Filter` records into FilterManager+0x358   (predicate, summary, sorter)
           └─ registers 11 category UI records into FilterPanel+0x1F8  (builder + category-button creator)
 └─ FilterPanel setup (FUN_180137370, called on the +0xC0 sub-object)
      ├─ loads filter_root / filter_logo / filter_result_number / filter_change_selection / shades
      ├─ creates GridPanel #1 (category list)  → FilterPanel+0x220
      ├─ creates GridPanel #2 (item area)      → FilterPanel+0x228
      ├─ FUN_18011f530: creates the 11 FilterCategoryButtons in fixed order
      └─ FUN_180138d20: sizes both GridPanels from filter_root placeholder layers

opening / focusing a category (category button's stored std::function → FUN_180127010)
 ├─ GridPanel#2.clear()                         (destroys previous FilterButtons → dtor fires)
 ├─ invoke category builder(capture, factory)   (factory = lambda88 → FUN_180127c50)
 │     for each entry: btn = factory(selection_index); SetTemplate(btn, N); btn.label = "<prefix>_<key>"
 └─ append a full-size "Reset" (Deselect all and close) navigation target
```

Key players (RTTI names, `sequence::selectmusic::` unless noted):

| Class | Size | Role |
|---|---|---|
| *FilterManager* (no RTTI name; global `DAT_1806f2d50`) | ≥ 0x3FC | Owns selection state, registered `Filter`s, sorters, simple-mode flag. Constructed by `FUN_1800fd2f0`, which also loads selections from the profile. |
| `FilterPanel` | ≥ 0x240 | The overlay. Holds the category map (+0x1F8), both GridPanels, AFP handles. |
| `sequence::GridPanel` | 0x240 | Generic flow-layout container with cursor, navigation and optional follow-cursor scroll. |
| `FilterCategoryButton` | 0x170 | Left-column row (`filter_item` template, label `sefi_item_<name>`). |
| `FilterButton` | 0x1D0 | One selectable item (`filter_switch_baseNN` template, label `sefi_<prefix>_<key>`). |
| `FilterHeader` | 0xF8 | Invisible full-width 1-px row break (VERSION only). |
| generic button (`FUN_18004fe40`) | 0x148 | The per-category "Reset" navigation target, tags `Reset`/`System`. |
| `FilterCard` | — | Song-select header chip showing active filter summaries and "N Songs"/"N Charts". |
| `ChartMetadata` | 0x90 | Per-song (or per-chart) record the predicates evaluate. |

The FilterPanel lives for the whole song-select scene; **item buttons are rebuilt every time
a category is opened** (the existing `series_filter_scroll` service relies on this).
The static tables are built exactly once per process.

---

## 3. Question 1 — what decides a row's layout

### 3.1 The layout engine: `GridPanel` flow layout

Both columns of the overlay are `sequence::GridPanel` instances (ctor `FUN_180048cb0`,
vtable `0x18035FCB8`). Every frame `GridPanel::Update` (`FUN_18004BD90`, vtable slot 7)
calls `FUN_18004BAD0` → `FUN_18004AD10`, which recomputes every child's position from
scratch (this is why writing a FilterButton's `+0x90` Y has no effect). Reconstructed:

```text
main  = orientation(+0xC0) == 0 ? X : Y        // FUN_18004BD30 caches axis selectors at +0x138/+0x140
cross = the other axis
per_line_forced = (+0x238 > 0) ? child_count / +0x238 : 0
cursor_main = 0; line_cross = 0; offset_cross = 0; line_start = 0

for i, child in children:                       // children vector at +0x68..+0x70, in insertion order
    if !child.layout_visible(+0xB8): continue   // invisible children take no space
    size = child.size(+0xA0,+0xA8)
    forced = per_line_forced && i != 0 && i % per_line_forced == 0
    if cursor_main + size[main] > container.size[main]  or forced:     // strict '>'
        offset_cross += gap[cross](+0xD0/+0xD8) + line_cross (+ extra(+0x134) if forced)
        align_line(line_start .. i)             // FUN_18004B6A0
        cursor_main = 0; line_cross = 0; line_start = i
    child.pos(+0x88,+0x90) = (cursor_main, offset_cross)   on (main, cross)
    cursor_main += size[main] + gap[main]
    line_cross   = max(line_cross, size[cross])
align_line(line_start .. end)
content_extent(+0x208/+0x210) = offset_cross + line_cross + ...
then (FUN_18004BAD0): optional follow-cursor scroll offset (+0x150/+0x158) added to every child
```

`align_line` (`FUN_18004B6A0`) shifts each finished line along the main axis by
`(container_main − used_main) × (+0xC4 × 0.5)`: `+0xC4` = 0 left, 1 centre, 2 right.

**Filter item area configuration** (set in `FUN_180137370` / `FUN_180138D20`):

| Field | Value | Source |
|---|---|---|
| orientation `+0xC0` | 0 (rows along X) | ctor default |
| main-axis align `+0xC4` | 0 (left) | ctor default — why LEVEL's last row "16 17 18 19" is left-aligned |
| gap `+0xD0/+0xD8/+0xE0` | 0 / 0 / 0 | explicitly zeroed |
| wrap-around navigation `+0x12C` | 1 | set |
| `+0x128` | 1 (ctor default 2; meaning unknown) | set |
| size `+0xA0/+0xA8` | **216 × 266** | width/height of layer `switch_usr/dummy_choice_usr` in `filter_root` (shape `filter_root_shape133`, bounds 0,0–216,266) |
| position `+0x88/+0x90` | derived from the same layer's position (+10 px Y) | `FUN_180138D20` |

The category list GridPanel is sized the same way from `dummy_conditions_usr`
(**232 × 352**, i.e. exactly 11 × 32-px rows).

### 3.2 Cell templates and where their size comes from

`FilterButton::SetTemplate` (`FUN_1801345B0`) stores the template index at
`FilterButton+0xF0`, then looks the size up in a process-wide cache
(`std::map<int,{double w,h}>` at `DAT_180CF34A0`, `FUN_18013A190`). On a miss it
instantiates the AFP movie `filter_switch_base%02d` (index printed with `%02d`) and reads
its size through `CLayer` vtable `+0x120` (`FUN_18026EE40` → libafp `Ordinal_71`
`afp_layer_get_info`). The result is copied into `FilterButton+0xA0/+0xA8`.

The five templates live in `data/arc/bm2d/select_music_option_v3.arc` →
`select_music_option_v3.ifs` → `afp/filter_switch_base0N` (+ `geo/filter_switch_base0N_shape*`).
Their AFP header dimensions (bemaniutils `afputils parseafp`):

| Template | Dimensions | Highlight textures (per-width) | Label canvas used by stock labels |
|---|---|---|---|
| 01 | 220 × 26 | `sefi_select_base01` / `sefi_on_base01` 224×24 | 220×20 (GENRE, EVENT, NO RANK); 104×20 (DIFFICULTY, BPM, FLARE SKILL TARGET) |
| 02 | 108 × 26 | `sefi_select_base02` / `sefi_on_base02` 112×24 | 104×20 (64×20 for CLEAR TYPE) |
| 03 | 72 × 26 | `sefi_select_base03` / `sefi_on_base03` 76×24 | 64×20 (32×20 for CLEAR RANK) |
| 04 | 54 × 26 | `sefi_select_base04` / `sefi_on_base04` 56×24 | 44×20 |
| 05 | 42 × 26 | `sefi_select_base05` / `sefi_on_base05` 44×24 | 32×20 |

All five share one internal structure (children `choices_usr` = label, `mark_usr` /
`mark2_usr` = check mark, `select_base`, `switch_base`, `ef`; frame labels `in`, `loop_off`,
`loop_select`, `loop_switch`, `loop_switch_select`, `out`, `end`) and differ only in
geometry, BSI scrambling and the per-width background textures. The index is a plain
`%02d`, so **a sixth template (`filter_switch_base06`) is addressable by index 6** if an
AFP with that name exists in the IFS (see [9.3](#93-constraints-and-gotchas)).

Items-per-row therefore follows from `floor(216 / cell_w)` with zero gap:
220 → 1 (overflows; every item wraps), 108 → 2, 72 → 3, 54 → 4, 42 → 5.
The screenshots are consistent once the CrossOver window's non-square scale
(≈1.48 × horizontal, ≈1.38 × vertical) is divided out: 108/54/42-px column pitch and
26-px row pitch in the item area; 32-px row pitch in the category list.

### 3.3 Who chooses the template

Every builder calls `SetTemplate(button, N)` right after creating the button:

| Category | Template source | Value(s) |
|---|---|---|
| MUSIC TITLE | builder capture `+0x00` | 4 |
| VERSION group tabs | literal in builder | 3 |
| VERSION entries | builder capture (`param_1[9]`) | 2 |
| GENRE, BPM, EVENT, FLARE SKILL TARGET | builder capture `+0x00` | 1 |
| LEVEL | builder capture `+0x00` | 5 |
| CLEAR RANK | builder capture `+0x04` | 3 |
| DIFFICULTY | **per-entry** table field `+0x08` | 1 for all five |
| FLARE RANK | **per-entry** table field `+0x08` | 1 for "NO RANK", 5 for I … EX |
| CLEAR TYPE | **per-entry** table field `+0x08` | 2 for all nine |

The capture values are written by the filter-init function as immediates
(`local_1a8 = 4` for title, `local_120 = 2` for version, …), identical in 20250805.

### 3.4 Row breaks: `FilterHeader`

The VERSION builder (`FUN_180124220`) is the only user. Between the group-tab loop and the
entry loop it allocates a 0xF8-byte `FilterHeader`, pushes it into the item GridPanel's
children and sets its parent (`+0x60`). Its layout hook (`FUN_1801340C0`, Component
vtable slot 3) sets `width = parent.width` (216) and `height = 1.0`. In the flow algorithm a
full-width child always starts a new line and forces the *next* child onto another line,
so it acts as an explicit line break costing 1 px of height. It owns an empty `CLayer`
(nothing is drawn) and is not focusable, so navigation skips it.

Practical row-break options for custom menus, in order of cost:

1. A full-width item (template 01) always occupies a whole row.
2. Rows naturally break when widths no longer fit (e.g. three 72-px cells fill 216 exactly,
   so the next 108-px cell wraps even without a header).
3. A `FilterHeader`-style spacer (either a real `FilterHeader` built with the game's vtables
   — RTTI-derivable — or any `sequence::Component` with the desired size).
4. `GridPanel+0x238` (forced line count) — uniform only, not per-row.
5. Setting a child's `+0xB8` to 0 removes it from layout and navigation without deleting it.

### 3.5 Order

Children are laid out in **insertion order**, which is the builder's creation order — not
the selection index, not the table order. The VERSION builder walks its tables backwards
(group tabs 2→0 = GOLD, WHITE, CLASSIC; entries 8→0 = WORLD, A3, A20–A20 PLUS, A, …,
1st–5thMIX), which is why the newest version is first. CLEAR RANK walks display rows
0..16 but hands the factory a remapped selection index ([5.8](#58-clear-rank-category-7)).

### 3.6 After the items: the Reset target

`FUN_180127010` appends a generic button (tags `Reset`, `System`, sound
`se_common_cancel_b`) whose size is set to the **whole container** (216 × 266), so it always
lands on its own line after the last item. It is a navigation target only; the visible
"Deselect all and close" graphic is `switch_usr/button_usr` in `filter_root`, whose frame
label is toggled `loop_on`/`loop_off` by `FUN_180127E90` when the Reset target has focus.

### 3.7 Navigation

GridPanel binds four `std::function` handlers (`+0x178/+0x198/+0x1B8/+0x1D8`, input codes
3/4/1/2). Left/right step the index (±1, wrapping when `+0x12C` is set). Up/down
(`FUN_18004A970`) walk the children in index order until one's **cross-axis coordinate
differs by more than a threshold** (i.e. the next visual row), skipping non-focusable and
layout-invisible children; the remembered main-axis coordinate `+0x1F8` is used to land in
the nearest column. Navigation is therefore purely geometric: mixed rows, headers and
different widths need no special handling.

Range selection (Decide held + direction, "Select Range and Close") is implemented in
`FilterPanel` input (`FUN_180136050`) over GridPanel indices: it records the anchor
(`FilterPanel+0x218`, −1 when idle), then walks the cursor from the anchor back to the
current position and sends every `FilterButton` on the way the same `ON`/`OFF` tag event,
chosen from the anchor button's is-selected state. It works on GridPanel index order and is
independent of selection indices and tables (so a custom builder's creation order also
defines what a "range" is).

### 3.8 Capacity, overflow and scrolling

* Visible area 266 px ⇒ **10 full rows of 26 px** (10.2). Stock menus use ≤ 6 rows
  (VERSION: 1 tab row + header + 5 entry rows; CLEAR RANK: 6 rows; LEVEL: 4).
* **No clipping.** FilterButtons are independent BM2D movie clips in the pool; they only
  inherit the scale (`CLayer+0xC0`) and colour/alpha (`CLayer+0x90`) of
  `switch_usr/dummy_choice_usr` each frame (`FUN_1801355C0`). The `aep_set_rect_mask` calls
  in `filter_root`/`filter_switch` bytecode only mask the panel's own wipe animations.
  Overflowing rows draw outside the panel (observed by series-expansion testing).
* GridPanel has a **native follow-cursor scroll**: when `+0x100` is set, `FUN_18004BAD0`
  eases an offset (`+0x150/+0x158`, factor `+0x120` = 0.75) toward
  `anchor(+0xE8) − focused_child.pos` on the cross axis and adds it to every child. That pins
  the focused row at a fixed anchor (wheel-style) rather than scrolling only at the edges,
  and it still needs external clipping. The filter panel leaves `+0x100` = 0.
* `src/services/series_filter_scroll.rs` implements edge scrolling for VERSION by hooking
  BM2D `set_position` and masking rows with `afp_layer_set_mask`.
* The left category list is sized for exactly 11 rows (232 × 352); a 12th category would
  overflow it the same way.
* All FilterButtons share the 1024-slot BM2D pool with the rest of the scene.

---

## 4. Question 2 — how per-item filters are defined and applied

### 4.1 Pipeline

For each category the filter-init function (`FUN_18011F600`, called from the FilterPanel
ctor) does:

1. **Static table** (once per process; guard bit in `DAT_181237450`): an array of
   fixed-stride structs in `.data`, filled with SSO `std::string`s via
   `FUN_180003990`/`FUN_180003AC0` and integer immediates. An `atexit` destructor is
   registered. Tables are listed in [section 5](#5-per-category-reference).
2. **Summary** `std::function<std::string()>` (e.g. lambda4 → title summary) — produces the
   text in the song-select `FilterCard` chip ("TITLE A~C", "Level 12", …).
3. **Predicate** `std::function<bool(shared_ptr<ChartMetadata>)>` (lambda3/7/12/…) with
   capture `{FilterManager*, category_id}`, wrapped either by `FUN_18011F230` (**song-level**,
   `Filter+0x00 = 0`) or `FUN_18011F350` (**chart-level**, `Filter+0x00 = 1`).
4. Optionally a **Sorter** copied from `FilterManager+0x3D8[sort_id]` (`FUN_18011F4D0`) so
   that, when this is the *only* active filter, the song list auto-sorts by that dimension.
5. The combined **`Filter`** (0xB0) is stored in `FilterManager+0x358[category_id]`
   (`FUN_1801028D0` + `FUN_180102100`, or `FUN_180102180` for chart-level ones).
6. **UI record** in `FilterPanel+0x1F8[category_id]` (`FUN_180128500` + a per-category
   `FUN_1801288xx..129xxx` setter): a `std::function<void()>` that creates the category
   button and, through it, the builder `std::function<void(std::function<FilterButton*(int)>)>`
   (lambda5/10/14/…) with its capture (template, label prefix, table pointers).

### 4.2 Selection model and evaluation

* **State**: `FilterManager+0x378` — `std::map<int category, std::list<int>>` of selected
  **selection indices** (`FUN_1801D59E0` = lookup, `FUN_1801D5680` = set/clear one,
  `FUN_1801D5740` = clear category, `FUN_1801D57B0` = clear all).
* **Button ↔ state**: the factory (`FUN_180127C50`) binds each `FilterButton` to
  `(category, selection_index)` through four lambdas: lambda92 (pre-press: clear all if
  Simple mode), lambda93 (toggle → `FUN_1801D5680`, then notify `FUN_180137230`), lambda94
  (post-press: close if Simple mode), lambda95 (is-selected → drives the check mark).
* **Active filters** (`FUN_180100AD0`): every registered `Filter` whose category has a
  non-empty selection list. (If `FilterManager+0x2C8` is set, a single override filter at
  `+0x2A8` replaces the whole set.)
* **Combined predicate** (`FUN_180101180`): logical **AND** of all active filters'
  predicates. Each category predicate returns true if **any** selected index matches
  (**OR**). Categories with no selection impose nothing.
* **Song vs chart level**: if any active filter is chart-level the list counts charts
  ("%d Charts" in `FilterCard`), otherwise songs ("%d Songs"). Chart-level predicates read
  the chart at the player's current difficulty cursor unless the `ChartMetadata` is already
  bound to one difficulty (`FUN_1801A70F0`; `ChartMetadata+0x70` = difficulty, 5 = song).
* **Recount**: any toggle calls `FUN_180137230`, which marks the manager dirty
  (`+0x1C1/+0x1C2`), posts event 6 (rebuild list + "Filtering Results"), and flags the panel.

### 4.3 The three matching shapes used by stock predicates

| Shape | Test | Categories |
|---|---|---|
| **Range** | `table[i].start <= v < table[i+1].start` (needs a sentinel row) | TITLE, VERSION, BPM, CLEAR RANK (via remap), CLEAR TYPE |
| **Equality** | `v == table[i].value` | DIFFICULTY, LEVEL, FLARE RANK, FLARE SKILL TARGET |
| **Bit test** | `(1 << table[i].bit) & property` (+ bit-8 quirk) | GENRE |
| **Hardcoded** | switch on the selection index | EVENT |

`v` always comes from a single extractor per category ([section 5](#5-per-category-reference)).
Every predicate is a small leaf function that reads **one hardcoded table base** and
**one hardcoded stride** — the two things a table-extension patch must redirect.

### 4.4 Sorter coupling

`FUN_180100C00` picks the active sort: an explicit user sort (`FilterManager+0x3D0` ≠ 0,
profile `filtersort/sort_type`) wins; otherwise, if exactly one active filter carries a
sorter, that sorter is used; sorter id 3 is always appended as the final tie-breaker.
Sorter ids attached by the filter init: VERSION→4, BPM→5, LEVEL→2, FLARE RANK→6,
CLEAR RANK→7, CLEAR TYPE→8, FLARE SKILL TARGET→9 (TITLE, GENRE, EVENT, DIFFICULTY attach
none). The sorters themselves (`_anon_6457BC0D` lambda pairs `int(ChartMetadata)` /
`string(ChartMetadata)`) generate song-wheel group headers; they are out of scope here but
a custom category can reuse one.

---

## 5. Per-category reference

Category ids are the keys of every map and the index of the persisted bitfield. The
left-column order is a hardcoded `int[11]` in `FUN_18011F530`:
`{5, 11, 1, 0, 7, 12, 6, 8, 3, 2, 4}`.

| Id | Menu | Category-button label | Item label prefix | Template | Entries | Table (stride) | Predicate | Level |
|---|---|---|---|---|---|---|---|---|
| 0 | MUSIC TITLE | `sefi_item_music_title` | `title` | 4 | 20 | `0x180CF67C0` (0x88) | `FUN_1801239D0` | song |
| 1 | VERSION | `sefi_item_version` | `version` | 3 tabs / 2 entries | 3 + 9 | `0x180CF3E40` (0x30) + `0x180CF6270` (0x88) | `FUN_180123E40` | song |
| 2 | GENRE | `sefi_item_genre` | `genre` | 1 | 7 | `0x180CF5FD0` (0x60) | `FUN_180124600` | song |
| 3 | BPM | `sefi_item_bpm` | `bpm` | 1 | 8 | `0x180CF5DD0` (0x38) | `FUN_1801249E0` | song |
| 4 | EVENT | `sefi_item_event` | `event` | 1 | 3 (hardcoded) | — | `FUN_180124DC0` | song |
| 5 | LEVEL | `sefi_item_level` | `level` | 5 | 19 | `0x180CF5790` (0x38) | `FUN_1801259F0` | chart |
| 6 | FLARE RANK | `sefi_item_flare_rank` | `flare` | per entry | 11 | `0x180CF5310` (0x68) | `FUN_180125E20` | chart |
| 7 | CLEAR RANK | `sefi_item_clear_rank` | `rank` | 3 | 17 | `0x180CF4C50` (0x60) + remap `0x180370F70` | `FUN_180126270` | chart |
| 8 | FLARE SKILL TARGET | `sefi_item_flare_skill_target` | `fltarget` | 1 | 2 | `0x180CF4780` (0x60) | `FUN_180126BB0` | chart |
| 9 | *(rival flare skill — no UI)* | (`sefi_item_vs_rival_flare_rank` texture only) | — | — | 3 | — | — | — |
| 10 | *(rival score rank — no UI)* | (`sefi_item_vs_rival_score_rank` texture only) | — | — | 3 | — | — | — |
| 11 | DIFFICULTY | `sefi_item_difficulty` | `dif` | per entry | 5 | `0x180CF5BC0` (0x68) | `FUN_180125560` | chart |
| 12 | CLEAR TYPE | `sefi_item_clear_type` | `cl` | per entry | 9 | `0x180CF4840` (0x68) | `FUN_180126740` | chart |

"Entries" is what the persistence count function (`FUN_1801D55B0`) reports; builders and
predicates hardcode the same numbers as loop bounds (`CMP off, count*stride−1`).
Category-button label names are string literals inside each category's button-creator
lambda (e.g. `FUN_180123D60` passes `"music_title"`), not the item prefix.

### 5.1 MUSIC TITLE (category 0)

**Table** `0x180CF67C0`, stride 0x88, 20 rows + sentinel:

| Off | Type | Field |
|---|---|---|
| +0x00 | u32 | row index |
| +0x08 | std::string | key (`line_a`, …, `other`) |
| +0x30 | i32 | **range start** (title class) |
| +0x38 | std::string | summary first-label |
| +0x60 | std::string | summary last-label |

| Row | Key | Start | Covers classes | Summary labels |
|---|---|---|---|---|
| 0–9 | `line_a` `line_ka` `line_sa` `line_ta` `line_na` `line_ha` `line_ma` `line_ya` `line_ra` `line_wa` | 0 … 9 | one kana line each | あ~お, か~こ, さ~そ, た~と, な~の, は~ほ, ま~も, や~よ, ら~ろ, わ~ん (Shift-JIS) |
| 10 | `abc` | 10 | 10–12 (A–C) | A / C |
| 11 | `def` | 13 | 13–15 | D / F |
| 12 | `ghi` | 16 | 16–18 | G / I |
| 13 | `jkl` | 19 | 19–21 | J / L |
| 14 | `mno` | 22 | 22–24 | M / O |
| 15 | `pqr` | 25 | 25–27 | P / R |
| 16 | `stu` | 28 | 28–30 | S / U |
| 17 | `vwx` | 31 | 31–33 | V / X |
| 18 | `yz` | 34 | 34–35 | Y / Z |
| 19 | `other` | 36 | 36 | Other / Other |
| 20 | sentinel | 37 | — | — |

**Builder** `FUN_180123B90`: rows 0→19 in order, selection index = row, template 4 ⇒ 5 rows
of 4 (screenshot 3.png). Labels `sefi_title_<key>` (44×20).

**Value** `ChartMetadata+0x88`, computed once in the ChartMetadata ctor (`FUN_1801A6BF0`)
by the **title classifier** `FUN_1801A7840` on the result of `FUN_1801A7710`:

1. Source string = `music::Info` vtable `+0x28` (`FUN_1801B20D0`): `title_yomi` (Info+0x40)
   if non-empty, else `title` (Info+0x18).
2. `FUN_1801A7710` converts encoding (`me::text::Encoding` kind 2 → kind 0; inferred to be
   UTF-8 → Shift-JIS, since the boundaries it is compared with are Shift-JIS).
3. First byte ASCII letter → `tolower(c) − 0x57` (a=10 … z=35).
   Printable non-letter ASCII (digits, symbols, space) → 36 ("other").
   Otherwise (multi-byte): the highest *i* ∈ 0..9 such that the kana boundary *i* ≤ string
   (`_mbscmp`-style `FUN_18027B200`), boundaries at `0x180380090`: ア カ サ タ ナ ハ マ ヤ ラ ワ
   (katakana, SJIS `83 41` … `83 8F`); if below ア → 36.
   Quirk: anything sorting at or after ワ — including kanji and ヴ — classifies as ワ行;
   a hiragana-initial yomi classifies as "other".

**Granularity**: 37 classes. A–Z are already individual, so single-letter items are
possible with table changes only. Individual kana (ア, イ, …) or finer buckets need a hook
on `FUN_1801A7840` (single caller) to emit extra class values — keeping existing numbering
stable if anything else consumes `+0x88` (see [open questions](#14-open-questions)).

**Summary** `FUN_180123A60`: prefix `"TITLE "`, `FUN_1801235B0(count = 20, …)`.

### 5.2 VERSION (category 1)

Fully documented in [series_filter_internals.md](series_filter_internals.md); summary:

**Entry table** `0x180CF6270`, stride 0x88, 9 rows + sentinel: `+0x00` group index,
`+0x08` key, `+0x30` range start (mapped series), `+0x38` code (summary first-label),
`+0x60` display (summary last-label). Rows: `1th5th`(1) `maxex`(6) `novanova2`(9) `x`(11)
`1314`(14) `a`(17) `a20plus`(18) `a3`(20) `world`(21), sentinel 22.

**Group table** `0x180CF3E40`, stride 0x30, 3 rows + sentinel: `+0x00` first entry index,
`+0x08` key. `classic`(0) `white`(4) `gold`(6), sentinel 9.

**Builder** `FUN_180124220`: group tabs 2→0 with template 3 (via `FUN_180124000`, a direct
FilterButton factory, not the category factory), then a `FilterHeader`, then entries 8→0
through the category factory with template 2. Labels `sefi_version_<key>` (104×20 for
entries, 64×20 for group tabs). Group-tab press (`FUN_180127810`): clear category 1, then
select entries `[group[g].start, group[g+1].start)` — a macro, not a filter of its own.

**Value** `FUN_1800FFCB0` (series mapper): raw `<series>` u8 (Info+0x138), switch-mapped
(1–21 identity except 16→15; unknown → 0).

**Granularity**: raw series values (1..21 stock, 1..255 possible). One entry per series is a
table change; separating the two values the mapper merges needs the mapper patched (the
series-expansion mod already owns that default-case patch).

**Summary** prefix `"DDR "`, count 9 (the `filter_label_builder_count` site).

### 5.3 GENRE (category 2)

Table `0x180CF5FD0`, stride 0x60, 7 rows, no sentinel: `+0x00` row, `+0x08` key,
`+0x30` property bit, `+0x38` display.

| Row | Key | Bit | Display |
|---|---|---|---|
| 0 | `popmusic` | 2 | POP MUSIC |
| 1 | `virtualpop` | 3 | VIRTUAL POP |
| 2 | `animegame` | 4 | ANIME & GAME |
| 3 | `touhou` | 5 | TOUHOU… |
| 4 | `variety` | 8 | VARIETY |
| 5 | `hinabitabanmeshi` | 7 | ひなビタ♪&バンめし♪ |
| 6 | `audition` | 9 | AUDITION |

Predicate: `property = Info+0x178 ?: Info+0x174`; match if `(property >> bit) & 1`, or bit
8 with `property & 0x40`. Template 1; labels `sefi_genre_<key>` (220×20, **language IFS**).
Details and folder interplay: [genre_filter_expansion_research.md](genre_filter_expansion_research.md).
Any of the 32 property bits can back an item.

### 5.4 BPM (category 3)

Table `0x180CF5DD0`, stride 0x38, 8 rows + sentinel: `+0x08` key, `+0x30` **u16** lower bound.
Rows `under100`(0) `over100`(100) `over120`(120) `over140`(140) `over160`(160) `over180`(180)
`over200`(200) `over300`(300), sentinel 0xFFFF. Value: `Info` vtable `+0x38` → u16 at
`Info+0x94` (believed to be `bpmmax`). Range match. Template 1; labels `sefi_bpm_<key>`
(104×20). Arbitrary BPM bands are a pure table change.

### 5.5 EVENT (category 4)

No table. Builder `FUN_180125010` emits up to three buttons with fixed selection indices:
0 → `event_pack` ("Privilege of early play", always), 1 → `event_league` (only if one of the
two event slots `DAT_1806F2ED0[0..1]` is active and lists event id 0x65), 2 →
`event_extra_savior` (always). Predicate `FUN_180124DC0` is a switch on the index:
0 → `Info+0x1AC` bit 15 or bit 27; 1 → `FUN_1800FF310` (league membership); 2 →
`Info+0x1AC` bit 25. Template 1; labels 220×20 in the language IFS
(`sefi_event_pack` / `sefi_event_extra_savior`; `sefi_event_league` in the main IFS).
The `Info+0x1AC` flag word is not a musicdb field (event/unlock data).

### 5.6 LEVEL (category 5)

Table `0x180CF5790`, stride 0x38, 19 rows + sentinel: `+0x00` row, `+0x08` key `"01".."19"`,
`+0x30` u16 level. Equality against `FUN_1800FE490` = `Info` vtable `+0x78(style, difficulty)`
for the chart at the player's current difficulty. Template 5 ⇒ 4 rows of 5 (1.png).
Labels `sefi_level_NN` (32×20); **`sefi_level_00` and `sefi_level_20` ship but are unused**.

### 5.7 FLARE RANK (category 6)

Table `0x180CF5310`, stride 0x68, 11 rows: `+0x00` row, `+0x08` **template**, `+0x10` key,
`+0x38` value, `+0x40` display. `norank`(0, template 1, "NO RANK"), `1`…`9` (template 5,
I…IX), `ex` (10, template 5, "EX"). Equality against `FUN_1800FE600` (flare rank of the
player's record). Layout: NO RANK full row, then 5 + 5 per row. Labels `sefi_flare_<key>`
(`norank` 220×20, others 32×20). Summary prefix `"FLARE "`.

### 5.8 CLEAR RANK (category 7)

Table `0x180CF4C50`, stride 0x60, 17 rows + sentinel: `+0x00` row, `+0x08` key, `+0x30`
range start, `+0x38` display. Rows in display order: `aaa` `aa_p` `aa` `aa_m` `a_p` `a` `a_m`
`b_p` `b` `b_m` `c_p` `c` `c_m` `d_p` `d` `e` `noplay` (starts 0…16), sentinel start 18.

**Selection-index remap** (`.rdata` `0x180370F70`, 17 × `{u64 selection_index, u64 row}`):
`0→AAA 1→AA 2→A 3→B 4→C 5→D 6→E 7→NO PLAY 8→AA+ 9→AA- 10→A+ 11→A- 12→B+ 13→B- 14→C+
15→C- 16→D+`. The builder (`FUN_180126440`) walks rows 0→16 and passes the remapped
selection index to the factory; the predicate maps selection → row → range. This keeps the
**persisted bits backward compatible** with the older 8-rank list (bits 0–7) while the
+/− ranks were appended as bits 8–16 — the pattern to copy when a menu's items must be
reordered without breaking saved selections.

Value `FUN_1800FFBA0`: rank of the player's record for the chart (record `+4`), 17 when no
record (falls into the `noplay` row's `[16,18)` range). Template 3 ⇒ 6 rows of 3. Labels
`sefi_rank_<key>` (32×20; `noplay` 64×20). Summary prefix `"RANK "`.

### 5.9 FLARE SKILL TARGET (category 8)

Table `0x180CF4780`, stride 0x60, 2 rows: `+0x08` key, `+0x30` **u8** value, `+0x38` display.
`target` (1, "Skill Target"), `non_target` (0, "Skill Non Target"). Equality against
`FUN_1800FE800`. Template 1; labels `sefi_fltarget_<key>` (104×20, language IFS).

### 5.10 Dormant categories 9 and 10

The count function still reports 3 entries for ids 9 and 10, the profile still loads/saves
`filtersort/rival_flare_skill` and `filtersort/rival_score_rank` (`u64`), and textures
`sefi_item_vs_rival_flare_rank`, `sefi_item_vs_rival_score_rank`, `sefi_vs_win/lose/draw`
remain in the IFS — but no registration, builder, predicate or category button exists in
the 20250805 or 20260915 filter init (and 20260915's `gamemdx` contains no `vs_rival_*`
strings at all). Two persisted slots are therefore available to a custom category without
touching the protocol.

### 5.11 DIFFICULTY (category 11)

Table `0x180CF5BC0`, stride 0x68, 5 rows: `+0x00` row, `+0x08` template (1), `+0x10` key,
`+0x38` value, `+0x40` display. `beg`(0) `bas`(1) `dif`(2) `exp`(3) `cha`(4). Equality
against the chart's difficulty (`FUN_180125560`; song-level metadata uses the player's
difficulty cursor). Labels `sefi_dif_<key>` (104×20).

### 5.12 CLEAR TYPE (category 12)

Table `0x180CF4840`, stride 0x68, 9 rows + sentinel: `+0x08` template (2), `+0x10` key,
`+0x38` range start, `+0x40` display. `noplay`(0) `failed`(1) `assist`(2) `cleared`(3 → covers
3–5) `l4`(6, "LIFE4") `fc`(7) `gfc`(8) `pfc`(9) `mfc`(10), sentinel 11. Value
`FUN_1800FFAA0`: clear lamp of the player's record (record `+8`, 0 when none). Labels
`sefi_cl_<key>` (64×20). Clear-lamp values 4 and 5 are folded into "CLEARED" (not
identified further).

---

## 6. Labels, textures and summary text

### 6.1 Item labels

At creation every builder builds `sprintf("%s_%s", prefix, key)` into `FilterButton+0xC8`.
`FilterButton::CreateVisual` (`FUN_180134A30`, the existing `filter_panel_builder`
signature) instantiates `filter_switch_base%02d` for the button's template, then applies
texture **`sefi_<prefix>_<key>`** to every `choices_usr` child (libafp `Ordinal_112`).
The check mark (`mark_usr`) is `sefi_switch_mark_on`/`_off` (`FUN_180135840`) and the frame
label follows focus/selection (`in` / `loop_select` / `loop_switch` / …).

Texture homes:

| IFS | Contents |
|---|---|
| `select_music_option_v3.ifs` (language-neutral) | `sefi_version_*`, `sefi_title_*`, `sefi_level_*`, `sefi_rank_*`, `sefi_flare_*`, `sefi_bpm_*`, `sefi_dif_*`, `sefi_cl_*`, `sefi_event_league`, templates, highlight bases, marks |
| `select_music_option_lang_{eng,jpn,kor}_v3.ifs` | `sefi_genre_*`, `sefi_event_pack`, `sefi_event_extra_savior`, `sefi_fltarget_*`, all `sefi_item_*` category labels, button and help text |

New labels must be atlas-cloned into these IFS (the `sefi_version_*` path in
`series_expansion.rs` / `avs_layeredfs::atlas_cloner` is the working precedent); the
`choices_usr` child expects a real atlas UV rect. Unused stock textures that can be
repurposed: `sefi_level_00`, `sefi_level_20`, `sefi_version_gp`, `sefi_vs_win/lose/draw`,
`sefi_item_vs_rival_*`.

### 6.2 Category-button labels

`FilterCategoryButton` visual (`FUN_180133000`) instantiates `filter_item` (232 × 32) and
applies **`sefi_item_<name>`** (160×16, language IFS) to `item_usr`. `<name>` is the literal
passed by the category's button-creator lambda (`music_title`, `version`, `genre`, `bpm`,
`event`, `level`, `flare_rank`, `clear_rank`, `flare_skill_target`, `difficulty`,
`clear_type`). The category's active indicator is `sefi_mark_on/off`, driven by lambda49
(`u64` selection mask from `FUN_1801D58A0`).

### 6.3 Summary (chip) text

`FilterCard` (`FUN_18011B090`) lists each active filter's summary string (or `"0 Filters"`),
plus `"%d Songs"`/`"%d Charts"`. Summaries are **font-rendered text**, not textures. The
common builder `FUN_1801235B0(out, count, is_selected(i), first_label(i), last_label(i))`
walks indices 0..count, merges consecutive selected indices into runs, and emits each run
as `first_label(run_start) … last_label(run_end)` (duplicates collapsed). Tables supply the
labels (`+0x38/+0x60` for TITLE/VERSION, the display string elsewhere); a category prefix is
prepended (`"TITLE "`, `"DDR "`, `"Level "`, `"FLARE "`, `"RANK "`, none for others).
Implication: **selection-index adjacency defines run merging**, so contiguous indices
should be semantically contiguous.

---

## 7. Selection state and persistence

* **Profile**: `ess.dll` `sys_playerdata_load_receiver` (`FUN_180025E10`) reads node
  `filtersort` with children (AVS type → player-buffer offset):
  `title` u64 +0x130, `version` +0x138, `genre` +0x140, `bpm` +0x148, `event` +0x150,
  `level` +0x158, `flare_rank` +0x160, `clear_rank` +0x168, `flare_skill_target` +0x170,
  `rival_flare_skill` +0x178, `rival_score_rank` +0x180, `sort_type` u64 +0x188,
  `order_type` s32 +0x190, `is_quickmode` bool +0x194, `cleartype` u64 +0x198,
  `difficulty` u64 +0x1A0. A missing node aborts the rest of the load.
  Categories 0–10 map to consecutive slots; 12 (`cleartype`) and 11 (`difficulty`) were
  appended later.
* **Load**: `FilterManager` ctor (`FUN_1800FD2F0`) walks categories 0..12 and, for each
  `bit < count(category)`, sets the selection if bit is set (`FUN_1801D5680`).
* **Save**: `FUN_1801D58A0(state, category)` rebuilds the `u64` from the selected set for
  `index < count(category)`.
* **Count**: `FUN_1801D55B0(category)` — jump table: 0→20, 1→9, 2→7, 3→8, 4→3, 5→19,
  6→11, 7→17, 8→2, 9→3, 10→3, 11→5, 12→9, else 0. Identical in 20250805/20260721/20260915.
  `series_expansion` already detours it (signature `filter_entry_count_table`).
* **Consequences**: ≤ 64 selectable items per category can persist; selection indices are
  the persisted bit numbers (renumbering items silently changes users' saved filters);
  indices ≥ count are neither loaded nor saved (they still work for the session). Whether a
  given network server stores unknown bits is unverified (a sibling `bemaniutils`
  checkout has no DDR World `filtersort` handling).
* **Simple mode** flag: `FilterManager+0x1C3`, from player work `+0x18F8`, persisted as
  `is_quickmode`.

---

## 8. Interaction behaviours

| Behaviour | Mechanism |
|---|---|
| Simple ("Change Selection Method: Simple") | lambda92 clears **all** categories before the toggle; lambda94 posts event 0x11 (close + apply) after it. Help/button textures swap to `*_simple` (`FUN_180138D20`). Net effect: one condition at a time, closes immediately. |
| Normal | Toggles accumulate; OR within category, AND across. |
| Group tabs (VERSION) | Replace the category selection with a contiguous entry-index range. |
| Select Range and Close | FilterPanel input (`FUN_180136050`) applies one ON/OFF state (from the anchor button) to every button between anchor and cursor in GridPanel index order, then closes. |
| Per-category Reset | Last navigation target of the item grid (full-container size); highlights `switch_usr/button_usr`. |
| Global "Deselect all and close" / Back | FilterPanel-level buttons created in `FUN_180137370` (lambdas 23–26 of `_anon_9E0B35AD`). |
| Filtering Results | Recomputed on every toggle via `FUN_180137230` → event 6. |

---

## 9. Implications for a fully configurable filter system

This section maps every dimension a user-defined filter menu would need onto the game
mechanism that controls it. It is intentionally not a schema.

### 9.1 The dimensions

| Dimension | Stock mechanism | Freely configurable? |
|---|---|---|
| Which categories exist, their ids | Registrations in `FUN_18011F600`; ids 0–12 | New ids need registration + count/persistence handling; ids 9/10 are free persisted slots |
| Left-column order | `int[11]` in `FUN_18011F530` | Yes (hook), but only 11 rows fit without scroll |
| Category button label | `sefi_item_<name>` literal per creator lambda | Texture yes; name via hook |
| Items and their order | Builder creation order | Yes (builder hook or table+loop patch) |
| Item width / items-per-row | Template 1–5 per button → 220/108/72/54/42 px in 216 px flow | Any mix of the 5; other widths need a new `filter_switch_baseNN` AFP |
| Row breaks | Overflow, full-width items, `FilterHeader` | Yes |
| Row alignment / gaps | GridPanel `+0xC4`, `+0xD0/+0xD8` (0 in stock) | Yes, per menu (container is shared, so reset per open) |
| Rows visible | 10 (266 px) | More needs scroll + clip (service exists for VERSION) |
| Item label | Texture `sefi_<prefix>_<key>` on `choices_usr` | Yes, atlas-cloned into the right IFS |
| Item matching rule | Per-category predicate over one extractor (range / equality / bit / switch) | Any rule via predicate hook; tables allow ranges/values |
| Value source | Title class, mapped series, property bits, bpm, event flags, level, flare, rank, lamp, skill target, difficulty | Any `music::Info` / `ChartMetadata` / record field via a custom extractor |
| Song vs chart level | `Filter+0x00` flag chosen at registration | Per category |
| Summary chip text | Summary lambda + table label strings + prefix | Yes (strings, font-rendered) |
| Auto-sort coupling | `Filter+0x48..0xAF` sorter copy | Per category, reuse existing sorter ids |
| Persistence | `u64` per category, `count(category)` | ≤ 64 items; index = bit |
| Selection macros | VERSION group tabs | Pattern reusable for any "select a range" button |
| Simple/Normal | Global flag | Unchanged |

### 9.2 Two implementation strategies

**A. Table extension (what `series_expansion` does today).** Allocate a bigger table near
the module, then patch every site that hardcodes the stock table base or count: the
builder's table pointer and loop bound, the predicate's table LEA, the summary builder's
count/table, and detour `count(category)`. Per category that is 4–5 sites, each needing an
AOB plus a shape check of the bytes after it. Keeps the game's lambdas, factory, textures and
persistence path intact, but only within the category's stock shape (same extractor, same
match rule, same template source) and the SSO string limit (≤ 15 bytes per key).

**B. Builder + predicate replacement.** Detour the category's builder leaf
(`FUN_180123B90` & co.) and predicate leaf (`FUN_1801239D0` & co.) and summary leaf, and drive
them from a mod-owned model:

* builder: for each configured item call the provided factory
  (`std::function<FilterButton*(int)>`, `vtable+8`) with the item's selection index, call
  `SetTemplate`, write the label key into `FilterButton+0xC8` (game-CRT `std::string`),
  and push spacers/headers where a row break is wanted;
* predicate: read the category's selected list from `FilterManager+0x378`, evaluate the
  configured rule per selected index against any extractor;
* summary: produce the chip string;
* count: report the configured item count (≤ 64).

This decouples layout, order, width and matching from the stock tables entirely and is
the route to "ultimate flexibility". New categories additionally need a `Filter` record in
`FilterManager+0x358`, a UI record in `FilterPanel+0x1F8`, and a slot in the left-column
order array.

### 9.3 Constraints and gotchas

* **One detour per target** (AGENTS.md): `series_expansion` already patches VERSION sites
  and detours the count function; `series_filter_scroll` detours `FilterButton::CreateVisual`,
  BM2D `set_position` and the FilterButton dtor. A general filter service would need to
  own these and offer them to the existing consumers.
* **`FilterButton+0xF0` is the template index, not a category.** `series_filter_scroll`
  tests `== 2` to find VERSION buttons; CLEAR TYPE buttons (per-entry template 2) also match.
  It is benign today only because opening another category destroys the old buttons
  (dtor hook clears state) before new ones are built.
* **Static tables are built once**, guarded by `DAT_181237450` bits; a table swap must
  happen before first use or replace every pointer consumer.
* **Allocators**: table strings and `FilterButton+0xC8` are game-CRT `std::string`s; SSO
  (≤ 15 chars) avoids heap ownership questions entirely.
* **Selection indices are persisted bits** — keep them stable across config edits (the
  CLEAR RANK remap shows the game's own approach), and ≤ 64.
* **Range tables need a sentinel** row; predicates read `table[i+1].start`.
* **New widths** require a new `filter_switch_baseNN` AFP in the IFS (cloned from an
  existing one with different header dimensions/geometry) plus `sefi_select_baseNN` /
  `sefi_on_baseNN` highlight textures; the size cache and `%02d` name lookup already accept
  any index. The repo's AFP tooling (`src/core/afp.rs`, `src/core/ap2/`, `afp_patcher`) is
  the likely route; untested.
* **Visible rows**: 10. Denser grids (e.g. TITLE A–Z individually at 5 per row = 6 rows,
  plus 2 kana rows and an "other" row = 9 rows) fit; a one-per-series VERSION menu at 2 per
  row (≈ 20 entries + tab row) does not without scrolling or 3 per row.
* **BM2D pool** is 1024 clips per scene; a few hundred filter items is fine, thousands is not.
* **No clipping** for items; any scroll solution must mask rows itself.
* **Build-dependent offsets**: FilterManager fields moved by +0x20 between 20250805 and
  20260721 ([section 11](#11-cross-version-notes)); derive them (e.g. from the LEA in the
  filter-init prologue) rather than hardcoding.
* **EVENT's league item** is conditional on live event data; a config-driven EVENT menu
  must preserve that gate or accept an always-present item.

---

## 10. Struct layouts

### 10.1 `sequence::Component` (base of every UI element, ctor `FUN_18003D850`)

| Off | Type | Field |
|---|---|---|
| +0x00 | ptr | vtable (slot 1 dtor, 3 layout hook, 4 input, 6/7 update) |
| +0x08 | — | ITag data (FNV-hashed tags such as `Reset`, `System`, `Active`) |
| +0x28 | ptr | secondary vtable (focus/press interface; slot 0 = is-focusable) |
| +0x30 | u8 | cursor-on flag (read as u8, see `afp_system.md` pitfalls) |
| +0x31 | u8 | secondary state |
| +0x38 | std::string | name |
| +0x60 | ptr | parent Component (GridPanel) |
| +0x68 / +0x70 / +0x78 | ptr×3 | children vector (begin/end/cap) |
| +0x88 / +0x90 / +0x98 | f64×3 | position (layout-owned) |
| +0xA0 / +0xA8 / +0xB0 | f64×3 | size |
| +0xB8 | u8 | participates in layout/navigation (default 1) |

### 10.2 `sequence::GridPanel` (0x240, ctor `FUN_180048CB0`)

| Off | Type | Field |
|---|---|---|
| +0xC0 | i32 | orientation (0 = rows along X) |
| +0xC4 | i32 | main-axis line alignment (×0.5: 0 start, 1 centre, 2 end) |
| +0xC8 | i32 | cross-axis alignment (same encoding; inferred) |
| +0xD0 / +0xD8 / +0xE0 | f64×3 | gap |
| +0xE8 / +0xF0 / +0xF8 | f64×3 | follow-scroll anchor |
| +0x100 | u8 | follow-cursor scroll enabled |
| +0x120 | f64 | scroll easing factor (0.75) |
| +0x128 | u64 | unknown (ctor 2, filter sets 1) |
| +0x12C | i32 | wrap-around navigation |
| +0x130 | u8 | circular placement (carousel mode) |
| +0x134 | i32 | extra spacing on forced breaks |
| +0x138 / +0x140 | u64 | main / cross axis selectors (derived from +0xC0) |
| +0x150 / +0x158 / +0x160 | f64×3 | current scroll offset |
| +0x168 / +0x16C | i32 | cursor index / previous |
| +0x170 | u8 | relayout-dirty |
| +0x178 / +0x198 / +0x1B8 / +0x1D8 | std::function×4 | navigation handlers |
| +0x1F8 | i32 | remembered main-axis coordinate for up/down |
| +0x200 | ptr | focused child |
| +0x208 / +0x210 | f64 | content extent (main / cross) |
| +0x220..+0x230 | f64×3 | scroll ratio (scrollbar) |
| +0x238 | i32 | forced line count (0 = off) |

### 10.3 `FilterButton` (0x1D0, ctor `FUN_180134160`) — identical 20250805…20260915

| Off | Type | Field |
|---|---|---|
| +0x00..+0xBF | — | Component |
| +0xC0 | u8 | internal flag |
| +0xC8 | std::string | label key `<prefix>_<key>` → texture `sefi_<label>` |
| +0xF0 | i32 | **template index** (1–5) |
| +0xF8 | std::function<void(bool)> | on-press (lambda93: toggle selection) |
| +0x118 | std::function<void(bool)> | pre-press (lambda92: Simple-mode clear) |
| +0x138 | std::function<void(bool)> | post-press (lambda94: Simple-mode close) |
| +0x158 | std::function<bool()> | is-selected (lambda95) |
| +0x178 | shared_ptr<BM2D movie> | `filter_switch_baseNN` instance (layer id at `[ptr+8]`) |
| +0x188 | ptr | FilterPanel `+0xC0` interface (source of the switch-area scale/colour) |
| +0x190 / +0x1B0 | vectors | per-frame callback lists |

### 10.4 `ChartMetadata` (0x90, ctors `FUN_1801A6BF0` song / `FUN_1801A6DA0` chart)

| Off | Type | Field |
|---|---|---|
| +0x00 | shared_ptr<music::InfoCommon> | song (dynamic-cast to `music::Info`) |
| +0x10 | shared_ptr<ChartMetadata> | parent song metadata (chart-level only) |
| +0x20 | 5 × 0x10 | per-difficulty data |
| +0x70 | i32 | difficulty (0–4; 5 = song-level) |
| +0x74..+0x84 | i32×5 | per-difficulty mapping (init 5) |
| +0x88 | i32 | title class (0–36) |

### 10.5 `Filter` (0xB0, in `FilterManager+0x358` map)

| Off | Type | Field |
|---|---|---|
| +0x00 | u8 | chart-level |
| +0x08 | std::function<bool(shared_ptr<ChartMetadata>)> | predicate |
| +0x28 | std::function<std::string()> | summary |
| +0x48 / +0x4C | i32×2 | sorter parameters |
| +0x50 / +0x70 / +0x90 | std::function×3 | sorter functions (+0x68/+0x88 impl non-null ⇒ "has sorter") |

### 10.6 FilterManager (global `DAT_1806F2D50`, 20260915 offsets)

| Off | Field |
|---|---|
| +0x04 + side×4 | per-side current difficulty index used by chart predicates |
| +0xC0 | event sink (events 6 = selection changed, 0x11 = close) |
| +0x150 | active side |
| +0x158 + side×8 | cached player record store |
| +0x1B0 | focused song (`shared_ptr`; its mcode is republished on every recount) |
| +0x1C1 / +0x1C2 | dirty flags |
| +0x1C3 | Simple mode |
| +0x2A8 / +0x2C8 | override filter / present flag |
| +0x358 | `std::map<int, Filter>` |
| +0x378 | selection state `std::map<int, std::list<int>>` |
| +0x3D0 | explicit sort type |
| +0x3D8 | `std::map<int, Sorter>` |
| +0x3F8 | default sort id |

### 10.7 FilterPanel (partial)

| Off | Field |
|---|---|
| +0xC0 | secondary interface (setup/AFP methods; `FUN_180137370` runs on it) |
| +0x140 | `filter_root` movie (from +0xC0 base: +0x80) |
| +0x1F8 | `std::map<int, CategoryUI>` (button-creator `std::function<void()>` at node value +0x18) |
| +0x218 | range-select anchor (−1 = none) |
| +0x220 | category-list GridPanel (11 category buttons + Back + Deselect-all targets) |
| +0x228 | item GridPanel |
| +0x230 | u8: which grid has focus (1 = category list, 0 = item grid) |

---

## 11. Cross-version notes

The whole system is structurally identical in 20250805, 20260721 and 20260915: the same
11 registrations, table contents, template choices, count jump table, FilterButton layout
and GridPanel algorithm. Only addresses and the FilterManager field block move.

| Anchor | 20250805 | 20260721 | 20260915 |
|---|---|---|---|
| filter init (xref `"line_a"`) | `0x180112830` | `0x18011F900` | `0x18011F600` |
| `FilterButton::SetTemplate` | `0x180127C50` | `0x180134990` | `0x1801345B0` |
| `FilterButton::CreateVisual` | `0x1801280D0` | `0x180134E10` | `0x180134A30` |
| category button visual (`"sefi_item_%s"`) | `0x180126130` | `0x1801333E0` | `0x180133000` |
| TITLE summary (`"TITLE "`) | `0x180116C90` | `0x180123D60` | `0x180123A60` |
| TITLE button creator (`"music_title"`) | `0x180116F90` | `0x180124060` | `0x180123D60` |
| FilterPanel setup (`"filter_root"`) | `0x18012ACB0` | `0x180137750` | `0x180137370` |
| FilterCard (`"0 Filters"`) | `0x18010E540` | `0x18011B3A0` | `0x18011B090` |
| GridPanel vtable / Update | `0x180340628` / `0x18004ACB0` | `0x18035FC78` / `0x18004AE20` | `0x18035FCB8` / `0x18004BD90` |
| count fn (`filter_entry_count_table`) | `0x1801BF3A0` | `0x1801D5750` | `0x1801D55B0` |
| FilterManager: Filter map / selection / sorter map | +0x338 / +0x358 / +0x3B8 | +0x358 / +0x378 / +0x3D8 | +0x358 / +0x378 / +0x3D8 |

Derivation hint: the filter-init prologue loads the selection-state address as
`LEA reg, [manager + 0x378]` (20260721/20260915) / `+0x358` (20250805); the string xrefs
above are unique per build.

---

## 12. Corrections to earlier notes

* `filter_switch_base%02d`'s number is the **cell template (column width)**, not a
  category index ([filter_ui_extension.md](filter_ui_extension.md),
  [filter_scroll_research.md](filter_scroll_research.md)). Consequently
  `FilterButton+0xF0` "category_index (2=version, 3=group tabs)" is the template index, and
  the `filter_button_panel_config` signature is `FilterButton::SetTemplate(this, template)`.
* `sefi_item_%s` labels **category buttons** (`filter_item` template); item labels come from
  `sefi_%s` applied to `"<prefix>_<key>"` on `choices_usr` (hence `sefi_version_world`).
* The "5 columns"/"2 columns" grid parameters are not stored anywhere: they emerge from the
  216-px flow layout and the template widths.
* The `filter_panel_builder` signature resolves to `FilterButton::CreateVisual`, called per
  button, not a per-category panel builder.

---

## 13. Address reference (20260915)

### Functions

| Address | Role |
|---|---|
| `0x18010B2E0` | song-select setup (creates FilterPanel via `0x1801102F0`) |
| `0x180135BA0` | FilterPanel ctor |
| `0x18011F600` | filter init: tables + Filter/UI registration |
| `0x180137370` | FilterPanel setup (AFP loads, GridPanels, categories) |
| `0x180138D20` | GridPanel sizing from `filter_root` placeholders; Simple/Normal textures |
| `0x18011F530` | category-button creation in fixed order |
| `0x180123240` | create one FilterCategoryButton |
| `0x180127010` | open category: clear grid, run builder, append Reset |
| `0x180127C50` | FilterButton factory (lambda88) |
| `0x180124000` | VERSION group-tab factory |
| `0x180127810` | VERSION group-tab press |
| `0x180134160` / `0x1801345B0` / `0x180134A30` | FilterButton ctor / SetTemplate / CreateVisual |
| `0x1801355C0` / `0x180135840` | FilterButton per-frame / mark+frame-label update |
| `0x1801340C0` / `0x180133FC0` | FilterHeader layout hook / update |
| `0x180133000` | FilterCategoryButton visual |
| `0x18013A190` / `0x18013A250` | template size cache lookup / init |
| `0x180048CB0` | GridPanel ctor |
| `0x180049590` | GridPanel clear |
| `0x18004BD90` / `0x18004BAD0` / `0x18004AD10` / `0x18004B6A0` / `0x18004BD30` | GridPanel update / scroll+layout / flow layout / line align / axis setup |
| `0x18004A970` / `0x18004A3A0` / `0x180049010` | GridPanel row-neighbour / step / input |
| `0x180123B90` `0x180124220` `0x180124810` `0x180124BF0` `0x180125010` `0x180125740` `0x180125BB0` `0x180125FD0` `0x180126440` `0x180126900` `0x180126D70` | builders: title, version, genre, bpm, event, difficulty, level, flare, rank, clear, target |
| `0x1801239D0` `0x180123E40` `0x180124600` `0x1801249E0` `0x180124DC0` `0x180125560` `0x1801259F0` `0x180125E20` `0x180126270` `0x180126740` `0x180126BB0` | predicates, same order |
| `0x1800FFCB0` `0x1800FE490` `0x1800FE600` `0x1800FFBA0` `0x1800FFAA0` `0x1800FE800` `0x1800FF310` | extractors: series, level, flare, rank, lamp, skill target, league |
| `0x1801A6BF0` / `0x1801A6DA0` | ChartMetadata ctors (song / chart) |
| `0x1801A7710` / `0x1801A7840` | title normaliser / classifier |
| `0x1801235B0` / `0x180123A60` | summary run builder / TITLE summary |
| `0x1801D5680` `0x1801D5740` `0x1801D57B0` `0x1801D58A0` `0x1801D59E0` `0x1801D55B0` | selection set / clear cat / clear all / build u64 / lookup / count |
| `0x1800FD2F0` | FilterManager ctor + profile load |
| `0x180100AD0` / `0x180101180` / `0x180100C00` | active filters / AND predicate / sorter choice |
| `0x18011B090` | FilterCard |
| `0x180137230` | selection-changed notify |
| `0x180136050` | FilterPanel input (range select) |
| `0x1801309F0` `0x180131EA0` `0x180131F50` `0x180132010` | lambda92/93/94/95 bodies |

### Data

| Address | Contents |
|---|---|
| `0x1806F2D50` | FilterManager* |
| `0x181237450` | table-init guard bits (1 title, 2 version, 4 groups, 8 genre, 0x10 bpm, 0x20 difficulty, 0x40 level, 0x80 flare, 0x100 rank, 0x200 clear, 0x400 target) |
| `0x180CF67C0` / `0x180CF6270` / `0x180CF3E40` | TITLE / VERSION / VERSION-group tables |
| `0x180CF5FD0` / `0x180CF5DD0` / `0x180CF5BC0` / `0x180CF5790` | GENRE / BPM / DIFFICULTY / LEVEL tables |
| `0x180CF5310` / `0x180CF4C50` / `0x180CF4840` / `0x180CF4780` | FLARE / RANK / CLEAR / TARGET tables |
| `0x180370F70` | CLEAR RANK selection↔row remap (`.rdata`) |
| `0x180380090` | 10 katakana line boundaries (SJIS) |
| `0x180CF34A0` | template size cache root |
| `0x18035FCB8` | GridPanel vtable |

### Assets

| Path | Notes |
|---|---|
| `select_music_option_v3.ifs` `afp/filter_root` | layout; `switch_usr/dummy_choice_usr` 216×266, `dummy_conditions_usr` 232×352 |
| `afp/filter_switch_base01..05` | item templates (220/108/72/54/42 × 26) |
| `afp/filter_item` | category button (232×32) |
| `afp/filter_switch`, `filter_switch_ef01..05`, `filter_item_base_ef`, `filter_button*` | panel frame and effects |

---

## 14. Open questions

1. Is `ChartMetadata+0x88` consumed anywhere else (e.g. the title sorter's group headers)?
   Changing the classifier's numbering must not break it.
2. Exact semantics of clear-lamp values 4 and 5 (folded into CLEARED) and of
   `Info+0x1AC` event bits.
3. Confirm `Info+0x94` is `bpmmax` against a song whose `bpmmin` ≠ `bpmmax`.
4. `GridPanel+0x128` and `+0xC8` meanings (inferred, not exercised).
5. Whether network servers persist bits beyond the stock counts, and whether they
   round-trip `rival_flare_skill` / `rival_score_rank` for a repurposed category.
6. Whether a cloned `filter_switch_base06` loads by index without an `afplist.xml` entry.
7. Precise up/down column choice (`+0x1F8` usage in `FUN_18004B840`) — only needed if a
   custom layout wants non-geometric navigation.

---

## 15. Addenda (2026-09-27)

Verified while building series_expansion's enhanced VERSION layout on all five supported
builds (20250805, 20260224, 20260721, 20260825, 20260915); the builder, tab factory, press body,
set-one, clear-category, SetTemplate, CreateVisual and the FilterButton dtor are
instruction-identical across them (only rel32/disp32 values differ).

* **Selection sets** are `std::map<int, stdext::hash_set<int>>`, not `std::list<int>` (§4.2).
  `FUN_1801D5680(state, cat, idx, on)` inserts with dedupe or erases.
* **FilterManager −0x20 layout** applies to 20260224 as well as 20250805 (§11).
* **VERSION builder capture** (`lambda10`, 0x50 bytes after the vtable): +0x00 selection state
  (`FilterManager+0x378`), +0x08 category, +0x10 FilterPanel, +0x18 prefix `std::string`
  ("version"), +0x40 FilterPanel again, +0x48 entry template. The builder owns its by-value
  factory `std::function` (impl at +0x18; vtable slot 1 invoke, slot 3 delete) and is reached
  only through its `_Do_call` thunk.
* **Group tabs:** the tab factory hardcodes the stock group table into each `lambda60` capture;
  the press body (`FUN_180127810`) inlines the set-one insert and reaches notify
  (`FUN_180137230`) by a tail `JMP`. Range select also fires the press of tabs inside the range.
* **Predicate** (`FUN_180123E40`): the second compare's disp32 (`0xB8`) sits at a fixed
  `version_predicate_lea + 0x34` on every build, and `+0x34` of a VERSION entry is never read or
  written anywhere — repointing it gives each row its own end. Compares are signed.
* **Chip summary** loop runs `0..=count` (the sentinel index is probed); runs are merged by
  selection-index adjacency and printed as `first～last` (Shift-JIS fullwidth tilde) joined by
  `", "`.
* **Persistence** load uses `SHL RDX,CL` and save `ROL RDI,1` + `ADD`: indices ≥ 64 alias.
* **CreateVisual is lazy**: it runs on a later Component tick only while the button's layout rect
  intersects the virtual screen band (y < 864 in 1280×720), and can re-run with a new layer id
  (§3.8's scrolling note: scrolling BM2D layers never builds an off-band row).
* **Textures in `select_music_option_v3.ifs` are stored per image** (`tex/md5(image)`), not per
  atlas (§6.1): a new name needs a texturelist `<image>` entry plus a per-image blob.
* **Thumbnail loop** (`FUN_18003C270`): `CMP RSI,imm8` sign-extends before an unsigned `JBE`;
  any bound ≥ 0x80 never terminates (pool exhaustion, then negative-index writes). Only
  `jacket_thumbnails_ja_0..21.arc` ship.


---

## 16. Addenda (2026-09-27): per-letter MUSIC TITLE (`improved_song_title_sorting`)

Verified on all five supported builds (20250805, 20260224, 20260721, 20260825, 20260915);
the builder, predicate, chip summary and the VERSION builder's FilterHeader block are
instruction-identical across them (only rel32/disp32 and the predicate's FilterManager
selection offset `+0x378`/`+0x358` differ). Sites: `derive_title_filter` in
`src/core/signatures.rs`.

* **Builder** (`FUN_180123B90`): the shared filter-builder prologue (identical in the GENRE,
  BPM, DIFFICULTY, LEVEL and CLEAR TYPE builders), told apart by its loop tail
  `CMP RSI,0xAA0` (20 × 0x88) at entry+0x168. Capture `+0x00` template, `+0x08` prefix
  string. Same by-value factory contract as VERSION (impl at +0x18, slot 1 invoke, slot 3
  delete). The factory pushes the button onto the item grid's `children` (`grid+0x68`,
  `FUN_180046CC0` push_back) and sets `button+0x60` = grid.
* **Title table readers** are exactly the builder (keys), the predicate (`LEA R8` at
  +0x3A: `start(i) <= class < start(i+1)`, stride 0x88, +0x30/+0xB8) and the chip summary
  (`FUN_180123A60`: one `LEA RCX,[table]` at +0x6F feeding both label lambdas, which read
  `+0x38` first / `+0x60` last via `FUN_180127770` / `FUN_1801277C0`; `MOV EDX,20` imm32 at
  +0xD4). The summary is the `filter_label_builder_count` shape with count 20
  (`title_label_builder_count`). Stock kana summary labels: あ/お か/こ さ/そ た/と な/の
  は/ほ ま/も や/よ ら/ろ わ/ん (Shift-JIS, first/last).
* **Count function** (`FUN_1801D55B0`) has exactly two callers: the FilterManager ctor
  (profile load) and the mask builder `FUN_1801D58A0` (save + the category's active
  indicator). Its detour now lives in `services::filter_entry_count` (per-category
  overrides; series_expansion → VERSION, improved_song_title_sorting → MUSIC TITLE).
* **Row break recreated from outside the VERSION builder.** The only FilterHeader
  construction (`filter_header_alloc`, VERSION builder +0x16D): `operator new(0xF8)`
  (`FUN_1802792A4`), `Component::Component` (`FUN_18003D850`), primary / `+0x28` vtables, an
  empty SSO string at +0xC0, zero qwords at +0xE8 / +0xF0, then an inlined push_back onto
  `FilterPanel+0x228`'s children (grow-by-one `FUN_1800D3140`) and `+0x60` = grid. Replaying
  that sequence from a builder detour (grid = the last factory button's `+0x60`) gives any
  menu a forced line break; the grid's clear frees it through the deleting destructor.
* **A cell's layout box can't be widened to force a break**: `FUN_1801355C0` positions the
  button's movie at `pos + size × 0.5` (`DAT_18038FC18` = 0.5), i.e. centred in the box, so a
  wider box moves the visual.
* **Template 2 outside VERSION**: the MUSIC TITLE OTHER cell uses template 2, so legacy
  `series_filter_scroll` (`Tracking::Template2`) records it when the menu opens — as it
  already does for the nine CLEAR TYPE cells. Benign for the same reason (§9.3): the count
  never reaches the VERSION total and the FilterButton dtor clears the tracking.
