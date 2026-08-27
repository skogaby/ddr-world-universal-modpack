# Plan: task-01 StretchTarget parameterization

Status: Approved 2026-08-15 (auto mode — stands on the verified approved
planning chain recorded in context.md, per the code-assist sop)

## Test scenarios (written first, per criterion)

All in `src/core/xact/tests.rs` unless noted; both physical entry orders
(`preview_first ∈ {false, true}`) wherever entry identity matters.

### T1 — Main-target regression pin (AC1)
New test `stretch_target_main_reproduces_the_shipped_plan`: for
`partial_tail_bank` (honest durations) and `build_bank`, at 75 % and 125 %,
assert the FULL layout surface of
`plan_virtual_bank(&bank, pct, StretchTarget::Main)` — per-entry
`streamed` values computed independently (`plan_entry_values` for the main
entry, stock values for the side entry), `rate`, offsets, `pre_data`
against the serializer oracle, `virtual_size`, `main_entry_index`, and
`target_entry_index == main_entry_index`.
Baseline: before the refactor these values are produced by the CURRENT
two-argument API — the same assertions are first captured against it (see
Cycle 1) so the refactor demonstrably preserves them.
Additionally: the ~19 existing call sites keep asserting their current
values with only the call spelling changed.

### T2 — Side-target inverse plan (AC2)
`stretch_target_side_inverts_the_plan`: `partial_tail_bank` (both orders)
at 75 %: side entry's `streamed` equals
`plan_entry_values(side_index, side_format, side_duration, …, 75)`
(stretched, block-quantized, honest tail), main entry's `streamed` equals
stock values with `RateRatio::IDENTITY` and no loop context;
`target_entry_index == 1 − main_entry_index`; offsets 2048-aligned per the
serializer; completed virtual bytes reparse (`parse_song_bank`) carrying
the stretched side metadata (reuse the `streamed_bank_bytes` oracle +
completion pattern from the existing pre-data test).

### T3 — Side-entry loop mapping (AC3)
`stretch_target_side_maps_the_side_loop`: `build_bank_bytes` with an
interior loop on the SIDE entry (e.g. start 128, length 768 over a
1024-frame side entry — the known 75 % vector: 176/1056) and a distinct
stock loop on the main entry. At `Side`/75 %: side plan carries the mapped
values + `LoopContext`; main plan carries its stock loop untouched and
`loop_context == None`.

### T4 — Rate-refusal identity follows the target (AC4)
`stretch_target_side_refusals_name_the_side_entry`: the existing
28-bit-ceiling construction (`build_bank_with_data_lengths` with
`CEILING_BLOCKS` on the SIDE entry) at `Side`/25 % refuses
`PlanError::EntryRate { index: side_physical_index, .. }`; the MAIN entry
at the ceiling passes through verbatim under `Side` (mirror of the
existing side-passthrough assertion).

### T5 — Identity plan coherence (AC5)
Extend the existing identity-plan tests with
`target_entry_index == main_entry_index` assertions; no other change.

## Implementation shape

1. `pub enum StretchTarget { Main, Side }` (+ derive set) with a
   doc-comment carrying the design rationale (gameplay vs preview roles).
2. `VirtualBankLayout` gains `pub target_entry_index: usize` (documented:
   the stretched, ring-served entry).
3. `plan_virtual_bank(source, percent, target)`:
   `target_entry_index = match target { Main => main, Side => 1 − main }`;
   `plan_indexed` keys on `index == target_entry_index`; the
   `EntryRate` mapping moves with it. Struct literal gains the field.
4. `plan_identity_bank`: field = `main_entry_index`; behavior untouched.
5. Update the module doc-comment (the "MAIN (played) entry" language
   becomes target-aware; note the preview inversion + design date).
6. Call-site sweep: `binding.rs:238` + the three test files + the
   validator script's two embedded call sites → `StretchTarget::Main`
   (script imports the enum through its existing `#[path]` harness).

## Cycle order (TDD)

- Cycle 1 (red→green on the pin): write T1 asserting against the CURRENT
  API spelling (two-arg) — green immediately (captures the baseline);
  then change the API (compile-red across call sites = the "failing test"
  edge for a pure refactor), update spellings, T1 green with identical
  values.
- Cycle 2: T2 (red: `Side` arm not yet implemented / wrong values) →
  implement the target selection → green.
- Cycle 3: T3, T4 (red on missing loop/refusal identity behavior if any) →
  green.
- Cycle 4: T5 + doc comments + fmt.

## Risks

- The validator script embeds Rust calling `plan_virtual_bank` — forgetting
  it breaks the validation gate, not the build. Sweep it explicitly and run
  the script in Validate.
- `binding.rs` consumes `layout.main_entry_index` internally (task-02's
  scope): this task must NOT change any of that logic — only the
  construction call spelling.
