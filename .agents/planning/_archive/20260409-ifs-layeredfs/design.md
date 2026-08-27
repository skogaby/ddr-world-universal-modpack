# Design: IFS LayeredFS Port

**Requirements**: [requirements.md](requirements.md)

---

## Overview

Port the [ifs_layeredfs](https://github.com/mon/ifs_layeredfs) C++ project into the Rust modpack as a native service, excluding texbin support (DDR World uses IFS, not texbin) and `data2/` path handling (DDR World only uses `data/`). This hooks Konami's AVS filesystem layer (`libavs-win64.dll`) to intercept file accesses at runtime, enabling transparent replacement and injection of game assets from a `data_mods/` folder without repacking container files. The port eliminates the need to run ifs_layeredfs as a separate DLL and unblocks the folder-expansion feature's geo file serving.

---

## Architecture Decisions

### Decision 1: Directory Module (not single file)

**Problem**: The port spans ~2000 lines across 7 concerns (AVS resolution, file dispatch, mod paths, RAM demangling, IFS textures, XML merging, bin-packing). A single `avs_layeredfs.rs` would be unmanageable.

**Decision**: Implement as `src/services/avs_layeredfs/` directory module with internal submodules. The public API surface remains a single `services::avs_layeredfs` module with `init()` / `is_available()`.

**Rationale**: Idiomatic Rust — directory modules are the standard way to organize large subsystems. The existing codebase uses flat files for services, but those are all <500 lines. This subsystem is 5-10x larger. The `mod.rs` file exposes the same public API pattern (`init`, `is_available`) that other services use.

**Alternatives Considered**:
- *Single file*: Would be 2000+ lines, hard to navigate and maintain. Rejected.
- *Multiple top-level services* (`avs_resolver`, `mod_paths`, `ramfs_demangler`, etc.): Violates the steering file rule against new top-level modules outside the established set. These are internal implementation details of one service, not independent services.

**Tradeoffs**: First directory module in `services/`. Sets a precedent, but the scope justifies it.

### Decision 2: AVS Resolution via GetProcAddress with Export Name Tables

**Problem**: AVS DLL exports use different naming schemes across versions — plain names for ≤2.12, obfuscated `XC*` prefixes for 2.13+. The C++ code defines 6 export table variants and iterates them.

**Decision**: Port the export table approach directly. Define a Rust array of `AvsExportTable` structs (one per AVS version), each mapping function names to their obfuscated export strings. Iterate tables, try `GetProcAddress` for each, stop on first complete match.

**Rationale**: This is the proven approach from the C++ code. The export tables are stable — they change only when Konami releases a new AVS version (rare, ~once per year). The alternative of AOB scanning the AVS DLL is fragile and unnecessary when export names are known.

**Alternatives Considered**:
- *AOB scanning the AVS DLL*: The modpack's existing pattern, but AVS exports are known by name — scanning is unnecessary overhead and fragility.
- *Hardcode only the 2.17.x table* (DDR World's version): Would work today but breaks forward compatibility. The full table costs ~2KB of static data and takes microseconds to iterate.

**Tradeoffs**: Carrying 6 export tables for versions we may never encounter. Negligible cost for significant forward compatibility.

### Decision 3: retour Static Detours for AVS Hooks

**Problem**: Need to hook 5 AVS functions (`avs_fs_open`, `avs_fs_lstat`, `avs_fs_mount`, `avs_fs_read`, `avs_fs_convert_path`). The hooks must call the original function and be thread-safe.

**Decision**: Use `retour` static detours — the same pattern used throughout the codebase for `scene_hook`, `judge_notes_hook`, `afp_stream_do_create_hook`, etc.

**Rationale**: Consistent with existing codebase. `retour` handles trampoline generation, original function preservation, and thread-safe hook installation. The `static-detour` feature stores detours in statics, which is required for hook callbacks that need to call the original.

**Alternatives Considered**: None — this is the only hooking mechanism in the codebase and it works well.

**Tradeoffs**: None. Standard pattern.

### Decision 4: BTreeMap with Linear Prefix Scan for Demangler (not hat-trie)

**Problem**: The RAM FS demangler needs longest-prefix matching — given a virtual path like `/data/imagefs/msc/xxxx/file.bin`, find the longest registered mount prefix that matches. The C++ code uses a hat-trie (`tsl::htrie_map`) for this.

**Decision**: Use `BTreeMap<String, String>` with a linear scan for longest-prefix match. The BTreeMap's ordered iteration lets us break early in some cases, but the primary reason is simplicity.

**Rationale**: The number of mounted filesystems at any time is small — typically dozens, not thousands. A linear scan over 20-50 entries takes nanoseconds. The hat-trie is an optimization for a problem that doesn't exist at this scale. Adding a trie crate dependency for a micro-optimization in a non-hot path is unjustified.

**Alternatives Considered**:
- *Port hat-trie or use `trie-rs` crate*: Adds dependency for negligible performance gain. The demangler is called once per file open, not per frame.
- *HashMap with iterative prefix truncation*: Check full path, then truncate last segment, repeat. Works but is more code than linear scan for the same result.

**Tradeoffs**: O(n) scan vs O(k) trie lookup where n=mount count and k=path length. At n<100, the scan is faster due to cache locality and no pointer chasing.

### Decision 5: New Crate Dependencies for Texture/Compression/XML

**Problem**: The C++ code uses lodepng, stb_dxt, rapidxml, MD5, and GuillotineBinPack. Need Rust equivalents.

**Decision**:

| C++ Library | Rust Crate | Rationale |
|-------------|-----------|-----------|
| lodepng | `png` (0.17) | Standard Rust PNG decoder/encoder. Lightweight, well-maintained. |
| stb_dxt | `texpresso` (2.0) | DXT1/DXT5 compression. Pure Rust, no unsafe. |
| MD5 | `md5` (0.7) | Tiny, single-purpose. Used for texture name hashing. |
| rapidxml | `quick-xml` (0.36) | Fast streaming XML reader/writer. Supports both read and write (unlike `roxmltree` which is read-only). Needed for XML merging which modifies and writes XML. |
| GuillotineBinPack | Inline port (~100 lines) | The algorithm is ~60 lines of logic. Pulling a crate for this is overkill. Port the `Pack` function directly into `texture_packer.rs`. |

**Alternatives Considered**:
- *`image` crate instead of `png`*: Much heavier (supports JPEG, GIF, BMP, etc.). We only need PNG decode. `png` is 1/10th the compile time.
- *`roxmltree` instead of `quick-xml`*: Read-only — can't write modified XML for merge output. Rejected.
- *`guillotiere` crate for bin-packing*: Atlas allocator, but API doesn't match our needs (we need offline packing, not dynamic allocation). The original algorithm is simple enough to port inline.

**Tradeoffs**: 4 new crate dependencies. All are small, well-maintained, and pure Rust. Total compile time impact: ~5-8 seconds incremental.

### Decision 6: Config in mod-config.json (not command-line flags)

**Problem**: The C++ code parses command-line flags (`--layered-verbose`, `--layered-devmode`, etc.). The Rust modpack uses `mod-config.json` for all configuration.

**Decision**: LayeredFS configuration lives in `mod-config.json` under a `"layeredfs"` key, consistent with the existing modpack pattern. No command-line parsing.

**Rationale**: The requirements explicitly state this (Resolved Question #1). The modpack already has a JSON config system. Adding a separate command-line parser would be inconsistent and confusing.

**Alternatives Considered**: None — requirements are clear.

**Tradeoffs**: None. Better UX than command-line flags for arcade operators.

### Decision 7: AVSLZ Compression via AVS DLL's Own cstream API

**Problem**: IFS textures may use AVSLZ compression. The C++ code calls the AVS DLL's `cstream_create`/`cstream_operate`/`cstream_finish`/`cstream_destroy` functions to compress converted textures.

**Decision**: Same approach — resolve and call the AVS DLL's cstream functions. These are already resolved as part of the AVS export table (Decision 2).

**Rationale**: AVSLZ is a Konami-proprietary compression format. There is no public Rust implementation. The AVS DLL already provides the compressor, and we're already loading it. Reimplementing would be reverse-engineering effort with no benefit.

**Alternatives Considered**:
- *Pure Rust AVSLZ implementation*: Would require reverse-engineering the format. Unnecessary when the DLL provides it.

**Tradeoffs**: Depends on AVS DLL being loaded (which it always is — the game can't run without it).

---

## Scope Exclusions

### Texbin Support (US-9) — Dropped
DDR World uses the IFS texture pipeline, not the texbin (`.bin` / PXET) format used by Gitadora and Jubeat. The texbin submodule, LZ77 compress/decompress, and the `.bin` dispatch path in `handle_file_open` are excluded from this port. Can be added later if needed for another game.

### `data2/` Path Handling — Dropped
DDR World only uses `data/`. The C++ code's special handling for `data2/` paths (keeping the `data2/` prefix in mod folders, scanning for additional game data roots) is excluded. Path normalization only needs to handle the `data/` prefix.

### pkfs Hooks — Already Out of Scope
Per requirements. DDR World does not use `libpackfs.dll`.

---

## Component Design

### New Components

| Component | Location | Responsibility |
|-----------|----------|----------------|
| `avs_layeredfs` (module) | `src/services/avs_layeredfs/mod.rs` | Public API: `init()`, `is_available()`, config loading. Orchestrates submodules. |
| `avs_resolver` | `src/services/avs_layeredfs/avs_resolver.rs` | Finds AVS DLL, resolves exports across version tables, stores function pointers. |
| `file_hooks` | `src/services/avs_layeredfs/file_hooks.rs` | 5 retour static detours on AVS functions. Core dispatch logic (`handle_file_open`). |
| `mod_paths` | `src/services/avs_layeredfs/mod_paths.rs` | Mod folder scanning, path normalization, file/folder lookup, caching. |
| `ramfs_demangler` | `src/services/avs_layeredfs/ramfs_demangler.rs` | Tracks open→read→mount chain. Demangles virtual paths to real IFS paths. Handles nested IFS (IFS-inside-IFS). |
| `ifs_textures` | `src/services/avs_layeredfs/ifs_textures.rs` | Parses texturelist.xml/afplist.xml, MD5 name mapping, PNG→game format conversion, AVSLZ compression, cache management. |
| `texture_packer` | `src/services/avs_layeredfs/texture_packer.rs` | GuillotineBinPack port for injecting new textures into IFS atlases. |
| `xml_merger` | `src/services/avs_layeredfs/xml_merger.rs` | `.merged.xml` detection, XML loading (binary prop + plain), node appending, cache with hash invalidation. |

### Component Interactions

```
Game calls avs_fs_open / avs_fs_lstat / avs_fs_mount / avs_fs_read / avs_fs_convert_path
    │
    ▼
file_hooks (retour static detours)
    │
    ├── avs_fs_mount → ramfs_demangler::on_mount()     (track ramfs/link/imagefs mappings)
    ├── avs_fs_read  → ramfs_demangler::on_read()      (track buffer→file mappings)
    ├── avs_fs_open  → handle_file_open()
    │       │
    │       ├── ramfs_demangler::demangle()             (virtual path → real IFS path)
    │       ├── mod_paths::normalise_path()             (strip /data/ prefix, normalize)
    │       ├── mod_paths::find_first_modfile()         (check mod folders for replacement)
    │       ├── xml_merger::merge_xmls()                (if .xml, check for .merged.xml)
    │       ├── ifs_textures::parse_texturelist()       (if texturelist.xml, build MD5 map + inject new textures)
    │       ├── ifs_textures::parse_afplist()           (if afplist.xml, build AFP/geo MD5 map)
    │       ├── ifs_textures::handle_texture()          (MD5 lookup → PNG → game format → cache)
    │       ├── ifs_textures::handle_afp()              (MD5 lookup → mod file path)
    │       └── call original avs_fs_open with replacement path (if any)
    │
    └── avs_fs_lstat → same handle_file_open() flow    (so game sees correct file sizes)
```

### Shared State (all behind Mutex)

| State | Owner | Accessed By | Purpose |
|-------|-------|-------------|---------|
| `AvsFunctions` | `avs_resolver` | `file_hooks`, `ifs_textures`, `xml_merger` | Resolved AVS function pointers (open, close, read, lstat, mount, property_*, cstream_*) |
| `ModCache` | `mod_paths` | `file_hooks` | Cached mod folder contents (non-dev mode) |
| `DemanglerState` | `ramfs_demangler` | `file_hooks` | open→buffer→ramfs→imagefs mapping chain |
| `TextureMap` | `ifs_textures` | `file_hooks` | MD5 path → texture info (name, format, compression, dimensions) |
| `AfpMap` | `ifs_textures` | `file_hooks` | MD5 path → AFP/geo mod path |

### No New Mods or Widgets

This is infrastructure — an always-on service. Not toggleable via mod menu. No UI elements.

### Removed Components

None — this is entirely new.

---

## Integration Points

**External DLLs** (resolved at runtime via `GetModuleHandle` + `GetProcAddress`):

| DLL | Functions Hooked | Functions Called | Purpose |
|-----|-----------------|-----------------|---------|
| `libavs-win64.dll` | `avs_fs_open`, `avs_fs_close`, `avs_fs_read`, `avs_fs_lstat`, `avs_fs_lseek`, `avs_fs_mount`, `avs_fs_convert_path` | `property_*` (6 functions), `cstream_*` (4 functions) | Core filesystem interception + binary XML handling + AVSLZ compression |

**Data Storage**:
- `data_mods/` — Mod folder root (configurable). Each subfolder is a mod.
- `data_mods/_cache/` — Converted texture cache, merged XML cache.

**Configuration** (in `mod-config.json`):
```json
{
  "layeredfs": {
    "verbose": false,
    "developer_mode": false,
    "mod_folder": "./data_mods",
    "allowlist": [],
    "blocklist": []
  }
}
```

**Existing Services Used**:
- `core::module_resolver` — to find `libavs-win64.dll` in process memory (or fall back to `GetModuleHandle` since AVS DLL names are known)

---

## Public Contracts (Signatures Only)

```rust
// src/services/avs_layeredfs/mod.rs — public API
pub fn init() -> bool;          // Resolve AVS, scan mods, install hooks
pub fn is_available() -> bool;  // Did init succeed?
```

All other submodules are `pub(super)` — internal to the `avs_layeredfs` module. No other service or mod calls into the submodules directly. The hooks are self-contained — once installed, they intercept AVS calls transparently.

---

## Changes to Existing Code

### `src/services/mod.rs`
- **Change**: Add `pub mod avs_layeredfs;`
- **Reason**: Register the new service module
- **Impact**: None

### `src/lib.rs`
- **Change**: Add `avs_layeredfs::init()` call in the init sequence, after module_resolver but before mod registration. Specifically between step 3 (derived addresses) and step 4 (widget renderer).
- **Reason**: LayeredFS must be active before any mod that depends on file replacement (folder_expansion). It doesn't depend on widget renderer, texture resolver, or any other service — only on the AVS DLL being loaded.
- **Impact**: None on existing services. The AVS hooks are on a different DLL (`libavs-win64.dll`) than any existing hooks (`gamemdx.dll`, `libafp-win64.dll`), so no conflicts.

### `Cargo.toml`
- **Change**: Add dependencies: `png`, `texpresso`, `md5`, `quick-xml`
- **Reason**: Texture conversion, hashing, XML parsing
- **Impact**: Increased compile time (~5-8s incremental). No runtime impact for unused features.

### `mod-config.json` (runtime)
- **Change**: New `"layeredfs"` key with config fields
- **Reason**: LayeredFS configuration
- **Impact**: Backward compatible — missing key means defaults apply

---

## Init Sequence (within avs_layeredfs::init)

1. Load config from `mod-config.json` `"layeredfs"` section (defaults if missing)
2. Find AVS DLL handle (`libavs-win64.dll`, `libavs-win32.dll`, or `avs2-core.dll`)
3. Iterate export tables, resolve all function pointers for the matching AVS version
4. If resolution fails → log warning, return false (graceful degradation)
5. Scan `data_mods/` for mod subfolders, apply allowlist/blocklist
6. Cache mod folder contents (non-dev mode) or prepare for live filesystem checks (dev mode)
7. Install 5 retour static detours on AVS functions
8. Log detected AVS version, mod folder count, config summary
9. Return true

---

## Deployment Sequence

1. Add crate dependencies to `Cargo.toml`
2. Implement `avs_layeredfs` service module with submodules
3. Wire into `lib.rs` init sequence
4. Build DLL (`./build.sh`)
5. Create test `data_mods/test_mod/` with a replacement file
6. Deploy DLL to test machine, verify file replacement works via log output
7. Test with folder-expansion: generate modified geo files into `data_mods/`, verify custom folder textures render correctly

**Rollback**: Remove `data_mods/` folder or set `"layeredfs": { "disable": true }` in mod-config.json. The service degrades gracefully — if no mod folders exist, hooks pass through with negligible overhead (one string comparison per file open).

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| AVS export names change in a future game update | High | Low | The export table approach supports multiple AVS versions. If DDR World ships a new AVS version, add a new table entry. The 2.17.x exports have been stable across multiple Konami games. |
| retour hook on AVS DLL conflicts with other hook DLLs | Medium | Low | The modpack replaces ifs_layeredfs entirely — they should not be loaded simultaneously. Document this in README. If both are loaded, the second hook installation will likely fail gracefully (retour returns error). |
| Thread safety issues in demangler or texture map | High | Medium | All shared state behind `Mutex`. The C++ code uses critical sections for the same reason. The lock granularity matches the C++ code — lock per operation, not per subsystem. Hot path (file open with no mod match) only takes the demangler lock briefly for prefix scan. |
| PNG→DXT5 conversion produces visual artifacts | Medium | Low | Using `texpresso` which is a Rust port of stb_dxt — the same algorithm the C++ code uses. Output should be byte-identical. Verify with a known test texture. |
| AVSLZ compression output differs from C++ version | Medium | Low | We're calling the same AVS DLL function (`cstream_create`/`cstream_operate`/`cstream_finish`). Output is deterministic for the same input. |
| Cache invalidation misses edge cases | Low | Medium | Port the C++ `CacheHasher` approach: hash input file timestamps + DLL timestamp + mod file timestamps. If any change, regenerate. Conservative — may regenerate unnecessarily, but never serves stale data. |
| Large mod folders cause slow startup in non-dev mode | Low | Medium | The C++ code walks all mod directories at startup and caches contents. For very large mod setups (thousands of files), this could take seconds. Acceptable — it's a one-time cost and matches the original behavior. Log progress for visibility. |
| `quick-xml` can't handle Konami's binary XML format | N/A | N/A | Not a risk — binary XML is handled by calling the AVS DLL's `property_*` functions to convert to text XML first, then `quick-xml` parses the text. Same approach as the C++ code (which uses rapidxml only for text XML). |

---

## Open Questions

None — all resolved during design review.
