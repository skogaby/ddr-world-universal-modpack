# R3 — Does World actually exhibit the menu-animation speedup?

**Status: mechanism analysis CONFIRMS World is delta-time-based; "no speedup" corroborated
by a live World test. Definitive answer is empirical (cabinet) — and already looks positive.**

This is the **high-impact** question from idea-honing Q8. The prior doc
(`hex_edit_porting.md` Hack 5) asserted that raising the FPS target makes menu/selection
animations run too fast (frame-counted advances). The maintainer's friend **tested the
actual World FPS-unlock hex edit and did NOT observe the speedup**; the speedup is
**confirmed real on older DDR versions** (pre-World) that were similarly hex edited. Q8
hypothesis: the prior doc carried older-version behavior into World by assumption.

## What the binary shows (build 20260324)

### The engine is overwhelmingly delta-time based

- **Global frame delta `DAT_1806ea714`** is computed every frame in the tick function
  `FUN_18020e230`:
  ```c
  now      = FUN_18023be70();                  // tick clock
  dt_meas  = (now - prev) * _DAT_1803573ec;    // scale to seconds
  if (DAT_1806ea70c < dt_meas)                 // CAP (clamp)
       DAT_1806ea714 = DAT_1806ea70c;          // clamp to cap
  else DAT_1806ea714 = dt_meas;                // else real dt
  FUN_18020dfe0();                             // smoothed-FPS bookkeeping
  ```
- **The clamp (`CAP`) is `DAT_1806ea70c`**, computed once in `onBoot`:
  `DAT_1806ea70c = DAT_1803574fc / DAT_1806ea700` where `DAT_1806ea700 = DAT_1803573b4`.
  (Prior doc described this as `DAT_18045f114 / 59.94`; the cap exists and bounds per-frame
  dt so dt-scaled motion can't teleport at low FPS. The exact constant operands differ from
  the prior doc's description but the role is confirmed: it's the per-frame dt clamp.)
- **`DAT_1806ea714` has ~100 readers** across gamemdx (xref count this session matches the
  prior doc's "~120 functions"). Anything multiplying motion by it stays correct at any FPS
  — this is why **gameplay arrow scroll is smooth/correct** at high FPS (the desired case).

### Representative animation reader DOES scale by delta

Sampled `FUN_18021a330` (an effect/particle update, a `DAT_1806ea714` reader):
```c
fVar3 = DAT_1806ea714;                  // global frame delta
if (DAT_1806ea4cd != '\0') fVar3 = DAT_1806ea654;   // (alt timebase, e.g. paused)
FUN_18021ddd0(..., DAT_1806ea67c * fVar3);          // advance scaled BY dt
```
i.e. it advances animation by `rate * dt`, not a fixed per-tick step. This is the
**dt-correct** pattern — at higher FPS each step is proportionally smaller, so wall-clock
speed is unchanged. Consistent with "no speedup on World."

## Honest assessment

- **Static analysis cannot prove a negative** across *every* animation path in an 18k+
  function binary. There may still exist isolated frame-counted advances (the prior doc's
  claimed failure mode) — but the engine's *dominant* pattern is dt-scaling, and the one
  animation reader sampled here scales by dt.
- **The authoritative test is empirical**, and it has effectively already been run: the
  **live World hex-edit test showed no menu speedup**. Our mod produces the *same* effect
  as that hex edit (same imm32), so we expect the same result.

## Impact on scope (decisive)

Combining R3 + R2:
- **R2:** the FPS target is consumed once at boot (latched into the D3D device); live
  per-scene rewrite is **infeasible** without a device reset.
- **R3:** World appears delta-time-correct and the live test shows **no menu speedup**.

→ **Milestone 2 (per-scene auto-switch) should be DROPPED, not just deferred.** It is both
(a) likely unnecessary (no speedup to fix) and (b) not cleanly implementable via this lever
(value isn't re-read). The **static global FPS value is the entire feature.**

→ **Overlay hint stays neutral** ("Display refresh target.") per Q8 — no side-effect
warning unless the cabinet test surprises us.

## Confirmation step (in the plan, not blocking design)

The Milestone-1 deploy itself confirms this for free: set a high FPS (e.g. 144), then on
the cabinet observe (a) gameplay scroll is smooth, and (b) menu/selection animations run at
**normal wall-clock speed**. If — contrary to the live test and this analysis — menus *do*
speed up, only THEN revisit per-scene gating (and R2's "infeasible" verdict would force a
different lever entirely, e.g. a device-reset path — a much larger effort to be scoped
separately).
