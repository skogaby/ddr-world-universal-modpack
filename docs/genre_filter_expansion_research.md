# Genre Filter Expansion Research (preliminary)

Reverse-engineering notes for adding **custom entries to the GENRE filter** on DDR
World's song-select Filter screen — the screen reached via FILTER → GENRE, which
lets the player filter the song list by one or more genres (POP MUSIC, VIRTUAL
POP, VARIETY, …). This is a sibling feature to the Folder Expansion mod but a
**separate subsystem**; this doc captures what's been found so far as a springboard
for a future feature.

**Game binary**: `gamemdx.dll` (MDX-003_20260526)
**Ghidra base**: `0x180000000`
**Status**: preliminary — mechanism confirmed; UI-injection layout/strategy not yet implemented.
**Addresses are version-specific.** Like the rest of the codebase, a real mod must
derive these from AOB signatures / xrefs, not hardcode the 20260526 values below.

---

## TL;DR

- **Folders (carousel) and genres (filter) are two independent subsystems**, but
  they resolve song membership through the **same mechanism**: the `<property>`
  field in `musicdb.xml`, treated as a **32-bit bitmask**, where bit *N* means "this
  song belongs to folder/genre with bit index *N*."
- The Folder Expansion mod's `folder_register` hook **only** populates the carousel.
  It does nothing for the GENRE filter, which is why custom folders don't appear
  there — not even as empty rows.
- The GENRE filter is built from a **static, hardcoded entry table** in the filter
  builder function (`FUN_18011eec0` on 20260526), not derived from FolderProperty
  objects.
- To add a custom genre filter entry you need to inject (1) a filter **entry row**,
  (2) a **predicate** that bit-tests your `bit_index` against the song property, and
  (3) a `sefi_item_{key}` **label texture** via LayeredFS. (1) and (3) are UI
  plumbing; (2) reuses the existing engine bit-test — see below.

See also: [folder_system_research.md](folder_system_research.md),
[filter_ui_extension.md](filter_ui_extension.md),
[series_filter_internals.md](series_filter_internals.md) (the VERSION-filter analog,
already implemented as the Series Expansion mod).

---

## Confirmed: the shared property-bitmask membership test

Both subsystems funnel song membership through one function (the
`property_bitmask_test` from the folder doc):

### `FUN_180144560` — per-song property bit test (20260526)

```c
undefined8 FUN_180144560(int *param_1, undefined8 *param_2)
{
  // param_1 -> functor; *param_1 = bit_index
  // param_2 -> song object (RTTI-cast music::InfoCommon -> music::Info)
  ...
  uVar4 = *(uint *)(song + 0x178);          // property (u32)
  if (uVar4 == 0) uVar4 = *(uint *)(song + 0x174);   // fallback offset
  if (((*param_1 == 8) && ((uVar4 & 0x40) != 0)) ||         // legacy bit-8 quirk
      (uVar3 = 1 << ((byte)*param_1 & 0x1f), (uVar3 & uVar4) == uVar3))
    return 1;   // song is a member of bit index *param_1
  return 0;
}
```

Key facts this establishes:

1. **`property` is read as a `u32`** from `[music::Info + 0x178]` (fallback `+0x174`).
2. Membership is `(property >> bit_index) & 1` — implemented as
   `(1 << (bit_index & 0x1f)) & property`. The `& 0x1f` shift mask means **bits
   0–31 are all usable**; no 10-folder limit.
3. **Legacy quirk:** bit index **8** additionally matches if `property & 0x40`
   (bit 6) is set. Irrelevant for custom bits ≥ 10, but don't reuse bit 8.

This is the same test that fills the per-folder song-count arrays the carousel's
has-songs predicate reads (`FUN_180145f10` indexes counts at `[…+0xd0]` /
`[…+0xf8]` by `bit_index`), and it is the natural predicate body a custom genre
filter entry should reuse.

---

## The GENRE filter is a separate, static subsystem

### Filter builder — `FUN_18011eec0` (20260526)

A single large function builds **all** filter categories (LEVEL, DIFFICULTY,
VERSION, MUSIC TITLE, CLEAR RANK/TYPE, FLARE RANK, FLARE SKILL TARGET, BPM, GENRE,
EVENT, …). Characteristics:

- Each category's entry list is a **static table** populated once and guarded by an
  init bitflag in `DAT_181236440` (the GENRE category is the `& 8` block), with an
  `atexit` cleanup registered. So the tables are build-once globals, **not** derived
  from the live FolderProperty list.
- Each entry is constructed with `FUN_180003860(&dst, "literal", len)` (std::string
  assign) for its key and display strings, plus small integer fields.
- The category is finalized by wrapping a predicate (a `std::tr1` callable, e.g.
  `…selectmusic::_anon_4A1D5FAC::<lambda12>` for GENRE) and pushing the built
  category into the filter object via `FUN_180127e00(param_1 + 0x3f, …)`.

