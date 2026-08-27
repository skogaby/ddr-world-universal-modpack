# Task: IO-Callback Detours, Readiness Restoration, and the Scaffold Gate

## Description

Install the XACT file-IO callback detour pair
(`src/services/song_rate/io_callback_hook.rs` — thin, allocation/log/
panic-free windows glue over task-02's serve dispatch), wire it into boot,
and deliberately flip the streaming integration live host-side: `binding::
integration_available()` reports the real installed state, the readiness
conjunction (req 40) can now be true, the SONG SPEED row registers again,
and the Step-1 identity assertions (test suites + the validator's
`identity_runtime` checks) are inverted in place. Add the interim
assist-tick scaffolding gate (req 32). This completes plan Step 4's demo:
full validator green including the runtime suites, and the row registering
in a host-reasoned readiness test. Still zero deployment — the first live
run is plan Step 5.

## Background

The detour pair is MANDATORY as a pair (design req 9;
`research/streaming-mechanism.md`): the stock getOverlappedResult callback
reports instant completion for any vector-listed handle, which would
corrupt a deferral into a spurious 0-byte completion — install both or
neither. One detour per target (repository rule); if a future consumer
ever needs these callbacks the module becomes the shared dispatcher per
the `judge_hook` pattern.

Detour semantics (design §io_callback_hook; reqs 10–11, 15, 21):

- **readFile detour** (runs on the game thread for the in-create header
  read AND on engine pump threads for packets — thread-agnostic,
  allocation-free, log-free): resolve handle→file_id with the stock
  locked-vector walk (task-01's derivation inputs); not the bound file →
  trampoline (byte-exact stock behavior for the preview player, other
  slots, every non-audio user). Bound → epoch guard, then task-02's serve
  dispatch: Served(n) → `*bytesRead = n`, `OVERLAPPED.Internal += n`,
  return TRUE; Pending → `SetLastError(ERROR_IO_PENDING)`, return FALSE.
- **getOverlappedResult detour**: unbound handle → trampoline. Bound → a
  completed pending slot or accumulated `Internal` → `*bytes = Internal`,
  zero it, TRUE; armed-incomplete → `SetLastError(ERROR_IO_INCOMPLETE)`,
  FALSE. (`bWait` is always 0 at the engine's single call site.)
- The `OVERLAPPED` offset is the full 64-bit union (`u.Pointer`);
  `Internal` is the completion accumulator — the exact stock protocol must
  be preserved for pass-through files (the trampoline guarantees it).

Readiness restoration (req 40): `integration_available()` becomes the real
conjunction (both IO-callback detours installed ∧ the preflight pipeline
present); `IdentityReadiness::binding` goes live; `integration_ready()`
can now be true, so `src/mods/song_playback_speed.rs`'s enable gate lets
the `song_speed` row register. Step-1 planted two deliberate tripwires
that MUST be flipped, not deleted: `wavebank_hook_tests.rs` proves the
readiness conjunction structurally false via the binding leg, and
`custom_options/availability_tests.rs` proves no row can register while
unready — both invert into positive conjunction tests (ready when all legs
true, not-ready when any single leg is false, row registers exactly when
ready). The validator's `identity_runtime` section checks that pinned the
identity-only base are updated in place to assert the new truth (same
schema `song-rate-validation/v1`, no section renames).

Assist-tick scaffolding gate (req 32, removed by plan Step 6): until the
tick content→wall conversion lands, tick synthesis must refuse when a
non-identity generation is committed (read the committed rate via
`song_rate::clock_patch::snapshot()` at the gameplay-start synthesis
site in `src/mods/assist_tick.rs`), so wrongly-timed claps can never ship
from an intermediate build. One bounded log line; 100% behavior
bit-identical to today's.

Boot wiring: `src/lib.rs` init installs the IO-callback hooks alongside
the existing wave-bank hook init (the init sequence is load-bearing —
signatures first, hooks before `song_rate::runtime::init` consumes
readiness).

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (reqs 9–11, 15, 21, 32, 40; §`services/song_rate/io_callback_hook.rs`;
  Threading and Synchronization table; Error Handling rows for AOB-
  unresolved and transient lag)

**Additional References (if relevant to this task):**
- `docs/xact_streaming_research.md` — §2 (callback signatures/bodies, the
  handle-vector walk under the AVS mutex), §3 (`bWait=0` single poll
  site, `FALSE + ERROR_IO_PENDING` tolerance), §7 (gotchas: the `Internal`
  protocol, thread-agnostic constraints)
- `.agents/planning/2026-08-08-song-rate-streaming/research/streaming-mechanism.md`
  — §Back-pressure (why both callbacks are mandatory as a pair)
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step01-task-02-binding-era-renames-and-identity-assertions/progress.md`
  — the two identity tripwires this task must flip deliberately

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `io_callback_hook.rs`: `GenericDetour` pair resolved from task-01's
   published addresses; installed as a pair or not at all (a second-hook
   failure uninstalls the first — mirror `wavebank_hook::init`'s
   rollback); `#[cfg(windows)]` glue kept thin over task-02's serve
   dispatch and pending-slot protocol, with any extractable pure decision
   logic host-tested.
2. Detour bodies strictly allocation-free, log-free, panic-free, and
   thread-agnostic; unbound handles take the trampoline unconditionally
   (including while no binding exists at all — the common case).
3. Handle→file_id resolution replicates the stock locked walk exactly
   (task-01 inputs); the bound comparison is `{file_id, generation}`
   against the published binding.
4. `binding::integration_available()` flips to the real installed-state
   conjunction; `runtime::integration_ready()` and the option row's
   enable gate work unchanged on top (no mod-side edits beyond what the
   flip exposes); boot wiring in `lib.rs` with one INFO on success and one
   WARN on unavailability (fail-open: unresolved signatures ⇒ row never
   registers, everything stock).
5. Step-1 tripwires inverted, not weakened: readiness-conjunction tests
   cover all-true ⇒ ready plus each single-leg-false ⇒ not-ready;
   availability tests prove the row registers exactly when ready; the
   validator's `identity_runtime` checks updated in place (schema and
   section names unchanged; the retired structurally-false assertions
   replaced by their live counterparts).
6. The assist-tick scaffold gate: synthesis refuses for rate-committed
   songs (non-identity committed snapshot) with one bounded log line and a
   host test; a 100%-committed or uncommitted boot synthesizes exactly as
   today (regression-pinned).
7. `identity_conversion_path` (the deleted-LayeredFS-seam pin) and the
   `identity_no_dynamic_redirect` validator check remain — the streaming
   design still supplies no dynamic path replacement.

## Dependencies

- `task-01-xact-io-callback-signatures` (the detour targets and walk
  inputs).
- `task-02-binding-core-and-generator` (the serve dispatch and pending
  protocol the detours call).
- `task-03-binding-preflight-and-transaction` (a publishable binding to
  gate on; readiness has meaning only with the full pipeline present).

## Implementation Approach

1. Build `io_callback_hook.rs` glue + install/rollback; wire `lib.rs`.
2. Flip `integration_available()`; invert the two Step-1 tripwire suites
   and the validator `identity_runtime` checks; add the readiness
   combination matrix.
3. Add the assist-tick scaffold gate + its tests.
4. Verify the step demo: full validator green including all runtime
   suites; the row-registers-when-ready test passes; the release DLL
   builds with the new signature set (task-01's Ghidra evidence already
   recorded).
5. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`); run the full gate set; tick
   plan Step 4's checklist item once all four sibling task records carry
   `Status: Complete`.

## Acceptance Criteria

1. **The pair installs atomically and passes through by default**
   - Given resolved signatures and a boot with no binding
   - When the hooks install and unbound reads/polls occur
   - Then both detours are live (or neither, on any failure, with one
     WARN), and every unbound call takes the trampoline with stock
     `Internal` accounting untouched

2. **Bound serving follows the native async contract**
   - Given a published binding and the serve dispatch
   - When reads are served, deferred, and polled (host-tested at the pure
     layer; windows glue thin over it)
   - Then synchronous serves return TRUE with `Internal` accumulated,
     deferrals return FALSE + `ERROR_IO_PENDING`, incomplete polls return
     FALSE + `ERROR_IO_INCOMPLETE`, and completed polls report-and-zero
     `Internal` exactly once

3. **Readiness conjunction and row registration go live**
   - Given all integration legs installed
   - When readiness is evaluated host-side
   - Then `integration_ready()` is true and the SONG SPEED row registers;
     with any single leg forced false it stays unready and no row
     registers (the inverted Step-1 tripwires prove both directions)

4. **The scaffold gate protects intermediate builds**
   - Given a committed non-identity generation
   - When assist-tick synthesis would start
   - Then it refuses with one bounded log line; at identity commitment or
     no commitment, synthesis output is bit-identical to today's

5. **Step demo holds, tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass (Windows check 0 warnings), the validator is fully
     green including the runtime suites with `identity_runtime` asserting
     the live truth under the unchanged schema

## Metadata

- **Complexity**: Medium
- **Labels**: song-rate, streaming, detours, readiness, assist-tick,
  host-validation
- **Required Skills**: Rust, GenericDetour/windows hook glue, the
  repository readiness/option-row conventions, repository host-validator
  harness
- **Generated By**: code-task-generator 2026-08-10
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 4: Wire the callback detours, binding, and generator into the transaction
