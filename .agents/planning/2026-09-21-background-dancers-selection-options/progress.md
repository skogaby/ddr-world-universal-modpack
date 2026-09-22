# Progress — Background Dancer / Stage selection options with live 3D previews

Updated: 2026-09-21
Status: ALL SIX STEPS COMPLETE; live previews CABINET-CONFIRMED 2026-09-22 (CrossOver, 1280×720) after the
availability-latch fix; badge margins rebalanced. Remaining: the rest of the §7 matrix (1080p / 480p, Windows, 2P
simultaneous, Step 4 1P/2P regression lines) at the maintainer's convenience. Uncommitted on `v1_4`.
NEXT ACTION (maintainer): deploy the release DLL + `data_mods/` (12 option PNGs, `background_dancers/tex/preview_random.png`)
+ the repo `mod-config.json`'s two `option_menu_settings` entries, then run the validation matrix below and record the
results here. Any FAIL ⇒ hand the log to an agent with this file; the compositor smoke (`DDR_DANCERS_VIEWPORT_SMOKE=1`)
is the bisect tool if the 3D never appears.

Resume protocol: read `implementation/plan.md` (checklist + per-step demo), `design/detailed-design.md`
(normative; note the §4.7 amendment of 2026-09-21), `research/preview-compositing.md` (Ghidra evidence for
Step 2/3), then this file. Task files: `.agents/tasks/2026-09-21-background-dancers-selection-options/stepNN/`;
per-task working records: `.agents/scratchpad/2026-09-21-background-dancers-selection-options/<task>/`.

## Done

- **Step 1 (2026-09-21)** — four tasks, all `Status: Complete (uncommitted)`:
  - task-01 `ScalarFormat::Dynamic(DynamicLabelFn)`; `format_scalar_value(id, value, format)` /
    `_utf8(id, …)` id-threaded; both call sites (`rows.rs`, `registry.rs`) pass `&opt.id`.
  - task-02 pure `background_dancers/catalog.rs` (`Catalog`/`Kind`/`split_key`/`label_for`/`build_catalog`/
    `clamp_to_catalog`, RANDOM label owned here) + `selection::resolve_choice` + `PickSource`; stock
    catalog pinned: 26 dancers / 25 stages, longest label `REPLICANT #6`; harness mounts `catalog.rs`.
  - task-03 `background_dancers/options.rs` (rows `background_dancer` / `background_stage`, scalar
    0..=N step 1/coarse 5, `PersistMode::Local`, `.in_game_only()`, `Duplicate` re-arm, stage row
    `versus_mirror`ed), `lifecycle::tables_snapshot`, `window_entry` order pin → option → random with
    per-element `{source}` in `Pick::summary()`, enable/disable wiring, `generate_chrome` → `pub(crate)`.
  - task-04 `option_strings.py` LABELS/TEMPLATES (SPLIT layout, marker `(191, 11, 170, 150)`),
    PNGs regenerated (4 new per language, nothing else changed), `check_option_takeover.py` skips
    reference-less templates, `mod-config.json` placement after `arrow_opacity` (`overlay: false`).
  - Gates: `cargo check` clean, `cargo fmt` (no unrelated churn), `validate_custom_options.sh` 57 ✓,
    `validate_background_dancers.sh` 117 ✓, `./build.sh` release clean.

- **Step 2 (2026-09-21)** — `scene3d_resolve_viewport`: nine AOBs (`render_graph_boot_attach`,
  `render_graph_2d_attach`, `viewport_detach`, `model_pass_ctor`, `model_pass_enable_tail`,
  `scene_manager_camera_copy`, `viewport_setup_rect`, `worker_gd_write`, `target_list_clear`), cross-checked
  all-or-nothing, published as `scene3d_vp_*` (8 addresses + 25 values), `Scene3dSites.viewport:
  Option<Scene3dViewportSites>`; OPTIONAL sub-group (never in `required_signatures`). Sweep ALL GREEN on
  20250805/20260224/20260721/20260825/20260915 with every value == the design table; `shape_diff.py` clean
  (window 0x100). `scene3d::init` logs `scene3d viewport pass: available|unavailable`.
