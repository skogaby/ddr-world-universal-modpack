# Task: core/ap2 editing primitives

## Description
Add the mutation surface to `core/ap2` per design §4.1: frame-label
read/write, `clone_labeled_segment` (+`TagRemap`), `add_shape`,
`add_place_object_named`, and `adjust_placements` — with host tests proving
each edit produces valid, re-parseable output.

## Background
Step 2 delivered a parse/serialize round-trip proven byte-identical on 76
real templates. This task adds the edits the feature's AFP patches perform:

- **Gameplay flash / FC splash** (Steps 4/6): clone the `in_marvelous` /
  `marbelous_in` labeled timeline segment as `in_smarvelous` /
  `s_marbelous_in`, re-pointing cloned art references (shape/character ids)
  to a newly added shape so the clone shows the injected S-Marvelous art.
- **Results score tab** (Step 7): add a named instance
  (`smarvelous_num_usr`) + label art placement to `body_tab_detail_result`,
  repositioning existing row placements to open a slot.

Model facts from Step 2 (see `src/core/ap2/model.rs`): labels are per-section
`Vec<Label>` (frame_number + string offset, `name` resolved at parse);
frames are `Vec<FrameSpan { start_tag, tag_count }>` indexing into the flat
per-section tag list; `Tag::PlaceObject` carries its payload opaquely with a
decoded `view()` and a from-scratch `PlaceObject::build` encoder; string
table is append-only interning (existing offsets never move);
`SectionLayout` metadata preserves byte-identity for unmodified regions.

Segment-clone semantics ("labeled segment"): the frame range starting at the
label's frame and ending just before the next label (or at section end).
Cloned frames append at the section end; their tags append to the tag list;
the new label points at the first appended frame. Character/shape id
references inside cloned PlaceObjects are remapped through `TagRemap`; ids
not remapped stay shared (intentional — e.g. shared masks/aep helpers).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-29-s-marvelous-judgement/design/detailed-design.md (§4.1, §4.2)

**Additional References (if relevant to this task):**
- src/core/ap2/ (Step 2 model — read model.rs + write.rs fully first)
- .agents/planning/2026-08-29-s-marvelous-judgement/research/afp-tooling.md (§2 tag model)
- docs/afp_system.md §9 (tag ordering, unique depths — constraints on generated PlaceObjects)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New submodule `src/core/ap2/edit.rs` (std-only like the rest), public API
   on `Ap2Doc`/`TagSection`:
   - `add_label(section_path, name, frame) -> Option<()>` (interns the
     string; rejects duplicate names within a section).
   - `clone_labeled_segment(sprite_path, src_label, new_label, remap: &TagRemap) -> Option<()>`
     — semantics per Background; `TagRemap = map<u16, u16>` applied to
     PlaceObject character ids (via view + rebuild) and Shape ids inside the
     cloned range; frames/tags append; label added; all packed-field limits
     re-validated at serialize.
   - `add_shape(&mut self, ...) -> u16` — appends an `AP2_SHAPE` tag with id
     `max_character_id() + 1` (returns the id; the geo binding
     `{exported_name}_shape{id}` is the caller's concern).
   - `add_place_object_named(section_path, params: PlaceObjectParams) -> Option<()>`
     — builds a create-mode PlaceObject (character id, unique depth within
     the target frame, instance name, optional translate) via
     `PlaceObject::build`, inserts it into a chosen frame's span (tag-list
     insert + FrameSpan fixups for all later frames + label-frame stability).
   - `adjust_placements(pred, dxy) -> usize` — for existing PlaceObjects
     whose view matches `pred`: rebuild with adjusted translate. NOTE: this
     is the one mutate-in-place case — if byte-exact rebuild of a matched
     tag's unmodeled fields is not achievable, constrain the API to tags
     whose payload the view fully models, and return 0 + a documented
     limitation otherwise (log-free; the caller WARNs). Justify the choice
     in the working plan.
2. Every primitive returns `Option`/count — no panics; failures leave the doc
   UNCHANGED (validate before mutating, or mutate a clone and swap).
3. Host tests (in `tests.rs`): each primitive on builder fixtures —
   clone correctness (frame spans, tag indices, label maps, remapped ids),
   serialize→parse round-trip of every edited doc, depth-uniqueness and
   duplicate-label rejection, adjust_placements predicate scoping; plus one
   integration-style test: fixture with an `in_marvelous`-shaped segment →
   add_shape + clone with remap + verify the clone references the new shape.
4. All tests green via `./scripts/validate_s_marvelous.sh`; crate `cargo
   check --target x86_64-pc-windows-msvc` stays clean.

## Dependencies
- Step 2's model/parser/serializer (in tree).

## Implementation Approach
1. Tests first per primitive; implement in dependency order: add_label →
   add_shape → clone_labeled_segment → add_place_object_named →
   adjust_placements.
2. Keep FrameSpan/label fixup logic in one helper; document the
   segment-boundary rule beside it.

## Acceptance Criteria

1. **Clone correctness**
   - Given a fixture sprite with labels A (frames 0..2) and B (frames 3..4)
   - When A is cloned as C with a remap {shapeX → shapeY}
   - Then frames 5..7 exist as copies of 0..2 with remapped ids, label C → 5,
     labels A/B unchanged, and the doc round-trips through serialize→parse

2. **Named instance insertion**
   - Given a section with existing placements at depths 1..3 in frame 0
   - When add_place_object_named inserts at frame 0 with a fresh depth
   - Then the tag lands inside frame 0's span, later frame spans shift
     correctly, the instance name resolves in the string table, and
     round-trip holds

3. **Failure leaves doc unchanged**
   - Given any primitive fed an invalid input (missing label, duplicate new
     label, unknown sprite path, occupied depth)
   - When it returns None/0
   - Then the doc serializes byte-identically to its pre-call state

4. **Harness green**
   - When `./scripts/validate_s_marvelous.sh` runs
   - Then all suites pass (including the unchanged Leg A/B dev legs)

## Metadata
- **Complexity**: High
- **Labels**: s-marvelous, core-ap2, timeline-editing, pure-module
- **Required Skills**: Rust, AP2 format (Step 2 module), invariant-preserving mutation
- **Generated By**: code-task-generator 2026-08-29
- **Source Plan**: .agents/planning/2026-08-29-s-marvelous-judgement/implementation/plan.md
- **Plan Step**: Step 3
