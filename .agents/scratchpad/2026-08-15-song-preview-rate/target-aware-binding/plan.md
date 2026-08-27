# Plan: task-02 Target-aware Binding runtime

Status: Approved 2026-08-15 (auto mode — verified approved planning chain,
see context.md)

## Test scenarios

### T1 — Main-path regression pin (AC1)
The existing suites (binding_tests, generator_tests, xact tests, validator
sections) pass with unchanged asserted values after every cycle. Only call
spellings change (`prepare_binding` gains `StretchTarget::Main`;
`make_binding`/oracle helpers gain a target parameter defaulted by their
existing call sites).

### T2 — Side-target replay byte-identity, both DSP modes (AC2 + AC3 + AC4)
New `side_target_replay_matches_the_oracle` in generator_tests: for
`preview_first ∈ {false,true}` × `percent ∈ {50, 175}` × `preserve_pitch ∈
{true,false}`: build a Side-target binding over `replay_fixture`, drive
`replay_via_serve` (header read spanning regions, per-entry packet reads,
defensive EOF read), compare the assembled file against the generalized
whole-buffer oracle (side entry stretched/resampled, main entry verbatim).
Covers AC2 (byte identity), AC3 (the main entry's replayed bytes equal the
oracle's verbatim bytes), AC4 (header/boundary/EOF semantics).

### T3 — Verbatim main serves without production (AC3)
New `main_entry_prepare_read_completes_without_side_production`: on a
fresh Side-target binding (no producer steps), a main-entry-range read
serves synchronously; ring produced watermark still at the target start
(no production).

### T4 — Retire-under-read on a Side binding (AC5)
New `side_target_retire_cancels_pending`: a Side binding target-entry read
goes Pending (nothing produced), `retire()` cancels it (poll completes
with 0 bytes), subsequent serve refuses.

### T5 — prepare_binding target wiring
Extend an existing prepare_binding test (or add one) asserting a
Side-target `prepare_binding(..., StretchTarget::Side)` yields
`binding.rate()` == the SIDE entry's plan rate and serves the side entry
stretched (small smoke via one packet read pumped through the spawned
generator — or fold into T2 by building via prepare_binding where
practical; T2 uses Binding construction + GeneratorCore like the existing
replay tests, so T5 is a separate compact test through `prepare_binding` +
`generator::spawn`).

## Implementation shape (cycles)

1. **Cycle 1 — vocabulary refactor (Main semantics, no new capability):**
   binding.rs field renames + build derivation from
   `layout.target_entry_index`; serve/check/copy renames;
   `target_data_start/end`; `ms_to_blocks`/`active_content_grid`/regen
   guard → target; generator.rs accessor updates + doc comments. Gates:
   validator 218 green unchanged, windows check clean.
2. **Cycle 2 — `prepare_binding` target parameter:** plan through target,
   rate from target entry, identity `debug_assert!(target == Main)`;
   sweep 12 call sites with `Main`. Gates green unchanged.
3. **Cycle 3 — Side-target tests (T2–T5):** generalize
   `transform_bank_oracle_mode` + `make_binding` with a target parameter;
   write the four tests; implement whatever they flush out (expected:
   nothing beyond Cycles 1–2 — the refactor IS the capability; the tests
   are the proof against independent oracles).
4. **Cycle 4 — validate:** validator, windows check, whole-crate fmt,
   `./build.sh`.

## Risks

- The serve dispatch's `check_spans`/`copy_spans` symmetry: a missed
  rename produces a binding that ring-serves the verbatim entry (instant
  test failure against the oracle — this is exactly what T2 exists for).
- `pace_limit`'s `consumed.max(target_data_start())`: consumed starts at 0,
  so the max() anchor must move with the ring base — covered by T2 at
  50 % (multi-packet pacing).
- Mapping semantics: no preview test sets a mapping (previews never do);
  the existing mapping suites (target == main) pin the behavior.
