# Assist Tick — R3 (note taxonomy) + R6 (active-side enumeration)

**Date:** 2026-07-25
**Program:** `gamemdx_20260721.dll` (Ghidra). All addresses are **file-relative to `0x180000000`**
and taken from that build unless stated otherwise.
**Scope:** read-only. No source file modified.

### Confidence legend

- **[OBS]** — read directly off the disassembly/decompilation of the cited address, or off the
  cited repo line. Quoted, not paraphrased, wherever it is load-bearing.
- **[INF]** — inferred from code shape or from combining two [OBS] facts; needs re-verification
  (a one-shot log line is proposed where that is the cheapest route).

---

## Overview

The whole taxonomy question turned out to be answerable from **one function**: the Results-vector
builder `FUN_180060D40`, which runs once per song per side and creates exactly one 0x40-byte
Result record for every note record with `kind >= 0`. It also *pre-judges* the records it does
not want the judge to touch, which is what makes the note classes distinguishable from the
outside.

The chain, end to end **[OBS]**:

```
DancePlaySequence broadcasts msg 0x1044 ("game play")
  → GamePlayActor::onReceiveMessage        0x18005E200  case 0x1044
      → XCnbrep700017d(... "game play : %lu")
      → FUN_18005BAC0(actor, param)        0x18005BAC0   ← "start play"
          FUN_180060990(actor+0xB0)                       clear Results vector
          FUN_1800608D0(actor+0xB0, *(actor+0x194) + *(actor+0x198) + *(actor+0x19C))
                                                          reserve = steps+freezes+shocks
          FUN_180060D40(&out, *(actor+0x90), *(actor+0x98), &{actor, 0})
                                                          ← BUILD the Results vector
          ... chart-length scan, per-second array at actor+0x110 ...
          *(actor+0x58 + stepIdx*8) = 4                   ← enter STEP_PLAY
GamePlayActor::onUpdate                    0x18005CCE0  case step == 4
      FUN_18005EB00(actor, musicCount)     0x18005EB00   broadcast 0x1045 to siblings
      footPanel->update(actor+0xB0, *(actor+0x168), musicCount)
      FUN_18005EA50(actor)
      FUN_18005EC70(actor, musicCount)     ← judgeNotes  (the judge_hook target, +0x5EC70 ✓)
```

Two consequences that the design depends on, both **[OBS]**:

1. **The Results vector is complete before the first judge tick.** It is built inside the same
   call (`FUN_18005BAC0`) that flips the actor to `STEP_PLAY = 4`, and `judgeNotes` is only
   reached from the `step == 4` branch of `onUpdate` (`0x18005CCE0`). There is no incremental
   population and no window.
2. **The actor's note vector is per-side** (`actor+0x90/+0x98/+0xA0`, stride `0x60`). The player
   modifier pass mutates it per that player's `Option` object (see §"Note-record kinds",
   rows `0x01`), so two sides in one session own two independent note vectors — following one
   actor gives exactly one side's chart, by construction.

---

## Note-record kinds

`kind` is the signed byte at note`+0x00`. The parser (`FUN_1801CC000`) initialises every
candidate record to `kind = 0xFD` with `beat_count = INT_MIN`, `music_count = INT_MIN`, and only
emits it if a kind was assigned **[OBS]** (`local_f8 = CONCAT31(..., 0xfd)`, sentinel
`cVar18 = -3` suppresses the push).

Every value the engine's own code compares/assigns:

| `kind` | Name | Written by (addr) | Compared at (addr) | Meaning |
|---|---|---|---|---|
| `0x00` | ARROW / step row | parser `0x1801CC000` (`local_f8 = ...<<8` on a non-zero step byte) | builder switch `0x180060DF4`; builder else-body `0x180060E03`; Analyze trim + stats `0x1801C8680`; modifier entry tests `0x1801CAE40`, `0x1801CAEE0`, `0x1801CAFC0`; collector `0x180024780` (`*pcVar3 == '\0'`); parser tail loop | tap / jump / **freeze head** / **shock arrow** — all four are this one kind |
| `0x01` | THINOUT | CUT mod `0x1801CAEE0`, JUMP-OFF mod `0x1801CAFC0` | builder `0x180060DF4` → body `0x180060F1C`; tail-link pass `0x1801C916B` (`CMP byte ptr [RCX + -0x60],0x1`) | **note suppressed by a player modifier** — see below |
| `0x02` | FREEZE_TAIL | parser `0x1801CC000`, final loop (`local_f8 = CONCAT31(..., 2)`) | builder `0x180060DFF` → body `0x180060E6D`; tail-link pass `0x1801C910C` (`CMP AL,0x2`) | synthesized freeze-end record |
| `0x14` (20) | MINE | **our mod** — `game_note.rs:89`, injected in `mines.rs:200` | `mines.rs:540`, `mines.rs:573` | mod-injected; falls through the builder's `else` arm |
| `0x80` (−128) | tempo marker | parser `0x1801CC000` (`local_f8 = CONCAT31(..., 0x80)`) | `0x1801C9440` (`*pcVar10 != -0x80`) | BPM change; carries a `double` BPM at `+0x10` |
| `0xFB` (−5) | event code-2 arg 1 | parser `0x1801CC000` | swept up by the `kind < 0` tests `0x180060D9A`, `0x1801C90BD`, `0x1801C9106` | song start |
| `0xFA` (−6) | event code-2 arg 2 | parser | same | chart start |
| `0xF9` (−7) | event code-2 arg 3 | parser | same | (unknown) |
| `0xFE` (−2) | event code-2 arg 4 | parser | same | song end |
| `0xF8` (−8) | event code-2 arg 5 | parser | same | (unknown) |
| `0xFD` (−3) | placeholder | parser init | never emitted | skipped before push |

That matches `docs/ssq_format.md §10` exactly, and adds `0x01`, which that table omits.
**There are no other values.** **[OBS]** — the parser is the only producer of `kind` besides the
two modifier lambdas and our own mine injection, and it only ever stores the values above.

### What is `kind == 1` (THINOUT)?

**It is a note that a *player modifier* removed. It is never stepped on, never rendered, never
judged. It must NOT tick.** **[OBS]**

