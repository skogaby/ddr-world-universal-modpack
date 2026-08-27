# Rough Idea: DLL Init Speedup

## Project: 20260522-dll-init-speedup
## Date captured: 2026-05-22

## Goal

Make the DDR World hook DLL load (initialize) significantly more quickly so that
hooks are installed as early as possible in the game's lifecycle.

## Current state (as understood at idea capture)

- Each mod / hook installer in the codebase performs its own full memory scan.
- These scans currently run serially.
- This serial scanning is suspected to be the dominant cost of DLL init time.

## Proposed approach

Centralize memory scanning:

- Each mod / hook / service registers the AOB pattern(s) it cares about with a
  centralized memory scanner.
- Memory is scanned a *single* time once `gamemdx.dll` (and its sibling Konami
  DLLs) are detected and loaded.
- As each pattern is matched, the corresponding hook is installed *as soon as*
  its address is available, rather than waiting for the entire registration
  phase to complete.

## Motivation

The DLL needs to detour several functions extremely early in the game's
lifecycle. The most painful concrete example: expanding the size of the game's
internal music database so that operators can install custom songs.

Empirically, the game crashes ~75% of the time on bootup when `musicdb.xml`
contains more than roughly 2000 songs. That non-determinism strongly looks like
a timing race between when the game starts touching the music DB allocation and
when our hook gets installed. Any reduction in time-to-hook-installed should
reduce this crash rate (and ideally drive it to zero).

Goal: remove non-determinism. Get hooks installed as quickly and as
deterministically as possible.

## Open questions to resolve in idea-honing

- Which hooks specifically are time-critical (must be installed before the game
  reaches a particular code path)? Which are not?
- Is the bottleneck really the scans themselves, or also the work each mod does
  before/after scanning (RTTI walks, FFI table builds, etc.)?
- What's the right unit of registration? AOB pattern only, or pattern + post-
  match derivation step (e.g. "scan for X, then RIP-relative-decode at +5")?
- How do we handle dependencies between hooks (one mod needs another mod's
  resolved address before it can install its own hook)?
- How do we handle "this mod needs hook A *and* hook B before doing init work"?
- Parallelism: does scanning in parallel across modules / chunks of memory help,
  or is the win mostly from de-duplicating overlapping scans?
- Backward-compat with the existing `core/scanner.rs` and `core/signatures.rs`
  primitives — keep, extend, or replace?
