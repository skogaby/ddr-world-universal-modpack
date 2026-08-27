# Design: Config Consolidation

**Requirements**: [requirements.md](requirements.md)
**Parent SIM**: N/A

---

## Overview

Consolidate all mod configuration into a single `mod-config.json` file by centralizing config reading in `config.rs` and migrating series expansion and folder expansion away from standalone JSON files. This eliminates duplicate file reads, establishes a single config loading pattern for all current and future consumers, and removes `series-expansion.json` and `folder-expansion.json` from the project.

---

## Architecture Decisions

### Decision 1: Centralized Config Store vs Per-Consumer File Reading
**Problem**: Today, `mod-config.json` is read independently by three consumers (`config.rs` for mod enable/disable, `avs_layeredfs/mod.rs` for LayeredFS config, and soon series/folder expansion). Each does its own `fs::read_to_string` + `serde_json::from_str`. Adding two more consumers makes this four separate parses of the same file.

**Decision**: Centralize all config reading in `config.rs`. Parse `mod-config.json` once into a `serde_json::Value`, store it, and let consumers extract their section via a generic accessor.

**Rationale**: 
- Single file read instead of four — simpler to reason about, easier to debug config issues
- Establishes a clear pattern for future mods: add your config struct, read it from the store
- The coupling already exists implicitly (everyone reads the same file) — this makes it explicit
- The save path stays narrow: only the `"mods"` key is ever written back (by mod_menu toggle)

**Alternatives Considered**:
- *Per-consumer reading (LayeredFS pattern)*: Each mod reads `mod-config.json` and extracts its own key. Simpler per-consumer, but duplicates file I/O and parsing. Doesn't scale — every new config section adds another full file parse. Rejected because it's the pattern we're trying to clean up.

**Tradeoffs**: `config.rs` becomes a dependency for LayeredFS (a service), not just mods. This is acceptable because the file is already shared — we're making implicit coupling explicit.

### Decision 2: Fully-Typed Top-Level Struct vs Generic `serde_json::Value` Store
**Problem**: How to represent the parsed config internally — a typed struct with all known sections, or a generic JSON value that consumers deserialize on demand?

**Decision**: Use a fully-typed `ConfigFile` struct with an `Option<T>` field per config section.

**Rationale**:
- Compile-time safety: typos in field access are caught by the compiler, not silently returning `None` at runtime
- One-shot deserialization: all sections parsed upfront in `init()` — malformed JSON for any section is caught immediately with a clear error
- Cleaner consumer ergonomics: `config::get().series_expansion.as_ref()` with IDE autocomplete vs `config::get_section::<SeriesConfig>("series_expansion")`
- The coupling cost is low: one `use` import per config type in `config.rs`. New config sections are added infrequently (once every few features), and there are only 4 sections total
- Single developer, all mods owned — no plugin-system extensibility needed

**Alternatives Considered**:
- *Generic `serde_json::Value` store with `get_section::<T>(key)`*: No changes to `config.rs` when adding sections, but loses compile-time key checking, defers deserialization errors to first access, and string-keyed access is error-prone. Better suited for plugin systems where config types aren't known at compile time. Rejected — overkill for this project's size and ownership model.

**Tradeoffs**: Adding a new config section requires adding a field + import to `config.rs`. Acceptable given the low frequency of new sections and the compile-time safety gained.

### Decision 3: Config Store Lifecycle — Static Singleton vs Passed Reference
**Problem**: How do consumers access the centralized config? The store needs to be available to both services (LayeredFS, initialized early) and mods (initialized later).

**Decision**: Use the existing `once_cell::Lazy<Mutex<>>` singleton pattern, consistent with every other service in the codebase. Initialize once in `lib.rs` init sequence, before LayeredFS and mod registration.

**Rationale**: Follows the established singleton pattern (`SCENE_MANAGER`, `INPUT_MANAGER`, etc.). Consumers call `config::get_section::<T>("key")` — same ergonomics as `scene_manager::is_available()`.

**Alternatives Considered**:
- *Pass config through `ModContext`*: Would require changing the `Mod` trait and `ModContext` struct. More invasive, and doesn't help LayeredFS (which isn't a mod).

