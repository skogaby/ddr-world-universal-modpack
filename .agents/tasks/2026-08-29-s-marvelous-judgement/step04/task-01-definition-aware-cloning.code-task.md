# Task: ap2 primitives for definition-aware cloning

## Description
Two additions to `core/ap2/edit.rs` needed by the dance_judge patch:
`clone_sprite_definition` (duplicate a DefineSprite under a new id with
internal character remap) and a **placements-only** variant of the labeled
segment clone (skip definition tags — the segment's frame 0 carries the
template's entire dictionary).

## Background
The real dance_judge template's `in_marvelous` segment starts at frame 0,
which executes 16 tags: 12 DEFINITIONS (sprites 3/6/35/53, shapes 5..32)
plus the segment's two placements (word sprite 35 at depth 2, shape 8 at
depth 3). The Step 3 demo's naive clone duplicated every definition —
tolerated by parsers, but wrong for the real patch: remapping inside a
duplicated definition would corrupt the dictionary, and re-definitions bloat
the timeline. The correct patch shape (see the structure notes in
.agents/planning/2026-08-29-s-marvelous-judgement/research/display-side-re.md §10):

1. `clone_sprite_definition(path, src_id=35, new_id, remap {32→54})` — a
   byte-level duplicate of the DefineSprite tag under `new_id`, internal
   PlaceObjects remapped, inserted immediately after the original
   definition (dictionary ordering: definition before first use).
2. `clone_labeled_segment` placements-only mode: cloned frame spans contain
   copies of NON-definition tags only (PlaceObject/RemoveObject/Opaque
   non-definition; skip DefineSprite/Shape/DefineFont-class tags); span
   tag-counts shrink accordingly. Definitions from the source segment stay
   where they are — the dictionary persists across frames.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-29-s-marvelous-judgement/design/detailed-design.md (§4.1, §4.2)

**Additional References (if relevant to this task):**
- src/core/ap2/edit.rs (Step 3 primitives — extend, don't fork; reuse remap_tag / insert_tags_at_frame_end / segment_end helpers)
- .agents/scratchpad/2026-08-29-s-marvelous-judgement/editing-primitives/progress.md (API notes + gotchas: inserts shift tag indices)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `clone_sprite_definition(&mut self, path: &SpritePath, src_id: u16,
   remap: &TagRemap) -> Option<u16>` — finds the DefineSprite tag with
   `src_id` in the section at `path`, deep-copies it (recursive nested
   section) under `max_character_id()+1`, applies `remap` to the COPY's
   internal tags (all nesting levels, same id scope as `remap_tag`), inserts
   the copy directly after the original definition (same frame span —
   tag-index fixups for later spans/frames), returns the new id. Rejects:
   unknown path/id, id-width overflow. Failure leaves the doc unchanged.
2. Placements-only segment clone: either a `mode` parameter on
   `clone_labeled_segment` or a sibling
   `clone_labeled_segment_placements_only` (implementer's call — keep the
   existing behavior available for tests/other callers; document when each
   is right). Definition-tag classification: DefineSprite + Shape + the
   definition-class tag ids carried as Opaque (0x78 font, 0x7D/0x7E text,
   0x82 morph, 0x83 image — classify by tag id, not by modeled type).
3. Both primitives: no panics, `Option` returns, serialize-revalidated
   limits, std-only.
4. Host tests: sprite-definition clone (nested remap correctness, insertion
   position, dictionary ordering, round-trip); placements-only clone on a
   fixture whose segment includes definitions (cloned spans exclude them,
   original definitions untouched, round-trip); failure-leaves-unchanged
   for both.
5. `./scripts/validate_s_marvelous.sh` fully green (incl. dev legs on this
   machine); `cargo check --target x86_64-pc-windows-msvc` clean.

## Dependencies
- Step 3's edit.rs (in tree).

## Implementation Approach
1. Tests first (fixture with a frame-0-definitions segment mirroring the
   real template's shape).
2. Implement, reusing the existing fixup helper — do NOT introduce a second
   frame-span fixup implementation.

## Acceptance Criteria

1. **Sprite definition clone**
   - Given a fixture with sprite S placing shape X, and remap {X→Y}
   - When clone_sprite_definition runs
   - Then a new sprite id exists directly after S's definition whose nested
     placements reference Y, S is untouched, and the doc round-trips

2. **Placements-only segment clone**
   - Given a segment whose frame 0 span holds definitions + placements
   - When the placements-only clone runs
   - Then cloned spans contain only the non-definition tags, the new label
     lands at the first cloned frame, and definitions are not duplicated

3. **Failure atomicity**
   - Given invalid inputs (unknown id, duplicate label)
   - When either primitive returns None
   - Then the doc serializes byte-identically to its pre-call state

## Metadata
- **Complexity**: Medium
- **Labels**: s-marvelous, core-ap2, timeline-editing, pure-module
- **Required Skills**: Rust, Step 3 ap2 module internals
- **Generated By**: code-task-generator 2026-08-29
- **Source Plan**: .agents/planning/2026-08-29-s-marvelous-judgement/implementation/plan.md
- **Plan Step**: Step 4
