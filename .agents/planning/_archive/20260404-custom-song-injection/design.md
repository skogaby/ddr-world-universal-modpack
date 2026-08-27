# Design: Custom Song Injection via Series Filter Extension

**Requirements**: [requirements.md](requirements.md)

---

## Overview

Extend DDR World's series filter system to accept custom series values (30+) and render corresponding entries in the VERSION filter UI. This enables modders to add custom songs to musicdb.xml with new series designations and filter them in-game.

The feature is RE-first: two research documents must be produced before implementation begins, because the exact hooking strategy depends on how the game internally represents series values and filter entries.

---

## Architecture Decisions

### Decision 1: Mod, not Service
**Problem**: Where does series expansion logic live in the codebase?
**Decision**: New mod `SeriesExpansionMod` in `src/mods/series_expansion.rs`
**Rationale**: This is a self-contained feature with enable/disable lifecycle, not a general-purpose game system that other mods depend on. It follows the same pattern as `AutoplayMod` and `FastBootupMod` — hooks installed on enable, removed on disable. No other mod needs to call into it.
**Alternative considered**: New service in `src/services/`. Rejected because services are for shared game system integrations (widget renderer, scene manager, input). Series expansion is a feature, not infrastructure.

### Decision 2: Dedicated config file for series definitions
**Problem**: Where do custom series definitions live?
**Decision**: Dedicated `series-expansion.json` in the game directory, read by the mod at init.
**Rationale**: `mod-config.json` stores simple enable/disable booleans. Series definitions are structured data (value, label, texture name, ARC path) that would bloat and complicate the existing config. A separate file also means users can edit series definitions without touching mod toggle state, and the file can be shared/distributed independently.
**Alternative considered**: Embed in `mod-config.json` under a `"series-expansion"` key. Rejected because it couples two different concerns and makes the config harder to hand-edit.

### Decision 3: RE-first phased approach
**Problem**: We don't know the exact internal representation of series values or filter entries.
**Decision**: Produce two RE documents before writing any Rust code. The implementation design below is our best architectural hypothesis — specific hook signatures, patch locations, and data structure layouts will be confirmed or revised by RE findings.
**Rationale**: Committing to an implementation before understanding the game internals risks building the wrong thing. The existing RE docs in `docs/` prove this approach works — each prior feature (widgets, autoplay, scene manager) was preceded by RE investigation.
**Tradeoff**: Slower to first code, but avoids throwaway work.

### Decision 4: Extensible from day one, but hardcoded proof-of-concept first
**Problem**: Should the first implementation be hardcoded (series 30 = "WORLD PLUS") or fully config-driven?
**Decision**: Build the config-driven architecture from the start, but validate with a single hardcoded entry first. The config file format supports an array of series definitions. The proof-of-concept ships with a default `series-expansion.json` containing one entry (series 30).
**Rationale**: The config parsing and array-based hook logic is not significantly more complex than a single hardcoded value. Building it right the first time avoids a refactor when the omnimix use case arrives.

---

## Component Design

### New Components

| Component | Location | Responsibility |
|-----------|----------|----------------|
| `SeriesExpansionMod` | `src/mods/series_expansion.rs` | Mod lifecycle, config loading, hook installation |
| `SeriesConfig` | `src/mods/series_expansion.rs` (internal) | Serde struct for `series-expansion.json` |
| RE doc: Series Internals | `docs/series_filter_internals.md` | How series values are parsed, stored, filtered |
| RE doc: Filter UI | `docs/filter_ui_extension.md` | How filter entries render, texture names, injection approach |

### No new services, widgets, or type modules required.

Existing services used:
- `asset_loader::register_arc()` — load custom ARC with series label textures
- `scene_manager` — scene awareness (filter UI is scene 25)
- Signature store — new AOB signatures for series-related functions

### Component Interaction

```
series-expansion.json ──read──→ SeriesExpansionMod
                                    │
                                    ├── asset_loader::register_arc(arc_path)
                                    │       └── loads custom series label textures
                                    │
                                    ├── Hook/Patch: Series Range Extension
                                    │       └── game accepts series values > 21
                                    │
                                    ├── Hook/Patch: Filter Entry Injection
                                    │       └── VERSION filter shows custom entries
                                    │
                                    └── Hook/Patch: Cabinet Generation Bypass (if needed)
                                            └── custom songs visible in all cabinet modes
```

---

## Config Format

```json
{
  "custom_series": [
    {
      "series_value": 30,
      "label": "WORLD PLUS",
      "texture_name": "series_world_plus"
    }
  ],
  "arc_path": "data/arc/bm2d/custom_series.arc"
}
```

- `custom_series` — array of series definitions, each with a numeric value, display label (for logging/debugging), and texture asset name (bare name, no path/extension — matches existing BM2D convention)
- `arc_path` — single ARC file containing all custom series label textures
- If the file is missing, the mod logs a warning and disables itself gracefully

---

## Expected Hooks and Patches (RE-Dependent)

These are architectural hypotheses. The RE phase will confirm the exact mechanisms.

### Hook 1: Series Range Extension
**What**: The game likely validates series values during musicdb.xml parsing or song list building. Values above the current max (21) are probably rejected or ignored.
**Expected mechanism**: Either a bounds-check comparison (`cmp` instruction, patchable like `TimerFreezeMod`) or a lookup table/array with a fixed size (requires reallocation or hook).
**Approach**: RE Phase 1 will identify the exact check. Patch or hook to accept values up to 255 (full u8 range).

