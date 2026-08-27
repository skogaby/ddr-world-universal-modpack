# Idea Honing: Centralized Scanner + Permanent Profiling

> Q&A log. Decisions are recorded as they're made.

## Empirical baseline (from prior feature, 2026-05-22)

```
resolve_all                      119ms     47 patterns × scan_pattern serial
resolve_derived                  729ms     multi-step chains (RTTI, xref, RIP-rel)
song_limit/init/alloc_scan        27ms     scan_pattern_all
song_limit/init/read_scan         29ms     scan_pattern_all
scan_pattern total                119ms    47 calls, slowest 25ms
scan_pattern_all total            828ms    29 calls, slowest 47ms ("00 DC 49 00")
Total warm-boot init             ~2000ms
```

Note: `scan_pattern_all`'s 828ms total / 29 calls includes calls
made from inside `resolve_derived` (xref walks, multi-match
patterns); the 56ms in song-limit-expansion is its own scan
pair.

## Questions

### Q1: Feature scope

**Decision**: Scope = scanner + xref-batch + profiling
restoration. All three land together in one feature. Profiling
is the validation mechanism for the other two.

### Q2: Migration approach for the multi-pattern scanner

**Decision**: Big-bang rewrite of `scan_pattern` /
`scan_pattern_all` internals. Existing call sites unchanged.
The new internal implementation uses a multi-pattern engine
internally; from a caller's perspective the API is identical.

**Caveat**: `scan_pattern_all` returning `Vec<ScanResult>`
means we need to support multi-match within a pattern (today's
SongLimitExpansion case finds 3 hits per pattern). The
single-pattern multi-match case still works fine with
Aho-Corasick prefilter — the prefilter finds all candidate
hits and verifier confirms each.

### Q3: Profiling permanence

**Decision**: Both per-phase ticks AND per-scan aggregate,
gated behind a single `"profiling": true` flag in
`mod-config.json`. When the flag is absent or false, no
`[init-prof]` lines are emitted.

The instrumentation lives permanently in the codebase. A
`static AtomicBool` initialized at config-load time gates every
`profiling::tick(...)` and `profiling::record_scan_pattern(...)`
call.

### Q4: Aho-Corasick crate vs. hand-rolled

**Decision**: Use the `aho-corasick = "1"` crate. Mature
(used by ripgrep), MIT/Apache, optional SIMD via Teddy
submodule for ≤ 100 needles. Saves ~200 lines of careful
unsafe scanning code we'd otherwise hand-roll. Trade-off
acceptable: one more dep, ~600 KB compile-time impact.

### Q5: `profiling` flag location

**Decision**: New top-level `diagnostics` section:
```json
{
  "mods": { ... },
  "diagnostics": {
    "profiling": true
  }
}
```

Reserves room for future debug toggles (verbose logging,
per-mod debug flags, scanner-internal counters, etc.) without
polluting the per-mod block or the top-level keys.

### Q6: Acceptance criteria

**All four criteria must hold**:

1. `resolve_all` < 30 ms (down from 119 ms; ~4× speedup).
2. `resolve_derived` < 200 ms (down from 729 ms; ~3.6×
   speedup, mostly from the batched xref walk).
3. Profiling adds zero overhead when disabled — verified by
   `log.txt` containing no `[init-prof]` lines on a boot
   without the flag.
4. Total init time on warm boot < 1500 ms (down from ~2000 ms).

Verified empirically on the cabinet by running with
`"profiling": true`, capturing the log, and reading off the
phase timing.

## Constraints inherited from the codebase

(From `CLAUDE.md`, applicable to this work specifically):

- Hot-path scan code is allowed `unsafe`, but blocks must stay
  narrow. The new multi-pattern scanner reads `&[u8]` slices,
  so the unsafe surface is limited to the
  `slice::from_raw_parts(base, size)` boundary.
- No `unwrap`/`expect`/`unreachable!` in the scanner itself.
  The new code can use `.ok()?` and `Result` returns.
- Logging via `log_info!` / `log_warn!` only — never `println!`.
- Stay inside the existing module layout. The new scanner
  internals live in `src/core/scanner.rs`. The profiling
  module lives in `src/core/profiling.rs` (recreated from
  `git stash@{0}`).
- Match `retour`'s nightly toolchain — already pinned.
