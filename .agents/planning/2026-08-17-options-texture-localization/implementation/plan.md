# Implementation Plan: Options Texture Localization (JPN/KOR)

Status: Approved 2026-08-17

Design: `.agents/planning/2026-08-17-options-texture-localization/design/detailed-design.md`

## Checklist

- [x] Step 1: Parameterize the DLL per language and prove the runtime pipeline end-to-end
- [x] Step 2: Move strings into option_strings.py and make the generator language-loop-ready
- [x] Step 3: Take over the hand-authored textures (art extraction + TEMPLATE generation)
- [x] Step 4: Add Japanese and Korean — fonts, CJK wrapping, translations, full 3-language generation
- [x] Step 5: Cabinet validation in all three languages and documentation

---

## Step 1: Parameterize the DLL per language and prove the runtime pipeline end-to-end

**Objective:** Eliminate the design's biggest unknown first — that the
donor-clone atlas build and preview chrome actually work against the stock
jpn/kor IFSes at runtime — before investing in the generation pipeline.

**Implementation:**

- `src/services/custom_options/asset_gen.rs`: replace the four `LANG_ENG_*`
  constants with the 3-entry `OptionLang` table from the design; loop
  `flush_label_atlas` over it with per-language WARN+skip failure isolation;
  make the fresh preview-atlas prefix language-distinct
  (`copt_prev_<code>_NNN`).
- `src/mods/webui_options/preview_gen.rs`: `PREVIEW_OUT_DIR` →
  `preview_dir(ifs_code)`; loop `generate_chrome` over the three language
  dirs (skip-if-missing per language); `marker_rect_for` reads eng-first with
  jpn/kor fallback.
- Scaffold content for testing: copy the current English tex dir verbatim to
  `data_mods/custom_options/select_music_option_lang_{jpn,kor}_v3_ifs/tex/`
  (placeholder — Step 4 overwrites these with real translations; the copies
  make the jpn/kor pipeline exercisable now).

**Tests:** `cargo check` → `cargo fmt` → `./build.sh`; deploy to cabinet.
Boot log shows three atlas builds (then cache hits on warm boot). In-game:
switch user language to Japanese and Korean — the MODS tab renders completely
(English text for now: labels, ribbons, previews, WebUI preview overlays
inside marker boxes, tab title, return icon), and English mode is unchanged.

**Integrates with:** nothing prior — this is the foundation the rest ships on.

**Demo:** A player who sets the game to Japanese or Korean sees the fully
functional MODS tab (still in English) instead of blank rows; boot log shows
`eng`/`jpn`/`kor` atlas passes.

## Step 2: Move strings into option_strings.py and make the generator language-loop-ready

**Objective:** Restructure `scripts/gen_option_labels.py` around the language
loop and the external string module without changing any output pixel.

**Implementation:**

- New `scripts/option_strings.py`: `LABELS` / `HEADER_LABELS` / `RIBBONS` /
  `PREVIEWS` / `TEMPLATES` per the design's data model, populated with the
  existing English content only (`en` keys; `ja`/`ko` added in Step 4).
- `gen_option_labels.py`: import the tables; add `LANGS`, `out_dir(code)`,
  a per-language `FontSet` (English-only entry for now), and a `--lang`
  CLI flag. All rendering functions take the FontSet instead of module-level
  fonts.
- Keep every layout constant untouched.

**Tests:** run the generator for `--lang en`; byte-compare the output against
the pre-refactor output for every currently-generated file (a temporary
before/after diff — expected: zero differing files). The hand-authored files
are untouched by this step.

**Integrates with:** Step 1's jpn/kor placeholder dirs remain as-is (the
generator doesn't write them yet).

**Demo:** `python3 scripts/gen_option_labels.py --lang en` reproduces today's
English texture set bit-for-bit from the new string module.

## Step 3: Take over the hand-authored textures (art extraction + TEMPLATE generation)

**Objective:** Every shipped texture becomes script-generated (the feature's
part 1) — English only at this point.

