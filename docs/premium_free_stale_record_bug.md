# Premium Free — Stale Stage-Record Bug (score saved under wrong difficulty)

**Status: root-caused, fixed, cabinet-verified 2026-07-10.**
All addresses file-relative to `gamemdx.dll` @ `0x180000000` unless noted.
Builds referenced: `20260526` (Ghidra primary) and `20260616` (cabinet build at
time of investigation).

## Symptom

With Premium Free (frozen stage counter) enabled: play the same song twice in a
row at *different* difficulties → the second play's score is submitted to the
backend under the **first** play's difficulty.

## Key structures

### Per-stage play record array (inside PlayerWork)

`PlayerWork + 0x590 + stage*0x2B8`, exactly **5 slots** (ctor:
`_eh_vector_constructor_iterator_(work+0x590, 0x2B8, 5, ...)` in the
PlayerWork ctor `FUN_1801e60f0` @0526). A separate **course-mode record** lives
at `PlayerWork + 0x2D8`, used when `GameWork+0x70 != 0`.

Record fields (record-relative):

| Offset | Meaning |
|---|---|
| +0x00 | mcode (music id; `-1` = virgin/empty) |
| +0x04 | **difficulty** (0=BEGINNER..4) |
| +0x08 | style (single/double) |
| +0x10/+0x14 | score / EX-score-adjacent |
| +0x28..+0x4C | judge counts |
| +0x50 | grade/rank |
| +0x54 | clearkind |
| +0x98/+0xB8/+0xD8 | judge/ghost vectors |
| +0x1A4 | "uploaded to e-amusement" flag (set by `FUN_1800c4350` @0526 post-save) |
| +0x268 | end-time qword (0 = not played; save-guard field) |
| +0x270 | clearkind (save-side; ==7 gates detailed-stats block, ==9 special) |

### Globals

- `DAT_1806f04f8` (both builds): global → ptr → **GameWork**. Stage counter =
  `*(i32*)(GameWork + 0xC)` (0-based). Course mode = `*(u64*)(GameWork+0x70) != 0`.
  `GameWork+0xD0 ∈ {1,2}` = event/special session modes.
- Player-work table: `DAT_1806f1ed0` (0526) / `DAT_1806f1ee0` (0616) — 2 slots,
  `table[side] → wrapper → PlayerWork` (same walk as the shipped
  `player_work_table` derived address).

### getStageRecord accessor (the fix's signature anchor)

`FUN_1800b57c0` (0526) / `FUN_1800b61c0` (0616) — a 60-byte leaf:
`(side, stage) → GameWork+0x70 ? work+0x2D8 : work + 0x590 + stage*0x2B8`.
Byte-identical across builds apart from the two RIP disp32s; sources every
layout constant the fix needs (see `stage_record_accessor` in
`core/signatures.rs`).

## Write/read paths around the record

| Path | Function (0526) | Role |
|---|---|---|
| Song-select commit | `FUN_1800fd580` (0616: `FUN_1800fd6d0`) | Writes (mcode, difficulty, style) + full record wipe via `FUN_1801e5380`, **guarded by `if (new_mcode != rec->mcode)`** |
| Record prepare | `FUN_1801e5380` (0616: `FUN_1801e4d50`) | The ONLY instruction site storing rec+0x00/+0x04; also zeroes scores/judges/ghost, sets grade sentinel |
| Result commit | `FUN_18005d180` | Writes scores/judges/grade/clearkind/end-time at song end; does NOT touch +0x00/+0x04 |
| PlayerWork reset | `FUN_1801e7000` | Session reset: all 5 records + course record → (mcode=-1, diff=0, style=2) |
| Save marshal | `FUN_180018580` (`ark::network::ReflectSavePlayerData(player, savekind, stage)`) | Copies the record into the save staging block; **wire difficulty = rec+0x04**, wire mcode = rec+0x00, stagenum = passed stage |
| Stage index source | `FUN_1800fc4f0` | Returns `GameWork+0xC` (or an override global `DAT_1806f290c` when `GameWork+0xD0 ∈ {1,2}` && counter==0) |

Save semantics (`ReflectSavePlayerData`): `savekind==2` (per-stage save)
marshals ONE record — `work + 0x590 + stage_param*0x2B8` — unconditionally;
`savekind==3` (game end) loops stages `0..min(counter,4)`, skipping records
with `mcode == -1 || end_time == 0`, compacting written entries. The
per-stage `SavePlayerDataActor` (ctor `FUN_1800b4080`, log
`"SavePlayerDataActor:%dP Stage%d"`) captures the stage index at construction
during the results flow, and the marshal runs within the first frames of the
results screen — well before the next song-select transition.

## Root cause

1. pfree NOPs the counter INC (`GameWork+0xC` frozen) → every play reuses the
   same record slot.
2. The song-select commit initializes the record **only when the picked mcode
   differs from the record's mcode**. In vanilla the guard always passes (a
   fresh stage's record is virgin, mcode=-1). With a frozen index, re-picking
   the *same song* skips the re-init entirely — `rec+0x04` keeps the previous
   play's difficulty (and the score wipe is skipped too).
3. The result commit writes fresh scores into the stale record; the per-stage
   save marshals fresh score + stale difficulty onto the wire.

Cabinet-confirmed with a CE hardware write-watch on `rec[0]+0x00..0x07`
(20260616): play 1 (BEGINNER) → exactly one write pair from the record-prepare
helper; play 2 (same song, EXPERT) → **zero** writes, record still read
difficulty 0 during EXPERT gameplay.

## Fix (shipped in `mods/premium_free.rs`)

On each scene transition into SONG_SELECT while the freeze patch is active,
write `mcode = -1` into `work + rec_base + frozen_stage*rec_stride` for both
players (course mode skipped — its init is unconditional). This restores the
vanilla invariant "the current stage's record is virgin during song
selection", so the game's own commit path performs its full re-init (fresh
difficulty + clean wipe) on the next decide. Manually validated via CE (write
`-1`, re-pick same song at a third difficulty → game re-committed the record
with the correct difficulty), then end-to-end on the cabinet: two plays of the
same song at different difficulties both saved under their true difficulties
in the backend.

Timing safety: the previous play's save payload is marshaled from the record
during the results screen (frames after results entry); the SONG_SELECT
transition is seconds later. Worst-case failure mode is a `-1` mcode in a
`/result` entry (backend-rejectable), never a mis-attributed difficulty.

**Version-agnosticism:** all constants decoded from the matched
`stage_record_accessor` bytes (game-work global RIP@+3, player-work table
RIP@+16 cross-checked against the derived `player_work_table`, course field
disp8@+23, stride imm32@+47, base disp32@+55); stage-counter offset read from
the patched INC's own disp8. Range + module-bounds sanity checks; any anomaly
fails the mod closed (freeze without the fix poisons server data).

**Known limitation:** event/special modes (`GameWork+0xD0 ∈ {1,2}`) may use
the override record index (`DAT_1806f290c`) at counter==0; the fix targets the
counter-indexed record, so that niche combination retains pre-fix behavior.
