# Design: WebUI Customization Options

**Requirements**: [requirements.md](requirements.md)

---

## Overview

A new mod (`WebUiOptionsMod`) that registers scalar custom-option rows for 9 player customization categories. On init, it scans the game's filesystem to discover available asset IDs per category, builds a sequential-to-asset-ID mapping for each, and registers one scalar option row per category through the existing `custom_options` service. When a player changes a value, the mod maps it back to the real asset ID and writes it directly into the `ddr::player::Customize` object in memory (bypassing the vtable setter bounds checks). Persistence flows through both the network hook and a per-player section in `mod-config.json`.

---

## Architecture Decisions

### Decision 1: Direct memory write vs vtable setter calls

- **Problem**: The game's Customize vtable setters enforce compile-time max validators that reject newer/unreleased asset IDs.
- **Decision**: Write directly to the Customize object's fields at their known offsets (+0x0C, +0x10, etc. from the Customize base), bypassing the vtable entirely.
- **Rationale**: Direct writes avoid all bounds checks, support arbitrary asset IDs, and are simpler than patching each setter. The game reads these fields on demand without caching.
- **Alternatives considered**: (a) Patch each setter's CMP immediate — invasive, 9 separate patches, fragile across versions. (b) Call setters with bounds-check NOP'd — same fragility.
- **Tradeoffs**: We lose the game's input validation (writing an invalid ID won't crash but may show no texture). Acceptable because the mod only writes IDs that were discovered on disk.

### Decision 2: Runtime asset discovery via filesystem scan

- **Problem**: Asset IDs are non-contiguous and grow with game updates. Hardcoding ranges would require mod updates on every data release.
- **Decision**: At mod init, scan `data/arc/custom/<category>/` for files matching the naming convention (e.g., `appeal_board_%04d.arc`) and extract the numeric IDs.
- **Rationale**: Fully data-driven — new assets are automatically available without code changes. The game's filesystem is available at DLL load time. AVS LayeredFS overlay directories are also scanned to capture mod-added assets.
- **Alternatives considered**: Parse a config file listing IDs — fragile, requires user maintenance. Scan at runtime from IFS manifests inside ARCs — more complex for the same result.
- **Tradeoffs**: Init cost of a directory listing per category (trivial — ~10ms total for 7 directories). No risk since `FindFirstFileW`/`FindNextFileW` is safe.

### Decision 3: Version-agnostic Customize offset via RTTI walk

- **Problem**: The `ddr::player::Customize` offset within PlayerWork shifted between 20250805 (+0x1770) and 20260324 (+0x1790). Cannot hardcode.
- **Decision**: At init, perform an RTTI vtable walk for `ddr::player::Customize`, then locate the constructor that writes the vtable pointer and extract the offset from the store instruction's displacement.
- **Rationale**: Same technique used for other version-shifting structs in this codebase. The TypeDescriptor string `.?AVCustomize@player@ddr@@` is a stable anchor.
- **Alternatives considered**: (a) Signature on the dispatch function to decode the `ADD` instruction — brittle if dispatch function changes. (b) Dual hardcoded offsets with version detection — violates the version-agnostic tenet.
- **Tradeoffs**: Slightly more init code, but robust against future version shifts.

### Decision 4: One mod with per-category on_change callbacks

- **Problem**: 9 option rows need to be registered, each mapping to a different Customize field.
- **Decision**: A single `WebUiOptionsMod` registers all 9 options in its `enable()` path. Each option gets a dedicated `on_change` callback that maps the sequential value to an asset ID and writes the appropriate Customize field.
- **Rationale**: Single mod keeps lifecycle simple. The on_change callbacks already receive `player_side`, which gives direct access to the correct PlayerWork via player_work_table.
- **Alternatives considered**: Per-category mods — excessive overhead for options that share identical structure.
- **Tradeoffs**: One larger mod file, but all logic is table-driven with minimal per-category code.

### Decision 5: Dual persistence (network + local JSON)

- **Problem**: Need persistence that works on supported servers and degrades gracefully for unsupported ones.
- **Decision**: Use the existing `custom_options` network persistence (the framework's `persist: true` flag) as primary. Additionally, on each value change, write to `mod-config.json` under a `"webui_options"` key with sub-keys `"p1"` and `"p2"`. On load, if network values arrive (via `resolve_from_load`), they win; otherwise fall back to the JSON values.
- **Rationale**: The custom_options service already handles network persistence and fires `on_change` on load. The JSON fallback only matters for servers that don't echo custom fields back.
- **Alternatives considered**: JSON-only — loses cross-machine persistence. Network-only — breaks for unsupported servers.
- **Tradeoffs**: Slightly redundant writes, but guarantees persistence regardless of server support.

---

## Component Design

### New / modified components

| Component | Location | Responsibility | Replaces / extends |
|-----------|----------|----------------|--------------------|
| `WebUiOptionsMod` | `src/mods/webui_options.rs` | Mod lifecycle, option registration, on_change callbacks, Customize writes | N/A (new) |
| `customize_discovery` (module) | `src/mods/webui_options/discovery.rs` | Filesystem scan for available asset IDs per category | N/A (new) |
| Label texture generator script | `scripts/gen_webui_option_labels.py` | Generate `seop_item_<id>.png` for each row label | N/A (new, follows `gen_scroll_dummy_labels.py` pattern) |

### Component interactions

```
┌─────────────────────────────┐
│  WebUiOptionsMod            │
│                             │
│  enable():                  │
│    1. discover_assets()     │──► filesystem scan (data/arc/custom/*)
│    2. detect_customize_offset() ──► RTTI walk for Customize vtable
│    3. register 9 scalar     │──► custom_options::register_option()
│       options (one per cat) │
│    4. load JSON fallback    │──► mod-config.json["webui_options"]
│                             │
│  on_change(side, value):    │
│    1. map sequential → ID   │
│    2. write Customize field │──► player_work_table → PlayerWork → Customize+offset
│    3. persist to JSON       │──► mod-config.json
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│  custom_options service     │
│  (existing)                 │
│  - Registers scalar rows    │
│  - Handles network persist  │
│  - Fires on_change on load  │
└─────────────────────────────┘
```

### Removed components

None.

---

## Integration Points

**Existing services consumed**:

- `custom_options`: `register_option()` with `UiKind::Scalar`, `get_value()`, network persistence via `persist: true`
- `player_work_table` signature: access to PlayerWork pointer per side
- `core/signatures.rs`: RTTI walk infrastructure for Customize offset detection

**Data storage**:

- `mod-config.json` under key `"webui_options"`: `{ "p1": { "appeal_board": 5, "background": 12, ... }, "p2": { ... } }` — values are Konami's actual asset IDs (not the mod's sequential indices), so selections remain stable across asset updates that fill gaps in the ID space

