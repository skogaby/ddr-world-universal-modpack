# Progress: parameterize-asset-gen-per-language

Updated: 2026-08-17
Status: Complete (uncommitted — maintainer commits per session instruction)

## Checklist

- [x] TDD: write language-table unit test (failed as expected: `OPTION_LANGS` absent, E0425)
- [x] Add `OptionLang` + `OPTION_LANGS`, remove `LANG_ENG_*` / `PREVIEW_ATLAS_PREFIX`
- [x] `rebuild_lang_eng_atlas` → `rebuild_lang_atlas(lang, xml)`; hoist AVAILABLE_PREVIEWS pass into `flush_label_atlas` (single eng-authoritative pass + language-invariance comment)
- [x] `flush_label_atlas` language loop with per-language WARN+skip + per-language log lines
- [x] Update module doc comment + all stale `lang_eng` doc references
- [x] Tests: `option_langs_table_invariants` passes; FULL suite 382 passed / 0 failed
      (run via CrossOver wine on the windows-target test exe — see note below)
- [x] Scaffold copy eng → jpn/kor tex dirs (91 files each, `diff -r` byte-identical, .DS_Store excluded)
- [x] Gates: `cargo check` clean → `cargo fmt` → `./build.sh` clean (logs/)
- [x] Validate + consistency review (diff reviewed; docs updated coherently)
- [x] Report — NO COMMIT (maintainer handles git)

## TDD record

1. RED: appended `tests::option_langs_table_invariants` referencing the
   not-yet-existing `OPTION_LANGS`; `cargo test --target x86_64-pc-windows-msvc
   --no-run` failed with E0425 (logs/test-no-run.log).
2. GREEN: added `OptionLang` struct + 3-entry longhand table; parameterized
   `rebuild_lang_atlas`; rebuilt flush loop. Test passes (logs/test-run.log).
3. Full suite: 382 passed, 0 failed (logs/test-run-full.log).

## Environment notes

- Host-native `cargo test` (aarch64-apple-darwin) is broken by a
  PRE-EXISTING retour 0.4.0-alpha.4 compile failure (`arch::meta` etc.) —
  unrelated to this change; also visible in rust-analyzer diagnostics before
  any edit. Windows-target tests were built with `cargo xwin test --no-run`
  and executed under CrossOver wine
  (`CX_BOTTLE=bemani wine target/x86_64-pc-windows-msvc/debug/build/.../ddr_world_hook-*.exe`).
  Treated as the project's host-test path for this crate on this machine;
  repairing host-native `cargo test` is a separate decision for the
  maintainer.
- `cargo test --target ... --no-run` WITHOUT xwin fails (`link.exe` not
  found) — use `cargo xwin test`.

## Deviations

- Sop Step 6 (Commit) intentionally not executed: maintainer instructed
  "don't run any git commits yourself". Files changed/added, left uncommitted:
  - `src/services/custom_options/asset_gen.rs` (modified)
  - `data_mods/custom_options/select_music_option_lang_jpn_v3_ifs/tex/` (new, 91 files)
  - `data_mods/custom_options/select_music_option_lang_kor_v3_ifs/tex/` (new, 91 files)
  Suggested commit message:
  `feat(custom-options): build Mods-tab atlases for all three game languages`
- None against the approved design.

## Known one-time effect

- `copt_prev` → `copt_prev_eng` prefix rename busts the eng preview atlas
  cache once on the next boot (expected; feature summary watch item).

## Cabinet expectations (Step 1 demo — pending deploy)

- Boot log: three `flushed lang_<code> atlas ...` lines (cold), cache-skip
  lines on warm boot.
- MODS tab renders (English text) with game language set to Japanese/Korean.
- Task-02 (preview_gen) should land before the deploy for the full step demo
  (jpn/kor chrome generation).

Status: Complete (uncommitted — maintainer commits)
