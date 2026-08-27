# Task: Restart executor + debounce + watchdog (live-edit preview restarts)

## Description

Completes the feature: editing SONG SPEED or PRESERVE SONG PITCH while a
preview plays restarts it at the new settings ~150 ms (debounced) after
the last tick, and the preview play watchdog makes pitch-preserved
previews reliable (deploy-#1 fix). Adds the `RefreshCell` debounce, the
game-thread restart executor + watchdog on a new input-manager per-frame
callback, un-stubs `preview::request_refresh`, and ends in cabinet
deploy #2 (full C1–C9 matrix).

## Background

Step 4 stashed the restart half's addresses (`init_restart` /
`restart_available` / `resolve_loader` + `loader_sane` in `preview.rs`).
Research findings that shape this step:

- **No per-frame poll registration exists.** `input_manager::poll()` runs
  once per render frame (driven by widget_renderer's `wrapper_render`
  detour, the thread quick_logout et al. call game APIs from) but only
  dispatches edge-triggered input events. ADD a minimal frame-callback
  API: `on_frame(Arc<dyn Fn() + Send + Sync>) -> usize` +
  `remove_frame_callback(id)`, dispatched at the TOP of `poll()` (before
  the ark-exports gate, so the executor runs even if ark I/O init
  failed), callbacks snapshotted out of the lock and each wrapped in
  `catch_unwind` (the scene_manager dispatch pattern).
- **The unregister entry needs stashing too** (5th pointer): the restart
  must call the PATCHED game entries so the detour prelude retires the
  preview binding — `GenericDetour::call` bypasses the detour, so the
  executor calls the raw `song_rate_wavebank_unregister` address
  (`unsafe extern "C" fn(i32)`); the re-create goes through
  `sound_bank_create_router` (`unsafe extern "C" fn(i32) -> u8`) whose
  XWB arm lands on the detoured `wavebank_create`. `cue_handle_stop` is
  `unsafe extern "C" fn(i32)`.
- **Supersession source**: `selected_song::selected_song() ->
  Option<SelectedSongInfo>` (atomics-only seqlock read — legal in option
  callbacks) carries a monotonic `generation: u32` republished on every
  dance-bank create.
- **Watchdog watermark**: compose from `Binding::ring_produced()` ≥
  `min(target_data_start() + 0x10000, target_data_end())` (both
  pub(crate); production starts at `target_data_start`). 0x10000 = the
  engine's fixed first ADPCM read (RE §8).
- **Time**: no QPC wrapper exists; the repo idiom is an `Instant` epoch +
  elapsed nanos in an `AtomicU64` (BankTimeline / Binding metrics).
- **Cue string**: copy `song_reset::read_msvc_string` (buf/ptr +0, len
  +0x10, cap +0x18, heap when cap ≥ 0x10, sanity-capped) and apply at
  `loader+0x48`; preview cues end `_s`.
- **Loader tick gate values**: row load-state ∈ {0, 5, 6, 8} (RE §1.3);
  `wavebank_hook::file_table_state(file_id)` reads it.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-15-song-preview-rate/design/detailed-design.md
  (§Architecture Flow 2, §Components 5 — executor steps 0–5 AND the
  watchdog amendment paragraph, §Components 7, §Data Models
  RefreshCell, §Error Handling, §Testing C1–C9)
- RE: .agents/planning/2026-08-15-song-preview-rate/research/preview-retrigger-re.md
  (§1.3 loader layout + tick gates, §2 why the façade can't re-trigger,
  §3 the restart steps, §8 the watchdog rationale)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `src/services/input_manager.rs`: `on_frame` / `remove_frame_callback`
   per the Background sketch. Idle cost when none registered: one
   uncontended lock + empty check per frame. Dispatch order: frame
   callbacks first, then the existing poll gates.
2. `preview.rs` — `RefreshCell` (cfg-free, host-tested; design §Data
   Models): `{requested: AtomicBool, stamp_nanos: AtomicU64,
   settle_generation: AtomicU32}` with injectable-time methods
   `stamp_at(now_nanos, settle_generation)` and `poll_at(now_nanos,
   scene, current_settle_generation) -> RefreshPoll {Idle, Pending,
   SceneCleared, Superseded, Fire}`:
   - not requested ⇒ Idle;
   - requested && scene ≠ SONG_SELECT ⇒ clear + SceneCleared (C9 —
     a stale edit must not restart after returning to select);
   - < 150 ms since stamp ⇒ Pending;
   - publication generation ≠ latched ⇒ clear + Superseded (a settle
     already created at the new values);
   - else clear + Fire. Document the benign stamp-vs-clear race (a
     re-edit racing the clear loses at most one restart; fail-open).
3. `request_refresh()` un-stubbed (cfg-free): latch
   `selected_song().map(|i| i.generation).unwrap_or(0)` + stamp with
   nanos from a module `OnceLock<Instant>` epoch. Atomics-only.
   `set_feature_active(false)` additionally clears the cell.
4. `init_restart` gains the 5th pointer (`song_rate_wavebank_unregister`
   — still all-or-nothing); `preview::init()` additionally registers the
   frame callback once (idempotent latch, `input_manager::on_frame`).
