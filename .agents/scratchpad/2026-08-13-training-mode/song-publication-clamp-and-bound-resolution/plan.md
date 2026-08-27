# Plan: song-publication-clamp-and-bound-resolution

Status: Approved 2026-08-13 (verified upstream approval, auto mode — same
chain as task-01; no plan-gate halt points in this task).

## Test scenarios (first)

### selected_song_tests.rs (song_rate sibling, host-mounted via song_rate/mod.rs)

1. Never-published cell reads `None`.
2. Publish → read round-trip: digest + len + even generation; second
   publish bumps generation by 2 and the reader sees the new values.
3. Torn-read guard: a cell mid-write (odd generation, via the write halves)
   reads `None` — the reader can never observe a mixed generation (AC 1).
4. `publication_from_bank`:
   - `replay_fixture(false)` + path `data/sound/win/dance/tst1.xwb` →
     `(song_code_digest("tst1"), 4096)` (32 768 frames @ 8 kHz), and
     `replay_fixture(true)` (preview-first entry order) → identical.
   - non-dance path → None; dance path + corrupt bytes → None (⇒ the
     detour publishes nothing, previous publication stays).

### section_math tests (training_mode pure module, harness-mounted leaf)

5. Effective clamp: (599 s row, 90 000 ms audio) → 90; (30, 90 000) → 30;
   no publication (None) → row unchanged (AC 2's "audio cap skipped").
6. Resolution formula (`chart_end = 120_000`, MARGIN 1000, MIN_SECTION 5000):
   - zero rows ⇒ no bounds (a = 0, b = 0/none) (AC "zero rows").
   - skip 60 ⇒ a = 60 000; omit 30 ⇒ b = 90 000.
   - skip past end (skip 599 ⇒ a = 119 000 = end − MARGIN).
   - omit past start (omit 599 ⇒ b floored at a + MIN_SECTION).
   - MIN_SECTION floor with both rows set (skip 118, omit 118 on 120 s:
     a = 118 000 → b = min(max(2000, 123 000), 120 000) = 120 000 ⇒ none —
     the floor gives way to the chart end and normalizes to the sentinel).
   - omit-only (skip 0): a stays 0/none, b real (AC 5's OMIT LAST leg).

### Behavior covered on-cabinet (engine-facing, per repo law)

Publication from the live detour, resolution latching through the driver
(task-03), triple-5 restore toast, INFO log visibility (AC 3/4).

## Implementation approach

1. `selected_song.rs`: `SelectedSongCell` (AtomicU32 generation seqlock —
   0 = never, odd = writing; AtomicU64 digest; AtomicU32 len_ms), a process
   static + `publish`/`selected_song()` accessors,
   `publication_from_bank(path, bytes) -> Option<(u64, u32)>` (dance-code
   filter → parse → main-entry duration/rate → ms), and
   `publish_from_bank` composing the two. Write halves exposed
   `pub(crate)`-for-test so scenario 3 can freeze a torn state.
2. `wavebank_hook::create_hook`: `publish_selected_song(file_id)` at the
   top (both the degraded and full paths see it); windows-only glue reading
   `file_table_path`/`file_table_source`; honest cost note in the comment.
3. `section_math.rs`: `MIN_SECTION_MS`, `effective_bound_seconds(row_s,
   audio_len_ms: Option<u32>)`, `resolve_bounds(skip_s, omit_s,
   chart_end_ms, margin_ms) -> ResolvedBounds { a_ms, b_ms }` (0 sentinels,
   b normalized to 0 when == chart_end).
4. `bounds.rs`: `ROW_A_MS`/`ROW_B_MS`/`CHART_END_MS` latches,
   `SESSION_ACTIVE`, `RESOLUTION_PENDING`; entry/exit lifecycle;
   `try_resolve_row_bounds()` (entered side = first side with
   `chart_end_raw`, compose audio clamp → formula → `quantize_marker` →
   latch + one INFO; idempotent, cheap when not pending);
   triple-5 → `restore_row_bounds()`; `set_marker` sets SESSION_ACTIVE;
   `training_session_active()` accessor.
5. Harness: mount `section_math` (+tests) under `mods::training_mode`.

## Risks

- The publication runs on every bank create (small bounded work, loading
  screens only) — matches the task's "EVERY create" requirement; comment
  updated where the old zero-cost claim lived.
- Resolution stays pending until task-03's driver retries it per frame —
  within-step ordering, demo runs after task-03.
