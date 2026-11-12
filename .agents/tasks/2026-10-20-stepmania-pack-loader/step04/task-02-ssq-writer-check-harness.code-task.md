# Task: SSQ writer + self-check + `validate_stepmania.sh`

## Description
Port the SSQ writer and its private orchestration glue from `ddr-chart-tools` into
`src/core/ssq/writer.rs`, add the runtime fidelity rules (quad split, delay nudge, gimmick
marking, per-chart-timing grouping, lead-in, `shift_seconds`), add `src/core/ssq/check.rs`
(self-check over the produced bytes), and create `scripts/validate_stepmania.sh` with a
byte-identity leg against the sibling CLI.

## Background
The tool's `src/ssq/writer.rs` (+ `events.rs`, `chunk.rs`, `steps.rs`, `mines.rs`) and the
private `src/job/mod.rs::{sm5_to_ddr, synthesize_events, chart_end_tick,
extend_tempo_pairs_to}` produce a valid SSQ from the model. Load-bearing invariants:
FINISH/END must be bracketed by tempo anchors (else the game locks at READY); `0x0F/0xF0/
0xFF` step bytes are shock arrows; type-20 mine chunk is the modpack's extension. The
existing `src/core/ssq/{ssq_chunk,timing}.rs` are the DLL's read-side walkers (used by
`fast_bootup::ssq_chunk_list_walkable`, `chart_length`, bot_sim) — `check.rs` must use them.
Harness precedent: `scripts/validate_se_bank_synth.sh` (throwaway crate + sibling path dep),
`tools/bot_sim` (mirrors `crate::core::ssq` via a `core` module).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md (§4.2, §2.3, §7)

**Additional References (if relevant to this task):**
- docs/ssq_format.md, docs/ssq_mine_chunk_format.md, docs/split_ssq_research.md §4
- docs/chart_strip_hud_research.md §4 (quantisation colour selector: `q = round(tick·3/64)`)
- .agents/planning/2026-10-20-stepmania-pack-loader/research/ddr-chart-tools.md

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `writer.rs` API: `write_song(&Song, &WriteOptions) -> Result<SsqBundle, WriteError>` with `WriteOptions { lead_in_ms: u32, sync_bias_ms: i32, gimmick_policy: HideChart, quad_policy: Split|ReduceToJump, edit_policy: Drop }` and `SsqBundle { files: Vec<SsqFile { index: u8 /* 0 = <b>.ssq, 1..5 = <b>_N.ssq */, bytes: Vec<u8>, charts: Vec<(slot u8, level u8)> }>, slots: [u8; 10] /* level or 0 */, rejects: Vec<(chart desc, Reason)> }`. Also `shift_seconds(ssq: &[u8], delta_ms: i32) -> Option<Vec<u8>>` (pure rewrite of the tempo chunk's seconds column).
2. Byte identity: with `lead_in_ms = 0`, `sync_bias_ms = 0` and a song needing no new rule, `files[0].bytes` == the CLI's `.ssq` output. Port the tempo synthesis (i128 fixed point), events, step/freeze emission and mine chunk VERBATIM before layering rules on top.
3. Rules (design §4.2): quads → `L+R` at *t*, `D+U` at *t+1* (per side on doubles; freeze heads too; never a shock byte on a note row); delays → stop at `delay_tick − 1`; `#FAKES` ranges drop notes; `Lift`/`Fake` dropped; `RollHead` → freeze; warps/negative BPM/negative stop → chart `Unrepresentable`; per-chart timing groups → split files per level when single/double of that level share a timing, else keep single, hide double; Edit dropped; duplicate (style, difficulty) → first kept, reason recorded; double Beginner dropped; a chart with zero tap/jump/shock rows is never declared; `tempo_data[0] = −offset·1000 + lead_in_ms + sync_bias_ms`.
4. `check.rs`: `check_ssq(bytes, declared_slots) -> Result<(), Vec<CheckFailure>>` using `ssq_chunk`/`timing`: tempo monotonic, no zero-Δms/non-zero-Δtick segment, FINISH/END bracketed, END ≥ last note, ≥ 1 tap/jump/shock per declared slot, no shock byte on a non-shock row, chunk list walkable to the terminator.
5. `scripts/validate_stepmania.sh`: throwaway host crate mirroring `crate::core::{sm,ssq}` via `#[path]`, deps `encoding_rs` + `ddr-chart-tools` (path, `DDR_CHART_TOOLS` / `--chart-tools` / `../ddr-chart-tools`, byte-identity leg SKIPPED with a note when absent); runs all mounted `#[cfg(test)]` suites plus: byte identity over a generated corpus (the tool's synthetic song generator in `tests/common/mod.rs` or equivalent in-harness generator: taps, jumps, holds, BPM changes, stops, mines, shocks); quad-split colour-class invariance (`round(tick·3/64)` equal for parent/child rows at ticks of 4th/8th/12th/16th/24th/32nd/48th/64th/192nd); self-check negatives (mines-only, holds-only, zero-Δms tempo, END before last note, shock-byte leak each fail); delay nudge placement; `shift_seconds` round trip; per-chart timing split grouping.
6. Register `writer`/`check` in `src/core/ssq/mod.rs`; `cargo check --target x86_64-pc-windows-msvc` clean; `cargo fmt`.

## Dependencies
- Task 01 (`core/sm`).

## Implementation Approach
1. Verbatim port → harness byte identity green.
2. Add rules one at a time with tests.
3. `check.rs`; harness legs; docs headers.

## Acceptance Criteria

1. **Byte identity** — Given the generated corpus, then every song's `files[0].bytes` equals the CLI output.
2. **Rules** — Given a chart with a quad at tick 0 and a 12th-note quad, then two rows 1 tick apart replace each, both in the same colour class, and no `0x0F` appears on a note row.
3. **Self-check** — Given each negative fixture, then `check_ssq` reports the specific failure; given every corpus song, it passes.
4. **Harness** — `./scripts/validate_stepmania.sh` exits 0 with every leg PASS (identity leg SKIP without the sibling).

## Metadata
- **Complexity**: High
- **Labels**: port, ssq, pure, harness
- **Required Skills**: Rust, SSQ format
- **Generated By**: code-task-generator 2026-10-22
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 4: Pure chart pipeline — `core/sm` parser, SSQ writer + self-check, harness
