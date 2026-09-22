# Implementation Plan — Background Dancer / Stage selection options with live 3D previews

Status: Approved 2026-09-21

Design: `design/detailed-design.md` (approved 2026-09-21). Section references below (`§x.y`) are
into that document. Repo conventions: no commits by agents; readiness gates before any cabinet
build = `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` (whole crate) → `./build.sh`;
plus `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN whenever `signatures.rs`
or a consumer-side offset changes. Engine-facing code has no host harness — every step ends with a
cabinet deploy and named log lines to look for. Maintain `progress.md` in this directory after
every step (resume point).

## Checklist

- [x] Step 1: Option rows end-to-end (catalog, `ScalarFormat::Dynamic`, registration, persistence, mirror, placement, textures, gameplay application) — implemented 2026-09-21, host suites green, release build clean; cabinet demo pending
- [x] Step 2: Viewport-pass signature derivations (`scene3d_resolve_viewport`, nine AOBs) + four-build sweep — 2026-09-21, ALL GREEN on 5 builds, shape_diff clean; cabinet boot-log check pending
- [x] Step 3: `camera_math` + `viewport_pass` compositor primitive + cabinet clear-rectangle smoke — code complete 2026-09-21 (host suites green, release build clean); **cabinet smoke PASSED on the load-bearing question** (a solid rectangle over the P1 box above the UI, no crash; it rendered azure because `SMOKE_COLOR` was authored `0xFF20A0FF` — fixed to `0xFFA020FF`; the 3 s dwell is to be confirmed from the maintainer's log timestamps; 1080p / 480p runs still to do)
- [x] Step 4: `Pick`/`Session` generalisation + `scene_window.rs` extraction (gameplay regression) — implemented 2026-09-21 (three tasks; harness 141 ✓, `cargo check` 0 warnings, release build clean, gameplay log surface byte-identical by inventory diff); **cabinet 1P + 2P regression pending**
- [x] Step 5: Preview driver — live dancer and stage previews in the options box — implemented 2026-09-21 (two tasks: pure `preview/{layout,state}.rs` + `Pick::{stage_only,dancer_only}` host-tested, engine-facing `preview/{mod,scene,camera}.rs`; harness 152 ✓, `cargo check` 0 warnings, release build clean); **cabinet demo pending**
- [x] Step 6: RANDOM badge, polish, documentation, final validation matrix — code + docs 2026-09-21 (`preview/badge.rs` + `preview_random.png`, per-key WARN latch, research §5, AGENTS.md row, README); **cabinet validation matrix pending** (recorded in `progress.md`)

---

## Step 1: Option rows end-to-end

**Objective.** The two rows exist, render their labels, persist locally, mirror (stage), sit under
PLAYFIELD STYLING OPTIONS, stay out of the overlay menu, and the next song honours the choice.
No preview yet (the box shows the generated chrome).

**Implementation guidance.**
1. `src/services/custom_options/api.rs`: add `DynamicLabelFn` + `ScalarFormat::Dynamic(DynamicLabelFn)`
   (§4.1). Thread the option id into `format_scalar_value` / `format_scalar_value_utf8` (two call
   sites: `rows.rs::push_scalar_value_text`, the overlay snapshot in `registry.rs`/`mod.rs`).
   `Dynamic(f) => f(id, value).unwrap_or_else(|| value.to_string())`.
2. `src/mods/background_dancers/options.rs` (new, §4.2): `split_key`, `label_for`, `build_catalog`,
   `Catalog` behind a `OnceLock`, `label()` (the `DynamicLabelFn`), `register()`, `set_available()`,
   `stage_choice()`, `dancer_choice(side)`, per-side value atomics, `on_change` handlers with the
   `versus_mirror::mirror_edit` tail on the stage row, `clamp_to_catalog` load transform. Keep the
   pure functions free of `crate::` imports (they are mounted by the host harness).
3. `lifecycle.rs`: `pub(super) fn tables_snapshot()`; call `options::register(build_catalog(..))`
   from `BackgroundDancersMod::enable()` after `init_tables()`; `options::set_available(false)` +
   `versus_mirror::unregister` in `disable()`. Treat `RegisterError::Duplicate` as success +
   `set_option_available(id, true)` (the standard re-enable shape).
4. `selection.rs`: pure `resolve_choice` (§4.3). `window_entry`: after the pin branch, build the
   per-side choice list from `options::dancer_choice(i)` for the entered sides (in entered order —
   dancer index *i* = the *i*-th entered side) and `options::stage_choice()`; call
   `resolve_choice` → `assemble_pick(.., pinned = any choice, ..)`; fall through to `make_pick` when
   it returns `None`. Extend `pick.summary()` with the per-element source (`random|option|pin`).
5. `scripts/option_strings.py`: `LABELS` (en/ja/ko) for both ids; `TEMPLATES` entries with one green
   16:9 marker `(x, y, 288, 162)` + two description lines (§4.7); `PREVIEWS` fallback specs.
   Regenerate with `scripts/gen_option_labels.py` (all three language sets; never hand-edit PNGs).
   `webui_options::preview_gen::generate_chrome` → `pub(crate)`; call it for both ids before
   `register_option` (the `webui_options` precedent).
6. `mod-config.json` `option_menu_settings`: insert `background_dancer` then `background_stage`
   (both `overlay: false, in_game: true`) after `arrow_opacity`. Registration also uses
   `.in_game_only()`.

**Tests.** Host: `split_key`, `label_for`, `build_catalog` over the full stock key list (26
dancers / 25 distinct stages, `dummy00` and duplicate rows collapsed, every label ≤ 15 bytes,
sorted by key), `clamp_to_catalog`, `resolve_choice` (chosen key ⇒ only that key's rows; unknown
key ⇒ `None`; all-RANDOM equals `pick_stage`/`pick_dancers` under the same seed), the
`format_scalar_value` `Dynamic` arm incl. the `None` fallback, and the existing ≤ 15-byte scalar
lint extended with the catalog. `scripts/validate_custom_options.sh` and
`scripts/validate_background_dancers.sh` stay green (mount `options.rs`).

**Integration.** Purely additive to `custom_options`; the mod's `enable/disable` gain the
registration calls; `window_entry` gains one branch between pin and random.

**Demo.** Cabinet: open OPTIONS → PLAYFIELD STYLING OPTIONS shows BACKGROUND DANCER / BACKGROUND
STAGE; values cycle `RANDOM, AFRO #1, AFRO #2, ALICE #1, …`; reboot keeps them
(`custom_options.p1.background_dancer` in `mod-config.json`); in 2P the stage row follows P1; the
0-0-0 menu does not list them; the next song's `BackgroundDancers: … pick` INFO names the chosen
stage/dancer with source `option`.

