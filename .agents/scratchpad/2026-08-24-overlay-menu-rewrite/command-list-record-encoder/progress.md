# Progress — command-list-record-encoder

Status: Complete (uncommitted — maintainer commits manually)

## Checklist

- [x] `src/services/overlay_draw/encode.rs` — dependency-free `RecordWriter`
      (scissor 0x0C, SetShader 0x13, SetVSConstantF 0x14, SetTexture 0x11,
      untextured quads 0x03) + `walk()` size-chain validator + `Quad`
- [x] `src/services/overlay_draw/mod.rs` + `pub mod overlay_draw;` in services
- [x] `scripts/validate_overlay_draw.sh` (temp-crate host harness,
      validate_judgement_offsets.sh pattern) — **11 tests pass**
- [x] Layout verification beyond the docs: decompiled the 20260616 walker handlers in
      the live Ghidra project — `FUN_180268090` (tag 0x03: 0x04-shaped header,
      perimeter corner order expanded (p0,p1,p2)(p2,p3,p0), color dword verbatim,
      2D-context transform applied) and `FUN_180269080` (tag 0x0C: u16 fields at
      +4..+0xC). Facts recorded in context.md and the module docs.
- [x] `cargo check` (0 warnings) → `cargo fmt` (no churn) → `./build.sh` clean

## Deviations

- TDD sequencing: tests and implementation were authored in one pass rather than
  stub-fail-implement — for a byte-layout transcription module the test vectors ARE the
  spec transcription, and the failing-first run would only have exercised `todo!()`
  panics. Recorded per SOP; the harness run + walk() negative tests carry the proof.

## Notes for task-02 (emitter)

- Three record types carry ABSOLUTE in-arena payload pointers → the emitter must
  reserve the arena block first (bump `cl+0x0C`/`+0x10` once for the whole batch),
  then `RecordWriter::new(cmd_addr)` and memcpy `bytes()` to `cmd_addr`.
- Tag 0x03 coordinates pass through the walker's CURRENT 2D-context transform
  (tag 0x07 state) — the POC's quad placement may inherit whatever context the
  active list last set; if placement looks wrong, emit a tag 0x07 context reset
  first (documented unknown for the diagnostics to resolve).
