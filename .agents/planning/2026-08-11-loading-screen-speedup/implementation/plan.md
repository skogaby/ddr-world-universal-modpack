# Implementation Plan — Loading-screen speedup, Phase 1

Status: Approved 2026-08-11

Design: `.agents/planning/2026-08-11-loading-screen-speedup/design/detailed-design.md`
(Phase 2 — the RAM cache preload — is deliberately NOT planned here; it stays a
design sketch until Phase 1's cabinet win is measured and found insufficient.)

- [x] Step 1: 1-based scalar display (`ScalarFormat::OffsetInteger`)
- [x] Step 2: Convert the 9 cosmetic categories to scalar rows (+ chrome, dead-code removal)
- [x] Step 3: Retire the item-ribbon assets (script + committed PNGs)
- [ ] Step 4: Docs + final build gates + cabinet validation

---

Step 1: 1-based scalar display (`ScalarFormat::OffsetInteger`)

- **Objective:** Add the display-only formatter variant the converted cosmetics
  need (design R3): `ScalarFormat::OffsetInteger { display_offset: i32 }`,
  rendered as `value + display_offset`.
- **Implementation:**
  - `src/services/custom_options/api.rs`: add the variant to `ScalarFormat` with
    a doc comment stating it is display-only (stored value, persistence,
    callbacks, clamping all operate on the raw value).
  - `src/services/custom_options/rows.rs::format_scalar_value`: add the arm
    `OffsetInteger { display_offset } => (value + display_offset).to_string()`
    (use `wrapping_add`/saturating semantics only if clippy complains; plain add
    is fine for the i32 ranges involved). Update the function doc comment.
  - No changes to `advance_value_scalar_trampoline`, clamping, or step logic.
- **Tests:** `format_scalar_value` lives in `rows.rs`, which is outside the host
  harness — so validation is: `cargo check --target x86_64-pc-windows-msvc`
  clean, plus `scripts/validate_song_playback_speed.sh`'s harness still compiling
  `api.rs` (it `#[path]`-includes `api.rs` + `registry.rs`; run the script if
  environment permits, otherwise note it for the maintainer). Exhaustive-match
  errors surface any missed `ScalarFormat` match site at compile time.
- **Integrates with:** nothing yet consumes the variant; existing scalar options
  (`song_speed`, `weight`, styling scales, `pacemaker_threshold`) are untouched.
- **Demo:** crate compiles with the new variant; no behavior change anywhere.

Step 2: Convert the 9 cosmetic categories to scalar rows (+ chrome, dead-code removal)

- **Objective:** The core conversion (design R1, R2, R4): cosmetics render as
  1-based numeric selectors with no per-value ribbon textures, preview
  chrome + live-art overlay intact; `EnumIndexed` machinery removed.
- **Implementation:**
  - `src/mods/webui_options/discovery.rs`: delete the `EnumIndexed` variant from
    `RenderMode` (keep `Scalar`, `EnumFixed`); flip the 9 cosmetic
    `CategoryDef.render` fields to `RenderMode::Scalar`; rewrite the `RenderMode`
    doc comment (drop the item-ribbon description, note scalar rows display
    1-based via `OffsetInteger`). `overlay_layers` / `bg_overlay` unchanged.
  - `src/mods/webui_options/mod.rs`:
    - `Scalar` arm becomes: `preview_gen::generate_chrome(option_id);` followed by
      `RegisterSpec::scalar(option_id, 0, count - 1, 1,
      ScalarFormat::OffsetInteger { display_offset: 1 })` — preserving the
      chrome-before-registration ordering the `EnumIndexed` arm documented.
    - Delete the `EnumIndexed` arm and `build_indexed_enum_values`; drop the
      `"enum-indexed"` mode log string; prune the now-unused `EnumValue` import
      if `build_fixed_enum_values` is its last remaining user (it isn't — keep
      whatever `EnumFixed` still needs).
    - Update stale doc comments referencing `seop_op_item_<NNN>`.
- **Tests:** compile-time exhaustiveness (removing the variant forces every
  `RenderMode` match site to be updated); `cargo check` clean. Grep gate:
  `rg "EnumIndexed|build_indexed_enum_values|seop_op_item" src/` returns nothing.
- **Integrates with:** Step 1's `OffsetInteger` variant (first consumer).
- **Demo:** full build (`./build.sh`) produces a DLL where the MODS tab cosmetic
  rows are numeric selectors reading "1".."N" with working chrome + live
  preview — deployable for a smoke test ahead of Step 4's formal validation.

Step 3: Retire the item-ribbon assets (script + committed PNGs)

- **Objective:** Stop generating and stop shipping the 150 `seop_op_item_*`
  textures (design R5, R6).
- **Implementation:**
  - `scripts/gen_option_labels.py`: remove `ITEM_RIBBON_COUNT` and the
    `[(f"item_{i:03}", …)]` comprehension appended to `RIBBONS`; keep the four
    bespoke ribbons; update the module docstring + the `RIBBONS` comment block
    (drop the shared-indexed-ribbon paragraphs).
  - Delete the 150 committed PNGs:
    `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/seop_op_item_*.png`.
- **Tests:** run `python3 scripts/gen_option_labels.py` on the host into the repo
  tree; assert (a) it exits clean, (b) it writes no `seop_op_item_*` file, (c) the
  label/bespoke-ribbon/preview outputs are byte-stable vs before the change
  (`git status` shows only deletions). Grep gate: `rg -l "seop_op_item"
  data_mods/ scripts/` returns nothing.
- **Integrates with:** Step 2 removed the only code that referenced these
  textures, so deletion cannot orphan a reference. (Cache note: the atlas
  rebuild's input-hash — `generate_cloned_atlases_cached`, `atlasbatch.md5` —
  covers the registered spec list, so with the ribbons no longer registered the
  next boot regenerates `texturelist.merged.xml` without the `seop_op_item_*`
  entries; the game then never *requests* those MD5s again. The 150 stale
  per-image files under `data_mods/_cache/` on the cabinet become dead bytes —
  harmless, optionally cleaned by deleting `_cache/`.)
- **Demo:** repo contains no `seop_op_item` artifacts; the generator reproduces
  the remaining asset set exactly.

Step 4: Docs + final build gates + cabinet validation

- **Objective:** Land the documentation (design R9) and run the full readiness +
  cabinet measurement that decides whether Phase 2 is needed.
- **Implementation:**
  - New `docs/scene_load_analysis.md`: the scene-18/21 load-path RE findings
    (FileManager pump once/frame, budget 4 new opens/pump at `+0x70`, synchronous
    package creation ⇒ no Fast-Bootup-style pacing hack), the dev-mode regression
    root cause, the per-open cost data, and the enum→scalar rationale with the
    measured 138/208 slow-path share.
  - `AGENTS.md`: update the WebUI Options row (cosmetic pickers are 1-based
    scalar selectors; `seop_op_item_*` retired); add the docs pointer.
  - `README.md`: update the WebUI Options feature entry's picker description.
- **Tests / gates:** `cargo check` → `cargo fmt` (whole crate) → `./build.sh`,
  all clean (repo readiness-gate convention).
- **Cabinet validation (maintainer):**
  1. Deploy; card in; open MODS tab: each cosmetic row shows "1".."N", cycles,
     applies, and previews (chrome + live art; backgrounds animate).
  2. Card-out/card-in: selections seed back correctly (SaveOnly round-trip via
     the server's native customize columns).
  3. One run with `layeredfs.verbose: true`: CAUTION-window
     `select_music_option_lang_eng_v3` cache-served (`LayeredFS: using`) open
     count should drop from ~208 to ~70; record scene-21 wall time vs the
     2026-08-11 baselines (~11 s verbose / ~7 s non-verbose). Scene 18 is
     expected UNCHANGED (it carries no injected textures).
  4. Decision point: if the measured CAUTION improvement satisfies, Phase 2
     stays shelved; otherwise reopen the design's Phase 2 sketch and plan it
     against the post-Phase-1 ~70-file population.
- **Integrates with:** everything prior; this is the ship gate.
- **Demo:** documented, fully-gated build on the cabinet with before/after
  numbers captured in this project's `progress.md`.
