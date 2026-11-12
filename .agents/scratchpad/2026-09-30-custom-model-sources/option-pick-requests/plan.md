# Plan — option-pick-requests
Status: Approved 2026-09-30 (auto — verified upstream approval chain)
No host tests (engine-facing; the pools and resolve_choice are tested in selection.rs). Validation: check/build + cabinet (§7.3 items 4).
Implementation: rewrite `option_pick` per design §4.7; `window_entry` passes `screen_filter`.
