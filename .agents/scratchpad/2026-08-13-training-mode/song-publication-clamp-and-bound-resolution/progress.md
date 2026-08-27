# Progress: song-publication-clamp-and-bound-resolution (Step 3, task-02)

Updated: 2026-08-13
Status: Complete (uncommitted — maintainer handles git; validated via harness
tests + `cargo check --target x86_64-pc-windows-msvc`; fmt/build.sh at the
Step-3 sequence end)

## Checklist

- [x] Explore (create detour anatomy, parse surface, fixture, sentinel decisions)
- [x] Tests first: selected_song_tests.rs (5 tests: never-published, round-trip
  + generation, torn-write guard via the exposed write halves, fixture-bank
  publication both entry orders at exactly 4096 ms, non-dance/corrupt ⇒ None)
  + section_math inline tests (8: clamp truncation incl. no-publication skip,
  zero rows, nominal formula, skip-past-end cap, omit-past-start MIN_SECTION
  floor, omit-only, floor-gives-way-at-chart-end normalization, dead chart end)
- [x] `src/services/song_rate/selected_song.rs`: `SelectedSongCell` seqlock
  (gen 0 = never / odd = writing / even = settled; bounded reader retries;
  write halves pub(crate) for the torn test), process static +
  `selected_song()`, `publication_from_bank` (dance-code filter →
  `parse_song_bank` → main-entry duration·1000/rate) + `publish_from_bank`
- [x] wavebank_hook: `publish_selected_song(file_id)` at the top of
  `create_hook` — fires on BOTH the degraded-identity and full-transaction
  paths, every create (armed or not); windows glue reads
  `file_table_path`/`file_table_source`; no logging, publish-nothing on any
  failure (previous publication stays, per the task text)
- [x] `src/mods/training_mode/section_math.rs`: `MIN_SECTION_MS = 5_000`
  (maintainer-approved), `effective_bound_seconds` (use-time audio cap,
  row value never rewritten), `resolve_bounds` (design §4.2 formula,
  0-sentinels; b ≥ chart_end normalizes to 0 = natural end)
- [x] bounds.rs: `ROW_A_MS`/`ROW_B_MS`/`CHART_END_MS` latches +
  `RESOLUTION_PENDING` + `SESSION_ACTIVE`; `clear_session_state` composes
  the Step-2 marker clear; GAMEPLAY entry queues the pending resolution
  (actors don't exist at the scene-change instant — `try_resolve_row_bounds`
  is retried per frame by task-03's driver); resolution = entered side
  (first side with a live `chart_end_raw`; armed sessions are never versus)
  → audio clamp → chart formula → `quantize_marker` → latch + seed live
  bounds + one INFO when nonzero; triple-5 → `restore_row_bounds` (restores
  row-derived; zero rows degenerate to Step-2 clear; toast "Restored
  markers"/"Cleared markers"); `set_marker` latches SESSION_ACTIVE;
  accessors `training_session_active()` / `row_derived_bounds()` for
  task-03 + Step 5
- [x] Harness mounts (`selected_song_tests` rides song_rate/mod.rs;
  `section_math` mounted as a `mods::training_mode` leaf) + gates:
  **229 passed / 0 failed** (216 → 229, +13), `cargo check` clean

## TDD cycles

1. selected_song_tests + section_math tests written → harness run failed to
   compile (module absent) → implementation → 229/229.
2. bounds/mod wiring (engine-facing, no host harness) → `cargo check` clean.

## Deviations

- The torn-read test drives the exposed `begin_write`/`finish_write` halves
  rather than a thread interleaving (deterministic; the seqlock is
  single-writer by construction).
- The effective audio clamp applies to BOTH rows (context.md assumption —
  harmless for omit, one helper).
- `restore_row_bounds` logs restore vs clear distinctly; no-change presses
  stay silent (Step-2 toast-gate convention).
- INFO visibility on cabinet arrives when task-03's driver retries the
  pending resolution (within-step ordering; the demo runs after task-03).
