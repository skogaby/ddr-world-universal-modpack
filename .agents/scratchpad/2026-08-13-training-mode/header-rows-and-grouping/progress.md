# Progress: Header rows (UiKind::Header) + TRAINING OPTIONS grouping

Updated: 2026-08-15
Status: Complete ff8aa39 (v6 demo PASSED, all task ACs verified on the
cabinet; code + assets landed in the maintainer's commit ff8aa39
"Finishing out most of the rest of training mode"; plan Step 8 ticked
with an "As landed" note)

## Deploy & test log

- **v6 (2026-08-15)**: demo round 5 (new1/new2.png): v5's 35px + nudge got
  the top margin right but STILL bled below (the text-zone origin/box
  geometry doesn't leave room for a full-box bitmap without overshoot).
  Maintainer decision: revert to the view.png presentation — the 352x16
  bar centered in the row's text zone with margin on all four sides —
  keeping the invalid_usr hide (gray gone, confirmed in round 4/5).
  Changes: texture back to 352x16 / baseline 12; HEADER_PIN_Y_NUDGE
  removed (plain pin). Final header anatomy: full-height row slot,
  stock row frame art, centered dark-blue/white heading strip, no value
  box, cursor-skipped, R10-gated. Gates: check/fmt/build clean. DLL
  deployed; PNG prepared IN-REPO ONLY — the maintainer copies data_mods
  wholesale per test (install copy deleted on their side; do not deploy
  assets directly anymore).
  **Demo round 6: PASS — "everything works as expected now"
  (maintainer, 2026-08-15).** Header renders as the centered dark-blue
  strip, no gray cover, cursor skips it, scrolls with its group.

- **v5 (2026-08-15)**: demo round 4 (view2.png + gap.png): gray cover GONE
  (invalid_usr fix confirmed); 32px texture bled ~4px into the next row
  with an 18px gap above. Screenshot-measured the geometry instead of
  guessing (gap.png, 2784px window): value boxes on a 60 screen-px pitch
  with tops at 618+60k ⇒ the header slot's grid line is 798; the blue bar
  rendered 810..853 (32 tex px = 44 screen px ⇒ tex→screen 1.375, and the
  label-layer origin sits 12 screen px BELOW the grid line). Fixes:
  (a) texture → 352x35 (35 x 1.375 ≈ 48 screen px = exactly one value-box
  height), baseline 22; (b) `HEADER_PIN_Y_NUDGE = -5.5` layout units
  (12 screen px at the width-derived 2.175 screen-px/unit) added to the
  header's per-frame position pin — safe because every other clip child
  is hidden, so only the label bitmap moves. Correction rule if residual
  offset shows: 1 screen px ≈ 0.46 layout units. Gates: check/fmt/build
  clean; DLL + PNG deployed.

- **v3 (2026-08-15)**: demo round 2 (3.png/4.png): cursor skip WORKS both
  directions; but the clip Y=0.5 scale scaled about the row's vertical
  CENTER (bar floated with gaps, still touched the next row) and the
  option_usr counter-scale left the text visually stretched. Maintainer
  DROPPED the half-height requirement: full-height row; header look
  carried entirely by the texture (opaque dark blue, white centered text
  at 70% label size). Removed the `+0xA8` halving + both scale calls;
  restyled the texture (352x16 at this point). Gates green, deployed.

- **v4 (2026-08-15)**: demo round 3 findings (view.png): cursor + layout
  fully correct; texture rendered half the row height (the stock 16px
  label canvas covers only a row's TEXT ZONE — about half the ~32px row
  box; the bitmap renders at natural size), and the whole row was
  darkened by a gray cover. Fixes: (a) header canvas → 352x32 (full row
  box), text size unchanged, baseline re-centered at 21; (b) the gray is
  the donor clip's `invalid_usr` disabled-row cover, visible in the
  clip's authored default state (ordinary rows get it cleared by the
  donor's `onFocusChanged(false)`, which headers deliberately stub) —
  `render_header` now hides `invalid_usr` per frame like `choice_usr`.
  Gates: check/fmt/build clean; DLL + PNG deployed.
  Watchpoint: if the 32px bitmap is anchored below the row top (rather
  than covering the box exactly), the bar will bleed into the next row —
  then we shrink to 24px or add a y offset on the option_usr layer.

- **v1 (2026-08-15)**: initial build. Demo round 1 (maintainer, screenshots):
  ordering/label/choice_usr-hide all correct — header shows as a gray
  full-width bar with centered TRAINING OPTIONS text (the donor clip's
  default frame state: our no-op `+0x28` slot-1 never runs the donor's
  `onFocusChanged(false)` normalization, so the clip keeps its authored
  gray look — a happy accident that reads as a heading). TWO BUGS:
  1. Row art still FULL height — `+0xA8` halving shrinks only the layout
     slot, so the next row packs half a row up and overlaps the bar's
     bottom half.
  2. Cursor BLOCKED at the header (not skipped). Root cause: the mod
     scroll driver (`options_scroll.rs`) replaces the native directional
     scan whenever the Mods tab overflows the 7-row viewport (33 rows ⇒
     always) — its `predict_target` was a bare `pos ± direction` clamp
     with no selectability test, so it steered focus onto the header and
     the native caller rejected the move every step. The research §2
     native-scan skip we relied on never runs on this tab.
- **v2 (2026-08-15)**: fixes — (a) `render_header` now component-scales the
  row clip to Y=0.5 (`mc_set_scale`, absolute + idempotent per frame) and
  counter-scales `option_usr` by 2.0 so the label texture renders at
  natural size (net 1.0 — no texture scaling, per maintainer preference);
  (b) `RowHandle` gains `selectable` (false for headers, from
  `rows::row_entries_for_side`), `predict_target` skips unselectable rows
  (signum-stepped, zero-direction guarded, boundary semantics preserved),
  and tab-strip entry lands on the first/last SELECTABLE row. Gates:
  harness 324/324, check/fmt/build clean. DLL redeployed (config + PNG
  unchanged).
  Round-2 watchpoints: bar actually half height with no overlap (if the
  bar shrinks toward its center instead of its top, we need a +y/4
  position correction in the pin); label text vertically placed sanely
  inside the half bar; cursor skips the header BOTH directions + from the
  tab strip; header scrolls with its group.

## Checklist

- [x] Setup: working dir, approval chain verified, context.md
- [x] Explore: ordering/builder_hook/api/registry/rows/training_mode/script read
- [x] Plan: plan.md (Status: Approved 2026-08-15, auto-mode upstream approval)
- [x] R10 ordering: harness mount + 9 tests (red: 2-arg signature) →
      compute_order takes (registered, is_header, configured: Option) with the
      identity fast-path inside the pure fn → green (9/9)
- [x] API/registry: header_rows_tests.rs, 9 tests (red: missing variant/ctor/
      error) → UiKind::Header + RegisterSpec::header +
      RegisterError::HeaderCarriesState + try_register validation matrix
      (persist≠None / non-default callback via std::ptr::fn_addr_eq /
      ShowWhen link / transforms / default_value≠0) + preview_image_names
      Header arm → green (harness 324/324)
- [x] rows.rs: RowKind::Header; shared +0x28 vtable (lazy, donor-COL at [-1],
      mod stubs header_not_selectable/header_focus_noop);
      allocate_header_row_for_option (ArrowColor donor, primary slots 4/6
      noop + slot 7 render_header_trampoline, NO +0xC0 clone, +0xA8 f64
      halved with finite/positive guard, Page6 tag, fail-open WARN paths);
      render_header (position pin + option_usr label bind + choice_usr
      subtree hidden per frame); Header arms on the two exhaustive UiKind
      matches (push_scalar_value_text, press_body — both defensive returns)
- [x] builder_hook: RowKindTag::Header, is_header slice into the 2-arg
      display_order_for, Header allocation routing, injection-time label bind
      extended to Enum|Header
- [x] training_mode: OPT_HEADER = "header_training_options" registered
      best-effort after the placement row (failure = one WARN, never blocks
      the functional rows); set_option_available(true/false) at enable/disable
- [x] Asset: gen_option_labels.py HEADER_LABELS family (352x16, black,
      centered, same baseline/pads) → seop_item_header_training_options.png;
      regen confirmed byte-identical for every pre-existing PNG
- [x] row_order: "header_training_options" inserted before training_loop_song
      in BOTH mod-config.json copies (repo + install)
- [x] Gates: harness 324/324 (306 baseline + 18 new) → cargo check clean →
      cargo fmt clean → ./build.sh clean
- [x] Deploy: DLL + header PNG copied to $DDR_WORLD_INSTALL
- [x] Cabinet demo — PASSED round 6 (2026-08-15), all ACs verified
- [x] On demo PASS: plan Step 8 ticked (+ "As landed" note), this record
      closed (Status: Complete (uncommitted)), feature progress → Step 9

## Demo checklist (plan Step 8)

Launch with a CLEAN environment (no stale DDR_*_FAULT vars). On the MODS tab:

1. Slim full-width TRAINING OPTIONS header renders above the training rows
   (position 19 of row_order, before LOOP SONG) — no value box, no marker,
   no tri-arrows, no preview panel when anything near it is focused.
2. Cursor skips it: navigate down across it, up across it, and re-open the
   tab — focus never lands on the header.
3. Scroll: with enough rows visible, scroll the window across the header's
   position — it masks/unmasks like any row and stays with its group.
4. R10 removal leg: delete "header_training_options" from
   custom_options.row_order in the install config, relaunch — the header is
   absent entirely; all normal rows unchanged.
5. Log greps: "allocated header row for" on menu open;
   no "header … — header absent" WARNs; no new WARN spam.

Look-tuning knobs if the demo wants adjustments (all cheap):
- Slot height: the 0.5 factor in allocate_header_row_for_option.
- Label art width/centering/styling: HEADER_WIDTH / render_header_label in
  scripts/gen_option_labels.py (regenerate + copy PNG).

## TDD cycles

1. ordering tests (9) written against the new 3-arg compute_order → red
   (E0061 ×8 in harness) → refactor → green, suite 315.
2. header_rows_tests (9) → red (E0599 missing Header/header/HeaderCarriesState)
   → api.rs + registry.rs implementation → green, suite 324.
3. rows.rs/builder_hook/training_mode (engine-facing, no harness surface) —
   compile-driven; exhaustive-match errors surfaced the two defensive Header
   arms. Cabinet demo is the real validation.

## Deviations

- None from the task contract. Auto-mode decisions of note (context.md
  "Assumptions"): +0xC0 preview-vtable clone skipped for headers; choice_usr
  subtree hidden per frame in the header render (the donor clip otherwise
  shows the value-box art); header label = 352x16 centered black (2× label
  width, natural-size render extends across the hidden value area); callback
  check via std::ptr::fn_addr_eq against the api-module default noop.
- SOP commit step intentionally not run: repo convention (AGENTS.md Workflow)
  — the maintainer manages all git; step-7 record closed the same way.

## Validation results

- Harness: 324 passed / 0 failed (baseline 306 + 9 ordering + 9 header
  validation). Logs: logs/harness-final.log.
- cargo check x86_64-pc-windows-msvc: clean (one fn-cast lint fixed with
  fn_addr_eq during the run). Logs: logs/cargo-check.log.
- cargo fmt --check: clean.
- ./build.sh: clean, DLL at target/x86_64-pc-windows-msvc/release/. Logs:
  logs/build.log.
- Asset regen: only the new header PNG appeared in git status — every
  regenerated existing PNG byte-identical.

## Files touched (this task)

- src/services/custom_options/ordering.rs (R10 + in-file tests)
- src/services/custom_options/api.rs (UiKind::Header, RegisterSpec::header,
  HeaderCarriesState, is_default_on_change)
- src/services/custom_options/registry.rs (validation + preview arm)
- src/services/custom_options/rows.rs (header build/render/vtables)
- src/services/custom_options/builder_hook.rs (Header routing + is_header)
- src/services/custom_options/mod.rs (test-module mount)
- src/services/custom_options/header_rows_tests.rs (new)
- src/mods/training_mode/mod.rs (OPT_HEADER registration/availability)
- scripts/gen_option_labels.py (HEADER_LABELS family)
- data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/
  seop_item_header_training_options.png (new)
- mod-config.json (row_order insertion; install copy too)
- Harness main.rs (ordering.rs + header_rows_tests.rs mounts — temp-dir,
  not in repo)
