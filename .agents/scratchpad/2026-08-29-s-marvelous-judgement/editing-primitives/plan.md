# Plan — step03 task-01 core/ap2 editing primitives

Status: Approved 2026-08-29 (auto mode — verified upstream approval chain)

## Shape

New submodule `src/core/ap2/edit.rs` (std-only, zero `crate::` imports like the
rest of the tree), `pub mod edit;` in `src/core/ap2/mod.rs` + re-exports added
to the house `#[allow(unused_imports)] pub use` block. All primitives are
`impl Ap2Doc` methods (they need `strings` and/or doc-wide id allocation, so
`TagSection` alone cannot host them); section addressing reuses the existing
`SpritePath`.

### Public API (exact signatures)

```rust
pub type TagRemap = std::collections::HashMap<u16, u16>;

pub struct NamedPlacement<'a> {
    pub frame: usize,                  // target frame index in the section
    pub depth: u16,                    // must be unique in that frame
    pub object_id: u16,
    pub source_tag_id: u16,            // character to place (create mode)
    pub instance_name: &'a str,        // interned by the primitive
    pub translate: Option<(i32, i32)>, // raw fixed-point /20
}

impl Ap2Doc {
    pub fn add_label(&mut self, path: &SpritePath, name: &str, frame: u16) -> Option<()>;
    pub fn add_shape(&mut self, path: &SpritePath, frame: usize, unknown: u16) -> Option<u16>;
    pub fn clone_labeled_segment(&mut self, path: &SpritePath, src_label: &str,
                                 new_label: &str, remap: &TagRemap) -> Option<()>;
    pub fn add_place_object_named(&mut self, path: &SpritePath,
                                  placement: &NamedPlacement) -> Option<()>;
    pub fn adjust_placements(&mut self, pred: impl Fn(&PlaceObjectView) -> bool,
                             dxy: (i32, i32)) -> usize;
}
```

Divergences from the task's sketch, all "adapt to the model's idioms":

- `add_shape` returns `Option<u16>` (not bare `u16`) — requirement 2 ("every
  primitive returns Option/count, no panics") + real failure modes (bad path /
  frame, character-id space exhausted at `u16::MAX`). It also takes an explicit
  target FRAME: definitions must live inside an executed frame span and must
  precede referencing PlaceObjects in the tag list (docs/afp_system.md §9); the
  shipped `core/afp.rs::patch_inject_children` precedent inserts definitions
  into frame 0's span for exactly this reason. A bare tag-list append would
  land AFTER any PlaceObject later inserted into an earlier frame.
- `add_place_object_named` takes a dedicated `NamedPlacement` (not the model's
  `PlaceObjectParams`): the primitive needs the target frame and the instance
  name STRING (it interns), while `PlaceObjectParams` carries a raw
  already-interned offset and no frame.
- `adjust_placements` walks the WHOLE doc recursively (root + nested sprites),
  per the design signature (no path parameter); the predicate scopes.

### The mutate-in-place decision (task requirement 1, last bullet)

`adjust_placements` does NOT rebuild via `PlaceObject::build` and does NOT
constrain itself to fully-modeled payloads. Instead it splices the new
`tx/ty` pair into a fresh copy of the payload at the field's byte offset,
which is fully determined by the flag word (the field-order walk `view()`
implements: prefix 8 (+4 if flag 0x8000_0000), +2 per 0x2/0x10/0x20/0x40,
+1 for 0x20000, realign to 4, +8 per 0x100/0x200, then 0x400 tx/ty).

Justification: a `build` rebuild only re-emits the modeled flags — it would
silently DROP unmodeled fields (0x10 label_id, 0x40 unk3, 0x20000 blend, and
the whole 0x800+ color/event/filter tail). The results-tab row placements this
API exists for are real-template tags that carry such fields. The
constrain-to-fully-modeled fallback the task offers would make the primitive
useless on those same tags. The splice achieves byte-exactness of every
unmodeled byte by construction: only the 8 translate bytes change. The model's
"never mutated in place" doctrine protects unmodified-doc byte identity;
`adjust_placements` is deliberately modifying these tags, and the splice is
the minimal possible modification. Constraint kept: tags matching `pred` but
LACKING flag 0x400 (no translate field) are skipped and not counted —
retro-fitting the field would restructure the payload mid-stream and shift the
opaque tail, whose internal invariants are unproven. Documented limitation,
log-free; the caller compares the returned count and WARNs.

