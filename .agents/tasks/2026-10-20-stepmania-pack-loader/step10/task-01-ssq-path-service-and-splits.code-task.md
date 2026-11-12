# Task: `services/ssq_path` resolver chain + split-timing SSQs

## Description
Promote `build_ssq_path`'s detour from `split-ssq-auto-discovery` to a shared service with
a priority resolver chain (StepMania provider → split discovery → stock), then let the index
declare `.ssc` songs whose difficulties carry different timing by materialising the writer's
`<b>_N.ssq` files and answering `build_ssq_path(b, d)` for them. Ship a fixture generator
for the cabinet demo.

## Background
`build_ssq_path(out[0x100], basename, difficulty)` is the game's single chooser between
`data/mdb_apx/ssq/<b>.ssq` and `<b>_<N>.ssq` (N = 1..5 = Beginner..Challenge; a `_N` file
holds both styles of that level). `split-ssq-auto-discovery` detours it privately with a
Rule-A resolver over files discovered on disk (`resolver.rs`, basename-opaque, divergence
oracle, fail-open). Step 4's writer emits, for a song with per-chart timing, ONE file per
level `index = level+1` (`SsqFile.index` 1..=5 → `<b>_N.ssq`) and NO base file; Step 6's
index therefore skips such songs (`no index == 0 file`). The game calls the builder for every
slot at boot (`CheckStepDataActor`), at stage load, and in course preload — all go through
the one detour. No pack on hand has a real split-timing song, so the demo needs a crafted
fixture.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md (§4.10, §4.2 per-chart timing)
- Plan: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md — Step 10

**Additional References (if relevant to this task):**
- src/mods/split_ssq_auto_discovery/{mod,resolver,discovery}.rs; docs/split_ssq_research.md (§4 file semantics, §7 signature, §8 special cases: `toho1..4`, `lesa`)
- src/core/ssq/writer.rs (`SsqBundle`, `SsqFile.index`, timing grouping), src/services/stepmania/{index,model,providers,cache}.rs (materialisation, resolver, `FORMAT_VERSION`)
- .agents/summary/interfaces.md (hook-ownership map — move the `build_ssq_path` row to `ssq_path`)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/services/ssq_path.rs`: owns the ONE `build_ssq_path` detour (signature `build_ssq_path` + its `_v2`/derived alternate exactly as the mod resolves it today); `init(signatures) -> bool`; `register(priority: u8, resolver: fn(&[u8] /*basename*/, usize /*difficulty*/) -> Option<Choice>)` where `Choice` is the existing `resolver::Choice` (move it into the service or re-export); the detour tries resolvers in ascending priority and falls back to the ORIGINAL (stock table) when none answers; keeps the mod's bad-arg/oversize guards and the divergence-oracle INFO (cap 64) for the split-discovery consumer. Detour installed when the first consumer registers (or at init if any is registered). Fail-open.
2. `split-ssq-auto-discovery` becomes a consumer (priority 20): its `enable` builds the index and registers; `disable` makes its resolver answer `None` (passthrough), never uninstalling. Behaviour otherwise identical (tests in `resolver.rs` untouched).
3. StepMania consumer (priority 10) in `services/stepmania/providers.rs` (or a small `ssq_split.rs`): for an injected basename, answer from the index: if the song has split files, `Choice::Split(N)` for the slot's level when `<b>_N.ssq` exists with that level, else the unsplit choice; non-injected → `None`.
4. Index: stop skipping bundles without `index == 0`; materialise every `SsqFile` (`ssq/<b>.ssq` and `ssq/<b>_N.ssq`), record per song which `N` files exist and which levels each holds (`SongEntry` field; `FORMAT_VERSION` → 4); the LayeredFS resolver maps `mdb_apx/ssq/<b>_N.ssq` for injected basenames; `chart_length`/`fast_bootup` already go through `find_first_modfile` → still fine. Self-check each split file; a song is declared only for slots whose file passed.
5. Fixture generator `.agents/planning/2026-10-20-stepmania-pack-loader/prototypes/make_split_test_ssc.py`: from a `.sm` song folder (e.g. `In The Groove/Anubis`) writes a new song folder `<Song> (split test)/` with an `.ssc` where ALL charts keep the song timing except Expert, which gets per-chart `#BPMS` at 2× the song BPM and its note data re-measured (each original 4-beat measure → two 4-beat measures holding the first and second halves of the rows; pad to an even row count by doubling resolution; holds spanning the split stay valid because `2`/`3` tails are per row); `#TITLE` suffixed ` (split test)`; copies the audio/art. Note times are identical, timing tables differ → the writer must produce `<b>_4.ssq` (+ others) and the song must play in sync at Expert AND Challenge.
6. Tests: pure chain ordering (priority, first-answer-wins, None → original) in a `crate::`-free `ssq_path_logic.rs`; writer/index tests for the split declaration; harness legs; `sm_pack_check` reports split files per song.
7. Gates: `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, `./scripts/validate_stepmania.sh` + `./scripts/validate_split_ssq.sh` green, path-leak grep, `interfaces.md` row.

## Acceptance Criteria
1. **Chain** — Given both consumers registered, then an injected basename resolves through StepMania, a stock split song through discovery (same answers as before), an unknown basename through the original.
2. **Split fixture** — Given the generated `Anubis (split test)`, when indexed, then the log shows the split files materialised and the song declared; on the cabinet Expert (2× BPM table) and Challenge (song timing) both play in sync; the difficulty panel shows all charts.
3. **No regression** — `validate_split_ssq.sh` green; stock split songs (`acef`, `sabm`) unchanged in the divergence oracle.

## Metadata
- **Complexity**: Medium
- **Labels**: service, hook-ownership, ssq
- **Required Skills**: Rust hooks, SSQ format
- **Generated By**: code-task-generator 2026-10-23
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 10: `ssq_path` service and per-chart timing split SSQs
