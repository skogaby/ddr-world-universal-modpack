# Task: Ordering + placement core — `option_menu_settings`, `row_order` deletion

## Description
Rework the custom_options ordering layer around the new operator schema: parse
`custom_options.option_menu_settings` (array of `{id, overlay?, in_game?}`,
array order = display order), extend the order computation to also produce
per-id placement overrides, DELETE all `row_order` reading code (leftover keys
silently ignored per D17), and stand up `scripts/validate_custom_options.sh`
so the reworked pure layer is host-testable on this machine.

## Background
Step 5 of the overlay-menu rewrite (design §4.3.5/§4.4). Today
`ordering.rs` (243 lines) stores a lowercased `Vec<String>` from
`custom_options.row_order` in a OnceCell (`set_configured_order`,
ordering.rs:48; read at mod.rs:137-145) and exposes
`display_order_for(ids, is_header)` (ordering.rs:227, sole caller
builder_hook.rs:208). The pure `compute_order` (ordering.rs:69-119,
8 in-file tests) implements: unconfigured ⇒ identity minus headers;
listed-first in listed order (headers allowed when listed); unlisted
non-headers appended in input order; unlisted headers EXCLUDED (R10);
duplicates place-once; unknown ids collected → warn-once
(UNKNOWN_WARNED, ordering.rs:42).

The new schema (design §4.4): each entry `{ "id": "...", "overlay": bool?,
"in_game": bool? }`. Array order = display order in BOTH menus. Omitted keys
inherit the registration default (placement enforcement itself is task-02/03
— this task only computes and exposes the overrides). `"overlay": false,
"in_game": false` = hidden everywhere. Unknown ids: one WARN, ignored.

IMPORTANT — current in-crate tests (ordering's included) cannot run on this
ARM host (`retour`); the new harness makes them real. `ordering.rs` imports
`crate::log_warn` (ordering.rs:33) — the harness's generated lib.rs defines a
`#[macro_export] macro_rules! log_warn` stub before the `#[path]` mounts so
the file mounts unmodified.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.3.5 ordering rework, §4.4 config schema, §6 unknown-id row)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-24-overlay-menu-rewrite/idea-honing.md — D3 (schema), D17 (row_order removed outright, silently ignored)
- scripts/validate_mod_menu.sh — the temp-crate harness template

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **config.rs**: in `CustomOptionsConfig` (src/mods/config.rs:15-59) DELETE
   `row_order` (field :55 + doc block :45-53); ADD
   `option_menu_settings: Option<Vec<OptionMenuSettingConfig>>` with a new
   serde struct `OptionMenuSettingConfig { id: String, overlay: Option<bool>,
   in_game: Option<bool> }` (all `#[serde(default)]` except id). serde ignores
   unknown JSON keys, so leftover `row_order` in operator config is silently
   ignored — no warn (D17).
2. **ordering.rs rework**:
   - Plain (serde-free) `pub(crate) struct OptionMenuSetting { pub id: String,
     pub overlay: Option<bool>, pub in_game: Option<bool> }`.
   - `set_configured_settings(Vec<OptionMenuSetting>)` replaces
     `set_configured_order` (same OnceCell one-shot + lowercase-at-store
     semantics).
   - Pure core: extend/replace `compute_order` so it returns, alongside the
     existing permutation + unknown-id list, the placement overrides for
     registered ids (e.g. a parallel `Vec<Option<(Option<bool>, Option<bool>)>>`
     or an id-keyed map — implementer's choice, but pure and parameterized for
     tests). ALL existing ordering semantics preserved verbatim (listed-first,
     unlisted-append, unlisted-headers-excluded, duplicates-once,
     case-insensitive, identity fast path when unconfigured).
   - Runtime queries: `display_order_for(ids, is_header)` keeps its exact
     signature/behavior (builder_hook.rs:208 compiles untouched);
     new `pub(crate) fn placement_override_for(id: &str) -> (Option<bool>,
     Option<bool>)` (in_game, overlay) — `(None, None)` when unconfigured or
     id unlisted. Case-insensitive like the rest.
   - Unknown-id warn-once preserved (message updated to name
     `option_menu_settings`).
