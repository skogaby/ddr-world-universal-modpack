# Design: Folder Expansion Mod

**Requirements**: [requirements.md](requirements.md)

---

## Overview

Add a config-driven mod that creates custom genre folders in DDR World's folder selection UI. The mod hooks into the game's folder initialization pipeline to inject new FolderProperty objects, hooks the has-songs predicate to ensure custom folders are always visible, and patches the difficulty restriction writes to unlock all difficulties for all folders. Follows the same architectural pattern as SeriesExpansionMod — JSON config, AOB signatures, SavedPatch for reversibility.

---

## Architecture Decisions

### Decision 1: Folder Creation via Game Function Calls (not memory-only table)

**Problem**: Custom folders need to be full FolderProperty objects (0x208 bytes each) with vtable-backed functors, shared_ptrs, and registration in the game's internal folder list. Unlike the series expansion (which patches an existing data table), the folder system requires calling C++ constructors and registration functions.

**Decision**: Resolve game function addresses via AOB signatures, cast to Rust function pointer types, and call them directly to construct and register FolderProperty objects.

**Rationale**: The codebase already uses this pattern — `asset_loader.rs` resolves `arc_load` via signature and calls it as `ArcLoadFn`. The folder system's constructor, functor constructors, and registration function all have stable calling conventions (standard x64 `__fastcall`). Replicating the 0x208-byte object layout manually would be fragile and miss internal state the constructor initializes.

**Alternatives Considered**:
- *Manual memory layout*: Allocate 0x208 bytes and write fields directly. Rejected — the constructor initializes internal collections, shared_ptrs, and flags that we'd need to reverse-engineer completely. Missing any field risks crashes.
- *Code cave with inline assembly*: Write x86_64 assembly that mirrors the game's folder creation sequence. Rejected — extremely fragile, hard to maintain, and unnecessary when we can call the functions directly.

**Tradeoffs**: Requires ~6 new AOB signatures for game functions. If any signature breaks on a game update, the mod gracefully disables (standard pattern). The function-call approach is more robust than manual layout because it inherits any internal initialization the constructor performs.

### Decision 2: Hook folder_register for Insertion Ordering

**Problem**: Custom folders must appear after vanilla genre folders but before ALL MUSIC. The game creates folders in a fixed order inside `folder_init` (`FUN_180141050`): 6 genre folders → ALL MUSIC → brave folders → special folder. Post-hooking folder_init would place custom folders after everything.

**Decision**: Hook `folder_register` (`FUN_180143db0`). In the hook, detect when ALL MUSIC is being registered (check `folder_type_id == 7` at offset +0x00 of the FolderProperty pointer). When detected, first create and register all custom folders, then let ALL MUSIC register via the original function.

**Rationale**: This is minimally invasive — we intercept a single registration call at exactly the right moment. No need to understand the folder list's internal data structure or manipulate it after the fact. The type_id check is reliable because ALL MUSIC always has type_id 7.

**Alternatives Considered**:
- *Post-hook folder_init + list reordering*: Hook the entire init, then find ALL MUSIC in the list and insert before it. Rejected — requires reverse-engineering the list container (likely `std::vector<shared_ptr<FolderProperty>>`) and performing unsafe insertions.
- *Mid-function hook*: Patch a jump inside folder_init between musicgamers and allmusic creation. Rejected — extremely fragile, depends on exact instruction layout.
- *Post-hook folder_init, accept ordering*: Append custom folders after ALL MUSIC. Rejected — violates the requirement, and the hook approach is clean enough.

**Tradeoffs**: The hook fires for every folder registration (including brave/special folders), but the type_id check is a single u32 read — negligible overhead. If the game changes ALL MUSIC's type_id, the hook would miss the insertion point and custom folders would appear after ALL MUSIC (graceful degradation, not a crash).

### Decision 3: NOP-Based Difficulty Unlock

**Problem**: 6 sites in `folder_init` write max_difficulty values (0 or 1) to FolderProperty objects, overriding the constructor's default of 4 (all difficulties). The FIRST STEP site uses a 7-byte encoding (`MOV [RDI+0xc0], R13D`) while the other 5 use a 10-byte encoding (`MOV dword [RDI+0xc0], 1`).