- **Step 3 (2026-09-21, code)** — `scene3d/camera_math.rs` (pure, engine LookAtRH + D3D off-centre proj, 6 tests),
  `scene3d/viewport_pass_layout.rs` (pure: `ClearViewport` 0x40 repr(C) pins, Clear record encoder, `RtRect`,
  prio/bit tables, 4 tests), `scene3d/viewport_pass.rs` (engine-facing: availability check incl. live filter
  free-bit verification, RWX vtable + worker Clear callback, `PassSet` create/set_rect/set_camera/set_enabled/
  detach, 2-frame reaper, `render_target_dims`), `background_dancers/viewport_smoke.rs` (the dev knob) + mod
  wiring (ONE `viewport_pass::reap()` per frame in `mod.rs`). Host: 127 ✓ / 57 ✓; release build clean.

- **Step 4 (2026-09-21)** — three tasks, all `Status: Complete (uncommitted)`:
  - task-01 pure `background_dancers/pick.rs` (`Pick.stage: Option<StageCandidate>`, `ParseOptions { shadow }` +
    `GAMEPLAY`/`PREVIEW`, `arcs_for(opts)`, `assemble_pick_opt`; `summary()` byte-identical for `Some`, `stage=none{src}
    parts=0` for `None`); `session::parse_pick(pick, opts)` skips stage / shadow / camera work accordingly.
  - task-02 pure `background_dancers/instance_plan.rs` (`InstanceKind`/`Instance`/`restyle_allowed` moved; `plan_instances`
    = the exact table order with `slot_base` + owner budget + `item_pass_mask` override, hull twins copy it) →
    `Session::new(.., slot_base, item_pass_mask)` (gameplay `0, None`), `Session::with_schedule(fallback)`,
    `schedule::synthetic_schedule(period_s)` (9.0 ⇒ cuts every 7.5 s).
  - task-03 `background_dancers/scene_window.rs` — `SceneWindow` (load → parse thread → residency-gated build → publish →
    three-phase teardown → finish/neutralise) extracted from `lifecycle::Window` verbatim behind a `tag`/`scope` prefix;
    `lifecycle.rs` is the gameplay wrapper (clock, tempo, hide, movie size, camera slot 0). Log-string inventory
    (55 strings) identical before/after.
  - Gates: `cargo check` 0 warnings, harness 141 ✓, `./build.sh` clean, `cargo fmt` no unrelated churn.

- **Step 5 (2026-09-21)** — two tasks, `Status: Complete (uncommitted)`, cabinet demo pending:
  - task-01 pure: `Pick::stage_only` / `Pick::dancer_only` (source `Option`), `preview/layout.rs` (`CHROME_ORIGIN`,
    `FALLBACK_MARKER`, §5.3 constants, dancer/fallback camera constants, `BACKDROP_ARGB`, `box_rect`, `crop_to_aspect` /
    `dancer_extents` / `fallback_extents` — the §4.7 crop rule), `preview/state.rs` (generic `SlotState<I>`: request /
    clear / poll → None|Teardown|Start with the 150 ms settle re-armed on value CHANGE only).
  - task-02 engine-facing: `preview/scene.rs` (`PreviewWindow`, `build_pick` with the 16-part cap, `make_session` =
    `Session::new(.., REAL_TIME, style::effective().style, HullPlan::none(), SLOT_BASE[side], Some(FILTER_BIT[side]))
    .with_schedule(synthetic_schedule(9.0))`), `preview/camera.rs` (`frustum_for`: camanm sample cropped / fallback /
    dancer; `apply`), `preview/mod.rs` (the per-side driver: callbacks record only; `on_frame` drives retiring → poll →
    start/teardown → live window → camera → passes enabled iff built ∧ !mod_menu open; lazy `PassSet` per side kept
    attached-but-disabled; one-shot WARN classes). Wired into `mod.rs` (`init` / `on_scene_change` / `on_frame` before
    the single `reap()` / `shutdown`). `options::{kind_for_option, choice_key}`.
  - Gates: `cargo check` 0 warnings, harness 152 ✓, custom_options 57 ✓, `./build.sh` clean, `cargo fmt`.

