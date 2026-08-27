# Context: Step 4 Task 2 — Integrate Pre-Generated Diagnostic Transaction

Task file: `.agents/tasks/song-playback-speed/step04/task-02-integrate-pre-generated-diagnostic-transaction.code-task.md`
Mode: auto (same verified approval lineage as Task 1; maintainer approved the
Step 4 breakdown in-session 2026-08-06). Host-only: **no deployment.**

## What exists (verified in source)

- Task 1 delivered: lifecycle state machine + eligibility + diagnostic spec
  (`song_rate/lifecycle.rs`), permanent scene callback + production sink
  (`song_rate/runtime.rs`), pending rate-save ledger + full-sanitization
  readiness (`score_guard`), save-trampoline rate election, stage counter,
  `song_playback_speed.diagnostic` config (dev-mode gated, 75% only).
- Step 3 identity transaction: `wavebank_hook` (identity detours,
  `IdentityReadiness`, maintenance enqueue on unregister), `xact_runtime`
  (4 fixed slots: claim/expose/recover_exposed/commit/quarantine/
  begin_release[_by_file]/finish_release/abandon; call-nonced TLS
  `enter_frame`/`FrameGuard::attach_slot`; fixed MPMC `MaintenanceQueue`),
  `clock_patch` (`RatePublication` seqlock — identity-only writers,
  `reset_pending` deferral; factor `AtomicU64`, Q31 stub installed at boot).
- Step 2 cache/worker: `CacheStore` (lookup/publish_checked idempotent
  valid-destination-wins, quarantine tombstones `check_quarantine`,
  recovery), `GenerationCoordinator` (entry table via builds or
  `refresh_inventory()`; `acquire_lease` requires Ready and may WAIT
  (Evicting condvar) and does store I/O (`publish_lru`); `transfer_lease`
  (consuming `CacheLease` → `LeaseId`), `release_transferred`,
  `quarantine_late_failure` — all take the coordinator Mutex ⇒ **forbidden in
  detour post-call paths**, allowed on the conversion thread and the
  maintenance drain).
- Step 1 audio: `GenerationRequest::from_source` performs the full source
  parse + exact 75% rate math + `CacheKeyInput` derivation. `CacheManifest`
  fields are all `pub` (direct construction possible for the import path).
- LayeredFS seam: `fs_convert_path_body` resolves the effective static source
  then calls the inert `wavebank_hook::identity_conversion_path` (always
  `None`). Static replacements already prove `original.call(dest,
  host_path_cstring)` works for host-path redirection.
- No runtime coordinator/store instance exists yet (Steps 1–3 host-only).
- `LeaseId` has no public raw u32 accessors (slot stores `lease_id: AtomicU32`).
- Nothing drains the maintenance queue at runtime yet.

## Key design constraints (design §Wave-Bank Hook / §Published Snapshot / §Cache)

- Detour exactly-once protocol: TLS frame → original once → consume exact
  token (TLS or owner/nonce/file-id slot recovery) → commit/late-fail —
  post-call is allocation-free, lock-free, no-panic; no exact recovery after
  known exposure ⇒ override return to 0, quarantine candidates, never recall.
- Commit order (infallible, lock-free): score protection → movie confirmation
  → seqlock snapshot (committed=true) → **non-identity Q31 last**. Reset
  writes Q31 identity FIRST; `RESET_PENDING` deferral already exists — a
  commit that raced a reset applies the pending identity reset after its
  safety publication.
- Late fail: slot Exposed→Quarantined + fixed maintenance record only;
  quarantine marker JSON + lease pinning happen on the drain. Identity clock
  retained (never written). No same-attempt stock retry. Not retried this
  boot or later boots with same identities (tombstone check before exposure).
- Unregister: release lease only after original unload completes; full queue
  ⇒ resources stay process-pinned.
- Score taint timing: commit appends one pending rate save per participating
  side + marks session taint. Late-fail does NOT taint at fail time; if
  gameplay unexpectedly starts while LateFailed, gameplay-ENTRY policy adds
  pending/session taint (design req 49).
- Quick Restart: idempotent re-exposure/recommit of the SAME committed
  generation must work (design req 53) — reload path unregisters (lease
  released) then re-creates; conversion re-exposes, commit re-runs
  idempotently (ledger append dedups by generation).

## Interpretations and decisions (auto mode)

1. **Diagnostic delivery = cache import.** The configured pre-generated XWB is
   validated against the live effective source (parse source via
   `GenerationRequest::from_source(source, code, 75)`; parse the diagnostic
   bank; require identical bank name/entry names/order/format profile and
   exactly the computed per-entry output frames + mapped loops), a
   `CacheManifest` is constructed directly, and the bank is published into
   the real store via `publish_checked` (idempotent — warm runs hit the
   existing entry). Leases/eviction/quarantine then use the real Step 2
   machinery unchanged, which is exactly what Task 3 must observe (lease
   transfer, unregister release, warm recommit in `75→100→75`).
