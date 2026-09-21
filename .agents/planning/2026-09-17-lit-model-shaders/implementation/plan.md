# Implementation Plan — Lit model shaders ("Dancer Lighting")

Status: Approved 2026-09-17

Design: `design/detailed-design.md`. Solo-maintainer repo: move straight through the steps in one
session; gates = `cargo check --target x86_64-pc-windows-msvc` (+ `--tests`) → `cargo fmt` (whole crate)
→ `./build.sh` → `./scripts/validate_overlay_draw.sh` → the design §7 cabinet checklist.

- [x] Step 1: Shaders + pure layout helpers
- [x] Step 2: Synthesis + config + operator row
- [x] Step 3: Cabinet + docs — deploy #1 PASS (subtle), deploy #2 preset B PASS ("looks pretty good")
- [x] Step 4: Cel + outline shaders (shared include, 6 new blobs) + layout helpers
- [x] Step 5: Synthesis style/outline plan + config migration + rows
- [x] Step 6: Hull instances in the dancers mod + cabinet + docs — deploy #3 PASS (dancers cel + outlines)
- [x] Phase 2c (2026-09-21, unplanned follow-up from deploy #3 feedback): whole-scene restyle via material
  re-point (`scene3d_shader_lookup`, 18 variant containers, `style.rs` rows), outline internal-silhouette
  fix (per-pixel front-face cull), stage hull twins + distance/width fix, full overlay coverage of the
  `background_dancers` section — deploys #4–#7 PASS. Docs closed 2026-09-21.

---

## Step 1: Shaders + pure layout helpers

**Objective.** Two committed VS blobs that compile under fxc and pack into valid 4-program containers
alongside the sliced stock `gs_model_default` PS, plus the host-tested pure pieces the synthesis
consumes.

**Guidance.** `shaders/src/mdl_lambert.hlsl` per design §3 (`vs_bg_main`, `vs_ch_main`; explicit
registers c14..c17/c18..c21/c22/c23/c24, `s3`; `#define`d light constants; the skinned path reproduces
the stock bone-texture addressing from RE §4.3 exactly). Manifest lines in `scripts/build_shaders.sh`:
`mdl_lambert:vs_3_0:vs_bg_main:mdl_bg_lambert.vs.d3dbc` and
`mdl_lambert:vs_3_0:vs_ch_main:mdl_ch_lambert.vs.d3dbc`. `shader_layout.rs`: `PlannedContainers.lit_models`,
`planned(aa, persp, themes, lit)`, `MODEL_PROGRAM_ENTRIES`, `model_programs()`, `fnv1_32`,
`LIT_MODEL_CONTAINERS`.

**Tests.** `shader_layout` unit tests (planned matrix incl. the lit-only case; `model_programs()` = 4 ×
`(0,0,0)`; `fnv1_32` pinned to the four stock hashes) via `./scripts/validate_overlay_draw.sh`. Offline
smoke: `gsp_pack.py pack` each blob with the sliced stock PS and 4 programs, `inspect --expect-name`
passes, blob stats show `vs_3_0` with a sane instruction count (bg ≈ 20, ch ≈ 100).

**Integration.** Nothing consumes the blobs yet (the synthesis plan does not know `lit_models`);
`cargo check` must stay green (the `planned()` signature change touches `shader_synthesis` — update the
call site with `false` for this step or land Step 2 immediately).

**Demo.** `./scripts/build_shaders.sh` writes the two blobs; `inspect` of a hand-packed
`mdl_ch_lambert.gsp` reports `programs=4 vs=1 ps=1`, hash `fnv1("mdl_ch_lambert")`.

## Step 2: Synthesis + config + operator row

**Objective.** The DLL synthesizes the two containers when the gate holds, honours the config key,
and exposes the DANCER LIGHTING row.

**Guidance.** `shader_synthesis.rs` per design §4 (plan gate with `mod_enabled_in_config`, soft-degrade
blob resolution, `build_all` lit branch with `extract_stock(arc, "gs_model_default").ps` + computed
hash + 4 programs, `"v5"` fingerprint with `lit=`, `planned_names`). `config.rs`:
`ShaderFixesConfig.lit_models` (default `true`, provisional). `shader_fixes.rs`: two live atomics,
`persist_section()`, second `register_enum_row`, enable line with `lit_models=`. Module docs of
`shader_synthesis.rs` (container table + "where it runs") updated.

**Tests.** `cargo check` (+ `--tests`), `cargo fmt`, `./build.sh`, `validate_overlay_draw.sh`.
Optional host check of the container bytes: pack the same inputs with `gsp_pack.py` and compare to
what `pack_gspw` would emit (the Rust packer is already byte-compatible; the only new input is the
computed hash, covered by the `fnv1_32` test).

**Integration.** `arc_handler` merges the two new entries into the shader.arc repack exactly like the
existing ones (`add_or_replace` ADDS entries the stock arc lacks).

**Demo.** Boot log per design §5/§7 item 1 on the maintainer's cabinet.

## Step 3: Cabinet + docs

**Objective.** Cabinet-proven look, tuned once or twice on feedback; documentation closed.

**Guidance.** Hand the maintainer the DLL + the two blobs + the exact log lines (design §7). Iterate
`AMBIENT`/`DIFFUSE`/`WRAP` on feedback (blob-only redeploys). Docs: AGENTS.md "Shader fixes" row (the
two containers, the by-name seam, 4-program rule, stock-PS reuse, fixed world light, the DLL-only-deploy
gotcha) + one sentence in the "Enable Background Dancers" row; README "Enable Background Dancers"
paragraph + the `shader_fixes` config row; `progress.md` closed; maintainer asked about the default.

**Tests.** Design §7 checklist.

**Demo.** Dancers shaded, stage/shadow stock, toggle OFF restores flat after restart.

---

# Phase 2b — cel shading + inverted-hull outlines (design: `design/cel-outlines-addendum.md`)

## Step 4: Shaders + layout helpers

**Objective.** Six new blobs (cel VS ×2, cel PS, outline VS ×2, outline PS) built by fxc from a shared
include, plus the pure layout pieces (`model_programs(outline)`, `DancerStyle`, planned-blob sets).

**Guidance.** Factor `mdl_lambert.hlsl` onto `shaders/src/mdl_common.hlsli` (registers, `skin`,
`to_clip`, `view_frame`); the two lit blobs MUST stay byte-identical after the refactor (fxc is
deterministic — verify with `git status` / sha256 against `49fd0ea3…` / `aa18278d…`). New
`mdl_cel.hlsl` per addendum §3. Manifest: 6 lines. `shader_layout.rs`: `model_programs(outline)`,
`model_table_counts(outline)`.

**Tests.** `validate_overlay_draw.sh` (layout); `gsp_pack.py pack` with `--program 1:1 --program 0:0 ×3`
+ `inspect`; `fxc /dumpbin` of each blob shows the expected registers (c14–c24, s3; PS c2/s0/s15).

## Step 5: Synthesis + config + rows

**Objective.** `Plan { style, outlines }`, migration from `lit_models`, two rows, `outline_programs_available()`.

**Guidance.** Addendum §4. Keep the `lit` path bit-identical (same blobs, same donor PS). Fingerprint
`v6`. Enable line `dancer_lighting=<style>, dancer_outlines=<b>`.

**Tests.** `cargo check` (+`--tests`), `cargo fmt`, `./build.sh`, `validate_overlay_draw.sh`.

## Step 6: Hull instances + cabinet + docs

**Objective.** Every dancer body/part gets a bit-31 twin item sharing its board slot when outlines are on.

**Guidance.** Addendum §5: `InstanceKind::Hull`, `Session::new(…, hulls)`, `mark_hull_records`,
`hull_record_flags` (pure), `GPU_REC_FLAG_MASK` drops bit 31, log tags. `validate_background_dancers.sh`
must stay green (mounted pure files: `render_item_layout.rs` gains the pure helper + test).

**Tests.** Host harnesses + the addendum §7 cabinet checklist; tune `OUTLINE_*` / ramp defines on
feedback (blob-only redeploys). Docs: AGENTS.md rows, README, RE doc §4.6 (done), progress.md.
