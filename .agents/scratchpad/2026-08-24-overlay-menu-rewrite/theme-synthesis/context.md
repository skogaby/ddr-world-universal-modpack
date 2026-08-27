# Context — task-02 theme-synthesis (Step 8, overlay-menu rewrite)

## Task
.agents/tasks/2026-08-24-overlay-menu-rewrite/step08/task-02-theme-synthesis.code-task.md

## Approval chain (verified)
Same as task-01 (plan/design Approved 2026-08-24; breakdown approved
2026-08-25). Mode: auto. Depends on task-01 (Complete — blobs exist).

## Build & test commands
- Host tests: `./scripts/validate_overlay_draw.sh` (12 tests; grows with
  the new pure layout module)
- Gates: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` →
  `./build.sh`
- Offline container check: `python3 scripts/gsp_pack.py inspect
  data_mods/_cache/shader_synthesis/gs_screencommand_default.gsp
  --expect-name gs_screencommand_default` (after a cabinet boot)

## Facts (verified in-repo)
- shader_synthesis.rs (500 lines): `plan()` returns `Plan { aa, persp,
  blob_paths }`; missing NEEDED blob ⇒ whole plan None (fail-open). The
  arrow arm of `build_all` runs UNCONDITIONALLY inside a bare block —
  must gain an `aa || persp` gate once themes-only configs exist.
  `planned_names`: arrow+judge when `aa||persp`; default when `persp`.
  Default arm builds `[(0,0,0),(0,1,0)]`.
- Fingerprint: `"v2 aa={} persp={} arc={fnv}"` + per-blob hashes from
  `plan.blob_paths` — theme blobs added to blob_paths ride the existing
  hash loop for free; prefix bump + ` themes={}` bit needed.
- **Cache-hit path skips build_all** — the index export must ALSO
  happen on cache hits (indices are pure functions of (persp, themes)).
- The mod menu's registry id is `"mod-menu"` (mod.rs:155).
- Task-01 blob filenames: `theme_passthrough.vs.d3dbc`,
  `theme_{arrows,bubbles,wavefield}.ps.d3dbc`.
- `validate_overlay_draw.sh` mounts `MODULES=(encode.rs)` from
  `src/services/overlay_draw` — a pure module elsewhere needs its own
  mount line (absolute #[path] works; the generator loop is per-file).

## Decisions (auto mode)
- Pure layout module: `src/services/avs_layeredfs/shader_layout.rs`
  (dependency-free), mounted into validate_overlay_draw.sh alongside
  encode.rs (second source dir constant). Functions:
  `planned(aa, persp, themes) -> PlannedContainers`,
  `default_programs(persp, themes) -> Vec<(u8,u8,u8)>`,
  `default_theme_indices(persp, themes) -> Option<[u8; 3]>`,
  `PERSP_PROGRAM_INDEX: u8 = 1`.
- Index export lives in `overlay_draw` (the consumer):
  `publish_theme_programs([u32; 3])` (called by synthesis on BOTH the
  build and cache-hit success paths when themes participate) +
  `theme_program_indices() -> Option<[u32; 3]>` (read by the emitter
  and by mod_menu's greyed gate in task-03). Backed by
  `once_cell::sync::OnceCell` (synthesis runs once per boot).
- Theme gating: `themes = mod_enabled("shader-fixes") &&
  mod_enabled("mod-menu") && all 4 theme blobs resolve`; a missing
  theme blob degrades themes only (one WARN), never the AA/persp plan.
- Plan viable when `aa || persp || themes`.
