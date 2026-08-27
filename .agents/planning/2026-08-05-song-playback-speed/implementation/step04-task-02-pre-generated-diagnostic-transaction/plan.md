# Plan: Step 4 Task 2 — Integrate Pre-Generated Diagnostic Transaction

Status: Approved 2026-08-06 (inherits the approved generated task, source plan,
and design; maintainer approved the Step 4 breakdown in-session 2026-08-06)

Host-only. No deployment. Produces one identity-restorable 75% diagnostic
release build. See `context.md` for the decision record.

## Shape

### 1. Pure transaction core — `src/services/song_rate/transaction.rs` (new)

The exactly-once wave-bank create protocol plus the commit/late-fail ordering,
parameterized for host tests:

```rust
pub struct TransactionParts<'a> {
    pub slots: &'a XactSlots,
    pub maintenance: &'a MaintenanceQueue<MAINTENANCE_CAPACITY>,
    pub publication: &'a RatePublication,
    pub ledger: &'a RateSaveLedger,
    pub lifecycle: &'a LifecycleState,
    pub movie: &'a dyn Fn(bool),            // movie confirm (atomic contributor)
    pub fault: FaultSelector,
}
pub fn call_create(parts, file_id, owner_thread, original: impl FnOnce(i32) -> u8) -> u8
```

- pre-original: `enter_frame` (TLS overflow ⇒ stock, no redirect); pre-call
  panic containment (clear frame, still call original exactly once).
- original exactly once, under all legs.
- post-original (allocation-free, lock-free, panic-contained): take the exact
  frame identity; resolve the slot via the frame's `slot_index` or
  `recover_exposed(owner, nonce, file_id)`.
  - success + exposed slot ⇒ `commit_no_panic`: `slots.commit` (token) →
    score protection (ledger append per participating side +
    `mark_session_tainted`) → movie confirm → snapshot
    `publication.publish_committed(token…)` with **Q31 factor stored last**
    (reset_pending wins if a reset raced) → `lifecycle.mark_committed`
    (or idempotent re-commit).
  - failure + exposed slot ⇒ `late_fail_no_panic`: `slots.quarantine` →
    enqueue `MaintenanceKind::Quarantine{slot, lease, key digest}` (full
    queue ⇒ stays pinned) → `lifecycle.mark_late_failed`. No Q31 write ever
    happened; movie stays; no score writes here.
  - known exposure but no exact recovery ⇒ override return to 0, quarantine
    every owner-candidate slot, conservative both-side session taint +
    ledger overflow flags (fail closed), never call original again.
- fault hooks: `pre-original` (panic before original), `post-original`
  (panic after original), `token-mismatch` (corrupt the frame nonce before
  recovery), `xact-reject` (force result 0 after exposure).

### 2. Pure conversion pipeline — `src/services/song_rate/conversion.rs` (new)

```rust
pub trait SourceFs { fn read(&self, path: &str) -> Result<Vec<u8>, String>; }
pub struct ConversionEnv<'a> { spec, lifecycle, store, coordinator, slots,
    clock_now_ms, quarantine_identity, fault: FaultSelector }
pub fn prepare_streaming_redirect(env, normalized_path, effective_source_path,
    resolve_stock_native: impl FnOnce() -> Option<String>, frame: FrameIdentity)
    -> Option<ExposedRedirect { generated_path: String }>
```

Steps (every failure ⇒ `mark_early_failed` + `None` ⇒ stock 100%, bounded
diagnostics, movie retained, no score taint):

1. Gate: lifecycle phase Armed (or Committed with same generation ⇒
   re-exposure leg) AND normalized path is `…/dance/<code>.xwb` for exactly
   the configured code AND an active TLS frame exists (`frame`).
   Mismatches return `None` WITHOUT early-failing (unrelated file).
2. `begin_preparing(generation)` (or `mark_reexposed` on the reload leg).
3. Resolve + read the effective source (static replacement path, else the
   stock native path from `resolve_stock_native`).
4. `GenerationRequest::from_source(source, code, 75)` → key_input/frames;
   tombstone check (`store.check_quarantine` + coordinator
   `is_quarantined`) ⇒ refuse exposure (early fail).
5. Read + parse the configured diagnostic XWB; require: same bank name,
   same entry names/order, v1 profile, per-entry durations ==
   `key_input.output_frames`, loops mapped per the exact rate rules
   (via the shared `rate` helpers), effective rate = source/output frames of
   the main (`<code>`) entry.
6. Construct `CacheManifest` directly; `store.publish_checked` (existing
   valid destination wins) → `coordinator.refresh_inventory()` →
   `acquire_lease(key)` → `transfer_lease(lease)` → `LeaseId`.
7. `slots.claim(owner, nonce, depth, file_id)` (table full ⇒ release
   transferred lease inline — conversion thread may lock — and early-fail) →
   assemble full `RedirectToken` (digests: normalized path, generated native
   path, cache key, output) → `slots.expose(…, lease_id.raw(), token)` →
   `frame.attach_slot` → `lifecycle.mark_exposed` → return generated path.
8. Fault hooks: `source-read`, `validation`, `conversion` (generated-path
   AVS conversion failure simulated by caller), `maintenance-saturation`
   (drain-side), `reset-overlap` (host test drives scene reset between
   expose and commit).

### 3. Shared-piece extensions

- `clock_patch::RatePublication::publish_committed(generation, percent, mask,
  rate) `— the only committed=true writer; bounded-spin acquire; fields →
  even sequence → reset_pending ? identity-reset : factor=q31 LAST.
