# Audio Diagnostic Track

Updated: 2026-09-08
Status: Complete (uncommitted; integrated and full builds passed; cabinet validation pending)
NEXT ACTION: Follow docs/frame_scheduling.md on cabinet; main progress.md records final integration/build evidence.

Scope and approval: parent `plan.md` Status: Approved 2026-09-08. Parent owns
integration and full builds. No git mutations, live debugging, or deployments.

## Done
- Read parent context/approved plan, config, judge dispatcher, bank hooks, clock patch and audio RE.
- Static current-build RE confirms PREPARE at RVA 0x1AA060, start wrapper 0x1AA120 -> manager 0x1AB1C0; DPS calls start on 0x1044.
- GPA raw field +0x178 is written AFTER judge dispatch; capture the argument separately.
- Tests written before implementation for bounded ring, concurrent loss accounting, attempt identity, sample cadence/discontinuities, QPC, output cap, and observer failure isolation.
- Implemented default-off config, 512-event try_lock ring, 8 MiB capped buffered CSV writer, loss summaries, optional hooks, judge post/Late subscriber, bank callout outside the rate gate, and frame API.
- TDD: missing implementation RED (`audio-red.log`), anchor-provenance RED (`audio-anchor-red.log`), missing CSV formatter RED (`audio-csv-red.log`), failed-QPC RED (`audio-qpc-red.log`), unavailable-scene RED (`audio-scene-red.log`). Final `audio-green.log`: **15 passed, 0 failed**.
- `audio-signatures.log`: **ALL GREEN**, all 22 cross-build gaps covered by alternates. New start/raw-store AOBs unique on all four builds; timing quartet unique via RDI primary / R12 v1. `audio-unique.log`, `audio-offset-unique*.log`, `audio-layouts.log` confirm counts.
- `audio-shapes.log`: prepare/ready/stop/start/broadcast identical through 0x100; quartet +0x4C and raw-store +0x48 divergences are outside consumed bytes. v1 independently verified in Ghidra and uniqueness check.
- `audio-check.log`: Windows-target **cargo check passed** using a temporary validation crate mounting the actual core/mod/service/widget/type files plus pending module declarations (no substitute implementations). One expected unused re-export warning because this mount omits lib.rs's boot entry. No parent source files changed. This is NOT the coordinated full DLL/Win7 build.
- Formatted the two new Rust modules with direct rustfmt; parent still owns whole-crate cargo fmt.
- Documented static XACT wave-start chain, immediate-due branch, masked post-submission stamp and unresolved cue/voice/output mapping in `docs/audio_sync_diagnostics.md`.
- Ghidra restored: only gamemdx_20260825.dll open/current; temporary XACT and 20250805 inspection programs closed without analysis/edits.

## Parent Integration (Completed)
1. `src/services/mod.rs`: `pub mod audio_sync_diag;`.
2. `src/lib.rs`, after existing song_rate initialization and judge_hook init: `services::audio_sync_diag::init(&signatures);`.
3. Once per genuine frame at the new scheduling boundary: `crate::services::audio_sync_diag::record_frame([frame_seq, polls, jobs, queue_depth]);` (all u64).

Files changed: `src/services/audio_sync_diag/{mod.rs,model.rs}`, `src/mods/config.rs`,
`src/services/song_rate/wavebank_hook.rs`, `src/core/signatures.rs` (four OPTIONAL
audio names), `scripts/validate_audio_sync_diag.sh`, `docs/audio_sync_diagnostics.md`.
No lib.rs/services mod.rs/Cargo.toml/widget/input/overlay/operator-config edits.

## Deviations and Resolved Issues
- The first concurrency fixture used 4096 stack records and overflowed the host test thread stack. Reduced to the production 512-slot capacity; assertions still account for all 2000 concurrent attempts via stored + full + contention.
- Existing rate snapshot reader spins. Instead validate the generated clock stub at init and read its actual aligned atomic Q31 multiplier. Missing/changed code means unavailable, not guessed identity.
- Current quartet AOB misses 20250805 because its ctor uses R12 and 20-byte cells. Added the statically verified v1, not a wildcard layout guess.
- Temporary Windows check initially saw parent in-flight unresolved deferred_work imports. Mounted the parent's actual new module in the temporary check crate only, then check passed.

## Limitations
- Runtime validation remains parent/maintainer-owned.
- The diagnostic never corrects clocks or labels requests as audible starts.
- File is `audio-sync-diagnostics.csv`, replaced on the next enabled launch. Lifecycle hooks stay passthrough after cap/IO failure. Frame/bank channel availability depends on parent wiring/existing bank hooks.
- Call records enqueue after the original; a synchronous scene transition inside that original gives enqueue-time scene attribution. Raw entry/end QPCs preserve the span; do not treat enqueue ordering as chronological ordering.
- No git mutations, deployments, live debugging or clock/offset writes performed.
