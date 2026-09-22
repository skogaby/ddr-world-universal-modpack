# progress — pick-generalisation (Step 4 task-01)
- [x] Baseline harness 127 ✓ (`logs/harness-baseline.log`).
- [x] `pick.rs` (pure): `Pick` (stage `Option<StageCandidate>`), `ParseOptions { shadow }` + `GAMEPLAY`/`PREVIEW`,
      `with_sources`, `arcs()` = `arcs_for(&GAMEPLAY)`, `arcs_for` (stage arc only when `Some`, shadow arc only when
      `shadow && dancers non-empty`), `summary()` (`Some` branch byte-identical; `None` ⇒ `stage=none{src} parts=0`),
      `make_pick`, `assemble_pick` (wraps `Some`), `assemble_pick_opt` (same rng draw order). 6 tests.
- [x] `session.rs`: Pick section replaced by `pub use super::pick::{assemble_pick, make_pick, ParseOptions, Pick}`
      (`assemble_pick_opt` is NOT re-exported — an unused re-export warns in this crate; Step 5 imports it from
      `pick` directly); `parse_pick(pick, opts)`: stage block under `if let Some(stage)`, shadow block gated on
      `opts.shadow`, camera block `match pick.stage` (None ⇒ `cameras = None`, no warning); unused selection imports
      dropped.
- [x] `lifecycle.rs`: `parse_pick(&pick_for_thread, &ParseOptions::GAMEPLAY)`; `mod.rs`: `pub mod pick;`.
- [x] Harness mounts `pick` → 133 ✓ (`logs/harness.log`). `cargo check` clean, 0 warnings (`logs/cargo-check.log`).
      `cargo fmt` — no unrelated churn.
## Deviations
- `assemble_pick_opt` not re-exported from `session` (see above); available as `super::pick::assemble_pick_opt`.
Status: Complete (uncommitted — maintainer commits manually)
