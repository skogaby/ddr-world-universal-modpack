# Progress — Step 4 task-03: Binding Preflight and the Reworked Create/Unregister Transaction

Updated: 2026-08-10
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] 1. `binding.rs` pure layer: SourceView, real BindRefusal variants, real
      `prepare_binding` pipeline, `qualify_bind` (+`bind_may_qualify` cheap gate),
      `BindingRegistry` + static, generation u32→u64 widen; refusal-leg + registry +
      qualify tests (T1–T5)
- [x] 2. `transaction.rs`: `BindOutcome` + bind injection surface; existing matrix
      proven unchanged with `|_| BindOutcome::Stock` (T6); bind-ordering + fault
      parse tests (T6b, T6c)
- [x] 3. `wavebank_hook.rs`: cfg-agnostic `BindContext`/`bind_for_create`/
      `unregister_prelude`/`retire_after_create` + windows glue (file-table stash,
      create/unregister hooks); composition tests (T7–T12)
- [x] 4. `xact_runtime::finish_quarantine` + test (T13); `runtime.rs` drain leg
      (event consumption → slot recycling, registry sweep + metrics log, refusal
      WARNs)
- [x] 5. Full gate set green; record closed

## What landed

- `src/services/song_rate/binding.rs`: `BindRefusal` grew the real typed variants
  (SourceRead/UnsupportedProfile/Plan/HeaderSynth/SourceCopy/ProducerStart/
  SlotExpose/Injected, each with a mailbox wire code; `IntegrationAbsent` deleted);
  `SourceView<'_>` (host buffers; `#[cfg(windows)] unsafe from_raw` over the
  FileManager row); the real `prepare_binding(file_id, generation, percent, view,
  fault)` pipeline in the design's order (fault bind-refused/source-read → parse →
  plan (PreData→HeaderSynth, else→Plan) → fault header-synth → try_reserve private
  copy → `Binding::new` → fault mid-song arms `set_fault_kill_after_blocks(64)` →
  fault generator-start → `generator::spawn`, retiring on spawn failure);
  `bind_may_qualify` (one atomic load — the ordinary-create fast path) +
  `qualify_bind` (Armed/Committed + song-digest gate; Armed+non-dance declines
  SILENTLY; Committed+same-digest = QuickRestart); `BindingRegistry`
  (const-constructible: active `AtomicPtr` from `Arc::into_raw`, 4-slot retired
  list, per-slot cooldown (init 2, decremented per eligible sweep, freed on the
  second — ≥1 extra 250 ms drain tick of grace after eligibility, on top of the
  unpublish→eligibility gap; the epoch-reclamation safety argument for the
  lock-free `with_active` reads), coalescing refusal mailbox
  `note_refusal`/`take_refusal`) + `static REGISTRY`/`registry()`.
  `Binding::generation` widened u32→u64 (matches the lifecycle + RedirectToken).
  `integration_available()` UNTOUCHED (constant false — task-04 flips it).
- `src/services/song_rate/transaction.rs`: `BindOutcome {Stock, Bound, Refused}`;
  `call_create` gained the pre-original `bind: FnOnce(i32) -> BindOutcome` — runs
  inside the EXISTING pre-original `catch_unwind` after the fault check, only when
  a TLS frame exists (overflow ⇒ no bind); a bind panic clears the frame → stock
  leg (or the existing conservative recovery if a slot was already exposed).
  Post-original code byte-for-byte untouched. `FaultSelector` gained the five
  streaming bools + parse legs (`source-read`, `header-synth`, `generator-start`,
  `bind-refused`, `mid-song-failure`).
