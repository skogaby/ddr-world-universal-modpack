# Task: auto-sync rework — measure only, apply through the judgement offset, hold until measured

## Description
Rework the Step 12 auto-sync so that **no SSQ or audio file is ever rewritten**. The sweep
only measures; each song's correction is one number in `sync.bin` and is applied at play
time through the game's own per-player JUDGEMENT OFFSET (`Option+0x24`, `timing_music`) by
the `per_song_judgement_offsets` mod's existing override lifecycle, composed under any
per-song value the player set. A song selected before it was measured is measured at
song-selection time on a normal-priority thread, and the game's own stage load is **held**
(bounded) until that measurement is in, so two plays of one song in one session can never
have different sync.

## Background (maintainer decisions, 2026-10-24)
- "Anything that would change the state of a song between two plays in a single session is
  unacceptable." Rewriting SSQs mid-session did exactly that → dropped.
- The modpack already controls per-song sync via the game's judgement offset
  (`src/mods/per_song_judgement_offsets/`, default ON): a CSV baseline + per-player session
  maps; at a side's FIRST judge dispatch of a play `override_hook::on_judge` writes the
  song's value into `Option+0x24` (stock cached, restored at gameplay exit; `Priority::Early`
  so assist-tick sees it). Auto-sync must drive THAT field, the same way.
- Sign: `timing_music` positive = judged later (docs/per_song_judgement_offsets.md). The
  estimator's `correction_ms` is "move the chart later by this much" → **offset delta =
  +correction_ms** (whole ms, as `Outcome::Apply{delta_ms}`; `Unchanged` = 0).
- Composition: `applied = (player's per-song entry for the song, else the stock value) +
  sync_delta`, clamped to ±100. The delta is invisible in the per-song rows and is never
  written to the CSV or the profile.
- Hold until measured: the stage load may wait for the measurement; a visible stall on the
  decide/stage-load transition is acceptable; a hang is not (bounded waits).

## What exists now (the subagent's Step 12; read before changing)
- `src/core/sync/{mod,onset,estimate,events,timemap,measure}.rs` — KEEP as is
  (`measure::{SongAnalysis, measure_file, file_slots, declared_chart, first_anchor,
  tempo_tps}`; one onset analysis per song serves all its files).
- `src/services/stepmania/sync_state.rs` — `sync.bin` table keyed by FILE with `ssq_td0`,
  `reconcile_shift`, `shifted_td0`, `applied_delta`. **Simplify** to a per-SONG table (below).
- `src/services/stepmania/sync_sweep.rs` — thread, pause in 26–28, queue + `note_committed`,
  commit/reconcile/rename (**delete** all file-writing), `beat0_delta` (**delete**).
- `src/services/stepmania/mod.rs` — `auto_sync_enabled()`, `start_sync_sweep()`,
  `on_gameplay_load` → `note_committed`, `beat0_ms + beat0_delta` for the scroll remap
  (**revert** to plain `beat0_ms`).
- `src/services/stepmania/index.rs` — `Settings.auto_sync` + re-applying a recorded shift on
  re-conversion (**delete** the re-apply; `Settings.auto_sync` may stay or go).
- `src/mods/stepmania_packs/mod.rs` — `enable` starts the sweep (keep).
- `tools/sm_pack_check --auto-sync`, `scripts/validate_stepmania.sh` leg 18,
  `scripts/validate_sm_pack_check.sh` leg 3 (CLI parity) — keep; adjust to the new table if
  they touch it.

## Technical Requirements

1. **`sync.bin` = per-song measurements** (`sync_state.rs`, pure, std only): `basename →
   {source_fingerprint: u64, state: Measured{delta_ms: i8} | Refused{reason: String}}`;
   magic/version bump; malformed ⇒ empty; atomic save (`cache::write_atomic`). A song with
   several split files is measured per file but stored as ONE delta: the `<b>.ssq` file's
   (or the lowest `_N` when there is no base); if files disagree by > 2 ms log one WARN naming
   the song. A record is valid only while `source_fingerprint == SongEntry.fingerprint`; stale
   records are dropped at load. Drop `ssq_td0`, `reconcile_shift`, `shifted_td0`,
   `applied_delta`, `file_key`/`basename_of_key` if unused.

