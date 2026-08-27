# Task 03 Progress — `mod_song_speed` backend persistence

Updated: 2026-08-08
Status: Complete (uncommitted — the maintainer owns all commits in both repos)

## Checklist

- [x] TDD red: tests written, failing (compile-red on missing wire field: E0560/E0609)
- [x] JSON model + codegen regen (diff = exactly the two new optional fields)
- [x] Migration 012 + db model + mysql plumbing
- [x] Save ingestion (only-when-present, NO validation) + load echo + new-player None
- [x] `.sqlx/` regenerated (migration applied to local MySQL; new query JSON staged)
- [x] Gates: cargo build OK / cargo test 236 passed 0 failed / clippy + fmt (see Deviations)
- [x] Canonical progress.md updated

## Record

- 2026-08-08: Setup + explore complete. Maintainer amendment received in-session:
  NO server-side range validation for `mod_song_speed` (store verbatim; client DLL
  owns the domain). Supersedes task spec TR-5 and the invalid-value test legs.
- TDD red: 5 new tests in `crates/game-server/src/handlers/ddr_world/playdata.rs::tests`
  compile-failed on the absent `mod_song_speed` field (E0560/E0609) before the model regen.
- Model: `"mod_song_speed": "s32?"` added to BOTH `PlayerdataLoadOption` (after
  `mod_assist_tick`, end of the PersistMode::Full block) and `PlayerdataSaveOption`
  (same position, before the SaveOnly `mod_customize_*` block).
  `cargo run -p codegen -- models/ddr_world/ crates/bemani-protocol/src/ddr_world/`
  produced exactly +4 lines in `playdata_3.rs` (skip_serializing_if on load,
  serde(default) on save).
- Migration `migrations/012_ddr_world_song_speed.sql` (touched + git-added before
  writing): single nullable `opt_mod_song_speed INT NULL DEFAULT NULL`, comment
  documents the verbatim-storage policy.
- DB plumbing: `opt_mod_song_speed: Option<i32>` in the entity struct,
  `row_to_profile!`, UPDATE column list + params (inserted after `opt_mod_assist_tick`
  in both lists — positional alignment verified by sqlx compile-time checking).
- Handler: one only-when-present line in the modpack-options block (with a comment
  recording the no-validation policy), load echo in `handle_playerdata_load`, `None`
  in `build_new_player_response`.
- sqlx: `sqlx migrate run` applied 012; `cargo sqlx prepare --workspace` regenerated
  the cache (3 modified + 1 removed + 1 new query JSON — the profile SELECT/UPDATE
  hashes changed as expected). New query file git-added.
- Tests green: 236 total (231 baseline + 5 new: present-parse ×2 values, absent-None,
  malformed-None, None-skipped-on-load serialization, Some-echoed-on-load serialization).

## Gate evidence (logs/)

- `cargo build`: OK (build.log)
- `cargo test`: 236 passed, 0 failed (test.log)
- `cargo clippy --workspace --all-targets`: exit 0; 29 pre-existing warnings
  (newer-clippy lints, e.g. `is_multiple_of`, `no_effect`) — count identical on the
  pristine tree (verified via stash), so this change introduces ZERO new warnings (clippy.log)
- `cargo fmt --check`: FAILS ON THE PRISTINE TREE (211 diffs under local rustfmt 1.9.0
  stable 2026-05-25) — see Deviations (fmt.log)

## Deviations

- TR-5/TR-7/AC-2 validation legs dropped per direct maintainer instruction
  (2026-08-08). `mod_song_speed` is stored verbatim like every sibling `mod_*` field.
  Malformed (non-numeric) values still skip via `child_i32` parse failure.
- **fmt gate: pre-existing toolchain skew, NOT introduced by this change.** The
  pristine bemani-buddy tree already fails `cargo fmt --check` with 211 diffs under
  the local rustfmt (1.9.0-stable 2026-05-25; repo pins no toolchain and has no
  rustfmt.toml) — the committed style uses compact multi-field-per-line struct
  literals that this rustfmt wants exploded. Running `cargo fmt` would churn ~100
  maintainer-owned files, so it was NOT run. New code was written to match the
  committed surrounding style (verified: the only fmt spans attributable to this
  change are the test helper matching the file's existing compact literal style and
  one pre-existing hunk split by a one-line insertion). Maintainer should confirm
  which rustfmt version the repo standardizes on.
- Handler round-trip through `handle_save_profile` itself is untestable offline (needs
  a DB; the crate has no DAO mock — precedent: the workout fields test only the parse
  mechanics). Live round-trip evidence lands in Task 04's cabinet pass (matrix leg i).

## Change surface (bemani-buddy, all uncommitted; migration + new .sqlx query staged)

- `models/ddr_world/playdata_3.json` (+2 lines)
- `crates/bemani-protocol/src/ddr_world/playdata_3.rs` (codegen, +4 lines)
- `migrations/012_ddr_world_song_speed.sql` (new)
- `crates/db/src/models/ddr_world/profile.rs` (+1)
- `crates/db/src/mysql/ddr_world/profile.rs` (+3 lines across macro/UPDATE/params)
- `crates/game-server/src/handlers/ddr_world/playdata.rs` (ingestion + echo + new-player
  + 5 tests with `load_option_all_none` helper)
- `.sqlx/` (regenerated: 3 modified, 1 removed, 1 added)
