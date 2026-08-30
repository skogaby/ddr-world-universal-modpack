# Plan — task-01 core/ap2 model + parser

Status: Approved 2026-08-29 (auto mode — verified upstream approval chain)

## Byte-identity strategy (constraint 5/6 decision — shared with task-02)

The core property is `serialize(parse(x)) == x` for every accepted `x`, with all
lengths/offsets genuinely recomputed. Strategy: **recompute everything, but carry
enough parse-time layout metadata that recomputation reproduces the original layout
when the model is unmodified**. Concretely:

1. **String table** — stored as the raw plaintext bytes exactly as parsed
   (`StringTable { raw: Vec<u8> }`) plus lookup/intern accessors. Appends are the
   only mutation (NUL-terminated, padded to 4); existing offsets never move, so
   every stored `name_offset` (labels, header name offset, exported assets,
   PlaceObject movie names inside opaque payloads) stays valid without rewrites.
   Unmodified table ⇒ byte-identical emission; header offset/size recomputed from
   emission position and `raw.len()`.
2. **Tag sections** — frames/tags/labels re-emitted from the model. A per-section
   `SectionLayout` records: `name_reference_flags` verbatim (semantics unknown —
   bemaniutils ignores it), the emission ORDER of the three regions (ascending
   original offset), the raw GAP bytes that preceded each region, and each
   region's original offset (re-emitted verbatim for regions that serialize to
   zero bytes — semantics-free values). Region offsets in the header are
   recomputed from the emission cursor; for unmodified sections the recorded
   order+gaps make the cursor land exactly on the original offsets.
3. **Tag padding** — each variable-size tag stores its parse-time pad bytes
   (0..=3, may be non-zero in real files); serialize re-emits them when the
   length still matches, else zero-pads.
4. **DefineSprite** — `sprite_flags`/`id` typed; new-style relative pointer
   reproduced as `8 + pre_section.len()` where `pre_section` is the (usually
   empty) slack between the sprite header and the nested section; `post_section`
   carries payload bytes past the nested section's extent. Nested section
   recursively per rule 2.
5. **PlaceObject** — the WHOLE payload stays opaque in the model
   (`PlaceObject { data, pad }`), with a decoded read-only VIEW
   (`PlaceObject::view()`) and a from-scratch ENCODER (`PlaceObject::build()`)
   for new tags. Rationale (constraint 6 fallback, chosen deliberately): the
   flag-conditional field order interleaves unmodeled fields (0x10 label,
   0x40 unk3, 0x20000 blend) *between/around* the modeled ones, and the
   mid-payload realign-to-4 skips catchup bytes whose content bemaniutils never
   validates (they may be non-zero in real files) — re-encoding from typed
   fields cannot guarantee byte identity. The feature only READS existing
   PlaceObjects and CREATES new ones, never mutates in place, so the opaque
   carriage + view + builder covers every consumer with trivially perfect
   byte identity.
6. **File level** — the file is modeled as: raw `prefix` (header + anything
   before the first known region), the root tag section, a raw `middle`, the
   string table, and a raw `suffix` (region order recorded — string table may
   precede the section). `total length @4`, `tag-section ptr @36`, `string
   table offset/size @48/52` are recomputed; the remaining file-offset header
   fields (`exported assets @40`, `imported tags @44`, `initializers @56` iff
   flags&0x4) are fixed up by the shift delta of the zone they point into
   (delta 0 for unmodified docs). All other header bytes carried verbatim.

Accepted-input restrictions that keep the property total (parser returns `None`,
mirroring serialize-side limits): data version must be 0x200; total length must
equal buffer length; string table ≤ 64 KiB and size % 4 == 0 (the game fatals on
misaligned tables, so no real file violates this); section regions must not
overlap and must start at/after the 24-byte section header; known file regions
must not overlap and must start at/after the fixed header (56, or 60 with
flags&0x4); every tag's 4-byte padding must be present in full.

## Module layout

```
src/core/ap2/mod.rs      — //! docs, pub mod decls, re-exports, rolling cipher (local copy)
src/core/ap2/model.rs    — types + accessors (Ap2Doc, TagSection, Tag, PlaceObject view/builder, StringTable, SpritePath)
src/core/ap2/parse.rs    — Ap2Doc::parse
src/core/ap2/write.rs    — Ap2Doc::serialize (task-02 implements; stub returns None until then)
src/core/ap2/fixtures.rs — #[cfg(test)] raw byte-image assembler + model-level fixture builder
src/core/ap2/tests.rs    — #[cfg(test)] suites
```

`pub mod ap2;` added to `src/core/mod.rs` (alphabetical order). Whole tree
std-only; carriage fields `pub(super)` so sibling submodules share them.

## Test scenarios (written before the parser implementation)

1. **structural parse (AC1)** — hand-assembled image: 2 frames, 3 root tags
   (opaque, DefineSprite with nested section carrying its own label + 1 frame,
   opaque), 1 root label, exported name. Assert frame spans, tag order/kinds,
   both label maps, exported name.
2. **opaque carriage (AC2)** — unknown tag ids parse as `Opaque` with byte-exact
   payloads and correct ids.
3. **malformed totality (AC3)** — battery over corrupt variants, all `None`, no
   panic: truncated header; bad magic; data version ≠ 0x200; length mismatch;
   string table out of range / misaligned size / > 64 KiB; tag-section pointer
   out of range; truncated section header; tag size overrunning the section;
   frame/label arrays out of range; label name offset past the table;
   DefineSprite new-style pointer < 8 or past payload; overlapping regions;
   truncated final-tag padding.
4. **accessors** — `exported_name()`, `find_sprite_by_label` (root ⇒ empty path,
   nested ⇒ tag-index path), `max_character_id` (sprite + shape ids),
   `section()/section_mut()` navigation, per-section label map.
5. **cipher** — encode/decode round-trip + a known vector matching the
   `core/afp.rs` semantics (cannot import it — value-checked instead).
6. **harness mountability (AC4)** — the suite compiles/passes via
   `./scripts/validate_s_marvelous.sh` (submodule `#[path]` mounting exercised).

TDD order: model types + fixtures + tests with `parse`/`serialize` stubbed to
`None` → red run (logged) → implement `parse.rs` → task-01 green (task-02 tests
red until its implementation).
