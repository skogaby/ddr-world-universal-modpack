# Progress — theme-panel-score-sets

Task: `.agents/tasks/2026-09-25-ddr-selection-a-a3-themes/step05/task-02-theme-panel-score-sets.code-task.md`

- [x] `score_set_logic.rs` + its tests (glyph map, name slots, World's name rule, digit places /
      leading zeros incl. 0 and 1,000,000, ranks 0..15 / none, FC marks 7..=10, difficulty
      textures, A3's area table + region rule, language suffix / area package, target rules,
      whole-side fills incl. no-record, missing-field and target cases), registered in
      `scripts/validate_ddr_selection.sh`. Note: the tests were written together with the
      implementation — no separate red run was recorded.
- [x] `signatures.rs` (additions only): `DdrSelScoreSetSites`, `derive_ddr_sel_score_set`
      (the `ghost_id_lookup` case-0 call, shape + callee-prologue gated), accessor
- [x] `services/cabinet.rs`: `licence_key_version()`, `game_language()` (shared getter cache)
- [x] `score_set.rs`: sites at init (one WARN if partial), `area_package(skin)`, `FillCtx`,
      `side_inputs` (probed reads, per-capability WARN once), `describe`
- [x] `panel.rs`: `Tickets` gains `texture` / `area` (+ `none()`, `ready()`); the theme arm
      requests `common_texture_v0` and the probed area package; `fill_theme` →
      `fill_score_sets` applies the writes; module doc updated
- [x] Gate: `cargo check` / `cargo fmt` / `./build.sh` clean; harness 181 passed (was 164);
      signature sweep ALL GREEN (best record + score db on all five builds); `shape_diff.py`
      only the known `ghost_id_lookup` +0x27 PlayerWork disp8 on 20250805 / 20260224
- [x] Cabinet demo (maintainer) — passed 2026-09-26

Status: Complete (uncommitted — maintainer commits manually)
