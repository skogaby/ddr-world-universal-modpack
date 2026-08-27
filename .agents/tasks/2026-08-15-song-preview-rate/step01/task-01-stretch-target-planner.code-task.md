# Task: StretchTarget parameterization of the virtual-bank planner

## Description

Add a `StretchTarget::{Main, Side}` parameter to the pure virtual-bank
planner so it can produce the INVERSE of today's plan: the side (`_s`
preview) entry stretched and the main entry verbatim. Today
`plan_virtual_bank` hardcodes main-entry-stretched + side-entry-verbatim
(the gameplay "preview passthrough" model). The song-select preview-rate
feature needs the inverse plan; gameplay plans must remain byte-identical.

## Background

The song-rate streaming engine synthesizes a "virtual" XWB whose header
advertises stretched entry values; the XACT engine parses that header once
at bank create and streams entry data through the detoured read callbacks.
For gameplay, the played entry is the MAIN one (named exactly like the
bank); the never-played `<code>_s` preview entry passes through verbatim.
At song select the roles invert: the game plays the `_s` entry and never
the main one.

`src/core/xact/virtual_bank.rs` is a pure module (host-tested, no game
dependencies). The identity rule — main entry = the entry whose name equals
the bank name, guaranteed unique by `parse_song_bank` — is unchanged; only
WHICH entry receives the stretched plan becomes a parameter.

The regression pin comes FIRST (TDD): snapshot-style tests proving
`StretchTarget::Main` plans are byte-identical to today's output on the
existing fixtures, written and passing against the pre-refactor code
(trivially, by calling the current API), then kept green through the
refactor.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-15-song-preview-rate/design/detailed-design.md
  (§Components 1 "Target-entry parameterization"; §Detailed Requirements R1, R13)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-15-song-preview-rate/research/engine-integration.md §2.1

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. New public enum in `src/core/xact/virtual_bank.rs`:
   `StretchTarget { Main, Side }` (Copy/Clone/PartialEq/Eq/Debug).
2. `plan_virtual_bank(source, percent, target: StretchTarget)` — the
   stretched plan (`plan_entry`) goes to the target entry, the verbatim
   plan (`passthrough_plan`) to the other. The `EntryRate` error-identity
   mapping follows the stretched entry.
3. `VirtualBankLayout` gains `target_entry_index: usize` (the stretched
   entry). `main_entry_index` keeps its current meaning (the identity
   rule). For `StretchTarget::Main`, `target_entry_index ==
   main_entry_index`; for `Side`, `target_entry_index == 1 −
   main_entry_index`.
4. `plan_identity_bank` is untouched in behavior; it sets
   `target_entry_index = main_entry_index` (both entries verbatim — the
   distinction is inert, but the field must be populated coherently for
   downstream consumers).
5. All existing callers (`src/services/song_rate/binding.rs`
   `prepare_binding`, plus any test callers) pass `StretchTarget::Main`
   explicitly in this task. Compilation must be clean for both host and
   `x86_64-pc-windows-msvc` targets.
6. Side-plan fixtures must have durations that are NOT block-exact (the
   repo's "honest fixtures" rule — a duration inside its final ADPCM block,
   like real banks).
7. Loop metadata on the side entry must map through the existing `map_loop`
   (half-up, one-frame clamp) when `target == Side`, exactly as main-entry
   loops do today.
8. No behavior change to `plan_entry` / `plan_entry_values` / `map_loop`
   themselves.

## Dependencies

- None (first task of the step; pure module).

## Implementation Approach

1. Write the regression pin first: a test that runs the CURRENT
   `plan_virtual_bank` on representative fixtures (both entry orders —
   main-first and main-second; with and without loops; 75 % and 125 %) and
   asserts the full layout surface (entry plans, offsets, `pre_data` bytes,
   `virtual_size`). Land it green before touching the signature.
2. Add `StretchTarget` + the parameter; thread `target_entry_index` through
   `VirtualBankLayout` construction in both plan functions.
3. Update callers to pass `StretchTarget::Main`; re-run the pin unchanged
   (only the call-site spelling in the pin may change — the asserted VALUES
   must not).
4. Add Side-plan tests (see Acceptance Criteria).
5. Run the readiness gates relevant to a pure change: `cargo test`,
   `cargo check --target x86_64-pc-windows-msvc`, whole-crate `cargo fmt`,
   and `./scripts/validate_song_playback_speed.sh`.

## Acceptance Criteria

1. **Main-target regression pin**
   - Given the existing fixtures and rates exercised by today's tests
   - When `plan_virtual_bank(source, percent, StretchTarget::Main)` runs
   - Then every field of the resulting layout (entry plans, entry offsets,
     pre-data bytes, virtual size, `main_entry_index`) is byte-identical to
     the pre-refactor output, and `target_entry_index == main_entry_index`

2. **Side-target inverse plan**
   - Given a fixture bank with a non-block-exact side-entry duration, in
     BOTH physical entry orders
   - When `plan_virtual_bank(source, 75, StretchTarget::Side)` runs
   - Then the side entry carries the stretched plan (duration/data_len/rate
     matching `plan_entry_values` for its metadata) and the MAIN entry
     carries stock values (`passthrough_plan`), with
     `target_entry_index == 1 − main_entry_index`, correct 2048-aligned
     offsets, and a pre-data block the `xwb` parser round-trips

3. **Side-entry loop mapping**
   - Given a side entry with an interior loop region
   - When planned at `StretchTarget::Side` and a non-100 rate
   - Then the side entry's mapped loop matches `map_loop`'s half-up values
     and carries a `LoopContext`, while the main entry's stock loop values
     pass through untouched

4. **Rate-refusal identity follows the target**
   - Given a side entry whose stretched duration exceeds the 28-bit XWB
     ceiling at 25 %
   - When planned at `StretchTarget::Side`
   - Then the error is `PlanError::EntryRate { index }` naming the SIDE
     entry's physical index

5. **Identity plan coherence**
   - Given any valid fixture
   - When `plan_identity_bank` runs
   - Then behavior is unchanged from today and `target_entry_index ==
     main_entry_index`

## Metadata

- **Complexity**: Medium
- **Labels**: song-rate, xact, planner, pure, refactor
- **Required Skills**: Rust, XWB format familiarity (from the design/research docs)
- **Generated By**: code-task-generator 2026-08-15
- **Source Plan**: .agents/planning/2026-08-15-song-preview-rate/implementation/plan.md
- **Plan Step**: Step 1: Target-entry parameterization (planner + binding)
