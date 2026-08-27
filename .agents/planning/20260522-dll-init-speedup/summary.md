# Project Summary: 20260522-dll-init-speedup

## Artifacts produced

```
.agents/planning/20260522-dll-init-speedup/
├── rough-idea.md                              ← initial framing
├── idea-honing.md                             ← Q1–Q9 + empirical baseline
├── research/
│   ├── current-scan-architecture.md           ← scanner inventory
│   ├── scan-bottleneck-analysis.md            ← scan cost model
│   ├── init-flow-and-timing.md                ← DllMain → first hook trace
│   ├── time-critical-hooks.md                 ← race-sensitive inventory
│   ├── centralized-scanner-prior-art.md       ← Aho-Corasick / SIMD survey
│   └── thread-suspension-feasibility.md       ← Win32 freeze + prior art
├── design/
│   └── detailed-design.md                     ← R1–R5, components, data, errors
├── implementation/
│   └── plan.md                                ← 9 demoable steps + checklist
└── summary.md                                 ← this file
```

Diagnostic instrumentation that produced the empirical baseline
is preserved in `git stash@{0}` for re-application if future
re-measurement is needed.

## Empirical baseline

A diagnostic build deployed on the cabinet on 2026-05-22 produced
this timing profile (game version `MDX:J:F:A:2026042100`):

| Phase | Time | Cumulative |
|---|---:|---:|
| `module_load` | 1 ms | 1 ms |
| `resolve_all` | 118 ms | 119 ms |
| `early_song_limit_patch` (diagnostic) | 60 ms | **184 ms** ← race won here |
| `musicdb_parser` ENTERED (diagnostic) | — | **748 ms** ← race window: 564 ms |
| `resolve_derived` | 729 ms | 913 ms |
| services (sum) | 280 ms | ~1200 ms |
| mod registration | 100 ms | ~1300 ms |
| `enable_with_config` (slow mods) | 4109 ms | 5400 ms |
| `init_complete` | — | **5186 ms** |

Key takeaways:
- The race is solved by reordering alone — no thread suspension
  or scanner refactor is required.
- Scanning is *not* the dominant cost. Two specific mod enables
  (folder-expansion 1283 ms, webui-options 2482 ms) are.
- The webui-options time is an O(N²) atlas-rebuild bug, not a
  scan-time issue.
- folder-expansion regenerates the same on-disk assets every
  boot — cacheable.

## Design (high-level)

Five requirements (R1–R5) defined in `design/detailed-design.md`:

- **R1. Race fix via `early_apply` trait method.** New optional
  method on `Mod` that runs after `resolve_all` but before
  `resolve_derived`. `SongLimitExpansionMod` implements it.
  Config-gated.
- **R2. folder-expansion enable() output cache.** Skip on-disk
  asset regeneration when config + source ARC mtime match the
  cached state. ~1283 ms → ~10 ms on warm boots.
- **R3. Atlas-rebuild fix.** `register_label_for` becomes
  append-only; new `flush_label_atlas()` invoked once at end of
  init. ~2482 ms → ~400 ms.
- **R3a. Slow-mod reorder.** Move folder-expansion + webui-options
  to last in the enable order. Other hooks land sooner.
- **R4. Profiling instrumentation removed.** Diagnostic dropped
  from production; preserved in `stash@{0}`.
- **R5. Success criteria.** Race fix proven by ≥ 5 consecutive
  boots with > 2000-song musicdb; folder-expansion warm enable
  < 50 ms; webui-options enable < 600 ms; no mod-level
  regressions.

Architectural changes are minimal — three additions and one
reorder to the existing init flow:

```
DllMain spawn init thread
  → wait_for_game_module                      (unchanged)
  → resolve_all                               (unchanged)
  → mods::config::init                         ← MOVED EARLIER
  → ★ NEW PHASE: early_apply on each mod (config-gated)
  → resolve_derived                           (unchanged)
  → services init                             (unchanged)
  → register all mods                         (unchanged)
  → enable_with_config (★ slow mods last)
  → register + enable mod-menu                (unchanged)
  → ★ NEW: custom_options::flush_label_atlas() once
  → splash screen                             (unchanged)
```

## Implementation plan

Nine steps in `implementation/plan.md`, each ending in a
deploy + observe verification (no unit-test harness in this
codebase):

1. Add `Mod::early_apply` trait method + `EarlyContext`.
2. Move `mods::config::init()` earlier in `lib.rs::init`.
3. Implement `SongLimitExpansionMod::early_apply` + flag-based
   no-op in `init`/`enable`.
4. Reshape `lib.rs::init` to construct mods early + run
   early_apply phase. **Race fix lands here.**
5. Reorder slow mods last in `enable_with_config`.
6. Remove rebuild from `register_label_for`; add
   `flush_label_atlas` API.
7. Wire the flush call into `lib.rs::init`.
8. Add folder-expansion enable() output cache.
9. Final acceptance test on cabinet with high-song-count
   `musicdb.xml`.

Step 4 is load-bearing for the race fix; the rest is UX-quality
improvement.

## Areas that may need further refinement

- **`sha2` dependency audit** (Step 8): confirm whether the crate
  is already in `Cargo.toml`. If not, decide between adding it
  vs. using a smaller alternative for the cache-key hash.
- **Canonical `FolderConfig` serialization** (Step 8): the
  detailed design notes that the cache key requires deterministic
  bytes. If `FolderConfig` already serializes deterministically
  via `serde`, this is free; otherwise a small wrapper is needed.
- **Step 9 timing measurement**: requires re-applying the
  diagnostic instrumentation from `stash@{0}` for one boot.
  Recommend doing this in a topic branch and discarding after,
  to avoid polluting the production tree.
- **Cross-mod parallelization** of `enable()` was deferred (Q2).
  After Steps 5–8 ship, re-evaluate whether the remaining
  serial enable time is worth a parallelization pass.

## Next steps for the user

1. Review `design/detailed-design.md` and
   `implementation/plan.md`.
2. Begin implementation at **Step 1**. Each step is small and
   independently revertable.
3. After **Step 4**, the race fix is in production. The
   high-song-count musicdb crash should be eliminated even
   before the slow-mod speedups land.
4. The full sequence ends at Step 9 with the cabinet
   acceptance test.