- **Step 6 (2026-09-21)** — two tasks, `Status: Complete (uncommitted)`:
  - task-01 `preview/badge.rs` (RANDOM badge `ImageWidget` per side, `scrub_indicator` asset pattern, one-shot WARNs) +
    `scripts/gen_preview_random_badge.py` → `data_mods/background_dancers/tex/preview_random.png` (170×150); driver step 4
    (`badge_on = modal_open ∧ focused ∧ wanted None ∧ live None ∧ !mod_menu open`), `focused_kind` tracking, cached
    `marker_for(kind)`.
  - task-02 WARN audit (the > 16-parts truncation WARN is now once per stage key), `docs/background_dancers_research.md`
    §5 (5.1–5.8), AGENTS.md Background Dancers row (options + compositor + driver + fail-open + knobs), README (the two
    options + the live preview), this matrix.
  - Gates: `cargo check` 0 warnings, harness 152 ✓, custom_options 57 ✓, `./build.sh` clean, `cargo fmt`, no new
    `/Users|/home` hits in tracked files (scratchpad logs sanitised).

## In flight

- Nothing — the cabinet validation matrix (below) is the maintainer's. Steps 1–6 uncommitted on `v1_4` (maintainer commits manually).

## Deploy & test log

- **2026-09-21 Step 1 cabinet demo: PASS** (maintainer, CrossOver install) — rows appear under PLAYFIELD
  STYLING OPTIONS with the expected labels, values cycle and function, gameplay honours the choice.
- **2026-09-21 Step 3 cabinet smoke: RECTANGLE SEEN (PASS on the load-bearing question)** — maintainer: a solid
  rectangle appeared over the P1 box at song-select entry above the UI, no crash. Two observations to fold in:
  (a) it was BLUE, not violet — the DLL's fault, not the engine's: `SMOKE_COLOR = 0xFF20A0FF` is a D3DCOLOR ARGB
  u32 = A FF, R 0x20, G 0xA0, B 0xFF = azure. Violet is `0xFFA020FF` (R A0, G 20, B FF). This CONFIRMS the record
  is consumed as an ARGB D3DCOLOR exactly as encoded. (b) it seemed to last 1–2 s rather than 3 s — unverified;
  check the log timestamps between `viewport_smoke: attached` and `… disabled` before assuming a driver problem
  (the phase timer is `Instant`-based inside `on_frame`). 1080p / 480p runs not yet done.
  **Follow-up applied:** `SMOKE_COLOR` corrected to `0xFFA020FF` (violet) in `viewport_smoke.rs`; Step 3 ticked.
- **2026-09-22 Steps 4–6 cabinet run (maintainer, CrossOver): previews PASS after one fix.** First deploy: the RANDOM badge
  rendered, but no dancer/stage preview — log lines 2224–2225 `viewport_pass: unavailable -- stock pass 0 … failed its
  identity gate` + `BackgroundDancers: 3D previews unavailable this boot` at MOD ENABLE: `preview::init()` probed the live
  MODEL pass objects before the engine's render-graph boot had constructed them (null globals) and the availability latch
  stuck "unavailable" for the boot. Fix: `viewport_pass::availability()` = `Available | Unavailable | NotYet` (null pass
  global / unreadable RENDER_2D list ⇒ `NotYet`, one INFO, NOT latched); `preview::init` checks only the derivation;
  `start_preview` retries on `NotYet`. Second deploy: **dancer + stage previews render, "everything looks great"**; the
  only note was the RANDOM badge's unbalanced top/bottom margins — `gen_preview_random_badge.py` now centres the
  die + caption block (regenerated PNG; deploy `data_mods/background_dancers/tex/preview_random.png` again).
