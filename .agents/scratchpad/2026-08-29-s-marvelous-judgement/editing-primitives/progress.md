# Progress — step03 task-01 core/ap2 editing primitives

Updated: 2026-08-29

## Checklist

- [x] Read task file + design §4.1/§4.2 + the WHOLE `src/core/ap2/` module +
      docs/afp_system.md §9 + research afp-tooling.md §2 + both prior
      sessions' progress notes + `core/afp.rs::patch_inject_children`
      (the shipped frame-insert fixup precedent)
- [x] context.md / plan.md (API signatures, splice-vs-rebuild decision,
      segment rule, failure-atomicity ordering, test scenarios)
- [x] Baseline harness green (logs/00-baseline.log: 48 tests, Leg A 76
      templates byte-identical, Leg B match)
- [x] Tests written first (17 new `edit_*` tests appended to
      `src/core/ap2/tests.rs`) against a stubbed `edit.rs`
- [x] `src/core/ap2/edit.rs` implemented (all five primitives + helpers)
- [x] `pub mod edit;` + `NamedPlacement`/`TagRemap` re-exports wired into
      `src/core/ap2/mod.rs` (house `#[allow(unused_imports)]` pattern)
- [x] Harness green incl. dev legs; cargo check msvc clean; cargo fmt
      (whole crate) + post-fmt re-run green

## TDD record

1. Baseline — logs/00-baseline.log: exit 0; 48 lib tests (38 ap2 + 10
   s_marvelous_state); Leg A 76 templates byte-identical; Leg B structural
   match.
2. RED — logs/01-red-stubs.log: 17 new tests against `None`/`0` stubs;
   exit 101, 50 passed / 15 failed. The 2 "passing" new tests are the
   pure-failure ones trivially satisfied by stubs
   (`edit_add_shape_failures`, `edit_adjust_placements_no_match_is_identity`).
3. GREEN — logs/02-green.log: first real implementation run; exit 0;
   65 passed / 0 failed (lib), 55 passed (ap2check bin mount = 38+17 ap2),
   Leg A 76/76 byte-identical, Leg B match.
4. logs/03-cargo-check-msvc.log: `cargo check --target x86_64-pc-windows-msvc`
   clean, zero warnings.
5. logs/04-final-green-post-fmt.log: post-`cargo fmt` re-run — exit 0,
   65 / 55 passed, Leg A/B green. `cargo fmt --check` clean.

## Test coverage delivered (17 tests)

add_label success (root + nested, round-trip resolution) and failure battery
(duplicate-in-section, frame OOB, bad/non-sprite path — byte-identical after;
cross-SECTION duplicate allowed). add_shape allocation (`max+1`, sequential),
insertion inside the target frame's span, later-frame shifts + label
stability, failures (bad path/frame, id space exhausted at `u16::MAX`).
clone_labeled_segment: AC1 exactly (labels A frames 0..2 / B 3..4 → clone A
as C with remap → frames 5..7 span-correct copies, label C→5, A/B unchanged,
remapped ids, shared instance-name offset, round-trip + fixed point);
section-end segment with empty remap (verbatim tag equality); Shape AND
DefineSprite definition-id remap; unmodeled-payload preservation
(blend/label_id/unk3/junk-realign/opaque-tail payload cloned byte-identical
except the 2 spliced id bytes; second-flag-word payload → splice at offset
12); failure battery (missing src label, duplicate new label, bad paths,
first-appended-frame index past u16, undecodable PlaceObject with non-empty
remap — all None + byte-identical; same undecodable segment with EMPTY remap
clones fine). add_place_object_named: AC2 exactly (depths 1..3 in frame 0 →
fresh depth lands in-span, later span shifts, name interned + resolves after
re-parse, translate carried); failures (occupied depth, frame OOB, bad path —
byte-identical after; per-FRAME uniqueness proven by inserting the same depth
into another frame). adjust_placements: predicate scoping (depth≥2 → count 2,
adjusted values exact, unmatched tag byte-identical, round-trip);
missing-translate skip + nested-section reach; no-match identity;
unmodeled-byte preservation (only the 8 translate bytes differ).
Integration (AC): add_shape → find_sprite_by_label → clone with remap
{old→new} → clone references the new shape, original keeps the old, re-parse
+ one-emission fixed point.

## The adjust_placements decision (task req 1, last bullet)

Neither of the task's two offered shapes (full `PlaceObject::build` rebuild /
constrain-to-fully-modeled): both would be useless or lossy on real
templates, whose placements carry unmodeled fields (colors/events/filters).
Implemented instead as a **surgical 8-byte translate splice** at the field's
flag-determined offset (`place_field_offsets` mirrors the `view()` walk) —
byte-exactness of every unmodeled byte by construction. Constraint kept:
matched tags WITHOUT flag 0x400 are skipped and not counted (retro-fitting
the field would shift the opaque tail, whose invariants are unproven);
likewise undecodable payloads and i32 overflow. Log-free; the caller compares
the returned count and WARNs. Full justification in plan.md.

