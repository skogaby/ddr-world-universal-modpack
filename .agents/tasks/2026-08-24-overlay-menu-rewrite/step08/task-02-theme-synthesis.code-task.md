# Task: Theme synthesis — default-container theme programs, fingerprint v3, index export

## Description
Extend `src/services/avs_layeredfs/shader_synthesis.rs` so the
`gs_screencommand_default` container carries the three theme programs
(appended AFTER all existing programs, in every synthesis configuration
where themes are wanted), record the resulting program indices in a new
export the overlay-draw emitter reads at runtime, and bump the
fingerprint cache to v3.

## Background
Step 8 of the overlay-menu rewrite (design §4.7). The player-perspective
rewrite hardcodes program index 1 positionally, so ordering is a hard
contract: **stock prog 0, perspective prog 1 (when enabled), theme progs
last**. The menu-bg program indices are recorded at synthesis time for
the emitter, in every synthesis configuration; shader-fixes mod disabled
⇒ no synthesis ⇒ no theme programs ⇒ Static degrade.

Current facts (verified 2026-08-25; file ≈500 lines):
- `plan()` (shader_synthesis.rs:92–144) gates on `mod_enabled("shader-fixes")`
  then computes `aa` (config `shader_fixes.anti_aliasing`, default true)
  and `persp` (`mod_enabled("player-perspective")`); `!aa && !persp` ⇒
  `None` (no synthesis at all). Needed blobs are resolved via
  `mod_paths::find_first_modfile("blobs/<name>")`; a missing blob fails
  the plan (fail-open to stock).
- `build_all` (:255–344) builds containers from `(flags, vs_idx, ps_idx)`
  tuples: ARROW always; JUDGE when `aa || persp`; **DEFAULT only when
  `persp`** — with `programs = [(0,0,0), (0,1,0)]` where entry 1 is the
  persp VS. "Index 1" is purely positional (the second appended entry).
- `planned_names` (:231–241) mirrors that: default only when persp.
- Fingerprint (:161–228): string `"v2 aa={} persp={} arc={fnv64}"` +
  per-blob `" {name}={hash:016x}"`, sidecar `fingerprint.txt` under
  `data_mods/_cache/shader_synthesis`; cache hit = sidecar match AND all
  planned `.gsp` files present; build failure poisons the sidecar.
- `pack_gspw` (:437–499): GSPW packing byte-compatible with
  `scripts/gsp_pack.py`; counts u8, index-range validated;
  `validate_blob` checks the D3D9 version token (`0xFFFE` vs /
  `0xFFFF` ps).
- **No program-index info is exported anywhere** — the only public
  surface is `synthesize() -> Vec<SynthEntry>`. Synthesis runs lazily on
  the game thread when the game opens `data/arc/shader.arc`, so the
  export must be late-written state the emitter polls (unset ⇒ Static).
- No test harness covers this file (offline validation =
  `scripts/gsp_pack.py inspect/selftest`).
- task-01 blobs: `theme_passthrough.vs.d3dbc`,
  `theme_{arrows,bubbles,wavefield}.ps.d3dbc` under
  `data_mods/shader_fixes/blobs/`.
- Theme identity: `src/mods/mod_menu/theme.rs` `THEMES` order is
  arrows(0), bubbles(1), wavefield(2), minimal(3) — minimal has no
  shader (stays Static; approved decision).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.7)

**Additional References (if relevant to this task):**
- docs/shader_replacement_research.md (GSPW container format, packing rules)
- .agents/tasks/2026-08-24-overlay-menu-rewrite/step08/task-01-theme-shaders.code-task.md (blob contract)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **Plan extension**: a `themes: bool` plan input — true when
   `mod_enabled("shader-fixes")` AND `mod_enabled("mod-menu")` (the
   overlay menu is the only consumer) AND all four theme blobs resolve.
   A missing/invalid theme blob degrades `themes` to false with one WARN
   (does NOT fail the whole plan — AA/persp synthesis must survive a
   broken theme blob). The plan is now viable when
   `aa || persp || themes`.
