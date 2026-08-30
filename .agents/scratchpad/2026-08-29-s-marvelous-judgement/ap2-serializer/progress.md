# Progress — task-02 core/ap2 serializer + round-trip identity

Updated: 2026-08-29

## Checklist

- [x] Read task file + design §4.1/§7 + docs/afp_system.md §2/§9
- [x] context.md / plan.md (serialization rules, test scenarios)
- [x] Serializer tests written alongside task-01's suite with write.rs stubbed
- [x] `Ap2Doc::serialize` + `serialize_section` + `encode_tag`/`append_tag` +
      zone-delta pointer fixups implemented in src/core/ap2/write.rs
- [x] `PlaceObject::build` from-scratch encoder implemented
- [x] Fixture builder (`FixtureBuilder` + raw byte-image assemblers) shared
      with task-01 in src/core/ap2/fixtures.rs
- [x] cargo fmt (whole crate) + cargo check --target x86_64-pc-windows-msvc clean
- [x] Harness green

## TDD record

1. RED — task-01 logs/01-red-stubs.log covers this task too: all round-trip /
   limit / fixup / build tests failing against the `None` stubs.
2. logs/01-write-implemented.log: first real serializer run — 47 passed /
   1 failed. The failure was a TEST expectation bug, not a serializer bug:
   `roundtrip_builder_fixture_matrix` compared parsed vs built models with
   derived `PartialEq`, but parsed models legitimately carry layout metadata
   (`RegionSlot.orig_offset: Some`, captured zero-pad bytes) that built
   models lack. Byte identity itself passed everywhere. Fixed by adding
   `assert_section_semantics_eq` (recursive semantic comparison ignoring
   layout carriage).
3. GREEN — logs/02-green.log: 48 passed / 0 failed, harness exit 0.
4. logs/03-cargo-check-msvc.log: msvc check surfaced one `unused_imports`
   warning on the mod.rs `pub use` re-export (cdylib crates warn on
   externally-unused re-exports); fixed with the house
   `#[allow(unused_imports)]` re-export pattern (precedent:
   src/services/se_bank_synth/mod.rs). Re-check clean, zero warnings.
5. logs/04-final-green-post-fmt.log: post-`cargo fmt` re-run — 48 passed /
   0 failed (38 ap2 + 10 pre-existing s_marvelous_state).

## Test coverage delivered (task-02 half, 18 tests)

Round-trip byte identity: builder fixture matrix ×6 shapes (minimal, root
labels, nested+doubly-nested sprites, tag-pad coverage 0..9, PlaceObjects
across all modeled flags, multi-frame spans) with emit→parse→emit stability
AND parse-model semantic equality (AC1/req3); hand-assembled identity the
serializer never produced (full fixture, inter-region gaps + non-zero tag
padding + file middle/suffix junk, strings-first file order, sprite
pre/post-section slack bytes). Mutation stability (intern + opaque append +
label append + sprite append → reparse reflects, one-emission fixed point)
(AC2). Header pointer fixup (@40 shifts by exactly the region growth; suffix
bytes move intact; unmodified doc byte-identical). Limits fail closed (AC3):
frame start >20 bits (+boundary ok), frame count >12 bits (+boundary ok),
tag id >10 bits, tag size >22 bits, label count >u16, string-table intern
past 64 KiB, `StringTable::from_plain_bytes` size/alignment. PlaceObject
build→view round trip across each modeled flag alone/combined with sign
edges (negative tx/ty, i32::MIN/MAX scales) + byte-shape parity with
core/afp.rs `make_place_object` (req 5).

## Byte-identity strategy (constraint 5/6 decision — as approved in plan.md)

- **PlaceObject: opaque payload + decoded view + from-scratch encoder**
  (the constraint-6 fallback, chosen deliberately): the flag-conditional
  field order interleaves unmodeled fields (0x10/0x40/0x20000) around the
  modeled ones and skips realign catchup bytes of unspecified content —
  typed re-encode cannot guarantee byte identity. The feature only reads
  existing PlaceObjects and creates new ones.
- **String table: raw-bytes carriage + append-only interning** — existing
  offsets never move; header offset/size recomputed.
- **Sections: full recomputation guided by parse-time layout metadata**
  (region order, gap bytes, empty-region original offsets, per-tag pad
  bytes) so recomputed offsets land on the original values for unmodified
  docs; file level adds raw prefix/middle/suffix carriage + zone-delta
  fixups for the opaque header pointers (@40/@44/@56-iff-flags&4).

## Deviations

- `PlaceObject::build` is currently infallible but returns `Option` per the
  task contract (future fields may add failure modes).
- Serializer's string-table size/alignment check is defense-in-depth only —
  unreachable through the public API (StringTable construction/intern
  already enforce both), so it has no direct test; `from_plain_bytes` and
  `intern` limits are tested instead.
- `fixtures.rs` is `#[cfg(test)]`-gated. Task-03 note: the harness compiles
  it (tests run under cfg(test)); if task-03's dev legs need the builder
  outside the test cfg, lift the gate in src/core/ap2/mod.rs (one line) —
  the module is already written as ordinary pub code.

Status: Complete (uncommitted — maintainer commits manually)
