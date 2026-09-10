# RE Findings — 2-Player BPL Mode (Ghidra pass, 2026-09-09)

Program: `gamemdx_20260825.dll` in the `DDRWorld_Ghidra` project (live MCP instance).
All addresses file-relative to `0x180000000`. Extends
`docs/in_shop_battle_local_versus_research.md`; items below either CONFIRM, CORRECT or
ADD to that document. Labels R1–R7 match the register's research items.

## R1 — `GameWork+0` flip inside `onInitialize` is safe

`GameWork` global = `DAT_1806f14f8` (520 xref sites program-wide). `onInitialize`
(`FUN_180071ce0`) reads it at exactly three sites, all `*(int*)*DAT_1806f14f8` (= `+0`):

| Site | Read | Effect when the wrapper presents 0 |
|---|---|---|
| `0x180071d7f` | `== 1` → `main_tag` else `main_single` | 2-participant layout |
| `0x180071fc8` | `== 0` → hide `score_3p/4p_usr`, `gauge_3p/4p_usr` | 3P/4P boards hidden |
| `0x180072316` | gauge art row: `(gw==1) ? (pos>1)+2 : (pos>1)` | `dama_gauge_{n}p_single` row |

Callee closure to depth 3 (`get_function_call_graph`, 45 internal functions) intersected
with the xref set: **zero readers**. The two leaves the graph did not expand were read
directly: `FUN_18010f960` = `shared_ptr` move-assign, `FUN_1801b76f0` = `lower_bound`
over 600-byte music-DB entries. Every `Ordinal_*` callee is a libafp/libavs export and
cannot see `GameWork`. Conclusion: flipping `GameWork+0` to 0 across the stock
`onInitialize` call changes only the three intended branches.

## R2 — `PlayerWork` name is an inline `char[]` at `+0x0C`; stock getName semantics

`FUN_1801e88a0(PlayerWork** wrapper)` (getName):

```
pw = *wrapper
if (pw->entered (byte +0x4) != 0 && pw->name[0] (byte +0xC) == 0)
    return pw->side (+0x0) < 2 ? {"PLAYER1","PLAYER2"}[side] : "PLAYER"
return (char*)(pw + 0xC)
```

The network record builder (`MatchingCautionSequence` ChildActor `FUN_1800a7410` case 1,
iterating `DAT_1806f2ee0[side]` = the same wrapper table `stage_records::player_work`
walks) copies **9 bytes** (`memcpy(rec+8, getName(), 9)` — 8 chars + NUL) and
`rec.ddrcode = PW+0x18`. `PW+0x0` = side index, `PW+0x4` = entered byte (both already used
by `stage_records`). ⇒ The mod can reproduce getName in ~4 lines without a new signature:
bounded copy of ≤ 8 bytes from `PW+0xC` (stop at NUL), fall back to `PLAYER{side+1}`.
`BATTLE_INFO.team_id` (`+0x20`) in stock comes from a virtual getter on `*(PW+0x1790)`,
NOT from a plain field — irrelevant here (D7: `team_id = 0`).

## R3 — Board sidedness: position index = cabinet side in stock 1v1