2. **Default container in all theme configs**: `planned_names` includes
   `gs_screencommand_default` when `persp || themes`; `build_all`'s
   default arm builds:
   - VS table: `[stock_vs]` + persp VS (when persp) + theme passthrough
     VS (when themes).
   - PS table: `[stock_ps]` + the three theme PS blobs (when themes).
   - Programs: `(0,0,0)`; `(0, persp_vs_idx, 0)` when persp — **assert
     this lands at program index 1** (a debug_assert plus a runtime
     check that logs + degrades themes if violated); then the three
     theme programs `(0, theme_vs_idx, theme_ps_idx)` appended LAST in
     arrows/bubbles/wavefield order.
   - The arrow/judge containers are untouched by themes.
3. **Index export**: new module-level export (suggest
   `pub fn theme_program_indices() -> Option<ThemeProgramIndices>` with
   `pub struct ThemeProgramIndices { pub arrows: u32, pub bubbles: u32,
   pub wavefield: u32 }`, backed by atomics/OnceCell written inside
   `build_all` after a successful default-container build — placement
   in `shader_synthesis.rs` with a re-export the emitter can reach, or
   a small shared cell module; implementer's choice, but the emitter
   must be able to poll it lock-free). Unset until synthesis runs;
   never set when themes were degraded/disabled; reset is unnecessary
   (synthesis runs once per boot).
4. **Fingerprint v3**: prefix bumped to `v3`, plus ` themes={}` and the
   four theme blob hashes when themes participate. Old v2 caches
   invalidate automatically.
5. **Pure plan tests**: extract the program-layout decision (which
   containers, VS/PS tables, program tuples, and theme indices for a
   given `(aa, persp, themes)`) into a pure function and cover it in a
   host test (new `MODULES` entry in an existing validate script or a
   small dedicated harness — `scripts/validate_overlay_draw.sh`'s
   pattern; the function must be dependency-free). Matrix: all 8
   `(aa, persp, themes)` combos — perspective at index 1 whenever
   present; theme indices always the last three; default container
   skipped when `!persp && !themes`.
6. **Offline container check**: after a full build, verify with
   `python3 scripts/gsp_pack.py inspect <cached default .gsp>
   --expect-name gs_screencommand_default` that program count and
   table sizes match the plan.

## Dependencies
- task-01 (theme blobs must exist for the runtime leg; the pure layout
  tests don't need them).

## Implementation Approach
1. Extract the pure layout function + tests first (red on the theme
   legs).
2. Thread `themes` through plan/planned_names/build_all; fingerprint v3.
3. Index export + the persp-index-1 assert.
4. Gates: harness(es) → `cargo check` → `cargo fmt` → `./build.sh`.
5. Runtime check (boot the cabinet build): log shows the default
   container synthesized with the expected program count in each config
   (themes on: persp on ⇒ 5 progs, persp off ⇒ 4); `gsp_pack.py
   inspect` on the cache file; v3 sidecar written; a boot with
   shader-fixes disabled serves stock and leaves the index export
   unset.

## Acceptance Criteria

1. **Ordering contract**
   - Given any `(aa, persp, themes)` combination
   - When the layout function runs
   - Then stock is program 0, perspective (when enabled) is EXACTLY
     program 1, and the three theme programs are the final three
     entries, in arrows/bubbles/wavefield order.

2. **Themes without perspective**
   - Given `persp = false, themes = true`
   - When synthesis runs
   - Then the default container IS synthesized with 4 programs
     (stock + 3 themes) and the index export reports 1/2/3.

3. **Degrade paths**
   - Given a missing theme blob, or shader-fixes disabled, or mod-menu
     disabled
   - When synthesis runs
   - Then AA/persp synthesis is unaffected (or wholly absent for
     shader-fixes-off), the index export stays `None`, and exactly one
     WARN names the degrade cause (blob case only).

4. **Cache correctness**
   - Given a prior v2 cache
   - When the new build boots
   - Then the fingerprint mismatch forces a rebuild and the v3 sidecar
     lands; a second boot cache-hits.

## Metadata
- **Complexity**: High
- **Labels**: shader-synthesis, layeredfs, theme, mod-menu
- **Required Skills**: Rust, GSPW container format, repo synthesis conventions
- **Generated By**: code-task-generator 2026-08-25
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 8: Animated shader backgrounds
