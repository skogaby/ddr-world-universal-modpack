# Implementation Plan — Background Movies on the stage screens

Status: Approved 2026-09-23

Design: `.agents/planning/2026-09-23-background-dancers-monitor-movies/design/detailed-design.md` (approved
2026-09-23). Requirement ids R1–R16 refer to its §2. Progress lives in
`.agents/planning/2026-09-23-background-dancers-monitor-movies/progress.md` (created with Step 1, updated
after every step — the AGENTS.md PDD convention).

- [x] Step 1: The two routing signatures and their derivation
- [x] Step 2: STAGE SCREENS end to end on stock screen stages
- [x] Step 3: Unlit screens in every mode and style
- [x] Step 4: MOVIE ONLY (NO DANCERS)
- [x] Step 5: Blender add-on `offscreen1` placeholder and convention
- [x] Step 6: Griffin House TV converted to a stage screen
- [x] Step 7: Documentation, full validation and the cabinet test pass (maintainer: everything works, 2026-09-23)

Every step ends with the repo's readiness gates: `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`
(whole crate), `./build.sh`, plus the step's own tests. No commits (the maintainer commits).

---

Step 1: The two routing signatures and their derivation

**Objective.** Resolve `movie_layer_select` (→ `movie_layer_select_imm`) and `movie_actor_fit_case` (→
`movie_fit_origin_off`, `movie_fit_size_off`) on every supported build, with the identity gates of design
§4.8, before any consumer exists.

**Guidance.** Add both `SignatureDefinition`s to `src/core/signatures.rs` with comments in the house style
(what the bytes are, what is read at which offset, the per-build match table from design Appendix A). One
all-or-nothing `derive_movie_screen_route` called from `resolve_derived` after `derive_layer_table` (it
cross-checks the `MOV RDX,[rip]` target against `layer_table` when resolved). Publish the imm address with
`resolved.insert` and the two offsets with `publish_value`. Neither name goes into any `required_signatures`.
Register the consumers with the sweep's consumer graph as the harness expects (the new names read through
`get_address` / `published_value`), and add the pair to `scripts/sig_harness/report.py` only if the harness
needs a soft-group entry.

**Tests.** `./scripts/validate_signatures.sh <dir of the five gamemdx builds>` ALL GREEN with origin
`0x108`, size `0x120`, imm `09` on each build; `scripts/sig_harness/shape_diff.py` over both — no divergence
inside the bytes read (match−0x1B..match+25 and match..match+61). `cargo test` unaffected.

**Integration.** Nothing consumes the names yet; the boot log gains the `[+]` / `(derived)` lines.

**Demo.** A boot log (or the sweep report) shows `movie_layer_select_imm` and the two published offsets on
all five builds.

---

Step 2: STAGE SCREENS end to end on stock screen stages

**Objective.** Selecting STAGE SCREENS routes a movie song on a stock screen stage onto its screens with A3's
fit; every other stage plays as THUMBNAIL (R1, R2, R4–R7, R9, R12, R13 route/fit diagnostics, R14).

**Guidance.**
- `movie_mode.rs` (pure): `MovieMode::StageScreens` (row 3, key `stage_screens`, label `STAGE SCREENS`, `ALL`
  display order `[Off, Thumbnail, StageScreens, Fullscreen]`), `size_override` row, `Capabilities` +
  `degrade`, `window_mode`, `routes_to_screens`, `arc_members_have_screen`, `SCREEN_RT_EXTENT`,
  `fit_writable`, `RouteImm` / `imm_action` (design §4.1; `probes_backdrop` and `SceneMask::dancers` wait for
  Step 4).
- `style.rs`: row values/labels from `ALL`, new hint, config parse/persist, the WARN listing the spellings.
- `movie_backdrop.rs`: `live_movie_actor()`.
- New `screen_route.rs` (design §4.2) + `init` from `mod.rs::init`, `on_frame` from the frame callback,
  `disarm` in `lifecycle::restore_movie_mode`.