**Tradeoffs**: None significant — this is the standard pattern in the codebase.

---

## Component Design

### New/Modified Components
| Component | Location | Responsibility | Replaces/Extends |
|-----------|----------|----------------|------------------|
| `ModConfigStore` (expanded) | `src/mods/config.rs` | Read `mod-config.json` once, deserialize into typed `ConfigFile` struct, provide `get()` accessor and `save_mod_states()` | Current `ModConfigStore` (load/save of `"mods"` only) |

### Modified Components
| Component | Location | Change |
|-----------|----------|--------|
| `SeriesExpansionMod::load_config()` | `src/mods/series_expansion.rs` | Read from `config::get().series_expansion` instead of `series-expansion.json` |
| `FolderExpansionMod::load_config()` | `src/mods/folder_expansion.rs` | Read from `config::get().folder_expansion` instead of `folder-expansion.json` |
| `avs_layeredfs::load_config()` | `src/services/avs_layeredfs/mod.rs` | Read from `config::get().layeredfs` instead of parsing `mod-config.json` directly |
| `lib.rs` init sequence | `src/lib.rs` | Call `config::init()` early in the sequence (before LayeredFS init) |

### Removed Components
| Component | Reason |
|-----------|--------|
| `series-expansion.json` | Config moved to `"series_expansion"` key in `mod-config.json` |
| `folder-expansion.json` | Config moved to `"folder_expansion"` key in `mod-config.json` |

### Component Interactions
```
lib.rs init
  → config::init()                          // reads mod-config.json once, deserializes ConfigFile
  → avs_layeredfs::init()
      → config::get().layeredfs
  → mod registration
      → SeriesExpansionMod::load_config()
          → config::get().series_expansion
      → FolderExpansionMod::load_config()
          → config::get().folder_expansion
  → config::get().mods                      // replaces current ModConfigStore::load()
  → mod_menu toggle
      → config::save_mod_states()           // writes only "mods" key back (unchanged)
```

---

## Integration Points

**Data Storage**:
- File: `mod-config.json` (read once at init, `"mods"` key written on mod toggle — unchanged)

**Configuration**:
- Removed: `series-expansion.json`, `folder-expansion.json`
- New keys in `mod-config.json`: `"series_expansion"`, `"folder_expansion"` (same schema as the standalone files, just nested under a key)

---

## Public Contracts (Signatures Only)

```rust
// config.rs — expanded public API

/// The top-level config file shape. All fields optional — missing keys use defaults.
struct ConfigFile {
    mods: HashMap<String, bool>,
    layeredfs: Option<LayeredFsConfig>,
    series_expansion: Option<SeriesConfig>,
    folder_expansion: Option<FolderConfig>,
}

/// Initialize the config store. Call once, early in init sequence.
fn init();

/// Check if the config store has been initialized.
fn is_available() -> bool;

/// Get a reference to the parsed config. Returns None if init() hasn't been called.
fn get() -> Option<&'static ConfigFile>;

/// Save mod enable/disable states back to the file. Only writes the "mods" key.
/// Preserves all other top-level keys in the file unchanged.
fn save_mod_states(states: &HashMap<String, bool>);
```

---

## Changes to Existing Code

### `config.rs`
- **Change**: Expand from a stateless `ConfigFile { mods }` deserializer to a singleton that stores a fully-typed `ConfigFile` with all config sections. Add `init()`, `get()`, `save_mod_states()`. The `ConfigFile` struct has `Option<T>` fields for `layeredfs`, `series_expansion`, `folder_expansion`, and a `HashMap<String, bool>` for `mods`. Uses `#[serde(default)]` on all fields so missing keys deserialize to `None`/empty.
- **Reason**: Centralized config reading for all consumers with compile-time type safety.
- **Impact**: `lib.rs` and `mod_menu.rs` update their call sites (`load()` → `get().mods`, `save()` → `save_mod_states()`). New imports for `LayeredFsConfig`, `SeriesConfig`, `FolderConfig`.

