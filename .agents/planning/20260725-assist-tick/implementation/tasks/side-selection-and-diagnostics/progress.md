# Progress — Step 4 Task 02: side selection + diagnostics

- [x] `GAMEPLAY_ACTOR_VTABLE` stashed at init via `get_address` (opportunistic;
      `required_signatures()` stays empty)
- [x] `enumerate_actors`: sibling walk from `*(actor+0x08)` (`+0x18`/`+0x10`), raw vtable compare,
      bounded at `MAX_SIBLING_WALK = 64`, containment check as the end-to-end validity proof;
      `None` = degrade
- [x] `choose_actor`: FR-5 with every side enabled (doubles/solo → the actor; 2 actors → min by
      the side FIELD, never list position; 2 actors + doubles style → warn once, treat as 2P)
- [x] Both research corrections encoded at the constants: `+0x88` is an int enum (never
      dereference), `doubles ⇒ side 0` never assumed (style field is the doubles discriminator)
- [x] Degraded mode: dispatched actor + exactly one WARN per session (`DEGRADED_WARNED` latch)
- [x] Latch = chosen actor pointer (stored as `usize`, `0` = inert, never dereferenced after the
      rebuild) + side; `tick_clock` identity check is now a pointer compare
- [x] List built from the CHOSEN actor (reachable via the walk even when the other side dispatched
      first); task-01 predicate unchanged
- [x] Diagnostic folded into the build line: `dispatched= siblings= sides= styles= chosen_side=`
      + the task-01 stats (closes design §7.2 items 1 and 3 — the data is now emitted per song;
      the 2P/doubles observations themselves are the maintainer's sessions to run)
- [x] Gates: `cargo check` 0, `cargo fmt` clean, `./build.sh` 0; installed

## Verification record

Solo regression (Ace out, the reference chart):

```
AssistTick: song build -- dispatched=0x9c7b6b0 siblings=1 sides=[0] styles=[0] chosen_side=0
            results=438 kept=340 rej_kind=7 rej_shock=91 rej_panel=0 rej_neg=0 coalesced=0 first=[8888, ...]
```

- Walk validated on the live game (no DEGRADED marker → vtable resolved, parent non-null,
  containment held); kept count identical to task 01 (AC6); tick lines fired normally (10 logged,
  deltas ≈ −148 with the 150 ms offset); zero AssistTick WARNs, no crash records.
- AC4 (degraded path): verified by inspection — all four bail-outs (null vtable, null parent,
  cap hit ending without containment, containment fail) funnel through the single
  `enumerate_actors → None` path, whose fallback is byte-for-byte Step 3's behaviour. No forced
  probe shipped; the healthy path exercising the same code shape live plus the WARN latch's
  triviality made a probe boot add nothing.
- AC1 (solo P2), AC2 (2P same/different difficulty), AC3 (doubles), AC5 (restart choice-once):
  maintainer's listening sessions — each row now checkable afterwards in the log via the
  diagnostic line (`siblings/sides/styles/chosen_side`). The doubles line will also settle the
  open `+0x84`-in-doubles question (record what it reads).

No deviations. Commit deliberately not made (maintainer owns commits).

Status: Complete