**Implication:** adding a folder via `folder_register` cannot affect this screen.
A genre-filter feature must hook `FUN_18011eec0` (or, more surgically, the GENRE
category's entry-append path) and inject an extra entry + predicate.

### Extracted GENRE entry table (20260526)

The `& 8` init block builds these entries. The per-entry **bit value** is the
property **bit index** (confirmed: it matches the folder `bit_index` for the four
overlapping genre folders). A second sequential integer per entry (1, 2, 3, …)
appears to be a display/order index — *meaning not yet confirmed.*

| key                | display              | property bit |
|--------------------|----------------------|--------------|
| `popmusic`         | POP MUSIC            | 2            |
| `virtualpop`       | VIRTUAL POP          | 3            |
| `animegame`        | ANIME & GAME         | 4            |
| `touhou`           | TOUHOU…              | 5            |
| `variety`          | VARIETY              | 8            |
| `hinabitabanmeshi` | ひなビタ♪&バンめし♪  | 7            |
| `audition`         | AUDITION             | 9            |

Notes:
- The GENRE filter covers property bits **2,3,4,5,7,8,9** — it intentionally omits
  bits **0** (`firststep`) and **1** (`musicgamers`) that *do* exist as carousel
  folders, and omits bit **6**. So the carousel set and the filter set are curated
  independently (matches the in-game screenshot: no FIRST STEP / FOR MUSIC GAMERS
  rows in the GENRE filter).
- This confirms a `property` bit can have: a folder only, a filter entry only, both,
  or neither. A future feature declares the **filter entry** for a bit; keeping it in
  sync with a carousel folder for the same bit is a mod-side concern, not enforced by
  the game.

> **Verify before relying on it:** the bit values above were read from the
> decompiled `FUN_18011eec0` init block; double-check the `variety`/`hinabita`/
> `audition` mapping (the order in the source interleaves key/display/value triples)
> against the folder doc's property table and a live test before trusting the exact
> bit each maps to.

### Entry label textures

Per [filter_ui_extension.md](filter_ui_extension.md), each filter entry renders its
label by setting texture `sefi_item_{key}` on the `"item_usr"` child of a
`"filter_item"` MovieClip. A custom entry with key `dogs` needs `sefi_item_dogs`
supplied via LayeredFS / a custom ARC — exactly the pattern the Series Expansion mod
uses for the VERSION filter.

---

## Proposed injection strategy (to be validated)

Mirror the Series Expansion approach, retargeted at the GENRE category:

1. **Entry row** — hook the GENRE category build inside `FUN_18011eec0` (or its
   append helper) and add a `{key, display, bit_value}` entry for each custom genre.
   *Open:* exact entry struct stride/layout and the category-append signature.
2. **Predicate** — give the custom entry a predicate that bit-tests the configured
   `bit_index` against the song property. The engine logic already exists in
   `FUN_180144560`; the predicate just needs to carry the bit index and call/inline
   that test. *Open:* how the GENRE lambda stores its captured value and where its
   function body lives (`_anon_4A1D5FAC::<lambda12>`).
3. **Texture** — supply `sefi_item_{key}` via LayeredFS, same as Series Expansion's
   `sefi_version_{key}`.

The filtering-results count and the actual song-list filtering both derive from the
predicate, so a correct predicate makes the "Filtering Results: N" counter and the
filtered list work for free.

---

## Open RE tasks (next steps)

1. **GENRE entry struct layout** — pin the per-entry stride and field offsets
   (key string, display string, bit value, the sequential index field's purpose).
2. **Category-append entry point** — find the narrow function that pushes a single
   entry into the GENRE category vector (the `FUN_180127e00(param_1 + 0x3f, …)` /
   per-entry push helpers), to hook surgically instead of rewriting the whole table.
3. **Predicate lambda** — locate `…_anon_4A1D5FAC::<lambda12>`'s function and how it
   captures/reads the bit value, so a custom predicate can be synthesized (donor
   vtable, à la custom_options row synthesis, or reuse `FUN_180144560`).
4. **Multi-select / range semantics** — the screen supports Simple/Normal modes and
   "Select Range and Close"; confirm a custom entry behaves under range selection.
5. **AOB signatures** — author stable signatures for the builder + append path so
   the feature survives version updates (the addresses below are 20260526-only).

---

## Addresses quick reference (20260526)

| Symbol | Address | Description |
|--------|---------|-------------|
| `filter_builder` | `0x18011eec0` | Builds all filter categories from static tables; GENRE block guarded by `DAT_181236440 & 8` |
| `property_bitmask_test` | `0x180144560` | Per-song membership test: `(property >> bit_index) & 1`; reads property at song `+0x178` (fallback `+0x174`) |
| `folder_has_songs_pred` | `0x180145f10` | Folder has-songs predicate (carousel side); indexes count arrays by `bit_index` |
| `folder_functor_ctor` | `0x1801440b0` | Folder property-bit functor ctor (stores bit_index at `+0x8`, vtable `0x180373150`) |
| `folder_filter_functor_ctor` | `0x180144060` | Folder filter functor ctor |
| `filter_init_flags` | `0x181236440` | Bitfield: per-category "table already built" guards (GENRE = `& 8`) |
| song `property` offset | `+0x178` (fallback `+0x174`) | u32 bitmask on `music::Info` |

Cross-reference the folder-side addresses in
[folder_system_research.md](folder_system_research.md) (note: that doc's primary
addresses are 20260324; the folder functions were re-anchored in 20260526 during the
Dan Ranking investigation — `folder_init` = `FUN_1801410f0`, `folder_register` =
`0x180143e50`).
