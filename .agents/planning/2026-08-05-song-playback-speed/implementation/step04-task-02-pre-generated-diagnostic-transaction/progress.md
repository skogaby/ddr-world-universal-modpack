# Progress: Step 4 Task 2 — Integrate Pre-Generated Diagnostic Transaction

Updated: 2026-08-06
Status: Complete (no commit — the maintainer commits personally per repo
workflow; the handoff explicitly forbids committing without request)

## Checklist

- [x] Setup + Explore + Plan (context.md, plan.md)
- [x] Harness extension → TDD red (`logs/validate-red.log`: died on absent
      `transaction.rs`)
- [x] publish_committed + mark_reexposed + gameplay-entry outcome + LeaseId raw
- [x] transaction.rs (+tests)
- [x] conversion.rs (+tests)
- [x] Windows glue (wavebank_hook, runtime, file_hooks, lib.rs)
- [x] Full gate suite green
- [x] Canonical progress.md updated (Task 3 next)

## TDD cycles

- RED: validator file-existence gate rejected the intentionally absent
  `src/services/song_rate/transaction.rs` (`logs/validate-red.log`).
- GREEN (pure layer): 137 → 138 host tests (111 prior + 11 transaction + 16
  conversion incl. the reload-refusal clock-reset case), full validator
  exit 0.
- Windows glue compiled with three small fixes (RecoveryReport field names,
  unsafe GetCurrentThreadId, unused import).
- Final gates: song-rate validator (138 tests + demos + stable report),
  se-bank validator, windows check (0 warnings), whole-crate fmt, release
  build — all green (`logs/validate-final.log`, `logs/build-final.log`).

## What was built

- `song_rate/transaction.rs` (+tests): the exactly-once `wavebank_create`
  protocol over injected parts — TLS frame discipline, exact-token
  consumption with owner/nonce/file-id slot recovery, commit ordering
  (ledger+session taint → movie confirm → committed snapshot → Q31 factor
  LAST via the new `RatePublication::publish_committed`, with `RESET_PENDING`
  deferral winning over a racing commit), late-fail quarantine (slot CAS +
  fixed maintenance record only), recovery-failure override (return forced 0,
  all candidates quarantined, conservative both-side taint), panic
  containment on both sides of the original, and the boot-only
  `FaultSelector` (`DDR_SONG_RATE_FAULT`).
- `song_rate/conversion.rs` (+tests): the diagnostic prepare/expose pipeline
  on the streaming conversion thread — exact dance-path gating, Step 1 source
  parse + exact 75% rate math (`GenerationRequest::from_source`), diagnostic
  bank validation against the live source (names/order/durations/loops via
  the shared `transform::map_loop`, representable Q31), idempotent cache
  import (`publish_checked`, valid destination wins), per-request quarantine
  tombstone identity (`quarantine_identity_for`), consuming lease →
  `transfer_lease` → slot claim/expose/attach, reload re-exposure
  (Committed→XactInFlight), and the reload-refusal clock reset (identity
  FIRST when a previously committed Q31 would outlive its audio).
- Shared-piece extensions: `clock_patch::publish_committed` (the only
  committed=true writer), `lifecycle::mark_reexposed` +
  `GameplayEnteredLateFailed` transition outcome, `LeaseId::{raw,from_raw}`,
  `XactSlots::{token,token_digest}`, `attach_slot_to_current` (TLS attach by
  nonce from the conversion stack frame), `score_guard::rate_ledger()`.
- Windows glue: real create-detour body (transaction parts from statics;
  identity protocol fallback when any piece is absent; detours stay
  allocation-free/log-free), `runtime.rs` coordinator/store singletons (lazy
  at first prepare — identity boots never create the cache dir; startup
  recovery on creation), 250 ms maintenance drain thread (lease release +
  manifest-derived quarantine tombstones; failures stay pinned), wine/windows
  platform + PE-header module digest boot identity, `convert_streaming_xwb`
  seam entry with per-generation bounded refusal warnings, gameplay-entry
  committed-snapshot log, gameplay-entry LateFailed taint policy;
  `file_hooks.rs` real seam + `resolve_native_via_original`; `lib.rs` fault
  selector parse + cache root + module header wiring.

## Deviations

- Slot claim happens at expose time (conversion path) instead of the design's
  pre-original claim — recovery only depends on Exposed slots carrying
  owner/nonce/file-id, and this keeps every unrelated wave bank free of slot
  churn/abandon legs (recorded in context.md §2).
- Coordinator lease-table-full (64-slot) exhaustion is not re-tested here —
  Step 2's worker tests own that case; the conversion pipeline maps every
  lease failure to the same early-fallback leg (`RefuseReason::Lease`).
- `worker.rs` had a fuzzy-match edit corruption (`request` body briefly
  called a nonexistent `request_with_epoch`); restored to
  `request_inner(request, false)` and verified against
  `request_superseding`. Root cause: an edit oldString written from memory
  instead of the file text.
- No commit (repo workflow; maintainer commits personally).