- (reference) Step 1 deploy needs the DLL **plus** the 12 new PNGs under
  `data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/` **plus** the two
  `option_menu_settings` entries in `mod-config.json` (a DLL-only deploy leaves the rows unlabelled and
  unplaced). Expect: rows under PLAYFIELD STYLING OPTIONS cycling `RANDOM, AFRO #1, AFRO #2, ALICE #1, …`
  (dancer) / `RANDOM, BOOM #1, …, BOOM #7, CLUB, CRYSTALDIUM, …` (stage); values persist in
  `custom_options.p1/p2.background_*`; stage mirrors P1→P2 in 2P; 0-0-0 menu does not list them; the next
  song's `BackgroundDancers: stage=…{option} … dancers=[…{option}] …` INFO. Boot log should carry
  `BackgroundDancers: option rows live -- BACKGROUND DANCER (26 entries) / BACKGROUND STAGE (25 entries, …)`.

## Validation matrix (design §7) — maintainer, results to be recorded here

Deploy: DLL + `data_mods/` (the 12 `seop_*background_*` PNGs, `background_dancers/tex/preview_random.png`) + the two
`option_menu_settings` entries. `mods["background-dancers"] = true`. Boot log must carry `scene3d viewport pass: available`
and `viewport_pass: available -- stock filters [0x1, 0x56, 0x10, 0x46] …` (else every preview item below degrades to
"badge/chrome only" by design).

| # | Item | Where | Expect (visual) | Expect (log) | Win 720p | Win 1080p | Win 480p | CrossOver |
|---|------|-------|-----------------|--------------|----------|-----------|----------|-----------|
| 1 | Rows + labels + persistence + mirror + not in 0-0-0 menu (Step 1, already PASS on CrossOver) | OPTIONS → PLAYFIELD STYLING | `RANDOM, AFRO #1, …` / `RANDOM, BOOM #1, …`; reboot keeps values; 2P stage follows P1 | `BackgroundDancers: option rows live -- … (26 entries) / … (25 entries …)` | | | | PASS 09-21 |
| 2 | Gameplay honours the choice; RANDOM unchanged (Step 1 PASS); Step 4 regression 1P + 2P | a song | dancers + stage as before | `BackgroundDancers: stage=…{option} … dancers=[…{option}]`; lifecycle lines unchanged, no new WARN | | | | |
| 3a | Compositor smoke (`developer_mode` + `DDR_DANCERS_VIEWPORT_SMOKE=1`) | song-select entry | VIOLET rectangle over the P1 box 3 s, gone 1 s | `viewport_smoke: attached … rt=WxH` → `… disabled` (≈3000 ms later) → `… detached -- sequence complete` → `viewport_pass: reaped 1 set(s)` | | | | azure seen 09-21 (colour fixed) |
| 3b | Dancer preview: BACKGROUND DANCER = `EMI #2`, focused ≥ 150 ms | options modal | Emi dances in the box over a dark backdrop, chrome around | `BackgroundDancers: preview P1 -- stage=none{random} … dancers=[emi01(F) …] (box rt=… aspect 1.133)`, `viewport_pass: attached clear@0x68 … filter=0x8`, `BackgroundDancers: preview P1: FileManager::Load accepted …` / `parsed in` / `built`, `… visible N ms after request` | | | | |
| 3c | Stage preview: BACKGROUND STAGE = `BOOM #3` | options modal | the stage animates, camera cuts every ~7.5 s, cropped (not squashed) | stage pick line with `dancers=[]`; same lifecycle lines | | | | |
| 3d | RANDOM badge | either row at `RANDOM` | the die badge fills the box; no 3D | one `WidgetRenderer`/resolve line for `preview_random` per boot | | | | |
| 4a | Scrub values fast → one restart after the last change | either row | preview follows the LAST value | one `… ends` + `preview exit -- N node(s) disabled` + `preview teardown -- destroy(s) queued` + `… destroyed by the engine flush` + `preview N arc handle(s) freed` per settled change | | | | |
| 4b | Focus another row / close the modal / leave song select mid-preview, then play a song | | preview gone; the next song's dancers normal | teardown lines; gameplay lifecycle lines unchanged | | | | |
| 4c | Both players preview at once (P1 dancer, P2 stage); open the 0-0-0 menu | both modals | both boxes render; both hidden while the overlay menu is open | P2 uses `clear@0x6b … filter=0x20` | | | | |
| 5 | Custom resolution 1920×1080 and 640×480 | options modal | box stays inside the panel (RT rect scales) | `(box rt=…)` = the 720p rect × dims/(1280,720) | | | | |
| 6 | CrossOver: the whole set (D3DMetal viewport + Clear semantics) | | | | — | — | — | |

