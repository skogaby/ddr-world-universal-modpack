# Task: Binding Preflight and the Reworked Create/Unregister Transaction

## Description

Replace `binding.rs`'s always-refusing Step-1 stub with the real preflight
pipeline (injected source view → validation → `plan_virtual_bank` → private
source copy → producer start → binding publication), and rework the
wave-bank transaction around it: the pre-original `bind` closure inside
`call_create`, unregister retiring the binding and enqueuing
`ReclaimBinding`, the maintenance drain performing reclamation at reader
quiescence and logging generator diagnostics, Quick-Restart re-create
regeneration, and the new `DDR_SONG_RATE_FAULT` streaming legs. Design
reqs 17, 23–26, 41 (plus 5 for Quick Restart). Host-tested through the
same injected-parts pattern the existing transaction suites use; the
IO-callback detours and readiness flip remain task-04's — readiness stays
structurally false through this task.

## Background

The Step-1 base kept the exactly-once transaction machinery intact:
`transaction::call_create` (TLS frame discipline, exact-token
commit/late-fail, recovery, the injectable `TransactionParts`),
`xact_runtime.rs` (slots with `claim`/`expose`/`commit`/`quarantine`/
release, `MaintenanceKind::ReclaimBinding`, the bank timeline), and
`lifecycle.rs` (`begin_binding`, `mark_exposed`, `mark_reexposed`,
`mark_committed`, `mark_late_failed`, `mark_early_failed`). The design
replaces the retired "expose" semantics with "bind": the create detour's
pre-original phase performs the bind and publishes the binding + token
into the in-flight slot; the original runs exactly once (the engine's
header read is served through the bound callbacks — task-04); the
post-original commit order is UNTOUCHED (score taint → movie confirmation
→ rate snapshot → Q31 LAST) as is every recovery leg.

Preflight (design §binding.rs, req 23 pre-original, req 24): verify armed
generation + slot-5 dance-bank path (`dance_bank_song_code`, kept) + song
consistency (`bind_song` digest, set at first bind of the generation);
read and validate the source from an injected `SourceView` (windows glue
passes the FileManager row's pointer/size resolved through task-01's
file-table derivation — the RAM copy the game itself loaded at song
confirm; hosts pass plain buffers); `parse_song_bank` profile validation;
`plan_virtual_bank` (28-bit overflow and unmappable loops refuse);
copy the source into a private allocation (req 17 — no reads of game-owned
memory after the bind returns); start the producer (task-02); publish
`{file_id, generation}`. Every refusal is typed (`BindRefusal` grows real
variants replacing `IntegrationAbsent`) and lands EarlyFailed: the
original runs unbound, the song plays stock at 100%, one bounded WARN via
the drain (req 24 — fail-open direction unchanged).

Unregister (req 26): retire the binding BEFORE the original destroys the
bank and closes the handle (state → Retired, pending slots cancelled with
clamp semantics), enqueue `ReclaimBinding`; the drain frees buffers only
at `readers == 0` (epoch guard) and logs the generator's
throughput/deferral metrics at generation end (feeds plan Step 5's
benchmark run sheet).

Quick Restart (req 5): a re-create of the same committed generation
(`mark_reexposed` path, `Committed → XactInFlight`) serves the generation
again from offset zero — regeneration through a fresh binding/producer,
not cache reuse.

