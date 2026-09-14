# Versus Impersonation — RE Findings (Step 4)

Ghidra pass on `gamemdx_20260825.dll` (primary) with cross-checks on `20250805` where a
layout could differ. Addresses are file-relative to `0x180000000`. Globals on 20260825:
GameWork ptr-ptr `DAT_1806f14f8`, PlayerWork wrapper table `DAT_1806f2ee0` (both already
derived at runtime by `src/services/stage_records.rs` from `stage_record_accessor` —
never hardcode them).

Verdict: **architecture B ("windowed impersonation") is confirmed at every seam.** The
game's own code models a cardless entered side (`PW+0x4 = 1, +0x5 = 0, +0x8 = −1,
+0x18 = 0, +0x1C = 0`) — the BPL guest-join helper (§3) builds exactly that shape.

## 1. U1 — the stage loader reads `PlayerWork+0x4` (CONFIRMED, both builds)

`TransitionSequence::createNextSequence` case `0x1d` (0-idx 28, gameplay) — 20260825
`FUN_18002e3b0` @ `0x18002fb03..`, 20250805 `FUN_18002e140` @ `0x18002fb03..` (same shape):

```
for side in 0..2:
    struct[side].entered    = *(u8*)(*table[side] + 0x4)      ; PlayerWork+0x4
    struct[side].is_main    = (GameWork+0x8 == side)
    struct[side].is_double  = (GameWork+0x4 == 1)
    struct[side].difficulty = FUN_1801e89b0(wrapper)          ; PW+0x5C, clamped
    struct[side].u64        = 0
DPS = DancePlaySequence::ctor(alloc(0x138), song_name, &struct[0], &struct[1], flag=0)
```

`DancePlaySequence::ctor` (`FUN_1800570a0`) heap-copies the two 16-byte structs to
`DPS+0xF0` / `+0xF8`. `DancePlaySequence::onUpdate` (`FUN_180057e10`, vtable
`0x180360ad8` slot 6) case 1 creates a `GamePlayActor` for each side whose
`side_info->entered != 0` (`plVar12[-2]`, loop over `DPS+0xF0..`), storing them at
`DPS+0x100/+0x108`; the LayoutActor is `DPS+0x110` (already what
`song_reset::gameplay_actors` walks). Everything else in the DPS (SSQ path build in
`onSetup` `FUN_1800573d0`, layout style array, song-info actors) runs for BOTH
`side_info` pointers regardless of `entered` — matching the r2 finding that the layout
style array is not a presence signal.

⇒ Setting `PlayerWork[bot]+0x4 = 1` before scene 28 is created yields a second
GamePlayActor natively. `GameWork+0x4` is **style** (1 = double), `GameWork+0x8` is the
**primary side** — settles the doc conflict (U5 of the orientation list).

## 2. U2 — who writes `PlayerWork+0x4`

Only two writers in the binary (byte pattern `C6 ?? 04 01` filtered):

| Writer | Site | What else it writes |
|---|---|---|
| `EAmEntryWindowActor::update_ARK_ENTRYFLOW_CREDIT_READY` | `FUN_180093b60` @ `0x180093e0d` (scene 19) | `PW+0x5 = 1` iff `!FUN_18001c7d0(side)` (registered player), then `FUN_180090f90` (profile/name) |
| BPL guest-join helper | `FUN_1800b2d10` (called from the SelectStyle START handler `FUN_1800b29f0` when VERSUS is chosen with an empty side) | `PlayerWork::reset(wrapper,1)`; refid `"X%020lld%s"` from the PARTNER's `PW+0x1748`; `+0x1748/+0x1750`, `+0x1758 = now`; **`PW+0x4 = 1`**; ark entry-flow notify `DAT_1806f2af8(side, {0,0x100,0})` |

Nothing in scenes 24–35 writes `+0x4`. `PlayerWork::reset` (`FUN_1801e7fb0`) sets
`+0x4 = 0, +0x5 = 0, +0x8 = −1, +0xC..= 0, +0x18 = 0, +0x1C = 0, +0x50 = 0,
+0x54 = −1, +0x5C = 0`, records `(mcode −1, diff 0, style 2)`, Option/Customize reset.
**U4 answered:** a never-entered side has `PW+0x8 = −1`, so the EAM-exit settle-up
(`arkExpireCredit` gate `PW+0x4 && PW+0x8 >= 0`) can never fire for the bot even if the
flag were still set — and D8 restores it long before scene 34 anyway.

## 3. U3 — `GameWork+0` semantics and writer (CONFIRMED)

`SelectStyleSequence::onUpdate` (`FUN_1800b0bc0`) STEP_WAIT case 2, after both sides'
mode windows settle:

```
count = 2 − (sides that terminated / made no selection)
GameWork+0x8 = primary side (highest-priority selection)
if count == 2 { GameWork+0x0 = 1; GameWork+0x4 = 0 }          ; versus forces SINGLE
else          { GameWork+0x0 = 0; GameWork+0x4 = chosen style }
for each side with PW+0x4: Option(PW)->vfunc[2](side, GameWork+0x4)   ; FUN_1801ea0e0
log "SSTYLE ... stylev" (versus) / "styles" / "styled"
```

`GameWork+0` is written ONLY here (scene 20) and by `GameWork::reset`. It is read as a
pure display/layout selector in the play window — every reader found:

| Reader | Effect of `GameWork+0 == 1` |
|---|---|
| `ResultSequence` build (`FUN_1800b9030`, scene 30) | `player_%dp_info_usr`, `profile_usr` and the tab `WindowActor` are populated for side *i* iff `GameWork+0 == 1 \|\| i == primary` (`ResultSequence+0xF4 = GameWork+8`); the tab's "versus" byte (`tab+0x134`) = the same predicate |
| `ResultSequence::onUpdate` (`FUN_1800bc120`) case 2 / case 0x11 | same predicate for the BPL rank write-back and the per-side detail rows |
| `createNextSequence` cases `0x16` (CAUTION), `0x2e`, `0x3b`, `0x24` | `FUN_1801aa500(GameWork+0 == 1)` → sound manager `DAT_1806f2d68 + 0x20c4` = **versus SE pan flag** (read by `FUN_1801aa220`: side 0 SEs panned `DAT_18035a704`, side 1 `_DAT_180359f64`, centre when 0); `FUN_18000c6f0(…, versus)` at THANK YOU |
| `ReflectSavePlayerData` (`FUN_180018ee0`) | staging `+0x44` (`savekind==1 ? 2 : GameWork+0`) and `+0x12C` (`GameWork+0`) → wire `/data/mode` and `/data/battle_mode` (see §7) |
| `two_player_bpl_mode` (DLL) | eligibility gate + layout selector |
| `MatchingDancePlaySequence::onUpdate` | `FUN_180069ed0(…, GameWork+0, …)` — the online-battle DPS only |

⇒ Writing `GameWork+0 = 1` at the flip and `0` at the restore is sufficient. Nothing
re-derives it from the entered flags during the window.

## 4. U6 — the song-select commit and the bot's record (CONFIRMED)

Commit `FUN_1800fdc90` (20260825):

```
for side in 0..2:                                   ; BOTH sides, entered or not
    rec  = record[FUN_1800fcc00()] of PW[side]      ; stage counter (or event override)
    diff = FUN_1801a7880(model, *(seq + 4 + side*4)) ; that side's OWN cursor
    if mcode != rec->mcode:
        FUN_1801e6010(rec, mcode, diff, GameWork+4) ; record prepare (header + wipe)
        FUN_1800fcc70(seq, diff, side)              ; PW+0x5C (+ cursor), clamp ≥1 for DP
