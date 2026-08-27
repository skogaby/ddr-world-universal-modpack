# Progress: Options Texture Localization (JPN/KOR)

Updated: 2026-08-17
Status: FEATURE COMPLETE 2026-08-17 — all 5 steps done, cabinet-validated; maintainer handling the commit
NEXT ACTION: none — feature closed. (Deferred follow-up: native-speaker review of the JA/KO strings; edits are option_strings.py + regenerate.)

Resume protocol: read `implementation/plan.md` (approved) + `design/detailed-design.md` (approved) + this file. Task working records live under `.agents/scratchpad/2026-08-17-options-texture-localization/<task_name>/progress.md`.

## Done

- **Step 1 — complete, cabinet-confirmed 2026-08-17** (checklist ticked).
  Maintainer verified the injected mod option rows render under both English
  and Japanese in-game (Korean rides the identical pipeline; explicit KOR
  spot-check can fold into Step 5's full pass). Details:
- Step 1 / task 01 (`parameterize-asset-gen-per-language`): `OptionLang` +
  `OPTION_LANGS` table in `asset_gen.rs`, per-language flush loop with
  WARN+skip, language-distinct atlas prefixes (`copt_mods_lang_<code>`,
  `copt_prev_<code>`), eng-authoritative `AVAILABLE_PREVIEWS` pass, scaffold
  copies of the eng textures into the jpn/kor tex dirs (91 files each,
  byte-identical). Table unit test + full suite green (382/0) via
  `cargo xwin test` + CrossOver wine; check/fmt/build.sh clean.
  **Uncommitted** — maintainer commits (session rule).
- Step 1 / task 02 (`parameterize-preview-gen-per-language`): `preview_dir`/
  `template_path_in` derived from `OPTION_LANGS`; `generate_chrome` loops
  languages (per-language skip-if-exists + missing-template warn+skip);
  `marker_rect_for` eng→jpn→kor fallback. Consumer signatures unchanged
  (`preview_overlay`/`bg_preview_overlay`/`mod.rs` untouched). New path test
  + full suite green (383/0); check/fmt/build.sh clean. **Uncommitted.**

## In flight

- Nothing — feature closed.

## Done (Step 5, code/docs half)

- README: new "Injected options-menu textures & languages" section (three
  languages, regeneration + harness commands, string-edit workflow,
  fail-open note).
- AGENTS.md: "Options-menu texture localization" key-entry-point row.
- Final validation: full regeneration rerun-stable (273 files byte-identical
  across reruns), harness OK, cargo check/fmt/build.sh clean.

## Done (Step 4)

- **Step 4 — complete** (ticked): Noto Sans JP/KR variable fonts + OFL
  licenses committed; kinsoku CJK wrap (Hangul space-wrapped; en legacy path
  byte-identical); full ja/ko translations in option_strings.py (design
  Appendix A, coverage-verified); all three 91-file sets generated, zero
  warnings; eng byte-stable; scaffold fully overwritten; marker invariance
  re-verified. Sample sheet: add-japanese-and-korean/lang_sample_sheet.png

## Done (Step 3)

- **Step 3 — complete** (ticked): all 91 eng textures now script-generated.
  7 art crops + 2 verbatim masters in `scripts/templates/`; 9 take-over
  PreviewSpecs + 9 TEMPLATES (explicit-lines model, character_p2 left→right
  fix) in `option_strings.py`; `render_template` + art compositing +
  verbatim family in the generator; committed harness
  `scripts/check_option_takeover.py` → OK all 20 take-over files; 71
  prior files byte-identical; visual sheet `takeover_diff_sheet.png` in the
  task scratchpad.

## Done (continued)

- **Step 2 — complete** (ticked): `scripts/option_strings.py` (data-only,
  strings transplanted programmatically), generator language loop +
  `--lang` + FontSet; byte-identical gate passed (71/71 vs pre-refactor
  snapshot AND vs shipped files). Record:
  `.agents/scratchpad/2026-08-17-options-texture-localization/extract-strings-and-language-loop/progress.md`

## Deploy & test log

- 2026-08-17 (Step 1 demo): deployed; maintainer confirmed the English-text
  mod options render in-game under both Japanese and English user language.
  → Step 1 ticked in the plan.
- 2026-08-17 (final pass): maintainer ran the Step 5 checklist — "everything
  worked perfectly" across the three languages. → Step 5 ticked; feature
  closed. Commit handled by the maintainer.

## Deviations & open questions

- Agent performs NO git commits (maintainer instruction) — task records close
  with `Status: Complete (uncommitted — maintainer commits)`.
- Host-native `cargo test` is broken by a pre-existing retour/aarch64 compile
  failure; windows-target tests run under CrossOver wine
  (`CX_BOTTLE=bemani`). See task 01's progress.md → Environment notes.

## Key facts for a cold resume

- Design/plan approved 2026-08-17; translations approved pending a later
  native-speaker pass (regenerate-and-redeploy only).
- `copt_prev` → `copt_prev_eng` rename busts the eng preview cache once on
  next boot (expected).
- Step 4 must overwrite the jpn/kor scaffold (English placeholder) textures.
- Stock jpn/kor ARCs + donors verified present (research/stock-lang-ifs.md);
  game install at `DDR_WORLD_INSTALL` env var; cabinet supports per-user
  language switching.