Two writers, both in the post-parse modifier pass `FUN_1801C8EA0` (`0x1801C8EA0`), which builds a
list of `void(step::Note&)` functors from the player's `Option` object and applies each to every
note with `kind >= 0`:

```c
// FUN_1801CAEE0 — installed when Option->vtable[0x2C8] != 0   ("CUT")
if (*param_2 == '\0') {                                   // kind == 0
  iVar2 = 1;
  iVar1 = (**(code **)(**(longlong **)(param_1 + 8) + 0x2c8))();
  if (iVar1 == 1)      iVar2 = 0x400;                     // 1024 ticks = 1/4 note
  else if (iVar1 == 2) iVar2 = 0x200;                     //  512 ticks = 1/8 note
  if (*(int *)(param_2 + 4) % iVar2 != 0) {               // beat_count off the grid
    *param_2      = '\x01';                               // kind = THINOUT
    param_2[0x18] = '\x01';
  }
}
```

```c
// FUN_1801CAFC0 — installed when Option->vtable[0x308] == 0   ("JUMP OFF")
if (*param_2 == '\0') {                                   // kind == 0
  piVar1 = (int *)(param_2 + 0x1c); iVar2 = 0;            // state[0..7]
  do { if (*piVar1 == 1) iVar2++; piVar1++; } while (piVar1 != (int *)(param_2 + 0x3c));
  if (1 < iVar2) {                                        // 2+ panels at once
    *param_2      = '\x01';
    param_2[0x18] = '\x01';
  }
}
```

Measure length is 4096 ticks (`docs/ssq_format.md §1`), so `0x400`/`0x200` are the 1/4 and 1/8
grids — i.e. DDR's `CUT: ON1 / ON2`. **[OBS]** JUMP-OFF thins any row with ≥2 simultaneous
panels (which incidentally also removes shock arrows, since those set 4).

Proof that a THINOUT note is not playable **[OBS]** — the builder pre-judges it, and the judge's
own gate then skips it:

```c
// FUN_180060D40, kind == 1 branch @ 0x180060F1C
uStack_7c = 5;  if (shock_shaped) uStack_7c = 7;   // grade = MISS (5) or NG (7)
iStack_80 = piVar13[-0xd];                         // judgeTimestamp = note.music_count
```
```c
// FUN_18005EC70 (judgeNotes) — the only gate that lets a Result be judged
if (((int)plVar18[1] < 0) && (*(int *)((longlong)plVar18 + 0xc) == 0xff)) { ...judge... }
//      result+0x08 < 0            &&        result+0x0C == 0xFF
```

Also note the mod-agnostic corollary: **a THINOUT note's `music_count` is still valid and still
inside the Results vector**, so a naive "tick every Result" implementation would clap on notes
the player was told were removed. Filtering on `kind == 0` is not optional.

---

## Freeze representation

**A freeze head is `kind == 0` with at least one `length[panel] > 0`. There is no separate head
kind.** **[OBS]**, corroborated at four independent sites:

1. **Parser tail emission** (`FUN_1801CC000`, final loop) — the definition, effectively:
   ```c
   piVar16 = max over note+0x3C .. note+0x5C            // max(length[0..7])
   if ((*(char *)(note) == '\0') && (0 < *piVar16)) {   // kind == 0 && max length > 0
     iStack_f4 = *piVar16 + note->beat_count;           // tail beat = head beat + length
     local_f8  = CONCAT31(..., 2);                      // tail kind = 2
     auStack_dc[0..7] = note->state[0..7];              // tail copies the head's state[]
     ...push...                                          // length[] left zero
   }
   ```
   So the tail's identity relative to its head is **an identical `state[]` array**, and the tail
   carries no `length[]`.
2. **Analyze's own freeze counter** (`FUN_1801C8680`):
   ```c
   if ((char)piVar7[-8] == '\0') {                      // kind == 0
     ... shock/normal counting ...
     piVar14 = piVar7 + 7;  piVar15 = piVar7 + 0xf;     // note+0x3C .. note+0x5C
     do { if (0 < *piVar14) break; piVar14++; } while (piVar14 != piVar15);
     if (piVar14 != piVar15) local_res20[1]++;          // stats[1] = freeze rows
   }
   ```
   Note that a freeze head is counted in **both** `stats[0]` (or `stats[2]` if shock-shaped)
   **and** `stats[1]` — the freeze flag is orthogonal to the row class.
3. **Results builder** (`FUN_180060D40` @ `0x180060DE0`): `if (0 < length[i]) → result+0x34/+0x38 = 0`
   (else they stay `-1`). Instruction-exact: `CMP dword ptr [RCX],R15D / JG` → `MOV qword ptr [RBP + 0xb],R15`.
4. **Tail-link pass** (`FUN_1801C8EA0` @ `0x1801C9166`): walks backwards from each `kind == 2`
   note for the note with an identical `state[]`, then
   ```asm
   1801C916B  CMP byte ptr [RCX + -0x60],0x1   ; head.kind == THINOUT?
   1801C9190  ...max over head+0x3C..head+0x5C...
   1801C91A3  CMP dword ptr [R9],0x0 / JG      ; max length > 0 → tail is live
   1801C91A9  MOV byte ptr [RDX + 0x18],0x1    ; else mark the tail "orphaned"
   ```
   i.e. note`+0x18` on a tail means "the freeze this tail belongs to is gone".

**"FREEZE ARROW: OFF" is implemented by zeroing `length[]`, not by removing notes** **[OBS]** —
`FUN_1801CAE40`:
```c
if (kind == 0 handled implicitly by the caller's kind>=0 filter)
*(undefined8 *)(param_2 + 0x3c) = 0;  *(undefined8 *)(param_2 + 0x44) = 0;
*(undefined8 *)(param_2 + 0x4c) = 0;  *(undefined8 *)(param_2 + 0x54) = 0;
```
So with freezes off, a former head becomes an ordinary tap (and **should still tick** — it is
still a note you step on), and its tail gets note`+0x18 = 1`. The predicate in §"The tick
predicate" gets this right for free by never consulting `length[]`.

