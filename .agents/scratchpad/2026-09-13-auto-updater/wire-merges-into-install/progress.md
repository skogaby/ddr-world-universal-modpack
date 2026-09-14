# Progress — Step 3 (consolidated): merges + wiring
Status: Complete (uncommitted — maintainer commits manually)

## Done
- [x] `merge/json.rs` — `merge_config` (recursive additive, user wins, arrays atomic, order preserved, delegation for `custom_options.option_menu_settings`), `to_pretty_json`; 9 tests incl. the committed `mod-config.json` self-merge no-op.
- [x] `merge/option_menu.rs` — §4.7 algorithm verbatim; 11 tests: the seven worked examples, section-end fallback, case-insensitivity, duplicates, id-less rows, verbatim copy/untouched flags, empty user list, committed list self-merge, committed list with ANY single row removed (41 cases) and any two adjacent rows removed reproduced exactly.
- [x] `merge/csv_grammar.rs` — `#[path]` mount of the DLL's `src/mods/per_song_judgement_offsets/csv.rs` (grammar parity BY CONSTRUCTION; its own tests run in the updater's suite) + `merge/csv.rs` `merge_csv` (cell-level fill via the DLL's `upsert`); 5 tests incl. committed CSV self-merge no-op and "old half of the file + release = full file".
- [x] `main.rs::merge_inputs` → `merge_one` (absent → release copy; parseable → merge, write only if changed; user unparseable → copy to `.ddr_world_hook_updater/unparseable/<name>` + WARN + skip; release unparseable → WARN + skip; user CSV with bad lines → WARN with counts, still merged) + §4.13 report lines.
- [x] e2e M1–M4 (+ A1/A7 updated for real merges): 95 unit + 12 apply e2e + 9 cli e2e green; host + Win7 builds warning-free; exe 602 112 B, no `ProcessPrng`.
- [x] Real-file check (COPY of the real install's `mod-config.json`/`judgement_offsets.csv` in a temp game folder, real 20260913 release zip, CrossOver): `Merging mod-config.json ... 1 key added (mods.series-expansion)`, CSV `no changes`; diff of the merged file vs the original = exactly that one appended key + trailing newline (DLL-alphabetical key order preserved). NOT applied to the real install — see finding below.

## Findings
- **New sections follow their release predecessor, not the end.** With the user's
  sections reordered (training first), a brand-new header from the release is
  inserted right after the user's TRAINING section (its predecessor in the
  release), not at the bottom — exactly the §4.7 rule ("inserted by the same
  rule"); the M1 expectation was corrected to match. Worth a sentence in the
  README's description of the merge.
- **Real install: the release would add `mods.series-expansion: false`.** The
  install's `mods` map (38 ids) lacks `series-expansion`; absent ⇒ the DLL's
  default is ON, so the merge would turn that mod OFF there (the committed config
  says `false`). This is D4 working as specified (the committed file is the
  release default), but it flips a mod on the maintainer's cabinet, so the real
  run was NOT performed in this step — maintainer's call.

## Deviations
- Unparseable-file copy lives in `.ddr_world_hook_updater/unparseable/<name>`
  rather than `backup/` (the task text): `apply::execute` resets `backup/` at the
  start of every apply and would have deleted the copy.
- CSV grammar is SHARED with the DLL via `#[path]` instead of re-implemented
  ("mirrored" in the design) — strictly stronger parity, zero duplicated code;
  the DLL file must stay dependency-free (it is documented as such).