2. **Live delta map + freeze rule** (`sync_sweep.rs` or a new small `sync_apply.rs`):
   `basename → {delta: Option<i8>, frozen: bool}` behind one Mutex. API:
   - `publish(basename, delta)` — called by any measurement; if the entry is `frozen`, it
     does NOT change the live value (only `sync.bin`), and logs INFO `measured late; applies
     from the next boot`.
   - `delta_for_play(basename, wait: Duration) -> Option<i8>` — the per-song provider. If an
     unfrozen entry has no delta and a measurement for `basename` is in flight (or can be
     started), wait up to `wait` for it; then **freeze** the entry with whatever value it has
     (`Some(d)` or `None` = as-authored) and return it. Both sides and every later play in
     the session get the frozen value. `None` is also returned for non-injected basenames,
     `auto_sync` off, or a `Refused` song (frozen as `None`).
   - Load at `start_sync_sweep` (before any play): every valid `sync.bin` record populates the
     live map (`Measured` → `Some(d)`, `Refused` → `None`, both unfrozen).

3. **Per-song judgement composition** (`src/mods/per_song_judgement_offsets/`):
   - `override_hook`: add `pub fn set_song_delta_provider(f: fn(&str) -> Option<i8>)` (one
     static `OnceLock`/atomic fn pointer). In `on_judge`, after resolving `code`: `let entry =
     store lookup (Option<i8>)`, `let delta = provider(code)` (None when no provider). If both
     are `None` → return as today (stock applies, nothing written). Otherwise read stock (same
     ±100 sanity), `value = entry.unwrap_or(stock) + delta.unwrap_or(0)` clamped to ±100,
     write it, cache stock, set ACTIVE, log `… override applied ({value} ms = {entry|stock}
     {+delta} for '{code}')`. Restore/sweep/leak paths unchanged (they only need
     STOCK_CACHE/ACTIVE). The provider call is the ONLY place the game thread may block
     (≤ 3 s, requirement 5); keep the rest of `on_judge` allocation-free as today.
   - `store::arm_decision`/`row_seed`/UI: unchanged (the delta is invisible).
   - Pure test for the composition rule (entry/stock/delta/clamp) in a harness-mounted file
     (`store.rs` or a new `compose.rs`), run by `scripts/validate_judgement_offsets.sh`.
   - If the per-song mod is not active (`per_song_judgement_offsets::is_active()` false) when
     `start_sync_sweep` runs, log one WARN: auto-sync measured but cannot be applied.

4. **On-decide measurement** (normal priority, its own thread): trigger = the song committed
   from the wheel. Use the scheduler's dwell state at the scene 25 → 26 transition
   (`scheduler.rs::on_scene_change` knows `basename` when `prev == SONG_SELECT && next ==
   SONG_TO_STAGE_INTERSTITIAL`; add a hook/callback there) and, as the fallback that also
   covers paths without a highlight, `stepmania::on_gameplay_load` (full-bank request). If the
   song has no live delta and is not `Refused`/`audio_failed`, spawn (or wake) the on-decide
   measurer for it: decode (`decode_whole`), `SongAnalysis`, `measure_file` per file, then
   `publish` + save `sync.bin`. One in-flight set shared with the sweep so a song is never
   measured twice concurrently; the sweep must not START a new song while an on-decide
   measurement is pending (it finishes its current one, then yields). Measurement under
   `catch_unwind`; any failure publishes `Refused{reason}` so waiters stop waiting.

5. **Hold until measured** (two bounded waits, both only for injected songs with a
   measurement in flight):
   - **Stage load**: in `providers.rs`, when the FULL-variant bank is requested
     (`serve_bank(.., Full)` path — this runs on the FileManager I/O worker, never the render
     thread; assert with `widget_renderer::is_render_thread()` and skip the wait if it ever
     is), wait ≤ 5 s for the song's measurement before returning the path. Do not hold the
     preview variant, SSQ opens, jackets or `chart_length`.
   - **First judge**: `delta_for_play(basename, 3 s)` inside the provider (req. 3).
   - On expiry: proceed as-authored, freeze `None`, one WARN `StepMania[sync]: '<song>' not
     measured in time -- played as authored this session (result applies from next boot)`.

