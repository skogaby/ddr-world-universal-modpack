# Task: Tabbed shell integration — dense layout, tabs, footer

## Description
Replace the mod menu's flat-list runtime with the tabbed shell: MODS and GLOBAL
SETTINGS tabs driven by the task-01 model, ~12 single-line rows, a fixed footer
(selected row's description + key hints), scrollbar-position text ("N/M"), and `1`/`3`
tab switching. Text-only rendering — the chrome (panel/sprites) is Step 4.

## Background
This lands the rewrite's core UX (design §4.1/§4.2/§4.8, FR-1/2/3/12/13) on the
Step 1 module layout, consuming the pure model from task-01. The existing public
registration API (`ScalarRowSpec`/`EnumRowSpec`/`remove_rows_for`) and the registry
toggle + `save_mod_states` path must keep working unchanged — the five registrant mods
are not modified in this step. The old always-visible per-row description line is
REPLACED by the footer (that's where the density comes from), and the old `>` cursor +
`[ON]`/value column rendering carries over restyled (smaller scale, value right-aligned
at a fixed column).

Widget budget note: allocate-once/reuse discipline stands; text-only shell needs
roughly: title 1, tab labels 4 (two active now — render all four labels only when
their tabs exist; this step renders 2), rows 12×2, footer 2, N/M 1, cursor 1 ≈ 31
(pool measured at 254 free).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.1, §4.2, §4.5 layout numbers, §4.8 input, §6 ladder)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `tabs.rs` (new): impure snapshot assembly — build `ModEntrySnap`s from the registry
   `entries_callback` and `ContributedSnap`s from `contributed_rows`
   (`parent_row_key` → `owning_mod_id`) — then call the model builders. Rebuild
   triggers: open, tab switch, any toggle/adjust (the old rebuild points), preserving
   the old "toggle may register/remove contributed rows" re-read semantics.
2. `render.rs`: new layout constants per design §4.5 (rows start under the tab bar,
   ROW_HEIGHT ≈ 34, 12 slots, label scale ≈ 0.55, footer block at the bottom, value
   column right-anchored ≈ x=1180 with `TextWidget` right-alignment if available —
   verify `set_alignment` from the widget API; else keep a fixed x). Tab bar = one
   TextWidget per tab label; the ACTIVE tab renders full-bright + a `[` `]` or color
   accent (text-only affordance this step; sprite indicator arrives with chrome).
   Footer line 1 = selected row's description (hint), footer line 2 = static key
   hints incl. `1/3: Tab`. Header rows render label-only, accent color, no value.
3. `input.rs`: `1`/`3` (NUM_1/NUM_3, `src/types/buttons.rs`) switch tab prev/next with
   wrap; navigation delegates to the model's `Navigator` (skip/wrap/memory); Left/Right
   activation keeps the existing three arms (registry toggle / contributed on_change /
   scalar+enum adjust) but resolves the selected row through the model.
4. `mod.rs`/state: `ModMenuState` gains the active `TabId` + per-tab `NavState` + the
   built tab row lists; open resets to MODS tab; close preserves nothing (FR-1).
   Boolean `visible_when` child gating within GLOBAL is subsumed by the model's
   grouping (children of a disabled mod simply aren't built).
5. No behavior change to: gesture open/close, exclusive consumer + suppression,
   repeat thread, `save_mod_states` persistence, splash hint text.
6. Old rendering paths (`refresh_slots` full-scale two-line layout, separator line)
   are deleted — no dead code.

## Dependencies
- task-01 (model). Consumes it exclusively for list building + navigation.

## Implementation Approach
1. Extend `ModMenuState` + rewrite `render.rs` layout; then `input.rs` tab keys;
   then `tabs.rs` snapshot plumbing; delete dead paths last.
2. Gates: `./scripts/validate_mod_menu.sh` (model untouched but re-run), `cargo check`,
   `cargo fmt`, `./build.sh`.
3. Autonomous boot: open the menu via spice2x-cli keypad injection (triple-0), walk
   tabs/rows via injected keys, harvest the log for panics/WARNs, and capture
   screenshots FOR THE MAINTAINER — **visual verdicts are maintainer-only** (standing
   instruction 2026-08-24). Hand off with screenshots + what to check.

## Acceptance Criteria

1. **Two working tabs**
   - Given the menu opened at attract
   - When pressing 3 / 1
   - Then the tab cycles MODS ↔ GLOBAL SETTINGS with per-tab cursor memory, MODS
     showing every registered mod (12 rows per page, N/M indicator), GLOBAL showing
     enabled mods' contributed rows under per-mod headers.

2. **Footer semantics**
   - Given any selected row
   - When the cursor moves
   - Then the footer shows that row's description; mod rows' descriptions explain the
     disable side-effects (existing registry descriptions suffice this step).

3. **Functional regression**
   - Given the four contributed-row registrants
   - When toggling mods and adjusting FPS TARGET / RESTART DELAY / AA / timing offsets
     from their new homes
   - Then behavior and persistence match the old menu (config files update; values
     survive reopen).

4. **Maintainer visual sign-off**
   - Given the deployed build and agent-captured screenshots
   - When the maintainer reviews layout/density/readability
   - Then the step's demo is accepted (or layout constants adjusted and re-deployed).

## Metadata
- **Complexity**: High
- **Labels**: mod-menu, ui, integration
- **Required Skills**: Rust, repo widget/render-thread conventions, spice2x-cli
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 3: Tabbed shell — row model, MODS + GLOBAL SETTINGS tabs, dense layout
