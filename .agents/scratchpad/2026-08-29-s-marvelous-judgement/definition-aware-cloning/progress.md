# Progress — step04 task-01 definition-aware cloning (core/ap2)

Updated: 2026-08-29

## Checklist

- [x] Read task file + design §4.1/§4.2 + research display-side-re.md §10 +
      the whole `src/core/ap2/` module + editing-primitives progress notes +
      `scripts/validate_s_marvelous.sh` (Leg C's embedded 4-arg
      `clone_labeled_segment` call — signature stability constraint)
- [x] context.md / plan.md (mode-vs-sibling decision, insert-fixup
      generalization plan, recursive-remap plan, 8 test scenarios)
- [x] Baseline harness green (logs/00-baseline.log: 65 lib + 55 bin tests,
      Leg A 76 templates byte-identical, Leg B match, Leg C render OK)
- [x] Tests written first (8 new tests + `template_fixture` appended to
      `src/core/ap2/tests.rs`) against `None` stubs
- [x] Implemented: `clone_sprite_definition` +
      `clone_labeled_segment_placements_only` + helper refactors in
      `edit.rs`; 3 new tag-id constants in `model.rs`
- [x] Harness green incl. dev legs; cargo check msvc clean; cargo fmt
      (whole crate) + post-fmt re-run green

## TDD record

1. Baseline — logs/00-baseline.log: exit 0; 65 lib tests, 55 ap2check-bin
   tests, Legs A (76/76 byte-identical) / B / C green.
