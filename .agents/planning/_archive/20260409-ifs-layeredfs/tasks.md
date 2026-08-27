# Tasks: 20260409-ifs-layeredfs

Tasks are sized to be CR-ready: one shippable unit that builds independently, roughly ~1 day of focused work.

## Workspace Info
**Primary Package**: ddr-world-hook
**All Packages**: ddr-world-hook

---

## Task 1: Foundation — AVS Resolver + Service Skeleton
**Package(s)**: ddr-world-hook
**Goal**: Ship the `avs_layeredfs` service module with AVS DLL export resolution. The service initializes, finds `libavs-win64.dll`, resolves all function pointers, and reports success/failure. No hooks installed yet.
**Scope**:
- Add `png`, `texpresso`, `md5`, `quick-xml` dependencies to `Cargo.toml`
- Create `src/services/avs_layeredfs/mod.rs` — public `init()` / `is_available()`, config struct (`LayeredFsConfig`) loaded from `mod-config.json` `"layeredfs"` key
- Create `src/services/avs_layeredfs/avs_resolver.rs` — AVS DLL handle lookup (`libavs-win64.dll` / `libavs-win32.dll` / `avs2-core.dll`), 6 export table definitions (plain, 2.13, 2.15, 2.16.1, 2.16.3-7, 2.17), `GetProcAddress` iteration, store resolved function pointers in a struct
- Register module in `src/services/mod.rs`
- Call `avs_layeredfs::init()` in `src/lib.rs` init sequence (after module resolver, before widget renderer)
- Graceful degradation: if AVS DLL not found or exports don't match, log warning and return false
**Tests**: Build passes. Service logs detected AVS version at startup. `is_available()` returns true when AVS DLL is present.
**Dependencies**: None

- [x] 1.1 Add crate dependencies to Cargo.toml
- [x] 1.2 Create avs_resolver.rs with export tables and resolution logic
- [x] 1.3 Create mod.rs with init/is_available/config
- [x] 1.4 Wire into services/mod.rs and lib.rs

---

## Task 2: Mod Path System
**Package(s)**: ddr-world-hook
**Goal**: Ship mod folder scanning, path normalization, and file lookup. The system discovers mods in `data_mods/`, caches their contents, and can resolve game paths to mod file paths.
**Scope**:
- Create `src/services/avs_layeredfs/mod_paths.rs`
- `init_mod_paths()` — scan `data_mods/` (configurable) for subfolders, apply allowlist/blocklist, sort case-insensitively
- `cache_mods()` — recursively walk each mod folder, store contents in a `Vec<ModContents>` (name + BTreeSet of relative paths)
- `normalise_path()` — strip `/data/` prefix, normalize slashes (`\\` → `/`), collapse `//`, apply ramfs demangling (stub for now)
- `find_first_modfile()` / `find_first_modfolder()` — iterate mods in priority order, return first match (cached or live filesystem check in dev mode)
- `find_all_modfile()` — return all matches across mods (needed for XML merging)
- `available_mods()` — return list of active mod folder paths
- Case-insensitive matching throughout (use `eq_ignore_ascii_case` or `to_lowercase`)
**Tests**: Build passes. Mod folders discovered and logged at startup. Path normalization handles edge cases (backslashes, double slashes, `/data/` prefix stripping).
**Dependencies**: Task 1 (service skeleton + config for mod_folder/allowlist/blocklist/dev_mode)

- [x] 2.1 Create mod_paths.rs with folder scanning and caching
- [x] 2.2 Implement path normalization
- [x] 2.3 Implement file/folder lookup (cached + dev mode)
- [x] 2.4 Wire into mod.rs init sequence

---