**Decision**: NOP all 6 write sites. The constructor already initializes `+0xc0` to 4 (all difficulties). By NOPing the overwrites, all folders keep the constructor default.

**Rationale**: This elegantly sidesteps the FIRST STEP encoding mismatch. NOPing 7 bytes and NOPing 10 bytes are both trivial operations. No code caves, no instruction rewriting, no size mismatches. The approach is uniform across all 6 sites.

**Alternatives Considered**:
- *Patch immediate bytes (01→04) for 5 sites + code cave for FIRST STEP*: More surgical for the 5 standard sites, but requires a code cave for FIRST STEP (7 bytes can't hold a 10-byte instruction). Rejected — unnecessary complexity when NOPing achieves the same result more simply.
- *Hook the constructor to change the default*: Set default to 4 and NOP all 6 sites. Rejected — the constructor already defaults to 4, so we only need the NOPs.

**Tradeoffs**: NOPing is slightly less "surgical" than patching a single byte, but it's simpler, uniform, and the end result is identical. All patches are reversible via SavedPatch.

### Decision 4: Has-Songs Predicate Hook (not vtable patch)

**Problem**: The game hides folders with zero matching songs. Custom folders using bit indices ≥ 10 won't have entries in the song count arrays (only 10 slots exist, indices 0-9). The predicate reads `[ListManager + bit_index*4 + 0xd0]` — for index ≥ 10, this reads out-of-bounds memory.

**Decision**: Hook `FUN_180145e80` (the has-songs predicate) directly via `retour`. Call the original first. If it returns true, pass through. If false, check whether the bit_index matches a configured custom folder — if so, return true.

**Rationale**: Direct function hooking is the standard pattern in this codebase. All folder functors share the same vtable, so hooking the function affects all folders uniformly. The hook only overrides behavior for configured custom indices — vanilla folders are unaffected.

**Alternatives Considered**:
- *Vtable patch*: Overwrite the vtable entry at `0x180370fc0 + 8` to point to our function. Rejected — vtable is in read-only memory (requires VirtualProtect), and the vtable is shared across all functors so we'd need the same conditional logic anyway.
- *Extend the count arrays*: Allocate larger arrays and patch the zeroing loops and read sites. Rejected — much more complex, and the predicate hook achieves the same visibility result with a single hook point.

**Tradeoffs**: The hook always returns true for configured custom folders, even if they genuinely have zero songs. This matches the requirement ("Custom folders with zero matching songs still appear"). If accurate empty-folder hiding is ever needed, the count array extension approach could be revisited.

### Decision 5: Folder Type ID Numbering

**Problem**: Custom folders need unique `folder_type_id` values. Vanilla uses 1-7 (genre), 8-0xa (brave), 0x63 (special).

**Decision**: Assign custom folders type IDs starting at 0x10 (16), incrementing by 1 per config entry.

**Rationale**: 0x10 is well clear of all known vanilla IDs (max genre=7, max brave=0xa, special=0x63). The gap between 0x0a and 0x10 provides buffer. The type_id is stored as a u32, so there's no overflow concern.

**Alternatives Considered**:
- *Start at 0x0b (11)*: Immediately after brave folders. Rejected — too close to vanilla IDs, risk of collision if Konami adds new folder types.
- *Start at 0x100*: Very safe but unnecessarily large. No practical difference from 0x10.

**Tradeoffs**: None significant. If Konami ever uses type IDs in the 0x10-0x62 range, we'd need to adjust. Unlikely given the current sparse usage.

---

## Component Design

### New Components

| Component | Location | Responsibility |
|-----------|----------|----------------|
| `FolderExpansionMod` | `src/mods/folder_expansion.rs` | Mod trait implementation. Config loading, signature resolution, patch management, folder creation orchestration. |
| `FolderConfig` | `src/mods/folder_expansion.rs` (struct) | Deserialized config from `folder-expansion.json`. |
| `CustomFolderEntry` | `src/mods/folder_expansion.rs` (struct) | Per-folder config: bit_index, key, label, voice_key, max_difficulty. |

### No New Services or Widgets

This mod does not introduce new services or widget types. It reuses:
- `asset_loader::register_arc` for ARC loading
- `core::memory` for allocation and patching
- `core::scanner` for AOB scanning (difficulty sites)
- The existing `SavedPatch` pattern (inlined in the mod, same as SeriesExpansion)

### Component Interactions

```
folder-expansion.json
    ↓ (read at init)
FolderExpansionMod::init()
    ├── Resolve 7 AOB signatures (folder_init, ctor, functors, register, predicate)
    ├── Scan for 6 difficulty patch sites within folder_init
    └── Store config + resolved addresses

FolderExpansionMod::enable()
    ├── Register ARC via asset_loader
    ├── Install folder_register hook (retour static detour)
    │     └── On ALL MUSIC detection (type_id == 7):
    │           For each custom folder in config:
    │             1. Call folder_property_ctor (allocate + init 0x208 bytes)
    │             2. Set type_id, max_difficulty, key, voice_key, mode_flag
    │             3. Call folder_functor_ctor (property bit functor)
    │             4. Call folder_filter_functor_ctor (filter functor)
    │             5. Call folder_store_ptr (shared_ptr at +0x1a8)
    │             6. Call original folder_register (register custom folder)
    │           Then call original folder_register for ALL MUSIC
    ├── Install has_songs predicate hook (retour static detour)
    │     └── Call original → if true, return true
    │         Read bit_index from functor → if in config, return true
    │         Otherwise return original result
    └── Apply 6 difficulty NOP patches via SavedPatch

FolderExpansionMod::disable()
    ├── Restore all SavedPatch entries (difficulty sites)
    ├── Hooks auto-removed by ModRegistry
    └── Note: created FolderProperty objects persist until game restart
```

### Removed Components

None — this is a new mod.

---

## Integration Points

**Game Functions Called** (resolved via AOB signatures):

| Function | Research Address | Calling Convention | Purpose |
|----------|------------------|--------------------|---------|
| `folder_init` | `0x180141050` | — (hooked, not called) | Hook target for folder_register interception |
| `folder_property_ctor` | `0x180140b60` | `__fastcall(RCX=this)` → void | Construct 0x208-byte FolderProperty |
| `folder_functor_ctor` | `0x180144040` | `__fastcall(RCX=out, EDX=bit_index)` → void | Create property-bit functor |
| `folder_filter_functor_ctor` | `0x180143ff0` | `__fastcall(RCX=out, EDX=bit_index)` → void | Create filter functor |
| `folder_store_ptr` | `0x180140ce0` | `__fastcall(RCX=folder_property)` → void | Store shared_ptr at +0x1a8 |
| `folder_register` | `0x180143db0` | `__fastcall(RCX=context, RDX=folder_property)` → void | Push folder into list (hooked) |
| `folder_has_songs` | `0x180145e80` | `__fastcall(RCX=functor)` → bool | Has-songs predicate (hooked) |

**Data Storage**: `folder-expansion.json` (read-only, loaded at init)

**Existing Services Used**: `asset_loader::register_arc`

---

## Config Format

```json
{
  "custom_folders": [
    {
      "bit_index": 10,
      "key": "bemani",
      "label": "BEMANI",
      "voice_key": "",
      "max_difficulty": 4
    }
  ],
  "unlock_all_difficulties": true,
  "arc_path": "data/arc/bm2d/custom_folders.arc"
}
```

| Field | Type | Required | Default | Notes |
|-------|------|----------|---------|-------|
| `custom_folders` | array | yes | — | May be empty (difficulty unlock still applies) |
| `custom_folders[].bit_index` | u32 | yes | — | Property bitmask bit index. Recommend ≥ 10. |
| `custom_folders[].key` | string | yes | — | BM2D texture key. Max 15 chars (SSO limit). |
| `custom_folders[].label` | string | yes | — | Display label. Max 15 chars (SSO limit). |
| `custom_folders[].voice_key` | string | no | `""` | Voice-over key. Empty = silent. |
| `custom_folders[].max_difficulty` | u32 | no | `4` | 0=Beginner, 1=Basic, ..., 4=Challenge (all). |
| `unlock_all_difficulties` | bool | no | `true` | Patch all vanilla folders to allow all difficulties. |
| `arc_path` | string | no | — | Path to custom ARC with folder textures. |

---

## New Signatures Required

7 new entries in `src/core/signatures.rs`:

| Name | Target Function | Purpose |
|------|-----------------|---------|
| `folder_init` | `FUN_180141050` | Hook target — post-hook to know when folder creation is happening |
| `folder_property_ctor` | `FUN_180140b60` | Construct FolderProperty objects |
| `folder_functor_ctor` | `FUN_180144040` | Create property-bit functor |
| `folder_filter_functor_ctor` | `FUN_180143ff0` | Create filter functor |
| `folder_store_ptr` | `FUN_180140ce0` | Wire shared_ptr at +0x1a8 |
| `folder_register` | `FUN_180143db0` | Register folder in list (hook target) |
| `folder_has_songs` | `FUN_180145e80` | Has-songs predicate (hook target) |

The 6 difficulty patch sites are found via inline AOB scanning within `folder_init`'s address range (same pattern as `SongLimitExpansionMod`), not as separate top-level signatures.

---

## Changes to Existing Code

### `src/core/signatures.rs`
- **Change**: Add 7 new `SignatureDefinition` entries to the `SIGNATURES` array
- **Reason**: FolderExpansionMod needs these game function addresses
- **Impact**: No effect on existing mods — signatures are resolved independently

### `src/mods/mod.rs`
- **Change**: Add `pub mod folder_expansion;`
- **Reason**: Register the new module
- **Impact**: None

### `src/lib.rs`
- **Change**: Add `reg.register(Box::new(mods::folder_expansion::FolderExpansionMod::new()), &ctx);` in the mod registration block
- **Reason**: Register the mod with ModRegistry
- **Impact**: None — if signatures are missing, registration is skipped gracefully

---

## Deployment Sequence

1. Add AOB signatures to `signatures.rs` (requires Ghidra analysis to determine stable byte patterns)
2. Implement `folder_expansion.rs`
3. Build DLL (`./build.sh`)
4. Create test `folder-expansion.json` with a test folder entry
5. Create test ARC with folder textures via `build_ddr_package`
6. Deploy DLL + config + ARC to test machine
7. Verify: custom folder appears in carousel, song filtering works, difficulty unlock works

**Rollback**: Remove `folder-expansion.json` from game directory — mod disables itself when config is missing. Or disable via mod menu.

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Game function calling conventions are wrong (crashes on ctor call) | High | Medium | Verify each function's register usage in Ghidra before writing the Rust type alias. Test with a single folder first. The research doc provides disassembly for each function. |
| folder_register hook fires at wrong time (race with game init) | High | Low | The hook is installed in `enable()`, which runs after game init. folder_init is called during scene setup, well after DLL init. |
| ALL MUSIC type_id changes in a game update | Medium | Low | Custom folders would appear after ALL MUSIC instead of before. Graceful degradation — no crash. Log a warning if type_id 7 is never seen. |
| SSO string overflow (key/label > 15 chars) | Medium | Low | Validate string lengths during config loading. Reject entries with keys > 15 chars. |
| Custom folders persist after disable (no destructor call) | Low | High | Accepted — FolderProperty objects are allocated for the game process lifetime. Disabling the mod restores difficulty patches and removes hooks, but created folders remain until game restart. Document this behavior. |
| Difficulty NOP sites not found (game update changes encoding) | Medium | Low | Scan for both the standard pattern (`C7 87 C0 00 00 00`) and the FIRST STEP pattern (`44 89 AF C0 00 00 00`) within folder_init's range. If expected count doesn't match, skip difficulty unlock and log warning. |
| Bit index 8 special case causes unexpected song matching | Low | Low | Document in README that bit index 8 has a legacy quirk. Recommend users start at index 10+. |

---

## Open Questions

1. **folder_register's first parameter** — The research shows `FUN_180143db0` is called as step 10 of folder creation. Need to confirm whether RCX is a context/manager pointer (likely `this` of the folder manager) or the FolderProperty itself. If it's a manager pointer, we need to capture it from the hook's first invocation. This will be resolved during signature analysis in implementation.
