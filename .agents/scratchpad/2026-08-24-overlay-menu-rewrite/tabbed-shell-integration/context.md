# Context — tabbed-shell-integration

Task: .agents/tasks/2026-08-24-overlay-menu-rewrite/step03/task-02-tabbed-shell-integration.code-task.md
Mode: auto. Approval chain verified (plan + design approved 2026-08-24).

## Facts

- Layout amendment (maintainer, 2026-08-24): modal margins ~50–75 px — using uniform
  60 px: modal 1160×600 @ (60,60). Design §4.5 updated.
- `TextAlignment::{Left,Center,Right}` exists (text_widget.rs:23) — value column and
  N/M indicator right-align at x=1180.
- All five registrants pass `parent_row_key = mod id` → owning-mod grouping holds with
  zero registrant edits.
- Registration API stays frozen: `rows.rs` keeps `MenuRow`/`RowKind` as the
  REGISTRATION record type; `tabs.rs` converts to `model::RowKind` for display
  (two RowKind types by design — public API untouched; noted in module docs).
- Visual verdicts are maintainer-only (standing instruction). Agent may keypad-inject
  + screenshot for telemetry/handoff material only.

## Layout constants (render.rs; first-deploy tunable)

Title (80,74) scale .75 · tab bar y=116 (labels x = 80 + i×260, scale .6; active =
`[LABEL]` + amber, inactive grey) · N/M right-aligned (1180,116) scale .5 · rows
x=100 label / x=1180 value (right-aligned), start y=156, ROW_H=34, 12 slots,
scale .55 · cursor `>` x=76 · footer desc (80,584) scale .45 wrap-free · key hints
(80,612) scale .45: `8/2: Nav  4/6: Adjust  +Start: Coarse  1/3: Tab  0x3: Close` ·
headers accent (0.35,0.75,1.0), greyed 0.45-grey, bool ON green / OFF red.

## Surgery map

- `mod.rs` state: DROP rows/visible_rows/selected_index/scroll_offset; ADD
  `tab_nav: model::TabNav`, `tab_rows: Vec<Vec<model::Row>>` (parallel TabId::ALL).
  open(): rebuild all tabs, `tab_nav.reset()` + clamp; close(): nothing extra
  (reset on next open; state preserved-but-unused after close is harmless — reset
  BOTH at open for determinism).
- `tabs.rs` NEW: snapshot assembly (`entries_callback` → ModEntrySnap;
  `contributed_rows` → ContributedSnap with kind conversion) + `rebuild_tabs(state)`.
- `rows.rs`: keep registration API + contributed_rows + value bookkeeping
  (`apply_row_value` on contributed_rows only); `toggle_registry_mod` now saves the
  mods map from a fresh `entries_callback` read; DELETE rebuild_rows/rebuild_visible/
  row_value/clone_row.
- `input.rs`: nav through `model::Navigator` (PAGE=12) on the active tab; 1/3 tab
  switch (NUM_1/NUM_3, wrap, per-tab memory); activation matches on `row.source`
  (RegistryToggle → toggle path; Contributed → step/cycle/set + on_change lookup by
  key in contributed_rows); repeat thread predicate via the model selection.
- `render.rs`: full rewrite per constants above; slots = (label, value) pairs;
  allocate-once; text-only.
