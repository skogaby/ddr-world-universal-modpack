# Progress: add-japanese-and-korean

Updated: 2026-08-17
Status: Complete (uncommitted — maintainer commits)

## Checklist

- [x] Fonts committed: `scripts/fonts/NotoSansJP[wght].ttf` +
      `NotoSansKR[wght].ttf` + `OFL-NotoSansJP.txt`/`OFL-NotoSansKR.txt`
      (google/fonts ofl trees)
- [x] `load_fonts`: ja/ko via variable-font wght axis (600 labels/headers/
      ribbons, 500 preview body); en path untouched
- [x] CJK wrap: `is_cjk_char` (Han/kana/fullwidth/ext-A; Hangul EXCLUDED —
      Korean wraps on spaces), `break_units` with glue flags + kinsoku merge
      (`NO_BREAK_BEFORE`/`NO_BREAK_AFTER`), `wrap_paragraph` dispatches —
      Latin/Korean text takes the LEGACY path verbatim (en byte-identity)
- [x] Checks: CJK preview warnings → hard SystemExit; kinsoku assertion on
      every wrapped line; template explicit lines width+kinsoku validated at
      render (hard fail)
- [x] Translations: full en/ja/ko coverage in `option_strings.py`
      (programmatic re-emit + verification: coverage sets exact, en content
      unchanged, markers/art fields preserved). Source: design Appendix A.
      Template JA/KO as explicit pre-broken lines (width-validated)
- [x] Generation: all three sets, 91 files each, zero warnings/hard-fails
- [x] eng byte-stable: 71 prior files == step-2 snapshot; take-over harness
      re-passes (incl. cross-language marker invariance)
- [x] jpn/kor scaffold fully overwritten: 91/91 regenerated; only the 2
      verbatim copies byte-match eng (expected)
- [x] Condensed labels: ja 5 / ko 2 (center_arrows_1p, timing_stats,
      pacemaker_to_mserror, step_data_export, customize_lanecover_single ja;
      center_arrows_1p + step_data_export ko) — eyeballed on the sample
      sheet, legible
- [x] Maintainer sheet: `lang_sample_sheet.png` (this directory) — labels/
      header/ribbons/panels/templates ×3 languages from the real generated
      files

## Deviations

- NO COMMIT per session instruction.
- JA mid-word kana breaks (e.g. するた/び) are standard Japanese typesetting
  and intentional (kinsoku respected); noted for the native-speaker pass.

Status: Complete (uncommitted — maintainer commits)