**Filesystem access**:

- `data/arc/custom/<category>/` — enumerated at init via `std::fs::read_dir` (or Win32 `FindFirstFileW` if needed for AVS path compat)
- `data_mods/*/` — also scanned to catch LayeredFS-added assets

**Configuration**:

- New: `mod-config.json["webui_options"]` — per-player selections
- Label textures: `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/seop_item_cust_<category>.png`

---

## Public Contracts (signatures only — NO implementations)

```
// Category definition table (compile-time constant)
struct CategoryDef {
    option_id: &'static str,        // e.g. "cust_appeal_board"
    display_name: &'static str,     // for label texture generation
    scan_dir: &'static str,         // e.g. "data/arc/custom/appeal_board"
    file_pattern_prefix: &'static str, // e.g. "appeal_board_"
    customize_field_offset: u8,     // offset from Customize base (0x0C, 0x10, etc.)
}

// Discovery result per category
struct DiscoveredCategory {
    def: &'static CategoryDef,
    asset_ids: Vec<u32>,            // sorted, non-contiguous actual IDs found on disk
}

// The on_change callback signature matches custom_options::OnChangeFn
fn on_change_callback(player_side: u8, new_value: i32)
    // Maps sequential value → asset_ids[value], writes to Customize field
    // Persists the real asset ID (not sequential index) to mod-config.json
```

---

## Changes to Existing Code

### `src/mods/mod.rs`

- **Change**: Add `pub mod webui_options;` declaration.
- **Reason**: Register the new mod module.
- **Impact**: None — purely additive.

### `src/lib.rs`

- **Change**: Add `WebUiOptionsMod` to the mod registration block.
- **Reason**: Make the mod available in the mod registry.
- **Impact**: None — follows existing pattern.

### `src/core/signatures.rs`

- **Change**: Add a new signature or RTTI-walk helper for deriving the Customize offset from PlayerWork. May extend `derive_player_work_table` or add a sibling `derive_customize_offset`.
- **Reason**: Version-agnostic detection of the Customize base offset.
- **Impact**: Adds a new derived address; no change to existing signatures.

---

## File Changes Summary

| File | Action | Purpose |
|------|--------|---------|
| `src/mods/webui_options.rs` (or `src/mods/webui_options/mod.rs`) | new | Mod implementation: lifecycle, option registration, on_change, Customize writes |
| `src/mods/webui_options/discovery.rs` | new | Filesystem scanning and ID discovery per category |
| `src/mods/mod.rs` | modified | Add `pub mod webui_options;` |
| `src/lib.rs` | modified | Register `WebUiOptionsMod` in mod registry |
| `src/core/signatures.rs` | modified | Add RTTI walk / derived address for Customize offset |
| `scripts/gen_webui_option_labels.py` | new | Generate row-label PNGs for the 9 option rows |
| `data_mods/custom_options/.../tex/seop_item_cust_*.png` | new | Generated label textures (output of script) |

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Customize offset detection fails on a future version (RTTI walk doesn't find the vtable write) | H — mod doesn't function | L — RTTI string is stable | Graceful degradation: log warning, skip all customization option registration. Fallback: add a second detection method (e.g., signature on the dispatch function). |
| Writing an invalid asset ID causes visual glitch (blank texture) | L — cosmetic only | M — possible if assets are partially shipped | The mod only writes IDs discovered on disk. If the game can't load the ARC, it falls back to a default appearance (observed behavior). |
| Filesystem scan misses LayeredFS-added assets in `data_mods/` | M — custom assets not selectable | L — scan both paths | Scan `data_mods/*/data/arc/custom/<category>/` in addition to the base game path. |
| Future game update adds a new customize category or changes field layout | M — new category unsupported | L — fields have been stable across 2 known versions | Table-driven design means adding a category is one new `CategoryDef` entry. Field layout changes would be caught by the RTTI offset detection. |
| JSON fallback and network values conflict on load | L — wrong value shown briefly | L — on_change fires for network after JSON | Network load fires `resolve_from_load` which calls on_change, overwriting any JSON-primed value. The last write wins, which is network (correct priority). |
| Asset update fills ID gaps, changing sequential indices | L — cosmetic mismatch | M — Konami ships new assets regularly | JSON persists real asset IDs, not sequential indices. On load, the mod reverse-maps the stored asset ID to the current sequential index for the UI. If the ID no longer exists on disk, fall back to default (index 0). |

---

## Open Questions

None — all technical questions are resolved by the RE research document (`docs/player_customization_system_research.md`).
