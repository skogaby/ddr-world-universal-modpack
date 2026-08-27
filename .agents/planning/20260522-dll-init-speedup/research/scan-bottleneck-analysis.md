# Scan Bottleneck Analysis

> Research compiled by an exploratory agent on 2026-05-22.
> Estimates below are derived from code reading; **verify with the
> profiling proposal in section 5 before acting on them**.

## 1. How a Single Scan Works

`scan_pattern()` implementation in `core/scanner.rs`:

```
1. Pre-filter: extract first byte of pattern -> fast lookup.
2. Scan loop: for each byte in module matching first byte:
   - Load next N bytes from module
   - Compare against full pattern (byte-by-byte)
   - Stop on first match, return ScanResult.
```

Memory access pattern:
- Sequential linear scan of module bytes: `base` -> `base + size`.
- OS prefetch + page cache -> memory-bandwidth-bound, not random IO.
- Pattern compare is CPU-bound (small patterns ~8 bytes typical).

## 2. Module-Bytes Acquisition Cost

Acquired once at init via `module_resolver.rs::resolve_module()`:

```
1. GetModuleHandleA(name) — O(1) lookup in process module list.
2. GetModuleInformation(handle) -> MODULEINFO {
     lpBaseOfDll, SizeOfImage, EntryPoint
   }.
3. No data fetch; the loader already mapped the module pages.
```

**Acquisition cost: O(1) plus a small syscall (~1-10 us).**

## 3. Cost Model: Quantitative Bottleneck

### Single-scan throughput

Assume gamemdx.dll ~ 50 MB.

For a pattern with first-byte pre-filter:
- First-byte hits: ~50 MB * (frequency / 256) ~ 50 MB * 5-10% ~ 2.5-5 MB.
- Pattern compares per first-byte hit: ~pattern_len / 8.
- Modern CPU: ~10^9 byte-compares per second per core.
- Per-pattern scan: ~0.1-1 ms (estimated). Empirical observation in
  similar engines puts it ~0.3 ms.

### Central resolution cost (signatures.rs::resolve_all + derived)

- 50+ patterns * 0.3 ms = **15-50 ms** total for resolve_all.
- `resolve_derived` adds **+10-20 ms** (multi-step chains, fewer scans).
- **Total: ~25-70 ms** (estimated).

### song_limit_expansion redundancy

- 2 * `scan_pattern_all()` on the same module.
- Each scans full ~50 MB for the pattern; returns all hits.
- **Redundant cost: ~0.6-2 ms** (estimated).

### Init timeline (estimated, happy path)

```
t=0       DllMain -> spawn init thread
t~10 ms   module_resolver poll detects gamemdx.dll
t~10-80   SignatureStore::resolve_all()       (15-50 ms)
t~90-110  resolve_derived()                   (+10-20 ms)
t~100ms+  AVS LayeredFS init                  (+5-50 ms, I/O bound)
t~150ms+  widget_renderer + texture_resolver  (+10-30 ms, GPU bound)
t~200ms+  custom_options + persistence        (+5-20 ms)
t~250ms+  scene_manager + judge_hook install  (+5-10 ms)
t~350ms+  Mod registration loop:
            song_limit_expansion::init        (+0.6-2 ms scan x 2)
            other mods                         (+1-2 ms each)
t~400 ms  ModRegistry::enable_with_config()
            retour detour installs            (+0.5-1 ms each)
t~410 ms+ Splash screen, render-hook live; game proceeds.
```

## 4. Other Init Costs (Non-Scan)

- **AVS LayeredFS init** (`avs_layeredfs.rs`): opens PAK files, builds
  ramfs indices. **I/O-bound, ~5-50 ms** (likely a top contributor).
- **Widget renderer D3D setup** (`widget_renderer.rs`): font/render
  context setup. **~10-30 ms.**
- **Custom options registry** (`custom_options/*.rs`): allocates
  per-option UI components, builds option tree. **~5-20 ms.**
- **retour `GenericDetour::install()`** (per hook): writes to .text
  pages via VirtualProtect; CPU + page-fault overhead. **~0.5-1 ms.**

## 5. Profiling Proposal

We should **measure before redesigning**. Before any architectural
change, ship a one-time diagnostic build that emits absolute and
delta wall-clock timing for each major init phase.

### Sketch (apply by hand against `src/lib.rs::init`)

```rust
let t0 = std::time::Instant::now();
let mut last = t0;
macro_rules! tick { ($label:literal) => {
    let now = std::time::Instant::now();
    log_info!("[init-prof] {} +{:?} (elapsed {:?})",
              $label, now - last, now - t0);
    last = now;
}}

let game_module = module_resolver::wait_for_game_module();
tick!("module_load");

let mut signatures = SignatureStore::new(&game_module);
let _ = signatures.resolve_all();
tick!("resolve_all");
signatures.resolve_derived();
tick!("resolve_derived");

// ... wrap each service init similarly ...
tick!("avs_layeredfs");
tick!("widget_renderer");
// ...

let mut reg = registry.lock().unwrap();
reg.register(Box::new(SongLimitExpansionMod::new()), &ctx);
tick!("song_limit_expansion::init");
// ... other mods ...
reg.enable_with_config(&ctx, &cfg);
tick!("enable_phase");
```

Optional: instrument `scan_pattern` and `scan_pattern_all` to log
their elapsed time when called, so we can see per-pattern cost.

### Expected output (estimated; for comparison after deploy)

```
[init-prof] module_load        +12ms  (elapsed 12ms)
[init-prof] resolve_all        +45ms  (elapsed 57ms)
[init-prof] resolve_derived    +15ms  (elapsed 72ms)
[init-prof] avs_layeredfs      +28ms  (elapsed 100ms)
[init-prof] widget_renderer    +22ms  (elapsed 122ms)
[init-prof] services_misc      +8ms   (elapsed 130ms)
[init-prof] mods_register      +12ms  (elapsed 142ms)
[init-prof] enable_phase       +18ms  (elapsed 160ms)
```

If actual numbers diverge a lot from these estimates, the design
should be retargeted at the **measured** dominant phase, not the
assumed one.

## 6. Bottleneck Ranking (estimated, pre-measurement)

1. **AVS LayeredFS I/O** — 20-50 ms (I/O-bound, hardware-dependent).
2. **Widget renderer D3D** — 10-30 ms (GPU-bound, hardware-dependent).
3. **Central signature scan** — 25-70 ms (CPU-bound, parallelizable).
4. **song_limit_expansion redundant scans** — 0.6-2 ms (eliminable).
5. **Custom options registry** — 5-20 ms (CPU-bound, optimizable).

## Key Observation: User Mental-Model Calibration

- **Serial vs. parallel scan win is real but smaller than expected.**
  Even if the 50+ signature scans were perfectly parallelized across
  4 cores (~5-10 ms instead of 25-70 ms), the rest of init
  (LayeredFS I/O, widget D3D) doesn't parallelize. Optimistic total
  init drop: ~135 ms -> ~60-80 ms.
- **The musicdb race window is in the 100-200 ms range.** Reducing
  init from 250 ms to 80 ms does not guarantee a win every boot, but
  it should make the failure mode rare rather than 75%.
- **Numbers above are estimates.** Profile first.