```

`FUN_1801e6010` stores only `rec+0x00 mcode, +0x04 diff, +0x08 style` and wipes
counters/streams — no difficulty-dependent state. The non-entered side's difficulty comes
from ITS cursor (`seq+4+side*4`), which nothing set in a 1P session ⇒ **the bot's record
would carry the wrong difficulty** unless fixed. The fix at the flip is three field
copies from the human's PlayerWork: `rec_bot+0x04 = rec_h+0x04`, `rec_bot+0x08 =
rec_h+0x08`, `PW_bot+0x5C = PW_h+0x5C` (and `+0x50`, `+0x54` for completeness). Precedent
for the game itself mirroring difficulty across sides: `FUN_1800fcc70` writes BOTH
sides' `PW+0x5C` in event modes.

Downstream readers of the bot's record header: the READY panel (`rec+0x04`,
`docs/premium_free_stale_record_bug.md`), the loader (`PW+0x5C` → DPS struct), the
results pane (`rec+0x04/+0x50/+0x54`).

## 5. U7 — results screen (CONFIRMED)

`FUN_1800b9030` (ResultSequence build, run from `onUpdate` cases 6/7): both `WindowActor`s
are created unconditionally (`FUN_1800c3a30(alloc 0xF0, root, side, primary)` with the
versus byte), and every per-side pane is shown by the `GameWork+0 == 1 || side == primary`
predicate — NOT by `PW+0x4`. `PW+0x4` is consulted only for the name (`PW+0x4 && name[0]
== 0 ⇒ "PLAYER1/2"`, else `PW+0xC`) and a PASELI BGM variant (`PW+0x4 && PW+0x8 == 3`).
Pane content = `record[ResultSequence+0xEC]` of `PW[side]`: rank `+0x50` → `scre_rank_*`,
clearkind `+0x54` → FC emblem, `+0x04` difficulty, flare `FUN_1801e61c0(rec)`, score/EX via
`FUN_1800bb910(seq, side)` — all written by the bot's own GamePlayActor result commit.
Tab kind (guest "Simple" vs registered "Details") follows the stock cardless path.

The `ResultSequence` ctor (`FUN_1800b6990`) per-side loop runs for `PW+0x4` sides: local
score-DB PB update `FUN_1801e2c40(PW+0x178, mcode, style, diff)` (in-memory, harmless for
the bot), rank/summary fields, and — gated on **`PW+0x5 != 0`** — the network rival/event
calls (`FUN_18001c9d0`), which the bot therefore skips.

## 6. U5 — saves for a cardless side

`SavePlayerDataActor::onUpdate` (`FUN_1800b53c0`): the per-stage actor waits in state 0
while `PW+0x1C != 0 && FUN_1801de420(2) && !(rec+0x1A4)`, then state 2 requires
`FUN_1800139a0(side, …)` — the ark entry-flow side-state check used throughout the entry
code — before `FUN_18001ecf0()` sends. A never-entered side sits at ark scene 0
(`ARK_ENTRYFLOW_NOENTRY`, `docs/quick_logout_research.md`) so the send never fires; the
DLL's ess `save_sender` detour has only ever observed one invocation per carded-in side.
The autoplay taint on the bot side (`score_guard::set_autoplay_taint`) is the backstop:
`savekind==2` suppressed, `savekind==3` sanitised. Cabinet verification: the trampoline
logs every invocation with side + kind.

## 7. U8 — the human's save marshal under versus (CONFIRMED, harmless)

`ReflectSavePlayerData` writes `GameWork+0` into two staging fields (`+0x44`, `+0x12C`)
that surface as the top-level `/data/mode` and `/data/battle_mode` scalars of the World
`playdata_3.playerdata_save` request (captured 1P saves show `mode=0, battle_mode=0`).
Neither server reads them: bemani-buddy's `handle_save_scores` never parses either
(`crates/game-server/src/handlers/ddr_world/playdata.rs`), and bemaniutils has no World
handler (its Ace analog `playstyle == 2` only picks a play-count bucket). Score identity
is `<result>/style` + `difficulty` — untouched. The per-stage save fires DURING scene 30
(flip active), so the human's save will carry `mode=1` in bot sessions. Accepted as a
known wire-visible side effect (it doubles as a future server-side bot marker).

## 8. Extra-stage grant considers every entered side (NEW side effect)

`FUN_1801ddcd0(0)` — called from `ResultSequence::onUpdate` case `0x16` (results
window-out) when `stage_counter == 0` — grants the extra stage (`GameWork+0x59 = 1`) only
if **every side with `PW+0x4 != 0`** has `record+0x50 (rank) >= 0xF`, payment normal
(`PW+0x1710 == 0`), gauge option ∈ {0, 0xC}, and `record+0x270 != 7`; gated on `!course`,
`GameWork+0x4 != 1`, `max_stage == 2`. With the bot entered at that moment, a low-level
bot that fails to AAA blocks the human's extra stage. The bot can never ADD a grant (the
human must still pass). Mitigation options recorded in the register (D21).

## 9. Flip / restore recipe (what the design implements)

Flip — at the scene change OUT of SONG_SELECT (25 → 26), after the commit has run:

1. Gate: exactly one `side_entered`, `GameWork+0x4 == 0` (SINGLE), `GameWork+0x70 == 0`
   (not course), `event_mode == 0`, `GameWork+0 == 0`, option ON for the entered side,
   `stage_records::is_available()`.
2. `bot = !human`. Snapshot `PW_bot+0x4`, `PW_bot+0xC..+0x14` (name), `GameWork+0`.
3. Mirror chart identity: `PW_bot+0x50/+0x54/+0x5C ← PW_h`, `rec_bot+0x04/+0x08 ←
   rec_h` where `rec = stage_record(side, stage_counter())`.
4. Copy the human's `Option` block (`PW+player_option_offset`, 0x98 bytes) into the bot's;
   force gauge `Option+0x18 = 0` (NORMAL) — see `bot-controller-re.md` §5 for the layout.
5. Name: `PW_bot+0xC = "BOT LV<n>\0"`.
6. `PW_bot+0x4 = 1`; `GameWork+0 = 1`; optionally sound-pan byte
   `*(sound_manager + 0x20c4) = 1`.
7. `score_guard::set_autoplay_taint(bot, true)`; arm the bot controller for `bot`.

Restore — at the first scene change INTO any scene ∉ {26, 27, 28, 29, 30}: undo 6 and 5
(restore snapshots), taint off, controller disarmed. Idempotent; also runs on mod disable.

## 10. Cross-build notes

- Loader struct build byte-shape identical on 20250805 and 20260825 (§1). AOB the loop
  (`MOVZX EDX,byte ptr [RAX+4]; MOV [R8-1],DL; CMP R11,R9; SETZ AL …`) only if a
  signature is wanted for attestation; the DLL needs NO new signature for the flip — every
  address involved is already derived (`stage_records`, `player_option_offset`).
- PlayerWork header (`+0x4, +0x5, +0x8, +0xC, +0x18, +0x1C, +0x50..+0x5C`) is identical
  across builds; only the record base (`0x570/0x590`) and Option offset (`0xF0/0xE0`)
  differ — both already derived.
- `GameWork+0x0/+0x4/+0x8/+0xC/+0x70/+0xD0` identical across builds.