## Task 3: RAM FS Demangler
**Package(s)**: ddr-world-hook
**Goal**: Ship the virtual path tracking system that maps RAM-loaded IFS mount points back to their original file paths, so mod lookups work for RAM-loaded IFS files.
**Scope**:
- Create `src/services/avs_layeredfs/ramfs_demangler.rs`
- `on_fs_open()` — track file handle → path mapping for `.ifs` files, clean up stale mappings on re-open
- `on_fs_read()` — track buffer address → path mapping (associates RAM buffer with source file)
- `on_fs_mount()` — handle `ramfs` type (extract `base=` pointer from flags, map ramfs path → original), `link` type (follow link to original), `imagefs` type (longest-prefix match from ramfs map → original)
- `demangle()` — given a virtual path, find longest matching mount prefix in `mangling_map`, replace prefix with original IFS path
- All state behind `Mutex` for thread safety
- `BTreeMap<String, String>` for ramfs_map and mangling_map (linear prefix scan per Decision 4)
- Handle IFS-inside-IFS: when imagefs mounts from a path that itself needs demangling, demangle the root before storing
**Tests**: Build passes. Demangler compiles and initializes. (Full integration testing happens in Task 4 when hooks are installed.)
**Dependencies**: Task 1 (service skeleton)

- [x] 3.1 Create ramfs_demangler.rs with state structs
- [x] 3.2 Implement on_fs_open, on_fs_read, on_fs_mount handlers
- [x] 3.3 Implement demangle with longest-prefix match
- [x] 3.4 Wire into mod.rs

---

## Task 4: File Hooks — AVS Interception + Basic File Replacement
**Package(s)**: ddr-world-hook
**Goal**: Ship the 5 retour static detours on AVS functions and the core `handle_file_open` dispatch. With this task, dropping a replacement file in `data_mods/my_mod/path/to/file` transparently replaces the game's file access. No texture conversion, no XML merging — just raw file replacement.
**Scope**:
- Create `src/services/avs_layeredfs/file_hooks.rs`
- 5 retour static detours: `hook_avs_fs_open`, `hook_avs_fs_lstat`, `hook_avs_fs_mount`, `hook_avs_fs_read`, `hook_avs_fs_convert_path`
- `hook_avs_fs_open` — skip null names, skip non-read mode, normalize path, check mod replacement, call original with replacement path if found
- `hook_avs_fs_lstat` — same flow as open (so game sees correct file sizes for replacements)
- `hook_avs_fs_mount` — feed ramfs_demangler, call original
- `hook_avs_fs_read` — feed ramfs_demangler, call original
- `hook_avs_fs_convert_path` — normalize, check mod replacement, call original
- IFS path expansion in mod lookup: `.ifs` → `_ifs` (iterative, for nested IFS)
- Wire demangler into normalise_path
- `install_hooks()` called from mod.rs init after AVS resolution and mod path scanning
- Thread-local `inside_pkfs_hook` equivalent not needed (pkfs excluded from scope)
**Tests**: Build passes. Hooks installed successfully (logged). Dropping a test file in `data_mods/test/` replaces the corresponding game file access (verified via verbose logging).
**Dependencies**: Task 2 (mod_paths), Task 3 (ramfs_demangler)

- [x] 4.1 Create file_hooks.rs with 5 static detour declarations
- [x] 4.2 Implement hook_avs_fs_open with handle_file_open dispatch
- [x] 4.3 Implement remaining 4 hooks (lstat, mount, read, convert_path)
- [x] 4.4 Wire hook installation into mod.rs init

---

## Task 5: XML Merging
**Package(s)**: ddr-world-hook
**Goal**: Ship `.merged.xml` support. Multiple mods can append content to the same game XML file (e.g., `music_db.xml`) without replacing the entire file.
**Scope**:
- Create `src/services/avs_layeredfs/xml_merger.rs`
- `merge_xmls()` — called from handle_file_open when path ends in `.xml`
- Detect `.merged.xml` files in mod folders (e.g., `music_db.merged.xml` merges into `music_db.xml`). Also check `_ifs` variant for XML inside IFS.
- Load original XML: check if binary prop format (first byte `0xA0`), if so use AVS `property_*` functions to convert to text XML, otherwise read as text
- Parse with `quick-xml`, append child nodes from each merged XML into original's last root node
- Write merged output to `_cache/` folder
- Cache with hash-based invalidation: hash input file timestamps + DLL timestamp + mod file timestamps (port `CacheHasher` pattern)
- Redirect file open to cached merged file
**Tests**: Build passes. A `.merged.xml` file in `data_mods/` is detected and merged into the original XML. Cache is created and reused on subsequent loads.
**Dependencies**: Task 4 (file_hooks dispatch calls merge_xmls)