---

## Step 2: Viewport-pass signature derivations

**Objective.** `Scene3dSites.viewport: Option<Scene3dViewportSites>` resolves on all four
supported builds (and the cabinet's), publishing every offset the compositor needs; a miss leaves
it `None` with one WARN.

**Implementation guidance.** `src/core/signatures.rs`: add the seven AOBs of §4.9
(`render_graph_boot_attach`, `viewport_detach`, `model_pass_ctor`, `scene_manager_camera_copy`,
`viewport_setup_rect`, `worker_gd_write`, `target_list_clear`) with descriptions in the file's
house style (what the consumer reads at `match+N`), and `derive_scene3d_viewport` (all-or-nothing;
cross-checks: the boot attach priorities decode to `0x66/0x67/0x68`; the ctor's `0xf8` alloc size;
the pass view/proj offsets equal the tick's memcpy destinations; the worker terminator immediate is
`0x4003a`; the Clear store immediate is `0x140000`; the stock filters read live from the four pass
globals are `{0x01, 0x56, 0x10, 0x46}`). Publish non-address values via `publish_value`
(`viewport_pass_size`, `viewport_gd_write_off`, …). Add the group to the sweep's consumer graph
(`scripts/sig_harness/report.py::ALT_GROUPS` if alternates are needed). Do NOT add it to any
`required_signatures` — the mod must stay usable without previews.

**Tests.** `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN (20250805 /
20260224 / 20260721 / 20260825); `scripts/sig_harness/shape_diff.py --json … --dir …` reviewed
for every new signature (all seven consumers read `match+N`). Host unit tests for any new
`scanner.rs` primitive.

**Integration.** Extends `Scene3dSites` with an optional sub-group like `texture_lookup`;
`scene3d::init` logs `scene3d viewport pass: available|unavailable(<why>)`.

**Demo.** Boot log on the cabinet lists `viewport_* (derived) = 0x…` values; the offline sweep
report shows the group green on all four builds.

---

## Step 3: Compositor primitive + cabinet clear-rectangle smoke

**Objective.** `scene3d::viewport_pass` can attach a ClearViewport and two pass clones into
RENDER_2D with a sub-rectangle, drive their matrices, toggle them, detach and reap them — proven on
the cabinet by drawing a solid coloured rectangle over the song-select UI.

**Implementation guidance.**
1. `src/services/scene3d/camera_math.rs` (pure, §4.4).
2. `src/services/scene3d/viewport_pass.rs` (§4.5): `#[repr(C)] ClearViewport` with size/offset
   asserts, RWX vtable built once (the `node.rs` pattern), `extern "C" fn clear_render(vp, ctx)`
   following the `node_visit` rules (no engine API / alloc / lock / log; write the 0x14-byte record
   at `*(ctx + gd_write_off)` and advance), `PassSet::{create, set_rect, set_camera, set_enabled,
   detach}`, `reap()`, `render_target_dims()`, `is_available()`. Clone = `is_readable` probe +
   `copy_nonoverlapping` + patch self/rect/filter; identity gates on the vftable and stock filter.
   All engine calls game-thread only; attach/detach only from an `on_frame` callback.
3. Dev-mode smoke (`layeredfs.developer_mode` + `DDR_DANCERS_VIEWPORT_SMOKE=1`): at each SONG_SELECT
   entry create a `PassSet` with `ClearSpec { depth: true, color: Some(0xFF20A0FF) }` at the P1 box
   rect for 3 s, then `set_enabled(false)` for 1 s, then `detach` — logging attach/detach/reap and
   the rect in RT pixels. Keep the knob after the step (bisect tool).

**Tests.** Host: `ClearViewport` layout asserts, Clear record byte-exactness, `RtRect` mapping at
three resolutions, `camera_math` cases (§7), priority/bit tables. Cabinet: the smoke.

**Integration.** New service module under `scene3d`, consuming Step 2's sites; nothing else calls
it yet.

**Demo.** Cabinet: a solid violet rectangle appears exactly over the P1 preview-box area of the
options panel region for 3 s after entering song select (above the wheel/UI), disappears for 1 s,
then is detached; the log shows `viewport_pass: attached P1 clear@0x68 opaque@0x69 trans@0x6A
rect=(x,y,w,h)` and `reaped`; a following song plays normally. Also run at 1920×1080 and 640×480.

---

## Step 4: `Pick`/`Session` generalisation + `scene_window.rs` extraction

**Objective.** The gameplay window keeps working exactly as before while its load/build/teardown
machinery becomes reusable, and picks/sessions can describe a stage-only or dancer-only scene with
a slot base and an item-pass-mask override.

**Implementation guidance.** `session.rs` (§4.8): `Pick.stage: Option<StageCandidate>`,
`Pick::arcs/summary`, `parse_pick(pick, ParseOptions { shadow })`, `Session::new(.., slot_base,
item_pass_mask: Option<u32>)`, `Session::with_schedule` (synthetic schedule when there are no
dancers). `lifecycle.rs`: move `Window`'s asset/scene phases and `request_load`, the
`build_pending` loop, `begin_teardown`, `drive_teardown`, `finish_window` into
`background_dancers/scene_window.rs` as `SceneWindow` (gameplay-only fields — clock, tempo, hide,
movie size, one-shot log latches — stay in a `lifecycle.rs` wrapper). No behaviour change: same
constants, same log lines, same orphan handling. Gameplay call sites pass `slot_base = 0`,
`item_pass_mask = None`, `ParseOptions { shadow: true }`.

**Tests.** Host: `Pick` with `stage = None` (arc list, summary), `Session::new` slot bases and
mask override (instances' `slot` and `pass_mask`), synthetic schedule cut times, stage-only /
dancer-only `Session` construction over fixture `Parsed` bundles. `scripts/validate_background_dancers.sh`
green. Cabinet regression: one 1P and one 2P song with dancers — the boot/pick/built/visible/
teardown INFO lines match the pre-step log shape; `BUG-`/WARN-free.

**Integration.** Refactor only; `lifecycle.rs` shrinks, `scene_window.rs` appears; Step 5 builds
on it.

**Demo.** A normal gameplay song shows dancers + stage as before (cabinet), and the log carries the
same lifecycle lines.

---

## Step 5: Preview driver — live dancer and stage previews

**Objective.** Focusing BACKGROUND DANCER / BACKGROUND STAGE with a non-RANDOM value shows the
live animated 3D preview in the row's box for that side; edits re-target after 150 ms; focus
loss / modal close / scene exit tear it down; both sides work at once.

**Implementation guidance.** `src/mods/background_dancers/preview/{mod.rs, scene.rs, camera.rs,
layout.rs}` (§4.6): subscribe to `custom_options::on_preview_request / on_menu_open /
on_menu_close` in `preview::init()` (called from the mod's `enable`), extend the mod's existing
`input_manager::on_frame` callback with `preview::on_frame()` and its scene callback with
`preview::on_scene_change`. Per-side `PreviewSlot`; `PassSet` created lazily on the first live
preview (filter bit / priority base / slot base per side from the constants table §5.3), rect
refreshed per preview from `layout::box_rect(side, marker_rect_for(id, Green))` ×
`render_target_dims()`. Scene construction per §4.6 (`scene.rs`): stage ⇒ `Pick { stage: Some(row
uniform over key), dancers: [] }` + synthetic schedule + `camera_schedule`; dancer ⇒ `Pick { stage:
None, dancers: [c] }` + shuffled playlist, `ParseOptions { shadow: false }`; `style::effective()`
style, `HullPlan::empty()`, `item_pass_mask = Some(FILTER_BIT[side])`. Per frame when Built: `t =
now − built_at`, `director::produce(sess, t, true)`, camera (`camera.rs`: `camera_frame` → or the
fixed viewer/fallback) → `camera_math::view_proj` → `passes.set_camera`; `set_enabled(!mod_menu::
is_open())`; `viewport_pass::reap()`. Teardown = `SceneWindow`'s existing path. Guard everything on
`viewport_pass::is_available()` (else the driver only manages nothing — Step 6 adds the badge).

**Tests.** Host: `layout.rs` (box rect per side, RT mapping, aspect), `scene.rs` pure pick
builders (stage-only/dancer-only shapes, playlist from the sex pool), settle-timer state machine
(`wanted`/`settle_at` transitions on request/value-change/close). Cabinet: the demo matrix.

**Integration.** Consumes Steps 1–4; adds two calls to the mod's existing callbacks and one
`init`/`shutdown` pair.

**Demo.** Cabinet: focus BACKGROUND DANCER, pick `EMI #2` → Emi dances in the box with the modal's
chrome around her (dark backdrop from the colour clear); scrub values quickly → the preview
follows after the settle; pick `BOOM #3` on BACKGROUND STAGE → the stage animates under its own
camera cuts; P1 and P2 preview different things simultaneously; closing the modal / leaving song
select tears down (`teardown … done` INFO) and the next song plays normally; the 0-0-0 menu hides
the preview while open.

---

## Step 6: RANDOM badge, polish, documentation, validation matrix

**Objective.** Ship-quality: the RANDOM value shows its badge, every WARN is one-shot, the feature
degrades cleanly without the compositor, and the documentation records the new engine facts.

**Implementation guidance.**
1. `preview/badge.rs` (§4.6): `ImageWidget` per side bound to
   `data_mods/background_dancers/preview_random.png` (author the 288×162 art), shown iff focused ∧
   value == 0 ∧ modal open ∧ `!mod_menu::is_open()`. Fail-open.
2. Polish: WARN latches per class; the `graph_stats().enabled == false` WARN; the > 16-instance
   truncation WARN; preview refusal while `viewport_pass` is unavailable logs once; `pick.summary()`
   wording; dancer-camera constants tuned on the cabinet (record the final values in the design's
   §4.6 tunables comment).
3. Docs: `docs/background_dancers_research.md` gains a "§5 Viewport passes / RENDER_2D compositing"
   section (target lists, attach/detach, pass layout, worker setup, Clear record, camera math, the
   tag-0x10 correction); AGENTS.md: the Background Dancers row gains the options + preview summary
   (row ids, `PersistMode::Local`, mirror rule, compositor one-liner, fail-open rule, the
   "`ScalarFormat::Dynamic` exists" note, the two dev knobs); README: the two options.
4. Validation matrix run on the cabinet (§7 items 1–6) and on CrossOver; record results in
   `progress.md`.

**Tests.** Host suites all green (`cargo test`, `validate_background_dancers.sh`,
`validate_custom_options.sh`, `validate_signatures.sh`); `git grep -nE "/(Users|home)/[^/ ]+/"`
adds no hits.

**Integration.** Final wiring; no new seams.

**Demo.** The complete feature as specified in the design §1, on Windows and CrossOver, at
1280×720 / 1920×1080 / 640×480.