Ctor `FUN_180071740`, `GameWork+0 == 0` (1v1) branch: `iVar15 = GameWork+0x8` (the local
player's side) and the local player's `player_index` is written to `position[iVar15]`;
the remote fills the remaining `-1` slot. So in stock 1v1 the P1-side local player is
position 0 (`score_1p_usr`, `gauge_1p_usr/main_gauge_usr`, art `dama_score_base_1p`), a
P2-side local player is position 1. Board art/index are selected from
`BATTLE_INFO[pos].+0x38` (`dama_score_base_{idx+1}p`, `dama_gauge_{idx+1}p_single`).
⇒ Mapping `position i ← side i`, `player_index = i`, `+0x38 = i` reproduces stock
sidedness exactly (D7 confirmed).

## R4 — `Actor::addChild` (`FUN_18021f230`) — shape + insert semantics

```
18021f230  48 3B CA            CMP RCX,RDX          ; child == parent → ret
18021f233  74 63               JZ ret
18021f235  48 85 D2            TEST RDX,RDX         ; null → ret
18021f238  74 5E               JZ ret
18021f23a  48 83 7A 08 00      CMP [RDX+8],0        ; already has parent → ret
18021f23f  75 57               JNZ ret
18021f241  48 83 7A 10 00      CMP [RDX+0x10],0     ; already has next → ret
18021f246  75 50               JNZ ret
18021f248  48 8B 41 18         MOV RAX,[RCX+0x18]   ; first child
18021f24c  45 33 C0            XOR R8D,R8D
18021f24f  48 85 C0            TEST RAX,RAX
18021f252  74 16               JZ insert_head
18021f254  44 8B 4A 28         MOV R9D,[RDX+0x28]   ; child requested priority
18021f258  44 39 48 24         CMP [RAX+0x24],R9D   ; sibling effective priority
18021f25c  76 0C               JBE stop
18021f25e  4C 8B C0            MOV R8,RAX
18021f261  48 8B 40 10         MOV RAX,[RAX+0x10]
18021f265  48 85 C0            TEST RAX,RAX
18021f268  75 EE               JNZ loop
...        ; link: child+0x08 = parent; splice after R8 (or at head); child+0x24 = child+0x28
```

Signature candidate (48 bytes, no relocations, distinctive):
`48 3B CA 74 63 48 85 D2 74 5E 48 83 7A 08 00 75 57 48 83 7A 10 00 75 50 48 8B 41 18 45 33 C0 48 85 C0 74 16 44 8B 4A 28 44 39 48 24 76 0C 4C 8B C0 48 8B 40 10 48 85 C0 75 EE`.
Semantics: children are kept in descending `+0x24` priority; a new child is spliced
BEFORE existing equal-priority siblings (newest-first). The ctor zeroes `+0x28`, so the
frame lands ahead of the (priority-0) `GamePlayActor`s — identical to stock, where the
matching DPS also adds it after them. Consequence: the frame's `onUpdate` sees the
previous frame's score (1-frame latency, invisible under the smoothing).

## R5 — Stock creation site, argument ABI, and the network-block dependency

Creation site `MatchingDancePlaySequence::onUpdate` `FUN_180061cc0` @ `0x1800621c0..0x1800622b3`:

```
record  = PW[first entered side] + 0x590 + (DPS+0xE8 /*stage*/) * 0x2B8   ; 0x590 build-dependent (stage_records derives)
mcode   = record+0x0 ; diff = record+0x4
isEx    = FUN_1801ea320()                       ; use-EX-score → R9B
isDouble= (*(DPS+0x118)+2 != 0) || (*(DPS+0x120)+2 != 0)
a       = agcs_heap_malloc(*DAT_180466030, 0x280, 0)
if a: ctor(a, *(DPS+0x138 /*LayoutActor*/) + 0x98, &DPS+0x128 /*GamePlayActor*[2] owned by the DPS*/, isEx, isDouble, mcode, diff)
addChild(DPS, a)    ; a may be NULL — addChild null-checks
```

Ctor prototype (MS x64): `ctor(this, layoutDesc, actorsArray, u8 isEx /*R9B*/, i32 isDouble, i32 mcode, i32 diff)` — args 5–7 on the stack; Rust `extern "C" fn(*mut u8, *const u8, *const *mut u8, u8, i32, i32, i32) -> *mut u8` matches.

**Correction to the research doc's "null blocks are harmless":** the ctor dereferences
`(&DAT_1806f3ac8)[DAT_1806f391c]` (local cabinet block) WITHOUT a null check
(`MOV R8,[R13+RAX*8+0x1d8]` then `CMP dword [RAX+R8+0xc],0`). It is safe in local play
only because of static initialization: `CNetworkManager`'s constructor `FUN_1801bbbe0`
runs from the CRT initializer table (`FUN_1802ce4d0`, entry in `.CRT$XCU` @ `0x1802d94e8`,
`atexit(FUN_1802d6780)` pairs the dtor) and sets `DAT_1806f3918/391c = -1,-1`
(role/local-idx), `DAT_1806f3ac8/3ad0 = 0` (cab blocks) and
`DAT_1806f3ac0 = new(0x178)` initialised by `FUN_1801bb970` (two 0x1C records with
`player_index = ddrcode = -1`). Index **−1** of the block array is `+0x1D0` =
`DAT_1806f3ac0`, i.e. a valid placeholder block whose records are all invalid
(`+0xC < 0`) ⇒ no position matches ⇒ in the versus (`GameWork+0 == 1`) branch every
unmatched player goes to positions 2/3 via `GetPlayerInfo(FUN_1801bccb0)`, which
iterates `DAT_1806f3ac8/3ad0` (both null → skipped) and resets the record to −1/empty.
Net effect: `BATTLE_INFO[0..1].+0x38 = -1`, `[2..3].+0x38 = 2,3`, four
`set player info : position=…` log lines, nothing else. The mod overwrites all of it.

Design consequence: add a cheap **network-idle pre-check** before the ctor — read the
local cabinet index and require `-1` (decoded from the ctor body's first
`48 63 05 disp32` = `MOVSXD RAX,[rip+DAT_1806f391c]` at `0x180071968`). A live matching
session is already excluded by the `event_mode` gate; the check makes the −1 dependency
explicit and fail-closed instead of implicit.

`+0x1B0` participants is set to 2 when `GameWork+0 == 0`, 4 when `== 1`, and LEFT
UNSET otherwise — the mod writes 2 explicitly (already in the plan).

## R6 — Package-resident pre-check anchor (the `dance_matching` slot pointer)

`onInitialize` @ `0x180071d6e`:

```
48 8B 05 <disp32>        MOV RAX,[rip+DAT_1806f2d70]   ; scene-resource-manager global (ptr to ptr)
48 8B 08                 MOV RCX,[RAX]
4C 8B A9 F0 07 00 00     MOV R13,[RCX+0x7F0]           ; slot-31 package pointer (dance_matching)
48 8B 05 <disp32>        MOV RAX,[rip+DAT_1806f14f8]   ; GameWork (the R1 read)
```

One AOB `48 8B 05 ?? ?? ?? ?? 48 8B 08 4C 8B A9 ?? ?? ?? ?? 48 8B 05` yields BOTH the
manager global (rip disp at match+3) and the slot offset (imm32 at match+13 — derived,
never hardcode `0x7F0`). Pre-check: `*(*mgr + slot_off) != 0`. Without it a missing
`dance_matching.arc` NULL-derefs inside stock `onInitialize`
(`(**(code**)(*layer+0xE8))(layer,1)` right after the failed `main_single` create).

The layout builder `FUN_18006bd40` (sole caller: `LayoutActor::onUpdate` `FUN_18006bb30`,
one class shared by both DPS variants) inserts `"dance_matching"` into the position map
UNCONDITIONALLY (`0x18006bfde`, with a zero/`XMM6` fallback when the `matching_usr`
marker is missing) — the anchor exists in local versus.

## R7 — `GamePlayActor` score-select shape (offset derivation + attestation)

Matching DPS case 0xB @ `0x1800628e8`:

```
80 B8 D0 01 00 00 00     CMP byte [RAX+0x1D0],0      ; isEx (BYTE)
74 08                    JZ +8
48 05 D8 01 00 00        ADD RAX,0x1D8               ; EX score
EB 06                    JMP +6
48 05 D4 01 00 00        ADD RAX,0x1D4               ; money score
8B 00                    MOV EAX,[RAX]
```

AOB `80 B8 ?? ?? 00 00 00 74 08 48 05 ?? ?? 00 00 EB 06 48 05 ?? ?? 00 00 8B 00` → the
three offsets are read from the match (match+2 / +11 / +18) instead of hardcoded, and
the signature IS the cross-build attestation the doc asked `shape_diff.py` for.
`+0x1D4/+0x1D8` agree with `song_reset`'s `GPA_SCORE_OFFSET`/`GPA_EX_SCORE_OFFSET`.

## Vtable / dispatcher facts (confirm + add)

`MatchingBattleFrameActor::vftable` `0x1803626f8`, **9 slots** (next COL follows slot 8):
`[0]` `FUN_180071ba0` deleting dtor · `[1]` `FUN_18019de30` · `[2]` `FUN_18021d980` ·
`[3]` `FUN_18021d7d0` system dispatcher · `[4]` `FUN_180071ce0` onInitialize ·
`[5]` `FUN_1800735f0` onFinalize · `[6]` `FUN_180072e20` onUpdate · `[7]` `FUN_180072f70`
onDraw · `[8]` `0x180001000` (ret 0). COL at `[-1]` = `0x1803bbdf0` (copy into the clone's
slot −1 like `custom_options/rows.rs::build_mod_vtable`).

`agcs::Actor::onMessage` (`FUN_18021d7d0`) — flags word at `this+0x50`: bit0 update
enabled, bit1 draw enabled, bit2 skip-one-update, bit8 initialised. `0x102` on an
un-initialised actor first broadcasts `0x101` to itself (→ slot 4) then calls slot 6;
`0x103` calls slot 7 only when bit8 is set; `0x104` calls slot 5 and clears bit8. The
ctor sets `+0x50 = 3`. ⇒ No need to send `0x101` ourselves; and the slot-4 wrapper's
SKIP path (package not resident) must clear bits 0–1 (`+0x50 &= !3`) so slots 6/7 never
run with a NULL layer. `onFinalize` null-checks the layer (`+0xA8`) — safe on the skip path.

`onUpdate` stock body: network read → `display = min((display + target + 1)/2, target)`;
`+0x34 = (f32)display / (f32)max` (`MOVSS` — a float, Ghidra mistypes it) guarded by
`max != 0`; tail `JMP rel32` → rank fn `FUN_180073650` (derive the rank fn from the last
instruction of stock slot 6). Rank fn with `GameWork+0 == 1` and 2 participants:
leader `diff = score − second`, second `diff = score − first` — coincides with the 1v1
formula (doc claim verified). Ties share rank 0 (both badges "1st" at 0–0, stock behaviour).

## Signature / derivation inventory for the design

| Name | Kind | Source |
|---|---|---|
| `battle_frame_actor_vtable` | RTTI | `.?AVMatchingBattleFrameActor@dance@sequence@@` (present) |
| `layout_actor_vtable` | RTTI | `.?AVLayoutActor@dance@sequence@@` (present) |
| `battle_frame_ctor` | AOB | prologue from the doc (byte-identical 20250805↔20260825) |
| `battle_frame_rank_fn` | derived | last instr of `vtable[6]` = `E9 rel32` |
| `matching_local_cabinet_idx` | derived | first `48 63 05 disp32` in `battle_frame_ctor` body |
| `actor_add_child` | AOB | R4 bytes |
| `dance_matching_slot_probe` | AOB (+ 2 derived values) | R6 bytes → `scene_resource_manager` (rip) + `dance_matching_slot_off` (imm32) |
| `gpa_score_select` | AOB (+ 3 derived offsets) | R7 bytes → `gpa_is_ex_off` / `gpa_ex_score_off` / `gpa_money_score_off` |
| `agcs_heap_malloc`, `app_heap_handle`, `gameplay_actor_vtable` | existing | — |

Everything else (GameWork, entered flags, PlayerWork, stage record header, DPS/child
walk, scene + frame callbacks) is existing service API.
