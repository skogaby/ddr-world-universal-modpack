# R1 / R2 / R6 / R7 — Mechanism verification (Ghidra)

All on `gamemdx_20260721.dll` unless noted; AOB checks also on `gamemdx_20260616.dll`.

## R1 — `sequence_finish` AOB uniqueness ✅

Pattern `48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 59 08 48 8B F1 8B FA F6 43 20 20`:

| Build | Matches | Address |
|---|---|---|
| 20260721 | 1 | `0x18021DF70` |
| 20260616 | 1 | `0x18021DB90` |

Both agree with `docs/quick_logout_research.md` §11.3. (Per user, 2-build
verification is sufficient; the doc had already covered 20260324.)

## R2 — the 0-idx 29 loader always loads `scene_result` ✅

Raw bytes at the `createNextSequence` case-`0x1E` mask selection
(`0x18002FEB1..`): `41 BC 00 00 01 00` (`MOV R12D, 0x10000`) is the
**unconditional default**, and the only other assignment on the path is
`41 BC 00 00 03 00` (`MOV R12D, 0x30000`) behind the extra-stage check
(`FUN_1801DD620 || FUN_1801DD660` — both false when entering from song select,
and harmless anyway: `0x30000 ⊃ 0x10000`). **Every** path through the 0-idx 29
loader makes `scene_result` resident before `TotalResultSequence` runs. The
`kind` argument (3/4/7 via the same branch chain) only affects background/movie
setup, not the load masks.

## R6 — forcing 0-idx 32 in course / event mode ✅ (no gates needed)

From the full decompile of `TotalResultSequence::update` (`FUN_1800C9A90`):

- **Course** (`GameWork+0x70 != 0`): state 0 takes the else-branch — it never
  touches the `scene_result` package. It requests shutter close
  (`FUN_1800334F0(2)`) *only if the shutter is currently open (state 0)*, then
  jumps to state 7, which waits for shutter==4 and calls `finish(this, 0)`.
  Entering from song select the shutter is open ⇒ close runs ⇒ state 7 fires.
  No crash, no hang — a summary-less wipe straight into the logout tail. The
  per-frame row-builder call at the function tail is also gated on
  `GameWork+0x70 == 0`.
- **Event/special** (`GameWork+0xD0 ∈ {1,2}`): the update reads `GameWork+0xD0`
  **nowhere**. And `getNextID` (`FUN_18002DD70`) has `case 0x21: case 0x39:
  return 0x22;` — the two total-results scenes share one successor, so the tail
  after a forced 32 is identical in event mode. Forcing 32 there just shows the
  normal summary chrome instead of the event variant. Confirmed safe; D6's
  "no mode gates" stands.

`getNextID` also confirms the doc's tail: `case 0x22: return arkEamOff() ? 0x24
: 0x23` (logout save runs whenever e-amusement is live) and `case 0x23: return
0x24`.

## R7 — course-session records ⚠️ one addition to the sanitiser

The marshal (see `savekind3-marshal.md`) selects records by
**`PlayerWork+0x4C == 10`** (style = course), not `GameWork+0x70`: a course
session saves the single course record at `PlayerWork+0x2D8` instead of the
5-slot array. Consequence for D22/D23: the sanitiser virginises **all 5 array
slots AND the course record** of a tainted side. The course-record offset
(`0x2D8`) is decodable from the already-matched `stage_record_accessor` bytes
(the `ADD RAX, imm32` at +36, currently wildcarded and undecoded).

Whether `GameWork+0x70` is non-zero while *browsing* courses at scene 25 turned
out not to matter: no mode gates remain (D6), the trigger reads neither field,
and `TotalResultSequence` handles the course fork safely on its own (R6).

## R4 — logout-request confirmation global: **skip**

`(&DAT_1806EC488)[side] = 0x13` is written by `PlayerDataSaveLogoutRequest`
(`FUN_18001EE90`), but the global has no anchor reachable from any signature we
already resolve — confirming it would need a new AOB for a log line. D14's
scene-chain timing diagnostics (WARN when 0-idx 34 is skipped or exits < 500 ms)
cover the same failure mode from the gamemdx side. Not worth a signature.

## R3 — numpad reachability inside the options modal: cabinet item

Deferred to the deploy test (low stakes — affects only whether the gesture
fires while the options modal is open; the modal is a child of scene 25, so the
scene gate passes either way).

## New concern found & resolved — re-entrant scene transition from the input callback

The trigger runs `finish()` from an `input_manager` callback; `finish` sends
message `0x201`, which *synchronously* runs `advanceToScene` →
`createNextSequence` → **our own `scene_manager` hooks** on the same thread.
Verified both sides are re-entrancy-safe:

- `input_manager::poll` dispatches callbacks **outside** its lock (it clones the
  callback list first — `src/services/input_manager.rs:393-413`), so the scene
  callbacks that fire inside `finish` can re-enter `input_manager` state without
  deadlock.
- `scene_manager`'s two hook bodies take `SCENE_MANAGER.lock()` in disjoint
  scopes (redirect lookup, then callback dispatch under a fresh lock), and
  nothing upstream of the trigger holds that mutex — the quick-logout mod must
  simply avoid holding any of its own locks across the `finish` call.
- Consequence to embrace: `scene_manager::current_scene()` is already `29` (the
  loader) by the time `finish` returns, which *is* the double-fire latch (D17).