**Freeze tails must be excluded, and `kind == 2` is a sufficient discriminator.** The builder
marks every tail as already judged, unconditionally **[OBS]** (`FUN_180060D40` @ `0x180060E6D`):
```c
iStack_80 = piVar13[-0xd];   // judgeTimestamp = tail.music_count
uStack_7c = 7;               // grade = NG
```

---

## Shock representation

**The engine's test is: `state[0..3]` all `== 1`, OR `state[4..7]` all `== 1`.** The "all four
per-side panels TRG" discriminator from `.agents/learnings/learnings.md:135` is **confirmed
verbatim**, and it exists in the binary as a standalone helper **[OBS]**:

```c
// 0x180024530  —  step::Note::isShock(note)
undefined1 FUN_180024530(longlong param_1)
{
  if (((((*(int *)(param_1 + 0x1c) != 1) || (*(int *)(param_1 + 0x20) != 1)) ||
       (*(int *)(param_1 + 0x24) != 1)) || (*(int *)(param_1 + 0x28) != 1)) &&
     (((*(int *)(param_1 + 0x2c) != 1 || (*(int *)(param_1 + 0x30) != 1)) ||
      ((*(int *)(param_1 + 0x34) != 1 || (*(int *)(param_1 + 0x38) != 1)))))) {
    return 0;
  }
  return 1;
}
```

`+0x1C/+0x20/+0x24/+0x28` = `state[0..3]`, `+0x2C/+0x30/+0x34/+0x38` = `state[4..7]`.

The same predicate is inlined at five more sites, which is why it can be trusted:

| Site | Address | Use |
|---|---|---|
| `judgeNotes` | `0x18005EC70` (3 occurrences in the body) | selects the shock hit-window (`note.mc − 0x22 .. note.mc + 0x54`), the NG judge code `0x1031`, the `"shock ng : pressedDir=%d, musicCount=%d, note.musicCount=%d, diff=%d"` log, and grade `6` (OK) vs `5` (MISS) on expiry (`*(uint *)(plVar18+0xc) = bVar3 + 5`) |
| Analyze stats classifier | `0x1801C8680` | `local_res20[2]++` (shock count) vs `*local_res20 += 1` (normal count) |
| Results builder, `kind == 0` arm | `0x180060E11 .. 0x180060E3F` | pre-judge grade `6` vs `0` for `music_count < 0` notes |
| Results builder, `kind == 1` arm | `0x180060F1C .. 0x180060F4A` | pre-judge grade `7` vs `5` |
| note collector | `0x180024780` (inlined) + a call to `0x180024530` in the per-panel loop | picks the shock pass vs the normal pass (`if ((cVar9 == param_12) && (*pcVar3 == '\0'))`) |

**Both sides and doubles.** The test is an OR over the two 4-panel groups and is *never* gated on
the actor's own `+0x84` side — so it is correct for a 1P-side actor, a 2P-side actor, and a
doubles actor without modification **[OBS]**. Which group a shock lands in is decided at parse
time (`FUN_1801CC000`):
```c
uVar11 = 0;
if ((int)uVar14 < param_6)                      // param_6 = panel count: 4 single / 8 double
  uVar11 = 1 << ((char)uVar14 + (char)uVar23 * '\x04' & 0x1fU);
*puVar20 = (uint)((uVar11 & bVar1) != 0);       // state[i] = (mask & stepByte) != 0
```
so in single mode only `state[0..3]` can ever be set (low-nibble branch fires for step byte
`0xFF`), and in doubles `0x0F → state[0..3]`, `0xF0 → state[4..7]`, `0xFF → both`. That maps 1:1
onto `docs/ssq_format.md §5.3`. **[OBS]**

Two caveats worth writing down:

- **The engine cannot distinguish a 4-panel "quad" jump from a shock arrow, and neither can we.**
  Matching the engine is therefore the correct behaviour, not a compromise. **[OBS]**
- `uVar23` above is a side index applied only when `1 < param_7` (a player-count-ish argument),
  which folds a high-nibble chart into `state[0..3]`. That path is presumably battle/versus mode
  (`docs/bpl_battle_mode_research.md` exists but was not read for this task). It cannot break the
  predicate — it only ever moves bits *into* the low group. **[INF]**

### `state[]` values

Parse time only ever writes `0` or `1` **[OBS]** (`*puVar20 = (uint)((uVar11 & bVar1) != 0)`), and
the turn/mirror modifier (`FUN_1801C9270`, via `0x1801CAD20` / `0x1801CADB0`) only *permutes*
`state[]`/`length[]` through a table at `DAT_18035A848` — it introduces no new values.
The `REC=2 / GEN=3 / REP=4` names in `game_note.rs:68-74` therefore have no producer that I could
locate in this pass **[INF]**. What *is* certain:

- `judgeNotes` treats a panel as steppable when `state[i] == 1 || state[i] == 4` **[OBS]**
  (`if ((cVar6 == '\0') || ((*pcVar20 != '\x01' && (*pcVar20 != '\x04'))))` and the mirror test
  in the same loop).
- The note collector draws a quad for any `state[i] != 0` **[OBS]** (the outer test in its
  per-panel loop reduces to `note+0x1C+4i != 0`).
- Analyze's trim pass guarantees **every surviving `kind == 0` note has at least one
  `state[i] == 1`** **[OBS]** (`FUN_1801C8680`: the first `kind == 0` note with no `state[i] == 1`
  starts a `remove_if` + `erase` over the rest of the vector).

⇒ For an *existence* test ("is there an arrow at this timestamp") use `state[i] != 0`, which is a
superset of both engine tests and cannot miss a note. For the *shock* test use `== 1` exactly, as
the engine does.

---

## Results-vector coverage

`actor+0xB0 / +0xB8 / +0xC0` = MSVC `begin / end / end_capacity` of `vector<Result>`, stride
`0x40` **[OBS]** (`ADD qword ptr [RBX + 0xb8],0x40` at `0x180061049`; `game_note.rs:100,141-143`).

**The vector covers the whole chart for that side, built once, and it includes shock arrows,
freeze tails, THINOUT notes and our injected mines.** The single filter is `kind >= 0`:

```asm
180060D90  MOVZX EDX,byte ptr [RDI + -0x3c]   ; note->kind        (RDI = note+0x3C)
180060D94  LEA   RBX,[RDI + -0x3c]            ; note*
180060D98  TEST  DL,DL
180060D9A  JS    0x180061057                  ; kind < 0  → skip note entirely
...
180060DA8  MOV   dword ptr [RBP + -0x21],0xffffffff   ; result+0x08 judgeTimestamp = -1
180060DAF  MOV   dword ptr [RBP + -0x1d],0xff         ; result+0x0C grade         = 0xFF
180060DCB  MOV   qword ptr [RBP + 0xb],-0x1           ; result+0x34/+0x38         = -1
180060DE0  CMP   dword ptr [RCX],R15D / JG            ; any length[i] > 0 ...
180060DF0  MOV   qword ptr [RBP + 0xb],R15            ;   ... → +0x34/+0x38 = 0
180060DF4  MOVSX ECX,DL / DEC ECX / JZ 0x180060F1C    ; kind == 1 → THINOUT arm
180060DFF  DEC ECX / JZ 0x180060E6D                   ; kind == 2 → TAIL arm
180060E03  MOV ECX,dword ptr [RDI + -0x34]            ; else: note->music_count
180060E06  CMP ECX,dword ptr [R12 + 0x8]              ; vs functor limit (== 0, see below)
180060E0B  JGE 0x180060f6a                            ; mc >= 0 → leave UNJUDGED
180060E11..0E3F  isShock(state[]) inlined             ; mc < 0 → grade 6 (OK) / 0 (MARVELOUS)
180061049  ADD qword ptr [RBX + 0xb8],0x40            ; push
```

The functor's second qword is `0` **[OBS]** — `FUN_18005BAC0` builds it as
`local_68 = actor; uStack_60 = (ulonglong)uStack_60._4_4_ << 0x20;` so `(int)param_4[1] == 0`.
So the "limit" comparisons above are literally `music_count < 0`: **notes at negative time (an
artefact of tempo interpolation before the first tempo marker) are auto-credited at build time
and can never be judged.** They must not tick either.

Resulting classification of every Result entry, by construction:

| Note | in vector? | `result+0x08` (ts) | `result+0x0C` (grade) | judged live? |
|---|---|---|---|---|
| tap / jump / freeze head, `mc >= 0` | yes | `-1` | `0xFF` | **yes** |
| shock arrow, `mc >= 0` | yes | `-1` | `0xFF` | **yes** |
| any `kind == 0`, `mc < 0` | yes | `mc` | `6` shock / `0` else | no (pre-credited) |
| `kind == 1` THINOUT | yes | `mc` | `7` shock-shaped / `5` else | no |
| `kind == 2` freeze tail | yes | `mc` | `7` | no |
| `kind == 20` MINE (ours) | yes | `-1` | `0xFF` | yes → why `mines.rs` pre-marks them |
| tempo `0x80`, events `0xF8..0xFE` | **no** | — | — | — |

Corroborations that this is the whole chart, not a window **[OBS]**:

- `judgeNotes` restarts from `*(actor+0xB0)` every frame and *breaks* on the first entry more than
  260 ms in the future — `if (iVar15 < *(int *)(lVar21 + 8) + -0x104) break;` — which only makes
  sense over a fully populated, time-ordered vector.
- The reserve is `*(actor+0x194) + *(actor+0x198) + *(actor+0x19C)` = Analyze's
  `stats[0] + stats[2] + stats[1]` = (all `kind == 0` rows) + (one tail per freeze row) — i.e.
  exactly the final entry count for a vanilla chart. Our mine injection overflows it and takes the
  engine's own grow path `FUN_180060C50`.
- Ordering: Analyze sorts the note vector by `(beat_count, music_count)` (`FUN_1801CA120`, called
  from `0x1801C8680`) *before* the modifier pass and *before* `music_count` is computed, and the
  builder walks it in order. `game_note.rs:367-381` already documents the comparator.

### `music_count` units — independently re-confirmed

`FUN_1801C9440` computes `note+0x08` from `note+0x04` by linear interpolation between adjacent
`kind == 0x80` tempo markers, then:

```c
if (param_2 != 1000) {                              // param_2 = the file's TPS
  piVar11 = (int *)*param_1 + 2;                    // &note->music_count
  do {
    fVar13 = (float)FUN_18028a398(((float)*piVar11 / (float)param_2) * fVar12 + fVar7);
    *piVar11 = (int)fVar13;                         // rescale to ms  (fVar12 = 1000.0f)
    piVar11 = piVar11 + 0x18;
  } while (...);
}
```

**[OBS]** — the engine normalises `music_count` to **integer milliseconds**, TPS-invariant. This
is the same normalisation `existing-mechanisms.md §C3` warns `note_types_expansion::timing.rs`
does *not* do. Reading `note+0x08` (as the design does) is correct on both TPS generations; a
tempo-chunk-derived timestamp would be wrong by ~6.67× on the 760 TPS-150 files.

### Doubles: one actor, all 8 panels, no duplication

**[OBS]** In doubles there is exactly one `GamePlayActor`, its `*(i32*)(actor+0x88) == 1`, and the
engine widens every per-panel loop from 4 to 8 against that same field:

```c
// judgeNotes 0x18005EC70, FUN_18005EA50, onUpdate 0x18005CCE0 — three independent reads
iVar8 = 4;  if (*(int *)(param_1 + 0x88) == 1) iVar8 = 8;
```

A doubles note record therefore spans `state[0..7]`, and a cross-pad jump is **one** record — no
duplication beyond what the timestamp dedup already handles. The only "duplication" risk in any
mode is two *distinct* records landing on the same `music_count`:

- a freeze tail coinciding with a tap (excluded by `kind != 2`),
- an injected mine coinciding with a tap (excluded by `kind == 0`),
- two adjacent step rows whose distinct tick values round to the same millisecond — genuinely
  possible on TPS-150 charts after the rescale above. **[INF]**

⇒ dedup by timestamp is required, and near-coincident timestamps (a few ms apart) should be
coalesced, else two claps flam. See open questions.

---

## The tick predicate

Given a `*const GameNote` (already null-checked and stride-validated by
`game_note.rs::for_each_result`):

