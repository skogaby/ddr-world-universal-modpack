# Summary: Options Texture Localization (JPN/KOR)

**FEATURE COMPLETE 2026-08-17.** Planned, implemented, and cabinet-validated
in all three languages the same day ("everything worked perfectly" —
maintainer, final pass). All 5 plan steps ticked. The maintainer is handling
the commit. Remaining follow-up (non-blocking): native-speaker review of the
JA/KO strings — edits are `scripts/option_strings.py` + regenerate.

## Final cabinet checklist (maintainer) — COMPLETED 2026-08-17

1. Deploy (`./scripts/deploy.sh`), cold boot. Expect: three
   `flushed lang_<code> atlas` lines; one-time cache regeneration for all
   three languages (new content + the `copt_prev_<code>` prefixes); warm
   boot shows cache skips.
2. English session: full MODS tab regression (labels, headers, ribbons,
   previews incl. the retextured take-over panels, WebUI preview overlays,
   tab title, return icon).
3. Japanese session: every row renders in Japanese; preview art lands inside
   the chrome boxes; take-over panels show JA text beside the baked art.
4. Korean session: same in Korean (first full KOR check — Step 1's demo
   covered ENG+JPN).
5. Fault-injection spot check (optional): rename
   `data_mods/custom_options/select_music_option_lang_kor_v3_ifs` away →
   one WARN, ENG/JPN unaffected; restore.
6. Commit (suggested single commit:
   `feat(custom-options): localize options-menu textures to Japanese and Korean`),
   then tick Step 5 in `implementation/plan.md`.

## What was built

- **DLL** (`asset_gen.rs`, `preview_gen.rs`): per-language donor-clone atlas
  builds + preview chrome for eng/jpn/kor, fail-open per language, driven by
  the shared `OPTION_LANGS` table. Cabinet-confirmed (ENG+JPN) in Step 1.
- **Generator** (`scripts/gen_option_labels.py` + data-only
  `scripts/option_strings.py`): all 91 textures per language fully
  script-generated (hand-authored panels taken over; art extracted to
  `scripts/templates/`), language loop + `--lang`, per-language FontSets
  (Noto Sans JP/KR variable fonts, OFL, committed), kinsoku-aware CJK
  wrapping (Hangul wraps on spaces), hard-fail checks for CJK overflow and
  kinsoku violations, `character_p2` "left→right" copy fix.
- **Harness** (`scripts/check_option_takeover.py`): layout regression vs a
  reference set — line structure, baked art, marker rects (incl.
  cross-language marker invariance). All 20 take-over files pass.
- **Content**: full JA/KO translations (design Appendix A, approved;
  native-speaker pass planned — string edits are regenerate-and-redeploy).
- **Docs**: README "Injected options-menu textures & languages" section;
  AGENTS.md key-entry-point row.

## Validation record

- 383/383 Rust tests (via `cargo xwin test` + CrossOver wine); cargo
  check/fmt/build.sh clean.
- English regeneration byte-identical through the Step 2 refactor (71/71)
  and byte-stable through Steps 3–4; full 3×91-file output rerun-stable.
- Visual sheets for maintainer review:
  `.agents/scratchpad/2026-08-17-options-texture-localization/take-over-hand-authored-textures/takeover_diff_sheet.png`
  (shipped vs regenerated) and
  `.../add-japanese-and-korean/lang_sample_sheet.png` (3-language samples).

## Artifacts

- `idea-honing.md` — 12-decision register, Readiness Confirmed
- `research/` — orientation, fonts (+ comparison sheets), stock-lang-ifs
- `design/detailed-design.md` — Approved 2026-08-17 (Appendix A =
  translation tables)
- `implementation/plan.md` — Approved 2026-08-17; Steps 1–4 ticked
- `progress.md` — live status
- `.agents/tasks/2026-08-17-options-texture-localization/step0{1..4}/` —
  task files; working records under
  `.agents/scratchpad/2026-08-17-options-texture-localization/`

## Watch items

- Translations awaiting native-speaker review (JA mid-word kana wraps are
  standard but worth their eyes).
- Host-native `cargo test` broken by a pre-existing retour/aarch64 issue
  (unrelated); use `cargo xwin test` + wine.
- The 2026-08-11 loading-screen analysis warned against stray PNGs in these
  `_ifs` folders — the generator writes exactly the 91 intended files per
  language; keep it that way.
