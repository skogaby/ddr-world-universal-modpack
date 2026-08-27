# Context — mod-menu-pure-model

Task: .agents/tasks/2026-08-24-overlay-menu-rewrite/step03/task-01-mod-menu-pure-model.code-task.md
Mode: auto. Approval chain verified (plan + design `Status: Approved 2026-08-24`).

## Touchpoint semantics transcribed (from Step 1's split files)

- `rows.rs::rebuild_rows`: registry rows in entries order (mod-menu excluded); each
  followed by contributed children (`visible_when` parent == mod id) in contributed
  order; orphans appended. The NEW model replaces child-inlining with per-mod GROUPS
  on the GLOBAL tab (design §4.2): header + rows for enabled owners only.
- `rows.rs::rebuild_visible` guards: cursor clamp to len-1 after shrink AND the
  scroll_offset > cursor underflow guard (refresh computes `cursor - scroll` as usize)
  — both become model tests.
- `render.rs::adjust_scroll`: scroll follows cursor into [scroll, scroll+page).
- Navigation: up/down wrap at ends (old code wraps; new adds header/greyed skip).

## Model API decisions (auto mode)

- `TabId { Mods, GlobalSettings }` + `TabId::ALL`/label/next/prev (wrap); later steps
  append variants. `TabNav` owns per-tab `NavState { cursor, scroll }` + active tab —
  cursor memory is model-owned and host-tested; `reset()` on menu close.
- Group header rows: `key = "__header_<mod_id>"`, `RowKind::Header`,
  `source = Contributed` (display-only; never selectable).
- `Navigator { rows, page }` pure over a built list: `up/down` (wrap+skip),
  `clamp_after_rebuild` (cursor clamp → nearest-selectable snap down-then-up → scroll
  guards incl. `scroll ≤ max(0, len−page)`), `selected() -> Option<usize>`,
  `scroll_indicator`, `page_window`.
- All-unselectable list: cursor parks (clamped), `selected()` = None, up/down no-op.

## Validation

New `scripts/validate_mod_menu.sh` mounting `model.rs` (validate_overlay_draw.sh
pattern). Gates: harness → `cargo check` → `cargo fmt` → `./build.sh`.