### `series_expansion.rs`
- **Change**: `load_config()` reads from `config::get().series_expansion.clone()` instead of reading `series-expansion.json`. Remove `CONFIG_FILENAME` constant.
- **Reason**: Config now lives in `mod-config.json`.
- **Impact**: No behavior change — same struct, same graceful disable on missing config.

### `folder_expansion.rs`
- **Change**: `load_config()` reads from `config::get().folder_expansion.clone()` instead of reading `folder-expansion.json`. Remove `CONFIG_FILENAME` constant.
- **Reason**: Config now lives in `mod-config.json`.
- **Impact**: No behavior change — same struct, same validation, same graceful disable.

### `avs_layeredfs/mod.rs`
- **Change**: `load_config()` reads from `config::get().layeredfs.clone().unwrap_or_default()` instead of reading and parsing `mod-config.json` directly.
- **Reason**: Eliminates duplicate file read.
- **Impact**: No behavior change. Requires `config::init()` to be called before `avs_layeredfs::init()` in `lib.rs`.

### `lib.rs`
- **Change**: Add `config::init()` call early in the init sequence (before LayeredFS). Replace `ModConfigStore::load()` with `config::get().mods.clone()`.
- **Reason**: Config store must be available before any consumer initializes.
- **Impact**: Init sequence gains one new step. No behavior change.

### `mod_menu.rs`
- **Change**: Update `ModConfigStore::save()` → `config::save_mod_states()`.
- **Reason**: Renamed method.
- **Impact**: No behavior change.

### `save_mod_states` — Preserve Other Keys
- **Critical detail**: The current `save()` writes a `ConfigFile { mods }` struct, which would clobber `layeredfs`, `series_expansion`, and `folder_expansion` keys. The new `save_mod_states()` must read the existing file (or use the stored `Value`), update only the `"mods"` key, and write the full object back. This preserves user-edited config sections.

---

## Target `mod-config.json` Schema

```json
{
  "mods": {
    "hello-world": false,
    "fast-bootup": true,
    "series-expansion": true,
    "folder-expansion": true
  },
  "layeredfs": {
    "verbose": false,
    "developer_mode": false,
    "mod_folder": "./data_mods"
  },
  "series_expansion": {
    "custom_series": [
      { "series_value": 30, "label": "WORLD RUBY", "texture_name": "world_ruby" },
      { "series_value": 31, "label": "WORLD SAPPHIRE", "texture_name": "world_sapphire" }
    ],
    "arc_path": "data/arc/bm2d/custom_series.arc"
  },
  "folder_expansion": {
    "custom_folders": [
      { "bit_index": 11, "key": "dogs" }
    ],
    "hide_difficulty_pane": true
  }
}
```

---

## Documentation Updates

- **README.md**: Update "Custom Series" and "Custom Folders" sections to reference `mod-config.json` keys instead of standalone files. Add a complete example `mod-config.json` showing all sections.
- **Steering files**: Update `product.md` (business rules about config), `tech.md` (config loading pattern), `structure.md` (remove standalone file references, document centralized config pattern).

---

## Deployment Sequence

1. Build the updated DLL
2. Create the consolidated `mod-config.json` on the target machine (merge contents of the three files)
3. Deploy the new DLL
4. Delete `series-expansion.json` and `folder-expansion.json` from the game directory

**Rollback**: Revert to previous DLL. The standalone JSON files can be restored from the old config. The consolidated `mod-config.json` is backward-compatible with the old DLL (it ignores unknown keys).

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| `save_mod_states` clobbers non-mods keys if implemented incorrectly | High — user loses layeredfs/series/folder config | Medium | Read-modify-write: load existing file, update only `"mods"`, write back. Test by toggling a mod and verifying other keys survive. |
| Init ordering — LayeredFS calls `config::get_section` before `config::init()` | High — LayeredFS gets `None` config, uses defaults | Low | `config::init()` is placed before LayeredFS in `lib.rs` init sequence. `get_section` returns `None` if store not initialized (same as missing key — graceful default). |
| User has old standalone files and new DLL — confusion about which config is read | Low — single user, clean break per requirements | Low | Log a warning if standalone files still exist on disk (optional, nice-to-have). |

---

## Open Questions

None — all clarified in requirements.
