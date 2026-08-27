# Implementation Plan: Centralized Scanner + Permanent Profiling

> Source-of-truth design: `../design/detailed-design.md`.
> Implementation principle: profiling lands FIRST so every
> subsequent step has before/after measurement on the cabinet.

## Checklist

- [ ] **Step 1**: Restore profiling.rs with the runtime gate; add `DiagnosticsConfig` to mod-config schema.
- [ ] **Step 2**: Wire `profiling::tick` calls into `lib.rs::init` and `mod_trait::enable` (no scan instrumentation yet).
- [ ] **Step 3**: Add `record_scan_pattern*` instrumentation hooks to existing `scan_pattern` / `scan_pattern_all`. Capture baseline numbers.
- [ ] **Step 4**: Add `aho-corasick` dep + implement `scan_patterns_batch` (the new internal multi-pattern engine).
- [ ] **Step 5**: Rewrite `scan_pattern` and `scan_pattern_all` internals to use the new engine.
- [ ] **Step 6**: Add `scan_xrefs_to_batch`; restructure `signatures::resolve_derived` to use it for the 3 xref calls.
- [ ] **Step 7**: Migrate `signatures::resolve_all` to call `scan_patterns_batch` once for all 47 patterns.
- [ ] **Step 8**: Acceptance test: deploy with profiling on, verify all four R4 criteria.

---

## Step 1: Restore profiling.rs with runtime gate; add DiagnosticsConfig

**Objective**: Re-create the profiling module with the
`set_enabled` / `ENABLED: AtomicBool` gate. Add the new
`DiagnosticsConfig { profiling: bool }` to the config schema.
The module compiles but no caller invokes it yet.

**Files touched**:
- `src/core/profiling.rs` (new — restored from `git stash@{0}`
  with the gate added).
- `src/core/mod.rs` — add `pub mod profiling;`.
- `src/mods/config.rs` — add `DiagnosticsConfig` struct, add
  `diagnostics: Option<DiagnosticsConfig>` field to
  `ConfigFile`, update default fallbacks.

**Implementation guidance**:
- Take the stashed profiling.rs as the starting point.
- Add `static ENABLED: AtomicBool = AtomicBool::new(false);`
  near the top.
- Add `pub fn set_enabled(on: bool)` that stores into ENABLED
  AND flushes any buffered ticks (per the buffering design).
- Add an `enabled()` helper inline at every public entry point:
  `start`, `tick`, `record_scan_pattern`,
  `record_scan_pattern_all`, `dump_scan_stats`. Each returns
  early when disabled.
- For `tick`: when `gate_decided == false` (set_enabled hasn't
  been called yet), buffer the tick. After `set_enabled`,
  flush buffer based on `on` value.
- Add a new `record_scan_batch(n_patterns, n_hits, dur)`
  function (used in Step 4).
- Drop `log_first_musicdb_entry` — it was diagnostic-only and
  isn't called anywhere now.
- Keep `unix_millis()` and `elapsed_since_start()` un-gated.
- Update `ConfigFile::init`'s default-fallback paths to include
  `diagnostics: None`.

**Test requirements**:
- `cargo check --target x86_64-pc-windows-msvc` passes clean.
- Profiling code is dead (no call sites yet) — that's expected.
  Use `#[allow(dead_code)]` only if rust-analyzer warnings are
  noisy; the codebase has `#![allow(dead_code)]` at the crate
  level so this should already be silent.

**Demo**:
- `cargo check` succeeds.
- Inspecting `git diff` shows the new module + schema change.
- Adding `"diagnostics": { "profiling": true }` to a local
  mod-config.json validates as JSON.

---

## Step 2: Wire `profiling::tick` calls

**Objective**: Add `profiling::start()` at the top of
`lib.rs::init` and `profiling::tick(label)` at every existing
phase boundary. Add `profiling::set_enabled(...)` right after
`mods::config::init()`. Add per-mod-enable ticks via
`mod_trait::enable`. No scan instrumentation yet.

**Files touched**:
- `src/lib.rs` — add `profiling::start()` first; add
  `profiling::tick(...)` at every phase boundary (matching
  the diagnostic deploy from the prior feature, which we
  know was useful: module_load, resolve_all, resolve_derived,
  config_store, avs_layeredfs, widget_renderer,
  texture_resolver, asset_loader, afp_patcher, bm2d_api,
  series_filter_scroll, custom_options, options_scroll,
  custom_options_persistence, scene_manager, input_manager,
  judge_hook, register_*, enable_with_config,
  mod_menu_register_enable, init_complete). Add
  `profiling::dump_scan_stats()` at end. Add `set_enabled`
  call after `mods::config::init()`.
