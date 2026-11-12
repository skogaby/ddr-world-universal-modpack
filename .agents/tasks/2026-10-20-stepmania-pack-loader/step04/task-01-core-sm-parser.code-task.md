# Task: `core/sm/` — pure StepMania simfile parser

## Description
Port the SM/SSC parsing layer of the sibling `ddr-chart-tools` repository into
`src/core/sm/` as pure, host-testable Rust, then extend it with the tolerance and
per-chart isolation the runtime needs (design §4.1).

## Background
`ddr-chart-tools` (`src/ssc/{mod,notes,msd}.rs`, `src/sm/mod.rs`, `src/model/`) parses
`.ssc`/`.sm` into a `Song` model. It fails whole files on one bad chart, reads UTF-8 only,
drops `#DELAYS`/`#WARPS`/per-chart timing with a warning, errors on keysound/attack
annotations, and ignores `#METER`, `#MUSIC`, `#TITLETRANSLIT`, `#SUBTITLE`, `#JACKET`,
`#BANNER`, `#BACKGROUND`, `#SELECTABLE`, `#DISPLAYBPM`. The runtime converts arbitrary
community packs unattended, so every path must be panic-free, bounded and per-chart
tolerant. The port's output for inputs needing none of the new behaviour must stay
identical to the tool's (the Step 4 harness byte-compares the SSQ built from it).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md (§4.1, §2.2 N7/N8, §7)

**Additional References (if relevant to this task):**
- .agents/planning/2026-10-20-stepmania-pack-loader/research/ddr-chart-tools.md
- docs/stepmania_runtime_packs_feasibility.md §8.2, §8.3

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Layout: `src/core/sm/{mod.rs,msd.rs,model.rs,notes.rs,simfile.rs,text.rs}`; registered in `src/core/mod.rs`. `//!` header on `mod.rs` per repo convention (mechanism, provenance = ddr-chart-tools commit `a733404`, fidelity policy).
2. Purity: no `windows`, no `log_*!` macros, no `crate::` imports other than `crate::core::sm::*`; warnings are returned in the model (`Vec<Warning>`), never logged. Harness-mountable via `#[path]` with a `core` module mirror.
3. `text.rs`: `decode(bytes) -> Cow<str>` strict UTF-8 (BOM stripped) → Shift-JIS → Windows-1252 (`encoding_rs`, add to `Cargo.toml`); input cap 4 MiB enforced by the caller.
4. Model: `Song { title, title_translit, subtitle, subtitle_translit, artist, artist_translit, music, banner, background, jacket, selectable, display_bpm: Option<(f64,f64)>, offset_s, timing: Timing, sample_start_s, sample_length_s, charts: Vec<Chart>, warnings }`; `Timing { bpms: Vec<(Beat,f64)>, stops, delays, warps: Vec<(Beat, Beat)>, fakes, offset_s }`; `Chart { style: Single|Double, difficulty: Beginner|Basic|Difficult|Expert|Challenge|Edit, meter: u32, description, timing: Option<Timing>, notes: Vec<Note>, warnings }`; `Note { beat: Beat (exact rational), kind: Tap|HoldHead{len}|RollHead{len}|Mine{panels}|Shock{side}|Lift|Fake, panels: u8 }`. Keep the tool's exact-rational `Beat` and its tick rounding helpers so the SSQ writer stays byte-identical.
5. `notes.rs`: tolerant row decode — strip `[n]` keysound and `{...}` attack annotations and `K`; `F`/`L` kept as kinds (the writer decides); `4` → `RollHead`; mines classified as the tool does (full-row → `Shock`, partial → `Mine`); a second head on an open panel closes the first as a tap and warns; bad row length / unclosed hold / orphan tail → that CHART is rejected with a reason, not the song.
6. `simfile.rs`: `parse_ssc(&str)`, `parse_sm(&str)`, `parse(path_hint, &str)` (pick by extension, `.ssc` over `.sm`); tolerant `#BPMS/#STOPS/#DELAYS/#WARPS/#FAKES` (trailing commas, blanks, `1e2`), case-insensitive tags and difficulty names (unknown → Edit with a warning), unsupported `#STEPSTYPE` → chart skipped with a reason; per-chart `#OFFSET/#BPMS/#STOPS/#DELAYS/#WARPS` captured into `Chart.timing`; negative BPM / negative stop / warps are PARSED (never rejected here).
7. `#[cfg(test)]` suites in each file covering the design §7 fixture list for parsing (synthetic strings; no external files), including a Shift-JIS fixture and a `.sm` with `#NOTES` 6-field form.

## Dependencies
- `encoding_rs` crate (pure Rust).

## Implementation Approach
1. Copy the tool's files verbatim into the layout above; strip `anyhow`/`log`; make the model self-contained.
2. Replace `?`-propagating chart errors with per-chart rejection; add the tolerant parsers; add the new tags.
3. Write tests; `cargo check --target x86_64-pc-windows-msvc`; the Step 4 harness (task-02) mounts and runs these suites.

## Acceptance Criteria

1. **Tool parity** — Given a simfile the tool parses, when `core::sm::parse` runs, then the resulting song timing, charts and notes are equivalent (same beats, kinds, panels, difficulties) — verified indirectly by SSQ byte identity in task-02.
2. **Per-chart isolation** — Given an `.ssc` with one `dance-solo` chart, one `Crazy` difficulty and one chart with an unclosed hold, then the song parses with those charts rejected (reasons recorded) and the remaining charts intact.
3. **Tolerance** — Given `#BPMS:0=1.4e2,,;` and a Shift-JIS encoded `#TITLE`, then both parse.
4. **Purity** — Then the files compile in a throwaway host crate with only `encoding_rs` and the `core::sm` mirror.

## Metadata
- **Complexity**: High
- **Labels**: port, parser, pure
- **Required Skills**: Rust, StepMania formats
- **Generated By**: code-task-generator 2026-10-22
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 4: Pure chart pipeline — `core/sm` parser, SSQ writer + self-check, harness