- `custom_scan.rs`: `read_arc_members` → `pub(super)`.
- `lifecycle.rs`: `Tables.screen_stages` built at the end of `init_tables` (+ the enable INFO);
  `window_entry` reads `has_screens` under the pick's `TABLES` lock; `apply_movie_mode(has_screens)` with the
  generalised `degrade` + one WARN per degraded mode per boot, arm-before-size-writes, the per-song INFO.

**Tests.** `movie_mode.rs` host tests (design §7.1 — everything but the MOVIE ONLY cases) run through
`scripts/validate_background_dancers.sh` (update its header comment for the new surface). Existing
`movie_mode` tests adjusted for the longer `ALL` and still green.

**Integration.** Uses Step 1's names; FULLSCREEN / OFF / THUMBNAIL behaviour unchanged (their size table
rows and scene masks are untouched).

**Demo (cabinet deploy #1).** STAGE SCREENS + Lighting Style STOCK, `DDR_DANCERS_PIN=replicant05` (developer
mode) on ENDYMION and `DDR_DANCERS_PIN=monitor00` on a 4:3 movie song: the movie plays on the screens; the log
shows the enable INFO listing ten screen stages, the per-song INFO `stage screens: yes, routed: yes`, the imm
write, one framed-actor INFO, the entry-10 INFO with a node count ≥ 1. A non-screen stage plays as THUMBNAIL
and never writes the byte. Song select after the song: byte back to `09`.

---

Step 3: Unlit screens in every mode and style

**Objective.** Screen materials keep their stock unlit shader and draw no outline in every mode and style,
in gameplay and in the options previews (R8, R13 texture-size diagnostic).

**Guidance.** Pure `materials_sampling` in `src/services/scene3d/render_item_layout.rs`; in
`render_item.rs::restyle_materials` read the per-material masked slot indices and the resource table hashes
(the layout `resolve_material_textures` already walks — share a small reader rather than duplicating it),
exclude sampling materials, count `kept_screen`. `session.rs::build_one`: `screen=<n>` in the restyle INFO
and the one-time `TextureData` size INFO for an item with screen materials. Target hash
`pure::fnv1_name_hash(movie_mode::SCREEN_TEXTURE_STEM)` — keep `render_item_layout.rs` free of
`crate::` imports (pass the hash in).

**Tests.** `materials_sampling` host tests (design §7.1) in the already-mounted `render_item_layout.rs`.

**Integration.** The restyle path is shared by the gameplay window and the previews; hull twins inherit the
exemption through `mark_hull_records` (records of non-restyled materials hidden).

