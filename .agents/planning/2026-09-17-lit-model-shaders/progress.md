# Progress — Lit model shaders ("Dancer Lighting")

Updated: 2026-09-17
Status: Complete (uncommitted — maintainer commits manually); cabinet-validated 2026-09-21 (deploys #1–#7)
NEXT ACTION: none — maintainer commits the working tree when ready. Feature complete for now (maintainer,
2026-09-21); the user-facing option layer is a later design.
Resume protocol: `implementation/plan.md` (checklist) → `design/detailed-design.md` →
`docs/background_dancers_research.md` §4 (the RE that fixed the design).

## Done
- Step 1a–1c RE (no code): World's shader registry is filled from the arc by EXTENSION (`gsp`) and looked
  up by the GSPW header hash — no allowlist (`FUN_18025f700`/`FUN_18025f190`/`FUN_18025f470`, pool 256);
  material→shader selection `FUN_1802745b0` (20260825) / `FUN_180233920` (20260915) = FNV-1 of the
  debug-info name → registry → fallback `gs_model_*_default` only on a miss. Register map confirmed on
  the 20260915 arc via `fxc /dumpbin`; **World IS bound at c14..c17** per draw (`FUN_18026ca40`), so the
  light lives in world space. **The model pass indexes `programs[stage]` unchecked with stage 2 in
  production ⇒ 4 identical program entries are MANDATORY** (every stock model container has them).
  Survey of all 141 World `mapset_*`/`pl_*` arcs: lambert = ALL 26 dancer bodies (`mdl_ch_lambert`) + ALL
  88 parts (`mdl_bg_lambert`); stages 0, shadow `mdl_bg_constant`. Recorded as
  `docs/background_dancers_research.md` §4.
- Design + plan written (`design/detailed-design.md`, `implementation/plan.md`).
- Step 1: `shaders/src/mdl_lambert.hlsl` (`vs_bg_main` 15 instr / `vs_ch_main` 65 instr, fxc 9.29;
  the skinned path compiled to the stock bone-addressing shape) + 2 manifest lines →
  `data_mods/shader_fixes/blobs/mdl_{bg,ch}_lambert.vs.d3dbc` (the 16 existing blobs byte-identical
  after the full rebuild). `gsp_pack.py pack` (4 programs, sliced stock `gs_model_default` PS) +
  `inspect --expect-name` OK for both. `shader_layout.rs`: `PlannedContainers.lit_models`,
  `planned(aa, persp, themes, lit)`, `MODEL_PROGRAM_ENTRIES = 4`, `model_programs()`, `fnv1_32`,
  `LIT_MODEL_CONTAINERS`; 4 new tests (24 green via `validate_overlay_draw.sh`).
- Step 2: `shader_synthesis.rs` — `Plan.lit_models` gated on `mod_enabled_in_config("shader-fixes") ∧
  ("background-dancers") ∧ shader_fixes.lit_models` (the plan's mod checks now all go through
  `mod_enabled_in_config`, so `DEFAULT_OFF_MODS` is honoured), soft-degrade lit blob resolution (one
  WARN naming the deploy gotcha), `build_all` lit branch (our VS + sliced `gs_model_default` PS,
  computed FNV-1 hash, 4 programs), fingerprint `v5 … lit=`, `planned_names`, module docs.
  `config.rs`: `ShaderFixesConfig.lit_models` (default `true`, provisional). `shader_fixes.rs`: two live
  atomics + `persist_section()` (whole-section writes), second enum row `shader-fixes-lit` "Dancer
  Lighting", enable line `anti_aliasing=…, lit_models=…`. Gates: `cargo check` (+`--tests`) ✓
  `cargo fmt` ✓ `./build.sh` ✓ (58 s) `validate_overlay_draw.sh` ✓. Built DLL copied to
  `release/ddr_world_hook.lit-shaders-deploy1.dll` (gitignored).

- **Phase 2b (cel + outlines), 2026-09-17:** RE §4.6 (bit-31 program selector in both bind callbacks,
  records counted from the RESOURCE ⇒ hull = a second item, `FUN_180261c80` reads only bits 1–4 +
  `&0xE0`, `DAT_1806f1548` as a future live style switch). Step 4: `shaders/src/mdl_common.hlsli` (the
  two lit blobs byte-identical after the refactor — verified) + `mdl_cel.hlsl` → 6 blobs (`mdl_{bg,ch}_cel.vs`
  39/91 instr, `mdl_cel.ps` 26, `mdl_{bg,ch}_outline.vs` 56/106, `mdl_outline.ps` 8; stipple sequence
  `mul/frc/texld/add/texkill` identical to stock); `shader_layout::{model_programs(outline),
  model_table_counts, MODEL_HULL_PROGRAM_INDEX, DancerStyle}` + 2 tests (25 green). Step 5:
  `ShaderFixesConfig { dancer_lighting: Option<String>, dancer_outlines, lit_models: Option<bool> (legacy) }`
  + `dancer_style()`; `shader_synthesis` `Plan { style, outlines }`, per-style blob resolution (soft
  degrade), `build_all` packs `[style, outline]` tables with `model_programs(outlines)`, fingerprint `v6`,
  `outline_programs_available()` published on both success paths; `shader_fixes.rs` three rows +
  `dancer_outlines_live()`. Step 6: `render_item_layout::{REC_HULL_BIT, REC_BLEND_GROUP_MASK,
  hull_record_flags}` (+ test, `GPU_REC_FLAG_MASK` → `0x7000_00FF`), `render_item::mark_hull_records`,
  `InstanceKind::Hull { of }` + `owns_slot()` + `Session::new(…, hulls)` + `built_counts` 5-tuple,
  `director::hide_all` skips non-owners, lifecycle gate `dancer_outlines_live() ∧
  outline_programs_available()`. Gates: `cargo check` (+`--tests`) ✓ `cargo fmt` ✓ `./build.sh` ✓ (75 s)
  `validate_overlay_draw.sh` 25 ✓ `validate_background_dancers.sh` 103 ✓. AGENTS.md / README updated.

- **Phase 2c (2026-09-21) — stage + whole-scene restyle, outline fix:** deploy #3 PASS on the dancers
  ("cel shading looks pretty good"), two follow-ups: (a) outlines missed INTERNAL silhouettes (arm over
  chest) — the facing-dependent depth push lost to a chest 0–20 mm behind the arm; fixed by an emulated
  front-face cull in the outline PS (`clip(dot(n_view, pos_view))`) + a 1 mm WORLD-metre constant push
  (`P22` recovered from WVP); (b) the stage. New mechanism (RE §4.7): the converter stores the resolved
  `gs::Shader*` at `mat+0x20`; the DLL re-points its PRIVATE material copies at synthesized VARIANT
  containers `<name>_lit` / `<name>_cel` (9 names × 2 = 18, `shader_layout::MODEL_VARIANTS`) looked up
  via the new optional derivation `scene3d_shader_lookup` (`model_shader_select_site`, unique + identical
  on all 5 builds, sweep ALL GREEN) — per record (blend group 0 only), per instance (never shadow / `_bg`
  skydome). Style is now the dancers mod's `background_dancers.style` / `.outlines` (`style.rs`, rows
  SCENE STYLE / SCENE OUTLINES under the Background Dancers header, NEXT SONG); the by-name `mdl_*_lambert`
  synthesis is retired; `shader_fixes` back to AA only (legacy keys read for migration). Shaders: define-
  driven (`UV3`/`VCOLOR`/`NOTEX` lit VS, `VCOLOR` cel VS, `CCOLOR`/`NOTEX` cel+outline PS) — the two
  original lit blobs stay byte-identical; `build_shaders.sh` grew a defines manifest field (and a
  bash-3.2 `set -u` empty-array bug that had silently kept stale outline blobs was fixed — `rm -f` before
  every compile now). Fingerprint `v7`. Host: overlay_draw 26 ✓ background_dancers 104 ✓ (incl.
  `restyle_eligible_materials`, `variant_table_is_consistent`, `style::gate_matrix`). Gates ✓, build ✓.