```rust
/// Tick iff this note record is a vanilla step row the player is expected to
/// hit: normal taps, jumps (one record ⇒ one tick), and freeze heads.
/// Excluded: freeze tails, shock arrows, modifier-suppressed notes, mod-injected
/// note types (mines), tempo/event markers, and pre-chart notes.
unsafe fn should_tick(note: *const GameNote) -> bool {
    let n = &*note;

    // 1. Vanilla step rows only. This single test excludes freeze TAILS (kind 2),
    //    modifier-suppressed notes (kind 1), tempo/event markers (kind < 0) and
    //    every mod-injected kind, present and future (MINE = 20, and whatever
    //    NoteTypeRegistry adds next). Whitelist, not blacklist.
    if n.kind != 0 {
        return false;
    }

    // 2. Shock arrows: the engine's own discriminator, verbatim (0x180024530).
    //    Correct for a 1P-side actor, a 2P-side actor and doubles, because it is
    //    an OR over the two 4-panel groups and never consults the actor's side.
    let low_all_trg = n.state[0] == 1 && n.state[1] == 1 && n.state[2] == 1 && n.state[3] == 1;
    let high_all_trg = n.state[4] == 1 && n.state[5] == 1 && n.state[6] == 1 && n.state[7] == 1;
    if low_all_trg || high_all_trg {
        return false;
    }

    // 3. There must be an arrow on some panel. Analyze's trim pass already
    //    guarantees this for kind == 0, so it is a cheap invariant guard, not a
    //    filter. `!= 0` (not `== 1`) matches what the renderer draws.
    if !n.state.iter().any(|&s| s != 0) {
        return false;
    }

    // 4. Notes before the chart start are auto-credited by the engine at Results
    //    build time and can never be played (FUN_180060D40 @ 0x180060E06).
    if n.music_count < 0 {
        return false;
    }

    true
}
```

**`length[]` is deliberately not consulted.** That is the finding, not an omission: a freeze head
is a tap you step on, so it ticks like any other `kind == 0` row, and the tail is already excluded
by kind. Reading `length[]` would also make the predicate wrong under `FREEZE ARROW: OFF`, which
zeroes `length[]` while leaving the (still steppable) head in place.

Consume it once, on the first judge tick of the song:

```rust
let (begin, end) = actor_results_range(actor);          // actor+0xB0 / +0xB8
let mut ticks: Vec<i32> = Vec::new();
for_each_result(begin, end, |_entry, note| {
    if should_tick(note) { ticks.push((*note).music_count); }
});
ticks.sort_unstable();
ticks.dedup();                                          // jumps are already one record;
                                                        // this catches tail/mine/rounding overlap
```

Then advance a cursor with the `(prev_music_count, music_count]` crossing test that `mines.rs`
already uses (`mines.rs:280-291`, `partition_point`) — but keyed per side, not globally
(`mines.rs` keeps one global `prev_music_count`, which `existing-mechanisms.md §C2` already flags
as not 2P-safe).

### Failure modes if a claim is wrong

Every branch is a pure read of the 0x60-byte record, so **nothing here can crash** — the worst
case is always an audible artefact:

| Claim | If wrong | Severity |
|---|---|---|
| shock ⇔ 4-per-side TRG | shock arrows tick (a clap on a note you must *avoid*), or a genuine quad-jump is silently skipped | audible, wrong-but-harmless |
| `kind == 2` ⇔ freeze tail | a second clap at each freeze release | audible |
| `kind == 1` ⇔ suppressed | claps on notes CUT/JUMP-OFF removed (a clap with no arrow) | audible, confusing |
| `kind == 0` whitelist | mines/future mod kinds tick | audible |
| `mc < 0` filter | a burst of claps before the chart's first visible arrow | audible |
| stride/offsets (`0x60`, `0x40`, `0xB0/0xB8`) | `for_each_result` bails out (span not a multiple of `0x40`) → **no ticks at all**, no crash | silent no-op |

The one thing that *could* fault is dereferencing a stale Results range after a restart — which is
why the tick list stores plain `i32` timestamps and never note pointers. See quick restart.

---

## Actor enumeration (what exists today)

`src/mods/quick_restart_or_fail.rs:263-295`, quoted in full:

```rust
263: /// Walks the active TS → DPS → children chain and returns every child
264: /// whose vtable matches `gameplay_actor_vtable`. Empty when not in
265: /// gameplay or when the actor tree isn't yet captured.
266: fn find_gameplay_actors() -> Vec<*mut u8> {
267:     let mut out = Vec::new();
268:
269:     let Some(transition_seq) = scene_manager::current_transition_sequence() else {
270:         return out;
271:     };
272:     let target_vtable = GAMEPLAY_ACTOR_VTABLE.load(Ordering::Acquire);
273:     if target_vtable.is_null() {
274:         return out;
275:     }
276:
277:     unsafe {
278:         let dps_slot = transition_seq.add(ACTIVE_CHILD_OFFSET) as *const *mut u8;  // +0x58
279:         let dps = *dps_slot;
280:         if dps.is_null() {
281:             return out;
282:         }
283:
284:         let mut child = *(dps.add(FIRST_CHILD_OFFSET) as *const *mut u8);          // +0x18
285:         while !child.is_null() {
286:             let vtable = *(child as *const *mut u8);
287:             if vtable == target_vtable {
288:                 out.push(child);
289:             }
290:             child = *(child.add(NEXT_SIBLING_OFFSET) as *const *mut u8);           // +0x10
291:         }
292:     }
293:
294:     out
295: }
```

Offsets, named at `quick_restart_or_fail.rs:37,40,43`:

| Offset | Const | On | Meaning |
|---|---|---|---|
| `+0x58` | `ACTIVE_CHILD_OFFSET` | TransitionSequence | active gosub child = the `DancePlaySequence` |
| `+0x18` | `FIRST_CHILD_OFFSET` | any actor/sequence | first child |
| `+0x10` | `NEXT_SIBLING_OFFSET` | any actor/sequence | next sibling |
| `+0x08` | (used at `:368`) | GamePlayActor | **parent** (the DPS) |