2. **Slot claim happens at expose time** (conversion path), not in the detour
   pre-original. Deviation from the design's "claims Free→Entered before the
   original call" letter: claiming at expose keeps the no-redirect common
   path (every unrelated wave bank) free of slot churn and abandon legs; the
   recovery contract only depends on Exposed slots carrying owner/nonce/
   file-id, which expose-time claiming preserves (same thread, same frame).
   Slot-table-full at expose ⇒ stock behavior (same net effect).
3. **Prepare work runs inline on the conversion thread** (the streaming
   `fs_convert_path` nested in `wavebank_create` — the design's only
   permitted waiting site): source read + parse + digest + import publish +
   `refresh_inventory` + `acquire_lease`/`transfer_lease`. No DSP exists for
   the pre-generated path; the 30-second budget is irrelevant here but the
   read/validate cost (~tens of MB) is loading-screen time, as designed.
4. **Transaction testability**: the create-detour body and the conversion
   pipeline are written against small env traits (`TransactionEnv` /
   injected store+coordinator+slots+publication+ledger+lifecycle instances)
   so the full ordering/fault matrix runs host-side; the windows detour/seam
   wraps the statics. Same pattern as Task 1's sink.
5. **Maintenance drain = dedicated background thread** (project precedent:
   JSON-load timer thread) polling the fixed queue every 250 ms; processes
   `ReleaseLease` (coordinator.release_transferred + slots.finish_release)
   and `Quarantine` (coordinator.quarantine_late_failure with the boot
   `QuarantineIdentity`, slot stays Quarantined/pinned). Drain failures leave
   resources pinned (fail closed) with bounded warnings.
6. **Fault selector**: boot-only `DDR_SONG_RATE_FAULT` env var (design
   Error-Handling section), parsed once at runtime init, honored only in
   LayeredFS developer mode, logged prominently. Values (one at a time):
   `source-read`, `validation`, `token-mismatch`, `pre-original`,
   `post-original`, `conversion`, `xact-reject`, `maintenance-saturation`,
   `reset-overlap`. Host tests inject the same fault points directly.
7. **Cache root** = `<layeredfs mod_folder>/_cache/song_playback_speed`
   (default `./data_mods/_cache/song_playback_speed`), created lazily at
   first prepare — a plain identity boot (no diagnostic) never creates it
   (Step 3's "cache dir absent" oracle stays true for identity boots).
8. **Quarantine identity** for tombstones: source/output digests + cache
   versions + game-module digest + platform string. Game-module digest =
   md5 of the module's on-disk file (computed once at runtime init on the
   conversion thread's first use — NOT in a detour); platform =
   `"windows"`/`"wine"` best-effort. Kept minimal; Task 3 only needs the
   marker to exist and hold.
9. **Re-exposure**: `lifecycle` gains `mark_reexposed(generation)`
   (Committed→XactInFlight, same generation only) and commit accepts
   XactInFlight→Committed (re-commit publishes identical snapshot; ledger
   append dedups). LateFailed re-arm stays as shipped.
10. **Q31/commit publication**: `RatePublication::publish_committed(...)`
    (new): bounded-spin writer acquire, write full non-identity fields +
    committed=true, release even sequence, then apply `reset_pending` (if
    set: identity reset wins — factor stays identity) else store the
    non-identity Q31 factor LAST. `publish_committed` is the ONLY API that
    can produce committed=true, and it lives behind the transaction commit.

## Files to touch

- `src/services/song_rate/conversion.rs` (new, pure + env traits) + tests
- `src/services/song_rate/transaction.rs` (new: detour protocol, commit/
  late-fail ordering, recovery) + tests — or folded into `wavebank_hook.rs`
  pure section; decide by size
- `src/services/song_rate/clock_patch.rs` (`publish_committed`)
- `src/services/song_rate/lifecycle.rs` (`mark_reexposed`, gameplay-entry
  late-fail outcome)
- `src/services/song_rate/xact_runtime.rs` (only if slot helpers are missing)
- `src/services/song_rate/worker.rs` (`LeaseId::raw`/`from_raw`)
- `src/services/song_rate/wavebank_hook.rs` (real create detour body, slots/
  maintenance accessors)
- `src/services/song_rate/runtime.rs` (coordinator/store singletons, drain
  thread, conversion entry point, fault selector, gameplay-entry policy)
- `src/services/avs_layeredfs/file_hooks.rs` (real seam call)
- `src/lib.rs` (runtime init extension)
- `scripts/validate_song_playback_speed.sh` (harness file list additions)

## Build and test commands

Same as Task 1 (`validate_song_playback_speed.sh` is the TDD loop; se-bank
validator, windows check, whole-crate fmt, `./build.sh` close the gates).
