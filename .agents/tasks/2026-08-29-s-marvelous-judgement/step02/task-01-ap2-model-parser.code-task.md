# Task: core/ap2 document model and parser

## Description
Create `src/core/ap2/` — the modpack's first full AP2 (AFP animation binary)
document model and parser: header, scrambled string table, tag sections
(frames, tags, frame-label name-reference arrays), recursive DefineSprite
subsections, and typed decode of the tags the S-Marvelous feature edits, with
opaque byte-preserving carriage of everything else.

## Background
The feature synthesizes AFP timeline edits client-side (add labeled segments,
named instances) via the shipped `afp_patcher` seam, which hands patch
functions the DESCRAMBLED AP2 bytes. bemaniutils (`bemani/format/afp/swf.py`,
public domain) is the complete read-side specification; the repo's
`core/afp.rs` already implements the descramble/cipher and minimal frame-0
injection but has no tag/timeline model. This parser is the foundation; the
serializer is task-02 of this step.

Format facts to implement (transcribed from bemaniutils and the design):
- Header: magic @0, total length @4, exported-name offset @10, tag-section
  pointer @36, string-table offset/size @48/52.
- String table: rolling-cipher scrambled (key starts 128, increments per
  byte), null-terminated UTF-8, u16 table-relative offsets, 4-byte alignment.
- Tag section header `<HHIIIII>`: name_reference_flags, name_reference_count,
  frame_count, tags_count, name_reference_offset, frame_offset, tags_offset
  (offsets relative to the section base).
- Frames: packed u32 each — low 20 bits = start index into the tag list,
  next 12 bits = tag count executed that frame.
- Tags: u32 header (`tagid = (w >> 22) & 0x3FF`, `size = w & 0x3FFFFF`) +
  payload, 4-byte aligned.
- Frame labels are NOT tags: a trailing name-reference array of `<HH>`
  (frame_number, string_offset) pairs; root movie AND every DefineSprite
  carry their own label map.
- `AP2_DEFINE_SPRITE (0x79)`: flags + sprite id, then a NESTED tag section.
- `AP2_PLACE_OBJECT (0x7F)`: flag-driven encoding — model at minimum:
  object id + depth, flag 0x2 source_tag_id (character id), flag 0x20
  movie_name (instance name, string-table offset), flag 0x400 translate
  (tx/ty s32, stored /20 fixed-point), flags 0x100/0x200 scale/rotate
  (s32/1024); carry unmodeled flag payloads as opaque trailing bytes so
  re-encode is total.
- `AP2_SHAPE (0x84)`: u16 shape id (binds geo `{exported_name}_shape{id}`).
- All other tags: `Opaque { tag_id, data }`.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-29-s-marvelous-judgement/design/detailed-design.md (§4.1, §5.4)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-29-s-marvelous-judgement/research/afp-tooling.md (§2 format map with bemaniutils file/line pointers, §4 gap list)
- docs/afp_system.md §1–§2 (repo's existing AP2 knowledge: header layout, alignment fatal, tag ordering)
- src/core/afp.rs (existing descramble/cipher — reference implementation to mirror, NOT to import; see Technical Requirements #6)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Module layout: `src/core/ap2/mod.rs` (public API + docs), `parse.rs`,
   `model.rs` (or equivalent — follow `core/ssq/` / `core/xact/` house style).
   Declare `pub mod ap2;` in `src/core/mod.rs`.
2. Model per design §4.1: `Ap2Doc` (header fields, string table as
   `Vec<String>` + offset map, root `TagSection`), `TagSection { frames,
   tags, labels }`, `Tag` enum (DefineSprite recursive, PlaceObject, Shape,
   Opaque). Every byte of the input must be either typed-decoded or carried
   opaquely — nothing dropped.
3. `Ap2Doc::parse(descrambled: &[u8]) -> Option<Ap2Doc>` — total over
   malformed input: every read bounds-checked, no panics (the caller is a
   hook-adjacent patch fn), `None` on any structural violation.
4. Read accessors needed by later steps: `exported_name()`,
   `find_sprite_by_label(label) -> Option<SpritePath>` (searches root +
   nested sprites), `max_character_id()`, label maps exposed per section.
5. The module must be **std-only / self-contained** (no `crate::` imports)
   so `scripts/validate_s_marvelous.sh` can mount it (it already auto-mounts
   `src/core/ap2/mod.rs` when present — verify submodule `#[path]` mounting
   works or inline the submodules into fewer files).
6. String-table cipher + BSI handling: implement the ~15-line rolling cipher
   locally (deviation from design "reuse core/afp.rs", forced by #5 —
   document the duplication in both files' doc comments, pointing at each
   other). Parser input is DESCRAMBLED data (the afp_patcher contract);
   scramble/descramble helpers here are for fixtures and dev validation.
7. Host tests (in-module `#[cfg(test)]`): parse a hand-built minimal AP2
   byte image (header + string table + one sprite + labels + opaque tags);
   malformed-input rejection (truncated header, out-of-range offsets,
   oversized tag sizes, label offset past table) — all `None`, no panics.
   Fixture BUILDERS may live in a `#[cfg(test)]` helper shared with task-02.

## Dependencies
- None on other tasks in this step (task-02 builds on the model).

## Implementation Approach
1. Write the model types + parse tests against hand-assembled byte images
   first (red), then the parser (green).
2. Keep every offset/width constant named and documented with its
   bemaniutils provenance (file/line in a comment) for future maintenance.

## Acceptance Criteria

1. **Structural parse**
   - Given a hand-built AP2 image with 2 frames, 3 tags (one DefineSprite
     containing a nested section with its own label), and a root label
   - When `Ap2Doc::parse` runs
   - Then the model reflects exact frame spans, tag order, both label maps,
     and the exported name

2. **Opaque carriage**
   - Given an image containing tags the model does not type
   - When parsed
   - Then those tags appear as `Opaque` with byte-exact payloads and correct
     ids

3. **Malformed-input totality**
   - Given truncated/corrupt variants of the fixture
   - When parsed
   - Then every variant returns `None` without panicking

4. **Harness mountability**
   - Given the module on disk
   - When `./scripts/validate_s_marvelous.sh` runs
   - Then the ap2 test suite compiles and passes in the temp harness

## Metadata
- **Complexity**: High
- **Labels**: s-marvelous, core-ap2, format-parser, pure-module
- **Required Skills**: Rust, binary format parsing, bemaniutils AP2 spec
- **Generated By**: code-task-generator 2026-08-29
- **Source Plan**: .agents/planning/2026-08-29-s-marvelous-judgement/implementation/plan.md
- **Plan Step**: Step 2