The same offset math powers `clone_labeled_segment`'s remap: the cloned
PlaceObject's `source_tag_id` (first conditional field) is spliced in the
CLONE (2 bytes), preserving the marvelous fade animation's color/matrix tail
that a view+`build` rebuild would destroy. The task's "(via view + rebuild)"
parenthetical is implemented as "via the view walk + surgical splice" — same
sanctioned decode, strictly higher fidelity; deviation recorded in
progress.md.

### Shared helper + segment-boundary rule

`insert_tags_at_frame_end(sec, frame, new_tags)` — ONE fixup implementation
(task Implementation Approach #2): insert at tag index `start + count` of the
target frame, grow that frame's `tag_count`, shift `start_tag` of every OTHER
frame with `start_tag >= insert_index`. Labels reference frame numbers, so
they are structurally stable under tag inserts. (Same rule as
`patch_inject_children`; frames whose span STRADDLES the insertion point —
possible only with overlapping spans, which real consecutive-span files don't
have — keep their count and are documented as out of contract.)

Segment rule (documented beside the helper): a labeled segment spans frames
`[label_frame, next_label_frame)` where `next_label_frame` is the smallest
label frame strictly greater, or the section frame count when none. Cloned
tags = the contiguous tag range `[lo, hi)` covered by the segment's non-empty
frame spans, appended at the tag-list end; cloned frames append at the frame
end with `start_tag` shifted by `(tags.len() - lo)`; the new label points at
the first appended frame (index must fit u16 — validated).

### Failure atomicity (requirement 2/3)

Validate-then-mutate ordering in every primitive: all read-only validation
first (path, frame bounds, span bounds, duplicate label, depth uniqueness,
remap decodability — including a pre-flight `PlaceObject::build` with a
placeholder offset where applicable), THEN the single fallible mutation
(`strings.intern`, which leaves the table untouched on its own failure), THEN
infallible pushes. No panics; packed-field width overflows produced by edits
are caught by `serialize()` returning `None` with the doc intact
(requirement 6 — already covered by Step 2's limit tests).

## Test scenarios (tests.rs, TDD red → green)

1. `edit_add_label_*`: success on root + nested (resolves via `label_frame`,
   interned string readable, round-trips); duplicate name → None + byte-
   identical; unknown path → None; frame out of range → None.
2. `edit_add_shape_*`: returns `max_character_id()+1`, tag lands inside the
   target frame's span, later frames shift, round-trips; nested-sprite id
   contributes to allocation; id space exhausted (existing id `u16::MAX`) →
   None; bad path/frame → None + unchanged.
3. `edit_clone_*` (AC1): fixture sprite with labels A(frames 0..2)/B(3..4) —
   clone A as C with remap {shapeX→shapeY}: frames 5..7 are span-correct
   copies, label C→5, A/B unchanged, cloned PlaceObject views show shapeY,
   non-remapped ids stay shared, serialize→parse round-trip. Segment at the
   section END (no next label). Shape-tag id remap inside a segment. Clone
   of a PlaceObject with unmodeled fields (blend + junk realign + opaque
   tail) — byte-identical except the 2 id bytes. Empty remap → verbatim
   clones. Failures (missing src label, duplicate new label, bad path) →
   None + byte-identical.
4. `edit_add_place_object_named_*` (AC2): fixture with depths 1..3 in frame 0
   + a later frame — fresh depth inserts inside frame 0's span, later span
   shifts, name resolves in the string table, view shows translate,
   round-trips. Occupied depth → None + unchanged; bad frame/path → None.
5. `edit_adjust_placements_*`: predicate scoping (depth/object-id subset) —
   count correct, matched translates moved by dxy, unmatched byte-identical,
   round-trips; matched-but-no-translate skipped (count excludes); no match →
   0 + byte-identical doc; nested-section placements reached; unmodeled-field
   payload — only the 8 translate bytes differ.
6. `edit_integration_smarvelous_shape` (AC "integration-style"): in_marvelous-
   shaped fixture → `add_shape` + `clone_labeled_segment` with remap
   {old→new} → clone's PlaceObjects reference the new shape id, label map
   complete, serialize→parse→re-serialize fixed point.

Failure tests assert doc-unchanged via serialize-bytes equality against the
pre-call serialization (AC3).

## Validation

`./scripts/validate_s_marvelous.sh` (synthetic suites + Leg A/B dev legs must
stay green — baseline log: logs/00-baseline.log, 48 tests + 76 templates
byte-identical + Leg B match); `cargo check --target x86_64-pc-windows-msvc`;
`cargo fmt` (whole crate) at the end.
