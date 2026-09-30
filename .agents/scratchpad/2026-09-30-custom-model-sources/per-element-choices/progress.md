# Progress — per-element-choices
- [x] `selection.rs`: `StageChoice` / `DancerChoice`, `resolve_choice`, `source_stage_pool`, `source_dancer_pool`, `PickSource::Source`; tests (211 total)
- [x] `pick.rs`: `{source}` summary test + doc; `lifecycle::option_pick` adapted (identical draws)
- [x] Gate: `cargo check` clean, `cargo fmt`, `./build.sh` clean (1m14s), both harnesses green (211 / 72)
## Deviations
- `source_stage_pool` returns `(subset, StagePool)` rather than a new variant (the task allowed either); `StagePool::rows(&subset)` then yields the source's rows for `NoneLeft`.
Status: Complete (uncommitted — maintainer commits manually)
