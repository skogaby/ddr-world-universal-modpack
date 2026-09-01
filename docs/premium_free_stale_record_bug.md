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

---

## Addendum 2026-09-01 — same-credit ghost cache + diagnostics (gamemdx 20260721)

Prompted by two field reports (results **graph tab** shows the previous
song's timing stats with an empty visualisation; **STAGE_INDICATOR** shows the
previous difficulty on a same-song/different-difficulty re-pick) and by the
DDR A3 `pfree_ghost` community hook.

### Verified record path under the freeze

| Piece | 20260721 | Stage index used |
|---|---|---|
| Song-select commit | `FUN_1800fdfa0` | `FUN_1800fced0()` (= `GameWork+0xC` or the event override). Guard: `if (new_mcode != rec->mcode) { FUN_1801e5110(rec, mcode, diff, style); FUN_1800fcf40(this, diff, side); }` |
| `FUN_1800fcf40` | | Clamps the difficulty (`style==1 ⇒ ≥1`, `≤ this+0`) and writes **`PlayerWork+0x5C`** (+ `SelectMusicSeq+4+side*4`) — the side's "selected difficulty" display field. **Also under the mcode guard**, so the virginise fix refreshes it too. `PlayerWork+0x54` = selected mcode. |
| Result commit | `FUN_18005d970` (GamePlayActor vt+0x28) | `GameWork+0xC`. Early-outs: `actor+0x280 != 0`, `actor+0x288 != 0` (commit skipped entirely). Stream writers **replace**: `FUN_180060760` → `rec+0x98` (note entries, 0x60 stride, from `actor+0x90`), `FUN_1801e5510` → `rec+0xB8` grades + `rec+0xD8` ms (from the active-note ring `actor+0xB0`, trimmed at the first note with `+0x30 == 0`), `FUN_1801e53e0` → gauge map `rec+0x78/+0x80` (from `actor+0x110` per-second doubles). |
| GraphTab ctor | `FUN_1800eb3a0` | stamps `tab+0x14C = GameWork+0xC` at results build (scene 30, BEFORE the scene-31 bump) |
| GraphTab ingest (vslot 6) | `FUN_1800eb9c0` | `rec = PW+0x590+tab_stage*0x2B8`. Numeric stats: mean `tab+0x1C8` / stddev `tab+0x1D0` from `rec+0xD8` (grades 0..3 only). Graph window: first note with `+0x18==0` → `tab` start; `has_data (tab+0x1C4) = first_ts < max(note_ts + max(len[8]))`. Series bucketed by `(ts − first)/1000`. Also reads `PlayerWork+0x1D/+0x68/+0x6C/+0x70/+0x74` → `tab+0x138/+0x1C0` (page / cycling params). |

Conclusion: commit, result commit and GraphTab all index the SAME frozen
record and every graph-tab input is rewritten by the result commit — no
static mechanism found for report 1 on the shipped code. The only escape is
a skipped result commit (early-outs). Report 2's display field sits under the
same guard the fix opens. Both are therefore instrumented (see below) rather
than "fixed" blind.

### The ghost bug (real, A3-identical)

