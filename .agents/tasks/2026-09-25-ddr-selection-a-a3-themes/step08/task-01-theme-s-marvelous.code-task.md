# Task: S-Marvelous on the themes

## Description
Theme-styled S-Marvelous on DDR SELECTION's three themes (design R13): the violet S-MARVELOUS word
(both Judgement Color settings), the violet S-MFC splash, and the all-S-Marvelous combo sheet. DDR A
(skin 6) gets its own art set; A3 White (7) and A3 Gold (8) share one (set 7). Today S-Marvelous
stands down on skins 6..=8 because `LEGACY_SKINS` is `[1..=5]`.

## Background
Design §4.13 and D11. The engine side is the eras' S-Marvelous machinery (`s_marvelous/legacy.rs`,
`afp_patches.rs`, `splash.rs`, `combo.rs`, `assets.rs`, pure names in `targets.rs`), keyed by the
armed skin through DDR SELECTION's `legacy_package` / `armed_skin` seam.

Verified against the stock install:
- Texture names:
  - word `dance_judge0000_marvelous`;
  - splash `dafu_eff_mar`, `dafu_{light,ring,rsring01,side_light}_marvelous`;
  - combo `dance_combo0000_marvelous_{0..9,combo}`.

  These are the same in `_v0` / `_v1` / `_v2`.
- Every theme `dance_fullcombo0000_vN` template (all four) has **5** Marvelous-art shapes, as the eras
  do, so `fc_expected_shapes` needs no change.
- Every `_v1` donor texture (word, 5 splash regions, 11 combo images) is pixel-identical to `_v2`'s.
  DDR A's `_v0` differs (its word, `dafu_eff_mar` and combo sheet).
- Word shapes:
  - DDR A: cream letters, a thick black outline, a white rim and a yellow glow outside it (the
    2013-A shape) ⇒ `violet_glow`;
  - A3: cream letters, a thick dark-olive outline with a thin light inner line, and a faint glow
    (the X-era shape) ⇒ `violet_outline`.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-09-25-ddr-selection-a-a3-themes/design/detailed-design.md (§4.13,
  §6.1)

**Additional References (if relevant to this task):**
- data_mods/ddr_selection/s_marvelous/README.md (the era sets, recipes, generator)
- .agents/planning/2026-09-22-ddr-selection/research/smarv-legacy.md

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `s_marvelous/targets.rs` (pure):
   - `LEGACY_SKINS` 1..=8;
   - `skin_bit` → `u16` (0..=15);
   - `tex_number` (0 for themes, mirroring `ddr_selection::policy::tex_number`);
   - `art_set` (1..=6 itself, 7 | 8 → 7) used by every art path;
   - `legacy_combo_has_grade_sheets` 4 | 5 | 6..=8;
   - combo texture / donor names via `tex_number`;
   - `fc_expected_shapes` 5 for the themes (counted).
2. `ddr_selection::policy::theme_package(base, skin)`: the theme row's package name without the NUL,
   whatever the adapter availability; `None` off a theme.
3. `s_marvelous/assets.rs`: theme targets are named through `policy::theme_package`, with a single
   candidate (`<name>.arc`, reached by World's bare rung); eras are unchanged. Log labels use the
   registered name.
4. Per-target masks `AtomicU8` → `AtomicU16` in `legacy.rs`, `afp_patches.rs`, `splash.rs`,
   `combo.rs` and `flash.rs`.
5. `combo::add_legacy` checks the seam: the staged sheet name must equal DDR SELECTION's
   `combo_math::sheet_prefix(skin, 0, true)` + `_0`, else it WARNs and stages nothing.
6. `scripts/gen_ddr_selection_smarv_art.py`:
   - `--skins` takes art sets 1..=7 (default all);
   - set 6 donors `…0000_v0`, set 7 `…0000_v2`;
   - a `_v1` ≡ `_v2` pixel guard (set 7 fails and writes nothing on a mismatch);
   - `OUTLINE` rows 6 (glow) and 7 (outline);
   - grade sheets on 4..=7.

   Generate `data_mods/ddr_selection/s_marvelous/{6,7}/`.
7. `scripts/validate_s_marvelous.sh`: the targets tests; Leg H covers skins 6..=8 (word recipe,
   splash recipe, art sizes against the theme donors).
8. Docs: the `data_mods/ddr_selection/s_marvelous/README.md` table / recipes, and the `//!` docs of
   `targets.rs` / `legacy.rs` / `s_marvelous/mod.rs`.
9. Readiness gate: `cargo check`, `cargo fmt`, `./build.sh`, `scripts/validate_ddr_selection.sh`,
   `scripts/validate_s_marvelous.sh`, and the generator's `--check-world`.

## Dependencies
- Plan Steps 1–2 (the theme judge / full-combo / combo rows and the combo adapter's theme sheets).

## Implementation Approach
1. Failing tests: the targets rules for 6..=8 and `policy::theme_package`.
2. Implement pure changes, then the engine (targets, masks, seam check, labels).
3. Generator, then generate the sets. Inspect a contact sheet.
4. Harness Leg H, docs, gate; record the cabinet procedure.

## Acceptance Criteria

1. **Targets**
   - Given skins 6 / 7 / 8
   - When the targets are asked
   - Then `target_skin(true, s)` is `Some(s)`:
     - art sets 6 / 7 / 7;
     - combo texture `dance_combo0000_smarvelous_<key>`, donor `dance_combo0000_marvelous_0`;
     - grade sheets `true`;
     - `skin_bit(8)` = `0x100`.

2. **Theme packages**
   - Given `dance_judge` / `dance_fullcombo` / `dance_combo` on skins 6 / 7 / 8
   - When the staging names its target
   - Then it is `…0000_v0` / `…0000_v2` / `…0000_v1`, and `None` on an era

3. **Theme S-Marvelous**
   - Given S-Marvelous on and a theme song
   - When S-Marvelous judgements, an all-S-Marvelous full combo and an all-S-Marvelous combo happen
   - Then the violet word, S-MFC splash and combo sheet show in the theme's style

4. **Stand-down**
   - Given a theme art set missing
   - When a theme song plays
   - Then that theme shows a plain Marvelous (one INFO at staging)

## Metadata
- **Complexity**: Medium
- **Labels**: ddr-selection, s-marvelous, assets, generator
- **Required Skills**: Rust, Python (numpy / Pillow), in-process hooking conventions of this repo
- **Generated By**: code-task-generator 2026-09-26 (breakdown pre-approved: the maintainer asked for autonomous execution between cabinet tests)
- **Source Plan**: .agents/planning/2026-09-25-ddr-selection-a-a3-themes/implementation/plan.md
- **Plan Step**: Step 8: S-Marvelous on the themes