**Identification is a raw vtable-pointer compare** (`:286-287`) against
`gameplay_actor_vtable`, resolved by RTTI `.?AVGamePlayActor@dance@sequence@@`
(`signatures.rs:1912-1921`; the RTTI type-descriptor string lives at `0x180482FD0` in this build).
No dynamic_cast, no fixed slot. **[OBS]**

`current_transition_sequence()` is a snapshot of the `this` argument of every
`TransitionSequence::createNextSequence` call (`scene_manager.rs:48-50` and the store at the top
of `scene_hook`), so it is non-null from the first scene transition onward and stable. **[OBS]**

Invocations today: `fail_song` (`:318`) and `is_course` (`:363`) — both reached from an input
gesture mid-song (`on_input_event`, `:231-261`), i.e. late.

The **`+0x18` / `+0x10` chain is the engine's own** — `FUN_18005EB00`, called from `onUpdate`
immediately before `judgeNotes` every frame, broadcasts message `0x1045` over exactly those two
offsets, starting from the actor's parent at `+0x08` **[OBS]**:

```c
plVar6 = *(longlong **)(param_1 + 8);                        // actor->parent (the DPS)
if (((*(byte *)(plVar6 + 4) & 0x20) == 0) &&
   (iVar5 = (**(code **)(*plVar6 + 0x18))(plVar6, 0x1045, &local_18), iVar5 == 0)) {
  lVar2 = plVar6[3];                                         // parent + 0x18 = first child
  while (lVar2 != 0) {
    lVar1 = *(longlong *)(lVar2 + 0x10);                     // + 0x10 = next sibling
    FUN_18022eaa0(lVar2, 0x1045, &local_18, 0);
    lVar2 = lVar1;
  }
}
```

That is a strong result: **the sibling list is provably live and walkable at the first judge tick,
because the engine walks it one call earlier in the very same frame.**

---

## Timing of availability

Is `find_gameplay_actors()` correct on the first `judgeNotes` tick of a song?

1. **The actor exists and is in a child list of `*(actor+0x08)`.** [OBS] — the `0x1045` broadcast
   above, plus `FUN_18005BAC0` being reached through `onReceiveMessage` (`0x18005E200`), which is
   itself a broadcast to the DPS's children. An actor that were not in the list could not have
   received `0x1044` and so could not be in `STEP_PLAY`.
2. **All active sides are already in the list on the first tick.** [INF, high confidence] — the
   list is built when the DPS constructs its children (before `STEP_PLAY`), and `0x1044` is
   broadcast to *all* children, so both sides flip to step 4 in the same dispatch. The sibling
   walk therefore returns the complete set even on the first tick, before the second actor's
   `judgeNotes` has run this frame. This is the single most important reason to prefer the walk
   over "accumulate actors as the dispatcher shows them to us".
3. **The Results vector is complete.** [OBS] — built by `FUN_180060D40` inside `FUN_18005BAC0`,
   the same call that sets step 4. There is no frame in which the actor is judging against a
   partially built vector.
4. **`TS+0x58` is the risk, not the child walk.** For a *sequence*, `+0x58` is also where the
   `agcs::StackStep` array lives (`FUN_180032360` reads `param_1+0x58` indexed by
   `*(u16*)(param_1+0x82)` on the DPS; `quick_restart_or_fail.rs:45-49` uses `+0x58` as the step
   slot on the *actor*). The shipped Quick Restart proves `TS+0x58` really does hold the child
   sequence pointer for the TransitionSequence class **[OBS, empirical]**, but it is a
   class-layout coincidence that the assist tick does not need to rely on — see the recommended
   algorithm, which starts from `*(actor+0x08)` instead.

**Diagnostic to close (2) statically-unprovable gap** — one-shot, first judge tick of a song:

```
AssistTick: first tick — actor=%p parent=%p siblings=%d sides=[%d,%d] style=[%d,%d] results=%d ticks=%d
```
Expect `siblings == 2` with `sides == [0,1]` on a 2P song, `siblings == 1` with `style == 1` on
doubles. Log it once per song behind a config flag.

---

## Side determination

**`*(i32*)(actor+0x84)` is the play side and `*(i32*)(actor+0x88)` is the play style.** Both
verified. **[OBS]**

- `+0x84`: `autoplay.rs:52-56` declares it; the engine indexes two per-side global arrays with it —
  `(&DAT_1806F2ED0)[*(int *)(actor + 0x84)]` (the per-side player-option holder, used in
  `onUpdate` and `onReceiveMessage`) and `(&DAT_1804C5AC8)[side]` in `FUN_18005EB00`. Also copied
  into the judge's scratch struct (`local_108 = *(undefined4 *)(local_148 + 0x84)` in
  `judgeNotes`, mirrored by `mines.rs:49`).
- `+0x88` is an **int play-style enum, `1 == DOUBLE`**, read three independent times to switch
  every per-panel loop between 4 and 8 panels (`judgeNotes`, `FUN_18005EA50`, `onUpdate`
  `if ((int)param_1[0x11] == 1) lVar20 = 8;`). `FUN_180032360` confirms the enum: its style loop
  is `local_230 ∈ {0,1}` and maps `difficulty_code = difficulty + 5` when `style == 1`, matching
  `docs/ssq_format.md §5.1` Single/Double codes.

⚠ **Two corrections to prior notes that the design must not inherit:**

1. **`power_user_statistics/data_feed.rs:27` `ACTOR_SESSION_OFFSET = 0x88` is wrong** (and so is
   the recommendation in `existing-mechanisms.md §C4(b)` to read both sides' difficulties through
   `*(actor+0x88) → +0x118/+0x120`). `actor+0x88` is an `int` compared against `1` by three
   engine functions; it is not a pointer. Dereferencing it would read address `0x1` or `0x0`.
   The constants are dead code today (`#![allow(dead_code)]`), so nothing is broken, but the
   assist tick must not use that chain. **[OBS]**
2. **"In doubles mode `+0x84` is `0`" (`autoplay.rs:54-55`) is unverified and unnecessary.** I
   found no code that forces `+0x84 = 0` for doubles; a right-side-only card-in playing DOUBLE
   would plausibly yield `+0x84 == 1`. The recommended algorithm below never assumes it — it
   reads the style field instead, and uses `+0x84` only as "which side's option value applies".
   **[INF — would need a doubles-from-the-P2-side cabinet test to settle; the algorithm is
   correct either way.]**

