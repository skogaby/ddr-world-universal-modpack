# Context — Step 4 Task 02: Side Selection + First-Tick Diagnostic

**Task file:** `.agents/tasks/20260725-assist-tick/step04/task-02-side-selection-and-diagnostics.code-task.md`
**Mode:** auto. Approval chain verified (breakdown approved 2026-07-26; plan/design Approved).
Dependency: Step 4 task 01 complete.

## Requirements

1. Rebuild enumerates live gameplay actors: `dps = *(actor+0x08)`, walk `+0x18` first-child /
   `+0x10` next-sibling, raw vtable compare vs `gameplay_actor_vtable` (existing RTTI signature —
   consumed via `ctx.signatures.get_address`, NOT `require_address`; `required_signatures()`
   stays empty). Null checks on dps/vtable.
2. Containment check: walked list must contain the dispatched actor, else distrust → degraded.
3. Classify: any `style(+0x88) == 1` → doubles (expect 1 actor; 2 with doubles → treat as 2P,
   warn once); 1 actor → solo on its own side; 2 actors → sort by side FIELD, choose side 0
   (everything enabled until Step 5).
4. Build the list from the CHOSEN actor (may differ from dispatched); task-01 predicate unchanged.
5. Latch chosen ACTOR POINTER + side; per-dispatch identity = pointer compare. Scene callback
   remains the primary reset (covers allocator address reuse across restarts).
6. Degraded mode (walk empty / vtable unresolved / parent null / containment fail): dispatched
   actor, one WARN.
7. One-shot per-song diagnostic (design §7.2 items 1+3): dispatched/parent ptrs, sibling count,
   per-actor side+style, chosen side, results, kept — folded into the build line.
8. Panic-free; sibling walk bounded (max-iteration cap) so a corrupt chain cannot loop inside the
   judge dispatch.
9. Corrections honored: `+0x88` is an int (never a pointer); never assume doubles ⇒ side 0.

## Implementation notes

- Vtable stash: `static GAMEPLAY_ACTOR_VTABLE: AtomicPtr<u8>` set in `init` (precedent:
  `quick_restart_or_fail.rs:72,167` — but with `get_address`, since the mod degrades rather than
  requiring it).
- `SongState` latch: store the actor as `usize` (0 = none) to keep `SongState: Send` without an
  `unsafe impl`; keep `tick_side` alongside for logging and Step 5's enable gate.
- Walk cap: 64 iterations (a DPS has a handful of children; 64 is unreachable except corruption).

## Verification

- Agent: solo regression on the reference chart (same kept count, `siblings=1 sides=[0]`,
  identity check works, deltas unchanged); gates.
- Maintainer: 2P (same/different difficulty), doubles, solo-P2 listening rows — each checkable
  afterwards via the diagnostic line.
