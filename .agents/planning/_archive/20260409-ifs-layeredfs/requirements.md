# Requirements: IFS LayeredFS Port

## Overview
Port the entire [ifs_layeredfs](https://github.com/mon/ifs_layeredfs) project from C++ into the Rust modpack as a native service + mod. ifs_layeredfs hooks Konami's AVS filesystem layer (`libavs-win64.dll`) to intercept file accesses at runtime, allowing transparent replacement and injection of game assets from a `data_mods/` folder without repacking container files. The port brings this capability into the existing hook DLL, eliminating the need to run ifs_layeredfs as a separate DLL.

### Source Reference
The C++ source lives at `~/Desktop/ifs_layeredfs/`. Key files:
- `src/avs.cpp` / `avs.h` — AVS DLL export resolution, function hooking, property/XML helpers
- `src/hook.cpp` / `hook.h` — Core file interception dispatch (`handle_file_open`), HookFile abstraction
- `src/modpath_handler.cpp` — Mod folder scanning, path normalization, file lookup
- `src/ramfs_demangler.cpp` — Tracks file→RAM→mount chain for RAM-loaded IFS files
- `src/imagefs.cpp` — IFS texture parsing (texturelist.xml), PNG→game format conversion, AFP/geo MD5 mapping, XML merging
- `src/texbin.cpp` — Texbin (.bin) texture container parsing and repacking (Gitadora/Jubeat format)
- `src/config.cpp` — Command-line flag parsing
- `src/texture_packer.cpp` — Bin-packing for injecting new textures into IFS atlases

### Immediate Motivation
The folder-expansion feature (20260408) needs AVS hooks to serve modified GE2D geo files so custom folder cards render with the correct textures. See `.spec/workflow/20260408-folder-expansion/afp-texture-handoff.md` for the full analysis. But beyond that immediate need, the full port enables the modpack to transparently replace any game asset — textures, XML configs, AFP animations, geo shapes — from loose files on disk.

## User Stories

### US-1: AVS Function Resolution
**As a** modpack developer
**I want** the hook DLL to locate and resolve AVS filesystem function exports from `libavs-win64.dll`
**So that** the layeredfs system can hook into the game's file I/O layer

**Acceptance Criteria:**
- [ ] Resolves AVS DLL handle (`libavs-win64.dll`, `libavs-win32.dll`, or `avs2-core.dll` — whichever is loaded)
- [ ] Supports multiple AVS export naming schemes (plain names for ≤2.12, obfuscated `XC*` names for 2.13+) by iterating the export table definitions from `avs.cpp`
- [ ] Resolves at minimum: `avs_fs_open`, `avs_fs_close`, `avs_fs_read`, `avs_fs_lstat`, `avs_fs_fstat`, `avs_fs_lseek`, `avs_fs_mount`, `avs_fs_convert_path`
- [ ] Resolves AVS logging functions (`log_body_fatal`, `log_body_warning`, `log_body_info`, `log_body_misc`) for optional log routing
- [ ] Resolves property/compression functions needed for binary XML handling: `property_read_query_memsize`, `property_read_query_memsize_long`, `property_create`, `property_insert_read`, `property_mem_write`, `property_destroy`, `property_query_size`, `cstream_create`, `cstream_operate`, `cstream_finish`, `cstream_destroy`
- [ ] Detects AVS version and stores it for version-dependent behavior (e.g., `avs_fs_open` mode flag differs between old/new AVS)
- [ ] Gracefully degrades if AVS DLL is not found (logs warning, disables layeredfs)

### US-2: Core File Interception Hooks
**As a** modpack developer
**I want** AVS filesystem functions to be hooked so file accesses can be intercepted
**So that** mod files can transparently replace game files at runtime

**Acceptance Criteria:**
- [ ] Hooks `avs_fs_open` — intercepts file opens, checks for mod replacements, calls original with replacement path if found
- [ ] Hooks `avs_fs_lstat` — intercepts stat calls so the game sees correct file sizes for replacement files
- [ ] Hooks `avs_fs_mount` — intercepts mount operations to track ramfs/imagefs mappings (feeds the demangler)
- [ ] Hooks `avs_fs_read` — intercepts reads to track buffer addresses for ramfs demangling
- [ ] Hooks `avs_fs_convert_path` — intercepts path conversion for mod file resolution
- [ ] Only intercepts read-mode opens (mode check differs by AVS version)
- [ ] Passes through unmodified files to the original AVS functions with zero overhead beyond the path check
- [ ] Thread-safe — multiple game threads may call AVS functions concurrently

### US-3: Mod Path System
**As a** game modder
**I want** to place replacement files in a `data_mods/` folder structure
**So that** I can mod game assets without touching the original game files

**Acceptance Criteria:**
- [ ] Scans `data_mods/` (configurable) for mod subfolders at init time
- [ ] Each subfolder in `data_mods/` is a separate mod (e.g., `data_mods/my_texture_mod/`)
- [ ] Normalizes game paths: strips `/data/` prefix, normalizes slashes, collapses double slashes
- [ ] Handles `data2/` paths (used by some games) — these keep their `data2/` prefix in the mod folder
- [ ] Supports IFS path expansion: `graphics/ver04/logo.ifs` → mod path `graphics/ver04/logo_ifs/` (`.ifs` → `_ifs`)
- [ ] Supports nested IFS expansion (IFS inside IFS)
- [ ] Case-insensitive file matching
- [ ] Mod priority: folders sorted case-insensitively, first match wins
- [ ] Caches mod folder contents at startup for fast lookup (non-dev mode)
- [ ] Dev mode: checks filesystem on every access instead of using cache (slower but allows hot-reload)
- [ ] Allowlist/blocklist support for selectively enabling/disabling mod folders

### US-4: RAM FS Demangler
**As a** modpack developer
**I want** the system to track when the game loads IFS files into RAM and mounts them as virtual filesystems
**So that** mods can replace files inside RAM-loaded IFS containers

**Acceptance Criteria:**
- [ ] Tracks `avs_fs_open` → file handle mapping for `.ifs` files
- [ ] Tracks `avs_fs_read` → buffer address mapping (associates RAM buffer with source file)
- [ ] Tracks `avs_fs_mount` with type `ramfs` — extracts `base=` pointer from flags, maps ramfs path to original IFS path
- [ ] Tracks `avs_fs_mount` with type `link` — follows link mounts to original paths
- [ ] Tracks `avs_fs_mount` with type `imagefs` — maps imagefs mount point to original IFS path via longest-prefix match
- [ ] `normalise_path` demangles virtual paths back to real IFS paths before mod lookup
- [ ] Cleans up stale mappings when an IFS file is re-opened (prevents memory leaks)
- [ ] Thread-safe — mount/open/read can happen from different threads

### US-5: IFS Texture Replacement
**As a** game modder
**I want** to replace textures inside IFS files by dropping PNG files in the mod folder
**So that** I can mod textures without repacking IFS containers

**Acceptance Criteria:**
- [ ] Parses `texturelist.xml` from IFS files to learn texture names, formats, dimensions, and compression
- [ ] Maps MD5-hashed texture filenames (used internally by IFS) back to human-readable names
- [ ] Converts replacement PNG files to the correct game format:
  - ARGB8888REV (RGBA → BGRA byte swap)
  - DXT5 (PNG → DXT5 compression with word-swapped endianness)
- [ ] Handles AVSLZ compression (compresses converted textures using AVS's cstream API)
- [ ] Caches converted textures to avoid re-conversion on subsequent loads
- [ ] Cache invalidation: re-converts when PNG timestamp changes, DLL timestamp changes, or cache hash mismatches
- [ ] Supports both `mod_folder/tex/name.png` and `mod_folder/name.png` paths for texture placement
- [ ] Validates replacement PNG dimensions match the original texture dimensions

### US-6: IFS Texture Injection
**As a** game modder
**I want** to add entirely new textures to IFS files (not just replace existing ones)
**So that** I can extend the game's texture set without modifying original files

**Acceptance Criteria:**
- [ ] Detects PNG files in mod folders that don't correspond to existing textures in the IFS
- [ ] Packs new textures into atlas canvases using a bin-packing algorithm (GuillotineBinPack)
- [ ] Generates modified `texturelist.xml` with new texture entries (canvas nodes, image rects, UV rects)
- [ ] New textures use ARGB8888REV format with nearest-neighbor filtering
- [ ] Writes modified texturelist.xml to cache folder and redirects the game to load it

### US-7: AFP and Geo File Replacement
**As a** game modder
**I want** to replace AFP animation files and GE2D geometry files inside IFS containers
**So that** I can mod animations and shape data without repacking

**Acceptance Criteria:**
- [ ] Parses `afplist.xml` from IFS files to learn AFP names and their associated geo shape IDs
- [ ] Maps MD5-hashed AFP/geo filenames back to human-readable names (e.g., `folder_firststep_shape41`)
- [ ] Supports replacement of AFP files (`afp/` subfolder), BSI files (`afp/bsi/` subfolder), and geo files (`geo/` subfolder)
- [ ] Mod files are looked up by human-readable name in the mod folder, served when the game requests the MD5-hashed name

### US-8: XML Merging
**As a** game modder
**I want** to append content to game XML files (e.g., musicdb.xml) without replacing the entire file
**So that** multiple mods can add content to the same XML file

**Acceptance Criteria:**
- [ ] Detects `.merged.xml` files in mod folders (e.g., `music_db.merged.xml` merges into `music_db.xml`)
- [ ] Loads the original XML (handles both binary property format and plain XML)
- [ ] Appends child nodes from each merged XML into the original's root node
- [ ] Supports multiple mods merging into the same file (all `.merged.xml` files are applied in mod priority order)
- [ ] Caches merged output with hash-based invalidation
- [ ] Works for XML files both at the top level and inside IFS containers

### US-9: Texbin Support
**As a** modpack developer
**I want** the system to handle `.bin` texture containers (texbin format used by Gitadora/Jubeat)
**So that** the layeredfs port is feature-complete with the original

**Acceptance Criteria:**
- [ ] Parses PXET-format texbin files (header, name table, data entries, rect entries)
- [ ] Replaces existing textures by name (loads PNG, converts to ARGB8888 TXDT format, LZ77 compresses)
- [ ] Adds new textures to texbin files
- [ ] Handles rect entries (sub-image regions within a parent texture)
- [ ] Supports LZ77 compression/decompression (the texbin-specific variant, not AVSLZ)
- [ ] Caches repacked texbin files with hash-based invalidation
- [ ] Validates replacement image dimensions match originals

### US-10: Configuration
**As a** game operator
**I want** to configure layeredfs behavior via mod-config.json
**So that** I can control verbosity, dev mode, and mod selection

**Acceptance Criteria:**
- [ ] Configuration lives in `mod-config.json` alongside existing mod settings
- [ ] Supports `verbose` (bool) — detailed logging of every file access
- [ ] Supports `developer_mode` (bool) — skip cache, check filesystem every access for hot-reload
- [ ] Supports `allowlist` (string array) — only load listed mod folders
- [ ] Supports `blocklist` (string array) — exclude listed mod folders
- [ ] Supports `mod_folder` (string) — custom mod folder path (default: `./data_mods`)
- [ ] Logs detected AVS version, loaded mod folders, and config at startup

### US-11: Integration with Existing Modpack
**As a** modpack developer
**I want** the layeredfs port to integrate cleanly with the existing hook DLL architecture
**So that** it coexists with existing mods and services without conflicts

**Acceptance Criteria:**
- [ ] Implemented as an always-on service (`src/services/avs_layeredfs.rs` or similar) following the existing singleton pattern
- [ ] Not toggleable via mod menu — it's infrastructure, not a user-facing mod
- [ ] Uses the existing `retour` hooking framework for AVS function hooks
- [ ] Uses the existing logging macros (`log_info!`, `log_warn!`, etc.)
- [ ] Initializes after module resolution but before mods that depend on it (e.g., folder_expansion)
- [ ] Does not conflict with existing hooks on `libafp-win64.dll` or `gamemdx.dll`
- [ ] AVS hooks are installed on the AVS DLL's own exports, not on IAT entries in gamemdx (avoids conflicts)

## Out of Scope
- **DLL injection mechanism** — The modpack is already loaded via spice2x's `-k` flag. The d3d9/opengl32/dxgi proxy DLL injection from ifs_layeredfs is not needed.
- **pkfs hooks** — DDR World does not use `libpackfs.dll`. The pkfs hooking code from ifs_layeredfs is not ported. Can be added later if needed for other games.
- **Windows XP compatibility** — The original ifs_layeredfs maintains XP support. The Rust modpack targets modern Windows only.
- **AVS standalone mode** — The `avs_standalone.cpp` test harness is not ported.
- **32-bit support** — DDR World is 64-bit only. No 32-bit build target.

## Resolved Questions
1. **Config mechanism**: Any layeredfs configuration goes in `mod-config.json`, consistent with the existing modpack pattern. No command-line flag parsing.
2. **Mod menu integration**: LayeredFS is an always-on service, not a toggleable mod. It underpins too many other mods to be meaningfully disabled at runtime. Implemented as a service in `src/services/`.
3. **Cache location**: Reuse `data_mods/_cache/`, matching the original ifs_layeredfs convention. Existing mod setups work without changes.
4. **Binary property handling**: Mimic ifs_layeredfs — call the AVS DLL's own property API functions (`property_create`, `property_insert_read`, `property_mem_write`, etc.) to convert binary XML to text XML. A pure-Rust kbin implementation exists in `~/Desktop/Projects/bemani-buddy/crates/bemani-core/src/kbin/` as a fallback reference if needed.
5. **Third-party library equivalents**: Deferred to PE during design phase. The original uses lodepng, stb_dxt, libsquish, rapidxml, and GuillotineBinPack — Rust equivalents need to be selected.

## Dependencies
- `libavs-win64.dll` must be loaded in the process before layeredfs initializes
- The existing `retour` hooking framework for installing AVS hooks
- The existing module resolution infrastructure (`core/module_resolver.rs`) to find the AVS DLL
- PNG decoding crate for texture conversion
- DXT compression crate for DXT5 texture format support
- XML parsing crate for texturelist.xml / afplist.xml / merged XML handling

## Assumptions
- DDR World uses AVS 2.17.x (`libavs-win64.dll` with `XCgsqzn0` export prefix), but the port supports all AVS versions from the original for forward compatibility
- The `data_mods/` folder convention from ifs_layeredfs is preserved — existing mod folder layouts work without changes
- The game's AVS DLL is loaded before the modpack's init sequence reaches the layeredfs service
- AVS function signatures (parameter types, calling convention) are stable across the versions defined in `avs.cpp`