### Distinguishing the three cases with only the actor list + those fields

| Actors found | `style == 1` on any? | Conclusion | Tick side |
|---|---|---|---|
| 1 | yes | **DOUBLES** | that actor; option side = its own `+0x84` |
| 1 | no | **SOLO** on side `+0x84` (0 = left/P1, 1 = right/P2 — covers "solo on the P2 side") | that actor |
| 2 | no | **2-PLAYER versus** | prefer side 0 if enabled, else side 1 |
| 2 | yes | impossible by construction | treat as 2P; log a warning |

Note the style field is *required*: without it, "solo on the P2 side" and "doubles" are only
separable if the unverified `doubles ⇒ +0x84 == 0` claim holds.

### Cheaper / alternative signals, compared

| Signal | Cost | Tells you | Failure mode |
|---|---|---|---|
| **Sibling walk from `*(actor+0x08)`** (recommended) | ~2-4 loads, once per song | the exact set of active actors, with side + style per actor | needs `gameplay_actor_vtable` (already a resolved signature, `signatures.rs:1912`); if the vtable is unresolved you get an empty list → must degrade |
| `find_gameplay_actors()` as written (TS → `+0x58` → children) | same + a `scene_manager` atomic load | same | extra dependencies: `scene_manager` availability *and* the `TS+0x58` layout assumption; returns empty outside gameplay |
| Set of actors seen by the judge dispatcher | free | active sides — but only *after* a full frame | no frame delimiter is available in `fn(actor, music_count)`; both sides usually report the *same* `music_count`, so you cannot tell "second actor of this frame" from "first actor of next frame". Also latches on dispatch order, which is not guaranteed to be P1-first (child lists are commonly built by prepend ⇒ reverse order) |
| `player_work_table[side]` presence (`webui_options/mod.rs:394-434`, `mine_render.rs:249-296`) | one pointer chain | "side N is carded in" | answers a *different* question: profile slot occupancy at song select, not gameplay participation; says nothing about doubles vs 2P; a null wrapper means "not carded in" but a non-null wrapper does not prove that side has a `GamePlayActor` this stage |
| Play-mode global `*DAT_1806F14F8` | one load | `[0] == 1` looks like "style == double"; `[2]` is a side-ish int asserted against `((side+1)&1)` in `FUN_18005EB00` | semantics not pinned down (the `[2]` assert reads as "the *other* side", which cannot hold for both actors of a 2P session); would need a new signature + more RE. **[INF]** Not worth it — the actor fields are already authoritative |

---

## Recommended algorithm

Pick the tick side **once per song, on the first judge tick**, then never re-evaluate.

```
// per-song state, cleared on scene_manager GAMEPLAY entry AND exit
latched: Option<{ actor: *mut u8, side: u8, ticks: Vec<i32>, cursor: usize, prev_mc: i32 }>

on_judge_tick(actor, music_count):                 // judge_hook post @ Normal
  if latched.is_none():
     // ---- 1. enumerate, from the actor we were handed (no scene_manager, no TS) ----
     actors = [actor]                                       // always at least this one
     dps = *(actor + 0x08)
     if dps != null && gameplay_actor_vtable resolved:
        walk dps+0x18 / +0x10, keep children whose vtable == gameplay_actor_vtable
        if the walk contains `actor`, use the walked list; else keep [actor] and warn once
        //  ^ the containment check is the cheap validity proof for the whole chain

     // ---- 2. classify ----
     doubles = any a in actors with *(i32*)(a+0x88) == 1
     if doubles:                    candidates = [the single actor]           // 1 actor expected
     else if actors.len() >= 2:     candidates = actors sorted by *(i32*)(a+0x84)   // side 0 first
     else:                          candidates = actors

     // ---- 3. choose: first candidate whose OWN side has assist tick enabled ----
     chosen = candidates.find(|a| enabled_for_side(*(i32*)(a+0x84)))
     if chosen.is_none(): latch a "silent" marker for this song and return

     // ---- 4. build the tick list from the chosen actor, right now ----
     //  Results vector is complete: built in the same call that entered STEP_PLAY.
     ticks = walk chosen's Results (actor+0xB0/+0xB8) with should_tick(), collect music_count,
             sort, dedup, coalesce ticks closer than COALESCE_MS
     latched = { chosen, side, ticks, cursor: 0, prev_mc: i32::MIN }
     log ONE line (see the diagnostic in "Timing of availability")

  // ---- 5. only the latched actor drives the clock ----
  if actor != latched.actor: return
  if music_count <= latched.prev_mc: return          // paused / duplicate / reordered frame
  fire one clap per tick in (prev_mc, music_count]   // partition_point, mines.rs:286-291 shape
  latched.prev_mc = music_count
```

Sort candidates **by the side field, not by list position** — child-list order is not documented
and is plausibly reverse-of-creation.

### Critique of the design doc's fallback

> "lock onto the first observed actor whose side has the option enabled, preferring side 0 if both
> are seen in one frame"

