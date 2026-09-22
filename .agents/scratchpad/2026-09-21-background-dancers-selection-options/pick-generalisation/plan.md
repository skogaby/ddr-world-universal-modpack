# plan — pick-generalisation (Step 4 task-01)

Status: Approved 2026-09-21 (auto mode: the approved plan Step 4 + design §4.8 stand in for plan approval)

## Test scenarios (pure, `pick.rs` `#[cfg(test)]`, mounted by the harness)
1. `gameplay_pick_is_unchanged` — stage `boom00` row 0 + dancers `emi01`, `rage00` (from the real fixture rows,
   `exists = |_| true` so every part arc is present): `pick.stage == Some(stage)`, `arcs()[0] == "data/arc/mapset_boom00.arc"`,
   `arcs().last() == "data/arc/pl_shadow00.arc"`, `arcs_for(&GAMEPLAY) == arcs()`, summary starts with
   `stage=boom00[0]{random} parts=5 dancers=[`, and `assemble_pick` ≡ `assemble_pick_opt(Some(stage))` under the same seed
   (field-by-field: camera lists, playlists, parts).
2. `stage_only_pick` — `assemble_pick_opt(Some(stage), rows, vec![], ..)`: arcs == `[mapset]`, playlists/parts empty,
   summary contains `dancers=[] clips=[] wear=[]`.
3. `dancer_only_pick` — `assemble_pick_opt(None, rows, vec![emi01], ..)`: no `mapset_` in `arcs()`; `arcs()` ends with
   the shadow arc; `arcs_for(&PREVIEW)` has no shadow arc; `camera_main/non` empty; summary contains
   `stage=none{random} parts=0`.
4. `parse_options_consts` — `GAMEPLAY.shadow && !PREVIEW.shadow`.

## Implementation
1. Cut the "Pick" section (struct, `with_sources`, `arcs`, `summary`, `make_pick`, `assemble_pick`) from `session.rs`
   into `pick.rs`; keep `use super::selection::{…}` only.
2. `stage: Option<StageCandidate>`; `assemble_pick_opt`; `assemble_pick` = `assemble_pick_opt(Some(stage))`;
   `arcs()` pushes the stage arc only when `Some`; `arcs_for(opts)` filters the shadow arc; `summary()` branches the
   stage token.
3. `session.rs`: `pub use super::pick::*` names; `parse_pick(pick, opts)`; branch on `pick.stage` / `dancers.is_empty()`
   / `opts.shadow`; camera warning only for `Some(stage)`.
4. `lifecycle.rs`: `parse_pick(&pick_for_thread, &ParseOptions::GAMEPLAY)`.
5. `mod.rs`: `pub mod pick;`. Harness: mount `pick`.
6. Gates: `cargo check`, harness, `cargo fmt`.

## Risks
- `summary()` regression: pinned by scenario 1's prefix check; the `Some` branch keeps the existing `format!` verbatim.
- Import breakage: every external consumer imports from `session`; re-exports keep those paths.
