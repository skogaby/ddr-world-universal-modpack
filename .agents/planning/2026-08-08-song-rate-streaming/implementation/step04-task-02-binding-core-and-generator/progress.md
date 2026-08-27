# Progress — Step 4 task-02: Binding Runtime Core — Ring, Pending Slots, Generator, Serve Dispatch

Updated: 2026-08-10
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] 1. Data structures in `binding.rs` (BindingState, Ring, PendingSlot, Binding,
      epoch guard, retire/cancel, metrics) + focused protocol tests
- [x] 2. `generator.rs` (GeneratorCore + spawn + fault hook + metrics) + byte-equality
      tests vs the rebuilt oracle
- [x] 3. Serve dispatch + poll/consume (pure) + synchronous-pump replay tests
- [x] 4. Failure legs (silence-fill, behind-window regen, cancellation) + metrics
      exposure tests
- [x] 5. `mod.rs` + validator file-presence list updates; all five gates green; record
      closed

## What landed

- `src/services/song_rate/binding.rs` (+~700 lines): the streaming runtime core.
  `BindingState` (Active/SilenceFill/Retired), `Ring` (16 MiB production constant
  `RING_CAPACITY`; cursors `produced`/`consumed` as absolute virtual FILE offsets —
  one linear producer cursor over entry0 + gap + entry1; `rewinds` seqlock counter;
  `UnsafeCell` buffer + justified `unsafe impl Sync`), `PendingSlot` ×4 (SPSC:
  Free→Arming→Armed→Completing→Complete; buffer/accumulator raw ptrs; armed-at
  nanos), `Binding` (design shape + regen target atomic + pre-encoded per-entry
  silent blocks + reparsed formats + metrics + stop token), epoch guard
  (`reader_enter/exit`, `reclaim_eligible` = Retired ∧ readers==0), `retire()`
  (0-byte EOF-clamp cancellations), `enter_silence_fill()` (never overrides
  Retired; silence-completes armed slots), `metrics_snapshot()` (frames/wall/
  deferral count/max latency — task-03's drain logs these), fault knob
  `set_fault_kill_after_blocks`. The pure serve dispatch: `serve(offset, len,
  dest, accumulator)` → Served(n)/Pending/Refused with seqlock post-copy
  re-validation, full-request deferral, behind-window regen-target recording, and
  the stock accumulate-on-serve protocol; `poll(accumulator)` →
  Incomplete/Complete(reported-and-zeroed, slot freed)/NotPending. Both
  allocation/log/panic-free — task-04's detours call them verbatim. Producer
  surface pub(crate): `ring_append`/`ring_rewind`/`producer_complete_ready_slots`/
  `take_regen_target`/`pace_limit`/`armed_slot_pending`. Step-1 helpers +
  always-refusing `prepare_binding` + `integration_available()` (constant false)
  UNTOUCHED.
- `src/services/song_rate/generator.rs` (new, ~430 lines): `Feed` (the Step-3
  `EncodedFeed` productionized: BlockCachePcm → StretchState → whole-block
  accumulate → encode_block; `try_capture` loop-start checkpoint;
  `positioned_at` = restore-or-fresh + produce-and-discard to the block-aligned
  target — the `restore_at_block` mechanics), `GeneratorCore::step()` (stop token →
  regen target → complete ready slots → pace check (armed slots override) →
  bounded chunk production; Idle at end-of-stream — the thread stays alive for
  regeneration until the generation token stops it), `spawn` (`song-rate-generator`
  thread; `catch_unwind` → `enter_silence_fill` → `record_wall`). Pure — no
  logging macros anywhere.
- `src/services/song_rate/generator_tests.rs` (new, ~670 lines): fixture builder +
  whole-buffer oracle copies (core/xact test module not importable) + 7 tests.
- `mod.rs`: `pub mod generator;` + `#[cfg(test)] mod generator_tests;`.
- `scripts/validate_song_playback_speed.sh`: file-presence list gains
  `generator.rs generator_tests.rs` (plain bash area, not the unquoted heredoc).

## TDD cycles

1. Wrote `generator_tests.rs` (7 tests) + wired `mod.rs` → confirmed red
   (unresolved module + missing API).
2. Implemented binding runtime + generator → compile green; individual tests:
   deferral ✓, replay ✓, behind-window HANG.
3. Deadlock root cause: the fixed 16-block chunk (2240 B) overshot the whole
   2048 B test ring, sliding the window past the armed range every pass →
   endless regen ping-pong (and any single append > capacity self-corrupts).
   Fix: `produce_blocks` never exceeds its byte bound (excess whole blocks stay
   pending for the next call; `finished()` = done ∧ drained), chunk and gap
   writes bounded by `ring_capacity/4`, pump safety bounds tightened.
4. Full suite green: 106/106 in the fast harness (0.79 s), then 137/137 in the
   validator's cargo-test phase (7.97 s — well under the ~30 s budget).
5. Removed a genuinely-unused `std::io::Write` import from the new test file
   (the `write_song_bank_streaming` callback takes `&mut dyn Write`).

## Acceptance criteria → tests

1. Ring bytes == oracle (both orders × {50,175}, reparse + decode):
   `replay_through_the_serve_dispatch_matches_the_oracle`
2. Deferral exactly-once + stock accounting + slot freed:
   `deferred_read_completes_exactly_once_with_stock_accounting`
3. Behind-window deterministic regeneration:
   `behind_window_loop_restart_regenerates_identical_bytes`
4. Silence-fill keeps the stream valid (real thread + fault hook):
   `mid_stream_producer_death_switches_to_valid_silence`
5. Reclamation quiescence + 0-byte cancellations:
   `retire_cancels_pending_and_waits_for_reader_quiescence`
6. Gates green + identity-only unchanged: see below; tripwire suites ran
   unmodified inside the validator (`wavebank_hook_tests`,
   `availability_tests`); `integration_available()` still constant false.
   Plus thread lifecycle/metrics: `spawned_generator_covers_the_bank_and_records_metrics`,
   `core_idles_at_end_of_stream_and_stops_on_request`.

## Gates (all green, logs in `logs/`)

1. `./scripts/validate_song_playback_speed.sh` — validation passed; cargo-test
   phase 137/137 (was 130; +7) in 7.97 s (`validate_song_playback_speed.log`)
2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED (`validate_se_bank_synth.log`)
3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings (`cargo_check_windows.log`)
4. `cargo fmt` — whole crate, no churn (`cargo fmt --check` clean)
5. `./build.sh` — release DLL OK in 46.4 s (`build.log`)

## Deviations

- **Chunk bounding vs the design sketch (implementation detail, not a design
  deviation — field shapes/internal mechanics free per breakdown approval):**
  producer chunks are bounded by `min(16 blocks, ring_capacity/4)` and
  `produce_blocks` never overshoots its bound; required for correctness on any
  ring the chunk could out-race (found via the shrunken test ring; with the
  production 16 MiB ring the 16-block bound binds).
- `Ring.base` omitted (derivable as `produced − capacity` with the modulo
  mapping); a `rewinds` seqlock counter added instead for torn-read protection —
  recorded in context.md before implementation.
- Slot protocol gained two internal states beyond the sketch (Arming — CAS claim
  before field publication; Completing — exactly-one-completer claim among
  producer/silence-flip/retire-cancel) — protocol hardening, wire-visible
  behavior unchanged.
- Pre-existing (not this task): two `--tests`-profile unused-import warnings in
  `src/core/xact/tests.rs` (proven core, untouched). The standing gate — the
  plain windows check — is 0 warnings.

## Notes for siblings

- task-03 consumes: `Binding::new` (production capacity), `BindingError`,
  `retire`/`reclaim_eligible` (drain), `metrics_snapshot` (generation-end log),
  `set_fault_kill_after_blocks` (`mid-song-failure` selector), `generator::spawn`.
- task-04 consumes: `serve`/`poll` verbatim (detour bodies), `reader_enter/exit`
  if it holds ring-derived state across its own critical section, `ServeOutcome::
  Refused` = hard-fault leg (no free slot is structurally unreachable with the
  stock engine).
- Sibling task records outstanding for plan Step 4: task-03, task-04 (plan
  checkbox stays unticked).

Status: Complete (uncommitted — maintainer commits personally)
