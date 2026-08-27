# Plan — mod-menu-pure-model

Status: Approved 2026-08-24 (auto mode; verified approval chain per context.md)

## Test scenarios (host harness)

1. `build_mods_tab`: excludes `mod-menu`; preserves input order; Boolean rows carry
   name/description/enabled; source = RegistryToggle.
2. `build_global_tab` matrix: enabled owner (header + rows in order), disabled owner
   (group absent), owner with zero rows (no header), unowned rows (trailing, no
   header), empty inputs (empty list).
3. Navigator skip/wrap: header at list start and end + greyed run mid-list — down from
   last selectable wraps to first selectable; up from first wraps to last; cursor
   never rests on header/greyed.
4. All-unselectable: up/down no-ops, `selected()` None, clamp parks in range.
5. Clamp-after-rebuild: cursor beyond new len; cursor lands on now-greyed row (snaps
   to nearest selectable, down first); the scroll>cursor underflow guard; scroll
   beyond `len−page` after shrink.
6. Scroll follow: paging down/up across 12-row pages; `page_window` bounds at the tail.
7. `scroll_indicator`: (1,N) at top, (N,N) at bottom, len<page cases.
8. TabNav: per-tab cursor/scroll memory across switches; next/prev wrap; reset.

## Implementation

`src/mods/mod_menu/model.rs` per context.md API; `pub(super) mod model;` in mod.rs
(no consumers yet — task-02); `scripts/validate_mod_menu.sh`.
