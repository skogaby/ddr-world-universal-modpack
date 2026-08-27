# DLL Initialization Flow and Timing

> Research compiled by an exploratory agent on 2026-05-22. Verify
> file:line citations against the current source before acting.

## 1. From DllMain to First Hook

**Entry point**: `src/lib.rs` — `DllMain` dispatch.

```
DllMain(reason=DLL_PROCESS_ATTACH)
+-> std::thread::spawn(init)
    +-> init() async on new thread
        +- Wait for gamemdx.dll
        +- SignatureStore::resolve_all()
        +- SignatureStore::resolve_derived()
        +- Init services (widget_renderer, texture_resolver, ...)
        +- Register mods
        +- Enable mods (load config + call enable())
        +- Register mod menu
        +- Spawn deferred splash-screen thread
```

**Why a separate thread?** `DllMain` runs under the Windows loader
lock. Doing significant work (esp. `LoadLibrary` or
`GetModuleHandleA` on modules that are still mid-load) can deadlock.
The init thread runs after `DllMain` returns and the loader lock is
released. Documented in `.spec/steering/tech.md`.

## 2. Module-Load Detection and Polling

**Code**: `src/core/module_resolver.rs::wait_for_game_module()`.

```rust
pub fn wait_for_game_module() -> GameModule {
    loop {
        if let Some(m) = resolve_module(GAME_MODULE_NAME) {
            return m;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
```

**History**: Commit `caa2d55` (2026-05-12) changed polling from
100 ms -> 10 ms to accommodate early DLL-injection scenarios.

**Flow**:
- `GetModuleHandleA("gamemdx.dll")` every 10 ms.
- On hit, fill `GameModule { base, size, handle }` via
  `GetModuleInformation`.
- Return immediately; the rest of init begins.

If gamemdx.dll already loaded at injection: ~10 ms wait.
If injected before gamemdx.dll exists: 100 ms - 1 s+ wait.

## 3. ModRegistry and Mod Init Sequencing

**Code**: `src/mods/mod_trait.rs::ModRegistry::register()`.

Two-phase mod lifecycle:

1. **Registration** (init time): `mod_impl.init(ctx)` runs.
   - `required_signatures()` is checked first; missing signatures
     cause the mod to be silently skipped (no partial registration).
2. **Enable** (after config load): `mod_impl.enable()` runs.
   - This is where `retour::GenericDetour` install actually happens
     for most mods.

**Mod registration order** (from `src/lib.rs`):

1. `SongLimitExpansionMod` — scans for 6 patch sites at init.
2. `HelloWorldMod`
3. `FastBootupMod` — captures hook addresses + global table ptr.
4. `SkipIntrosMod`
5. `TimerFreezeMod`
6. `AutoplayMod`
7. `SeriesExpansionMod`
8. `FolderExpansionMod`
9. `NoteTypesExpansionMod` — resolves allocators, judge_submit at init.
10. `WebUiOptionsMod`

**Then**: `ModRegistry::enable_with_config()` reads `mod-config.json`
and calls `enable()` on each registered mod.

## 4. Per-Module Init Flow Diagram

```
spice2x -k ddr_world_hook.dll -> game.exe
                  |
                  +-> DllMain (loader lock held)
                      +-> spawn init thread (background)
                          |
                          +- Polling phase: poll gamemdx.dll every 10 ms
                          |   typical: ~10-50 ms; worst: 1 s+ early-inject
                          |
                          +- SignatureStore::resolve_all()  ~25-70 ms
                          |   (serial scan_pattern() per signature)
                          |
                          +- resolve_derived()  ~10-20 ms
                          |   (RIP-rel decode, RTTI walks, xrefs)
                          |
                          +- Services init (serial)
                          |   +- widget_renderer (install render hook)
                          |   +- texture_resolver
                          |   +- asset_loader
                          |   +- afp_patcher
                          |   +- bm2d_api
                          |   +- custom_options
                          |   +- judge_hook (shared dispatcher)
                          |
                          +- Mod registration phase (serial)
                          |   +- SongLimitExpansionMod::init   ~5 ms
                          |   +- NoteTypesExpansionMod::init   ~5 ms
                          |   +- Others <1 ms each
                          |
                          +- Mod enable phase (serial)
                          |   +- SongLimitExpansionMod::enable ~1 ms (6-byte patch)
                          |   +- Other mods install retour detours ~0.5-1 ms each
                          |
                          +- Game proceeds (render-hook live, scene events firing)

Approximate totals (happy path, DLL loaded at injection time):
  Poll: 10-50 ms      Signatures: 35-90 ms
  Services: 20-30 ms  Mod register/enable: 30-50 ms
  --- Total: ~150-250 ms
```

The polling phase is the largest variable. With early injection,
init can be delayed by 500 ms - 1 s+.

## 5. Hooks With Pre-Init Work

These mods perform significant work *before* their hooks can be
installed:

### SongLimitExpansionMod

- `init()`: AOB scans for 6 patch sites
  (3 parsers x {alloc, read} sites each); verifies the byte 0x10 at
  each.
- `enable()`: writes 0x80 over each 0x10 byte.
- The verification step at init is **mandatory** and cannot be
  deferred — this is the time-critical mod.

### NoteTypesExpansionMod

- `init()`:
  1. Resolve `agcs_heap_malloc` and `agcs_heap_free` from signatures.
  2. Resolve `judge_submit_fn` via RTTI walk.
  3. Stash both in `static mut` globals (used at hook callback time).
- The mine-hit judge callback uses these allocator pointers. If they
  weren't resolved at init, the callback would crash.

### FastBootupMod

- `init()`: captures `check_step_data_update` signature address;
  stores `step_data_global_table` pointer in `static mut`. Used by
  the per-frame hook callback to inspect ready-state bytes.

### Deferred Widget Creation

- The splash screen waits for `widget_renderer::is_available()`.
  That flag is set when the render-function hook captures the font
  pointer on the first rendered frame. Pure deferred work, not
  pre-init work.

## 6. Sequencing Constraints (Load-Bearing Order)

| Step | Service | Must complete before |
|------|---------|----------------------|
| 1 | Config store | LayeredFS, mods |
| 2 | AVS LayeredFS | Mods that use file replacement |
| 3 | Widget renderer | Texture resolver, splash screen |
| 4 | Texture resolver | Image widgets |
| 5 | Asset loader | Mods that load custom ARC |
| 6 | AFP patcher | Custom options |
| 7 | BM2D API | Series filter scroll |
| 8 | Custom options | Options scroll, custom_options_persistence |
| 9 | Scene manager | Mods needing scene events |
| 10 | Input manager | Render-thread input polling |
| 11 | Judge hook | Autoplay, NoteTypesExpansion (register pre/post) |

The sequence is fully **serial** today. The chief
parallelization opportunity is signature scanning: 50+ patterns
each scan O(M) module bytes; an Aho-Corasick-prefilter multi-pattern
pass would be ~O(M) for all of them combined.

## Key Observation

`required_signatures()` is a **declarative filter** that gates mod
registration. If a mod declares a signature it needs and that
signature didn't resolve, the mod is skipped — no partial install.
This is good hygiene and we should preserve it.