`sequence::dance::GhostActor` init = **`FUN_180056ad0`** (A3's
`GhostInit`; A3's 64-bit actor offsets match World exactly):

| Field | Offset | Meaning |
|---|---|---|
| state pairs | `+0x58 + idx*8` (`i32 state, f32 timer`), idx at `+0x82` (u16), count `+0x80` | 0 = network load pending, 1 = polling, 2 = ready |
| side | `+0x84` | |
| NoteResultActor* | `+0x88` | its `+0xC0` byte = pacemaker visibility (the byte `pacemaker_swap` forces) |
| ghost id | `+0x90` (i64) | from `FUN_18001dc00(side)`: score-DB entry (`PW+0x178`, keyed `PW+0x54 mcode / GameWork+4 style / PW+0x5C diff`) `+0x10` |
| ghost vector | `+0x98` (`vector<u8>`, grade byte per note) | |

Resolution: `id == 0` → state 2, empty. `id > 0` → state 0 (network load
via `ark::network::GhostDataLoadRequest`; on success `FUN_18001e140` decodes
the server string into `+0x98`, ready byte = 1). **`id < 0` → local slot**:
`side = (-1-id)/5, stage = (-1-id)%5`, `FUN_180056f80(actor+0x98,
PW[side]+0x590+stage*0x2B8+0xB8)` (vector<u8> copy-assign), state 2, ready
byte 1. Course mode (`GameWork+0x70 && GameWork+0xC ≥ 1`) copies the course
record's `+0x2A8` vector instead.

Under the freeze the local slot is always `record[frozen]`, which the
virginise fix + the game's re-prepare have just emptied ⇒ the copy yields an
EMPTY vector ⇒ pacemaker target 0 on any replay of a chart PB'd this credit.
A3's "ghost is 0" bug, 1:1.

### Fix (shipped, `mods/premium_free/ghost_cache.rs`)

Post-original detour on `result_commit`: snapshot `rec+0xB8..0xC0` keyed by
`(side, rec+0x00, rec+0x08, rec+0x04)`, keep-if-better on `rec+0x10`
(course mode skipped). Post-original detour on `ghost_actor_init`: when the
freeze is on, `actor+0x98` is empty (or id < 0) and no network load is in
flight (`state==0 && id>0`), look the chart up via the freshly prepared
`record[frozen]` header, copy the cached bytes in via the game's own
`ghost_vec_copy` (derived from the CALL at `ghost_local_slot_copy_site+25`),
set state 2 / timer 0 / ready byte — exactly the local-slot branch. Cache is
session-scoped (cleared at EAM_EXIT / attract). Fail-open.

Signatures (unique on 20260616/0721/0825; absent on 20250805, which already
fails `stage_records` closed): `ghost_actor_init`, `ghost_local_slot_copy_site`,
`result_commit`. Derived: `ghost_vec_copy`.

### Diagnostics (`mods/premium_free/diag.rs`, freeze-gated — REMOVED after the run below; recipe kept for re-use)

Per scene entry (25 post-virginise, 26, 27, 28, 30): frozen index, per
entered side `rec[frozen]` mcode/diff/style/score, `PW+0x54/+0x5C`, note /
grade / ms stream lengths, gauge-map size. WARN `BUG-2 SIGNATURE` when the
record header ≠ PlayerWork display fields at GAMEPLAY entry; WARN
`BUG-1 SIGNATURE` when streams are non-empty at GAMEPLAY entry (record not
re-prepared) or when the result commit's early-outs fire. Plus a post-commit
line with what landed in the record.

### 2P toggle plumbing (fixed 2026-09-01)

Cabinet test of the diag build (1P) reproduced neither report but exposed a
real 2P defect in the option plumbing: profile loads d
### Cabinet run 2026-09-01 (1P, 20260825 build) — result

Three plays of one chart (diff 3 → 2 → 3) under the freeze:

- Every DECIDE / STAGE_INDICATOR / GAMEPLAY line showed `rec[0]` and
  `PW+0x54/+0x5C` in agreement, streams empty at GAMEPLAY entry, and
  RESULTS_DETAIL reading the just-played record. **Neither field report
  reproduced in 1P.**
- Ghost fix confirmed: 3rd play logged
  `ghost injected P1 mcode=38909 diff=3 id=-1 state=2 had=0 bytes=442` —
  the game resolved ghost id **−1** (local slot side 0 / stage 0 = the
  frozen record), copied an EMPTY vector, and the cache re-supplied the
  442-byte stream from play 1. Pacemaker target followed the first attempt.
- `BUG-1 SIGNATURE` fired during the ATTRACT DEMO (`actor+0x280 = 1`, both
  sides) — that byte is the demo's "never commit" flag, not a bug. The tap is
  now gated to `current_scene() == GAMEPLAY`.
- The song-select commit writes BOTH sides' `record[0]` regardless of who is
  entered (`reset frozen stage-1 record for P2 (was mcode 38909)` in a 1P
  session).

### 2P / versus: option plumbing defect (fixed the same day)

The tester's session was 2P. `custom_options::resolve_from_load` fires
`on_change` on every accepted profile load, and the former
`premium_free_on_change` did an unconditional `set_value(other_side, v)`.
With two carded-in profiles whose saved values differ the freeze therefore
followed whichever load landed LAST, and that load also overwrote the other
player's cached preference (persisted at logout). Whether that is the
tester's root cause is unproven (the reports did not reproduce in 1P), but it
is a real defect in the shape "can't tell whether pfree was on for both".

Fix: per-side `DESIRED` atomics; the APPLIED freeze = `effective_freeze()`
over the ENTERED sides (P1 governs when both in; `p1 || p2` when entered
state is unknown), re-resolved at every option change and every scene
change; the row is registered with `services/versus_mirror` (P1 seeds at
the first both-entered song-select frame, live edits propagate, disengages
when either side leaves). A load on one side no longer touches the other.

### 2P RE pass (2026-09-01, 20260721) — pre-diagnostic-run facts

- **READY panel** (`ShutterActor` kind 3 populate `FUN_1800359f0`, layout
  `shutter_play`): per side `rec = PW + 0x590 + (GameWork+0xC)*0x2B8`;
  **difficulty label = `rec+0x04`** (`cosh_dif_%s`, `cosh_dif_%s_level_%02d`
  via the music-info vfunc `+0x78(style, diff)`), panel shown iff `PW+0x04`
  (entered) && !course; target score = score-DB lookup
  `FUN_1801e1e00(PW+0x178, GameWork+0x18, GameWork+4, rec+0x04)`; stage
  caption `cosh_call_{1st,2nd,3rd,4th,final,extra}` from `GameWork+0xC`.
  ⇒ Report 2 is "record difficulty stale at scene 27" — the display face of
  the original stale-record bug; `PW+0x5C` is NOT what the panel reads.
- **Confirm paths.** Solo `FUN_18010d9e0` (once-gated `seq+0x1E0`): plays
  `select_difficulty`, `FUN_1800fdc80` (writes `PW+0x54` mcode + side-panel
  cursors for entered sides), then the commit `FUN_1800fdfa0`. Versus
  `FUN_180114ef0` (once-gated `seq+0x2E1`): `FUN_1800fcf40(seq,
  cursor[side], side)` for both sides (writes `PW+0x5C` unconditionally),
  then the same commit (skipped in event modes). The commit loops both sides
  with the per-side `new_mcode != rec->mcode` guard; `FUN_1800fced0()` is the
  stage source (`GameWork+0xC`, or `DAT_1806f396c` in event modes at
  counter 0). No double-commit path found; no 2P-only stale path found
  statically — hence the diagnostic run.

### 2P cabinet run 2026-09-01 — result

Both carded in (P1 profile pfree ON, P2 OFF), song A diff 3 → song A diff 2.
The mirror seeded P2 ← P1 at the first both-entered song-select frame
(`side=1 desired ON`); every scene line showed both sides' `rec[0]` and
`PW+0x54/+0x5C` agreeing, `mcode=-1` on BOTH sides after each virginise,
streams empty at GAMEPLAY entry, both result commits landing, results reading
the fresh record. **Neither report reproduced in 2P either.** The diag was
trimmed to WARN-only signatures and left in place so a recurring field report
carries the evidence (WARN lines reach spice2x's log.txt — the level is only
a tag in the OutputDebugString text). Diag note: the result commit runs after
the scene id has advanced past GAMEPLAY, so its attract gate is
`>= SONG_SELECT`, not `== GAMEPLAY`.
