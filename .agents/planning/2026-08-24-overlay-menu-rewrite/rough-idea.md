# Rough Idea: Overlay Menu (triple-press 0) Complete Rewrite

Captured 2026-08-24 from the maintainer's briefing (lightly organized; content verbatim in intent).

## Problem

The current triple-press-0 overlay menu (`src/mods/mod_menu.rs`) is one of the oldest
pieces of the modpack, written immediately after the text-widget RE work. Issues:

- Very ugly; horrendous UI and UX. One endless scroller — no tabs, no separation of
  concerns, no scroll indicators.
- Hodgepodge of concerns: rows that enable/disable a mod wholesale sit beside rows
  that *configure* mods. For some it's unclear what turning it off in the 000 menu
  does vs. toggling in-game options; it's not obvious that disabling a mod in the
  overlay disables the injection of its in-game custom option rows.
- Poor screen real estate: only 6–7 rows render even though there's room for more;
  text may be needlessly oversized.
- Plain white text drawn straight over the game screen — no backing texture, nothing
  to obscure gameplay.

Needs a complete and total rewrite.

## High-Level Vision (maintainer is not married to specifics)

### Multi-tabbed view

1. **Mods tab (enable/disable):** top-level mod on/off, basically what the menu does
   today. Disabling a mod here means it no longer functions and no longer injects
   custom options in-game.
2. **Global config tab:** globally-configure mods that are already enabled. Options
   that apply across both players actively, regardless of profile (today these are
   mixed into the same scrollview as the on/off rows).
3. **Per-player/per-profile tab:** a subset (later: possibly the sole home) of the
   options that currently exist as injected in-game options. Rationale: eventually
   some options migrate out of the in-game injected Mods tab and live solely in the
   overlay — mainly mods whose context is genuinely per-player/per-cabinet-side but
   whose values are rarely touched (set-and-forget). For THIS pass: options exist
   *both* in-game and in the overlay, mirroring each other. After end-user UAT the
   maintainer decides each option's final home. Bake that flexibility into the design.
   - If no active session is live (attract screen or any scene before login/music
     selection), this tab — or all options within it — should be greyed out and
     unselectable. Configuring per-player options during attract makes no sense
     (server login could clobber them).

### Replicate ALL in-game injected options into the per-player tab

Whenever a custom option is registered by any mod (including decorative header rows),
that row is also replicated into the overlay menu. A registration parameter configures
which menu it appears in (one or both), defaulting to both if unspecified. May need
one or two new parameters on the custom option registration API (display name,
description text, etc.) — determine during discovery.

### mod-config.json expansion

Broaden the `row_order` concept: allow config to specify which menu (one, both, or
neither) each option appears in. Potentially replace/deprecate `row_order` with
`option_menu_settings` — an array of structs instead of bare strings; each struct has
the row name plus either an enum key ("OVERLAY" | "IN_GAME" | "BOTH") or two boolean
keys (`overlay:`, `in_game:`). Enum might be cleaner but more error-prone for
hand-editing — decide during clarification.

### Presentation

- Pleasing to the eye; ideally fits DDR World's presentation style thematically.
- Bare minimum: semi-transparent textures backing the menu to obscure gameplay.
- Modal presentation: does NOT obscure the entire game — sits above the game screen
  with visible edges and rounded corners, so a little of the game remains visible
  around the edges. The menu as a whole is semi-transparent (~80% opaque / 20%
  transparent) so the game shows through slightly.

### Theming system

- Maintainer is in final polish phases before public release / open sourcing; UI/UX
  polish is the focus. A theme system would make this feel like a polished product.
- Ship with 3–4 built-in themes. Each theme dictates the menu color scheme AND
  provides an animated background for the overlay (replacement for, or addition to,
  the semi-transparent backing).
- Animations via D3D shaders: slick, procedurally generated, parameterized, light on
  storage. NOT background movies.
- An option (under a themes tab?) to disable the background animation shaders, for
  users who dislike them or run low-end hardware.
- Built-in theme ideas: at least one with a scrolling DDR-arrow pattern background
  (subtle/low-opacity so it isn't distracting); others winamp-visualizer-esque —
  bouncing circles/bubbles; something geometric (e.g. cubes riding the surface of a
  moving 3-D wave). Open to ideas; goal is presentation value while staying
  procedural.
- Precedent: the repo already compiles custom shaders (`shaders/`,
  `scripts/build_shaders.sh`, runtime `shader_synthesis`), but those OVERRIDE
  existing game shaders. Loading/presenting shaders at-will may need new RE work.

## Notes

- The maintainer offered to capture a screenshot of the current experience if needed.
- Stream-of-consciousness braindump; treat everything above as negotiable except the
  core: tabs, replication of custom options, presentation/theming direction, config
  expansion.