Fault legs (req 41, dev-mode boot selector): `source-read`,
`header-synth`, `generator-start`, `bind-refused` inject at their
preflight sites; `mid-song-failure` arms task-02's kill-after-N-packets
hook (exercises silence-fill live in Step 5). The surviving transaction
legs (pre/post-original panic, token-mismatch, xact-reject,
maintenance-saturation) are unchanged.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (reqs 5, 17, 23–26, 41; §`services/song_rate/binding.rs`;
  §`services/song_rate/wavebank_hook.rs` + `transaction.rs`;
  §`services/song_rate/lifecycle.rs, runtime.rs, xact_runtime.rs`; Error
  Handling table; Lifecycle State Machine diagram)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/research/streaming-mechanism.md`
  — §Binding identity (pre-original, race-free with the in-create header
  read), §Commit authority, §Source lifetime hazard (why the copy)
- `docs/xact_streaming_research.md` — §4 (`wavebank_create`/unregister
  precise semantics: duplicate guard, handle-vector insert timing,
  synchronous DoWork inside create; unregister ordering), §5 (the preview
  player creates slot-5 banks through the identical path — binding must be
  gated on the armed generation, never on path alone)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `binding.rs`: a `SourceView` abstraction (bytes + length; windows
   construction from the task-01 file-table derivation is thin glue here,
   host tests inject buffers) and the real `prepare_binding` pipeline in
   the Background's order, returning a live binding handle or a typed
   `BindRefusal` (profile, plan/28-bit, copy allocation, producer start,
   plus the fault-injected legs). `integration_available()` STAYS false —
   its flip is task-04's deliberate act (the readiness conjunction and row
   registration must not change in this task).
2. `transaction.rs`: `call_create` gains the pre-original bind step as an
   injected closure/part (design: `bind: FnOnce(file_id) -> BindOutcome`
   replacing the retired nested-convert exposure), panic-contained like
   the existing pre-original fault leg; a bind failure ⇒ no token exposed
   ⇒ the stock leg. `CreateOutcome` variants and the post-original
   commit/late-fail/recovery orderings are byte-for-byte the current
   behavior (existing suites keep passing with only the injection-surface
   additions).
3. `wavebank_hook.rs`: the windows `create_hook` builds the bind closure
   (armed check → preflight with the FileManager-row `SourceView` →
   slot expose + binding publish); `unregister_hook` retires the binding
   pre-original and enqueues `ReclaimBinding`. The identity fallback
   (shared pieces absent) is preserved.
4. `runtime.rs` drain: `ReclaimBinding` events reclaim the binding's
   buffers only at `Retired ∧ readers == 0` (re-enqueue or re-poll
   otherwise, bounded), log one line per reclaimed generation carrying the
   producer's throughput/deferral metrics, and emit the preflight-refusal
   WARNs (detours and preflight never log directly — the drain does).
5. Quick Restart: a create for the same committed generation re-binds and
   regenerates from offset zero (fresh producer, same generation identity,
   taint/ledger idempotence per the existing once-per-generation append).
6. `FaultSelector::parse` accepts the five new streaming legs; each
   injects at its documented site; production (non-dev) boots parse none.
7. Host tests (extend the existing sibling suites; new
   `binding_tests.rs` legs): every refusal → EarlyFailed with NO binding
   and a stock create; bind → create-success → commit ordering with Q31
   last (extend `transaction_tests.rs` with the bind surface); create
   failure after bind → binding retired + LateFailed (Q31 never
   published); unregister-with-pending → cancellation with clamp
   semantics then reclamation at quiescence; Quick-Restart re-create
   regeneration; all five new fault legs; the surviving
   recovery/token-mismatch/saturation suites pass unmodified in behavior.

## Dependencies

- `task-02-binding-core-and-generator` (the Binding state, producer,
  pending slots, metrics, fault hook).
- `task-01-xact-io-callback-signatures` (the file-table derivation for
  the windows `SourceView` glue; host tests do not need it — if task-01
  lands first as ordered, wire it; the pure pipeline is testable either
  way).

## Implementation Approach

1. Land `SourceView` + the preflight pipeline pure-first with refusal-leg
   tests (fake sources: unparseable, wrong profile, 28-bit overflow,
   allocation-failure injection, producer-start failure).
2. Add the bind injection surface to `call_create` and prove the existing
   transaction matrix is behaviorally unchanged before wiring the real
   closure.
3. Wire `create_hook`/`unregister_hook` + the drain reclamation and
   diagnostics; then Quick Restart and the fault selector legs.
4. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`); run the full gate set.

## Acceptance Criteria

1. **Refusals fail open with no binding**
   - Given each preflight refusal leg (profile, plan, copy, producer
     start, and the injected fault legs)
   - When a qualifying create runs
   - Then the attempt is EarlyFailed, no binding or token exists, the
     original ran exactly once unbound, and one bounded WARN reaches the
     drain

2. **Commit order is untouched with the bind in front**
   - Given a successful bind and a successful create
   - When the transaction completes
   - Then the ordering is score taint → movie confirm → snapshot → Q31
     LAST, the outcome is Committed, and the existing recovery /
     token-mismatch / late-fail suites pass with unchanged semantics

3. **Create failure and unregister retire cleanly**
   - Given a bound create that the engine rejects, and separately a bound
     bank being unregistered with a pending read armed
   - When the post-original / unregister paths run
   - Then the binding is retired (LateFailed for the rejection; Q31 never
     published), pending slots complete with clamp semantics, and
     reclamation happens only at reader quiescence with the generation's
     metrics logged once

4. **Quick Restart regenerates**
   - Given a committed generation whose bank is re-created
   - When the re-create transaction runs
   - Then the same generation serves again from offset zero via
     regeneration, with taint/ledger appends not duplicated

5. **Readiness is unchanged**
   - Given the completed task
   - When readiness and row registration are evaluated host-side
   - Then `integration_available()` is still false and no SONG SPEED row
     registers (the flip is task-04's)

6. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: High
- **Labels**: song-rate, streaming, transaction, binding, fault-injection,
  host-validation
- **Required Skills**: Rust, the repository's exactly-once transaction
  machinery, lifecycle/CAS discipline, repository host-validator harness
- **Generated By**: code-task-generator 2026-08-10
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 4: Wire the callback detours, binding, and generator into the transaction
