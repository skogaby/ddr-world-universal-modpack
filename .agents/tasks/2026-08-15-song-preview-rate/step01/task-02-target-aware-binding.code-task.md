# Task: Target-aware Binding runtime (ring, serving, generator)

## Description

Generalize the streaming binding runtime so the ring/producer machinery
follows the plan's TARGET (stretched) entry and the other entry is served
verbatim from the private source copy — regardless of which physical entry
is which. Today `Binding` and the generator are hardwired to
"main = stretched (ring-served), side = verbatim": ring base, serve
dispatch, rate selection, and the generator's cursor all reference
`main_entry_index`. After this task, a `StretchTarget::Side` layout
produces a binding that streams the stretched `_s` entry through the ring
while the (never-played) main entry serves verbatim — the preview binding
the song-preview-rate feature publishes.

## Background

`src/services/song_rate/binding.rs` holds the runtime state of one bound
bank: the planned `VirtualBankLayout`, a private copy of the stock XWB
(the generator's source AND the verbatim entry's serving bytes), a bounded
ring the producer fills with the stretched entry's generated stream,
pending-slot deferral, and the retire/reader epoch lifecycle.
`src/services/song_rate/generator.rs` is the producer thread: it decodes
the target entry's source ADPCM, runs WSOLA or the resampler
(`DspState::{Wsola, Resample}` selected by `Binding.preserve_pitch`), and
appends encoded blocks to the ring.

Key structural facts (verified in code):

- `Binding::build` derives `side_entry = 1 − layout.main_entry_index`,
  captures `side_source_offset` (verbatim serving base) and
  `main_source_offset`/`main_source_len` (identity-passthrough base), and
  bases the ring at `layout.entry_offsets[main_entry_index]`.
- The serve dispatch routes `Region::EntryData { entry == side_entry }` to
  verbatim source copy-out and the other entry to ring/serve-mode logic
  (`binding.rs` around lines 1560–1780).
- `prepare_binding` picks the binding's `rate` from
  `layout.entries[main_entry_index].rate` and passes the layout through.
- The generator's cursor and regeneration logic consume
  `binding.main_data_start()/main_data_end()` and
  `layout().main_entry_index` (`generator.rs` around lines 426–590).
- The content mapping (training-mode pre-shift/seeks) operates on the
  ring-served entry's block grid.

Per the approved breakdown decisions:

1. `percent == 100` with `StretchTarget::Side` is unreachable by design
   (preview qualification requires ≠ 100; the identity plan has no target
   distinction): `debug_assert!` + documented fall-through to the identity
   plan.
2. Content mapping generalizes naturally: it applies to the TARGET entry's
   grid (previews never set one; no refusal path added).
3. Internal `Binding` fields rename to `target_*` (ring-served) /
   `verbatim_*` (source-served); `layout.main_entry_index` (the identity
   rule) and `layout.target_entry_index` (from task 01) are the inputs.

The existing test suites (`binding_tests.rs`, `generator_tests.rs`, the
serve-dispatch oracles, identity-passthrough and mapping suites) are the
de-facto byte-identity regression pin for the Main path — they must pass
unchanged in asserted values.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-15-song-preview-rate/design/detailed-design.md
  (§Components 2 "Target-aware bindings"; §Detailed Requirements R1, R13; §Testing Strategy item 2)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-15-song-preview-rate/research/engine-integration.md §2.2

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `prepare_binding` gains `target: StretchTarget` (from task 01) and:
   - plans through `plan_virtual_bank(&bank, percent, target)`;
   - takes `rate` from `layout.entries[layout.target_entry_index].rate`;
   - `percent == 100` keeps the identity-passthrough path exactly as today
     (`debug_assert!(target == StretchTarget::Main)` + doc comment).
2. `Binding::build` derives everything from `layout.target_entry_index`:
   - ring base = `layout.entry_offsets[target_entry_index]`;
   - verbatim entry = `1 − target_entry_index`, with its source offset as
     the verbatim serving base;
   - the identity-passthrough length guard applies to whichever entries are
     verbatim-served (unchanged for identity: both);
   - internal fields renamed `target_*` / `verbatim_*` per the approved
     naming decision.
