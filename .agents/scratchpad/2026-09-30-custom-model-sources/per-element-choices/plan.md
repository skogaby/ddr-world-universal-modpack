# Plan — per-element-choices
Status: Approved 2026-09-30 (auto — verified upstream approval chain)
Tests: `resolve_choice_rules` migrated (+ empty-pool cases); new `random_pools_are_respected`, `source_pools` (Filtered / NoneLeft-relative-to-source / All / unknown keys; dancer pool order); `pick.rs` `{source}` assertion.
Implementation: enums + `resolve_choice` (Key looked up in full tables, Random over the pool), `source_stage_pool -> (subset, StagePool)`, `source_dancer_pool`, `PickSource::Source`; `option_pick` maps today's Option<String> to Key / Random(global).