## Deviations & open questions

- Step 2: nine AOBs, not seven (`render_graph_2d_attach` for the RENDER_2D list offset; `model_pass_enable_tail`
  for the DISTANTVIEW global used by the free-bit check); `model_pass_enable_tail` legitimately hits TWICE (pass
  ctor tail + SceneGraphManager ctor) — both must agree. Published names are `scene3d_vp_*`.
- Step 3: pure parts of the compositor live in `viewport_pass_layout.rs` (harness-mountable); the smoke's
  `CHROME_ORIGIN` constant moves into `preview/layout.rs` in Step 5.
- Step 4: the instance-table planner is a PURE file (`instance_plan.rs`, host-tested) rather than inline in `Session::new`
  — the plan's "Session::new slot bases / mask override" host tests needed a mountable home; `Session::new` consumes it.
  `assemble_pick_opt` is NOT re-exported from `session` (an unused re-export warns in this crate) — import it from `pick`.
  The camera-director INFO clones the two camera-name lists once (borrow split); text unchanged.
- Step 5: the colour clear (`BACKDROP_ARGB` dark blue-black) is applied to BOTH preview kinds (the design left the
  stage's optional) — it covers the chrome where a cropped stage has no geometry. A hard start failure marks the identity
  "attempted" (no per-frame retry; a value change resets). `shutdown` detaches the pass sets but their blocks wait for a
  later `reap()` (the frame callback is gone at disable). The preview time base is `TempoOptions::REAL_TIME` (a `Session`
  parameter — `dance_schedule` is `None` without dancers anyway).

- **Design §4.7 amended by the maintainer (2026-09-21):** templates keep the SPLIT layout (text left,
  preview box right); the box is NOT 16:9 (170×150) — the stage preview will render a cropped view
  (Step 5 `preview/camera.rs`: keep the `.camanm` vertical extent, horizontal = `half_tangent_y ×
  box_aspect`), the dancer preview frames at the box aspect. FR-8 / §2.3(4) read with this amendment.
- Pure catalog functions live in `catalog.rs` (harness-mountable), not `options.rs` (which needs `crate::`).
- `stage_choice(side)` takes the side (the design's had none) — the first entered side's value is read.
- `Pick` provenance renders `{random}` too, not only option/pin.
- Open: none.

## Key facts for a cold resume

- Rows exist only while `background-dancers` is enabled (it is in `DEFAULT_OFF_MODS` — flip
  `mods["background-dancers"] = true` in `mod-config.json` to test).
- Live enable ⇒ row textures appear next launch (framework-wide one-shot atlas flush) — expected.
- Step 2 must NOT add `derive_scene3d_viewport` to any `required_signatures`; it is an OPTIONAL sub-group of
  `Scene3dSites` (like `texture_lookup`).
- Step 3 smoke knob: `layeredfs.developer_mode` + `DDR_DANCERS_VIEWPORT_SMOKE=1`; if the violet rectangle
  does not appear / bleeds / crashes ⇒ STOP and report (invalidates design §4.5).
- Preview slot bases P1 = 0 / P2 = 16; filter bits 0x08 / 0x20; priorities 0x68–0x6A / 0x6B–0x6D.
- Step 5 builds on: `SceneWindow::{start, drive_assets, publish, begin_teardown, drive_teardown, finish}`
  (`scene_window.rs`), `pick::assemble_pick_opt` + `ParseOptions::PREVIEW`, `Session::new(.., slot_base,
  Some(FILTER_BIT[side]))` + `with_schedule(synthetic_schedule(9.0))`, `viewport_pass::{create, PassSet}`,
  `camera_math::view_proj`, `CamSample::frustum()`; `viewport_pass::reap()` is called ONCE per frame in `mod.rs`.