6. **Sweep** (`sync_sweep.rs`): keep the thread (below-normal; start after index complete +
   scene ≥ WARNING_SPLASH; pause in 26–28 between songs; `catch_unwind` per song). Per song:
   skip if a valid record exists or `audio_failed`; measure; `publish` + save. Remove every
   file write, the reconcile, `beat0_delta`, and the index re-apply. One INFO per song
   (`StepMania[sync]: '<pack>/<song>' measured <m:+.2> ms -> delta <d> ms` / `refused
   (<reason>)`), one summary per pass. `auto_sync: false` → no thread, no on-decide
   measurement, provider returns `None`, nothing written anywhere.

7. **Clean-up of the previous design**: `mod.rs` scroll-remap publish uses plain `beat0_ms`;
   index.rs no longer shifts; delete dead code and the old module header text; update
   `//!` docs of `sync_sweep.rs`, `sync_state.rs`, `stepmania/mod.rs` (Auto-sync paragraph),
   `per_song_judgement_offsets/{mod,override_hook}.rs` (composition + provider), and the
   `.agents/summary/interfaces.md` row if one mentions SSQ rewriting.

8. **Tests**: `sync_state` round trip + stale-fingerprint drop + malformed; freeze rule (pure:
   publish-before-freeze changes value, publish-after-freeze does not; `None` freeze;
   refused); composition rule; split-file delta pick + disagreement flag; keep leg 18 + the CLI
   parity leg green (they test measurement, which is unchanged). If a test needs the Mutex-
   backed map, put the decision logic in a pure struct and test that.

9. **Gates**: `cargo check --target x86_64-pc-windows-msvc` (no new warnings), `cargo fmt`
   (whole crate), `./build.sh`, `./build_win7.sh`, `./scripts/validate_stepmania.sh`,
   `./scripts/validate_sm_pack_check.sh`, `./scripts/validate_judgement_offsets.sh`,
   `./scripts/validate_fast_bootup.sh`, `./scripts/validate_split_ssq.sh`; path-leak grep over
   touched files; `grep -rn "shift_seconds" src/services` must return nothing.

## Acceptance Criteria
1. No code path under `src/services/stepmania/` or `src/mods/` writes to `_cache/stepmania/
   ssq/` after indexing (grep + review).
2. Cold cabinet: pick an unmeasured ITG song straight away → log shows the on-decide
   measurement, the stage load may stall briefly, the judge line shows `… = stock +9` (or the
   song's value), and `sync.bin` gains the record. Playing the same song again in the session
   logs the same applied value.
3. A song whose measurement is forced to be late (e.g. a 15-min stamina song) plays
   as-authored with the WARN, and a replay in the same session is also as-authored.
4. Warm boot: every measured song's judge line shows its delta from the first play; the sweep
   measures only the remainder.
5. `auto_sync: false`: no sweep, no on-decide thread, judge lines show plain per-song/stock
   behaviour.
6. `./scripts/validate_judgement_offsets.sh` and all previous harnesses green.

## Constraints
- Allowed files: `src/services/stepmania/{sync_sweep.rs, sync_state.rs, sync_apply.rs (new,
  optional), mod.rs, index.rs, providers.rs, scheduler.rs, taint.rs}`, `src/mods/
  per_song_judgement_offsets/{mod.rs, override_hook.rs, store.rs, compose.rs (new, optional)}`,
  `src/mods/stepmania_packs/mod.rs`, `src/core/sync/**` (only if a measurement helper is
  missing), `tools/sm_pack_check/**`, `scripts/validate_stepmania.sh`,
  `scripts/validate_sm_pack_check.sh`, `scripts/validate_judgement_offsets.sh`,
  `.agents/summary/interfaces.md` (one row). Nothing else without reporting why.
- Do NOT install new detours. Do NOT touch `fast_bootup`, `avs_layeredfs/file_hooks.rs`,
  `score_guard`. Never `git commit`/push. Do not deploy or write under `$DDR_WORLD_INSTALL`.

## Metadata
- **Complexity**: High
- **Labels**: auto-sync, stepmania, judgement-offset
- **Generated By**: orchestrating agent 2026-10-24 (supersedes task-01 for the apply side)
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md — Step 12
