# Task: Virtual Bank Layout, Pre-Data Synthesis, and Region Resolve

## Description

Fill `src/core/xact/virtual_bank.rs` to its design shape: `plan_virtual_bank`
producing a `VirtualBankLayout` (both entries planned in physical order, the
main-entry index, the synthesized pre-data block, per-entry virtual data
offsets, and the virtual size), plus `resolve(offset, len)` mapping any
virtual file offset to a serving region with the stock EOF clamp. This is the
pure layout/header/mapping layer of plan Step 3 (design reqs 12–14); the
engine-replay harness (next task) drives it.

## Background

Step 1 relocated the per-entry planning logic (`plan_entry`,
`plan_entry_values`, `map_loop` with the half-up one-frame-clamp rule and the
28-bit refusal via `rate::target_for_percent`) into this module as a stub.
Step 3 composes it into the whole-bank layout the read detour will serve in
Step 4: offsets below the wave-data offset (2048) come from a synthesized
stock-shaped pre-data block; data-region offsets map through the virtual
layout (entry 0 data at 2048; entry 1 at the next 2048-aligned offset;
inter-entry gap zero-filled); ALL reads clamp to
`min(len, virtual_size − offset)` — the exact stock EOF contract.

The pre-data block MUST be emitted by the same canonical layout code
`core::xact::xwb`'s streaming serializer uses (design req 13: the engine
already parses that exact layout in the proven pipeline). The emission path
lives in xwb.rs's private `write_stream_header` / `validate_stream_write_layout`;
factor it for reuse rather than duplicating it — approved at breakdown, with
the serializer's own suites passing unmodified as the guard.

One read can SPAN regions: the engine's single header read is 0x1000 bytes at
offset 0, which covers the 2048-byte pre-data block AND the start of entry 0's
data. The resolve surface must let a caller serve a spanning read by
iterating: region at `offset` plus the contiguous length available within it.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (§`core::xact::virtual_bank — layout, plan, and header synthesis`; reqs
  12–14; Appendix: the canonical pre-data layout and engine parse facts)

**Additional References (if relevant to this task):**
- `docs/xact_streaming_research.md` — engine header-parse rules (WBND /
  version 42 / segment table from the single 0x1000 read; pre-data ≤ 0x1000)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `plan_virtual_bank(source: &xwb::SongBank<'_>, percent: u32) ->
   Result<VirtualBankLayout, PlanError>`: plans BOTH entries via the existing
   `plan_entry` (per-entry refusals — 28-bit ceiling, unmappable loops —
   propagate with their entry identity), preserves the source bank's physical
   entry order, and records `main_entry_index` (the entry named exactly like
   the bank).
2. `VirtualBankLayout` carries: the two `EntryPlan`s, per-entry virtual data
   offsets (entry 0 at virtual offset 2048; entry 1 at
   2048 + round_up(entry0.data_len, 2048)), the synthesized pre-data block
   (exactly the first 2048 bytes of the virtual file), `main_entry_index`,
   and `virtual_size` (= 2048 + entry1 offset-within-segment + entry1
   data_len; segment 4 ends exactly at `virtual_size`).
3. Pre-data synthesis reuses xwb.rs's canonical streaming emission: factor
   the private header/layout writer so both the serializer and
   `plan_virtual_bank` call one code path. The refactor must leave
   `write_song_bank_streaming` / `serialize_song_bank` byte-identical (their
   existing suites pass unmodified).
4. `resolve(offset, len)` (exact field shapes free per the sketch-elision
   rule; behavior binding): returns the region containing `offset` — pre-data
   (with block offset), entry data (entry index + offset within the entry's
   generated stream), gap (zero fill), or EOF — together with the contiguous
   byte count servable from that region, after clamping the request to
   `min(len, virtual_size − offset)`. Reads at or past `virtual_size` resolve
   to EOF (zero bytes). Spanning reads are served by repeated calls.
5. The 28-bit duration refusal and loop-mapping refusals remain exactly the
   Step-1 stub's (`rate::target_for_percent` + `map_loop`); `plan_virtual_bank`
   adds no new numeric policy.
6. Tests live in `src/core/xact/tests.rs` (module-suite convention), running
   through the validator harness's cargo-test phase.

## Dependencies

- None within Step 3 (first task; Step 2 is complete). Blocks
  `task-02-synthetic-engine-replay-harness`.

## Implementation Approach

1. Factor the pre-data emission out of xwb.rs (layout + header write into a
   buffer) and re-run the serializer suites untouched before building on it.
2. Compose `plan_virtual_bank` from `plan_entry` + the factored emission;
   then `resolve` as pure offset arithmetic over the layout.
3. Property-test reconstruction: serve every byte of `[0, virtual_size)`
   through `resolve` (pre-data bytes from the block, entry bytes from
   synthetic payload buffers, zeros for gaps) and compare against
   `write_song_bank_streaming`'s full output for the same `StreamedEntry`
   values and payloads — byte-identical, for BOTH physical entry orders.
4. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`); run the full gate set.

## Acceptance Criteria

1. **Pre-data bytes are canonical**
   - Given a parsed synthetic bank (each physical entry order) and a percent
   - When `plan_virtual_bank` synthesizes the pre-data block
   - Then its 2048 bytes equal `write_song_bank_streaming`'s first 2048 bytes
     for the same `StreamedEntry` values, and the block re-parses via
     `parse_song_bank` when completed with matching payloads

2. **Resolve reconstructs the serializer's physical layout**
   - Given the same layout and synthetic per-entry payloads
   - When `[0, virtual_size)` is served through `resolve` in arbitrary chunk
     sizes (including chunks spanning region boundaries and the real
     0x1000-at-0 header-read shape)
   - Then the reassembled bytes byte-match the serializer's output, and
     `virtual_size` equals `serialized_song_bank_len`

3. **EOF clamp and refusals**
   - Given reads at, past, and straddling `virtual_size`, and plans at the
     28-bit ceiling or with unmappable loops
   - When resolving / planning
   - Then reads clamp to `min(len, virtual_size − offset)` (EOF at/past the
     end) and the refusal legs return the existing `PlanError` identities

4. **Serializer untouched in behavior**
   - Given the existing xwb serializer suites
   - When the validator harness runs after the emission refactor
   - Then they pass unmodified

5. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: Medium
- **Labels**: xwb, layout, song-rate, streaming, host-validation
- **Required Skills**: Rust, XWB container layout, repository host-validator
  harness
- **Generated By**: code-task-generator 2026-08-09
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 3: Build the virtual bank and the synthetic engine replay
