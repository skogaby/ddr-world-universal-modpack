# Requirements: 20260408-folder-expansion

## Overview
Add a new `FolderExpansion` mod that enables users to define custom genre folders in DDR World's folder selection UI. Custom folders are configured via a JSON file (`folder-expansion.json`) and rendered natively by the game using the same FolderProperty system as vanilla folders. The mod also unlocks all difficulty levels for all folders (vanilla and custom) as a QOL improvement.

## User Stories

### US-1: Define custom genre folders via JSON config
**As a** DDR World modder
**I want** to define new genre folders in a `folder-expansion.json` config file
**So that** I can organize songs into custom categories without recompiling the hook DLL

**Acceptance Criteria:**
- [ ] Mod reads `folder-expansion.json` from the game directory at init
- [ ] Each entry in `custom_folders` array creates a new folder in the game's folder carousel
- [ ] Config fields per folder: `bit_index` (u32), `key` (string), `label` (string), `voice_key` (string, optional), `max_difficulty` (0-4, optional, defaults to 4)
- [ ] Songs with the corresponding bit set in their `<property>` bitmask appear in the custom folder
- [ ] If the config file is missing or empty, the mod disables itself gracefully (no crash, log warning)
- [ ] If the config file has parse errors, the mod disables itself and logs the error

### US-2: Custom folders appear in the folder carousel
**As a** DDR World player
**I want** custom folders to appear in the game's folder selection carousel alongside vanilla folders
**So that** I can browse and select them like any built-in folder

**Acceptance Criteria:**
- [ ] Custom folders appear after the last vanilla genre folder but before ALL MUSIC
- [ ] Each custom folder displays its configured texture (`folder_{key}`, `mufo_folder_base_{key}`)
- [ ] Selecting a custom folder filters the song list to songs matching that folder's `bit_index`
- [ ] The folder confirmation popup shows the correct difficulty restriction based on `max_difficulty`
- [ ] Custom folders with zero matching songs still appear (has-songs predicate hooked to return true for configured indices)

### US-3: Unlock all difficulty levels for all folders
**As a** DDR World player
**I want** all folders (vanilla and custom) to allow selection of all difficulty levels
**So that** I'm not restricted to Beginner/Basic when browsing genre folders

**Acceptance Criteria:**
- [ ] All 6 vanilla genre folders are patched to max_difficulty=4 (Challenge) when the mod is enabled
- [ ] This includes FIRST STEP (which defaults to Beginner-only in vanilla)
- [ ] Custom folders respect their configured `max_difficulty` value (default 4 if omitted)
- [ ] When the mod is disabled, vanilla folders revert to their original difficulty restrictions

### US-4: Custom folder textures loaded from ARC
**As a** DDR World modder
**I want** to provide custom folder textures in an ARC file
**So that** the game renders my folder banners and info text natively

**Acceptance Criteria:**
- [ ] Config supports an `arc_path` field (string) specifying the path to a custom ARC file
- [ ] The ARC is loaded via the existing `asset_loader::register_arc` service
- [ ] Textures follow the game's naming convention: `folder_{key}`, `mufo_folder_base_{key}`, `mufo_txt_folder_info_{key}`
- [ ] If `arc_path` is omitted or the ARC fails to load, the mod still initializes (folders may render without textures)

### US-5: Voice-over support for custom folders
**As a** DDR World modder
**I want** to optionally specify a voice-over key for custom folders
**So that** the game can play a voice clip when the folder is selected

**Acceptance Criteria:**
- [ ] If `voice_key` is provided in config, it is written to the FolderProperty's voice string field
- [ ] If `voice_key` is omitted or empty, the voice string is set to empty (silent selection)
- [ ] The game does not crash when a folder has an empty voice key

## Out of Scope
- Carousel scroll modifications (the game's existing carousel handles additional folders natively)
- Creating new texture authoring tooling (existing `build_ddr_package` workflow is used)
- Shipping pre-configured folders for bits 6-9 (config ships empty; users define their own starting at index 10+)
- Hiding or removing vanilla folders
- Modifying the song measurement / count arrays (the has-songs predicate hook bypasses this)
- Voice-over audio file creation or bundling
- Folder display order customization beyond "after vanilla, before ALL MUSIC"

## Open Questions
1. **FIRST STEP patch encoding** — The FIRST STEP difficulty site uses `MOV [RDI+0xc0], R13D` (7 bytes) instead of the 10-byte `MOV dword [RDI+0xc0], imm32` used by the other 5 folders. The implementation will need to handle this differently (code cave, NOP padding, or constructor hook). This is a PE-level detail for the design phase.
2. **Bit index 8 special case** — The research notes a quirk where bit index 8 also matches if bit 6 is set (`TEST R8B, 0x40`). Custom folders using index 8 may inherit unexpected songs. Users should be advised to prefer indices ≥ 10.
3. **Folder type IDs for custom folders** — Vanilla folders use type IDs 1-7. Custom folders need unique IDs (e.g., 0x10+). The exact numbering scheme is a design decision.

## Dependencies
- `core/memory.rs` — Memory allocation near game module, read/write/protect
- `core/signatures.rs` — AOB signature scanning for patch targets
- `services/asset_loader.rs` — ARC file registration
- `services/afp_patcher.rs` — Potentially needed if AFP template patching is required for folder UI
- `mods/mod_trait.rs` — Mod trait implementation, ModContext, ModRegistry
- Existing `build_ddr_package` script for creating ARC texture packs

## Assumptions
- The game's folder carousel natively supports more than 7 entries without UI modifications
- The property bitmask test in `FUN_1801444f0` supports indices up to 31 (confirmed in research: `SHL EDX, CL` with x86 shift masking)
- The FolderProperty constructor (`FUN_180140b60`) and registration function (`FUN_180143db0`) can be called for custom entries after vanilla init completes
- Empty voice key strings do not cause crashes (the game likely checks string length before attempting playback)
- The `folder-expansion.json` config format follows the pattern established by `series-expansion.json`
