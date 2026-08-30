# Plan — task-02 core/ap2 serializer + round-trip identity

Status: Approved 2026-08-29 (auto mode — verified upstream approval chain)

## Byte-identity strategy

Shared with task-01 — full write-up in
`.agents/scratchpad/2026-08-29-s-marvelous-judgement/ap2-model-parser/plan.md`
("Byte-identity strategy" section). Summary of the serializer's half:

- Every offset/length/count is RECOMPUTED from the model; the parse-time layout
  metadata (region order, gap bytes, empty-region offsets, tag pad bytes, raw
  prefix/middle/suffix carriage, raw string-table bytes) makes the recomputation
  land on the original values for unmodified docs — so identity is a theorem of
  the emission algorithm, not a cached-bytes shortcut.
- PlaceObject: opaque payload carriage + from-scratch `PlaceObject::build()`
  encoder for new tags (constraint-6 fallback; justification in the task-01 plan).

## Implementation shape

`src/core/ap2/write.rs`:
- `serialize_section(&TagSection) -> Option<Vec<u8>>` — one offset-fixup walk:
  build region payloads (frames / tags / name-refs, each width-validated), then
  emit gaps+regions in the recorded order, recording each region's landing
  offset; patch the 24-byte header last.
- `encode_tag(&Tag) -> Option<(u16, Vec<u8>, &pad)>` — recursion happens here
  for DefineSprite (new-style pointer reproduced as `8 + pre_section.len()`).
- `Ap2Doc::serialize()` — root section + string table + raw carriage assembled
  in the recorded file order; header fields @4/@36/@48/@52 recomputed; opaque
  pointers @40/@44/(@56 iff flags&0x4) shifted by zone delta.
- `PlaceObject::build(&PlaceObjectParams) -> Option<PlaceObject>` — emits the
  modeled flags only (0x2, 0x20, 0x100, 0x200, 0x400) in bemaniutils read
  order with the mid-payload realign.

## Test scenarios (written before the serializer implementation)

1. **byte-identity round trip (AC1)** — for a matrix of builder fixtures
   (minimal doc; labels; nested sprites with own labels/frames; opaque tags;
   PlaceObjects covering each modeled flag; multi-frame spans):
   `b = doc.serialize()`, `parse(b).serialize() == b`.
2. **hand-assembled identity** — images the serializer did NOT produce:
   inter-region gap bytes, non-zero tag padding, string table before the tag
   section (StringsFirst order), trailing file suffix bytes — all must
   round-trip byte-identically (`serialize(parse(x)) == x`).
3. **mutation stability (AC2)** — parse a fixture, mutate (intern new string,
   append opaque tag, add label, append sprite), serialize; re-parse must
   reflect the mutation and `serialize(parse(serialize(m))) == serialize(m)`
   (fixed point after one emission).
4. **pointer fixups** — image with an opaque suffix region referenced by header
   @40; after a mutation that grows the tag section, @40 must shift by exactly
   the suffix delta; unmodified doc leaves it untouched.
5. **limits fail closed (AC3)** — frame start > 20 bits, frame count > 12 bits,
   tag size > 22 bits (oversized opaque payload), tag id > 0x3FF, label count
   > u16, string-table intern past 64 KiB (returns None) — serialize/`intern`
   return `None`, no panic.
6. **PlaceObject encode/decode (AC-req 5)** — `build()` → `view()` round-trip
   across each modeled flag alone and combined, with sign edges (negative
   tx/ty, i32::MIN/MAX scales); hand-crafted payload with pre-matrix extras
   (0x10 label, 0x40 unk3, 0x20000 blend + realign catchup) decodes the
   modeled fields correctly and carries opaquely byte-exact.
7. **harness green (AC4)** — all via `./scripts/validate_s_marvelous.sh`.

TDD order: serializer tests land alongside the task-01 suite with `write.rs`
stubbed (`None`) → red run (logged) → implement → green run (logged).
