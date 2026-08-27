# Task: Add `mod_song_speed` Backend Persistence (bemani-buddy)

## Description
Add the `mod_song_speed` per-player profile field end to end in the sibling `bemani-buddy` backend: JSON wire-format model entries, codegen regeneration, database migration and column plumbing, save-handler ingestion, load-response emission, and tests — giving the Task 2 option card portability across cabinets. Valid values are multiples of 5 in `25..=175` (maintainer-approved design change 2026-08-07, superseding the design's 75/100/125 enum).

## Background
This task is performed in the SIBLING repository `bemani-buddy` (checked out next to this repo), not in the modpack DLL. All paths below are relative to the bemani-buddy repository root.

Maintainer-supplied constraint (2026-08-07): the Rust wire-format models are CODEGEN'D from JSON files living in the same repo. Changing the profile wire format requires editing the JSON models and re-running the codegen tool; generated `@generated` files are committed but never hand-edited (the model wins on disagreement).

The established pattern for modpack-injected option fields is the `opt_mod_*` pattern (like `mod_premium_free`/`mod_assist_tick`): the save handler is registered raw (input shape varies by `savekind`) and hand-walks the XML DOM, applying a field only when present so an absent field never clobbers the stored value; storage is a NULLABLE `opt_mod_*` profile column (stock, un-hooked clients never send the field, and echoing a default `<option>` child can crash an un-hooked game); the load response echoes the field as an optional `mod_*` child of the load `<option>` block, skipped when `None`. There is intentionally NO server-side default — the column stays NULL until a hooked client first saves; the 100 default lives client-side in the modpack's load transform (Task 2).

On the modpack side nothing further is required: a `PersistMode::Full` row auto-emits the `mod_song_speed` wire field on save and consumes the load echo.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md` (Backend Persistence)
- Plan Step 6: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- bemani-buddy `AGENTS.md` and `CONTRIBUTING.md` (codegen invocation, sqlx offline cache, gates, never-commit rule)

**Additional References (if relevant to this task):**
- bemani-buddy `models/ddr_world/playdata_3.json` (`PlayerdataLoadOption` ~line 108, `PlayerdataSaveOption` ~line 398 — the existing `mod_*` field blocks)
- bemani-buddy `crates/codegen/` and generated `crates/bemani-protocol/src/ddr_world/playdata_3.rs`
- bemani-buddy `crates/game-server/src/handlers/ddr_world/playdata.rs` (save-side `mod_*` application ~line 688-700, load-side echo ~line 330-342, new-player response ~line 416, test module ~line 1426)
- bemani-buddy `crates/db/src/models/ddr_world/profile.rs` and `crates/db/src/mysql/ddr_world/profile.rs`
- bemani-buddy `migrations/008_ddr_world_mod_options.sql` and `migrations/011_ddr_world_more_mod_options.sql` (the nullable `opt_mod_*` convention)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Add `"mod_song_speed": "s32?"` to BOTH `PlayerdataLoadOption` and `PlayerdataSaveOption` in `models/ddr_world/playdata_3.json`, placed consistently with the existing `mod_*` blocks.
2. Re-run the codegen tool (`cargo run -p codegen -- models/ddr_world/ crates/bemani-protocol/src/ddr_world/`) and include the regenerated `playdata_3.rs`; never hand-edit `@generated` output.
3. Add the next sequential migration (`migrations/012_*.sql`): `ALTER TABLE ddr_world_profiles ADD COLUMN opt_mod_song_speed INT NULL DEFAULT NULL;` — nullable, no server-side default, per the `opt_mod_*` convention.
4. Thread the column through the profile entity (`crates/db/src/models/ddr_world/profile.rs`, modpack-options block) and the MySQL layer (`crates/db/src/mysql/ddr_world/profile.rs`: row mapping plus UPDATE column list and params).
5. Save-side ingestion in `handle_save_profile`: apply `mod_song_speed` only when present, validating the value at the handler boundary (accept exactly the multiples of 5 in `25..=175`; reject/skip anything else without clobbering the stored value) — matching the repo's boundary-validation guidance while preserving the only-when-present policy across all savekinds that carry the profile `<option>` block.
6. Load-side emission: echo `mod_song_speed: profile.opt_mod_song_speed` in the load `<option>` block (skipped when `None` via the generated `skip_serializing_if`), and add the field as `None` in `build_new_player_response`.
7. Tests in the existing handler test module following the workout-field precedent: present/absent parses; boundary and representative valid values (25, 175, 100, one interior multiple of 5); invalid values (out of range, non-multiple-of-5, malformed) rejected without clobbering; only-when-present non-clobbering; P1/P2 isolation; save->load round-trip; new-player response emits nothing.
8. Regenerate the sqlx offline cache after the migration (`sqlx migrate run --source migrations/` + `cargo sqlx prepare --workspace`) and include the updated `.sqlx/` directory.
9. Run the bemani-buddy gates at its workspace root: `cargo build`, `cargo test`, `cargo clippy --workspace --all-targets` (clean), `cargo fmt` (clean).
10. Do NOT commit or push in either repository — the maintainer owns git history in both. Do NOT deploy; live round-trip evidence is Task 4's cabinet pass.

## Dependencies
- Task 2 defines the client-side wire behavior (`PersistMode::Full` auto-emission/consumption of `mod_song_speed`); the field name is fixed by the design, so this task may proceed in parallel once Task 2's row id is locked as `song_speed`.
- A local MySQL 8+ instance and `sqlx-cli` for the offline-cache regeneration (per bemani-buddy `CONTRIBUTING.md`).

## Implementation Approach
1. Write the failing handler tests first (parse/round-trip/non-clobber/new-player).
2. Edit the JSON model; run codegen; verify only the expected generated diff.
3. Add the migration and thread the column through entity/MySQL layers; regenerate `.sqlx/`.
4. Implement save ingestion with boundary validation and load emission.
5. Run all four bemani-buddy gates.

## Acceptance Criteria

1. **Model-Driven Wire Format**
   - Given the JSON model edit and a codegen run
   - When the generated protocol code is rebuilt
   - Then `mod_song_speed` exists as `Option<i32>` on both load and save option shapes, the generated file carries no hand edits, and un-hooked clients see no new element (skipped when `None`)

2. **Only-When-Present Persistence**
   - Given saves with the field absent, present with boundary/representative valid values, and present with invalid values (out of range, non-multiple-of-5)
   - When `handle_save_profile` processes them
   - Then absent and invalid saves leave the stored value untouched, valid saves persist exactly, and P1/P2 rows are isolated

3. **Round-Trip and New Player**
   - Given a stored `opt_mod_song_speed` and a fresh profile
   - When the load response is built
   - Then the stored value echoes as `mod_song_speed`, a NULL column emits nothing, and the new-player response emits nothing

4. **Gates and Cache**
   - Given the migration and query changes
   - When `cargo build`, `cargo test`, `cargo clippy --workspace --all-targets`, and `cargo fmt` run at the bemani-buddy workspace root
   - Then all pass with the regenerated `.sqlx/` offline cache included and no commits made

## Metadata
- **Complexity**: Low
- **Labels**: rust, backend, bemani-buddy, codegen, wire-format, migration, sqlx, persistence, step-6
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-07
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Steps 5+6 (merged delivery, maintainer-approved 2026-08-07) — Step 6: Add player-facing policy, persistence, and backend support
