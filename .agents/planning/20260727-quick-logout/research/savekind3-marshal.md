# R5 — The `savekind == 3` (logout) marshal, and what the sanitiser must touch

Source: Ghidra decompile of `ReflectSavePlayerData` = `FUN_180018e50`
(`gamemdx_20260721.dll`, file-relative). This is the function that fills the
per-side network-save staging block (`DAT_1804d0f40 + side*0xBED8`) that ess.dll's
sender later serialises. `param_2` = savekind (2 = per-stage, 3 = logout).

## 1. Where score data comes from — all of it is record-sourced

The `savekind == 3` path marshals score data in exactly two places, and **both
gate every stage block on the same two record fields**:

```c
// header: "which stages have data" bitmask + count
count = min(GameWork->0xC + 1, 5);
for (i = 0; i < count; i++)
    if (rec[i].mcode != -1 && rec[i].end_time != 0)  bit_set(i);

// body: one block per stage
for (i = 0; i < count; i++) {
    rec = course_session ? PlayerWork + 0x2D8            // single course record
                         : PlayerWork + 0x590 + i*0x2B8; // 5-slot array
    if (rec->mcode == -1 || rec->end_time == 0) continue;   // <<< THE SKIP
    ...marshals mcode, difficulty, style, score, EX, combo, judgments,
       fast/slow, lamp, per-panel steps string, ghost vector, grade fields...
}
```

- `mcode` = record `+0x00` (int). `end_time` = record `+0x268` (qword).
- `course_session` here is **`PlayerWork+0x4C == 10`** (style 10 = course), *not*
  `GameWork+0x70`. A course session marshals the **course record at
  `PlayerWork+0x2D8`** once and breaks — so the sanitiser must virginise that
  record too, not just the 5 array slots.
- One record-derived aggregate: a per-side qword accumulates `rec->+0x4C`(EX)
  over *emitted* stages only — consistent with an empty stage list (stays 0).
- The `savekind == 2` path reads the same records with the same skip test, so
  the mechanism cannot desync the two save kinds.

**Everything else in the payload is profile data** sourced from
`PlayerWork`/`Customize`/`GameWork`, wanted in the save: name, weight and
`is_disp_weight`-adjacent fields, ~29 `Customize` vtable getters (the cosmetics),
side-panel cursors, played-music tree (`PlayerWork+0x1768`), option cursors,
event-data table (`DAT_1804cfb40`), region/entry metadata, session tick count.

## 2. `mcode = -1` is sufficient — with one documented residual

Writing `mcode = -1` into all 5 array slots **and** the course record of a
tainted side suppresses 100 % of per-stage score emission and keeps the header
bitmask/count consistent (they are derived from the same fields).

**Residual (accepted, documented):** one score-adjacent block is *not* gated on
`mcode`/`end_time`: a dan/grade-course block near the tail, emitted only when

```c
FUN_1801d8250(side, extra_idx)            // decompiled: requires NOT course,
                                          // NOT event, extra_idx != gw->0x10,
                                          // side has e-am data, entry class == 2
&& rec[extra_idx].+0x270 == 9             // extra-stage slot's class field == 9
```

where `extra_idx = DAT_18047E784 + 1` (the extra-stage slot). Its payload comes
from a `GameWork+0x140` grade table, not the records. It can only fire for a
session that played a **class-9 (grade-check) extra stage** — never for a quick
logout from song select (the extra slot is virgin; class is 0), and effectively
never on this cabinet (Premium Free freezes the counter so extra stage never
arms, and dan play is course mode, which the gate excludes). Not worth chasing
the `+0x270` offset for; noted in the design's limitations.

There is also a header quirk: `if (rec[extra_idx].+0x270 == 9)` zeroes three
`Customize`-sourced header fields. Same reasoning — virgin extra slot ⇒ inert.

## 3. Sanitiser timing

`ReflectSavePlayerData(side, 3)` is called from `PlayerDataSaveLogoutRequest`
(`FUN_18001EE90`), which `SavePlayerDataActor`'s update reaches a few frames
after `EAmExitRootSequence::onSetup` creates the actors (scene 0-idx 34). The
summary (scene 32) renders **before** 34. Therefore:

- Sanitising **at scene-34 entry** is early enough for the marshal and late
  enough to leave TOTAL RESULTS intact. (A `scene_manager::on_scene_change`
  callback with `next == 34` — no new detour.)
- The records are dead after this point regardless: `GameWork::reset` and the
  record constructors wipe everything at the next session start, and no
  further song-select commit happens in this session. Trashing them is free.

## 4. Offsets the sanitiser needs, and where they come from

| Constant | Source (already shipped) |
|---|---|
| GameWork ptr-ptr global | `stage_record_accessor` +3 RIP disp32 |
| player_work_table | `stage_record_accessor` +16 RIP disp32 (cross-checked vs derived `player_work_table`) |
| record base `0x590` / stride `0x2B8` | `stage_record_accessor` +55 / +47 imm32 |
| **course record offset `0x2D8`** | `stage_record_accessor` **+36 imm32** (the wildcarded `ADD RAX, imm32` in the accessor's course branch — already inside the matched bytes, not yet decoded by `premium_free`) |
| stage counter offset `0xC` | `premium_free_stage_inc` disp8 (only needed for logging; the sanitiser wipes all 5 slots regardless of the counter) |
| `mcode` offset | record `+0x00` (the accessor returns the record base) |

No new signature is required for the sanitiser. The `premium_free` decode block
is the reference implementation; hoisting it into a shared helper (D24) adds
only the +36 read.

## 5. Interaction with the existing suppression

Current behaviour (in `custom_options_persistence::save_sender`): tainted side +
`savekind == 3` ⇒ pretend-success, packet never sent. New behaviour: the
scene-34 sanitiser virginises the records, `save_sender` **lets the logout save
through** for a tainted side, and keeps suppressing `savekind == 2` unchanged.
Fail-closed rule (D25): if the sanitiser could not arm (decode failed,
scene_manager unavailable) the old suppression path remains in force — the
save_sender check consults "was this side sanitised this session?" state, not
just the taint flag.