- `src/mods/mod_trait.rs` — add `profiling::tick("enable/{id}")`
  inside the public `enable(&mut self, id: &str)` after the
  `entry.mod_impl.enable()` call.

**Implementation guidance**:
- The phase tick labels should match the prior diagnostic
  deploy's labels exactly so old log files remain comparable.
- The `set_enabled` call is critical — if it's missed,
  buffered ticks for `module_load` and `resolve_all` are
  silently lost.
- `enable_with_config` already loops over enable; the per-mod
  tick happens automatically inside `enable` itself.

**Test requirements**:
- `cargo check` passes.
- Build, deploy, boot with `"diagnostics": { "profiling": true }`.
  Inspect log.txt: should see ~25 `[init-prof]` lines covering
  every phase, matching the format from the prior feature's
  diagnostic deploy.
- Boot again with the flag absent or `false`. Inspect log.txt:
  should see ZERO `[init-prof]` lines.
- Sanity check that the cabinet still boots normally on both
  configurations.

**Demo**:
- Two log files, side by side: one with profiling, one without.
- The "with" log shows phase timing comparable to the prior
  diagnostic deploy.

---

## Step 3: Add scan instrumentation hooks

**Objective**: Wrap `scan_pattern` and `scan_pattern_all` with
timing + record calls. Compares cleanly against post-scanner-
rewrite numbers.