## In flight
Nothing. All work is unstaged in the working tree.

## Deploy & test log
- deploy #1 (2026-09-17, `release/ddr_world_hook.lit-shaders-deploy1.dll` + the two blobs, CrossOver /
  gamemdx 20260915) — **PASS.** Log: `synthesizing (aa=true, persp=true, themes=true, lit=true)`,
  `mdl_bg_lambert → 1340 bytes (4 programs, 1 VS, 1 PS)`, `mdl_ch_lambert → 2364 bytes (4 programs, 1
  VS, 1 PS)`, zero WARNs; everything else in the shader set unchanged (arrow/judge/default sizes as
  before). Maintainer: "everything works in-game, the effect is just subtle — character models look a
  little less flat, subtle shading on curved surfaces like arms instead of everything blending
  together." Look tuning pending the maintainer's call (see below).
- deploy #2 (2026-09-17, blobs ONLY — same DLL; preset B: `LIT_AMBIENT 0.55`, `LIT_DIFFUSE 0.65`,
  `LIT_KEY_DIR (0.7, 0.6, 0.5)` — lit ∈ [0.55, 1.2], lower/lateral key so limbs get a light/shadow
  split; blob sha256 `49fd0ea3…` / `aa18278d…`) — **PASS**, maintainer: "looks pretty good in-game".
  Preset B is the shipped LIT style.