- **"if both are seen in one frame" is not implementable from the judge callback.** The callback
  gets `(actor, music_count)` and nothing else; `music_count` is computed per-actor
  (`iVar6 = option_offset + (globalAudioTime − actor->0x16C − actor->0x160)` in `onUpdate`), so it
  is *usually* identical for both sides but is not a frame delimiter. You would have to introduce
  one (the actor's own frame counter at `+0x188`, or `*(u32*)(DAT_1806F2CF0 + 0x1268)`), i.e. more
  RE surface for a case the sibling walk removes entirely.
- **Without a frame delimiter it silently violates the accepted rule.** In a both-enabled 2P
  session where P2's actor happens to be dispatched first, "first observed enabled actor" latches
  P2, but the accepted rule says P1 wins.
- **It is still the right *degraded* mode.** Keep it for the case where the sibling walk yields
  nothing (null parent, or `gameplay_actor_vtable` unresolved): lock onto the dispatched actor iff
  its own side is enabled. Then the only misbehaviour is "in 2P we may follow P2's chart", which is
  audible-but-benign and one WARN line explains it in the log.
- One refinement: in the degraded mode, do **not** latch on the first frame if the dispatched
  actor's side is *disabled* — leave the latch open so the other side's actor can claim it later in
  the same or next frame. That converts "P1 disabled, P2 enabled" from silence into correct
  behaviour without needing frame boundaries.

---

## Quick-restart handling

What Quick Restart does (`quick_restart_or_fail.rs:308-357`): sets `m_isDead`, forces
`+0x58 = STEP_GAME_OVER (5)` on **every** active actor, and installs a one-shot
`STAGE_RESULT → GAMEPLAY` redirect (`scene_manager::add_redirect_once`). The framework then tears
the gameplay scene down and builds a **fresh** one.

Therefore, after a quick restart **[OBS]/[INF]**:

- New `DancePlaySequence`, new `GamePlayActor`s, new note vectors, **new Results vector** — the old
  `actor+0xB0` buffer is freed. Any cached note/Result pointer dangles.
- The tick *side* choice is usually still valid (same song, same difficulty, same sides), but must
  be re-derived anyway because the actor pointer it was latched to is gone.
- The tick *list* must be rebuilt. Our list holds only `i32` timestamps, so a stale list is a
  timing bug, never a memory-safety bug — keep that property.

Detection, in priority order:

1. **`scene_manager::on_scene_change` with `next == scene::GAMEPLAY` → clear the latch.**
   Authoritative and already the in-tree idiom for exactly this case —
   `power_user_statistics/mod.rs:103-105`: *"Entering gameplay — either fresh start or quick
   restart. Reset buffers."* Fires for both fresh entry and restart, because the redirect rewrites
   the reported scene id before callbacks run (`scene_manager.rs:77-96`). Also clear on
   `prev == GAMEPLAY` exit.
2. **`actor != latched.actor` in the judge callback → re-latch.** Defence in depth for any path
   that produces new actors without a GAMEPLAY scene callback. Caveat: the allocator can hand back
   the *same* address for the new actor, so this can false-negative — which is why (1) is primary.
3. **`music_count` went backwards → rewind the cursor** (and, if you also see (2), re-latch).

**Is "`music_count` went backwards" sufficient on its own? No.** **[INF]**

- It is *sufficient in the common case*: the gesture is a mid-song triple-press, and the fresh
  chart restarts near (or below) zero, so the first tick of the new song is far below the last tick
  of the aborted one.
- It is *not sufficient in general*: restart during the first few hundred milliseconds can produce
  a new first tick that is **≥** the aborted run's last tick, so no backwards step is observed, the
  stale latch survives, and the cursor stays ahead — the first stretch of the replay would be
  silent. Narrow, but real.
- It is also *not necessary*, and it is not a safe *rebuild* trigger on its own: the fields
  `music_count` is derived from (`actor+0x16C` sound offset, `actor+0x160` start base) are actor
  state; anything that rewrites them mid-song would look like a restart.

⇒ Use (1) as the reset, (2) as a cross-check, and (3) only as a cursor guard — exactly the
`if music_count <= self.prev_music_count { return; }` shape `mines.rs:280-282` already uses.

One more: Quick **Fail** (triple-3) does *not* redirect; the song ends through STAGE_RESULT. The
`prev == GAMEPLAY` exit callback covers it. And the actors are driven to `STEP_GAME_OVER`, so
`judgeNotes` stops being called at all (`onUpdate` only calls it in the `step == 4` branch) —
the tick stream stops on its own.

---

## Open questions

1. **`state[] == 2 / 3 / 4` (`REC / GEN / REP`) — who writes them?** `judgeNotes` explicitly
   accepts `state[i] == 1 || state[i] == 4` as "steppable", but the parser only writes `0/1` and
   the turn modifier only permutes. No producer located. **[INF]** The predicate above is immune
   (it uses `!= 0` for existence and `== 1` only for the shock test), but if a producer exists it
   could in principle create a note whose panels are all `4` — which would then *not* be
   classified as a shock by the engine either, so we would still match the engine. *Diagnostic:*
   one-shot log of any note in the Results vector with `state[i] > 1`, with `kind`, `music_count`
   and the full `state[]`.
2. **Coalescing window.** Distinct records can round to the same or adjacent milliseconds
   (TPS-150 rescale, §"Results-vector coverage"). `dedup()` handles exact collisions;
   near-collisions need a `COALESCE_MS` (the clap is ~214 ms long, so anything under ~15-20 ms is
   indistinguishable from a single clap anyway). Needs a cabinet listen test, not static analysis.
3. **Does a non-playing side ever get a `GamePlayActor`?** The recommended algorithm assumes
   "present in the DPS child list ⇒ that side is playing". Strong circumstantial support: an actor
   unconditionally judges notes and indexes `(&DAT_1806F2ED0)[side]`, so a phantom actor would
   produce visible P2 judgments in a 1P game. **[INF]** *Diagnostic:* the first-tick log line in
   §"Timing of availability" — `siblings` must be 1 on a solo song.
4. **Doubles from the P2 side:** does `+0x84` report `1`? Affects only which side's option value
   gates a doubles session. **[INF]** *Diagnostic:* log `side`/`style` on the first tick of a
   doubles song started from the right-side card reader.
5. **`+0x18` on a note record.** Set to `1` by the CUT / JUMP-OFF mods on the suppressed note, and
   by the tail-link pass on an orphaned tail (`0x1801C91A9`). It lives inside `game_note.rs`'s
   `_pad2` (`+0x0C..+0x1C`). Not needed by the predicate (`kind` already covers both cases), but
   worth naming in `game_note.rs` if anyone touches that struct — it is the engine's "this note is
   inert" flag.
6. **`DancePlaySequence+0x50` difficulty is probably per-stage, not per-side.** `FUN_180032360`
   resolves difficulty from a stage-entry table at `DPS+0x88` (`0xC`-byte entries), and both actors
   share one DPS — so `csv_export.rs:70`'s per-actor read of `DPS+0x50` likely yields the same
   value for both sides. Irrelevant to assist tick (we never compare difficulties; we follow one
   actor's own note vector), but it means the brief's "2P on different difficulties" case needs no
   special handling at all. **[INF]**
