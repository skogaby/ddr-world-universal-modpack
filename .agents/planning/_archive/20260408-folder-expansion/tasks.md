# Tasks: 20260408-folder-expansion

## Task Breakdown

- [x] **Task 1: Signatures + Config + Mod Skeleton**
  - **Goal**: Ship a `FolderExpansionMod` that loads config, resolves all 7 signatures, and registers with ModRegistry. No hooks yet.
  - **Scope**:
    - Add 7 new AOB signatures to `signatures.rs` (folder_init, folder_property_ctor, folder_functor_ctor, folder_filter_functor_ctor, folder_store_ptr, folder_register, folder_has_songs)
    - Create `src/mods/folder_expansion.rs` with `FolderExpansionMod` implementing `Mod` trait
    - Implement `FolderConfig` / `CustomFolderEntry` serde structs
    - Config loading from `folder-expansion.json` with graceful disable on missing/invalid
    - Validate config constraints (key length ≤ 15, bit_index range, etc.)
    - Register mod in `mod.rs` and `lib.rs`
  - **Tests**: Build passes. Mod appears in mod menu. Config loads and validates. Missing config disables mod gracefully.
  - **Dependencies**: None

- [x] **Task 2: Difficulty Unlock Patches**
  - **Goal**: All 6 vanilla folder difficulty restriction writes are NOPed when mod is enabled, restored on disable.
  - **Scope**:
    - Scan for 6 difficulty write sites within folder_init's address range (5x 10-byte `C7 87 C0 00 00 00` + 1x 7-byte `44 89 AF C0 00 00 00`)
    - Apply NOP patches via SavedPatch pattern on enable
    - Restore original bytes on disable
    - Respect `unlock_all_difficulties` config flag (skip patches if false)
    - Log warning if expected site count doesn't match
  - **Tests**: Build passes. With mod enabled, all vanilla folders allow Challenge difficulty. With mod disabled, original restrictions restored.
  - **Dependencies**: Task 1 (mod skeleton + folder_init signature)

- [x] **Task 3: Custom Folder Creation + Registration Hook**
  - **Goal**: Custom folders appear in the carousel after vanilla genre folders, before ALL MUSIC.
  - **Scope**:
    - Hook `folder_register` via retour static detour
    - Detect ALL MUSIC registration (type_id == 7 at offset +0x00)
    - For each config entry: call folder_property_ctor, set fields (type_id, max_difficulty, key, voice_key, mode_flag), call functor constructors, call folder_store_ptr, call folder_register
    - Capture the context pointer (RCX) from the hook for use in custom folder registration
    - Type IDs start at 0x10, incrementing per entry
    - Load custom ARC via asset_loader::register_arc if arc_path configured
  - **Tests**: Build passes. Custom folder(s) visible in carousel. Selecting a custom folder filters to songs with matching property bit.
  - **Dependencies**: Task 2 (difficulty patches working, all signatures resolved)

- [x] **Task 4: Has-Songs Predicate Hook + Polish**
  - **Goal**: Custom folders always show as having songs. End-to-end feature complete.
  - **Scope**:
    - Hook `folder_has_songs` via retour static detour
    - Call original — if true, pass through
    - If false, read bit_index from functor struct, check against configured custom folder indices, return true if match
    - Verify filter count updates correctly when custom folder selected
    - Verify custom folders combine with other filters
    - Update README.md with folder expansion documentation
    - Update steering files (product.md, structure.md) to reflect new mod
  - **Tests**: Build passes. Custom folders with zero songs still appear. Song filtering works correctly. Multiple custom folders work independently.
  - **Dependencies**: Task 3 (folders must exist in carousel first)
