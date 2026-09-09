# XACT Diagnostic Track

Updated: 2026-09-09
Status: Complete (uncommitted - maintainer commits manually)
NEXT ACTION: Runtime off/on capture; parent integration and normal/Win7 build gates are now complete (see progress.md).
Resume: Read context.md and plan.md in this directory. Only this track's assigned files may be edited.
Completion scope: engine track and parent integration complete; runtime validation outstanding.

## Checklist

- [x] Read approved approach and shared integration contract.
- [x] Static RE establishes both event and streaming-wave deferrals.
- [x] RED/GREEN actual pure correlation and site/identity tests (19 passing).
- [x] Engine observers, pre-Initialize bootstrap, and passive cursor observer.
- [x] Actual engine binary and four-game factory/manager attestations.
- [x] Final Windows type check clean. Parent owns full builds/integration.

## Scope And Tests

Files: src/services/audio_sync_diag/xact.rs, xact_model.rs, xact_sites.rs;
scripts/validate_xact_diagnostics.sh. Optional core/signatures.rs factory anchor.
No git mutations, operator config edits, deployment, audio calls added by observers,
or live debugger. Approval: plan.md, Status: Approved 2026-09-08. CODEASSIST.md absent.

Tests: asynchronous token retention; handle/cue reuse; unbound-token destruction;
stop/unregister invalidation; contention epoch poisoning; reciprocal ownership
rejection; submission skipped flag; cursor failure/reset/format/buffer/wrap gaps;
PE identity, unique anchors and changed consumed code rejection. Host harness
mounts the actual modules and scanner. Logs: logs/xact-*.log. Parent's build gates
remain cargo check, cargo fmt, normal/Win7 release builds and signature sweep.

## Decisions

- Exact supported-code fingerprints supplement PE identity and AOBs. They reject
  unknown consumed layouts rather than silently treating familiar prologues as proof.
- Prepare retains a token by handle; start attaches the live cue/bank, before
  original. Destruction invalidates unbound tokens too: they cannot identify which
  just-destroyed cue previously occupied a now-reusable game handle.
- No persistent wave-to-song guess. Submission reconstructs reciprocal ownership.
- All producer state is fixed-size and try-lock-only. Failed critical updates
  poison a monotonic epoch independently of CSV queue delivery.
- Game start reads only the stable, game-owned cue interface/bank. Sound/track
  traversal waits for an engine-serialized callback. The pure model tests lookup
  on a different thread after this partial attachment.
- Cursor output is hard-decimated to four records/second, including backend churn
  and failures. A skipped failure or sampler-lock contention still invalidates
  continuity of the next record WITHOUT resetting the emission budget.
- Runtime snapshots omit writable and discardable sections. Host validation applies
  actual DIR64 relocations and changes the IAT; all code attestations still pass.
- No core/signatures.rs edit needed: local factory pattern is tested on all four
  game images; the existing SignatureStore manager pattern supplies its attestation.

## Parent Integration

Final parent refinement: init_factory takes &GameModule, runs before the full
signature scan, and resolves its manager locally. Prepare now binds the stable
cue/bank atomically; schedule/voice traces expose pre-probe cost separately.
Final XACT harness has 21 passing tests, including these two refinements.
The original integration contract below is retained as track history.

Exports in xact.rs: init_factory(&SignatureStore), channel_bits()->u64,
on_prepare(i32,i32,[u8;32],Context), on_start(*mut u8,i32), on_stop(i32),
on_bank_unregister(). The parent owns the early init and unregister ENTRY call.
Sibling agent owns mod.rs forwarding and Event/Kind extensions.

Channel bits: 16 factory observer armed; 17 schedule; 18 streaming submission;
19 sound stop; 20 cue destructor; 21 mixed-output cursor. Bits 17..21 install
transactionally before Initialize. The module reference and all trampolines stay
alive even if rollback fails; READY remains false and retained observers passthrough.

Payload schema is documented at the top of xact.rs. detail_valid low bits are
per-cell availability; OutputCursor bit 8 is continuity, not per-song progress.
VoiceStart detail[7]: -1 unknown, 0 skipped, 1 Start returned success, 2 failure.
EngineStatus result: 0 unavailable, 1 armed, 2 installed, 3 missed window,
4 unsupported, 5 install failed, 6 factory unavailable, 7 manager unavailable.
EngineStatus details: channel mask, correlation epoch, critical-update losses.
EngineStatus 3..7 should be warnings in the parent's offline report/writer, NOT
formatted/logged by a hot producer. Boot-detected failures already log WARN.

The early window is NOT guaranteed by a polling init thread. Already-loaded or
arrived-during-scan engines are refused with WARN; if the armed wrapper never
fires, first prepare emits MissedWindow. No late engine scan/patch exists.
Try-lock loss and conservative unbound-token invalidation can produce unmatched
records; they must not be repaired by nearest-time/last-song guesses.

## Validation

- logs/xact-red.log: expected missing model/site implementation errors.
- logs/xact-fingerprint.log: models GREEN, identity fixture intentionally rejected
  until measured fingerprints were supplied.
- logs/xact-chain-red.log: expected missing ownership/submission/observer helpers.
- logs/xact-binding-red.log: partial cue-only binding rejected before refinement.
- logs/xact-hardening-red.log: factory body mutation and cursor flood tests fail.
- logs/xact-relative-red.log: expected missing bounded decoder wrapper; the final
  wrapper range-checks before using the shared scanner's pointer arithmetic.
- logs/xact-cursor-contention-red.log: expected missing continuity invalidation
  helper, which now preserves the emission budget during repeated contention.
- logs/xact-green.log: 19 passed, 0 failed; four game factories normalize identically;
  every attestation span's tail mutation is rejected, as are specific field changes.
- logs/xact-check.log: cargo check --target x86_64-pc-windows-msvc clean.
- logs/xact-format-check.log and xact-shell-check.log: scoped rustfmt check and
  bash syntax check clean. Parent still runs whole-crate cargo fmt/build gates.
- Ghidra engine closed; original main remains current. No annotations/live debugging.
- No runtime success claim. No staging, commit or deploy.

## Review Outcomes

- Fixed game-side mutable-sound reads by deferring the entire sound/track walk to
  engine-serialized callbacks; no new engine lock acquisition or audio calls.
- Tightened factory acceptance from entry/string checks to a full normalized-body
  fingerprint identical across the four supplied game builds.
- Made enable failures AND enable panics attempt every rollback step. A rollback
  error/panic retains process-lifetime passthrough trampolines and module pin.
- Destructor/sound-stop payload IDs now come from live entry observations, not
  fabricated zeros from the intentionally cue-only game-side binding.
- Remaining risks are live hook timing/overhead and an init thread missing the
  factory window; both require the parent's next diagnostics-off/on capture.
