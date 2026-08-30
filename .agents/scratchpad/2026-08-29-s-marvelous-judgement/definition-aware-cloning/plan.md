# Plan — step04 task-01 definition-aware cloning (core/ap2/edit.rs)

Status: Approved 2026-08-29 (auto mode — verified upstream approval chain)

## Scope

Two new `Ap2Doc` primitives in `src/core/ap2/edit.rs` + supporting refactors,
per the task file and the real dance_judge template structure
(research `display-side-re.md` §10):

```rust
pub fn clone_sprite_definition(&mut self, path: &SpritePath, src_id: u16,
                               remap: &TagRemap) -> Option<u16>;
pub fn clone_labeled_segment_placements_only(&mut self, path: &SpritePath,
                               src_label: &str, new_label: &str,
                               remap: &TagRemap) -> Option<()>;
```

## Design

### clone_sprite_definition

Phase 1 (read-only): allocate `new_id = max_character_id()+1` (checked);
resolve section; find the FIRST `Tag::DefineSprite` with `id == src_id`
(tag index `ti`); find the first frame whose span COVERS `ti` (a definition
outside every executed span has no "same frame span" to insert into — fail
closed); deep-copy the tag (`DefineSprite: Clone` — recursive nested
`TagSection`); apply `remap` to the copy's internals at ALL nesting levels via
`remap_section_recursive` (per-tag scope identical to `remap_tag`: Shape ids,
DefineSprite ids, PlaceObject source ids via the surgical splice; undecodable
PlaceObject + non-empty remap ⇒ None); set `copy.id = new_id` (the outer id is
NEVER remapped-from-map); pre-validate the insert at `ti+1` attributed to the
covering frame. Phase 2: mutate through the shared insert core. No string
interning — the whole fallible surface precedes the first mutation.

Copy `pad` carriage stays valid: id splices are 2-byte overwrites, payload
lengths never change.

### Placements-only clone: SIBLING FUNCTION (the task's requirement-2 choice)

Chosen: a sibling `clone_labeled_segment_placements_only`, NOT a mode
parameter on `clone_labeled_segment`. Justification:

- The 4-arg signature is live in the validate harness's embedded `edit-demo`
  binary (`scripts/validate_s_marvelous.sh`) and 17 existing tests; a mode
  param churns every call site including a generated-script heredoc.
- Two names document intent at call sites: **placements-only** is right for
  real templates whose segment frames carry the dictionary (dance_judge frame
  0 defines everything — duplicating + remapping definitions would corrupt the
  dictionary); **the verbatim clone** remains right for segments without
  definitions and for fixtures/tests that assert whole-segment duplication.
- Logic sharing where the semantics genuinely coincide: the validation prelude
  (dup-label, source-label resolve, dangling-label clamp, u16 label-frame
  bound) is extracted to `segment_clone_bounds`, and both paths reuse
  `segment_end` + `remap_tag` + the three-phase atomicity pattern. The tag-copy
  loops intentionally differ: the verbatim clone copies the contiguous
  `[lo, hi)` range ONCE (preserving sharing under overlapping spans); the
  placements-only clone must filter per frame, so it walks each span and
  appends only non-definition tags (`copied` per frame = the shrunken span
  count). Zero-copied frames emit `FrameSpan { start_tag: base + appended_so_far,
  tag_count: 0 }` — semantics-free at count 0, always ≤ the final tag-list
  length; serializer acceptance covered by a dedicated test (serialize
  enforces only the packed-field widths, parse never validates spans).

### Definition classification (by TAG ID, incl. Opaque carriage)

`is_definition_tag_id(id)`: 0x78 (font), 0x79 (DefineSprite), 0x7D (text),
0x7E (edit text), 0x82 (morph), 0x83 (image), 0x84 (shape). New named
constants for 0x78/0x7D/0x82 in `model.rs` (house "named for future use"
pattern). Classification runs on `Tag::tag_id()` so definition-class tags
carried as `Tag::Opaque` are covered.

### Insert-fixup refactor (no second implementation)

`frame_end_insert_point` / `insert_tags_at_frame_end` become thin wrappers
over a generalized core that accepts any index within the target frame's span:

