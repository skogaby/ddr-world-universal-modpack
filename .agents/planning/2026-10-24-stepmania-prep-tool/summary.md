# Summary — StepMania Prep Tool

Planned 2026-10-24; design and plan approved the same day.

## Artifacts

| file | content |
|---|---|
| `rough-idea.md` | the idea and the maintainer's revision (tool replaces the in-game index) |
| `research/orientation.md` | cache layout and ownership today, warm/cold costs, which DLL files are mountable, updater patterns to inherit |
| `idea-honing.md` | decision register D1–D16 (12 accepted, 4 assumed), Readiness Confirmed |
| `design/detailed-design.md` | approved design — tool crate, slim DLL reader, cache ownership split, validation, error handling, test matrix |
| `implementation/plan.md` | approved 5-step plan with checklist |
| `progress.md` | to be created at the start of implementation (AGENTS.md: live resume point) |

## Design in brief

`ddr_world_hook_prep.exe` (crate `prep/`, built like `updater/`) is the only producer of the
StepMania cache: it indexes `stepmania/Songs/`, converts with the DLL's own mounted
pipeline (moved `index.rs`), checkpoints `index.bin`, reports the injection ceiling,
measures auto-sync in parallel for the songs the ceiling admits, and writes `sync.bin`. It
refuses to run while the game runs and absorbs `tools/sm_pack_check` as `--dry-run`.

The DLL loads `index.bin` at `early_init`, validates records read-only on a thread (the
`startup.arc` fingerprint waits, 15 s cap), injects under the ceiling, and applies sync
deltas from the ledger. It no longer discovers, converts, measures, sweeps, holds at stage
load, or writes any file the tool reads. `index_timeout_s` is retired.

## Plan in brief

1. `prep/` skeleton + dry-run port (sm_pack_check deleted)
2. Indexer moves into the tool; tool writes the warm cache (DLL untouched)
3. DLL becomes a reader; index thread/timeout removed; `validation.rs`
4. Sync measurement moves; sweep/on-decide/hold removed
5. Release plumbing, README/`gamestart.bat`, `songs_readme.txt`, learnings, summary refresh

## Next steps

1. Maintainer commits the current tree (the loader feature's ~70 paths plus the 2026-10-24
   ceiling/arena guard rails) before the refactor starts.
2. Run the code-task-generator sop against `implementation/plan.md` (one step at a time),
   then code-assist on each task in order; maintain `progress.md` in this directory.

## Watch items

- Step 2 is the step most likely to surface a purity problem when mounting `index.rs` /
  `cache.rs`; keep the DLL's own index in place until Step 3 so the cabinet keeps working.
- The 15 s validation cap and the 64 MiB arena are untested on a cabinet; the first Step 3
  deploy verifies both.
- `EXPANDED_ARENA_BYTES` moves to `ceiling.rs` in Step 2 so the tool and the DLL share it.
- Cabinet matrix (design §7) includes a 14k synthetic library — the scenario that started
  this work.
