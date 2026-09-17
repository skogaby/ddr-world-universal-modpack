# Task: Cross-build sweep + shape-diff review of the `scene3d` group, recorded in the RE doc

## Description
Run the offline four-build signature sweep and the post-match shape diff over every function whose
byte layout the Background Dancers feature will rely on, review every divergence against what the
consumer actually reads, and record the result table in `docs/background_dancers_research.md` §1.9.
This is Step 1's validation gate: nothing engine-facing may be written (Step 2 onward) until this is
green.

## Background
An AOB hitting on all builds proves nothing about the bytes a consumer reads at `match+N` or about the
struct offsets an engine function hardcodes (AGENTS.md "Cross-build signature sweep" row). The
`scene3d` group publishes ~40 offsets decoded from instruction streams AND the feature will hand
engine-owned functions (collector, bone-texture upload, draw, `SceneGraph::update`, camera rebuilds)
objects laid out per design §5.2/§5.3 — every field those functions read must be at the same offset on
20250805 / 20260224 / 20260721 / 20260825. `scripts/sig_harness/shape_diff.py` disassembles a window
after every resolved match on every build and reports the first divergence; for engine functions that
are NOT AOB'd (the three item consumers) the sweep JSON must be extended with their per-build offsets
(they are all reachable as CALL targets from AOB'd functions, or via `--names` with an explicit RVA
map — see the script header).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§5.2 render-item layout, §5.3 camera slot, §7.2)
- RE record: `docs/background_dancers_research.md` §1.7 (what is read at each `match+N`), §1.8–§1.9 (where the results go)

**Additional References (if relevant to this task):**
- `docs/background_dancers_feasibility.md` §2.2 (the consumer functions `FUN_180263430` / `FUN_180261780` / `FUN_180262670` and how they read the item), §5.1.1 (the per-offset item ABI table)
- `docs/cross_build_signature_sweep.md` (how previous sweeps were recorded; the report format)
- `scripts/sig_harness/shape_diff.py` and `scripts/sig_harness/report.py` headers (CLI, `ALT_GROUPS`, JSON schema)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `./scripts/validate_signatures.sh ~/Desktop/ddr_modules --json <tmp>/sweep.json` exits 0 with every `scene3d_*` and every new raw AOB name green on all four builds; any `[-]` for the group is a task failure to be fixed in task 01's code (report back rather than weakening a gate).
2. `scripts/sig_harness/shape_diff.py --json <tmp>/sweep.json --dir ~/Desktop/ddr_modules --names <the twelve new AOB names>` with a window large enough to cover every `match+N` the derivation reads (0x200 default; use `--window 0x400` for `sg_destroy_flush` and `bg_root_create_site`). Every divergence offset must be either (a) beyond every offset the derivation reads, or (b) explained in the results table.
3. Shape diff over the NON-AOB consumers, per build: the opaque collector `FUN_180263430`, the bone-texture upload `FUN_180261780`, the draw `FUN_180262670`, `SceneGraph::update` `FUN_180214570`, the camera view/projection rebuilds `FUN_180220b80` / `FUN_1802376e0`, and the pass driver `FUN_1802606d0`. Locate each build's twin from the sweep (they are CALL targets of AOB'd sites — the tick's callees, the update-job's callee — or reachable from the pass entry; where no AOB'd caller exists, find the twin by the harness's byte-shape search or a short unique prefix and document the method). Report for each: the first divergence offset and whether any item/camera field offset (design §5.2/§5.3 tables) differs between builds. The expected answer is "identical layout on all four builds"; anything else STOPS the feature until the design is amended.
4. Verify the derived VALUES are identical across the four builds (the sweep's `[+] name (derived) = 0x…` lines) and list any that differ — a differing value is fine (it is derived) but must be noted for the consumer.
5. Write §1.9 of `docs/background_dancers_research.md`: a results table (name → per-build match offset → first divergence → verdict), the exact commands run, the date, and a one-paragraph verdict. Paths in the doc must be `~`-relative or repo-relative — never the absolute home path.
6. Tick the Step 1 checkbox in `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` and update `progress.md` (Status/NEXT ACTION/Done/Deploy log) only when 1–5 hold.

## Dependencies
- Task 01 of this step complete (the group exists and resolves)
- Host tooling: python3 with `pefile` and `capstone` (already used by `shape_diff.py`), the four builds under `~/Desktop/ddr_modules` (plus one `libafp-win64*.dll` for the IAT-compare derivations)
- Inferred: `scripts/sig_harness/main.rs` may need no change; if the JSON lacks the CALL-target twins needed for requirement 3, prefer computing them in a throwaway script under the temp dir over modifying the harness (the harness is shared by every feature)

## Implementation Approach
1. Run the sweep with `--json`; confirm exit 0 and grep the raw logs for `scene3d_`.
2. Run `shape_diff.py` over the twelve AOB names; for each divergence, re-read task 01's derivation to confirm the divergent offset is past everything it decodes.
3. For the non-AOB consumers, resolve per-build addresses (from the sweep's derived `scene3d_scene_graph_update` etc. and the pass driver chain) and run the disassembly diff (either `shape_diff.py --names` with a hand-built JSON, or a small capstone script in the temp dir following `shape_diff.py`'s normalisation).
4. Record everything in §1.9; tick the plan; update progress.md.

## Acceptance Criteria

1. **Sweep green**
   - Given task 01 merged into the working tree
   - When `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` runs
   - Then the exit status is 0 and every `scene3d_*` name is `[+]` on all four builds

2. **Consumer shapes attested**
   - Given the shape diff over the item consumers, `SceneGraph::update` and the camera rebuilds on all four builds
   - When the item offsets `+0x00/+0x40/+0x50/+0x60/+0x68/+0x70/+0x78/+0x80/+0x98/+0xA0/+0xA8/+0xAC/+0xB0/+0xB4` (and the draw-record `+0x10/+0x18/+0x20/+0x28/+0x2C` reads), the node offsets `+0x08/+0x0C/+0x18/+0x20/+0x78/+0xE8` and the camera offsets `+0x268…+0x2B3` are compared
   - Then every one is read identically on all four builds, or the divergence is written up with its consequence

3. **Documentation complete**
   - Given the finished sweep
   - When `docs/background_dancers_research.md` §1.9 is read
   - Then it contains the results table, the commands, the date, the verdict, and no absolute local paths (`git grep -nE "/(Users|home)/[^/ ]+/" -- docs/background_dancers_research.md` is empty)

4. **Plan and progress updated**
   - Given criteria 1–3 hold
   - When the plan checklist and `progress.md` are read
   - Then Step 1 is ticked and `progress.md` says `Status: Step 2 of 10 — not started` with a NEXT ACTION pointing at Step 2's first file

## Metadata
- **Complexity**: Medium
- **Labels**: validation, signatures, shape-diff, scene3d, background-dancers, step-1
- **Required Skills**: the repo's sig harness, capstone/pefile scripting, x86-64 reading
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 1: Signatures and derivations for the `scene3d` group + cross-build sweep
