# Project Summary: 20260522-centralized-scanner

## Artifacts produced

```
.agents/planning/20260522-centralized-scanner/
├── rough-idea.md                    ← scaling motivation (20+ mods)
├── idea-honing.md                   ← 6 Q's, decisions
├── design/
│   └── detailed-design.md           ← R1-R4, components, error handling
├── implementation/
│   └── plan.md                      ← 8 demoable steps + checklist
└── summary.md                       ← this file
```

The companion feature `../20260522-dll-init-speedup/` produced
the empirical baseline (`research/scan-bottleneck-analysis.md`,
`research/centralized-scanner-prior-art.md`) that informs this
design. The prior-art research and the cabinet timing data did
not need to be redone for this feature.

## What this feature does

Three coordinated changes, all in service of "let the DLL scale
to 100+ signatures without user-visible boot hitches":

1. **Multi-pattern single-pass scanner** — replace the byte-by-
   byte loop in `scan_pattern` / `scan_pattern_all` internals
   with an Aho-Corasick prefilter from the `aho-corasick` crate.
   Add a new `scan_patterns_batch` for callers (mainly
   `signatures::resolve_all`) that benefit from compiling many
   patterns at once.
2. **Batched xref walk** — replace 3 separate
   `scan_xrefs_to` calls in `signatures::resolve_derived` with
   one `scan_xrefs_to_batch`.
3. **Permanent profiling** — restore `core::profiling.rs` from
   stash, gated behind a new `"diagnostics": { "profiling":
   true }` mod-config flag. When disabled, every entry point
   short-circuits on one atomic load.

## Key design decisions

| # | Decision |
|---|---|
| Q1 | Land all three changes (scanner, xref-batch, profiling) in one feature. |
| Q2 | Big-bang rewrite of `scan_pattern*` internals; existing API unchanged. |
| Q3 | Profiling lives permanently in the codebase, gated by mod-config. |
| Q4 | Use `aho-corasick = "1"` crate. Saves ~200 lines of careful unsafe code. |
| Q5 | New top-level `diagnostics` config section, leaves room for future debug flags. |
| Q6 | Acceptance: resolve_all <30 ms, resolve_derived <200 ms, total init <1500 ms, zero overhead when disabled. |

## Implementation plan (high-level)

8 incremental steps. Profiling lands first so every subsequent
step has measurable before/after data.

1. Restore profiling.rs with the runtime gate; add
   DiagnosticsConfig.
2. Wire `tick` calls in lib.rs and mod_trait.
3. Add `record_scan_pattern*` instrumentation hooks. Capture
   baseline.
4. Add `aho-corasick` dep + new `scan_patterns_batch`.
5. Rewrite `scan_pattern` / `scan_pattern_all` internals.
6. Add `scan_xrefs_to_batch`; restructure `resolve_derived`.
7. Migrate `resolve_all` to batch all 47 patterns in one call.
8. Acceptance test on cabinet (with/without profiling).

Step 5 is the highest-blast-radius step. Step 4 lands the new
engine in isolation so a Step 5 regression is recoverable by
reverting just `src/core/scanner.rs`.

## Areas that may need refinement during implementation

- **`scan_pattern_all` semantics with Aho-Corasick** — the
  detail of "find every match" requires iterating AC hits and
  verifying each, vs. AC's default first-match-per-needle.
  The implementation will use `find_iter` over the literal-run
  needle and verify each candidate. Should be straightforward;
  flag if the API doesn't fit.
- **Patterns with no ≥ 2-byte literal run** — the design
  documents a slow-fallback path. Audit existing patterns
  during implementation; if any fall into this case, consider
  rephrasing them.
- **Buffered tick edge case** — first three ticks
  (module_load, resolve_all, config_store) fire before
  `set_enabled` is called. The buffering scheme handles this,
  but the implementation needs careful testing to verify the
  buffered ticks emit in the right order at flush time.
- **`retour 0.4.0-alpha.4` + `aho-corasick = "1"` toolchain
  compatibility** — both should work with the pinned nightly,
  but verify during Step 4.

## Next steps for the user

1. Review `design/detailed-design.md` and
   `implementation/plan.md`.
2. Begin implementation at Step 1. Each step is
   independently revertable.
3. After Step 7, run the acceptance test on the cabinet.
4. Add 20+ mods. Profile each new boot via the mod-config
   flag. Watch for regressions.

## Out of scope (deferred)

- RTTI walk optimization inside `resolve_derived`.
- Cross-mod parallelization of `enable()`.
- Memoizing scan results to disk across boots.
- Migrating SongLimitExpansion's local scans into the batch
  API.

These are tracked for future consideration but not load-bearing
for the "scale to 20+ mods" goal.