- [x] 5.1 Create xml_merger.rs with merge_xmls entry point
- [x] 5.2 Implement binary prop detection and AVS property API conversion
- [x] 5.3 Implement XML parsing, node appending, and output writing
- [x] 5.4 Implement CacheHasher (MD5 of timestamps) and cache invalidation

---

## Task 6: IFS Texture Replacement
**Package(s)**: ddr-world-hook
**Goal**: Ship IFS texture replacement. Modders can drop PNG files in `data_mods/my_mod/graphics/ver04/logo_ifs/tex/warning.png` and the game loads the converted texture instead of the original.
**Scope**:
- Create `src/services/avs_layeredfs/ifs_textures.rs`
- `parse_texturelist()` — called from handle_file_open when path ends in `texturelist.xml`. Parse XML, extract texture entries (name, format, compression, dimensions). Compute MD5 of each texture name. Store in `TextureMap`: MD5 path → texture info.
- `handle_texture()` — called from handle_file_open for non-XML files inside IFS. Look up MD5 path in TextureMap. If found and PNG exists in mod folder, convert and cache.
- PNG → ARGB8888REV conversion (RGBA → BGRA byte swap)
- PNG → DXT5 conversion (via `texpresso` crate, then word-swap endianness)
- AVSLZ compression (call AVS `cstream_*` functions)
- Cache converted textures with hash-based invalidation (reuse CacheHasher pattern)
- Validate replacement PNG dimensions match original
- Support both `mod_folder/name.png` and `mod_folder/tex/name.png` paths
- Thread-safe TextureMap behind Mutex
**Tests**: Build passes. A PNG in the correct mod folder path replaces the corresponding IFS texture. Cache is created. Dimension mismatch logs a warning and skips replacement.
**Dependencies**: Task 4 (file_hooks dispatch), Task 5 (CacheHasher pattern reusable)

- [x] 6.1 Create ifs_textures.rs with TextureMap and parse_texturelist
- [x] 6.2 Implement PNG→ARGB8888REV and PNG→DXT5 conversion
- [x] 6.3 Implement AVSLZ compression via AVS cstream API
- [x] 6.4 Implement handle_texture with cache management

---

## Task 7: AFP/Geo Mapping + Texture Injection + Polish
**Package(s)**: ddr-world-hook
**Goal**: Ship AFP/geo file replacement, new texture injection into IFS atlases, and final polish. Feature-complete layeredfs port.
**Scope**:
- Add to `ifs_textures.rs`: `parse_afplist()` — parse afplist.xml, map AFP names + geo shape IDs to MD5 paths. Store in `AfpMap`.
- Add to `ifs_textures.rs`: `handle_afp()` — look up MD5 path in AfpMap, redirect to mod file if found.
- Create `src/services/avs_layeredfs/texture_packer.rs` — port GuillotineBinPack (~80 lines). `pack_textures()` takes a list of bitmaps, returns packed atlas canvases.
- Add to `ifs_textures.rs`: new texture injection in `parse_texturelist()` — detect PNGs not matching existing textures, pack into atlas canvases, generate modified texturelist.xml with new texture/image nodes, write to cache.
- Wire `parse_afplist()` and `handle_afp()` into file_hooks dispatch
- Update README.md with layeredfs documentation (data_mods folder structure, supported features, config options)
- Update steering files (product.md, structure.md) to reflect new service
**Tests**: Build passes. AFP/geo files in mod folders replace originals. New PNGs not in original IFS are injected as new textures. README documents the feature.
**Dependencies**: Task 6 (ifs_textures module, TextureMap)

- [x] 7.1 Implement parse_afplist and handle_afp
- [x] 7.2 Create texture_packer.rs with GuillotineBinPack port
- [x] 7.3 Implement new texture injection in parse_texturelist
- [x] 7.4 Update README.md and steering files

---

## QA Section
**Status**: Pending
**Test Results**: 
**Feedback**: 

## Acceptance Section
**PM**: pending
**Status**: Pending
**Notes**: 
