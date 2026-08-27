# Research: 2-Player Custom-Option Load Misrouting (pre-existing bug)

Surfaced while testing the score feature in a 2P session. **Independent of the
score feature** — a pre-existing defect in the `20260531-custom-options-json-persistence`
load path. Captured here with the RE needed to fix it.

## Symptom

In a 2-player session, custom in-game options load with **default values** for both
players (P2's saved options especially). In a 1-player (P1) session they load
correctly. Confirmed in `log.txt` (2026-06-10 session):

- 2P session: cards P1 (`E0040159F4D19F72`) + P2 (`E00401D96424D35A`) inserted; **two**
  `load_receiver` hits, **both** reported `side 0` (`loaded 11/16 ... for side 0`).
  P2's values were written to side 0; side 1 never primed → P2 sees defaults.
- 1P session: single load, `side 0` — correct, because solo P1 *is* side 0. That's
  why the bug only manifests with two simultaneous loads.

## Root cause

`load_receiver_trampoline` derives the side from `*(job as *const u32)` (`job[0]`).
The diagnostic dump proves `job[0]` is **always 0** — for both P1 and P2 loads, the
entire dumped job header is identical (`job[0]=0, job+0x08=0x1, job+0x10=…ce0,
job+0x18=…d50`). The ess `sys_playerdata_load_receiver` uses a **single shared
savedata buffer** (`savedata = *(job+0x18)`), reused per call; the player side is
**not** expressed anywhere in the job/savedata as an index. So `job[0]` was never a
valid side source — it happened to work only for solo-P1.

The save path is unaffected (it reads `*(savedata+0x90)=playside` from a per-side
save buffer, verified correct in the same log: P2 saves show `side 1`). Save and
load are architecturally different; the save fix does not transfer.

## The correct discriminator: ddrcode join (live-confirmed in CE)

The side must be recovered by matching the load's **ddrcode** (numeric DDR ID) to
the per-side `PlayerWork` that holds the same ddrcode, then reading that
PlayerWork's table index.

| Datum | Location | Notes |
|---|---|---|
| Load savedata base | `*(job + 0x18)` | the ess load receiver's `param_1+0x18`; reused buffer |
| ddrcode (incoming) | `*(savedata + 0x48)` | parsed by `sys_playerdata_load_receiver` as `("ddrcode", 6 /*s32*/, *(job+0x18)+0x48)` |
| `player_work_table` | derived global (RVA `+0x6F1ED0` this build) | 2-slot array; `table[side]` → wrapper, `*wrapper` → PlayerWork |
| ddrcode (profile) | `PlayerWork + 0x18` | **live-confirmed**: P1=90553528 (`0x05658CB8`), P2=76127136 (`0x04899BA0`) — match the operator's two cards |
| side (profile) | `PlayerWork + 0x00` | 0 / 1; equals the table index (research doc PlayerWork+0x00 = side) |

**Live validation (Cheat Engine, 2P session):**
- `player_work_table[0]` → wrapper `0x107BF7A0` → PlayerWork `0x1B7ED220`; `+0x00`=0,
  `+0x18`=90553528.
- `player_work_table[1]` → wrapper `0x105D2920` → PlayerWork `0x1B7EEB30`; `+0x00`=1,
  `+0x18`=76127136.
- Dancer names at `PlayerWork+0x0C` ("AAAAAAAA" / "BBBBBBBB") corroborate distinct
  per-side profiles.

(Note: `PlayerWork+0x50` also differs per side — 366581/366546 — but is NOT the
ddrcode; the operator-supplied ddrcodes matched `+0x18`, not `+0x50`. Don't use +0x50.)

## Timing correction (live-confirmed) — the join must be DEFERRED

First fix attempt resolved the side *inside* `load_receiver` (read ddrcode from
savedata, match to PlayerWork+0x18). Cabinet log proved it always misses:

```
load — could not resolve side from ddrcode=90553528 (defaulting to side 0)   (P1)
load — could not resolve side from ddrcode=76127136 (defaulting to side 0)   (P2)
```

The ddrcodes read from the savedata are **correct** (90553528 / 76127136), but
`side_from_ddrcode` returns None because **`PlayerWork+0x18` is not populated when
`load_receiver` runs** — the profile load is what *fills* PlayerWork, so the join
target doesn't exist yet. CE confirmed both PlayerWorks hold the correct ddrcodes
*after* the load (P1=90553528 @ live PlayerWork `0x19DB91A0+0x18`, P2=76127136 @
`0x19DBB2C0+0x18`). So the offsets/values are right; only the timing was wrong.

**Fix: defer application to SONG_SELECT (scene 25) entry** (maintainer-chosen
trigger — profiles guaranteed in memory by then). `load_receiver` now only
*captures* `(ddrcode, [mod values])` into a `PENDING_LOADS` buffer; a scene-change
callback drains it on SONG_SELECT entry, resolving ddrcode→side then (PlayerWork
populated) and calling `resolve_from_load` per value. Network still wins over the
JSON prime (drain runs on SONG_SELECT, after the ~12s JSON timer).

## Fix plan (implemented)

1. `custom_options_persistence::init(signatures)` — take the `SignatureStore`, resolve
   and store `player_work_table` (optional; absence → fall back to current behavior).
2. New `side_from_ddrcode(ddrcode) -> Option<u8>`: for `side in 0..2`, walk
   `table[side]` → `*wrapper` → PlayerWork, compare `*(PlayerWork+0x18)` to `ddrcode`;
   return the matching index.
3. `load_receiver_trampoline`: after the original parses the response, read
   `ddrcode = *(*(job+0x18) + 0x48)`, resolve `side` via `side_from_ddrcode`. Fall
   back to the legacy `job[0]` only if resolution fails (table unresolved / no match),
   logging a WARN. Emit a one-shot INFO with the resolved (ddrcode, side) for
   cabinet confirmation.

**Deploy validation:** 2P session, both players with distinct saved custom options →
each player's options load correctly (P2 no longer shows defaults); log shows two
loads resolving to side 0 and side 1 respectively, with matching ddrcodes.

## Status: FIXED + cabinet-verified (2026-06-11)

Confirmed on a carded 2P session: each player's network options load to the
correct side, and network values override the JSON-primed cache as designed.

Implementation (all in `custom_options_persistence.rs` unless noted):
- `init(signatures)` resolves `player_work_table` (RVA `+0x6F1ED0`); `lib.rs`
  step 4i passes `&signatures`.
- `load_receiver_trampoline` no longer resolves the side or applies values; it
  reads the load's `ddrcode` (`*(*(job+0x18)+0x48)`) and stashes
  `PendingLoad { ddrcode, values }` into `PENDING_LOADS`.
- `register_pending_load_drain()` registers a `scene_manager::on_scene_change`
  callback (once) that calls `apply_pending_loads()` on `scene::SONG_SELECT`.
- `apply_pending_loads()` drains the buffer, resolves each `ddrcode → side` via
  `side_from_ddrcode` (PlayerWork+0x18, now populated), and calls
  `resolve_from_load` per value.

**Two bugs found and fixed along the way (both via diagnostic logging):**
1. *Timing* — resolving the side inside `load_receiver` always missed because
   `PlayerWork+0x18` is populated only *after* the load; fixed by deferring to
   SONG_SELECT.
2. *Init order* — `register_pending_load_drain` originally gated on
   `scene_manager::is_available()`, but persistence init (step 4i) runs *before*
   `scene_manager::init()` (step 5), so the gate was always false and the drain
   callback was never registered. Fixed by dropping the gate — `on_scene_change`
   only appends to the callback list and doesn't require the detour to be live
   yet (it fires after step 5, before any real scene change).

## Scope note

Pre-existing; not caused by the score feature (the score work added only a
`reset_session()` call to this trampoline, which doesn't touch side derivation).
Fixing it now per maintainer direction.
