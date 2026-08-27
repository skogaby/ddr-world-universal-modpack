# Sound-Bank Slot 4 Safety + Version-Stable Anchors — RE Notes (assist-tick)

**Primary build:** `gamemdx_20260721.dll` (image base `0x180000000`). Cross-checked against
`gamemdx_20260616.dll`, `gamemdx.dll` (=20260421) and `gamemdx.dll` (=20260324).
All addresses are **file-relative** (`+0xNNNNNN`); because the base is `0x180000000`, the
Ghidra VA quoted inline (`0x1801AB7A0`) and the file-relative form (`+0x1AB7A0`) are the
same number with the base stripped.

**Evidence discipline:** every claim is tagged **[OBS]** (read directly off the
disassembly / decompilation in Ghidra, address cited) or **[INF]** (inferred — must be
re-proven before it is depended on). Per this project's rule, **do not treat any [INF]
line as fact when implementing.**

Companion document: `game-sound-engine.md` (same directory) — the general audio-subsystem
note. This document supersedes three of its claims; see
[Corrections to `game-sound-engine.md`](#corrections-to-game-sound-enginemd).

---

## Overview

The question this note answers: **can we park our own `IXACT2SoundBank*` in the audio
manager's unused sound-bank slot 4 (`mgr+0x50`) for the process lifetime, and play through
it with `se_play(4, cue, pan)`?**

Short answer: **yes, with two cheap runtime guards.** [OBS] The sound-bank slot array is
exactly 6 entries; **exactly three** functions in the entire 19 MB module write to it
(constructor, `soundbank_create`, `sound_file_unregister`); the only destroyer selects its
victim **by `file_id` equality** against a file-manager record id, and slot 4's `file_id`
is initialised to `-1` and can never be written, because the slot-selection function
`bank_slot_of_file` provably returns only `{0,1,2,3,5}`. There is no slot loop that
destroys unconditionally, no consistency validator, no `bank_id`-indexed table anywhere
downstream, and the manager is torn down exactly once — from `Application::onShutdown`.

Both load-bearing constants (the "4 named banks" loop bound and the "fallback = 5"
immediate) are **bytes inside a proposed AOB signature**, so a future build that adds a
fifth named bank breaks the pattern match instead of silently colliding with us.

---

## Slot-array layout and bound

`mgr = *(void**)(+0x6F2D60)` (20260721). Object is `0x20F0` bytes. **[OBS]**

| Offset | Field |
|---|---|
| `+0x00` | `IXACT2Engine*` |
| `+0x08 + slot*0x10` | `int file_id` (`-1` = empty) |
| `+0x10 + slot*0x10` | `IXACT2SoundBank*` (`NULL` = empty) |
| `+0x68 / +0x70 / +0x78` | `vector<{int file_id; int slot; HANDLE file; IXACT2WaveBank*}>` (0x20 stride) — **the member immediately after the slot array** |

### Bound = exactly 6 slots (0..5), proven three independent ways

**1. Constructor memset size. [OBS]** `audio_mgr_ctor` (`+0x1AAB60`):

```
1801aaba6  MOV  [RCX],R13            ; mgr+0x00 = NULL  (engine ptr, filled later)
1801aabab  LEA  R8D,[R13+0x60]       ; size = 0x60
1801aabaf  ADD  RCX,0x8              ; dst  = mgr+0x08
1801aabb3  CALL 0x18027bf90          ; memset(mgr+0x08, 0, 0x60)   == 6 * 0x10
1801aabb8  MOV  [RSI+0x68],R13       ; next member begins at +0x68
```

**2. Constructor `-1` fill — exactly 12 stores = 6 {file_id, bank} pairs. [OBS]**
`+0x1AAC57 .. +0x1AACB9`:

```
1801aac57  MOV  dword [RBP-0x9],0xFFFFFFFF
1801aac5e  MOV  RAX,[RBP-0x9]
1801aac62  MOV  [RSI+0x08],RAX   ; slot0.file_id = -1
1801aac66  MOV  [RSI+0x10],R13   ; slot0.bank    = NULL
1801aac6a  MOV  RAX,[RSI+0x08] / 1801aac6e  MOV [RSI+0x18],RAX   ; slot1.file_id
1801aac72  MOV  RAX,[RSI+0x10] / 1801aac76  MOV [RSI+0x20],RAX   ; slot1.bank
1801aac7a  ...                  / 1801aac7e  MOV [RSI+0x28],RAX   ; slot2.file_id
1801aac82  ...                  / 1801aac86  MOV [RSI+0x30],RAX   ; slot2.bank
1801aac8a  ...                  / 1801aac8e  MOV [RSI+0x38],RAX   ; slot3.file_id
1801aac92  ...                  / 1801aac96  MOV [RSI+0x40],RAX   ; slot3.bank
1801aac9a  ...                  / 1801aac9e  MOV [RSI+0x48],RAX   ; slot4.file_id  <-- OUR SLOT
1801aaca2  ...                  / 1801aaca6  MOV [RSI+0x50],RAX   ; slot4.bank     <-- OUR SLOT
1801aacaa  ...                  / 1801aacae  MOV [RSI+0x58],RAX   ; slot5.file_id
1801aacb2  ...                  / 1801aacb6  MOV [RSI+0x60],RAX   ; slot5.bank
```

Last pair written is `+0x58 / +0x60`. **The array is `[0x08, 0x68)` — 6 slots. There is no
slot 6+; a "higher unused slot" does not exist.** [OBS]

**3. The destroyer's loop terminator is literally `mgr+0x68`. [OBS]** See
[Teardown analysis](#teardown-analysis).

**Confirmed for our design:** `bank[4]` is at **`mgr+0x50`** and `file_id[4]` is at
**`mgr+0x48`**. [OBS]

### `bank_slot_of_file` can never return 4

`+0x1AA3C0`, tail of the match loop. **[OBS]**

```
1801aa440  MOV  R9,[RSI]              ; RSI walks a 4-entry stack array of char*
1801aa45b  CALL strncmp
1801aa460  TEST EAX,EAX
1801aa462  JZ   0x1801aa476           ; match -> return index
1801aa464  INC  EBX
1801aa466  ADD  RSI,0x10
1801aa46a  CMP  EBX,0x4
1801aa46d  JC   0x1801aa440           ; while (EBX < 4)
1801aa46f  MOV  EAX,0x5               ; no match -> 5
1801aa474  JMP  0x1801aa478
1801aa476  MOV  EAX,EBX               ; match    -> 0..3
```

Range is `{0,1,2,3} ∪ {5}`. **4 is unreachable.** The four names are the adjacent literals
at `+0x3800D0` `"bgm_menu"`, `+0x3800E0` `"se_system"`, `+0x3800F0` `"se_normal"`,
`+0x3800FC` `"voice"` (LEA'd at `+0x1AA3D6 / +0x1AA3E5 / +0x1AA3F4 / +0x1AA403`). [OBS]

**Cross-version [OBS]:** the byte pattern covering `INC EBX / ADD RSI,0x10 / CMP EBX,imm8 /
JC / MOV EAX,5` matches **uniquely** on all four builds, and the `imm8` reads `0x04` and
the fallback reads `0x05` in **all four**:

| Build | loop match | `CMP EBX,imm8` | fallback |
|---|---|---|---|
| 20260721 | `+0x1AA440` | `04` | `5` |
| 20260616 | `+0x1A93B0` | `04` | `5` |
| 20260421 | `+0x1A8580` | `04` | `5` |
| 20260324 | `+0x1A78B0` | `04` | `5` |

---

## Every slot reader / writer

Method: `ghidra_get_xrefs_to(+0x6F2D60)` returns **424** reference sites. I ran an
exhaustive scripted screen (Ghidra inline script) over all 424: for each site, take the
destination register of the `MOV reg,[rip+mgr]` and walk forward 40 instructions looking
for any store into `[reg + 0x08..0x67]`. **8 candidates**; all 8 manually triaged as
false positives (the walker crosses function boundaries and does not track register
clobbering):

- `1801A84B9 / 1801A84C1` (`MOV qword [RAX+8],-1`) — reached from the mgr load at
  `+0x1A843B` inside `se_play_helper`, but that RAX is dead after
  `1801A8445 CMP byte [RAX+0x20c4],0` and the function `RET`s at `+0x1A8489`; the flagged
  stores are past the boundary, inside `FUN_1801A8490`'s per-side last-handle table. **[OBS]**
- 6 hits at `1801AAB7B..1801AAB83` (`MOV [RAX+0x10],RBX` etc.) — the **prologue register
  spills of `audio_mgr_ctor`** (`RAX = RSP` there), reached by the walker falling out of the
  4-byte-body functions `+0x1AAAE0` / `+0x1AAB20`. **[OBS]**

Resulting complete accessor table. `slot[i].fid` = `mgr+0x08+i*0x10`,
`slot[i].bank` = `mgr+0x10+i*0x10`.

| Function | Addr | Access | Which slot | Condition | Tag |
|---|---|---|---|---|---|
| `audio_mgr_ctor` | `+0x1AAB60` | **WRITE** `fid=-1`, `bank=NULL` ×6 | 0..5 | once, from `Application::onBoot` `+0x28A5` | [OBS] `+0x1AABB3`, `+0x1AAC57..+0x1AACB9` |
| `soundbank_create` | `+0x1AAFA0` | **WRITE** `bank=<new>` then `fid=file_id` | `bank_slot_of_file(file_id)` ∈ {0,1,2,3,5} | **only if** `bank[s]==NULL && fid[s]==-1` | [OBS] `+0x1AAFAF..` |
| `sound_file_unregister` | `+0x1AB3D0` | **DESTROY + CLEAR** (`Destroy()`, `bank=NULL`, `fid=-1`) | first slot in `[0x08,0x68)` whose `fid == arg` | `.xsb` extension branch only | [OBS] `+0x1AB41E..+0x1AB463` |
| `se_play_inner` | `+0x1AB7A0` | READ `bank[bank_id]` | `bank_id` arg, **unbounded** | — | [OBS] `+0x1AB7AF..+0x1AB7C8` |
| `se_prepare_inner` | `+0x1AB680` | READ `bank[bank_id]` | `bank_id` arg, **unbounded** | — | [OBS] `+0x1AB68F..` |
| ~50 inlined `se_play_inner` copies in scene code | e.g. `judgeNotes` `+0x5F21C`, `+0x1A29E6`, `+0x1A2B56`, `+0x70C7A`, … | READ, **constant displacement** (`[RSI+0x30]` = bank 2) | fixed literal | — | [OBS] (sampled `+0x1A29C0`, `+0x1A2B30`, `+0x5EC70`) |

**Nothing else in the module touches `[0x08,0x68)`.** [OBS — exhaustive over all 424 mgr
references; residual risk is only code that obtains `mgr` without referencing the global,
of which the only instance is `FUN_1801AAED0(mgr)`, read below and shown not to touch the
array.]

Notable non-writers (all **[OBS]**), each of which one might have expected to loop the slots:

| Function | Addr | What it actually loops |
|---|---|---|
| `wavebank_create` | `+0x1AB050` | the wave-bank `vector` at `mgr+0x68/0x70/0x78` and the sorted handle vector at `mgr+0x20C8` — **never the sound-bank slots** |
| `se_stop_all` | `+0x1AA850` | handle slots `mgr+0xA0 .. mgr+0x20A0` |
| `audio_frame_update` (reaper) | `+0x1ABB30` | handle slots `mgr+0xA0 .. mgr+0x20A0` |
| `se_set_volume` inner | `+0x1AB930` | handle slots (re-apply matrices) |
| set-attenuation | `+0x1AA9B0` | handle slots (re-apply matrices) |
| `handle_slot_alloc` | `+0x1AB5B0` | handle slots, round-robin `mgr+0x20E8` |
| manager pre-dtor | `+0x1AAED0` | frees the 3 vectors (`+0x68`, `+0x20A0`, `+0x20C8`) — **does not read or clear the slot array at all** |

Single-caller facts, which bound the reachability of the two mutators. **[OBS]**

- `soundbank_create` (`+0x1AAFA0`): exactly **one** caller — `sound_file_register`
  (`+0x1AA520`) at `+0x1AA586`.
- `sound_file_unregister` (`+0x1AB3D0`): exactly **one** caller — `FUN_1801AC6C0` at
  `+0x1AC6F0`, which is the `audio::Xsb/XwbFileCallback` **unload** vtable method
  (`FUN_1801AC6C0(this, file_id) → lock → sound_file_unregister(file_id) → unlock`).

---

## Teardown analysis

### The one destroyer: victim selection is by `file_id` equality

`sound_file_unregister` (`+0x1AB3D0`), `.xsb` branch. Full disassembly of the loop **[OBS]**:

```
1801ab3e6  MOV  RSI,[0x1806f2d60]     ; RSI = mgr
1801ab3ed  MOVSXD RDI,ECX             ; RDI = file_id argument
1801ab40e  LEA  RDX,[0x180380120]     ; "xsb"
1801ab415  CALL strncmp
1801ab41c  JNZ  0x1801ab473           ; not .xsb -> wave-bank branch
1801ab41e  LEA  RAX,[RSI + 0x68]      ; end   = mgr+0x68   (one past slot 5)
1801ab422  LEA  RBX,[RSI + 0x8]       ; begin = mgr+0x08   (slot0.fid)
1801ab426  CMP  RBX,RAX
1801ab429  JZ   0x1801ab594
1801ab430  CMP  dword [RBX],EDI       ; slot[i].fid == file_id ?
1801ab432  JZ   0x1801ab43d           ; first match wins
1801ab434  ADD  RBX,0x10              ; stride 0x10
1801ab438  CMP  RBX,RAX
1801ab43b  JNZ  0x1801ab430
1801ab43d  CMP  RBX,RAX
1801ab440  JZ   0x1801ab594           ; no match -> destroy NOTHING, return
1801ab446  MOV  RCX,[RBX + 0x8]       ; the IXACT2SoundBank*
1801ab44d  JZ   0x1801ab455
1801ab452  CALL [RAX + 0x30]          ; SoundBank::Destroy()
1801ab455  MOV  qword [RBX + 0x8],0   ; bank = NULL
1801ab45d  MOV  dword [RBX],-1        ; fid  = -1
1801ab472  RET
```

**This is the strongest single piece of evidence for the whole design.** The loop is a
linear `find_if(fid == arg)` over `[mgr+0x08, mgr+0x68)` with **early exit on no-match and
no fallback**. `EDI` is the caller-supplied `file_id`, which the same function has already
used as an index into the file-manager record table
(`file_id*0xA0 + *(fm+0x28)`, `+0x1AB3F0..+0x1AB402`) — i.e. it is a **non-negative index
into a live record**. `file_id[4] == -1` therefore never matches, so **slot 4 is
unreachable by the only destroyer in the binary.** [OBS; the "file ids are non-negative"
step is **[INF]** — it follows from the record-table indexing and from the callback being
invoked by the file manager for a record it owns, but I did not trace the file manager's id
allocator.]

### No unconditional slot loop anywhere

Searched (and found nothing) for: scene-exit / song-end / game-over / attract-reset paths
that iterate slots and `Destroy()` unconditionally. The evidence is the accessor table
above: after the exhaustive 424-reference screen, the **only** loop over `[0x08,0x68)` in
the module is the one quoted here. [OBS]

Per-song bank lifecycle for comparison: `song_bank_load` (`+0x61680`) loads
`data/sound/win/dance/<code>.xsb|.xwb` → `bank_slot_of_file` → slot **5**; at song end the
file manager unloads the record and calls the callback → `sound_file_unregister(file_id)` →
matches `fid[5]` → clears slot 5. Slot 4 is untouched by that whole cycle. [OBS for the
mechanism; **[INF]** for "song end is what triggers the unload" — not traced.]

### Manager destruction happens exactly once, at app shutdown

`FUN_1801AA490` is the audio-manager teardown. **[OBS]**

```
void audio_mgr_shutdown() {
  if (mgr) {
    FUN_1801aaed0(mgr);            // frees the 3 vectors; slot array untouched
    engine->ShutDown(); engine->...; engine->Release();
  }
  mgr = 0;                          // +0x1AA507: the only WRITE to the global besides onBoot
  FUN_1801ff1b0(file_manager, DAT_18046534c);   // release ddr.xgs
}
```

Callers of `+0x1AA490`: **one** code caller, `FUN_180002AF0` at `+0x2B48`. That function is
`Application::onShutdown` **[INF on the name, [OBS] on the behaviour]**: it tears down every
manager in the process (`+0x1AAB60`'s peer at `+0x1ACB60`, the input/render/net managers)
and unregisters *every* FileCallback group by tag — `"sound"`, `"model"`, and four more
(`+0x180002AF0` body). Its own xrefs are DATA only (`+0x444288`, `+0x44426C` — Application
vtable slots). **⇒ the slot array is initialised once and destroyed once, at process
teardown.** [OBS]

Two WRITE xrefs to `+0x6F2D60` exist in the entire binary: `+0x28AF` (onBoot, store the
freshly constructed manager) and `+0x1AA507` (shutdown, store 0). [OBS]

### Is there a slot↔file_id consistency validator?

**No.** [OBS] The only place the two fields are examined together is `soundbank_create`'s
admission guard:

```c
// +0x1AAFA0
if (mgr->bank[s] == NULL && mgr->file_id[s] == -1) { ...CreateSoundBank...; mgr->file_id[s] = file_id; }
```

Our installation deliberately produces the state `bank[4] != NULL && fid[4] == -1`, which
violates the invariant the game maintains — but **nothing reads that invariant**, so it is
inert. The one way it could ever bite is covered under
[Risks](#risks-and-what-i-could-not-determine) (R-1).

---

## `bank_id == 4` end-to-end trace

Given `bank[4]` = our sound bank and `fid[4] == -1`, `se_play(4, "asti", 0.0f)` executes:

**1. `se_play` (`+0x1AA6E0`) — the mute-filter gate. [OBS]**

```
1801aa6f9  MOVAPS XMM6,XMM2          ; pan arrives in XMM2 (NOT R8D)
1801aa701  MOV  R8D,ECX
1801aa704  DEC  R8D                  ; R8D = bank_id - 1
1801aa707  JZ   0x1801aa72e          ; bank_id == 1 -> skip filter
1801aa709  CMP  R8D,0x4
1801aa70d  JZ   0x1801aa72e          ; bank_id == 5 -> skip filter
1801aa70f  MOV  dword [RSP+0x50],0x5
1801aa717  LEA  RCX,[RSP+0x50]
1801aa71c  CALL [0x1806f2420]        ; se_mute_filter(&state)
1801aa722  CMP  dword [RSP+0x50],0x6
1801aa727  JNZ  0x1801aa72e
1801aa729  OR   EAX,-1 ; JMP ret     ; VETOED -> return 0xFFFFFFFF
```

⚠️ **`bank_id == 4` is NOT exempt** — only 1 and 5 are. Our tick therefore rides the same
`se_mute_filter` veto that the game's own `se_normal` (bank 2) plays ride, including
`judgeNotes`' `se_game_shockarrow`. **[OBS]** That is the *only* behavioural difference
between `se_play(4,…)` and calling `se_play_inner(4,…)` directly (plus the AVS lock at
`+0x1AA73A`/`+0x1AA75A`, taken only when `audio_lock_count (+0x6F38F8) > 0`).

**2. `se_play_inner` (`+0x1AB7A0`) — slot index arithmetic, no bounds check. [OBS]**

```
1801ab7af  MOV  RSI,[0x1806f2d60]    ; unconditional deref of the global -- we must null-check it
1801ab7b6  MOVSXD RDI,ECX            ; RDI = 4
1801ab7be  LEA  RAX,[RDI + 0x1]      ; 5
1801ab7c5  ADD  RAX,RAX              ; 10
1801ab7c8  MOV  RBX,[RSI + RAX*0x8]  ; = [mgr + 0x50]  == bank[4]   <-- exactly our slot
1801ab7cf  JZ   fail                 ; NULL -> -1
1801ab7d7  CALL [RAX]                ; SoundBank::GetCueIndex(cue)      vtable +0x00
1801ab7de  CMP  AX,CX ; JZ fail       ; 0xFFFF -> -1
1801ab805  CALL [R10 + 0x20]         ; SoundBank::Play(idx,0,0,&cue)    vtable +0x20
1801ab80b  JS   fail                 ; hr < 0 -> -1
1801ab81b  CALL 0x1801ab5b0          ; handle_slot_alloc(mgr, cue, bank_id=4 in R8D, pan in XMM3)
```

**No comparison against `bank_id` anywhere in this function** — `bank_id` is used only as
the index and then passed through. [OBS]

**3. `handle_slot_alloc` (`+0x1AB5B0`) — where `bank_id` is stored. [OBS]**

```
1801ab5d0  MOV  EBX,[RCX+0x20e8]                 ; round-robin cursor
1801ab5e8  MOV  [RCX+0x20e8], (EBX+1)&0xFF       ; 256 entries
1801ab5ef  CMP  qword [ (i+5)*0x20 + RCX ],0     ; slot free iff cue==NULL ...
1801ab5fa  CMP  qword [ i*0x20 + RCX + 0xa8 ],0  ; ... AND wave==NULL
1801ab651  MOV  dword [i*0x20 + R11 + 0xb8],R8D  ; handle.bank_id = 4      (mgr+0xA0+i*0x20+0x18)
1801ab664  MOVSS      [i*0x20 + R11 + 0xb4],XMM3 ; handle.pan             (+0x14)
1801ab659  MOV  word  [i*0x20 + R11 + 0xb0],0    ; prepared/deferred flags(+0x10/+0x11)
1801ab66e  MOV  qword [(i+5)*0x20 + R11],RDI     ; handle.cue             (+0x00)
1801ab61f  CALL 0x1801abf90                      ; apply_pan_matrix(mgr, cue, bank_id, pan)
```

**4. `apply_pan_matrix` (`+0x1ABF90`) — the `bank_id` ladder. This is the OOB question. [OBS]**

```
1801abfe4  MOV  EBX,R8D                    ; EBX = bank_id
1801abfc1  CMP  byte [RCX+0x98],0          ; "policy enabled" flag
1801ac00a  CALL [0x1806f2420]              ; se_mute_filter again (only if +0x98 set)
1801ac016  CMP  EBX,0x1 ; JZ  ...          ; bank 1 exempt from attenuation
1801ac01b  MOVSS XMM13,[RDI+0x94]          ; else attenuation = mgr+0x94        <-- applies to bank 4
1801ac02a  MOVSS XMM11,[RDI+0x88]          ; normaliser  (fixed offset)
1801ac045  TEST EBX,EBX  ; JZ  0x1801ac16b ; bank 0  -> mute byte mgr+0x99
1801ac04d  JLE  0x1801ac061                ; bank < 0 -> no mute test
1801ac04f  CMP  EBX,0x4  ; JLE 0x1801ac162 ; bank 1..4 -> mute byte mgr+0x9A   <-- BANK 4 LANDS HERE
1801ac058  CMP  EBX,0x5  ; JZ  0x1801ac16b ; bank 5  -> mute byte mgr+0x99
1801ac061  ...                             ; bank > 5 -> no mute test
1801ac065  MOV  EBX,[RDI+0x20c0]           ; EBX reused: final-mix channel count
1801ac07e  MOVSS XMM9,[RDI+0x8c]           ; volume A  -- FIXED offset, not indexed
1801ac0fd  MOVSS XMM7,[RDI+0x90]           ; volume B  -- FIXED offset, not indexed (>=4ch only)
1801ac25e  CALL [RAX+0x40]                 ; Cue::SetMatrixCoefficients(2, nCh, coeffs)
1801ac162  CMP  byte [RDI+0x9a],0          ; <-- bank 4's mute byte
1801ac16b  CMP  byte [RDI+0x99],0
```

**⇒ `bank_id` indexes NOTHING. It selects between two hard-coded byte offsets (`+0x99` /
`+0x9A`) via a compare ladder, with explicit `JLE` / `JZ 5` fall-through for
out-of-range values. There is no table, therefore no out-of-bounds read for
`bank_id == 4`.** [OBS] Bank 4 shares the `mgr+0x9A` mute byte with banks 1/2/3 — the SE
mute flag cleared by `+0x1AAB20`. If the game mutes SEs, our tick is muted with them
(coefficients forced to 0.0), which is the desired behaviour.

**5. Volume categories are not bank ids. [OBS]** `se_set_volume` inner (`+0x1AB930`) writes
`*(float*)(mgr + 0x8C + category*4)`. Its only code caller, `FUN_1800081A0`, passes
`param_3 != 0` — i.e. **category ∈ {0,1}** only, and the ctor initialises exactly two
floats there (`+0x1AAE93 MOV RAX,0x3f8000003f800000; MOV [RSI+0x8c],RAX`). `bank_id` never
reaches this function. The re-apply loop in `+0x1AB930` (and in `+0x1AA9B0`) reads
`handle.bank_id` from `+0x18` and feeds it back into `apply_pan_matrix` — i.e. our stored
`4` re-enters the safe ladder above, forever. [OBS]

**6. Per-frame reaper (`+0x1ABB30`).** Walks handle slots, `Cue::GetState`, and on
`STOPPED (0x20)` does `Destroy()` + zero the slot. Does not look at `bank_id`. Our cue is
reaped exactly like a native SE. [OBS]

**7. `se_stop_all` (`+0x1AA850`)** issues `Cue::Stop(0)` on every live handle, ours
included. Harmless (a stop, not a destroy; the reaper collects it next frame). [OBS]

### Could the game itself ever call `se_play(4, …)`?

Enumerated argument sources for `bank_id`: **[OBS]**

- literal at the call site: only `0,1,2,3,5` observed (`se_play_helper` `+0x1A82F0`'s 49
  callers pass literals; sampled call sites in `judgeNotes`, `+0x1A29C0`, `+0x1A2B30`,
  `+0x70CF3` are all `2`);
- `bank_slot_of_file` (`+0x1AA3C0`) → `{0,1,2,3,5}`, proven above;
- `FUN_1801ADD50(cue_name)` (used by `FUN_1801ADE50` → `se_play`) → returns
  `strncmp(name,"vo_",3)==0 ? 3 : 2` (`return bVar7 + '\x02'`) → **{2,3}**.

I did not exhaustively read all 49 `se_play_helper` call sites' literals **[INF]**, but
even a stray `se_play(4, "se_xxx")` is harmless: `GetCueIndex` on our bank returns
`0xFFFF` → clean `-1`. The only way it could misfire is a **cue-name collision** between
our bank and a real game cue name → mitigation: name our cue something that cannot collide
(e.g. `"modtick"`, not `"se_*"` / `"vo_*"`).

---

## R1 verdict

> **SAFE — with two cheap runtime guards.**

Confidence: **high** for the primary build and the three older builds present in the
Ghidra project; **medium-high** for unseen future builds (the failure mode is benign and
detectable — see R-1).

Ranked evidence:

1. **[OBS]** The only destroyer (`+0x1AB3D0`) is a `find_if(fid == arg)` over
   `[mgr+0x08, mgr+0x68)` with **no-match ⇒ destroy nothing** (`+0x1AB440 JZ exit`), and
   `fid[4]` is `-1` from the constructor and can never be written because
   `bank_slot_of_file` provably returns `{0,1,2,3,5}` (`+0x1AA46A CMP EBX,4`). Slot 4 is
   structurally unreachable by every mutator.
2. **[OBS]** Exhaustive screen of **all 424** references to the manager global found
   **zero** writes into the slot array outside the three known functions; all 8 script
   candidates were boundary-crossing false positives, individually triaged.
3. **[OBS]** `apply_pan_matrix` handles `bank_id` with a compare ladder over two fixed byte
   offsets, not a table — `bank_id == 4` reads `mgr+0x9A`, in bounds, and every downstream
   re-apply path funnels back into the same ladder.
4. **[OBS]** The manager is constructed once (`onBoot +0x28A5/+0x28AF`) and destroyed once
   (`Application::onShutdown +0x2B48 → +0x1AA490`); the pre-dtor `+0x1AAED0` does not even
   read the slot array.
5. **[OBS]** All of (1)'s structural facts are byte-identical on 20260616 and 20260421
   (unique single matches for both the destroyer loop and the ctor 6-pair fill), and the
   `bank_slot_of_file` bound reads `4` on all four builds.

### Required guards (both trivial)

**G1 — install-time slot claim, computed not hard-coded.** At install, read the manager and
pick the free slot instead of assuming 4:

```rust
// s in 0..6 such that fid[s] == -1 && bank[s] == null
let s = (0..6).find(|i| *(mgr+0x08+i*0x10) as i32 == -1 && *(mgr+0x10+i*0x10) == null)?;
```
Refuse to install (log + fall back) if none is free, or if the `bank_slot_of_file` name
count read from the AOB match (`match+0x2C`) is not `4`. Never write `fid[s]` — leave it
`-1`; writing a fake id is the one thing that could make the destroyer target us.

**G2 — null-check the manager global before every play.** `se_play_inner +0x1AB7AF`
dereferences `*(void**)mgr_global` unconditionally. [OBS]

### Notes that shape (but do not block) the design

- **Prefer `se_play_inner` over `se_play`** if we want the tick to be immune to
  `se_mute_filter` (bank 4 is *not* exempt — `+0x1AA707/+0x1AA70D`). `se_play_inner` gives
  the identical pan / handle-table / reaper / SE-volume integration and skips both the
  filter and the AVS lock. The cost is losing the lock, which the game only takes when
  `audio_lock_count > 0`, and we are on the game thread anyway **[INF — thread affinity of
  our call site not yet fixed]**.
- **Handle-table exhaustion leaks a cue.** `handle_slot_alloc` on failure (`+0x1AB611`)
  sets `EBX=-1`, calls `apply_pan_matrix` on the cue, and returns `-1` **without
  destroying it** (`+0x1AB611..+0x1AB624`). The prior note's claim that it "destroys the
  cue it was given" is **wrong** [OBS]. With 256 shared slots this only matters under
  pathological SE pressure, but an assist tick is the highest-rate SE producer in the game,
  so: rate-limit, and treat a `0xFFFFFFFF` return as a signal to back off.

### If the verdict had been unsafe

Fallback (not needed): keep our `IXACT2SoundBank*` entirely outside the manager, call
`SoundBank::GetCueIndex` (vtable `+0x00`) / `SoundBank::Play` (vtable `+0x20`) directly, and
run our own `GetState & 0x20 → Destroy` reaper mirroring `+0x1ABB30`. All four vtable
indices are game-exercised **[OBS]**. Cost: we hand-roll pan (`SetMatrixCoefficients` at Cue
`+0x40`, needs `mgr+0x8C/0x90/0x94/0x99/0x9A/0x20C0` semantics replicated), lose the SE
volume/mute category, and own the cue lifetime. Slot 4 buys all of that for free.

---

## Proposed signatures

Format and conventions follow `src/core/signatures.rs` (`?`/`??` = wildcard byte,
space-separated hex, one `SignatureDefinition` per logical target, derived addresses
computed in a `derive_*` method). Every byte that is an address, a RIP displacement, a
stack-frame displacement, or a branch displacement is wildcarded. Semantic immediates
(`0x04` bank-name count, `0x05` fallback slot, `0xFFFF` cue-index sentinel, vtable indices
`+0x20`) are deliberately **left literal** so that a semantic change breaks the match
instead of silently mis-resolving.

### S1 — `se_play_inner_body` (primary; yields `se_play_inner` **and** the `mgr` global)

```
48 8B 35 ?? ?? ?? ?? 48 63 F9 0F 29 74 24 ?? 48 8D 47 01 0F 28 F2 48 03 C0 48 8B 1C C6
48 85 DB 74 ?? 48 8B 03 48 8B CB FF 10 B9 FF FF 00 00 66 3B C1 74 ?? 4C 8B 13
48 8D 4C 24 ?? 45 33 C9 48 89 4C 24 ?? 45 33 C0 0F B7 D0 48 8B CB
48 C7 44 24 ?? 00 00 00 00 41 FF 52 20
```

Anatomy (offsets are from the **match**, which is `se_play_inner + 0x0F`):

| Off | Bytes | Meaning |
|---|---|---|
| `+0x00` | `48 8B 35 d32` | `MOV RSI,[rip+audio_manager]` → **`decode_rip_relative(match+3)` = the mgr global** |
| `+0x07` | `48 63 F9` | `MOVSXD RDI,ECX` — bank_id |
| `+0x11` | `48 8D 47 01` / `48 03 C0` / `48 8B 1C C6` | `bank = *(void**)(mgr + ((bank_id+1)*2)*8)` — **the 0x10 slot stride is inside the pattern**, so a struct-layout change fails the match rather than mis-indexing |
| `+0x22` | `FF 10` | `CALL [RAX]` = `SoundBank::GetCueIndex` (vtable `+0x00`) |
| `+0x24` | `B9 FF FF 00 00` / `66 3B C1` | the `0xFFFF` not-found sentinel |
| `+0x56` | `41 FF 52 20` | `CALL [R10+0x20]` = `SoundBank::Play` — **the discriminator**; the byte-identical sibling `se_prepare_inner` has `41 FF 52 18` |

Why the pattern must run all the way to `+0x56`: `se_prepare_inner` (`+0x1AB680`) and
`se_play_inner` (`+0x1AB7A0`) are **byte-for-byte identical for their first 0x65 bytes**
apart from the mgr RIP displacement (verified: both read as
`48895c24084889742410574883ec40488b35…` differing only in `ca765400` vs `aa755400`, both
resolving to `+0x6F2D60`). Truncating the pattern before the vtable index yields 2 matches.
[OBS]

Verified matches (`ghidra_search_byte_patterns`, **exactly one match per build**):

| Build | match | `se_play_inner` = match−0x0F | derived `mgr` global (RIP decode) |
|---|---|---|---|
| 20260721 | `+0x1AB7AF` | **`+0x1AB7A0`** | disp `0x005475AA` → **`+0x6F2D60`** |
| 20260616 | `+0x1AA74F` | **`+0x1AA740`** | disp `0x00547612` → **`+0x6F1D68`** |
| 20260421 | `+0x1A98EF` | **`+0x1A98E0`** | disp `0x0054458A` → **`+0x6EDE80`** |
| 20260324 | `+0x1A8C4F` | **`+0x1A8C40`** | disp `0x0053308A` → **`+0x6EBCE0`** |

(The mgr global moves in **every** build — hard-coding it is not an option.
Independent corroboration that each derived address is the audio manager: in each build it
sits between that build's `se_mute_filter` and `audio_lock_count` globals as read off the
corresponding `se_play` — 20260721 `6F2420 < 6F2D60 < 6F38F8`; 20260616
`6F1428 < 6F1D68 < 6F28A8`; 20260421 `6ED540 < 6EDE80 < 6EE9B0`. [OBS])

### S2 — `se_play` (optional; only if we want the mute-filter + lock wrapper)

```
40 57 48 83 EC 40 48 C7 44 24 ?? FE FF FF FF 48 89 5C 24 ?? 0F 29 74 24 ?? 0F 28 F2
48 8B FA 8B D9 44 8B C1 41 FF C8 74 ?? 41 83 F8 04 74 ?? C7 44 24 ?? 05 00 00 00
48 8D 4C 24 ?? FF 15
```

Match = **function entry**. The distinctive core is `41 FF C8` / `74 ??` / `41 83 F8 04` /
`74 ??` — the "bank_id ∈ {1,5} skips the mute filter" ladder — followed by the literal
mute-filter state `5`. The pattern ends at the `FF 15` opcode so the following disp32
(`se_mute_filter`) is excluded and can be derived: `decode_rip_relative(match + len)`.

Verified matches (**exactly one per build**):

| Build | `se_play` | first `CALL rel32` in body | target |
|---|---|---|---|
| 20260721 | **`+0x1AA6E0`** | `+0x1AA753` (entry+0x73) | `+0x1AB7A0` ✓ = S1 |
| 20260616 | **`+0x1A9650`** | `+0x1A96C3` (entry+0x73) | `+0x1AA740` ✓ = S1 |
| 20260421 | **`+0x1A8820`** | `+0x1A8893` (entry+0x73) | `+0x1A98E0` ✓ = S1 |
| 20260324 | **`+0x1A7B50`** | (not disassembled) | — |

**S1 ⟷ S2 mutual cross-check (recommended, mirrors `derive_app_heap_handle`'s CALL-target
assertion):** `scan_first_call_rel32(se_play, 0x80)` must equal S1's derived
`se_play_inner`. If they disagree, log a warning and refuse to install. I verified there is
no stray `0xE8` byte before the real call in the `se_play` bodies of 20260721 / 20260616 /
20260421 (all intervening calls are `FF 15` indirect and none of the RIP/branch
displacements contains `E8`) **[OBS]**, so `scan_first_call_rel32` is sound on those three;
the assertion makes it self-policing on future builds.

### S3 — `bank_slot_of_file_loop` (the slot-4 safety gate — **required by guard G1**)

```
4C 8B 0E 48 83 C9 FF 33 C0 49 8B F9 48 8B D5 F2 AE 48 F7 D1 4C 8D 41 FF 49 8B C9
E8 ?? ?? ?? ?? 85 C0 74 ?? FF C3 48 83 C6 10 83 FB ?? 72 ?? B8 05 00 00 00
```

| Off | Meaning |
|---|---|
| `+0x1B` | `E8 rel32` → `strncmp` (wildcarded; derivable as a sanity check) |
| `+0x2C` | **imm8 = number of named banks. MUST read `0x04`.** If it reads `0x05`, a build has added a fifth named bank and slot 4 is no longer free — refuse to use it. |
| `+0x30` | imm32 = fallback slot (`5`) — kept literal, so a change breaks the match |

Verified (**exactly one match per build**, imm8 = `04` and fallback = `5` in **all four**):

| Build | match | `+0x2C` | `+0x30` |
|---|---|---|---|
| 20260721 | `+0x1AA440` | `04` | `05 00 00 00` |
| 20260616 | `+0x1A93B0` | `04` | `05 00 00 00` |
| 20260421 | `+0x1A8580` | `04` | `05 00 00 00` |
| 20260324 | `+0x1A78B0` | `04` | `05 00 00 00` |

### S4 — `audio_string_pool` (pure **content** anchor; corroborates S3, does not yield `mgr`)

```
62 67 6D 5F 6D 65 6E 75 00 ?? ?? ?? ?? ?? ?? ?? 73 65 5F 73 79 73 74 65 6D 00 ?? ?? ?? ?? ?? ??
73 65 5F 6E 6F 72 6D 61 6C 00 ?? ?? 76 6F 69 63 65 00 ?? ?? ?? ?? ?? ??
64 61 74 61 2F 73 6F 75 6E 64 2F 77 69 6E 2F 64 64 72 2E 78 67 73 00
```

i.e. `"bgm_menu\0" "se_system\0" "se_normal\0" "voice\0" "data/sound/win/ddr.xgs\0"` as one
contiguous, zero-padded `.rdata` blob (padding wildcarded). Verified layout **[OBS]**:

| Build | pool | contents |
|---|---|---|
| 20260721 | `+0x3800D0` | `+0x3800D0 bgm_menu`, `+0x3800E0 se_system`, `+0x3800F0 se_normal`, `+0x3800FC voice`, `+0x380108 data/sound/win/ddr.xgs`, `+0x380120 "xsb"`, `+0x380124 "Global"` |
| 20260616 | `+0x37F0D0` | identical layout (single unique match for the composite pattern) |

`"data/sound/win/ddr.xgs\0"` alone is also unique per build (`+0x380108` / `+0x37F108`).
**Only `bank_slot_of_file` references the pool head** — `ghidra_get_xrefs_to(+0x3800D0)`
returns 5 sites, all inside `FUN_1801AA3C0`. [OBS]

---

## Derivation chains

### Chain A (recommended) — one AOB, two addresses

```
scan_pattern(S1 "se_play_inner_body")              -> core::scanner::scan_pattern / scan_patterns_batch
  match
  ├─ decode_rip_relative(match + 3)                -> core::scanner::decode_rip_relative
  │     = audio_manager global  (+0x6F2D60 on 20260721)
  └─ se_play_inner = match - 0x0F                  -> verify prologue bytes, or
                                                      core::scanner::find_function_entry(match, base)
```
Verify at `match-0x0F` the 14-byte prologue
`48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 40` (identical on all four builds **[OBS]**)
before accepting the entry; if it does not match, fall back to `find_function_entry`
(the preceding function ends in `C3` + `CC` padding, which is what that helper looks for).

### Chain B — `se_play` and its cross-check

```
scan_pattern(S2 "se_play")                         -> scan_pattern
  match = se_play
  ├─ se_mute_filter = decode_rip_relative(match + <pattern_len>)   -> decode_rip_relative
  └─ assert scan_first_call_rel32(match, 0x80) == ChainA.se_play_inner
                                                   -> core::scanner::scan_first_call_rel32
```

### Chain C — the slot-4 gate

```
scan_pattern(S3 "bank_slot_of_file_loop")          -> scan_pattern
  match
  ├─ named_bank_count = *(u8*)(match + 0x2C)       ; MUST == 4, else do not claim slot 4
  ├─ fallback_slot    = *(u32*)(match + 0x30)      ; == 5 (guaranteed by the literal run)
  └─ bank_slot_of_file = find_function_entry(match, base)   (optional; not needed at runtime)
```

### Chain D (fallback / independent corroboration of the `mgr` global)

Only needed if S1 ever stops matching. Two content-anchored routes, both implementable with
existing primitives:

```
D1:  scan_pattern(S4 pool ASCII)                   -> scan_pattern            (ASCII-as-AOB;
     scan_lea_xrefs_to(base, size, pool)           -> core::scanner::scan_lea_xrefs_to   works today,
     find_function_entry(lea_site, base)           -> find_function_entry      no new primitive)
       = bank_slot_of_file        [1 function, 5 LEA/READ sites, all in it]

D2:  xsb = pool + 0x50            ("xsb" literal, +0x380120 on 20260721)
     scan_lea_xrefs_to(base, size, xsb)            -> 6 sites (20260721): +0x1AB40E (sound_file_unregister),
                                                      +0x1AA574 (sound_file_register), +0x1AAEEB, +0x1AC57A/+0x1AC589,
                                                      +0x1A9FF0  -- needs disambiguation
     for each site: e = find_function_entry(site, base); scan e..e+0x40 for `48 8B 35|1D|0D d32`
     mgr = decode_rip_relative(that + 3)
```
D2 is weaker (6 candidate sites, manual disambiguation) and is documented only as a
last resort. `sound_file_unregister`'s mgr load is at entry`+0x16` (`+0x1AB3E6`) **[OBS]**.

**Deliberately not proposed:** a `se_game_shockarrow`-string chain. That string
(`+0x360D28`, LEA'd at `+0x5F290`) leads to `judgeNotes`' **inlined** copy of
`se_play_inner`, which yields the mgr global but *not* a callable `se_play` — the wrapper
is not reachable from it. [OBS]

### Wildcard rationale (per S1/S2/S3 byte)

| Wildcarded | Why |
|---|---|
| `48 8B 35 ?? ?? ?? ??` (S1+0x03) | RIP disp32 to the mgr global — **moves in every build** (`0x5475AA` / `0x547612` / `0x54458A` / `0x53308A`) |
| `0F 29 74 24 ??`, `48 8D 4C 24 ??`, `48 89 4C 24 ??`, `48 C7 44 24 ??`, `48 C7 44 24 ?? FE…`, `48 89 5C 24 ??` | stack-frame displacements — compiler frame-layout choices; identical today but the cheapest thing to lose |
| `74 ??`, `72 ??` | rel8 branch displacements — shift with any body edit |
| `E8 ?? ?? ?? ??` (S3+0x1B) | `strncmp` rel32 |
| `FF 15` truncation (S2 tail) | the following disp32 is `se_mute_filter`'s address |
| `83 FB ??` (S3+0x2A) | the immediate is the *value we want to read*, not match on |
| **kept literal on purpose** | `41 FF 52 20` (Play vs Prepare vtable index), `B9 FF FF 00 00` (0xFFFF sentinel), `41 83 F8 04` (bank-5 exempt test), `B8 05 00 00 00` (fallback slot 5), `48 8D 47 01`/`48 03 C0`/`48 8B 1C C6` (the 0x10 slot stride) |

---

## Risks and what I could not determine

**R-1 (the only real cross-version risk) — a future build adds a fifth *named* bank.**
If `bank_slot_of_file` ever grows a 5th entry (`… "voice", "se_event"` → slot 4), then
`soundbank_create` would find `bank[4] != NULL` (ours) and **silently skip loading the
game's bank** (`+0x1AAFA0`'s admission guard returns 0 with no log) — that SE bank goes
mute for the whole session. Severity: user-visible, silent.
**Mitigations, both implemented as guards:** S3 reads the loop bound at runtime (must be
`4`); G1 computes the free slot from the live `{fid,bank}` pair rather than hard-coding 4.
With both, the failure mode degrades to "assist tick declines to install".

**R-2 — `se_mute_filter` (`+0x6F2420`) policy is still unknown.** Bank 4 is *not* exempt
(only 1 and 5 are, `+0x1AA707/+0x1AA70D` **[OBS]**), and `apply_pan_matrix` calls it a
second time when `mgr+0x98 != 0`. If it vetoes during gameplay we silently lose ticks.
Evidence it does not: `judgeNotes` plays `se_game_shockarrow` through the same gate on
bank 2 **[OBS]**. Sidestep entirely by calling `se_play_inner` directly. **[INF]** — carried
over unresolved from `game-sound-engine.md` open question #5.

**R-3 — cue leak on handle-table exhaustion.** `handle_slot_alloc` returns `-1` without
destroying the cue (`+0x1AB611..+0x1AB624` **[OBS]**). Rate-limit the tick and treat
`0xFFFFFFFF` as back-pressure.

**R-4 — the global is dereferenced unchecked.** `+0x1AB7AF` / `+0x1AA6E0`'s callee path.
Guard G2. **[OBS]**

**R-5 — thread affinity.** The wrapper takes an AVS lock when `audio_lock_count > 0`, so it
is probably thread-safe, but every observed caller is on the game thread. Our tick site
must stay on the game thread. **[INF]** — unchanged from the prior note.

**R-6 — file-manager id domain.** The "no valid `file_id` is ever `-1`" step in the R1
argument rests on `sound_file_unregister` using its argument as a record-table index
(`+0x1AB3F0`) and on the callback being invoked per owned record. I did **not** read the
file manager's id allocator. **[INF]** — cheap to close by logging the argument at
`+0x1AC6C0` for a session, or by reading `FUN_1801FEF30`'s allocator.

**Not determined / out of scope:**

- Whether all 49 `se_play_helper` call sites pass literal bank ids ≤ 5 (sampled only).
  Impact if one passes 4: none, unless the cue name collides with ours — so **do not name
  our cue `se_*` or `vo_*`**. **[INF]**
- The XACT2 engine vtable beyond `+0x58` (`PrepareWave` / `PrepareInMemoryWave`) — unchanged
  from `game-sound-engine.md` open question #1. Not needed for this design: slot 4 only
  requires `CreateInMemoryWaveBank` (`+0x50`), `CreateSoundBank` (`+0x48`),
  `GetCueIndex` (`+0x00`) and `Play` (`+0x20`), all game-exercised.
- `mgr+0x88`'s exact role (a divisor applied to both volume floats;
  `+0x1AC02A MOVSS XMM11,[RDI+0x88]`, clamped against `+0x35A420` at `+0x1AC03B`).
  Initialised to `1.0f` by the ctor (`+0x1AABC6`) and, as far as I can tell, never written
  again — so it is a no-op today. **[INF]**
- Whether the tiny functions `+0x1AAAA0` / `+0x1AAAE0` / `+0x1AAB20` (writers of
  `mgr+0x98` / `+0x99` / `+0x9A`) are called during gameplay — i.e. whether SE mute can
  engage mid-song and silence our tick. **[INF]**, not chased.

**Side effect to disclose:** while investigating I invoked
`ghidra_disassemble_bytes(+0x1AC030, 96)` on `gamemdx_20260721.dll` before realising it is a
mutating tool. The region was already disassembled code inside `apply_pan_matrix`, so the
call was a no-op; no other Ghidra-side writes were made. No files in the repo were modified
other than this document.

---

## Corrections to `game-sound-engine.md`

Three claims in the companion note are wrong or imprecise; carried here so the next reader
does not inherit them. All **[OBS]**.

| Prior claim | Correction | Evidence |
|---|---|---|
| "final-mix `nChannels` stored into `mgr+0x2118`" | It is **`mgr+0x20C0`** | ctor `+0x1AAE8D MOV [RSI+0x20c0],ECX`, preset to 2 at `+0x1AAC1A`; consumed at `+0x1AC065 MOV EBX,[RDI+0x20c0]` |
| "`+0x8C, +0x90` per-category volumes; `+0x94` master" (both [INF]) | **Confirmed [OBS]**, and there are exactly **two** categories (0,1); `+0x88` is an additional divisor/normaliser | `se_set_volume` inner `+0x1AB930` writes `mgr+0x8c + category*4`; its only caller `FUN_1800081A0` passes `param_3 != 0`; ctor `+0x1AAE93` writes `1.0f,1.0f` to `+0x8C/+0x90` |
| "handle-slot exhaustion: `+0x1AB5B0` **destroys the cue it was given** and returns -1. No leak" | **Wrong — the cue is leaked.** On exhaustion it calls `apply_pan_matrix` on the cue and returns `-1`; no `Destroy` | `+0x1AB608 CMP R10D,0x100` → `+0x1AB611 OR EBX,-1` → `+0x1AB61F CALL apply_pan_matrix` → `+0x1AB624 MOV EAX,EBX ; RET` |

Additionally confirmed (previously **[INF]**, now **[OBS]**): handle-slot table is
`mgr+0xA0 + i*0x20`, 256 entries (`memset(mgr+0xA0, 0, 0x2000)` at `+0x1AABF7`), with
`[0]=cue`, `[+8]=wave`, `[+0x10]=prepared`, `[+0x11]=deferred`, `[+0x14]=pan`,
`[+0x18]=bank_id`, and a free slot requires **both** `cue == NULL` and `wave == NULL`
(`+0x1AB5EF`, `+0x1AB5FA`); round-robin cursor at `mgr+0x20E8`, masked `& 0xFF`
(`+0x1AB5E8`).
