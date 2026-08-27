# Requirements: Custom Song Injection via Series Filter Extension

## Goals

Enable modders to add custom songs to DDR World by extending the game's series filter system. Songs added to `musicdb.xml` with new series values (30+) should appear in-game and be filterable through a new entry in the VERSION filter UI. The system must be extensible to support multiple custom series for future use cases (e.g., console-exclusive song omnimix packs).

## Deliverables

This feature has three distinct deliverables, reflecting the RE-first nature of the work:

1. **RE Document: Series Internals** (`docs/series_filter_internals.md`) — How the `series` flag in musicdb.xml is parsed, stored in memory, and used for filtering. What limits the range of valid series values. What hooks or patches are needed to extend it.

2. **RE Document: Filter UI Extension** (`docs/filter_ui_extension.md`) — How the VERSION filter UI renders its entries (textures, layout, data structures). How group tabs (GOLD/WHITE/CLASSIC) work and relate to cabinet generations. What's needed to inject a new filter entry into the UI. What texture assets are required and what ARC/IFS they live in.

3. **Rust Implementation** — A new mod (or extension to core) that patches the game to accept new series values and renders corresponding filter UI entries. Proof of concept: series 30 = "WORLD PLUS". Designed for extensibility to multiple custom series.

## User Stories

### US-1: Custom songs appear in-game with new series value
**As a** modder, **I want to** add songs to musicdb.xml with a custom series value (e.g., `<series>30</series>`) and have them appear in the song list, **so that** I can add custom content to DDR World.

**Acceptance Criteria:**
- [ ] Songs with series value 30 in musicdb.xml load and appear in the ALL MUSIC song list
- [ ] Songs are playable (selectable, chart loads, gameplay works)
- [ ] Songs with series 30 are available regardless of cabinet generation (GOLD/WHITE/CLASSIC groups)
- [ ] No existing songs or series filters are broken by the extension

### US-2: New series filter entry in VERSION filter UI
**As a** player, **I want to** see a "WORLD PLUS" entry in the VERSION filter, **so that** I can filter the song list to show only custom songs.

**Acceptance Criteria:**
- [ ] A "WORLD PLUS" entry appears in the VERSION filter panel (as seen in the Filter Switch UI)
- [ ] Selecting "WORLD PLUS" filters the song list to only songs with series 30
- [ ] The filter entry has a proper texture label (user-provided, loaded from a custom ARC/IFS)
- [ ] The "Filtering Results" count updates correctly when the new filter is applied
- [ ] The new filter entry can be combined with other filters (difficulty, BPM, etc.) as normal

### US-3: Extensible to multiple custom series
**As a** modder, **I want** the system to support adding multiple custom series values (30, 31, 32, ...) each with their own filter label, **so that** I can organize different song packs into separate folders (e.g., console exclusives, community charts).

**Acceptance Criteria:**
- [ ] The implementation does not hardcode a single custom series value — it supports a configurable list
- [ ] Each custom series can have its own filter label name and texture
- [ ] Adding a new custom series does not require recompiling the DLL (configuration-driven, e.g., via mod-config.json or a dedicated config file)
- [ ] Multiple custom series entries appear as separate entries in the VERSION filter UI

### US-4: RE documentation of series internals
**As a** developer/reverse engineer, **I want** a thorough document explaining how the series system works internally, **so that** I understand what to hook/patch and can maintain the mod across game updates.

**Acceptance Criteria:**
- [ ] Document lives at `docs/series_filter_internals.md`
- [ ] Documents how musicdb.xml `series` values are parsed and stored in memory
- [ ] Documents the data structures that hold per-song series information
- [ ] Documents how the game filters songs by series value
- [ ] Documents what limits the valid range of series values (hardcoded bounds, array sizes, bitmasks, etc.)
- [ ] Documents the specific functions/addresses involved (with AOB signatures or RTTI names where applicable)
- [ ] Documents the proposed approach for extending the series range

### US-5: RE documentation of filter UI system
**As a** developer/reverse engineer, **I want** a thorough document explaining how the VERSION filter UI works, **so that** I know how to inject new filter entries.

**Acceptance Criteria:**
- [ ] Document lives at `docs/filter_ui_extension.md`
- [ ] Documents how filter entries are rendered (texture names, layout logic, data structures)
- [ ] Documents which ARC/IFS files contain the existing series filter label textures
- [ ] Documents the texture naming convention for series labels
- [ ] Documents how the group tabs (GOLD/WHITE/CLASSIC) work and their relationship to cabinet generations
- [ ] Documents the proposed approach for injecting new filter entries into the UI
- [ ] Documents what texture assets a modder needs to provide for a new series entry

## Out of Scope

- **Song content creation** — Creating or converting song audio, charts, or metadata is not part of this feature
- **musicdb.xml editing tools** — The user manually edits musicdb.xml; no tooling is provided
- **Song jacket textures** — The user handles jacket art separately from series filter textures
- **Texture art creation** — The user provides pre-made texture assets for series labels; the mod loads them but doesn't generate them
- **Console song porting** — The omnimix use case is a future feature that builds on this foundation
- **Lua scripting API** — No scripting integration; this is a core Rust mod
- **Online/e-amusement compatibility** — Custom songs are for local/offline play only

## Assumptions

- The game's series value is stored as a `u8` (per musicdb.xml schema), giving a theoretical max of 255. The actual usable range may be smaller depending on internal data structures (bitmasks, fixed-size arrays, etc.) — the RE effort will determine this.
- The VERSION filter UI has a finite number of entry slots. The RE effort will determine whether this is a fixed array or dynamically sized, and what the practical limit is.
- Custom ARC/IFS files can be loaded via the existing `asset_loader` service (already proven with `custom_mod.arc` for HelloWorld textures).
- The user has Ghidra and Cheat Engine available via MCP servers for the RE work, with a live DDR World instance attached.

## Open Questions

- **Series value range**: Is the series stored as a raw u8 throughout, or does the game map it to an index/bitmask at some point? (RE deliverable 1 will answer this)
- **Filter entry limit**: Is there a maximum number of VERSION filter entries the UI can display? (RE deliverable 2 will answer this)
- **Texture format**: What exact format/dimensions are the series label textures? (RE deliverable 2 will answer this)
- **Config file location**: Should custom series configuration live in `mod-config.json` alongside other mod settings, or in a dedicated file like `custom-series.json`? (Design phase decision)

## Dependencies

- Ghidra + Cheat Engine (MCP servers) for reverse engineering
- Live DDR World instance for testing
- Existing `asset_loader` service for loading custom ARC files
- Existing `scene_manager` for scene awareness (filter UI is on scene 25 = SONG_SELECT)
- Existing AOB scanner and RTTI walker for finding game functions
