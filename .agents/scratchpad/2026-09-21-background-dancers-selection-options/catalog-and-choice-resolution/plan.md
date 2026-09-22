# plan — catalog-and-choice-resolution

Status: Approved 2026-09-21 (auto mode; verified approval chain)

## Tests (first)
catalog.rs:
- `split_key_cases`: emi01→(EMI,2), crystaldium00→(CRYSTALDIUM,1), boom06→(BOOM,7), abc→(ABC,1),
  replicant05→(REPLICANT,6), "" → ("",1).
- `label_for_cases`: ("EMI",2,3)→"EMI #2"; ("CLUB",1,1)→"CLUB"; ("BOOM",7,7)→"BOOM #7".
- `stock_catalog`: dancers from the 26 stock keys (fixture rows through `dancer_candidates`), stages from
  `selection::real_map_rows` (34 rows) through `stage_candidates`: sorted, 26/25, dummy00 absent, boom00 once,
  named labels present, every label ≤ 15 bytes, `Catalog::label/key/count` semantics (0→"RANDOM"? — no:
  `label(kind, 0)` returns None; the RANDOM text is the options layer's concern; document).
- `clamp_edges`: (-1,25)→0, (0,25)→0, (25,25)→25, (26,25)→0, (3,0)→0.

selection.rs:
- `resolve_choice_rules`: stage Some("boom00") over 2000 seeds ⇒ rows {0,32} both seen, only boom00;
  dancers [Some("emi01"), None] ⇒ d0 always emi01, d1 varies; unknown keys ⇒ None; empty dancer_keys ⇒ None;
  all-None reproduces `pick_stage` + `pick_dancers` under the same seed.

## Implementation
- catalog.rs as specified; `real_map_rows` exposed `#[cfg(test)] pub(crate)` in selection.rs (harness mounts
  both at the crate root, `super::selection::tests` is private → expose the fixture as `pub(crate) fn` in a
  `#[cfg(test)] pub(crate) mod fixtures`).
- `resolve_choice` in selection.rs.
- Harness mount + `pub mod catalog;`.