3. **mod.rs**: replace the config read at src/services/custom_options/mod.rs:137-145
   — map `option_menu_settings` config structs into
   `ordering::OptionMenuSetting` and call `set_configured_settings`.
4. **Doc/string sweep** (references to the dead key): api.rs:50
   (`UiKind::Header` doc), builder_hook.rs:192/199 comments, ordering.rs doc
   header/test comments, `src/mods/decorative_option_headers.rs` — doc
   comments (:12/:14/:30), the mod description string (:60), and the enable
   log line (:103) all name `custom_options.row_order`; update to
   `option_menu_settings` (semantics unchanged: headers render only when
   listed).
5. **Migrate the shipped `mod-config.json`** (repo root, `row_order` array at
   :72): convert the existing array to the equivalent
   `option_menu_settings` list (`{"id": "..."}` per entry, order preserved) so
   the maintainer's cabinet ordering survives the next deploy.
6. **New `scripts/validate_custom_options.sh`** (validate_mod_menu.sh
   template): mounts `src/services/custom_options/ordering.rs`; generated
   lib.rs defines `log_warn!` (and `log_info!`/`log_debug!` for future
   mounts) stub macros BEFORE the `#[path]` mounts; `[dependencies]`
   `once_cell = "1"` (ordering uses OnceCell). Tasks 02/03 extend MODULES.
7. In-file tests updated/added (all runnable via the new harness): the 8
   existing semantics tests carried forward against the new core; config-parse
   shapes are config.rs's serde (covered by the resolution tests taking plain
   structs); NEW placement tests — listed id with explicit flags, listed id
   with omitted flags ⇒ `(None, None)`… wait, omitted flags on a LISTED id ⇒
   `(None, None)` (inherit registration default); unlisted id ⇒ `(None,
   None)`; the "neither" case `(Some(false), Some(false))` reported verbatim;
   case-insensitive id match; overrides present even when the entry only
   affects ordering.
8. Crate builds for the msvc target; in-game behavior unchanged EXCEPT
   `row_order` no longer honored (covered by requirement 5's migration).

## Dependencies
- None on tasks 02/03 (this lands first; enforcement consumes
  `placement_override_for` later).

## Implementation Approach
1. Read ordering.rs fully; port the pure core + tests first under the new
   harness (red on the new placement queries).
2. Land config.rs + mod.rs read + string sweep + mod-config.json migration.
3. Gates: `./scripts/validate_custom_options.sh` → `cargo check` →
   `cargo fmt` → `./build.sh`.

## Acceptance Criteria

1. **Order semantics preserved**
   - Given the 8 pre-existing ordering scenarios
   - When run against the reworked core via the new harness
   - Then all pass unchanged (listed-first, unlisted-append, header rules,
     duplicates, case-insensitivity, identity fast path).

2. **Placement overrides**
   - Given settings `[{a, overlay:false}, {b}]` over registered ids a/b/c
   - When placement is queried
   - Then a ⇒ `(None, Some(false))`, b ⇒ `(None, None)`, c ⇒ `(None, None)`,
     and the display order is a, b, c.

3. **row_order fully dead**
   - Given the src/ tree
   - When grepping for `row_order`
   - Then no code reads it (only unavoidable historical planning docs; README
     migration is Step 9), and a config carrying the key parses without
     warnings.

4. **Host harness**
   - Given `./scripts/validate_custom_options.sh`
   - When run on this host
   - Then ordering's tests compile and pass (log stub in effect).

## Metadata
- **Complexity**: Medium
- **Labels**: custom-options, ordering, config, pure-layer
- **Required Skills**: Rust, serde, repo host-test harness conventions
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 5: custom_options extensions — placement, display strings, observer, snapshot, `option_menu_settings`