- `lifecycle`: `mark_reexposed(generation)` (Committed→XactInFlight, same
  generation), `mark_committed` accepts re-commit; `on_transition` gains
  `GameplayEnteredLateFailed` outcome (prev≠28, next==28, phase LateFailed)
  — runtime applies conservative score taint (design req 49).
- `worker::LeaseId::{raw, from_raw}` (pub).
- `wavebank_hook`: expose `slots()`/`maintenance()` accessors; create detour
  body switches from `call_create_identity` to the real `transaction::
  call_create` wired to statics; unregister path unchanged (already
  enqueues); keep `call_create_identity` for the existing identity tests.

### 4. Windows runtime glue (`runtime.rs`, `file_hooks.rs`, `lib.rs`)

- Singletons: `CacheStore` + `GenerationCoordinator` (OnceLock), root
  `<mod_folder>/_cache/song_playback_speed`, created lazily at first
  prepare; boot `QuarantineIdentity` (module digest + platform) computed on
  first conversion use.
- `runtime::convert_streaming_xwb(norm, effective_source) -> Option<String>`
  — the seam entry: requires active TLS frame + diagnostic + coordinator;
  builds the env (StdFs, stock-native resolver via original AVS convert into
  a private buffer) and calls the pure pipeline. `file_hooks.rs` replaces the
  inert seam call; returned path feeds `original.call(dest, cpath)` exactly
  like static replacements.
- Maintenance drain thread (250 ms poll): ReleaseLease ⇒
  `release_transferred` + `finish_release`; Quarantine ⇒
  `quarantine_late_failure` (slot stays Quarantined). Failures ⇒ pinned +
  bounded warn.
- Gameplay-entry LateFailed policy: append pending rate saves + session
  taint for the token's participant mask.
- `DDR_SONG_RATE_FAULT` parse at init (dev mode only, logged prominently).

## Test scenarios (host)

**AC1 exact redirect isolation**
1. Matching dance path + armed + frame ⇒ exposed redirect with full token.
2. Unrelated path / wrong code / no frame / not armed ⇒ `None`, no state
   change, no early-fail for unrelated files.
3. lstat-style probe (no TLS frame) never exposes.
4. Stale generation (re-arm between prepare and expose) refused.
5. Static-replacement source is read as the effective source.

**AC2 commit and reset ordering**
6. Successful create: effect order = score append → session taint → movie
   confirm → snapshot(committed=true) → factor Q31 (recording env asserts
   strict order; factor read asserts non-identity only after snapshot).
7. Snapshot never mixes: readers during commit see either full identity or
   full committed state (reuse seqlock stress with the new writer).
8. Reset overlap: reset during XactInFlight defers (`RESET_PENDING`); commit
   publishes safety state then applies identity reset — final factor
   identity, committed=false, no late non-identity write.
9. Ledger append is once per generation across Quick Restart recommit.

**AC3 exactly-once failure safety**
10. Original called exactly once: normal, unrelated, pre-original panic
    (no redirect + original still once), post-original panic (contained),
    nested frames, success, late-failure, token-mismatch.
11. Pre-exposure faults (source-read, validation, tombstone, lease table
    full, slot table full) ⇒ stock fallback, EarlyFailed, movie retained,
    identity factor, no ledger append.
12. XACT rejection after exposure ⇒ return stays 0, slot Quarantined, lease
    id preserved for the drain, identity factor, no ledger append at fail
    time; maintenance event enqueued exactly once; full queue ⇒ pinned slot.
13. Token mismatch after known exposure ⇒ return forced 0 + all candidate
    slots quarantined + conservative taint.
14. Gameplay entry while LateFailed ⇒ pending taint applied for the mask.

**AC4 lease and reload lifecycle**
15. Normal unload: begin_release_by_file → drain → release_transferred +
    finish_release; slot Free; coordinator lease slot Free; second unload
    no-ops.
16. Quick Restart same-generation re-exposure: second create for the same
    bank re-acquires a lease, re-exposes, recommits idempotently (same
    snapshot, single ledger entry).
17. Late failure keeps lease ProcessPinned after drain; tombstone present;
    next conversion for the same key refuses exposure (early fail).
18. Import idempotency: second prepare with identical source/diagnostic hits
    the existing published entry (no rewrite, `PublishOutcome` existing-wins
    or lookup Hit).
19. Validation rejections: wrong bank name, wrong entry order/names, wrong
    output frames, wrong loops, malformed diagnostic file.

**AC5 deployment candidate readiness** — full gate suite green, no deployment,
identity_runtime report checks stay green (identity path untouched when no
diagnostic configured), release DLL builds.

## Implementation order (TDD)

1. Harness/script file-list extension (+`conversion.rs`, `transaction.rs`,
   their test files) → RED.
2. `publish_committed` + `mark_reexposed`/gameplay-entry outcome +
   `LeaseId::{raw,from_raw}` (+ focused tests).
3. `transaction.rs` (+tests: ordering, exactly-once, recovery, late fail).
4. `conversion.rs` (+tests: gates, validation, import, lease/slot flow,
   faults, tombstones, re-exposure).
5. Windows glue: wavebank_hook real body, runtime singletons + drain +
   fault selector + gameplay-entry policy, file_hooks seam, lib.rs.
6. Full gates; progress updates (Task 3 next).

## Risks / notes

- `acquire_lease` writes the LRU index (store I/O) under no lock but after
  the coordinator lock — conversion-thread only; never in detours. Enforced
  by construction (transaction.rs has no coordinator/store references).
- The `xact-reject` fault must not be reachable outside dev mode; selector
  parsing requires developer_mode and logs prominently at boot.
- Re-verify no behavior change for identity boots: without a diagnostic
  spec, `convert_streaming_xwb` returns `None` before touching the
  store/coordinator (no cache dir creation).