**Files touched**:
- `src/core/scanner.rs` — split `scan_pattern` body into
  `scan_pattern_inner` (today's logic) plus a thin wrapper
  that records timing. Same for `scan_pattern_all`.

**Implementation guidance**:
- Identical to the diagnostic deploy from the prior feature.
- The `record_*` calls are no-ops when profiling is off, so
  this is safe to leave in production.

**Test requirements**:
- `cargo check` passes.
- Build, deploy, boot with profiling on. Inspect log.txt for
  the new `[init-prof] scan_pattern: N calls, total Xms,
  slowest Xms (...)` summary line near init_complete.
- Numbers should match the prior diagnostic deploy:
  ~119 ms total for `scan_pattern`, ~828 ms for
  `scan_pattern_all`. This establishes the **before**
  baseline for the scanner rewrite.

**Demo**:
- Log shows scan_pattern aggregate stats comparable to prior
  baseline (within noise).

---

## Step 4: Add aho-corasick dep + implement `scan_patterns_batch`

**Objective**: Add the new multi-pattern engine as a private
function. Existing call sites remain on the slow path; this
step is purely additive.

**Files touched**:
- `Cargo.toml` — add `aho-corasick = "1"` to `[dependencies]`.
- `src/core/scanner.rs` — add `scan_patterns_batch` function
  with the design's signature.

**Implementation guidance**:
- Helper: `extract_longest_literal_run(parsed: &[Option<u8>])
  -> Option<(start: usize, bytes: Vec<u8>)>`. Walks the
  parsed pattern, finds the longest run of `Some(byte)`
  values, returns the offset within the pattern and the
  literal bytes.
- Build the AC matcher: `AhoCorasickBuilder::new()
  .build(literal_runs)`. Use `MatchKind::Standard` (first-hit
  semantics within a needle).
- For each AC hit: look up the source pattern by needle ID,
  back-step to where the full pattern would start
  (hit_offset - run_offset_within_pattern), then verify the
  full pattern matches (including wildcards) at that
  position. If yes, record.
- Patterns with no run ≥ 2 bytes: fall back to scalar
  (`scan_pattern_inner` per pattern). Log a warning so the
  user knows the pattern is in slow mode.
- Wrap the body with `record_scan_batch` instrumentation.
- IMPORTANT: result is a `HashMap<String, ScanResult>` keyed
  by the caller's name strings, not a HashMap-indexed-by-
  pattern. The function takes `&[(&str, &str)]` —
  `(name, pattern)` pairs.

**Test requirements**:
- `cargo check` passes (adds the new dep).
- Build, deploy. With profiling on, log shows zero
  `scan_batch_calls` (no callers yet). Sanity check.
- (Optional) Add a test call site in a `#[cfg(test)]` module
  if convenient — not strictly required since the codebase
  has no test harness, but useful to validate correctness
  before exposing to production code paths in Step 7.

**Demo**:
- `cargo check` succeeds with the new dep.
- Build size delta is reasonable (~600 KB additional release
  output).

---

## Step 5: Rewrite `scan_pattern` and `scan_pattern_all` internals

**Objective**: Big-bang replacement of the byte-by-byte loops
inside `scan_pattern_inner` and `scan_pattern_all_inner` with
calls to a single-pattern variant of the new engine.

**Files touched**:
- `src/core/scanner.rs` — rewrite the `*_inner` function
  bodies to use Aho-Corasick over the longest literal run +
  full-pattern verification.

**Implementation guidance**:
- The single-pattern case is conceptually a 1-needle batch.
  Either:
  - (a) Implement `scan_pattern_inner` directly with a
    `memchr`/`memmem`-style search over the longest run +
    verify, since 1-needle is a degenerate case and avoiding
    AC's setup cost may be worthwhile.
  - (b) Just call `scan_patterns_batch(base, size,
    &[("", pattern)])` and pull the result.
- (b) is simpler and correct; (a) is only worth doing if
  measurement shows scalar cases dominating after the rewrite.
  Start with (b); fall back to (a) if profiling reveals an
  issue.
- For `scan_pattern_all`, similar approach but accumulate every
  verified hit. The batch API as designed returns first-hit
  per pattern, so `scan_pattern_all` either uses a different
  internal that runs AC and verifies all hits (preferred), or
  pre-fragments the search range and calls the first-hit
  variant repeatedly (less elegant).
- Use the inner of the AC matcher with `MatchKind::Standard`
  and iterate `matcher.find_iter(haystack)` collecting verified
  hits.

**Test requirements**:
- `cargo check` passes.
- Build, deploy with profiling on. Inspect log.txt:
  - `resolve_all` total (`[init-prof] scan_pattern: 47 calls`)
    drops dramatically. Expected: 119 ms → < 30 ms.
  - SongLimitExpansion's `scan_pattern_all` × 2 cost drops
    proportionally.
  - All mod registrations succeed (count of "Mod registered"
    lines unchanged from prior boot).
  - All hooks install (look for the various "hook installed"
    log lines).
- Smoke test: enter song-select, enter options, basic gameplay.

**Demo**:
- Log shows the speedup. Compare side-by-side with Step 3's
  baseline log.

---

## Step 6: Add `scan_xrefs_to_batch`; restructure `resolve_derived`

**Objective**: Land the batched xref walk for the three known
xref targets in `signatures::resolve_derived`.

**Files touched**:
- `src/core/scanner.rs` — add `scan_xrefs_to_batch`. Make
  `scan_xrefs_to` a thin wrapper around `_batch`.
- `src/core/signatures.rs` — restructure
  `resolve_derived` so the three derivation chains that need
  xref walks (folder_register, file_manager_load,
  metadata_insert) collect their targets first, do one
  `scan_xrefs_to_batch` call, and then run their per-chain
  derivation logic on the returned vec-of-vecs.

**Implementation guidance**:
- The current `resolve_derived` calls each derivation method
  in sequence. Each method that uses `scan_xrefs_to`
  currently does a single-target walk. We need to:
  1. Hoist the xref-collection step out of each
     derivation method into a new pre-pass at the top of
     `resolve_derived`.
  2. Pass the relevant pre-computed xref list into each
     derivation method.
- Two concrete options for the API:
  - **Pre-compute approach**: build a
    `HashMap<*const u8, Vec<*const u8>>` mapping target →
    xrefs at the top of `resolve_derived`, then derivation
    methods look up by target pointer.
  - **Targeted batch**: at the top of `resolve_derived`,
    collect targets that derivation methods will need (we
    know there are 3), call `scan_xrefs_to_batch`, then pass
    each result vec into the corresponding derivation
    method as a parameter.
- The pre-compute approach is simpler. The targets are
  already known at the start of `resolve_derived` because
  they're top-level signatures (folder_register,
  file_manager_load, metadata_insert) resolved by
  `resolve_all`.
- Keep `scan_xrefs_to` (single-target) as a public function
  for any future ad-hoc use, implemented as a thin wrapper:
  ```rust
  pub unsafe fn scan_xrefs_to(base, size, target) -> Vec<*const u8> {
      scan_xrefs_to_batch(base, size, &[target])
          .into_iter().next().unwrap_or_default()
  }
  ```