5. Restart sequence as a host-testable seam (cfg-free): `trait RestartIo
   { stop_cue(handle); unregister(file_id); create(file_id) -> bool;
   rearm_loader(); }` + `run_restart_sequence(&LoaderSnapshot, &mut dyn
   RestartIo) -> RestartOutcome`: stop iff handle ≠ −1 → unregister XSB
   then XWB (stock order) → create XWB then XSB (abort on first create
   failure, no re-arm — the loader keeps its stopped state, silent
   fail-open) → re-arm. Windows impl calls the stashed fns; re-arm =
   `handle = −1; failed = 0` writes on the loader.
6. Executor frame body (windows): `feature_active` gate → `poll_at`
   (Fire ⇒ restart) → watchdog step. Restart preconditions at fire time
   (design step 1): `resolve_loader` + `loader_sane` + both rows'
   `file_table_state` ∈ {0,5,6,8} + cue read at `loader+0x48` ends
   `_s`. Precondition failures: request already cleared, one latched
   WARN per class (chain / rows / cue / create-failed — an AtomicU32
   bitmask); successful restarts log one INFO (user-triggered, bounded
   by human input). Pure predicates host-tested: `row_state_loaded`,
   `cue_is_preview`.
7. Watchdog step (design §Components 5 amendment): while
   `restart_available()` and a LIVE preview binding's
   `ring_produced() ≥ watchdog_cover(start, end)` (pure helper,
   host-tested: `min(start + 0x10000, end)`): resolve the loader chain;
   require `loader_sane`, `snapshot.xwb_id == binding.file_id()`, and
   `snapshot.failed`; then re-arm (`failed = 0; handle = −1`) and latch
   the binding's generation (`WATCHDOG_RETRIED_GENERATION` — ONE retry
   per preview generation). One INFO per re-arm.
8. Zero-footprint audit: idle frame cost = one `feature_active` load +
   one `requested` load + one `with_preview` Acquire + the
   `restart_available` loads; no allocation, no locks beyond the frame
   dispatch's own.
9. Both cfg targets compile; validator harness runs the new host tests;
   existing suites unchanged.

## Dependencies

- Step 4 (derivations + `resolve_loader` + `loader_sane`) — complete on
  the tree, gates green.

## Implementation Approach

1. Host tests first (`preview_tests.rs`): RefreshCell matrix
   (stamp/coalesce/fire/clear, scene-gate suppression, supersession),
   restart-sequence ordering + create-failure abort under a recording
   mock `RestartIo`, `watchdog_cover` / `row_state_loaded` /
   `cue_is_preview`.
2. `input_manager::on_frame`; preview.rs executor + watchdog + wiring;
   5th stash pointer.
3. Full gates (validator, windows check, whole-crate fmt, build.sh).
4. Hand to the maintainer for cabinet deploy #2 with the full C1–C9
   matrix (design §Testing) + the Step-4 demo lines (derivation resolves
   at boot, chain-probe INFO, restart-half INFO at enable) + the
   watchdog fix check: pitch-preserved previews at 75 % reliably audible
   (~0.6 s late is ACCEPTED), resample previews unchanged.

## Acceptance Criteria

1. **Debounce semantics**
   - Given rapid value ticks within 150 ms of each other
   - When the executor polls each frame
   - Then exactly one Fire results, ≥ 150 ms after the LAST tick (C8),
     and a wheel settle during the window suppresses it (Superseded)

2. **Scene-gate decline**
   - Given an edit followed by an immediate song confirm (C9)
   - When the scene leaves SONG_SELECT before the debounce elapses
   - Then the request clears silently, no restart runs, gameplay is
     untouched

3. **Restart ordering**
   - Given a Fire with sane preconditions
   - When the sequence runs (mock-recorded on host)
   - Then the order is stop(handle ≠ −1 only) → unregister(xsb) →
     unregister(xwb) → create(xwb) → create(xsb) → re-arm, aborting
     without re-arm on the first create failure

4. **Watchdog single retry**
   - Given a failed-latched loader and a live preview binding whose
     produced watermark covers `min(start+0x10000, end)`
   - When the watchdog steps
   - Then exactly one re-arm happens for that preview generation
     (repeat frames and repeat failures do not retry again)

5. **Cabinet demo (deploy #2)**
   - Given the full C1–C9 matrix from the design
   - When executed on the cabinet
   - Then every row passes — notably C2 (single restart ~150 ms after
     the last tick), C3 (DSP switch restarts), C4 (100 % restores stock),
     C5/C6/C1 regressions clean — and pitch-preserved previews are
     reliably audible across a long browse session (the deploy-#1 bug
     fixed by the watchdog)

## Metadata

- **Complexity**: High
- **Labels**: song-rate, preview, executor, debounce, watchdog, cabinet
- **Required Skills**: Rust, detour-context law, the preview restart RE, game-thread discipline
- **Generated By**: code-task-generator 2026-08-16
- **Source Plan**: .agents/planning/2026-08-15-song-preview-rate/implementation/plan.md
- **Plan Step**: Step 5: Restart executor + debounce + wiring (cabinet deploy #2 — full matrix)
