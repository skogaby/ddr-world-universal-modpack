# Progress: parameterize-preview-gen-per-language

Updated: 2026-08-17
Status: Complete (uncommitted — maintainer commits per session instruction)

## Checklist

- [x] TDD: path-derivation test written first (red: E0425 `preview_dir` /
      `template_path_in` absent — logs/check-red.log)
- [x] `PREVIEW_OUT_DIR` → `preview_dir(lang)` + `template_path_in(lang, id)`
      derived from `custom_options::asset_gen::OPTION_LANGS` (single source
      of truth; `template_path_for` removed — no external callers, verified)
- [x] `generate_chrome(option_id)` loops languages: per-language
      skip-if-exists, per-language missing-template warn+skip, returns
      any_ok (caller contract preserved; signature unchanged)
- [x] `marker_rect_for(option_id, color)` resolves eng→jpn→kor, first clean
      hit wins, info log when the hit isn't eng, warn+None when no language
      yields a rect (signature unchanged)
- [x] Module doc comment updated (per-language chrome, language-invariant
      marker geometry)
- [x] Tests green: new `preview_paths_derive_from_option_langs` + full suite
      383 passed / 0 failed (CrossOver wine, logs/test-run-full.log)
- [x] Gates: cargo check clean (no new warnings) → cargo fmt → ./build.sh
      clean (logs/)
- [x] Consistency review: `find_marker` / `apply_gamma` / `find_asset_arc*` /
      `MarkerRect` untouched; `preview_overlay.rs` / `bg_preview_overlay.rs` /
      `mod.rs` unchanged (source-compatible surfaces, acceptance criterion 5)
- [x] Report — NO COMMIT (maintainer handles git)

## TDD record

1. RED: `tests::preview_paths_derive_from_option_langs` referencing the
   missing helpers; check fails E0425.
2. GREEN: helpers + per-language generate_chrome/marker_rect_for; full suite
   383/0 under wine.

## Deviations

- Sop Step 6 (Commit) intentionally not executed (maintainer instruction).
- "Log once per option on fallback": implemented as a log at each fallback
  resolution rather than a deduped once-ever set — marker lookups run at
  overlay build time (bounded), recorded in context.md as a judgment call.

## Step 1 status after this task

Both step01 tasks complete:
- task-01: Status: Complete (uncommitted) — see sibling working dir
- task-02: this file

Plan checklist item "Step 1" NOT ticked yet: the step's demo (cabinet deploy —
MODS tab renders in all three game languages, three per-language atlas passes
in the boot log) is maintainer-performed and still outstanding. Tick after a
successful cabinet pass.

Changed/added files awaiting maintainer commit (suggested single commit):
- src/services/custom_options/asset_gen.rs
- src/mods/webui_options/preview_gen.rs
- data_mods/custom_options/select_music_option_lang_jpn_v3_ifs/ (scaffold)
- data_mods/custom_options/select_music_option_lang_kor_v3_ifs/ (scaffold)
Suggested message:
`feat(custom-options): build Mods-tab textures for all three game languages`

Status: Complete (uncommitted — maintainer commits)