```rust
fn validate_insert_in_frame(sec, frame, insert_index, n) -> Option<()>; // read-only
fn insert_tags_in_frame(sec, frame, insert_index, new_tags) -> Option<()>; // THE fixup
```

Rule unchanged: target frame's count grows by n; every OTHER frame whose
`start_tag >= insert_index` shifts right. New containment check
`span.start <= insert_index <= span.end` (trivially true for end inserts).

### remap refactor

`remap_tag`'s body extracted to `remap_tag_in_place(&mut Tag, remap)`;
`remap_tag` = clone + in-place. New `remap_section_recursive(&mut TagSection,
remap)` applies it to every tag and recurses into nested DefineSprite
sections — used ONLY on the sprite copy's private tree (the non-recursive
scope of `remap_tag` for segment clones is unchanged: those tags' nested
sections are shared definitions).

## Test scenarios (written first — RED against stubs, then GREEN)

Fixture `template_fixture()` mirroring §10 / implementation-note 4: root
frame 0 = [DefineSprite 3 (places shape 32), Shape 32, Shape 8, DefineSprite
35 (places 32, named+translated), PlaceObject(35, depth 2, named),
PlaceObject(8, depth 3)]; frames 1..3 = translate-only placement updates;
labels seg1@0, seg2@2. (Ids mirror the real template: 35 = word sprite,
32 = word shape, 8 = flash shape.)

1. `edit_clone_sprite_def_ac1` — add_shape (→36), clone sprite 35 with
   {32→36} → 37; copy directly after tag 3; copy id 37; nested PO source 36,
   translate/name preserved; original nested PO still 32; frame 0 count +1,
   later spans shift; labels stable; round-trip + fixed point + re-parse
   nested check.
2. `edit_clone_sprite_def_recurses_nesting` — sprite with 2 nesting levels
   (inner Shape def, inner DefineSprite def, innermost PlaceObject); remap
   remaps all three id kinds at both levels in the copy; original untouched.
3. `edit_clone_sprite_def_failures` — unknown path / non-sprite path; src_id
   absent; src_id matching a Shape (not a sprite); definition not covered by
   any frame span; id space exhausted (Shape id u16::MAX); undecodable nested
   PlaceObject + non-empty remap (None) vs empty remap (Some). All None cases
   byte-identical after.
4. `edit_clone_placements_only_ac2` — template fixture, clone seg1 →
   "seg1_s" with {35→55}: exactly 3 tags appended (PO35→55 w/ shared name
   offset, PO8, the frame-1 update), spans (9,2),(11,1), label @4, zero new
   definition tags, originals byte-identical, round-trip + fixed point.
5. `edit_clone_placements_only_zero_tag_frames` — segment whose frames hold
   [def-only], [placement], [count-0]: cloned spans (N,0),(N,1),(N+1,0);
   count-0 span pointing AT the final tag-list length serializes + re-parses
   (the explicit serializer-accepts-count-0 test).
6. `edit_clone_placements_only_opaque_definition_ids` — segment carrying
   Opaque 0x78/0x7D/0x7E/0x82/0x83 + Opaque 0x80 + Opaque 0x50: only the two
   non-definition opaques clone; span count shrinks 7→2.
7. `edit_clone_placements_only_failures` — missing src label, duplicate new
   label, bad/non-sprite paths, span past the tag list, undecodable
   PlaceObject + non-empty remap; all None + byte-identical; empty remap on
   the undecodable segment succeeds (PlaceObject is non-definition ⇒ cloned
   verbatim).
8. `edit_integration_definition_aware_patch_shape` — the full §10 sequence
   (add_shape → clone_sprite_definition {32→new} → placements-only clone
   {35→new_sprite}); single definition per id doc-wide; cloned placements
   reference the new sprite; new sprite references the new shape; round-trip
   fixed point.

## Validation gates

- `./scripts/validate_s_marvelous.sh` (RED log, GREEN log, final post-fmt
  log) — dev legs A/B/C must stay green (Leg C exercises the UNCHANGED
  verbatim clone on the real template).
- `cargo check --target x86_64-pc-windows-msvc` clean.
- `cargo fmt` (whole crate) at the end + re-run harness.
