# Tasks: 20260415-config-consolidation

Tasks are sized to be CR-ready: one shippable unit that builds independently, roughly ~1 day of focused work.

## Workspace Info
**Primary Package**: ddr-world-hook
**All Packages**: ddr-world-hook

---

## Task 1: Centralize config store and migrate all consumers
**Package(s)**: ddr-world-hook
**Goal**: All config reading goes through a single `config::init()` / `config::get()` pattern. Standalone config files are no longer referenced.
**Scope**: Expand `config.rs` with `ConfigFile` struct, `init()`, `get()`, `save_mod_states()`. Migrate `series_expansion.rs`, `folder_expansion.rs`, `avs_layeredfs/mod.rs` to read from the config store. Update `lib.rs` init sequence and `mod_menu.rs` save call. Remove `CONFIG_FILENAME` constants from individual mods.
**Tests**: Build succeeds. Mod toggle persists correctly (save_mod_states preserves other keys). Each mod disables gracefully when its config key is missing. No panics when `mod-config.json` is absent.
**Dependencies**: None

- [x] 1.1 Expand `config.rs`: add `ConfigFile` struct with `Option<T>` fields for all sections, `init()`, `get()`, `save_mod_states()` (read-modify-write to preserve non-mods keys)
- [x] 1.2 Migrate `avs_layeredfs/mod.rs` to read from `config::get().layeredfs`
- [x] 1.3 Migrate `series_expansion.rs` to read from `config::get().series_expansion`
- [x] 1.4 Migrate `folder_expansion.rs` to read from `config::get().folder_expansion`
- [x] 1.5 Update `lib.rs` init sequence: call `config::init()` before LayeredFS, replace `ModConfigStore::load()` with `config::get()`
- [x] 1.6 Update `mod_menu.rs` to use `config::save_mod_states()`

---

## Task 2: Documentation and cleanup
**Package(s)**: ddr-world-hook
**Goal**: README and steering files reflect the single-config-file pattern. Standalone config files removed from the project.
**Scope**: Update README.md (Custom Series, Custom Folders sections, add complete mod-config.json example). Update steering files (product.md, tech.md, structure.md). Delete `series-expansion.json` and `folder-expansion.json` from the repo if present.
**Tests**: Build succeeds. Documentation accurately describes the new config structure.
**Dependencies**: Task 1

- [x] 2.1 Update README.md: Custom Series section references `mod-config.json` `"series_expansion"` key
- [x] 2.2 Update README.md: Custom Folders section references `mod-config.json` `"folder_expansion"` key
- [x] 2.3 Add complete example `mod-config.json` to README showing all sections
- [x] 2.4 Update steering files (product.md, tech.md, structure.md) to reflect centralized config pattern
- [x] 2.5 Remove standalone `series-expansion.json` and `folder-expansion.json` from repo

---

## QA Section
**Status**: Skipped
**Test Results**: Work completed outside workflow tracking; retroactively closed.
**Feedback**: 

## Acceptance Section
**PM**: accepted
**Status**: Complete
**Notes**: Retroactively closed on 2026-04-27. Task 2 work was completed but not tracked in the workflow at the time.
