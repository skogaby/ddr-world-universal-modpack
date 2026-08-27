# Progress: Step 5 — bemani-buddy backend persistence

Covered by the feature plan Step 5 (approved 2026-08-12); autonomous run.
All work in the SIBLING repo (bemani-buddy checkout), stacked on the
in-flight uncommitted 012/013 changes (untouched).

## Done

- [x] `models/ddr_world/playdata_3.json`: `"mod_preserve_pitch": "s32?"` in
  both the load and save `<option>` shapes (after `mod_song_speed`).
- [x] Codegen: `cargo run -p codegen -- models/ddr_world/playdata_3.json
  crates/bemani-protocol/src/ddr_world/` — regenerated playdata_3.rs
  (+4 lines, both structs; never hand-edited).
- [x] `migrations/014_ddr_world_preserve_pitch.sql` — nullable no-default
  `opt_mod_preserve_pitch INT`, house doc-comment (verbatim storage,
  un-hooked-client echo safety).
- [x] DB model (`DdrWorldProfile.opt_mod_preserve_pitch: Option<i32>`) +
  MySQL DAO (row_to_profile! macro, UPDATE column list, bind params).
- [x] Handler: load map, new-player None, save only-when-present applier;
  `load_option_all_none()` test helper gains the field.
- [x] Five handler tests (present/absent/malformed parse; None-skipped /
  Some-echoed on load) — all green.
- [x] `sqlx migrate run` (applied 14) + `cargo sqlx prepare --workspace`
  (DATABASE_URL from config.toml; local MySQL). `.sqlx/` refreshed.
- [x] `cargo build` OK; `cargo test`: 246 passed / 0 failed.

## Notes

- Migrations 012/013 were already applied to the local DB (only 014 ran).
- Working tree churn (`.sqlx/` adds/removes/modifies) is expected sqlx
  prepare output; left uncommitted with everything else per the
  maintainer's fold-into-one-commit instruction.
- No `cargo fmt` run needed in bemani-buddy (edits matched existing
  formatting; build+tests clean).

## Deviations

(none)