**Demo (cabinet deploy #2).** Lighting Style CEL + SCENE OUTLINES on a pinned screen stage under STAGE
SCREENS: screens show the movie unshaded with no ink rim; the log shows `screen=1+` on the screen parts and
the screen texture as 1280 × 1280.

---

Step 4: MOVIE ONLY (NO DANCERS)

**Objective.** A3's default: while a movie is drawn the whole 3D scene is hidden and the song looks stock
(R3, R12).

**Guidance.** `movie_mode.rs`: `MovieMode::MovieOnly` (row 4, key `movie_only`, label
`MOVIE ONLY (NO DANCERS)`, appended to `ALL`), `size_override` → `None`, `degrade` rule (probe only),
`probes_backdrop`, `SceneMask::dancers` + `NOTHING`, `scene_mask` MovieOnly rule. `director.rs::produce`:
dancer bodies and parts hidden when `!mask.dancers`. `lifecycle.rs::drive_live`: probe when
`probes_backdrop(w.movie_mode)`, log names the mode. `style.rs` hint covers five values.

**Tests.** The MOVIE ONLY cases of design §7.1 (`size_override`, `degrade`, `scene_mask` + `wants_bg_hide`,
`probes_backdrop`, row/key/label round trip over the five-value `ALL`).

**Integration.** Reuses the FULLSCREEN probe and the existing mask plumbing; FULLSCREEN's `DANCERS_ONLY`
now carries `dancers: true` explicitly.

**Demo.** MOVIE ONLY: a movie song shows the stock World background + movie with no 3D; a non-movie song
shows the dancers; VIDEO SIZE OFF keeps the dancers; the log shows the backdrop transitions.

---

Step 5: Blender add-on `offscreen1` placeholder and convention

**Objective.** Custom stages can have screens by naming an image `offscreen1`; the exporter ships an 8 × 8
black placeholder instead of the image pixels (R5 contract, R11).

**Guidance.** `tools/blender_ddr_addon/export_model.py::write_textures_for` (fold = lower-case, `_`
stripped, the same fold the game and `ktmdl.texture_registry_key` use); README "Stage screens" section
(design §4.9); `scripts/build_blender_addon.sh` repackages as usual.

**Tests.** `tools/blender_ddr_addon/tests/synthetic_test.py` case (texture name `offscreen1`, DDS header
8 × 8), run through `scripts/validate_blender_addon.sh <unpacked A3 data root>` — all existing tests still
PASS.

**Integration.** The DLL's has-screens check (Step 2) sees the placeholder member in any stage exported with
the convention.

**Demo.** A headless export of a quad textured with an image named `offscreen1` produces
`offscreen1.dds` (8 × 8) and a model whose texture table names `offscreen1`.

---

Step 6: Griffin House TV converted to a stage screen

**Objective.** The shipped custom stage's TV plays the song's movie under STAGE SCREENS (R10).

**Guidance.** Design §4.10: a script in the maintainer's Blender project (outside this repo; `blender-local`
skill, Blender 5.2.1) that opens the current room `.blend`, renames the TV image to `offscreen1` with a
Blender-only letterboxed preview, rewrites the two screen quads' UVs to the 16:9 band, saves a new
`.blend` version (the previous kept) and exports with the Step 5 add-on. Replace the files in
`data_mods/custom_models/stages/Griffin House/mapset_griffin00/gm_griffin00_room/` from the export (keep
`camera/` untouched), delete `lr_screen.dds`. Render a Blender check image of the TV and inspect it.

**Tests.** `scripts/ktmdl_dump.py` on the new model: mesh 11 samples `offscreen1`, shader
`mdl_bg_constant_vc`, UV v range 0.21875–0.78125, u 0–1; the add-on's `write_model(model_to_spec(m)) ==
data` round trip; the folder contains `offscreen1.dds` (8 × 8) and no `lr_screen.dds`; the other 14 meshes'
vertex/index counts unchanged.

**Integration.** The DLL packs the folder into its cache arc on the next boot and the Step 2 scan lists
`griffin00` among the stages with screens.

**Demo (cabinet).** STAGE SCREENS with `DDR_DANCERS_PIN=griffin00` on a 16:9 and a 4:3 movie song: the TV
plays the movie upright and unmirrored.

---

Step 7: Documentation, full validation and the cabinet test pass

**Objective.** Record the shipped behaviour and close the design's test list (R15, R16).

**Guidance.** `docs/background_dancers_research.md` §8: mark §8.6 implemented, note the deviations
(arm-before-writes, fit writer on the frame callback, the added diagnostics, MOVIE ONLY), the cabinet
results. README: Background Movies paragraph (five values) and the `background_dancers.movie_mode` config
row. AGENTS.md: the Background Dancers Key Entry Points row (STAGE SCREENS / MOVIE ONLY / screen_route /
the two signatures / unlit screens / custom-stage contract) and the `background_dancers` config entry.
Finalise `progress.md`.

**Tests.** Full readiness gates + `scripts/validate_background_dancers.sh`, `scripts/validate_signatures.sh`,
`scripts/validate_blender_addon.sh`; the design §7.3 cabinet list (quick restart, course, training seek,
mod disable mid-song, Custom Resolution 1080p on top of the per-step demos).

**Integration.** No code beyond fixes the cabinet pass finds.

**Demo.** Every §7.3 check recorded in `progress.md`'s deploy log.