### Hook 2: Filter Entry Injection
**What**: The VERSION filter UI builds a list of selectable entries from an internal data structure. We need to append custom entries.
**Expected mechanism**: Either a fixed array in `.rdata` (patch to point to a larger allocated array) or a dynamically-built list (hook the builder function to append entries).
**Approach**: RE Phase 2 will identify the data structure. Hook the function that populates filter entries to append custom series after the built-in ones.

### Hook 3: Cabinet Generation Bypass (conditional)
**What**: The GROUP GOLD/WHITE/CLASSIC tabs filter songs by cabinet generation. Custom songs must appear in all groups.
**Expected mechanism**: A per-song flag or a series-to-cabinet mapping. Custom series values may fall outside the mapping and be excluded.
**Approach**: RE Phase 2 will determine if this is needed. If cabinet filtering is independent of series filtering (as the user suspects), this hook may not be necessary. If it is, hook the cabinet filter to whitelist custom series values.

### New Signatures
New `SignatureDefinition` entries will be added to `src/core/signatures.rs` as the RE phase identifies target functions. Expected additions (names TBD by RE):
- Series validation/bounds check function
- Filter entry list builder function
- Possibly: cabinet generation filter function

---

## RE Investigation Approach

### Phase 1: Series Internals

**Starting points:**
- User's Ghidra lead: `FUN_18011eda0` (possible sort/filter logic)
- Search Ghidra for string references to `"series"` to find musicdb.xml parsing code
- Search Cheat Engine for byte value `21` (current max series) near known song data in memory

**Investigation sequence:**
1. Find where musicdb.xml `<series>` is parsed — trace the XML parser for the "series" element name
2. Identify the per-song data structure and which field stores the series value
3. Find all code that reads the series field — these are the filtering/validation points
4. Identify bounds checks (comparisons against max series value)
5. Test: patch the bounds check in Cheat Engine, add a series-30 song to musicdb.xml, verify it appears in the song list
6. Document findings, AOB patterns, and proposed patch approach

### Phase 2: Filter UI Extension

**Starting points:**
- The VERSION filter is active on scene 25 (SONG_SELECT)
- Existing filter labels are textures — search ARC/IFS files for texture names containing "WORLD", "A3", etc.
- Use `texture_resolver` to probe candidate texture names at runtime

**Investigation sequence:**
1. Find the filter entry data structure — search for the count of VERSION filter entries (~11) or pointers to texture name strings
2. Identify how entries map series values to display labels (direct mapping? lookup table?)
3. Find the function that builds/renders the VERSION filter panel
4. Identify the texture naming convention for series labels
5. Document the group tab (GOLD/WHITE/CLASSIC) mechanism — how it relates to cabinet generation and whether it affects series filtering
6. Test: inject a fake entry via Cheat Engine memory editing, verify it renders
7. Document findings, texture names, data structures, and proposed hook approach

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Series stored as bitmask (e.g., 32-bit for series 0–31) | High — limits custom series to ≤31, or requires bitmask expansion | Medium | RE Phase 1 will determine this early. If bitmask: either use values within the mask range (22–31) or patch the bitmask to a larger type. Fallback: use series 22–29 instead of 30+. |
| Filter entry list is fixed-size array | Medium — can't just append, need to reallocate | Medium | Allocate a new larger array via `memory::alloc_zeroed()`, copy existing entries, append custom ones, redirect the game's pointer. Same pattern as `AutoplayMod`'s `AUTO_PANEL` allocation. |
| Filter textures are hardcoded by index, not name | Medium — can't just add a texture, need to match expected indices | Low | If indexed: allocate entries at specific indices. If named: follow the naming convention. RE Phase 2 will clarify. |
| Cabinet generation filter excludes unknown series | Low — custom songs invisible in some cabinet modes | Medium | Hook the cabinet filter to whitelist custom series values, or set a "all cabinets" flag on custom songs. RE Phase 2 will determine if this is even an issue. |
| FUN_18011eda0 is a dead end | Low — wastes some RE time | Medium | Multiple investigation paths defined (string search, memory scanning, texture probing). Not dependent on a single lead. |
| Game update changes function signatures | Low — mod breaks on update | Ongoing | All hooks use AOB signatures, not hardcoded offsets. New signatures will follow the same pattern. Signatures may need updating on game patches, but the architecture survives. |

---

## Changes to Existing Code

### `src/core/signatures.rs`
- **Change**: Add new `SignatureDefinition` entries for series-related functions (exact patterns determined by RE)
- **Impact**: None to existing signatures

### `src/mods/mod.rs`
- **Change**: Add `pub mod series_expansion;`
- **Impact**: None

### `src/lib.rs`
- **Change**: Register `SeriesExpansionMod` in the mod registration block. Register custom series ARC via `asset_loader::register_arc()`.
- **Impact**: None to existing init sequence

### No changes to existing services, widgets, or other mods.

---

## Deployment Sequence

1. Complete RE Phase 1 → produce `docs/series_filter_internals.md`
2. Complete RE Phase 2 → produce `docs/filter_ui_extension.md`
3. Implement `SeriesExpansionMod` based on RE findings
4. User creates series label textures, packages as ARC/IFS
5. User adds songs to musicdb.xml with series 30, repackages startup.arc
6. User places custom series ARC at configured path
7. User creates `series-expansion.json` with series definitions
8. Test: verify songs appear, filter entry renders, filtering works

---

## Open Questions (Deferred to RE)

1. What is the internal representation of series values — raw u8, index into array, or bitmask?
2. Is the filter entry list a fixed array or dynamically built?
3. What are the exact texture names for existing series labels?
4. Which ARC/IFS contains the series label textures?
5. Does cabinet generation filtering interact with series filtering?
6. Are there any other places that validate series values beyond the initial parse?
