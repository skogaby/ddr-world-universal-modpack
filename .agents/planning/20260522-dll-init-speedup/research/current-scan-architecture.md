# Current Scan Architecture

> Research compiled by an exploratory agent on 2026-05-22.
> Source-of-truth: source code as of HEAD `7819120`. Verify against
> current code before acting on any specific file:line citation.

## 1. Scanner Primitives

The scanner module (`src/core/scanner.rs`) exports 8 public functions:

1. `scan_pattern(base, size, pattern) -> Option<ScanResult>`
   - Single AOB pattern scan, first match only.
   - First-byte pre-filter + byte-by-byte comparison.
   - Called: `signatures.rs::resolve_all()` loop (once per signature).

2. `scan_pattern_all(base, size, pattern) -> Vec<ScanResult>`
   - Full module scan, returns all matches.
   - Called: `song_limit_expansion.rs:73, 84` — independent mod-level scans.

3. `decode_rip_relative(disp_addr) -> *const u8`
   - Resolves RIP-relative addressing for MOV/LEA instructions.
   - Used by: `signatures.rs` derivation chains (~20+ call sites).

4. `decode_call_rel32(call_addr) -> *const u8`
   - Resolves CALL rel32 (E8 + 4-byte rel32).
   - Used by: `signatures.rs` derivation chains; services module.

5. `scan_first_call_rel32(start, len) -> Option<*const u8>`
   - Finds first CALL rel32 instruction after a landmark.
   - Used by: `signatures.rs` derivation chain starts.

6. `scan_xrefs_to(base, size, target) -> Vec<*const u8>`
   - Cross-reference scan: finds instructions referencing a target.
   - Used by: `signatures.rs::derive_file_manager_singleton()`.

7. `scan_lea_xrefs_to(base, size, target) -> Vec<*const u8>`
   - Specialized xref: only LEA references.
   - Used by: `signatures.rs::derive_customize_offset()`.

8. `find_function_entry(addr, module_base) -> *const u8`
   - Walks backward to function prologue (PUSH RBP / MOV RSP,RBP).
   - Used by: signature derivation helpers.

## 2. Scan Call-Site Inventory

### Core Module

- `signatures.rs::resolve_all()`: 50+ calls to `scan_pattern()`, one per
  `SignatureDefinition`. Patterns include `arc_load`, `timer_update_jz`,
  `render_function`, `widget_factory`, `series_label_lookup`,
  `folder_register`, `gameplay_obj_alloc`, `game_malloc`,
  `agcs_heap_malloc`, `judge_submit`, etc. Module: `gamemdx.dll`
  (base, size from `GameModule`).

- `signatures.rs::resolve_derived()`: 11 calls to derivation helpers.
  Each helper uses scanner primitives (decode_rip_relative,
  scan_first_call_rel32, scan_xrefs_to, scan_lea_xrefs_to,
  find_function_entry). No independent module scans; all use
  base/size from context.

### Mods Module

- `song_limit_expansion.rs:73`:
  `scan_pattern_all(base, size, ALLOC_PATTERN)`
  **Independent scan** of gamemdx.dll for `"45 33 C0 BA 00 00 10 00 E8"`
  (3 expected hits: license, musicdb, coursedb allocation sites).

- `song_limit_expansion.rs:84`:
  `scan_pattern_all(base, size, READ_PATTERN)`
  **Independent scan** of gamemdx.dll for `"C7 44 24 20 00 00 10 00"`
  (3 expected hits: corresponding read-size sites).

- All other mods (autoplay, series_expansion, folder_expansion,
  note_types_expansion, etc.) use `signatures.get_address()` /
  `signatures.require_address()` — no independent scans.

### Services Module

- No independent pattern scans found. All use signature addresses
  retrieved from `SignatureStore`.

## 3. Pattern-Derivation Patterns

`signatures.rs` implements 15 derivation methods chaining scanner
primitives:

1. **RIP-relative decode chain** (most common)
   `landmark` -> `scan_first_call_rel32()` -> `decode_rip_relative()`
   Used by: arc_addresses, file_manager_singleton, customize_offset.

2. **RTTI-based vtable discovery**
   `landmark` -> `find_function_by_debug_string()` -> scan xrefs ->
   validate RTTI.
   Used by: find_sprite_vtable, find_option_tab_vtable,
   find_event_lambda_vtable_slots.

3. **Xref walking with filtering**
   `landmark` -> `scan_xrefs_to()` -> filter by instruction pattern.
   Used by: derive_gameplay_obj_addresses (xrefs to game_malloc),
   derive_app_heap_handle.

4. **Multi-call chain resolution**
   `landmark` -> `scan_first_call_rel32()` -> decode ->
   `scan_first_call_rel32(result)` -> ...
   Used by: find_check_step_data_actor (2-call chain),
   find_auto_foot_panel (3-call chain).

5. **Backward search for function prologue**
   `landmark + offset` -> `find_function_entry()`.
   Used by: derive_event_lambda_vtable_slots.

## 4. Scan Deduplication Status

### Already Centralized (good)

- 50+ signature patterns scanned once in `resolve_all()` loop.
- Scanned module: gamemdx.dll
  (`GetModuleInformation` -> `lpBaseOfDll`, `SizeOfImage`).
- Results cached in `SignatureStore::resolved` HashMap.

### Decentralized (redundant)

- `SongLimitExpansionMod` performs 2 independent
  `scan_pattern_all()` calls.
- Rescans identical module bytes (gamemdx.dll base, size).
- Occurs **after** central `resolve_all()` completes — wasted passes.

### Module-Level Caching

- `GameModule { name, base, size, handle }` acquired once via
  `GetModuleHandleA` + `GetModuleInformation`.
- Passed to `SignatureStore`, then to all services/mods via
  `ModContext`. No module reloading between scans.

## 5. Quantitative Summary

| Metric | Count | Notes |
|--------|-------|-------|
| Distinct AOB patterns (`signatures.rs`) | 50+ | All in `SIGNATURES`, scanned once in `resolve_all()` |
| Mods using independent scans | 1 | song_limit_expansion only |
| Independent `scan_pattern_all()` calls | 2 | song_limit_expansion.rs:73, 84 |
| Derivation methods chaining scanners | 15 | resolve_derived() helpers |
| Services performing scans | 0 | All use SignatureStore lookups |
| Other mods using signatures | 8+ | autoplay, series_expansion, folder_expansion, note_types_expansion, etc. |
| Total module bytes scanned (central) | 1x gamemdx.dll | ~50 MB typical |
| Total module bytes scanned (song_limit_expansion) | 2x gamemdx.dll | redundant |
| **Total redundant scan bytes** | **~100 MB** | 2 extra passes of ~50 MB |

## Key Mental-Model Corrections

1. **Most mods do NOT rescan independently.** Only
   `song_limit_expansion` does. Others (autoplay, series_expansion,
   folder_expansion, note_types_expansion) consume centralized
   signatures.

2. **Module bytes acquired only once.** `GetModuleInformation()` is
   O(1); the loader has already mapped gamemdx.dll. No reloading
   happens between scans.

3. **Derivation chains don't rescan.** The 15 `resolve_derived()`
   helpers call scanner primitives but operate on the
   already-acquired module pointers. They do not re-invoke
   `scan_pattern` or trigger module reloading.

4. **The redundancy in song_limit_expansion is small (~0.6-2 ms).**
   Removing it is correct hygiene but won't itself unblock the
   musicdb race.
