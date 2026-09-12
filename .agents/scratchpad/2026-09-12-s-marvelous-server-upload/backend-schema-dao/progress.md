# Progress — Step 4: bemani-buddy migration 019, models, DAO, tie-break
Status: Complete (uncommitted — maintainer commits manually); live-DB round trip pending (migration applies at server start)
- `migrations/019_ddr_world_smarv.sql` (7 nullable cols × both tables, contract header).
- `DdrWorldSmarv` (+`CLEAR_KIND_SMFC`, `is_smfc`) + `smarv: Option<DdrWorldSmarv>` on the 4 score structs; re-exported.
- `mysql/ddr_world/score.rs`: `SELECT_SCORES`/`ScoreRow`/`row_to_score!` (all-or-nothing `smarv_from_row`), INSERT/UPDATE/attempt INSERT bind 7 more (`smarv_cols`), `replaces_personal_best` (points >, or == with higher smarv count). 2 unit tests.
- `db` re-exports `chrono` (test fixtures downstream). No `.sqlx/` regen needed (runtime queries).
