# Progress — task-01 core/ap2 model + parser

Updated: 2026-08-29

## Checklist

- [x] Read task file + design §4.1/§5.4/§7 + research afp-tooling §2/§4 +
      docs/afp_system.md §1–§2/§9 + src/core/afp.rs + harness script
- [x] Transcribe format facts from bemaniutils swf.py (provenance in context.md)
- [x] context.md / plan.md (byte-identity strategy + test scenarios)
- [x] Module skeleton: src/core/ap2/{mod,model,parse,write,fixtures,tests}.rs
- [x] Cipher duplicated locally; mirror comments in BOTH src/core/ap2/mod.rs
      and src/core/afp.rs
- [x] Tests written against hand-assembled byte images BEFORE the parser
- [x] Parser implemented (bounds-checked, Option-total, layout capture)
- [x] `pub mod ap2;` wired into src/core/mod.rs (alphabetical)
- [x] cargo fmt (whole crate) + cargo check --target x86_64-pc-windows-msvc clean
- [x] Harness green

## TDD record

1. RED — logs/01-red-stubs.log: parse/serialize stubbed to `None`; harness
   `exit=101`, 32 passed / 16 failed (the 16 = everything needing the real
   parse/serialize; the passing 32 = cipher, view decode, malformed-battery
   tests trivially satisfied by the stub, and the 10 pre-existing
   s_marvelous_state tests). Confirms harness `#[path]` submodule mounting
   works (task AC4 mechanics).
2. GREEN(parse) — logs/02-parse-implemented.log: task-01 suite green except
   `malformed_label_name_offset_past_table` — a TEST bug (I read the section
   header offsets wrong: name_reference_offset is @12, frame_offset @16,
   tags_offset @20). Fixed the test, not the parser. Remaining 12 failures
   all task-02 (write stub).
3. Final — see task-02 logs/04-final-green-post-fmt.log: 48 passed / 0 failed
   (38 ap2 + 10 s_marvelous_state); standalone ap2-only mount re-verified:
   38 passed / 0 failed.

## Test coverage delivered (task-01 half, 20 tests)

Structural parse (AC1), opaque carriage incl. non-zero padding (AC2), shape
typed decode, accessors (exported_name / find_sprite_by_label root+nested /
max_character_id / section navigation / label maps), exported-name offset-0
null string, malformed battery ×10 test fns (truncated header, bad
magic/version, length mismatch, string-table bounds/misalignment/64 KiB,
section pointer, tag-size overrun, frame/label arrays OOB, label name offset
past table, sprite subtag pointer, overlapping regions, truncated final-tag
padding — all `None`, no panics) (AC3), PlaceObject view decode ×3
(interleaved pre-matrix fields + realign catchup, second flag word, short
payload rejection), cipher ×2 (round trip + known vector matching
core/afp.rs semantics).

## Deviations

- Cipher implemented locally instead of reusing core/afp.rs (task Technical
  Requirement #6 sanctioned this — forced by the std-only harness-mount
  constraint; cross-pointer comments added in both files).
- `AP2_SHAPE` with size ≠ 4 degrades to `Opaque` carriage instead of failing
  the doc (bemaniutils raises; opaque carriage preserves totality and byte
  identity — size ≠ 4 does not occur in real files).
- Malformed nested DefineSprite sections fail the WHOLE document (strict
  `None`) rather than degrading to opaque — recursion is load-bearing for
  the editing API, and "None on any structural violation" is the task
  contract.
- `label_id` (flag 0x10), `unk3` (0x40), `blend` (0x20000) are decoded in
  `PlaceObjectView` beyond the task's minimum field list — they physically
  precede/straddle the modeled fields, so walking them is required anyway;
  exposing them costs nothing.
- Test-offset bug found during TDD (see TDD record #2) — fixed in tests.rs.

Status: Complete (uncommitted — maintainer commits manually)
