# Rough Idea: Centralized Single-Pass Scanner + Permanent Profiling

## Project: 20260522-centralized-scanner
## Date captured: 2026-05-22

## Goal

Make DLL initialization scale gracefully as the number of mods
grows. The hook DLL today resolves ~50 AOB signatures during
init; the user plans to add **20+ more mods in the near future**,
and many of those will declare additional signatures or
derivation chains. We need a scan architecture that handles
hundreds of patterns without the cost growing linearly.

## Background — what we already know

The **20260522-dll-init-speedup** feature shipped a few hours
ago. From its empirical profiling deploy:

| Phase | Time |
|---|---:|
| `resolve_all` (47 patterns × `scan_pattern` each) | 119 ms |
| `resolve_derived` (multi-step chains, RTTI walks, xref scans) | **729 ms** ← dominant |
| `scan_pattern_all` aggregate (2 calls today) | 56 ms |

`resolve_derived` is 6× more expensive than `resolve_all`. The
slowest scan is `scan_pattern_all` with a low-uniqueness first
byte (e.g. `00 DC 49 00`) at ~47 ms per call. With 20+ new mods
adding signatures and (potentially) derivation chains:

- Linear growth: `resolve_all` could go from 119 ms → 250-400 ms.
- `resolve_derived` could go from 729 ms → 1500+ ms if new
  derivation chains follow today's patterns.
- Independent `scan_pattern_all` calls (like SongLimitExpansion)
  multiply directly with mod count.

## Proposed approach — three coordinated changes

### 1. Multi-pattern single-pass scanner

Replace the `scan_pattern` / `scan_pattern_all` byte-by-byte
loops with a single-pass **Aho-Corasick prefilter** over the
longest contiguous wildcard-free literal in each pattern,
followed by full-pattern verification on candidate hits.

- ~50 patterns × ~50 MB module = O(N×M) today
  (~120 ms measured).
- Multi-pattern single-pass is O(M + total_hits) (estimated
  ~10-15 ms regardless of pattern count up to several hundred).
- Aho-Corasick crate (MIT/Apache, mature, includes optional
  SIMD via Teddy submodule for ≤ ~100 needles).

### 2. Batched xref walk

`signatures::resolve_derived` currently calls `scan_xrefs_to`
three separate times, each O(M) over the module. Replace with a
single `scan_xrefs_to_batch(targets: &[*const u8])` that walks
the module once and routes each `CALL rel32` to whichever
target it matches.

- 3 calls today × ~50 ms each ≈ 150 ms.
- Single-pass batch ≈ ~50 ms regardless of how many targets.
- Drops in directly with no derivation-chain restructuring.

### 3. Permanent profiling instrumentation

The diagnostic profiling rolled back at the end of the prior
feature (currently in `git stash@{0}`). Restore it permanently
with the following changes:

- **Both** per-phase ticks AND per-scan aggregate stats are
  always available.
- **Gated** behind a single `mod-config.json` flag like
  `"profiling": true`. When false (default), zero log output.
- Build-time cost stays compile-only (no runtime branches if
  flag is off and the static is constant).

This gives us a measurement story for free on any boot — when
something regresses, the user flips the flag, redeploys, and
gets the data without rebuilding.

## Motivation

Operator quote: *"There are at least 20 different mods I plan on
adding in the very near future, I want this DLL to be as
scalable as possible with regards to the number of mods it can
handle."*

The current architecture works fine at 10 mods. It will visibly
hitch at ~30. We have the prior-art research already done
(`research/centralized-scanner-prior-art.md` from the prior
feature) — `aho-corasick` is the recommended dependency, with
literal-prefilter + verify as the recommended approach.

## Decisions already made (from initial Q&A)

- **Scope**: scanner + xref-batch + profiling restoration —
  all three.
- **Migration**: big-bang rewrite of `scan_pattern` /
  `scan_pattern_all` internals; existing call sites unchanged.
- **Profiling**: per-phase ticks AND per-scan aggregate, gated
  behind a `"profiling": true` flag in mod-config.json.

## Open questions still to resolve in idea-honing

- Aho-Corasick crate (`aho-corasick = "1"`) vs. hand-rolled
  multi-needle scanner.
- Where does the `"profiling"` flag live in the config schema?
  Top-level, or inside the existing `mods` block?
- Acceptance criteria: what timing improvements define "done"?
