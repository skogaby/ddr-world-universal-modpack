# Tasks: 20260511-webui-options

Tasks are sized to be commit-ready: one shippable unit that builds independently, roughly ~1 day of focused work.

## Workspace Info

**Primary module/package**: `src/mods/webui_options/`

---

## Task 1: Customize offset detection signature

**Module(s)**: `src/core/signatures.rs`
**Goal**: Add a derived address `customize_offset` that resolves the byte offset of `ddr::player::Customize` within `PlayerWork` at runtime via RTTI walk.
**Scope**: RTTI walk for the `.?AVCustomize@player@ddr@@` TypeDescriptor string → vtable → find the constructor that writes the vtable pointer → extract the displacement from the store instruction. Expose via `get_address("customize_offset")`. Graceful degradation if detection fails.
**Tests**: `cargo check` passes. Verify via deploy log output that the offset resolves to `0x1790` on 20260324 or `0x1770` on 20250805.
**Dependencies**: Existing RTTI walk infrastructure, existing `player_work_table` signature.

- [x] 1.1 Add RTTI walk logic to locate the Customize vtable from the TypeDescriptor string
- [x] 1.2 Locate the constructor function (xref that writes the vtable pointer) and decode the store displacement
- [x] 1.3 Store the resolved offset as a derived address in the SignatureStore
- [x] 1.4 Log the detected offset at init time for verification

### Acceptance

**Status**: Pending
**Notes**:

---

## Task 2: Asset discovery module

**Module(s)**: `src/mods/webui_options/discovery.rs`
**Goal**: Filesystem scanner that discovers available asset IDs per customization category by enumerating ARC files on disk.
**Scope**: Define the `CategoryDef` table (9 categories, excluding BGM). Implement `discover_all()` that scans `data/arc/custom/<category>/` and `data_mods/*/data/arc/custom/<category>/` for files matching the naming pattern, extracts numeric IDs, deduplicates, and returns sorted `Vec<u32>` per category. No option registration or Customize writes yet.
**Tests**: `cargo check` passes. Deploy and verify log output shows correct ID counts per category.
**Dependencies**: None (pure filesystem logic).

- [x] 2.1 Define `CategoryDef` struct and static table of 9 category definitions (option_id, display_name, scan_dir, file_pattern_prefix, customize_field_offset)
- [x] 2.2 Implement filename→ID extraction (parse `%04d` from filenames matching the prefix, deduplicate variants like `_result`, `_1p`, `_2p`)
- [x] 2.3 Implement `discover_all()` that scans both game and data_mods paths, returns `Vec<DiscoveredCategory>`
- [x] 2.4 Log discovered counts per category at init for verification

### Acceptance

**Status**: Pending
**Notes**:

---

## Task 3: Mod skeleton and option registration

**Module(s)**: `src/mods/webui_options/mod.rs`, `src/mods/mod.rs`, `src/lib.rs`
**Goal**: Create the `WebUiOptionsMod` struct implementing the `Mod` trait. On `enable()`, run discovery, then register one scalar option row per category via `custom_options::register_option()`. Placeholder no-op on_change callbacks.
**Scope**: Mod lifecycle (new/init/enable/disable), required_signatures declaration, option registration with correct min/max from discovered ID count. No Customize writes or persistence yet — callbacks are stubs.
**Tests**: `cargo check` passes. Deploy and verify mod appears in mod menu and option rows render on the Mods tab with correct ranges.
**Dependencies**: Task 1 (customize_offset signature), Task 2 (discovery module).

- [x] 3.1 Create `src/mods/webui_options/mod.rs` with `WebUiOptionsMod` implementing the `Mod` trait
- [x] 3.2 Add `pub mod webui_options;` to `src/mods/mod.rs`
- [x] 3.3 Register the mod in `src/lib.rs`
- [x] 3.4 In `enable()`: call `discover_all()`, store results, register scalar options with `min=0, max=count-1`
- [x] 3.5 Declare `player_work_table` and `customize_offset` in `required_signatures()`

### Acceptance

**Status**: Pending
**Notes**:

---

## Task 4: Customize memory writes (on_change callbacks)

**Module(s)**: `src/mods/webui_options/mod.rs`
**Goal**: Wire up the on_change callbacks to map sequential values to real asset IDs and write them directly to the Customize object fields in memory.
**Scope**: Replace stub callbacks with real logic. On value change: look up asset_ids[value] from the discovery result, resolve PlayerWork via player_work_table for the given side, write the asset ID to the correct field at `PlayerWork + customize_offset + field_offset`. Handle the lane category specially (two sub-fields at +0x18/+0x1C selected by category def).
**Tests**: `cargo check` passes. Deploy, change an option in-game, and verify the customization visually changes (e.g., appeal board on result screen, lane skin in gameplay).
**Dependencies**: Task 3.

- [x] 4.1 Implement the write path: player_work_table → wrapper → PlayerWork → Customize base + field offset
- [x] 4.2 Implement sequential-to-asset-ID mapping lookup in the callback
- [x] 4.3 Wire each category's on_change to the shared write logic with the correct field offset
- [x] 4.4 Handle edge cases: null PlayerWork pointer (player not logged in), index out of bounds

### Acceptance

**Status**: Pending
**Notes**:

---

## Task 5: Label texture generation script and assets

**Module(s)**: `scripts/gen_webui_option_labels.py`, `data_mods/custom_options/.../tex/`
**Goal**: Generate the 9 row-label PNG textures for the option rows, following the existing convention.
**Scope**: Python script that generates `seop_item_cust_<category>.png` for each of the 9 categories at 176x16 RGBA with appropriate label text (e.g., "APPEAL BOARD", "BACKGROUND", "CHARACTER", etc.). Output to the LayeredFS texture directory. Run the script and commit the generated PNGs.
**Tests**: Visual verification that labels render correctly in-game on the Mods tab.
**Dependencies**: None (can be done in parallel with other tasks, but needed before final visual verification).

- [x] 5.1 Create `scripts/gen_webui_option_labels.py` following the `gen_scroll_dummy_labels.py` pattern
- [x] 5.2 Define label text for each category ("APPEAL BOARD", "BACKGROUND", "CHARACTER", "LANE (SINGLE)", "LANE (DOUBLE)", "LANE COVER (S)", "LANE COVER (D)", "CATEGORY 6", "CATEGORY 7")
- [x] 5.3 Generate PNGs and place in `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`
- [x] 5.4 Verify labels render in-game

### Acceptance

**Status**: Pending
**Notes**:

---

## Task 6: JSON persistence (dual save/load)

**Module(s)**: `src/mods/webui_options/mod.rs`
**Goal**: Persist selections to `mod-config.json` under `"webui_options"` with per-player keys, storing real asset IDs. Load on init as fallback when network values aren't available.
**Scope**: On value change, write the real asset ID (not sequential index) to JSON. On `enable()`, after discovery, read JSON and reverse-map stored IDs to sequential indices to prime the options. If a stored ID no longer exists on disk, fall back to index 0.
**Tests**: `cargo check` passes. Deploy, change options, restart game, verify selections persist without network.
**Dependencies**: Task 4.

- [x] 6.1 Implement JSON save: on each on_change, write the asset ID to `mod-config.json["webui_options"]["p1"/"p2"][category_key]`
- [x] 6.2 Implement JSON load: on enable, read stored asset IDs, reverse-map to sequential indices
- [x] 6.3 Handle missing/invalid IDs gracefully (fall back to index 0 / default)
- [x] 6.4 Ensure network-loaded values (via resolve_from_load) override JSON-primed values

### Acceptance

**Status**: Pending
**Notes**:

---