## Deviations

- **Clone remap is "view walk + surgical splice", not the task's literal
  "(via view + rebuild)"**: a `PlaceObject::build` rebuild only re-emits the
  modeled flags and would drop the color/event/filter tail the real
  marvelous fade animations carry — visibly breaking Step 4. The splice
  writes the 2 `source_tag_id` bytes at their deterministic offset in the
  CLONE (the model's "never mutate parsed payloads in place" doctrine is
  about unmodified-doc byte identity; clones are new tags). Same mechanism
  reused for adjust_placements per the decision above.
- **`add_shape` returns `Option<u16>` (task sketch: bare `u16`) and takes an
  explicit `(path, frame)`** — requirement 2 (no panics ⇒ fallible
  allocation/path) and docs §9 ordering (definitions must precede referencing
  PlaceObjects and live inside an executed frame span; the shipped
  `patch_inject_children` precedent). A bare tag-list append would land AFTER
  any PlaceObject later inserted into an earlier frame.
- **`add_place_object_named` takes a dedicated `NamedPlacement` struct**
  (task sketch: the model's `PlaceObjectParams`) — the primitive needs the
  target frame and the instance-name STRING (it interns); `PlaceObjectParams`
  carries a raw offset and no frame.
- **`add_label` validates `frame < frames.len()`** (not in the task text) —
  "validate before mutating" hygiene; every feature use labels existing
  frames.
- **DefineSprite ids participate in `TagRemap`** (task lists PlaceObject +
  Shape ids only) — cloning a segment containing a definition would otherwise
  emit a duplicate character id; ids absent from the map still clone as-is
  (caller's responsibility), no recursion into nested sections.
- **Bonus validations beyond the task list**: clone rejects a first-appended-
  frame index the u16 name-reference field cannot address, and rejects
  segments with undecodable PlaceObjects when the remap is non-empty
  (fail-closed: an uninspectable segment cannot be remapped safely).

## API notes for the next task (edit-demo on real templates / harness subcommand)

Exact signatures (all on `Ap2Doc`, re-exported from `core::ap2`):

```rust
pub type TagRemap = std::collections::HashMap<u16, u16>; // old id → new id

pub struct NamedPlacement<'a> {
    pub frame: usize,
    pub depth: u16,
    pub object_id: u16,
    pub source_tag_id: u16,
    pub instance_name: &'a str,
    pub translate: Option<(i32, i32)>, // raw fixed-point /20
}

pub fn add_label(&mut self, path: &SpritePath, name: &str, frame: u16) -> Option<()>;
pub fn add_shape(&mut self, path: &SpritePath, frame: usize, unknown: u16) -> Option<u16>;
pub fn clone_labeled_segment(&mut self, path: &SpritePath, src_label: &str,
                             new_label: &str, remap: &TagRemap) -> Option<()>;
pub fn add_place_object_named(&mut self, path: &SpritePath,
                              placement: &NamedPlacement<'_>) -> Option<()>;
pub fn adjust_placements(&mut self, pred: impl Fn(&PlaceObjectView) -> bool,
                         dxy: (i32, i32)) -> usize; // doc-wide recursive walk
```

Gotchas for callers:

- **Workflow order**: inserts (`add_shape`, `add_place_object_named`) shift
  tag indices at/after the insertion point in the TARGET section — resolve
  `SpritePath`s (`find_sprite_by_label`) AFTER any insert into an ancestor
  section (the integration test demonstrates the order: add_shape → find →
  clone). Clones/labels only APPEND — they never invalidate paths.
- `add_shape` inserts at the END of the given frame's span; for the
  dance_judge patch use frame 0 of the root (definition visible before
  anything plays), then clone — cloned tags append after it, satisfying the
  docs §9 ordering.
- `adjust_placements` returns the ADJUSTED count only; matched-but-skipped
  tags (no translate field / undecodable / overflow) are silent by design —
  compare the count against the expected row count and WARN from the caller.
- The dance_judge/fullcombo remap should map the donor art's SHAPE id (the
  `source_tag_id` the segment's PlaceObjects carry), not the sprite id.
- `unknown` (Shape's leading u16): copy the donor shape's value when cloning
  art semantics; 0 is a safe default for fresh art.
- All packed-field limits are re-validated at `serialize()` — an edited doc
  exceeding them serializes to `None` with the doc intact, so patch functions
  should serialize and fall back to the original bytes on `None`.

## Files created/changed

- NEW `src/core/ap2/edit.rs` (the five primitives + `segment_end`,
  `frame_end_insert_point`/`insert_tags_at_frame_end`, `remap_tag`,
  `place_field_offsets`)
- `src/core/ap2/mod.rs` (`pub mod edit;` + re-exports)
- `src/core/ap2/tests.rs` (+17 `edit_*` tests, module-doc update)

Status: Complete (uncommitted — maintainer commits manually)