**Test requirements**:
- `cargo check` passes.
- Build, deploy with profiling on.
- Inspect `[init-prof] resolve_derived` time. Expected:
  729 ms → < 200 ms.
- Verify the three derivation chains still resolve correctly:
  - `folder_init (derived from folder_register xrefs)` log
    line still appears with a sensible address.
  - `file_manager_singleton (derived from file_manager_load xref)`
    log line still appears.
  - `string_assign (derived from metadata_insert xref pair-locality)`
    log line still appears.
- Smoke test: folder-expansion still works, custom options
  still register and persist.

**Demo**:
- Log shows resolve_derived under 200 ms.

---

## Step 7: Migrate `resolve_all` to use `scan_patterns_batch`

**Objective**: The largest single optimization site —
replace the 47-iteration scan loop with a single batch call.

**Files touched**:
- `src/core/signatures.rs::resolve_all` — replace the
  `for sig in SIGNATURES { match scan_pattern(...) { ... } }`
  loop with:
  ```rust
  let pattern_pairs: Vec<(&str, &str)> = SIGNATURES
      .iter()
      .map(|s| (s.name, s.pattern))
      .collect();
  let results = scan_patterns_batch(self.base, self.size, &pattern_pairs);
  // Then per-signature: look up name in results, store in self.resolved,
  // log "[+] name @ +0xN" or "[-] name -- pattern not found"
  ```

**Implementation guidance**:
- The per-signature log format must stay identical to today's
  ("[+] {} @ +0x{:X}" / "[-] {} -- pattern not found") so
  log-comparing tools still work.
- The order of log lines is no longer guaranteed by SIGNATURES
  array order if the HashMap iteration shuffles. Iterate
  `SIGNATURES` to drive the per-signature log to preserve
  order.
- This step's individual scan instrumentation
  (`record_scan_pattern`) goes silent because we're now
  using the batch path. The batch counter
  (`record_scan_batch`) takes over. The `dump_scan_stats`
  output should show the migration:
  ```
  [init-prof] scan_pattern: 0 calls
  [init-prof] scan_pattern_all: 2 calls (only SongLimit)
  [init-prof] scan_batch: 1 call, 47 patterns, M hits, Xms
  ```

**Test requirements**:
- `cargo check` passes.
- Build, deploy with profiling on.
- Inspect log:
  - All 47 `[+]`/`[-]` per-signature lines appear, in
    SIGNATURES array order.
  - `[init-prof] resolve_all` time dropped further from Step 5
    (we already did the per-pattern speedup; this batches the
    AC setup cost across all 47).
  - `[init-prof] scan_batch: 1 call, 47 patterns, ...`
    appears in the dump.
- Smoke test: every mod still registers/enables. All hooks
  install.

**Demo**:
- Log shows the 47-pattern batch happening in one shot.

---

## Step 8: Acceptance test

**Objective**: Validate all four R4 criteria from the design.

**Files touched**: None — deploy + observe.

**Implementation guidance**:
- Boot once with `"diagnostics": { "profiling": true }`.
  Capture log. Verify:
  - `[init-prof] resolve_all` < 30 ms.
  - `[init-prof] resolve_derived` < 200 ms.
  - `[init-prof] init_complete` elapsed < 1500 ms.
- Boot once without the flag. Capture log. Verify:
  - Zero `[init-prof]` lines anywhere in the file.
- Cabinet smoke test on both boots:
  - SongLimit: songs load, scene transitions to song-select.
  - folder-expansion: custom folders visible in song-select.
  - webui-options: options menu shows custom rows; values
    can be changed and persist.

**Test requirements**:
- All four R4 criteria pass.
- No mod-level regressions.
- No new error-level log lines.

**Demo**:
- Two log files (with/without profiling).
- Quick screenshots / video of cabinet smoke test.

---

## Notes on rollback

Each step is independently revertable. The biggest blast
radius is **Step 5** (rewrite of `scan_pattern` internals);
if the new engine has a regression, every signature
resolution would fail and the DLL wouldn't bring the game
up properly. Mitigation:

- Step 4 lands the engine as a separate function before
  Step 5 wires it in. Step 4 is testable in isolation.
- If Step 5's deploy reveals a regression, revert just
  `src/core/scanner.rs` to the previous commit. The new
  `scan_patterns_batch` from Step 4 stays — it's not
  called by anyone yet, so it's dead-but-correct code.

Steps 6 and 7 each touch `signatures.rs` in a way that's also
revertable per-file.
