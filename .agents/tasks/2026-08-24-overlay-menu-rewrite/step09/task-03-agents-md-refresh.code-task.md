# Task: AGENTS.md documentation refresh for the mod-menu rewrite

## Description
Update AGENTS.md so a future agent finds accurate entry points and config
notes for the rewritten mod menu: the tabbed shell, theme system, anchor
emitter, `option_menu_settings` (replacing `row_order`), and the new
`overlay_menu` config section. (README is explicitly OUT of scope — the
maintainer is overhauling all user-facing docs in a separate session before
open-sourcing.)

## Background
AGENTS.md still documents the pre-rewrite menu: the Config section lists
`row_order` (deleted in Step 5, replaced by
`custom_options.option_menu_settings` [{id, overlay?, in_game?}]), and there
is no entry-point row for the mod_menu module family or the overlay_draw
emitter. The music_wheel_song_length row documents "X/Y live-tunable from the
mod-menu overlay scalar rows" (removed in task-02). The authoritative facts:
`.agents/planning/2026-08-24-overlay-menu-rewrite/` (design/progress),
`docs/overlay_draw_research.md` (final anchor-emitter architecture), and the
Step 5–8 module docs.

## Technical Requirements
1. Key Entry Points table: add a "Mod Menu (overlay)" row covering
   `src/mods/mod_menu/` (mod.rs lifecycle/gestures, model.rs pure tab/nav
   model, tabs.rs builders, rows.rs frozen contributed-row API, input.rs
   exclusive consumer, render.rs widgets/chrome/anchor, chrome.rs +
   chrome_loader.rs synthesis, theme.rs palettes/backgrounds) +
   `src/services/overlay_draw/` (encode.rs pure encoders; the identity-gated
   anchor emitter; `emitter_ready`/`set_background`/`set_emit_anchor`
   surfaces; the "same list ≠ same z" lesson pointer) + harness pointers
   (`scripts/validate_mod_menu.sh`, `scripts/validate_overlay_draw.sh`,
   `scripts/validate_custom_options.sh`) + planning-dir pointer. Follow the
   existing table rows' dense single-cell style.
2. Config section: REPLACE the `row_order` bullet with
   `custom_options.option_menu_settings` (array of {id, overlay?, in_game?};
   array order = display order in both menus; unknown ids warn once;
   operator-authored) and ADD an `overlay_menu` bullet ({theme:
   arrows|bubbles|wavefield|minimal, animate_background, opacity 25..=100
   snap 5}; DLL persists on APPEARANCE-tab changes — one of the few sections
   the DLL writes).
3. Update the music_wheel_song_length entry-point row: X/Y offsets are
   config-only now (drop the "live-tunable … overlay scalar rows" clause,
   keep the config keys).
4. Sweep AGENTS.md for other stale `row_order` mentions (the custom_options
   Config bullet mentions it) and stale mod-menu claims (e.g. "pinpad nav"
   descriptions if present); fix only mod-menu-related staleness — no
   unrelated rewrites.
5. Preserve the Custom Instructions section verbatim and the file's overall
   structure/tone.

## Dependencies
- task-02 (the music-wheel row text must describe post-removal behavior).

## Implementation Approach
1. Read the current AGENTS.md mod-menu-adjacent rows + Config section.
2. Draft the new/updated rows against the planning dir + research doc.
3. Verify every named file/symbol exists before citing it.

## Acceptance Criteria

1. **Entry points accurate**
   - Given a fresh agent reading only AGENTS.md
   - When it needs the mod-menu shell, theme system, or background emitter
   - Then the table names the correct files, APIs, harnesses, and the
     research doc for the emitter's evidence chain

2. **Config docs match the code**
   - Given the Config section
   - When compared with `src/mods/config.rs` + custom_options ordering +
     chrome_loader parsing
   - Then `option_menu_settings` and `overlay_menu` are documented with the
     right shapes/defaults and `row_order` no longer appears anywhere

3. **No stale mod-menu claims**
   - Given a grep for `row_order`, `mwsl-offset`, "live-tunable"
   - When run over AGENTS.md
   - Then only accurate post-Step-9 text remains

4. **Non-goals respected**
   - Given the final diff
   - When reviewed
   - Then README.md is untouched and AGENTS.md's Custom Instructions section
     is byte-identical

## Metadata
- **Complexity**: Low
- **Labels**: docs, agents-md
- **Required Skills**: repo conventions
