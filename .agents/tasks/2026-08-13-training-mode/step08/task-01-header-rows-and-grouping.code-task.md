# Task: Header rows (UiKind::Header) + TRAINING OPTIONS grouping

## Description

Add non-selectable, display-only **header rows** to the custom_options
framework (`UiKind::Header` — design §4.8) and register the TRAINING
OPTIONS group heading with them. A header is a normal framework row
whose selectability interface is swapped out (the engine's own
gray-row mechanism), rendered as a slim full-width label. Per R10,
a header is injected **only when its id appears in the operator's
`row_order`** — unlisted headers are absent entirely (no orphaned
headers), unlike normal rows which keep the append-at-end policy.

This is plan Step 8. The grouped default `row_order` + README example
land in Step 9 — this task ships the mechanism plus the one real
header (`header_training_options`), verified by listing it in the
local `mod-config.json` for the demo.

## Background

- **The native mechanism** (RE, `docs/option_header_rows_research.md`):
  the game's gray "MIN~CORE~MAX" row is an ordinary layout slot the
  cursor skips because ONE thing differs — its 2-slot MI interface at
  `row+0x28` has slot-0 (`isSelectable`) hardcoded to `return 0`. All
  three cursor paths (first/last walk, directional scan, tab-open
  focus) test slot-0 and skip failures; the layout engine packs every
  `+0xB8 = 1` row regardless. A mod header = donor-clone row + a
  mod-owned 2-slot `+0x28` vtable `{return 0, no-op}` + label-only
  render. **Zero new signatures** — both slots are mod code, the table
  is `memory::alloc_zeroed` + leak (the existing synthesized-vtable
  pattern in `rows.rs`, which already synthesizes the primary and
  `+0xC0` tables per row).
- **Slim slot**: the grid layout advances by each row's own y-extent at
  `row+0xA8` (a per-row layout input the ctor fetched from the
  `"option_item"` metrics — research §5). Read the donor-written value
  after the ctor, halve it, write it back — subsequent rows pack
  closer; no other row is affected.
- **Render ownership**: the framework's slot-7 render override already
  owns everything drawn per mod row; a `RowKind::Header` draws only
  the full-width label texture (no value box, no marker, no
  tri-arrows, no preview). The `+0x110` abort gate is irrelevant —
  header rows never call the native render.
- **Ordering today** (`ordering.rs`): pure `compute_order(registered,
  configured)` → display permutation (listed first, unlisted appended,
  unknown ids warn-once); `builder_hook.rs` applies it to its per-open
  handle snapshot (registry indices never move). R10 extends the pure
  logic: unlisted HEADERS are dropped from the result instead of
  appended.