- deploy #3 (`release/ddr_world_hook.lit-shaders-deploy3.dll` + 8 model blobs; Phase 2b cel + outlines) —
  **PASS on the dancers** ("cel shading looks pretty good on the character"); two findings → Phase 2c:
  outlines only on the whole-figure silhouette (screenshot 2026-09-21), and the stage was untouched.
- deploy #4 (`release/ddr_world_hook.lit-shaders-deploy4.dll` + 18 model blobs; Phase 2c) — cel on the
  dancers + stage props confirmed working; **stage OUTLINES missing** (log: `built … 2 hull` = dancer +
  part only): a `cargo fmt` reflow had defeated the hull-creation edit, the `Dancer|Part`-only filter
  survived. Fixed → `restyle_allowed` (deploy #5). Maintainer also asked for the menu wording: group header
  "BACKGROUND DANCERS" (was the mod name "Enable Background Dancers"), row "LIGHTING STYLE" with "STOCK
  (UNLIT)" / "SMOOTH SHADING" / "CEL SHADING" (was SCENE STYLE / STOCK / LIT / CEL). Config keys unchanged.
- deploy #5 (`release/ddr_world_hook.lit-shaders-deploy5.dll`, same 18 blobs) — hull twins now built for every
  eligible prop (`built … 8 hull`, per-prop `[hull] … marked bit-31` lines, menu wording confirmed) but
  **stage outlines still invisible**: the rim width fell off ∝ 1/w past `OUTLINE_REF_DIST = 5 m` and the props
  sit 8–30 m from the stock cameras ⇒ sub-pixel rims (RE §4.7 addendum). Fixed: REF_DIST 25 m + per-item
  width via `ModelParameters.w` (`background_dancers.outline_px` 2.0 / `outline_px_stage` 1.5).
- deploy #6 (`release/ddr_world_hook.lit-shaders-deploy6.dll` + the 2 rebuilt outline VS blobs) — **PASS**
  ("everything is working"): stage props outlined at stage distances, cel on the whole scene.
- deploy #7 (`release/ddr_world_hook.lit-shaders-deploy7.dll`, DLL only): full overlay coverage of the
  `background_dancers` knobs — 4 more rows under BACKGROUND DANCERS: OUTLINE WIDTH (DANCERS) / (STAGE)
  (0.50–6.00 PX in 0.25 steps, config in px), BPM SYNC, STOP SLOW-MOTION (the two tempo switches are now
  LIVE values read at every session — next song, no relaunch) — **PASS** ("everything works great").
- Docs pass 2026-09-21: README hero section "Background Dancers — The 3D Stage Is Back" (+ the cel robot
  screenshot `screenshots/background_dancers.png`), feature table + config table; AGENTS.md Shader-fixes row
  rewritten for the variant design, Background Dancers row renamed + Lighting Style passage, config entry;
  RE doc §4.4 superseded note. `progress.md` closed. Expected: `scene style -- cel outlines=true (rim px
  dancers=2 stage=1.5 …)`, `[hull] … rim 1.5 px` on props / `rim 2 px` on dancers; visible outlines on
  props at stage distances.
  Expected log: `synthesizing (… scene variants=true, outlines=true)`, 18 lines
  `mdl_<name>_{lit,cel} → … (4 programs, 2 VS, 2 PS)`, `ShaderFixes: enabled (… scene variants served,
  outline programs served …)`, `BackgroundDancers: scene style -- cel outlines=true (variants served,
  outline programs served; applies per song)`; per song one `… [stage|dancer|part] style cel -- materials
  restyled=N kept: blend=M no-variant=0` per instance and `[hull] … marked bit-31 …` per twin, `built …
  (… N hull)`. Failure signatures: `scene-style blob '…' not found` (→ stock), `scene3d_shader_lookup
  (optional) -- …` at boot (→ stock, one WARN at session), `no-variant=K>0` (a material's stock hash has
  no variant — report the model), `[outline programs not served]`.
  (deploy #3 expectations, superseded:) Expected log (style cel, outlines on): `synthesizing (… style=cel, outlines=true)`,
  `mdl_bg_lambert → … (4 programs, 2 VS, 2 PS)`, `mdl_ch_lambert → … (4 programs, 2 VS, 2 PS)`,
  `ShaderFixes: enabled (… dancer_lighting=cel, dancer_outlines=true [outline programs served] …)`;
  per song one `… [hull] N record(s) marked bit-31 …` + `… [hull] item built …` per body/part and
  `built … (… N hull), 0 skipped`. Failure signatures: `dancer-lighting blob '…' not found` (style →
  stock), `outline blob '…' not found` (outlines off), `[outline programs not served]` in the enable
  line with outlines=true ⇒ no hulls built (check the two lines above).

## Deviations & open questions
- **Row name is DANCER LIGHTING, not the handoff's "STAGE LIGHTING"** — the survey showed no stage
  model names a lambert material; only dancers (bodies + parts) are affected. Config key stays the
  handoff's neutral `shader_fixes.lit_models`.
- **Stock PS reuse instead of a new lit PS**: the lit factor multiplies `tint.rgb`, so it rides the VS's
  COLOR0 output and the game's own `gs_model_default` PS (stipple + alpha bit-exact) is the PS of both lit
  containers — two committed blobs (VS only), zero new PS.
- **World-space light** (c14..c17 IS bound in the model pass — the Phase-2 note's "only WVP" was wrong).
- **Shipped default — RESOLVED (maintainer, 2026-09-17 after deploy #1):** this is a PROOF OF CONCEPT.
  When the final Background Dancers ships, lighting becomes a USER-facing option (stock lighting vs
  custom lighting); until then the operator row + `lit_models` (default `true`) stand as-is and no
  further default decision is needed here. The gate on `background-dancers` already keeps it inert for
  cabinets without the scene.
- ~~No rim term in v1~~ — Phase 2b recovers the view frame from World + WVP (`view_frame` in
  `mdl_common.hlsli`, RE §4.6), so the cel style has rim ink and the hull knows its facing.
- **Outlines without cull control:** the hull cannot cull front faces (render states come from the
  shared GPU record), so it uses a facing-dependent depth push instead — `OUTLINE_PUSH_RIM 0.0002` /
  `OUTLINE_PUSH_FACE 0.004` (NDC) are the two constants most likely to need cabinet tuning (interior
  black patches ⇒ raise FACE; silhouette gaps ⇒ lower RIM). Alpha-blended meshes get no hull.
- `model_programs(outline)` puts the outline pair at program 0 ONLY; a stock-shaped 4×(0,0,0) table is
  emitted when the outline blobs are missing, so a bit-31 record can never bind an out-of-range index.
- The Background Dancers feature was committed by the maintainer (`319b080`) between the handoff and this
  session; the two `2026-09-16-enable-background-dancers/{plan,progress}.md` modifications in the tree are
  the maintainer's own post-commit edits — untouched.

## Key facts for a cold resume
- Mechanism: two synthesized GSPW containers `mdl_bg_lambert` / `mdl_ch_lambert` (FNV-1 name hash
  COMPUTED — the only computed hash in `shader_synthesis`), 4 × `(0,0,0)` programs, our VS + the sliced
  stock `gs_model_default` PS; delivered through `arc_handler`'s shader.arc repack; fingerprint `v5`.
- Gate: `shader-fixes` ∧ `background-dancers` (both via `mod_enabled_in_config` — honours
  `DEFAULT_OFF_MODS`) ∧ `shader_fixes.lit_models`; missing VS blob ⇒ lit dropped, one WARN.
- Shader contract = stock `gs_model_default` PS inputs: `o1 TEXCOORD0.xy`, `o2 COLOR0`; bone texture
  `create(4, bone_count)`: rows at u 0.125/0.375/0.625, `v = (idx+0.5)/(c22.x+c22.z)`, `w0 = 1 − Σw`.
- Tunables are `#define`s at the top of `shaders/src/mdl_lambert.hlsl` (`LIT_AMBIENT 0.65`,
  `LIT_DIFFUSE 0.35`, `LIT_WRAP 0.0`, `LIT_KEY_DIR (0.3, 1.0, 0.6)` world space); rebuild with
  `./scripts/build_shaders.sh` (fxc golden path; the 16 other blobs must stay byte-identical — check
  `git status`), redeploy the blobs only — the fingerprint includes blob hashes.
- A DLL-only deploy does NOT carry `data_mods/shader_fixes/blobs/*.d3dbc` — always ship both (8 model
  blobs now: `mdl_{bg,ch}_{lambert,cel,outline}.vs` + `mdl_{cel,outline}.ps`).
- Hull mechanism: `rec+0x28` bit 31 ⇒ the bind callbacks select program 0; hull = a SECOND render item
  (records are counted from the resource) reading the body's board slot; `GPU_REC_FLAG_MASK` now clears
  bit 31 on body items. Never build hulls for stage parts / shadow (4×(0,0,0) containers ⇒ duplicate draw).
- Future live style switch: `DAT_1806f1548` (program 2 vs 3) — one derivation, no detour (RE §4.6).
- Cabinet = gamemdx 20260915 (ALSO open in Ghidra as `gamemdx_20260915.dll`).
- Phase 2c key facts: restyle = write `gs::Shader*` into `mat_copy+0x20` (`render_item::restyle_materials`,
  eligibility `render_item_layout::restyle_eligible_materials` + `session::restyle_allowed`); variant object
  = `texture::lookup_shader(fnv1("<material>_<style>"))` (derived `scene3d_shader_lookup`); hull twins for
  EVERY eligible instance incl. stage props, records of stock-kept materials hidden; the style is a per-SONG
  decision (`style::effective()` at session creation). Legacy `shader_fixes.dancer_lighting/outlines/
  lit_models` → migration sources only.
- Never `git commit`/push; never write absolute local paths/usernames into tracked files.