2. RED — logs/01-red-stubs.log: 8 new tests against `None` stubs; exit 101,
   65 passed / 8 failed (every new test RED — the failure-battery tests end
   in a success assertion, so stubs don't trivially satisfy them).
3. GREEN — logs/02-green.log: exit 0; 73 lib passed / 0 failed, 63 bin
   passed, Leg A 76/76, Leg B match, Leg C render OK.
4. logs/03-cargo-check-msvc.log: `cargo check --target
   x86_64-pc-windows-msvc` clean, zero warnings.
5. logs/04-final-green-post-fmt.log: post-`cargo fmt` re-run — exit 0,
   73 / 63 passed, Legs A/B/C green. `cargo fmt --check` clean (fmt produced
   no changes in the ap2 tree; the 6 modified tracked files in `git status`
   are prior steps' uncommitted work, untouched here).

## The mode-vs-sibling decision (task requirement 2)

**Sibling function** `clone_labeled_segment_placements_only`, not a mode
parameter. Rationale (full text in plan.md): the 4-arg
`clone_labeled_segment` signature is live in the validate harness's embedded
`edit-demo` binary and 17 existing tests; two names document intent at call
sites (placements-only for real templates whose segment frames carry the
dictionary; verbatim for definition-free segments/fixtures). Shared where
semantics coincide: new `segment_clone_bounds` prelude (dup-label /
source-label / dangling-label / u16-label-frame validation) used by BOTH,
plus the existing `segment_end` + `remap_tag` + three-phase atomicity
pattern. The copy loops intentionally differ (contiguous `[lo,hi)` verbatim
copy vs per-frame filtered walk).

## Test coverage delivered (8 tests)

`template_fixture()` mirrors the real dance_judge shape (§10 /
implementation-note 4): frame 0 = [DefineSprite 3 (places 32), Shape 32,
Shape 8, DefineSprite 35 (places 32, named+translated), PlaceObject(35,d2,
named), PlaceObject(8,d3)]; frames 1..3 translate-only updates; labels
seg1@0 / seg2@2.

- `edit_clone_sprite_def_ac1` — AC1 exactly: add_shape→36, clone sprite 35
  with {32→36} → 37; copy at src_index+1, copy id 37 (NOT remapped-from-map),
  nested placement remapped to 36 with translate/name-offset preserved,
  original untouched, frame-0 count +1 + later spans shifted, labels stable,
  round-trip + nested re-parse check + fixed point.
- `edit_clone_sprite_def_recurses_nesting` — 2 nesting levels: Shape id,
  nested DefineSprite id, innermost PlaceObject source all remapped in the
  copy; original tree byte-level untouched at every level.
- `edit_clone_sprite_def_failures` — unknown/non-sprite path, absent src_id,
  src_id belonging to a Shape, definition covered by no frame span
  (fail-closed: no "same span" to insert into), id space exhausted at
  u16::MAX, undecodable NESTED PlaceObject + non-empty remap (None) vs empty
  remap (Some) — all None cases byte-identical after.
- `edit_clone_placements_only_ac2` — AC2 exactly: 3 non-definition tags
  appended (remapped word placement w/ shared name offset, un-remapped flash
  placement, frame-1 update), spans shrink to (9,2),(11,1), label at first
  cloned frame, zero new definitions, originals untouched, round-trip +
  fixed point.
- `edit_clone_placements_only_zero_tag_frames` — definitions-only span
  clones to count 0; source count-0 frame stays count 0; **the explicit
  serializer-accepts-count-0-spans test** incl. a span pointing AT the
  tag-list end; frames survive the round trip bit-exact.
- `edit_clone_placements_only_skips_opaque_definition_ids` — Opaque-carried
  0x78/0x7D/0x7E/0x82/0x83 all skipped; Opaque 0x80 (RemoveObject) and
  unknown 0x50 cloned; span 7→2.
- `edit_clone_placements_only_failures` — missing src label, duplicate new
  label, bad/non-sprite paths, span past the tag list, undecodable
  PlaceObject + non-empty remap — all None + byte-identical; empty remap on
  the undecodable segment succeeds (PlaceObject is non-definition ⇒ cloned
  verbatim).
- `edit_integration_definition_aware_patch_shape` — the full §10 patch
  sequence (add_shape → clone_sprite_definition {32→new} → placements-only
  clone {35→new_sprite}): every character id defined exactly once doc-wide,
  cloned segment places the new sprite + the shared flash shape, new sprite
  references the new shape, original segment still places {35, 8},
  round-trip fixed point.

## API notes for task-02 (the dance_judge patch builder)

Exact new signatures (on `Ap2Doc`, `core::ap2`):

```rust
pub fn clone_sprite_definition(&mut self, path: &SpritePath, src_id: u16,
                               remap: &TagRemap) -> Option<u16>;
pub fn clone_labeled_segment_placements_only(&mut self, path: &SpritePath,
                               src_label: &str, new_label: &str,
                               remap: &TagRemap) -> Option<()>;
```

Gotchas for the patch fn:

- **Workflow order for the real patch** (validated by the integration test):
  `add_shape(root, 0, donor_unknown)` → re-resolve paths →
  `clone_sprite_definition(path, 35, {32→new_shape})` → re-resolve paths →
  `clone_labeled_segment_placements_only(path, "in_marvelous",
  "in_smarvelous", {35→new_sprite})`. Both `add_shape` and
  `clone_sprite_definition` INSERT (shifting tag indices at/after the
  insertion point) — recompute any held `SpritePath` after each
  (`find_sprite_by_label`); the placements-only clone only APPENDS.
- `clone_sprite_definition` returns the copy's id — allocated as
  `max_character_id()+1` at CALL time, so call it before anything else
  allocates if the patch hardcodes expectations; better: use the returned
  values, never constants.
- The remap for the sprite clone maps the SHAPE id inside the sprite
  ({32→54}-shaped); the remap for the segment clone maps the SPRITE id the
  segment's PlaceObjects reference ({35→55}-shaped). Ids absent from either
  map stay shared (e.g. flash shape 8) — intentional.
- On the REAL template `in_marvelous` starts at frame 0 whose span carries
  the whole dictionary — the placements-only clone reduces that frame to
  exactly the 2 placements; later frames of the segment are pure placement
  updates and clone 1:1.
- Definition classification is by tag id (0x78/0x79/0x7D/0x7E/0x82/0x83/
  0x84) — `AP2_DEFINE_BUTTON` (0x7B) / `_BUTTON_SOUND` (0x7C) are NOT
  classified as definitions (never observed in dance templates; add to
  `is_definition_tag_id` in `edit.rs` if one ever appears in a patch
  target).
- Zero-tag cloned frames serialize fine (count-0 spans, start index
  semantics-free) — no special casing needed downstream.
- All packed-field limits still re-validate at `serialize()`; patch fns
  should serialize and fall back to the original bytes on `None`.

## Deviations

- **`clone_sprite_definition` rejects a definition covered by no frame
  span** (not in the task text): the task's "insert ... in the same frame
  span" has no referent for an unexecuted definition — fail-closed keeps the
  primitive total instead of guessing a span. Real templates always execute
  definitions (frame 0).
- **The undecodable-PlaceObject rule extends into the copied sprite's
  nested tree** (task only specifies it for segment clones): with a
  non-empty remap, any undecodable placement at any nesting level fails the
  definition clone — same fail-closed reasoning (an uninspectable tree
  cannot be remapped safely); empty remap copies verbatim.
- **Insert-fixup helpers refactored, not duplicated** (per the task's
  reuse mandate): `frame_end_insert_point`/`insert_tags_at_frame_end` are
  now thin wrappers over `validate_insert_in_frame`/`insert_tags_in_frame`
  (any in-span index). One added validation in the shared core: the frame's
  own span end must lie within the tag list (`e > tags.len()` refuses)
  where the old end-insert check was equivalent (`insert_index == e`); for
  mid-span inserts the containment check (`start <= index <= end`) is new.
  All 17 Step-3 tests still green.
- **`remap_tag` split into `remap_tag_in_place` + wrapper**, plus new
  `remap_section_recursive` for the sprite copy's PRIVATE tree only — the
  non-recursive scope of segment-clone remaps is unchanged (their nested
  sections are shared definitions).
- **Three new named tag-id constants** in `model.rs` (`TAG_DEFINE_FONT`
  0x78, `TAG_DEFINE_TEXT` 0x7D, `TAG_DEFINE_MORPH_SHAPE` 0x82; names
  verified against bemaniutils ap2.py) — house "named for future use"
  pattern, consumed by `is_definition_tag_id`.

## Files created/changed

- `src/core/ap2/edit.rs` — `clone_sprite_definition`,
  `clone_labeled_segment_placements_only`, `segment_clone_bounds`,
  `is_definition_tag_id`, `validate_insert_in_frame`/`insert_tags_in_frame`
  (generalized fixup core), `remap_tag_in_place`/`remap_section_recursive`,
  module-doc update.
- `src/core/ap2/model.rs` — +3 definition-class tag-id constants.
- `src/core/ap2/tests.rs` — `template_fixture()` + 8 new tests, module-doc
  update.

Status: Complete (uncommitted — maintainer commits manually)
