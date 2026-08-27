# Task: Preview registry slot + io miss-path routing

## Description

Give `BindingRegistry` an independent PREVIEW binding slot alongside the
active (gameplay) slot, route the XACT io callbacks' binding resolution
through a host-testable two-slot lookup (active first, preview on miss),
and cover preview bindings with the existing retire/sweep reclamation and
a refusal mailbox. This is the serving-side plumbing the song-select
preview-rate feature publishes into; nothing qualifies or publishes
preview bindings yet (that is plan Step 3).

## Background

The registry (`src/services/song_rate/binding.rs`, `BindingRegistry`)
holds ONE active binding as a raw `AtomicPtr<Binding>` published from
`Arc::into_raw`, a fixed retired list with per-slot reclamation cooldowns
swept by the maintenance drain, and a coalescing refusal mailbox. The io
detours (`src/services/song_rate/io_callback_hook.rs::bound_verdict`)
resolve a file handle to the game's file id via the stock lookup helper
and serve only when it matches the active binding's file id — one Acquire
load on the no-binding hot path.

Design (§Components 3): the preview slot is consulted only after the
active slot misses; `retire_by_file` must cover BOTH slots so the existing
unregister prelude retires preview bindings on every natural teardown with
no new call sites; retired preview bindings flow through the existing
sweep; preview bindings carry a separate monotonic generation counter
(R15) and never touch the gameplay lifecycle.

The io hook module's own law ("everything with judgment lives in the
host-tested serve/poll dispatch" — its doc header) dictates the shape: the
two-slot routing decision moves INTO the registry as host-testable
methods; `bound_verdict` (windows-only, compile-checked but not host-run)
stays pure plumbing.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-15-song-preview-rate/design/detailed-design.md
  (§Components 2 "the preview registry slot", §Components 3, §Data Models,
  §Detailed Requirements R8/R9/R15)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-15-song-preview-rate/research/engine-integration.md §2.3

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `BindingRegistry` gains a `preview: AtomicPtr<Binding>` slot with:
   - `publish_preview(Arc<Binding>)` — swap-in; a previous preview binding
     is retired and pushed to the retired list (same defensive pattern as
     `publish`);
   - `with_preview<R>(visit) -> Option<R>` — mirror of `with_active`
     (same non-escaping/cooldown safety contract);
   - `retire_preview() -> bool` — unconditional force-retire of the
     preview slot (the scene-exit defense and mod-disable path).
2. `retire_by_file(file_id)` checks the active slot first, then the
   preview slot; returns true if either retired. CAS race semantics
   identical to today's.
3. New host-testable routing surface:
   - `any_bound() -> bool` — true when either slot is non-null (the
     detours' fast gate; ≤ two Acquire loads);
   - `with_bound_for_file<R>(file_id, visit) -> Option<R>` — active first,
     preview on miss (file-id equality), `None` when neither matches.
4. `io_callback_hook::bound_verdict` rewires onto them: `any_bound()`
   gate → one stock handle lookup → `with_bound_for_file`. Behavior for
   the gameplay-only case is unchanged; the no-binding hot path stays
   allocation/log/panic-free with at most two Acquire loads.
5. Preview refusal mailbox: `note_preview_refusal(refusal, file_id)` /
   `take_preview_refusal()` mirroring the existing coalescing mailbox
   (separate atomics — preview refusals must not mask gameplay refusals).
6. Preview generation counter (R15): `next_preview_generation() -> u64`
   (process-wide monotonic `AtomicU64`, starting at 1) in `binding.rs`.
7. Sweep and reclamation unchanged: retired preview bindings reclaim
   through the existing `sweep` (cooldown, `reclaim_eligible`, report
   callback).
8. Both cfg targets compile; existing suites pass unchanged.

## Dependencies

- Step 1 (StretchTarget) — already complete on the tree; the cycle test
  constructs a Side-target preview binding.

## Implementation Approach

1. Registry: add the slot + methods (mirror the active slot's
   publish/with/retire code paths; share `push_retired`).
2. Routing: `any_bound` + `with_bound_for_file`; rewire `bound_verdict`.
3. Mailbox + counter.
4. Tests in `binding_tests.rs` (host-run through the validator harness):
   - publish/with/replace-retires-previous for the preview slot;
   - `retire_by_file` both-slot coverage (preview-only, active-only,
     both-live-different-files) and `retire_preview` force-retire;
   - routing: active-first precedence when both slots hold the same
     file id is unreachable in production (one bank = one binding) but
     the method's order is still pinned; miss cases fall through to
     `None`;
   - `any_bound` gate truth table;
   - preview refusal mailbox coalescing independent of the gameplay one;
   - full cycle: `publish_preview(Side-target binding)` → serve a target
     packet through `with_bound_for_file` → `retire_by_file` →
     `sweep` reclaims (count reaches zero, report fires once);
   - `next_preview_generation` monotonicity.
5. Gates: validator script, `cargo check --target x86_64-pc-windows-msvc`,
   whole-crate `cargo fmt`, `./build.sh`.

## Acceptance Criteria

1. **Preview slot lifecycle**
   - Given a registry with no preview binding
   - When a preview binding is published, replaced, and force-retired
   - Then `with_preview` sees the current one, a replaced binding is
     retired onto the retired list, and `retire_preview` empties the slot

2. **Both-slot retire coverage**
   - Given an active binding for file A and a preview binding for file B
   - When `retire_by_file(B)` then `retire_by_file(A)` run
   - Then each retires its binding (true), the other slot is untouched at
     the first call, and the unregister-prelude contract (retire before
     engine Destroy) is preserved by construction

3. **Routing order and hot path**
   - Given both slots live with different file ids
   - When `with_bound_for_file` runs for each id and for a third id
   - Then A resolves via the active slot, B via the preview slot, the
     third returns `None`; and with both slots empty `any_bound()` is
     false (the detour takes the trampoline with no lookup call)

4. **Reclamation and reporting**
   - Given a retired preview binding past its cooldown with no readers
   - When the drain sweep runs
   - Then the binding is freed exactly once with its metrics reported,
     and `retired_count` returns to zero

5. **Refusal mailbox independence**
   - Given interleaved gameplay and preview refusals
   - When both mailboxes are drained
   - Then each reports its own last-identity-plus-count and neither
     masks the other

## Metadata

- **Complexity**: Medium
- **Labels**: song-rate, binding, registry, io-hook, streaming
- **Required Skills**: Rust atomics/raw-pointer registry patterns, the song-rate serving architecture
- **Generated By**: code-task-generator 2026-08-15
- **Source Plan**: .agents/planning/2026-08-15-song-preview-rate/implementation/plan.md
- **Plan Step**: Step 2: Preview registry slot + io miss-path routing