- `src/services/song_rate/wavebank_hook.rs`: cfg-agnostic composition —
  `BindContext`, `bind_for_create` (qualify → digest bind + `begin_binding` /
  QR-no-phase-change → preflight → claim+expose+attach as the LAST fallible tail
  (each failure retires the fresh binding) → `mark_exposed`/`mark_reexposed` →
  registry publish → Bound; FirstBind refusal ⇒ mailbox + `mark_early_failed`;
  QR refusal ⇒ mailbox + `publication.reset_identity()`, phase stays Committed),
  `unregister_prelude` (pre-original retire + `begin_release_by_file` +
  ReclaimBinding enqueue — atomics-only), `retire_after_create`
  (LateFailed/RecoveryFailed after a bound call ⇒ retire, atomics-only). Windows:
  `FILE_TABLE` stash at init (task-01's `song_rate_file_table`),
  `file_table_source`/`file_table_path` row resolution (null-guarded at every hop;
  path capped at 0x7E inline bytes per the +0x8F SSO flag), `create_hook` builds
  the real closure over the process statics (identity fallback preserved;
  `retire_after_create` post-call), `unregister_hook` runs the prelude
  PRE-original (design req 26).
- `src/services/song_rate/xact_runtime.rs`: `finish_quarantine` (mirror of
  `finish_release` from Quarantined) — the late-fail slot recycling half.
- `src/services/song_rate/runtime.rs`: the 250 ms drain gained
  `drain_binding_maintenance()` — consumes `ReclaimBinding` events
  (ReleasePending→`finish_release`, Quarantined→`finish_quarantine`), sweeps the
  registry (buffers freed ONLY at `Retired ∧ readers==0` after the cooldown; one
  INFO per reclaimed generation with frames/wall/deferral/max-latency — plan
  Step 5's benchmark inputs), and emits the coalesced refusal WARN (preflight and
  detours never log).
- Tests: `binding_tests.rs` rewritten (refusal legs, qualify matrix, live-binding
  success pipeline, mid-song fault arm → real SilenceFill, registry quiescence/
  cancellation/mailbox); `transaction_tests.rs` (11 call sites gained
  `|_| BindOutcome::Stock` with every assertion IDENTICAL, + bind-ordering/
  bind-panic/pre-original-skip tests, + the 10-leg fault parse matrix with
  `source-read` moved to the positives); `wavebank_hook_tests.rs` `composition`
  module (T7–T12: refusal→EarlyFailed+stock+one note, missing-source→SourceRead,
  non-dance-while-Armed silent decline, bind→commit with Q31 last (ordering
  probe), create-failure-after-bind→retire+LateFailed+no-Q31, unregister-with-
  pending→clamp cancellation+ReleasePending+reclaim-at-quiescence, Quick-Restart
  re-bind (same generation, offset-zero header serve, ledger count still 1), QR
  refusal→Committed+identity-clock); `xact_runtime_tests.rs` finish_quarantine;
  `generator_tests.rs` fixture helpers made `pub(super)` (shared with
  binding_tests/wavebank_hook_tests — no third copy).
- The two Step-1 TRIPWIRES ran UNMODIFIED
  (`identity_base_readiness_is_structurally_false_without_the_binding_integration`,
  `availability_tests`); no validator edits (no new module files; zero references
  to the old stub names in the harness).

## TDD cycles

1. Baseline: fast harness 106/106; widened `Binding::generation`, replaced the
   stub `prepare_binding`/`BindRefusal`, added FaultSelector legs + `BindOutcome`
   + the bind parameter; rewrote `binding_tests.rs` referencing the not-yet-written
   registry → compile-red.
2. Implemented `BindingRegistry` → 113/115: two designed-in mismatches —
   `FAULT_KILL_AFTER_BLOCKS` 3000 never fired inside the ~544-block fixture
   (→ 64; see Deviations) and a sweep cooldown off-by-one (free on the second
   eligible sweep, not the third) → 115/115.
3. Added the T6b/T6c transaction tests → 118/118.
4. Wrote the composition helpers + the `composition` test module + windows glue →
   126/126 (one import-cfg fix: `bind_may_qualify` fully qualified in the windows
   hook so the shared import list stays warning-free on host).
5. `finish_quarantine` + T13 → 127/127; runtime drain leg (windows-only, compiles
   under the target check); killed the one new `--tests` warning (dead accumulator
   write → assertion of the stock accumulate protocol).

## Acceptance criteria → evidence

1. Refusals fail open: `refusal_lands_early_failed_with_a_stock_create_and_one_drain_note`,
   `missing_source_is_a_source_read_refusal`,
   `preflight_fault_legs_inject_at_their_documented_sites`,
   `preflight_refuses_an_unparseable_source`,
   `preflight_refuses_an_unmappable_loop_as_a_plan_refusal`.
2. Commit order untouched: `bind_then_success_commits_with_q31_last` (probe:
   ledger written, snapshot uncommitted, factor identity at movie-confirm) + the
   entire pre-existing transaction matrix passing with only the injection-surface
   addition.
3. Late-fail/unregister retire cleanly:
   `create_failure_after_bind_retires_the_binding_and_never_publishes_q31`,
   `unregister_prelude_cancels_pending_reads_and_releases_the_slot`,
   `registry_reclaims_only_at_quiescence_after_the_cooldown` (metrics reported
   exactly once, at the free),
   `registry_retire_cancels_an_armed_pending_read_with_clamp_semantics`.
4. Quick Restart: `quick_restart_re_binds_the_same_generation_from_offset_zero`
   (+ the conservative `quick_restart_refusal_keeps_committed_and_resets_the_clock`).
5. Readiness unchanged: tripwires unmodified; `integration_available()` asserted
   false in `preflight_refuses_an_unparseable_source`.
6. Gates: see below.

## Gates (all green, logs in `logs/`)

1. `./scripts/validate_song_playback_speed.sh` — validation passed; cargo-test
   phase 158/158 (was 137; +21) in 7.83 s (`validate_song_playback_speed.log`)
2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED (`validate_se_bank_synth.log`)
3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings (`cargo_check_windows.log`)
4. `cargo fmt --check` — clean (`cargo_fmt_check.log`)
5. `./build.sh` — release DLL OK in 46.7 s (`build.log`)

## Deviations

- **Plan-refusal fixture: degenerate mapped loop instead of the 28-bit ceiling.**
  Triggering the 28-bit overflow through a parseable bank needs ~67 M source
  frames (~73 MB fixture). A loop of length 1 at frame 64 of 32,768 maps
  degenerate at 175% (both boundaries round half-up to 37) — same
  `plan_virtual_bank` error surface, same `BindRefusal::Plan` mapping, real bank.
  The 28-bit mapping itself is one `match` arm shared with every PlanError.
- **`FAULT_KILL_AFTER_BLOCKS` = 64 (plan draft said ~3000).** The host fixture is
  ~544 blocks at 50% — 3000 never fires, and in production the producer runs far
  ahead of realtime so ANY bound dies during pre-roll; a small bound keeps the
  live fault leg and the host test the same mechanism (dev-mode only).
- **SourceCopy refusal is not force-injectable** (`try_reserve_exact` cannot be
  made to fail host-side; the five documented selectors deliberately exclude a
  copy leg). Covered by the mailbox code round-trip test; the leg is one
  `map_err`.
- **Req-4 interpretation (recorded in context.md):** ReclaimBinding EVENTS drive
  transaction-slot recycling; the binding BUFFERS are reclaimed by the registry's
  per-tick retired-list sweep — the sweep IS the "re-poll, bounded" (every 250 ms,
  never blocking), and it also covers bindings whose event was dropped on a
  saturated queue (the event-only design would leak them).
- **QR-refusal handling** (design gap): phase stays Committed,
  `reset_identity()`, one mailbox note — the conservative resolution proposed at
  handoff (stock audio must not run against a live non-identity Q31); the
  gameplay-exit boundary completes the generation; taint/ledger idempotent.
- `bind_may_qualify` added (one-atomic-load pre-gate before any path/game-memory
  work in the detour closure) — implementation detail, not in the task text.
- `generator_tests.rs` fixture helpers (`format`/`tone_pcm`/`build_bank_bytes`/
  `replay_fixture`) made `pub(super)` so binding/wavebank suites share them
  (avoids a third copy; the ORACLE stays private to generator_tests).

## Notes for siblings

- task-04 consumes: `binding::registry()` (the detours' active-binding lookup via
  `with_active` — the reference must not escape the closure; validity is the
  sweep cooldown), `bind_may_qualify` is NOT for the IO detours (it is the create
  path's gate; the read detour compares handle→file_id against
  `with_active(|b| b.file_id())`), `integration_available()` flip +
  `wavebank_hook::FILE_TABLE`-style stash pattern for its own derived addresses.
- The windows file-table row glue (`file_table_source`/`file_table_path`) is
  host-unverifiable — first live exercise is plan Step 5's bring-up.
- Sibling task records outstanding for plan Step 4: task-04 (plan checkbox stays
  unticked).

Status: Complete (uncommitted — maintainer commits personally)
