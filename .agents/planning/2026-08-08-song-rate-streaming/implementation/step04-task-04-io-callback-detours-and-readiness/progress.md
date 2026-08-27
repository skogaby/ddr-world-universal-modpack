# Progress — Step 4 task-04: IO-Callback Detours, Readiness Restoration, and the Scaffold Gate

Updated: 2026-08-10
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] 1. Tests first: U1 (gate predicate — red then green), U2/U3 (tripwire
      inversions, in place)
- [x] 2. `io_callback_hook.rs` (OVERLAPPED mirror, detour pair, install/rollback,
      `installed()`); mod.rs entry; validator file-presence line
- [x] 3. Readiness flip (`integration_available()`), `lib.rs` boot wiring
- [x] 4. Assist-tick scaffold gate (predicate in clock_patch + mod consumer)
- [x] 5. Full gate set green; record closed; plan Step 4 checkbox ticked (all
      four sibling records Complete); feature progress.md updated

## What landed

- **`src/services/song_rate/io_callback_hook.rs` (new, `#[cfg(windows)]`):** the
  XACT file-IO detour pair. Local `#[repr(C)]` OVERLAPPED mirror documenting the
  repurposed-field protocol (Internal @0 = completion accumulator, offset union
  @16 = full 64-bit read offset). `init(&SignatureStore)` resolves task-01's
  `song_rate_readfile_callback` / `song_rate_overlapped_callback` /
  `song_rate_handle_lookup` (any missing ⇒ one WARN, nothing installs), stashes
  the stock lookup helper, installs readFile then getOverlappedResult with a
  pair-or-neither rollback (mirror of `wavebank_hook::init`); one INFO on
  success. Detour bodies (allocation/log/panic-free, thread-agnostic):
  `bound_verdict` gates on ONE Acquire load of the registry's active binding
  (null ⇒ trampoline — the common case costs one load), then the stock
  handle→file_id helper (Option A: the locked sorted-vector walk, a cost class
  stock pays per read), then serve/poll. Outcome→ABI mapping: Served(n≠0) ⇒
  `*bytesRead = n`, TRUE; Served(0) ⇒ FALSE (byte-exact stock "TRUE iff
  copied != 0" — the defensive-EOF leg); Pending ⇒ FALSE + `ERROR_IO_PENDING`;
  Refused ⇒ trampoline (recorded decision: the binding retired mid-teardown —
  byte authority returns to stock, whose RAM copy is still loaded and whose EOF
  clamp owns the size difference); poll Complete(n) ⇒ `*bytes = n`, TRUE
  (≤ 64 KiB, exact); Incomplete ⇒ FALSE + `ERROR_IO_INCOMPLETE`; NotPending ⇒
  stock report-and-zero of `Internal`, TRUE.
- **Readiness flip:** `binding::integration_available()` now reports
  `io_callback_hook::installed()` on windows (false on host — no detours exist
  there); doc comment records the flip as this task's deliberate act.
  `IdentityReadiness::binding`, `runtime::integration_ready()`, and the SONG
  SPEED row's enable gate work unchanged on top.
- **`src/lib.rs`:** `io_callback_hook::init(&signatures)` installed between
  `wavebank_hook::init` and the `readiness(..)` computation (order load-bearing:
  the binding leg reads the installed state); the conjunction log line now
  reports "Song-rate streaming integration ready" / the incomplete WARN.
- **Tripwires INVERTED in place (not deleted):**
  `wavebank_hook_tests::readiness_binding_leg_tracks_the_installed_integration`
  (the live linkage `readiness(true).binding == integration_available()` + the
  everything-else-true conjunction equals the binding leg; the pre-existing
  all-true/each-single-false matrix test remains) and
  `availability_tests::song_speed_row_registers_exactly_when_the_integration_is_ready`
  (modeled enable gate BOTH directions + the live linkage). Validator
  `identity_runtime`: no check edits needed — the section contains no
  identity-base readiness pin (verified; recorded in context.md);
  `identity_no_dynamic_redirect` survives per req 7; schema unchanged
  `song-rate-validation/v1`. File-presence list gained `io_callback_hook.rs`
  (plain bash area).
- **Assist-tick scaffold gate (req 32):**
  `RateSnapshot::is_non_identity_commit()` in `clock_patch.rs` (host-tested —
  the predicate is the extractable decision logic) + the gate in
  `assist_tick::tick_clock`'s `Phase::AwaitAnchor` arm: a committed non-identity
  snapshot refuses the song's synthesis (phase → Idle, new `Action::RateGated`
  logs ONE line outside the lock — naturally once per song, the phase is Idle
  after). The anchor fires at the first judge dispatch, strictly after any
  loader-thread commit lands (closes the late-commit race a gameplay-entry gate
  would have). 100%/uncommitted: predicate false ⇒ the literally-identical
  existing code path.

