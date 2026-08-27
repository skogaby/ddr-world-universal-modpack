# Tasks: 20260404-custom-song-injection

## Task Breakdown

### Phase 1: Reverse Engineering

- [x] **Task 1: RE — Series Filter Internals**
  - **Goal**: Produce `docs/series_filter_internals.md` documenting how the `series` field is parsed, stored, validated, and used for filtering internally.
  - **Scope**:
    - Trace musicdb.xml parsing of `<series>` to find the per-song data structure and series field offset
    - Identify all code paths that read the series field (filtering, sorting, display)
    - Find bounds checks or validation that limits the series range
    - Determine internal representation (raw u8, index, bitmask, etc.)
    - Test extending the range via Cheat Engine memory patching
    - Document AOB signatures for all relevant functions
    - Investigate `FUN_18011eda0` as a starting lead
  - **Tests**: Series-30 song added to musicdb.xml appears in ALL MUSIC list after patching bounds check in memory
  - **Dependencies**: Ghidra + Cheat Engine MCP servers, live DDR World instance

- [x] **Task 2: RE — Filter UI Extension**
  - **Goal**: Produce `docs/filter_ui_extension.md` documenting how the VERSION filter UI renders entries and how to inject new ones.
  - **Scope**:
    - Find the filter entry data structure (count, layout, texture references)
    - Identify the function that builds/populates the VERSION filter panel
    - Document texture naming convention for series labels and which ARC/IFS contains them
    - Document the GOLD/WHITE/CLASSIC group tab mechanism and its relationship to cabinet generation
    - Determine if cabinet generation filtering affects custom series visibility
    - Test injecting a fake filter entry via Cheat Engine
    - Document AOB signatures for filter builder functions
  - **Tests**: Fake filter entry visible in VERSION filter UI after memory injection
  - **Dependencies**: Task 1 findings (series representation informs filter entry structure)

### Phase 2: Implementation

- [x] **Task 3: SeriesExpansionMod — Config + Series Range Extension**
  - **Goal**: Ship a mod that reads `series-expansion.json`, patches the series range validation, and makes series-30 songs appear in the song list.
  - **Scope**:
    - Create `src/mods/series_expansion.rs` with `SeriesExpansionMod` implementing the `Mod` trait
    - Implement `SeriesConfig` serde struct for `series-expansion.json`
    - Add new AOB signatures to `signatures.rs` (from Task 1 findings)
    - Implement the series range extension hook/patch (exact approach from Task 1)
    - Register mod in `mod.rs` and `lib.rs`
    - Graceful disable if config file missing
  - **Tests**: Build passes. Songs with series 30 in musicdb.xml appear in ALL MUSIC song list with the mod enabled.
  - **Dependencies**: Task 1 RE document (provides hook targets and patch approach)

- [x] **Task 4: SeriesExpansionMod — Filter UI Injection**
  - **Goal**: Custom series entries appear in the VERSION filter UI and filtering works correctly.
  - **Scope**:
    - Implement filter entry injection hook (exact approach from Task 2)
    - Load custom series label textures via `asset_loader::register_arc()`
    - Wire texture names from config to injected filter entries
    - Handle cabinet generation bypass if needed (from Task 2 findings)
    - Verify filter count updates correctly
    - Verify custom filter combines with other filters (difficulty, BPM)
  - **Tests**: "WORLD PLUS" entry visible in VERSION filter. Selecting it shows only series-30 songs. Filter count is correct.
  - **Dependencies**: Task 2 RE document, Task 3 (series range extension must be working)

- [x] **Task 5: Multi-Series Extensibility + Polish**
  - **Goal**: Multiple custom series work end-to-end. Config-driven, no recompile needed.
  - **Scope**:
    - Verify array-based config works with multiple entries (series 30, 31, etc.)
    - Each custom series gets its own filter entry with distinct texture
    - Test adding a second series without recompiling
    - Update README.md with series expansion documentation
    - Update steering files (product.md, structure.md) to reflect new mod
  - **Tests**: Two custom series (30 and 31) both appear as separate filter entries. Adding series 32 via config only (no recompile) works.
  - **Dependencies**: Task 4 (single series must work first)
