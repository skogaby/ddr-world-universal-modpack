# Requirements: Config Consolidation

## Overview
Consolidate all mod configuration into a single `mod-config.json` file. Currently, `series-expansion.json` and `folder-expansion.json` are standalone config files — these should be removed and their contents moved into `mod-config.json` as top-level keys, following the existing pattern established by the `layeredfs` section.

## User Stories

### US-1: Unified config file
**As a** modpack maintainer
**I want** all mod configuration in a single `mod-config.json` file
**So that** there's one canonical place to configure the modpack, and new mods follow a consistent pattern

**Acceptance Criteria:**
- [ ] `series-expansion.json` is no longer read or referenced anywhere in the codebase
- [ ] `folder-expansion.json` is no longer read or referenced anywhere in the codebase
- [ ] SeriesExpansionMod reads its config from the `"series_expansion"` key in `mod-config.json`
- [ ] FolderExpansionMod reads its config from the `"folder_expansion"` key in `mod-config.json`
- [ ] When the relevant key is missing from `mod-config.json`, the mod disables itself gracefully (same behavior as today when the standalone file is missing)
- [ ] When `mod-config.json` itself is missing, both mods disable gracefully (no crash, no panic)

### US-2: Centralized config loading
**As a** mod developer
**I want** a shared config loading mechanism in `mod-config.json`
**So that** new mods add config fields to the same file instead of creating new standalone files

**Acceptance Criteria:**
- [ ] The config loading pattern is consistent across LayeredFS, series expansion, and folder expansion (all read from top-level keys in `mod-config.json`)
- [ ] The `config.rs` module provides a way for mods to read their config section from `mod-config.json`, or each mod reads the file and extracts its own key (either approach is acceptable as long as it's consistent)

### US-3: Documentation updated
**As a** user setting up the modpack
**I want** accurate documentation reflecting the new config structure
**So that** I know to put all configuration in `mod-config.json`

**Acceptance Criteria:**
- [ ] README.md "Custom Series" section updated — instructions reference `mod-config.json` with a `"series_expansion"` key instead of `series-expansion.json`
- [ ] README.md "Custom Folders" section updated — instructions reference `mod-config.json` with a `"folder_expansion"` key instead of `folder-expansion.json`
- [ ] README.md shows a complete example `mod-config.json` with all sections (`mods`, `layeredfs`, `series_expansion`, `folder_expansion`)
- [ ] Steering files (`product.md`, `tech.md`, `structure.md`) updated to reflect the single-config-file pattern

## Out of Scope
- Changing the config schema for series expansion or folder expansion (field names, types, nesting stay the same — only the file location changes)
- Adding config validation or schema enforcement beyond what exists today
- Migrating old config files automatically (no migration tool — clean break)
- Changing the `mods` or `layeredfs` sections of `mod-config.json`

## Open Questions
- None — all clarified.

## Dependencies
- None — purely internal refactoring.

## Assumptions
- This is a private codebase with a single user/tester, so backward compatibility is not required.
- The existing config schemas for series expansion and folder expansion are correct and don't need changes — only the file they're read from changes.