- **Scroll/paging**: headers participate as ordinary rows (`+0xB8`
  masking via filter_hook + the scroll driver's window mask) — a
  header scrolls with its group and the cursor scan already skips
  unselectable rows across window edges. Engine-native, verify-only
  (research §4 edge behaviors).
- **Gotchas** (research §7): never point the `+0x28` table at the
  native `return 0`/no-op function addresses (mod stubs are free);
  `+0xB8` masking still owns visibility; slot-1 (`onFocusChanged`) is
  unreachable once unfocusable but must be stubbed anyway.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md
  (§4.8 header rows; R9 grouping via config, no hardcoded group lists;
  R10 header render policy)

**Additional References (if relevant to this task):**
- docs/option_header_rows_research.md — the complete mechanism RE
  (+0x28 interface, cursor predicate, +0xA8 height, implementation
  strategy §4, gotchas §7)
- docs/options_scroll_research.md — container/row layout, +0xB8/+0x168
  offsets (background only)
- docs/option_row_marker_render.md — slot-7 render ownership
  (background only)
- .agents/planning/2026-08-13-training-mode/implementation/plan.md —
  Step 8 (objective/guidance/tests/demo)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. **API** (`src/services/custom_options/api.rs`): `UiKind::Header`
   variant (no values, no scalar bounds) + a `RegisterSpec::header(id)`
   constructor. Headers carry no persistence (`PersistMode` n/a — hold
   no state), no change callbacks, no children/`ShowWhen`, no preview.
   Label texture follows the existing `seop_item_<id>` convention.
2. **Registry** (`registry.rs`): validate at registration — refuse a
   header spec carrying persistence, callbacks, parent/child links, or
   values, with the registry's existing error reporting. Host tests for
   the validation matrix (registry.rs is harness-mounted).
3. **Row build** (`rows.rs`): for `RowKind::Header` — donor ctor as
   today, then (a) swap `row+0x28` to ONE process-lifetime mod-owned
   2-slot vtable `{return 0, no-op}` (`memory::alloc_zeroed` + leak,
   shared by all header rows), (b) halve the y-extent at `row+0xA8`
   (read the donor-written value, halve, write back), (c) slot-7
   render draws only the label texture, full row width; value box,
   marker, tri-arrows, and preview are not drawn.
4. **R10 ordering** (`ordering.rs` + `builder_hook.rs`): extend the
   pure ordering fn so callers pass which ids are headers; a listed
   header takes its listed position, an UNLISTED header is EXCLUDED
   from the result (not appended); normal rows keep today's behavior
   byte-identically (identity fast-path included). `builder_hook`
   skips excluded handles at injection. Mount `ordering.rs` in the
   host harness and add the ordering-policy tests.
5. **Registration**: `training_mode`'s `enable()` registers
   `header_training_options` (best-effort, beside the bound rows;
   `set_option_available(false)` at disable). No wire field, no JSON
   cache, nothing persisted.
6. **Asset**: generate `seop_item_header_training_options` ("TRAINING
   OPTIONS") via `scripts/gen_option_labels.py`, sized/styled for the
   half-height full-width slot; ship via the existing LayeredFS
   data_mods path and copy to the install.
7. **Fail-open** (design §6): header injection failure (vtable
   synth/alloc/register refusal) ⇒ header absent, rows render
   ungrouped, one WARN — never blocks the normal rows or the mod.

## Dependencies

- `src/services/custom_options/rows.rs` synthesized-vtable pattern
  (shipped — the +0x00/+0xC0 tables; this task adds the +0x28 table)
- `ordering.rs` `compute_order` + `builder_hook` permutation apply
  (shipped, Step-6-era row_order machinery)
- `scripts/gen_option_labels.py` label pipeline (shipped)
- NO new AOB signatures, no new detours (research §6)

## Implementation Approach

1. R10 ordering first (TDD — pure logic + host tests in `ordering.rs`,
   harness mount added; the policy is the step's only host-testable
   surface besides registry validation).
2. `UiKind::Header` + `RegisterSpec::header` + registry validation
   (+ validation host tests).
3. `rows.rs` header build: +0x28 vtable swap, +0xA8 half-height,
   label-only render.
4. `builder_hook` exclusion wiring; training_mode registration; label
   asset generation + install copy.
5. Gates in order: harness `cargo test` → `cargo check --target
   x86_64-pc-windows-msvc` → `cargo fmt` (whole crate) → `./build.sh`;
   list `header_training_options` in the local `mod-config.json`
   `row_order` for the demo; cabinet demo.

## Acceptance Criteria

1. **Header renders as a slim full-width label**
   - Given `header_training_options` listed in `row_order` above the
     training rows
   - When the MODS tab opens
   - Then a half-height, full-width TRAINING OPTIONS heading renders at
     that position with no value box, marker, tri-arrows, or preview

2. **Cursor skips it in both directions and on tab open**
   - Given the header positioned between (or above) selectable rows
   - When the player navigates up/down across it, or opens the tab with
     the header first
   - Then focus lands only on selectable rows, never the header

3. **Header scrolls with its group**
   - Given more rows than the scroll window
   - When the window scrolls across the header's position
   - Then the header masks/unmasks exactly like any row (window slot
     semantics unchanged)

4. **R10: unlisted header is absent**
   - Given `header_training_options` NOT in `row_order` (or `row_order`
     absent)
   - When the MODS tab opens
   - Then the header is not injected at all, and normal rows render
     exactly as today (identity fast-path untouched)

5. **Normal-row ordering unchanged**
   - Given a `row_order` listing a mix of normal ids
   - When the display permutation is computed
   - Then normal listed rows order first and unlisted normal rows
     append at the end — byte-identical to the shipped behavior

6. **Registration validation**
   - Given a header spec carrying persistence, callbacks, values, or
     child links
   - When registration runs
   - Then it is refused with the registry's existing error path (host
     tests cover the matrix)

7. **Fail-open**
   - Given the header's vtable alloc or registration refused
   - When the menu opens
   - Then the normal rows render ungrouped with one WARN, and gameplay
     is unaffected

8. **Host tests green**
   - Given the temp-dir harness (with `ordering.rs` newly mounted)
   - When `cargo test` runs
   - Then the new ordering-policy + registry-validation tests pass and
     the suite stays green

## Metadata

- **Complexity**: Medium
- **Labels**: training-mode, custom-options, options-ui, header-rows,
  row-order
- **Required Skills**: Rust, in-process hooking discipline (AGENTS.md
  rules), the custom_options row factory / synthesized-vtable pattern,
  cabinet deploy validation
- **Generated By**: code-task-generator 2026-08-15
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 8: TRAINING OPTIONS header row + grouping