## TDD cycles

1. U1 written first → red (`no method named is_non_identity_commit`); predicate
   implemented → 128/128 in the fast harness. U2/U3 inversions rewritten in
   place (both suites green immediately — host linkage semantics).
2. `io_callback_hook.rs` + mod.rs + flip + lib.rs + assist-tick gate + validator
   line; two closure-unsafe fixes (closures do not inherit the unsafe fn
   context); windows check 0 warnings, `--tests` only the 2 pre-existing
   proven-core warnings; harness 128/128.

## Acceptance criteria → evidence

1. Pair installs atomically / passes through by default: pair-or-neither
   rollback in `init` (mirrors the proven wavebank rollback shape); unbound
   calls trampoline unconditionally (`bound_verdict` None legs — including the
   no-binding fast path). Windows-glue thin by construction; the decision logic
   (serve/poll accounting) is task-02's host-tested dispatch.
2. Bound serving follows the native async contract: the outcome→ABI mapping
   above; the accounting semantics are pinned by task-02's host suites
   (deferral exactly-once, report-and-zero, poll-before-complete incomplete).
3. Readiness conjunction + row registration live: the two inverted tests + the
   surviving all-true/each-single-false matrix +
   `song_speed_row_registers_exactly_when_the_integration_is_ready` (both
   directions, host-reasoned).
4. Scaffold gate: `non_identity_commit_predicate_gates_the_assist_tick_scaffold`
   (committed 75% gates; uncommitted/identity never gates); mod-side refusal is
   one log line per song with phase → Idle; the ungated path is the identical
   pre-existing code (bit-identical regression argument).
5. Step demo + tree green: gates below; validator fully green including the
   runtime suites under the unchanged schema.

## Gates (all green, logs in `logs/`)

1. `./scripts/validate_song_playback_speed.sh` — validation passed; cargo-test
   phase 159/159 (was 158; +1) in 8.15 s (`validate_song_playback_speed.log`)
2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED (`validate_se_bank_synth.log`)
3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings (`cargo_check_windows.log`)
4. `cargo fmt --check` — clean (`cargo_fmt_check.log`)
5. `./build.sh` — release DLL OK in 48.6 s (`build.log`)

## Deviations

- **`ServeOutcome::Refused` → trampoline** (the design leaves the mapping open;
  task-02 called it a hard-fault leg): Refused occurs only when the binding
  retired under the read (unregister teardown — engine Destroy follows within
  the same call) or the structurally-unreachable no-free-slot case; returning
  byte authority to stock keeps the engine inside its native protocol instead
  of surfacing a synthetic read failure during teardown. Poll consistency is
  automatic (retire unpublishes first, so subsequent polls trampoline too).
- **`Served(0)` returns FALSE without SetLastError** — byte-exact stock
  replication ("returns TRUE iff copied != 0"), preserved deliberately.
- **The detour byte-protocol itself has no new host tests**: the windows glue
  adds no decision logic beyond the two recorded mappings above; the accounting
  is task-02's host-tested serve/poll dispatch (the task's "extractable pure
  decision logic host-tested" — extracted where it always lived).
- **Scaffold gate log**: once per song (structural — the phase leaves
  AwaitAnchor), not a per-boot latch; more useful for plan Step 5's live matrix
  and still bounded.
- **The gate predicate lives on `RateSnapshot`** (clock_patch.rs) rather than in
  the mod: the mod is not host-mounted, the predicate is — this is what makes
  req 6's host test possible.

## Notes

- Plan Step 5 (first deployment) exercises live: the detour pair on a real
  cabinet boot, the file-table row glue, the QR/unregister teardown paths, the
  `DDR_SONG_RATE_FAULT=mid-song-failure` silence-fill run, and the
  throughput/deferral benchmark (drain INFO lines per generation).
- All four Step-4 sibling records now carry `Status: Complete` — plan Step 4's
  checkbox ticked with this task.

Status: Complete (uncommitted — maintainer commits personally)