**Implementation:**

- Extract the baked art into `scripts/templates/` per the design's asset
  table: right-of-divider crops from `premium_free_{on,off}`,
  `center_arrows_1p_{on,off}`, `customize_movie_size_{on,off,fullscreen}`;
  verbatim masters for `seop_return` and `seop_tab_title_mods`. Record each
  crop's paste position in `option_strings.py`'s specs.
- Measure the nine templates' marker rects (and appeal_board's red box) into
  `TEMPLATES` specs.
- Generator: add the SPLIT-with-art preview family, the TEMPLATE family
  (divider + text + exact solid marker fills), and the verbatim-copy family.
  Add the `character_p2` "left→right" copy fix from the design.
- Add the ENG regression harness (small script or generator `--check` mode):
  text-line ink-band structure and art bounding boxes of regenerated panels
  match the shipped originals; marker rects byte-equal.

**Tests:** the regression harness passes for all take-over files; a visual
side-by-side sheet (shipped vs regenerated) is produced for maintainer
eyeballing. Full-set regeneration leaves the already-script-generated files
byte-identical to Step 2's output.

**Integrates with:** output feeds the same eng dir Step 1's DLL consumes —
deployable at any time for an in-game sanity check.

**Demo:** delete the eng tex dir, run the generator, get a complete visually
identical English texture set back (plus the maintainer-reviewable diff
sheet).

## Step 4: Add Japanese and Korean — fonts, CJK wrapping, translations, full 3-language generation

**Objective:** The feature's part 2 — complete jpn/kor texture sets.

**Implementation:**

- Commit `NotoSansJP[wght].ttf`, `NotoSansKR[wght].ttf` + their OFL license
  files under `scripts/fonts/`; wire the `ja`/`ko` FontSets (w600 @16 /
  w600 @11.2 headers / w500 @13.5 body via `set_variation_by_axes`).
- Implement CJK-aware wrapping with the design's kinsoku sets (R12);
  Latin/Korean tokenization unchanged (space-delimited).
- Populate `ja`/`ko` in `option_strings.py` from design Appendix A.
- Generate all three language sets (overwriting Step 1's placeholder copies).
- Automated checks in the generator run: marker-rect pixel equality across
  the three languages per template (R4/test 2); no JA line starts with a
  NO_BREAK_BEFORE character; line overflow is a hard failure for CJK.

**Tests:** generator checks above pass; `(condensed)` console notes reviewed
(CJK labels are typically shorter — flag any that condense); spot-render
sheet of a sample from each family in each language for the maintainer.

**Integrates with:** replaces the placeholder jpn/kor content; the Step 1 DLL
pipeline picks the new files up with no further DLL change (cold boot
regenerates the affected language atlas caches automatically).

**Demo:** three complete texture sets in the repo; a rendered sheet showing
the same rows in English, Japanese, and Korean.

## Step 5: Cabinet validation in all three languages and documentation

**Objective:** End-to-end verification on hardware and closing the docs loop.

**Implementation & Tests:**

- Deploy (`./scripts/deploy.sh`); cold boot (atlas cache regeneration for
  jpn/kor), then warm boot (cache hits).
- Per-language cabinet pass (per-user language selection): every row label,
  header, ribbon, preview panel; customize preview chrome + live art overlay
  placement; tab title + return icon; one full option-edit/save/card-out
  cycle to confirm nothing else regressed.
- Fault-injection sanity check: temporarily rename one language's mod folder,
  confirm the per-language WARN + clean skip, restore.
- Documentation: README (custom textures / options sections gain the
  three-language note and the regeneration command), AGENTS.md key-entry-point
  row for the localization pipeline, and a short
  `docs/options_texture_localization.md` if the README additions don't cover
  operator needs.

**Integrates with:** everything prior; no new code.

**Demo:** cabinet screenshots/video of the MODS tab in English, Japanese, and
Korean, all fully rendered; boot log excerpt showing the three-language atlas
build and the fail-open skip behavior.