3. Serve dispatch: `Region::EntryData` routing keys on
   `entry == target_entry_index` (ring / serve-mode path) vs. the verbatim
   entry (direct source copy-out). Gap/pre-data/EOF semantics unchanged.
4. Regeneration targets, the pace limit, silent-block selection, and the
   content mapping all follow the target entry.
5. `generator.rs`: cursor bounds and entry selection consume the renamed
   target-relative accessors (`target_data_start()/target_data_end()`,
   `layout().target_entry_index`); DSP-mode selection unchanged.
6. All existing gameplay/training callers pass `StretchTarget::Main`;
   behavior (including `new_identity_passthrough` and every fault leg) is
   observably unchanged.
7. Both cfg targets compile clean (`cargo check` host and
   `--target x86_64-pc-windows-msvc`).

## Dependencies

- task-01-stretch-target-planner (provides `StretchTarget` and
  `VirtualBankLayout::target_entry_index`).

## Implementation Approach

1. Confirm the existing Main-path suites pass on the current tree (the
   pin baseline), then refactor `Binding::build` + the serve dispatch to
   the target/verbatim vocabulary with `StretchTarget::Main` semantics —
   suites must stay green with unchanged asserted values.
2. Thread the `target` parameter through `prepare_binding`; update callers
   (`wavebank_hook.rs` bind closure and test harnesses) with
   `StretchTarget::Main`.
3. Rename/redirect the generator's accessors; re-run generator suites.
4. Add the Side-target suites (Acceptance Criteria 2–5): construct
   bindings over Side-target layouts, drive the serve dispatch as the
   engine would (header read, target-entry packet reads from offset 0,
   verbatim-entry reads), compare against a whole-buffer oracle.
5. Readiness gates: `cargo test`, `cargo check --target
   x86_64-pc-windows-msvc`, whole-crate `cargo fmt`,
   `./scripts/validate_song_playback_speed.sh`, `./build.sh`.

## Acceptance Criteria

1. **Main-path regression pin**
   - Given the existing binding, generator, serve-dispatch,
     identity-passthrough, mapping, and fault-leg test suites
   - When the refactored code runs them
   - Then every suite passes with unchanged asserted values, and the
     song-rate validation script passes

2. **Side-target serve byte-identity (both DSP modes)**
   - Given a Side-target binding over a fixture bank at 50 % and at 175 %,
     once with `preserve_pitch = true` and once with `false`
   - When the target (`_s`) entry's virtual range is read to EOF through
     the serve/poll dispatch (engine-shaped chunked reads, including
     deferral/retry on `Pending`)
   - Then the assembled bytes equal the whole-buffer oracle (frozen WSOLA
     reference / frozen resampler reference) for that entry's stretched
     stream, and the advertised data length matches the plan

3. **Verbatim main-entry serving**
   - Given the same Side-target binding
   - When the MAIN entry's virtual range is read through the dispatch
   - Then the bytes are byte-identical to the stock main entry's data in
     the source copy, served without producer involvement (no ring
     production observed for that range)

4. **Header and boundary behavior**
   - Given a Side-target binding
   - When the engine-shaped 0x1000 header read and reads spanning
     pre-data/entry/gap/EOF boundaries are issued
   - Then spans resolve per the existing region semantics with the ring
     range following the target entry (EOF clamp, gap zero-fill, repeated
     partial serves)

5. **Retire-under-read unchanged**
   - Given a Side-target binding with an in-flight read
   - When the binding is retired (unregister semantics)
   - Then the dispatch returns `Refused` and reclamation proceeds exactly
     as the existing retire tests specify

## Metadata

- **Complexity**: High
- **Labels**: song-rate, binding, generator, streaming, refactor
- **Required Skills**: Rust (atomics, threads), the song-rate streaming architecture (design + engine-integration research)
- **Generated By**: code-task-generator 2026-08-15
- **Source Plan**: .agents/planning/2026-08-15-song-preview-rate/implementation/plan.md
- **Plan Step**: Step 1: Target-entry parameterization (planner + binding)
