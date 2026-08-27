# Quick Logout — Forced End-of-Session Research

RE notes for a mod that ends the current play session on demand ("quick logout"):
jump straight to the **TOTAL RESULTS** summary, run the **e-amusement logout save**
(profile + customize write-back), show **THANK YOU FOR PLAYING**, and return to
attract — without playing out the remaining stages.
Motivating case: Premium Free freezes the stage counter, so disabling it mid-session
leaves the counter far below the operator's max-stage value and the session will not
end for several more songs.

**Primary binary:** `gamemdx_20260721.dll` (Ghidra program `gamemdx_20260721.dll`).
All addresses **file-relative to base `0x180000000`** unless noted.

**Cross-version status:** the two new AOB anchors this feature needs were verified
unique on **20260324, 20260616 and 20260721** (address table in §11). Everything else
rides on signatures the modpack already resolves version-agnostically
(`scene_transition`, `advance_to_scene`, `stage_record_accessor`,
`premium_free_stage_inc`).

**Tools:** Ghidra static analysis. **Implemented and cabinet-validated 2026-07-28** —
the feature shipped as the `quick-logout` mod (Mechanism A) plus the logout-save
sanitisation policy; see §13 for the validation outcomes and the deviations from this
document's recommendations.

**Related docs:** [`scene_manager_research.md`](./scene_manager_research.md),
[`premium_free_stale_record_bug.md`](./premium_free_stale_record_bug.md),
[`input_system_research.md`](./input_system_research.md),
[`ddr_world_scene_ids.md`](./ddr_world_scene_ids.md).

---

## 1. TL;DR — what it takes
The end-of-session tail is a plain scene chain that the game drives with one
primitive, `agcs::Sequence::finish(this, nextSceneId)`. Nothing in that tail checks
"did the session end legitimately" — not the stage counter, not the max-stage
setting, not whether a per-stage save ran. The logout save is performed by
`EAmExitRootSequence` (0-indexed scene **34**), which expires the credit itself and
is entirely self-contained.
So a forced logout is: **call `finish()` on the currently-running child sequence with
the right scene id, from the render thread.** Three viable variants:
| # | Mechanism | Gets summary? | Risk | Verdict |
|---|---|---|---|---|
| **A** | `finish(child, 30)` (→ 0-idx 29 loader) + one-shot scene redirect `30 → 32` | **yes** | medium — needs the resource-load hop, emits one spurious POSEVT | **recommended primary** |
| **B** | `finish(child, 34)` (→ 0-idx 33 loader) | no | low — no resource dependency, no redirect | ship first as the proving milestone |
| **C** | write `GameWork + finalStageOverride = GameWork + stageCounter` | yes (natural) | **none** — 100 % vanilla code paths | ship alongside as "make this my last song".

The one hard constraint: **`TotalResultSequence` (scene 32) dereferences its BM2D package pointer without a null check**, and that package (`scene_result`) is *not* resident on the song-select screen. Jumping directly from song select to scene 32 crashes. Mechanism A exists purely to route through the loader that makes it resident. See §6.

---

## 2. What "logout" is in this binary
Three separable things happen at the end of a session. Only the second is the
"logout" proper:
1. **TOTAL RESULTS** — `sequence::result::TotalResultSequence`, 0-indexed scene **32**.
   The per-session summary (one row per stage record).
2. **The logout save** — `sequence::entry::EAmExitRootSequence`, 0-indexed scene **34**.
   Expires the credit/PASELI session, then runs a per-side
   `SavePlayerDataActor(side, stage = -1)` → `ark::network::PlayerDataSaveLogoutRequest`
   → `ReflectSavePlayerData(side, savekind = 3)`. This is the save that carries the
   profile/customize write-back (and the one the modpack's `custom_options_persistence`
   calls `SAVEKIND_LOGOUT`).
3. **THANK YOU FOR PLAYING** — `sequence::GameOverSequence`, 0-indexed scene **35**.
   Acks the ark entry flow, fires `arkCoreEventLogRequestGameEnd()`, returns to attract.
Note on naming: `GameOverSequence` in this binary is *not* mid-song failure — it is
the session-end "thank you" screen. Mid-song failure is `GamePlayActor::STEP_GAME_OVER`
(the existing `quick_restart_or_fail` mod's territory) and is unrelated.
---
## 3. The scene chain
`TransitionSequence::createNextSequence(this, sceneId)` = **`FUN_18002e240`** is a
~58-case switch that constructs one sequence class per scene. It begins with
`LEA EDI,[RDX-1]`, so **case index == sceneId − 1 == the 0-indexed scene id** the
modpack uses. Jump table at `0x180030FBC`; bounds check `CMP EDI,0x3A` @ `0x18002E331`.
### 3.1 Scenes relevant to a logout
| 0-idx | 1-idx | Class | ctor | Notes |
|---|---|---|---|---|
| 19 | 0x14 | `entry::EAmEntryRootSequence` | `0x180084A90` | login; vtable `0x180364218`, onSetup `0x180084C10`, onUpdate `0x1800857A0` |
| 25 | 0x1A | `selectmusic::SelectMusicSequence` | `0x1800FBE90` | vtable `0x18036E5F8`, update `0x1800FC100`, leave `0x1800FC580`, dtor `0x1800FBF60` |
| 26 | 0x1B | `selectmusic::SelectMusicTerminateSequence` | `0x180112870` (arg 0) | the decide transition |
| 27 | 0x1C | `LoadingSequence` | `0x18002D130` | load `0x8000`, unload `0x32000`, kind 0 |
| 28 | 0x1D | `dance::DancePlaySequence` | `0x180057150` | update `0x180057EC0` |
| **29** | **0x1E** | `LoadingSequence` | `0x18002D130` | **load `0x10000` (`scene_result`)** / `0x30000`, unload `0xF000`, kind 3/4/7. Mask imm at `0x18002FEB5`. Emits POSEVT `"playmusic"` @ `0x18002FECD` |
| **30** | **0x1F** | `result::ResultSequence` | `0x1800B6400` | vtable `0x1803690A8`, update `0x1800BBC30`, setup `0x1800B7270` |
| 31 | 0x20 | `WaitSequence` | `0x18002D060` | **`INC dword [GameWork+0xC]`** @ `0x180030368` — the stage bump (this is the instruction Premium Free NOPs) |
| **32** | **0x21** | `result::TotalResultSequence` | `0x1800C9680` | vtable `0x18036A1F8`, update `0x1800C9A90`, row builder `0x1800CB090`, teardown `0x1800CAFD0` |
| **33** | **0x22** | `LoadingSequence` | `0x18002D130` | **load `0x40000` (`scene_game_over` + `scene_eamusement_window`)**, unload `0x3F000`, kind 1 |
| **34** | **0x23** | `entry::EAmExitRootSequence` | `0x1800A41B0` | **the logout save.** vtable `0x180366738`, onSetup `0x1800A4330`, onUpdate `0x1800A47F0` |
| **35** | **0x24** | `GameOverSequence` | `0x1800B3830` | THANK YOU. onUpdate `0x1800B3C30` |
| 36 | 0x25 | `LoadingSequence` | `0x18002D130` | `(0, 0, kind 0)` |
The matching/battle chain mirrors this at 0-idx 47–57 (`0x30`–`0x3A`), and the
event/special chain (`GameWork+0xD0 ∈ {1,2}`) uses 0-idx 56 (`0x39`) for total
results instead of 32.
### 3.2 `getNextID` — the automatic tail
`TransitionSequence::getNextID()` = **`FUN_18002DD70`**, a switch on the **1-indexed**
`m_currentID` at `TS+0x68`, returning the next **1-indexed** id.
| current (1-idx) | → next | meaning |
|---|---|---|
| 0x1A (song select) | 0x1B | → terminate/decide |
| 0x1E (post-song loader) | 0x1F | → ResultSequence |
| 0x1F (ResultSequence) | 0x20 | → Wait (stage bump) |
| 0x20 | 0x19 | → back to song select |
| **0x21** (TotalResults) | **0x22** | → the gameover-group loader |
| **0x22** | **`arkEamOff() ? 0x24 : 0x23`** | → EAmExitRoot (**e-am on**) or straight to THANK YOU |
| 0x23 (EAmExitRoot) | 0x24 | → THANK YOU |
| 0x24 (THANK YOU) | 0x25 | → final loader |
| 0x25 | 0x07 | → attract loop |
`arkEamOff()` = **`FUN_18001B870`**: reads `/networkOptions/e_amusement/fixed` +
`/current` and `arkGetNetworkStatus`; returns **true when e-amusement is off/offline**.
With a live backend it returns false → `0x23` → **scene 34 runs → logout save
happens.** Good.
**Nothing in `getNextID` ever returns `0x21`.** The entry into TOTAL RESULTS is always
an explicit `finish(this, 0x21)` from `ResultSequence` — see §5.1. That is the
mechanism this feature copies.
---
## 4. How transitions are actually driven
### 4.1 `agcs::Sequence::finish` — the one primitive
**`FUN_18021DF70(this, nextSceneId1Indexed)`**:
```c
parent = *(void**)(this + 8);
if (!(parent->flags & 0x20) && parent->vt[0x18](parent, 0x201, nextSceneId) == 0)
    /* broadcast 0x201 down parent's children */;
// then mark this subtree dead:
for each child c of this: mark c's children, c->flags |= 4, ancestors |= 8
this->flags |= 4; ancestors |= 8;
```
`agcs::Sequence::onMessage` (**`FUN_18021E070`**, slot `+0x18` of
`agcs::Sequence::vftable` @ `0x180389D08`) handles `0x201`:
```c
if (msg == 0x201) { this[0xB] = 0; this->vt[0x48](this, (int)param); return 1; }
```
`TransitionSequence` overrides slot `+0x48` with **`advanceToScene`** =
`FUN_18002DC10` (TS vtable base `0x18035D5B8`):
```c
if (id == 0) id = getNextID();
seq = createNextSequence(this, id);
if (seq) installChild(this, seq);      // FUN_18021DEF0
*(int*)(this + 0x68) = id;
FUN_1801BB6F0(id);
```
`id == 0` means "ask `getNextID`". A non-zero id is an explicit destination. That's the
whole steering surface.
### 4.2 Object layout (verified)
`agcs::Actor::vftable` = `0x180389CA8`, `agcs::Sequence::vftable` = `0x180389D08`.
| off | field |
|---|---|
| `+0x00` | vtable |
| `+0x08` | parent |
| `+0x10` | next sibling |
| `+0x18` | first child |
| `+0x20` | tree flags (see below) |
| `+0x24` / `+0x28` | applied / desired sort priority |
| `+0x2C` | inline `char[]` class name |
| `+0x50` | lifecycle flags (1 = update, 2 = draw, 4 = skip-next-update, 0x100 = entered) |
| `+0x58` | **`Sequence`: current gosub child** — written only by `FUN_18021DEF0` |
| `+0x60` | bool "inside my own update" |
| `+0x68` | **`TransitionSequence` only: current 1-indexed scene id** (`m_currentID`) |
Tree flags at `+0x20`: `0x01`/`0x02` sort-dirty, **`0x04` = this node flagged for
destruction**, **`0x08` = a descendant is flagged** (propagates up), `0x10` = do not
`delete` (externally owned), `0x20` = destruction in progress → all message dispatch
suppressed. The composite test `flags & 0x24` is "dead or dying"; a flagged actor
immediately stops updating and drawing.
`TS + 0x58` is confirmed as the active gosub child (`FUN_18021DEF0` last line). The
modpack's `quick_restart_or_fail` already relies on this offset.
### 4.3 Safety of calling `finish` from a hook
The reaper is **`FUN_18022EBE0`**, called once per frame from the main loop
`FUN_180003020` at **`0x180003147`** — between the `0x102` (update) broadcast and the
`0x103` (draw) broadcast.
- **Nothing is freed inside `finish`.** It sends one message and sets bits. Calling it
  from the modpack's render-thread input poll (`widget_renderer::wrapper_render_hook`
  → `input_manager::poll`) is safe; worst case the corpse survives one extra frame,
  during which it neither updates nor draws.
- **Same thread is mandatory** — there is no locking anywhere in the tree code. The
  modpack's input poll already runs on the frame thread. ✔
- **Double-fire is memory-safe but wrong.** The flag half is idempotent
  (`if ((flags & 0x24) == 0)`), but the *message* half is not guarded: a second call
  produces a second `advanceToScene` → two live `TotalResultSequence` siblings. Must be
  latched. The cheapest natural latch: `advanceToScene` writes `TS+0x68`
  **synchronously**, so `if (scene_manager::current_scene() != SONG_SELECT) return;`
  rejects every subsequent press for free. Belt-and-braces: also test
  `*(u32*)(child + 0x20) & 0x24`.
- The incoming sequence is constructed **before** the outgoing one's `leave()` runs, and
  `FUN_18021DEF0` **appends** rather than replaces. That is identical to the game's own
  decide path — no new hazard.
- Anything the modpack parents under `TS+0x58` would also be marked and deleted by
  `finish` (the descendant-marking recursion). Not currently an issue.
### 4.4 `SelectMusicSequence` tears down cleanly from any state
`leave()` = **`FUN_1800FC580`** (vtable slot `+0x28`), invoked by the reaper's `0x104`
broadcast regardless of which state the sequence was in:
```c
if (this+0xB0) { FUN_1800FD930(model); free(model); }   // 0x400-byte song-select model
if (this+0xB8) { obj->vt[8](obj, 1); }                  // 0x4A0-byte object
```
`FUN_1800FD930` clears the global `DAT_1806F2D50` ("current song-select model", which
other code null-checks), releases two ref-counted resource handles, and frees its
vectors. There is **no** `onSetup` (slot `+0x20` is a stub) and **no** per-state
cleanup — setup is lazy behind a bool at `+0x68`, and if it never ran, `+0xB0`/`+0xB8`
are null and `leave` no-ops. The `0xF0`-byte clock actor at `+0xC0` is a tree child and
dies via the descendant marking.
**Conclusion: `finish()` on `SelectMusicSequence` from an arbitrary moment is clean.**
Its own two `finish` calls, for reference — both registered as model event handlers in
`FUN_1800FC100`:
| site | call | resolves to |
|---|---|---|
| `0x1800FC4EB` | `finish(this, 0)` | `getNextID(0x1A) = 0x1B` → the decide path |
| `0x1800FC53C` | `finish(this, 0x19)` | fade/wait → `getNextID(0x19) = 0x1A` → back into song select |
**There is no existing "abandon the session from song select" path.** All 37 call sites
of `FUN_18021DF70` were enumerated; the only two in the selectmusic module are the
above. The song-select timeout is *not* an abandon — it routes through model event 0
into the normal decide flow.
---
## 5. The end-of-session tail in detail
### 5.1 Where `finish(0x21)` comes from — `ResultSequence::update` state 30
`ResultSequence::update` = `FUN_1800BBC30`; internal state at
`*(int*)(this + 0x68 + 8*this->0x92)`; dispatch table `0x1800BEC18`. State 30 (`0x1E`)
is at `0x1800BE940`:
```
1800BE96D  CMP byte [GameWork+0x59],0     ; extra stage granted?
1800BE973  MOV ECX,0x2C                   ;   yes -> extra-stage chain
...
1800BE993  CMP dword [RDI+0xEC],1         ; event mode && stage >= 1 ...
1800BE99C  MOV byte [RDI+0xE8],0          ;   ... force "no continue"
1800BE9A3  CMP byte [RDI+0xE8],0          ; <== THE GUARD
1800BE9AA  JZ  1800BE9D6                  ;   == 0 -> SESSION OVER
           (else) step++  -> state 31 (keep playing)
1800BE9EF  MOV ECX,0x21                   ; TotalResultSequence (normal)
1800BE9F4  MOV EDX,0x39                   ; TotalResultSequence (event mode)
1800BE9FB  CMOVNZ ECX,EDX
1800BEA03  CALL 0x18021DF70               ; finish(this, 0x21)
```
State 31 (`0x1800BEA0D`) is the twin: `finish(this, 0x20)` → the Wait sequence that
bumps the stage counter → song select.
So **"session over" ≡ `ResultSequence + 0xE8 == 0`**.
### 5.2 How `+0xE8` is computed, and why Premium Free never ends a session
Seeded in the ctor `FUN_1800B6400`: `+0xE8 = 0` (default: no continuation),
`+0xE9 = (GameWork+0x70 != 0)` (course), `+0xEC = GameWork+0xC` (0-based stage
snapshot), `+0xF0 = GameWork+4`, `+0xF4 = GameWork+8`.
Decided in `FUN_1800B7270` (ResultSequence setup; writes at `0x1800B7764`,
`0x1800B7A2E`, `0x1800B7A73`, `0x1800B7A9E`, `0x1800B7AD5`, `0x1800B7B2C`):
```c
if (isCourse)                                              { +0xE8 = 0; goto end; }
stage = +0xEC;  gw = GameWork;
if (!course) {
  if (!event && DAT_18047E784 + 1 <  stage)                { +0xE8 = 0; goto end; }
  if (!event && stage == gw->0x10)                         { +0xE8 = 0; goto end; }   // (*)
  if (!event && stage != gw->0x10 && stage == DAT_18047E784+1) { +0xE8 = 0; goto end; }
}
if (FUN_1801DD550(stage))          // is last NORMAL stage?
    +0xE8 = <extra stage granted?>;
```
Key facts:
- **`DAT_18047E784` = the operator setting `/gameOptions/max_stage/current`.** Read via
  AVS at `0x18002E911` (`LEA RCX,["/gameOptions/max_stage/current"]`, out-pointer at
  `0x18002E903`), immediately after `GameWork::reset`. Current image value = **2**, so
  the normal stage count is `DAT_18047E784 + 1` = 3, and stage index
  `DAT_18047E784 + 1` (= 3, the 4th) is the EXTRA STAGE.
- **`GameWork + 0x10` is a final-stage *override*, and in this build nothing ever
  writes it except the reset-to-`-1`.** The sole writer is `GameWork::reset`
  (`FUN_1801DCAB0`) at `0x1801DCAF4`: `MOV dword [RAX+0x10], 0xFFFFFFFF`. Verified by a
  register-tracked scan of all 262 functions that reference `DAT_1806F14F8` — no
  `LEA [gw+0x10]` (so no AVS out-param write), no indexed writes, no aliasing spills of
  the GameWork pointer. **This is the clean, game-supported lever** — line `(*)` above.
- Premium Free NOPs the counter INC at `0x180030368`, so `GameWork+0xC` never advances
  and none of the "last stage" comparisons ever come true → `+0xE8` stays 0 but state 30
  is never reached with a stage index that ends the session. Once the freeze is removed,
  the counter resumes from its frozen value and the session runs to
  `DAT_18047E784 + 1` from there. That is exactly the user-visible problem.
The predicate family (all tiny leaves in the `0x1801DD5xx` block):
| fn | semantics | compare |
|---|---|---|
| `FUN_1801DD550(stage)` | is `stage` the last **normal** stage | **1-based**: `stage+1 == gw->0x10` or `stage+1 == DAT_18047E784+1` |
| `FUN_1801DD620()` | is the **current** stage the extra stage | **0-based**: `gw->0xC == gw->0x10 \|\| gw->0xC == DAT_18047E784+1` |
| `FUN_1801DD660()` | pure override test | **0-based**: `gw->0xC == gw->0x10` |
| `FUN_1801DD0B0(x)` | extra-stage granter | requires `gw->0x59==0 && x==0 && !course && gw->4 != 1 && DAT_18047E784 == 2`; sets `gw->0x59=1, gw->0x5A=0` |
⚠️ **`GameWork+0x10` is compared against both `stage` (0-based) and `stage+1`.** For
Mechanism C, write the **0-based** counter value (`gw->0x10 = gw->0xC`) — that trips
`FUN_1800B7270`'s early-out `(*)` and forces `+0xE8 = 0`. Writing `counter+1` instead
would make `FUN_1801DD550` report "last normal stage" and could *grant an extra stage*
— the opposite of what we want.
### 5.3 The logout save chain (scene 34)
`EAmExitRootSequence::onSetup` = **`FUN_1800A4330`**:
1. **Credit / PASELI settle-up**, per side, gated only on
   `PlayerWork+0x4 != 0` (side entered) and `PlayerWork+0x8 >= 0`:
   `PlayerWork+0x8 < 3` → `arkExpireCredit(side)` (`DAT_1806F2708`);
   `== 3` → `arkEACoinExpire(side)` (`DAT_1806F2AB0`).
2. Creates **two** `entry::EAmEntryWindowActor` (ctor `FUN_180086230`, 0x2B0 bytes),
   one per side, into the vector at `+0x98..0xA8`.
3. Builds the `main_root` layer from the always-resident common package
   (`*(*DAT_1806F2D68 + 0x30)`) and loads the bitmap **`coop_ope_logout`** into
   `operation_usr/text_usr` — i.e. this is literally the LOGOUT screen.
4. **No session-state gate of any kind** — no stage counter, no max stage, no
   prior-save requirement.
`EAmEntryWindowActor::update` = **`FUN_180088210`**. It polls
`arkEntryFlowGetCurrentScene(side)` (`DAT_1806F2AE8`) and dispatches through a
68-record handler table of `{code* handler; int val; const char* bitmap}` (0x18 stride)
indexed by the **ark entry-flow scene id**. Record `0x1B` = **`FUN_180094A30`**, self-
identified by its log string
`"sequence::entry::EAmEntryWindowActor::update_ARK_ENTRYFLOW_GAMEMODE"`. Its sub-state
machine:
| sub-state | action | gate |
|---|---|---|
| 0 | → 0x2D | none |
| 0x2D | walk the played-music vector `PlayerWork+0x1598..0x15A0`; then `PlayerWork+5 == 0 ? →4 : openWindow →6` | an empty music vector just skips the loop — **not** a gate |
| 6 | wait for window anim → **4** | animation only |
| **4** | `alloc(0x90)`; **`FUN_1800B4C80(obj, side, 0xFFFFFFFF)`** = `SavePlayerDataActor(side, stage = -1)` → 1 | **none** |
| 1 | wait on the `0x1014` reply flags `this+0x2AC` (done) / `+0x2AD` (ok) | save completion |
| 0x29→0x2A→0x2B→0x2C | result UI, SE, `arkEACoinQuerySessionState` wait, then `arkEntryFlowSetSceneResult` | PASELI session close |
`SavePlayerDataActor::SavePlayerDataActor` = `FUN_1800B4C80(this, playside, stage)`;
names itself `"SavePlayerDataActor:%dP GameEnd"` when `stage < 0`, else
`"...Stage%d"`. Its `onUpdate` = **`FUN_1800B4E30`**: with `stage < 0` **all per-stage
gating is bypassed** (`if (-1 < stage) { ...PlayerWork+0x1C, FUN_1801DD800(2/0x25),
rec+0x1A4... } else goto LAB_1800B4F46`); case 1 is only a per-side stagger delay
`(side+1) * DAT_18035B0AC`; case 2 needs only `arkIsNetworkFree` + a log gate; then:
```
FUN_18001EE90(playside)   // ark::network::PlayerDataSaveLogoutRequest
  → precondition: (&DAT_1806EC488)[side] == 0   (no request in flight)
  → FUN_180018E50(side, 3)                       // ReflectSavePlayerData savekind = 3
  → (&DAT_1806EC488)[side] = 0x13                // per-side network step: logout pending
```
`EAmExitRootSequence::onUpdate` = **`FUN_1800A47F0`**: state 1 blocks until **both**
window actors' adopted ark scene (`actor + 0x5C + idx*8`) reaches **`0x42`** (whose
handler is the no-op stub `FUN_18018E360`), then state 2 does `finish(this, 0)` →
`getNextID(0x23) = 0x24` → THANK YOU.
`GameOverSequence::onUpdate` = **`FUN_1800B3C30`**: when e-amusement is **off**
(`FUN_18001B870()` true, i.e. scene 34 was skipped) it acks ark scenes `0x1B` and
`0x29` itself (`0x1800B3CAC`/`0x1800B3CCA`, `0x1800B3CF0`/`0x1800B3D14`) — **direct
evidence that the per-side ark entry-flow scene at session end is `0x1B` = GAMEMODE.**
Its terminal gate:
```
state 3: arkEntryFlowGetCurrentScene(0) == 0 && (1) == 0 && no network op pending
         → arkCoreEventLogRequestGameEnd()   (DAT_1806F2848)
         → finish(this, 0)
```
**Why a forced early entry into scene 34 should still save:** the credit expiry in step
1 of `onSetup` is what pushes ark's entry flow into the GAMEMODE settle scene, and it
is called unconditionally on the side's `PlayerWork+0x4` flag. `EAmEntryRootSequence`
(login, scene 19) is structurally symmetric — it waits for the same `0x42` terminal
scene before finishing (`FUN_1800857A0`) — so `0x42` is the "idle" state during play and
`0x1B` is produced *in response to* the expire. Nothing in the chain reads session
progress. **This remains the single most important cabinet-validation item** (§10).
Per-side skip for a side that never entered: that side's ark flow stays at scene **0**
(`ARK_ENTRYFLOW_NOENTRY`, handler `FUN_180089370`), which merely acks and clears
`(&DAT_1806EC480)[side]` — it never reaches `0x1B`, so no save actor is created for it.
`PlayerWork+0x5` ("has e-am data to save") only controls whether the SAVING window /
result UI is shown, not whether the actor is created; a cardless side's request is a
no-op at the network layer.
### 5.4 `TotalResultSequence` tolerates abnormal entry — with two caveats
vtable `0x18036A1F8`; update `0x1800C9A90`; row builder `0x1800CB090`; teardown
`0x1800CAFD0`. Ctor `FUN_1800C9680` reads only `GameWork+0x8` (→ `this+0x9C`, the
primary side index, used unchecked as a `(&DAT_1806F2ED0)[...]` index — must be 0/1).
**Session-state reads:** `GameWork+0x70` (course fork), `GameWork+0x0` (both sides
visible), and `GameWork+0xC` in exactly two places:
- Row builder: `count = GameWork+0xC + 1`, used **only** as the per-row visibility flag
  `stage < count`. The loop bound is the literal **5**, guarded per-record by
  `mcode != -1`:
  ```c
  for (i = 0; i < 5; i++) {
      if (record[i].mcode == -1) continue;         // <<< the guard
      visible = i < count;
      ...load "total_result" package, place row, jacket/title/score...
  }
  this->0x94 = 1;                                  // build-once latch
  ```
- Update case 4: `for (stage = 0; stage <= GameWork+0xC; stage++)` — with counter 0 that
  is exactly **one** in-bounds pass over `record[0]` (plain int reads on a virgin record,
  no fault); with counter −1 it is skipped entirely.
No division by the stage count anywhere. No `record[-1]`. `GameWork+0x10` and
`DAT_18047E784` appear only in the stage-caption picker (FINAL STAGE vs 1st/2nd/…) —
label selection, no indexing.
Per-side "played" flags are honoured: `PlayerWork+0x5 == 0` zeroes the score deltas in
case 4 so no delta actor is created; `PlayerWork+0x4` only selects the name string.
Case 0 runs for both sides unconditionally and is entirely session-state-independent.
**Exit:** a 0→7 state chain; state 7 (`0x1800CACE6`) is the only exit:
```
MOV  RCX,[DAT_1806F2D40]         ; global ShutterActor
TEST RCX,RCX / JZ  skip
MOVZX EAX,word [RCX+0x82]
CMP  dword [RCX+RAX*8+0x58],4    ; shutter state == 4 (closed)
XOR  EDX,EDX
CALL 0x18021DF70                 ; finish(this, 0)  -> getNextID(0x21) = 0x22
```
**No timers, no network waits, no per-stage-save dependency.** State 3 even breaks early
on a button press. State 6 requests `close(0)` (`FUN_1800334F0` → shutter msg `0x1007`).
**Caveat 1 (dangerous):** case 0 dereferences its packages without a null check —
`FUN_18026EAE0(&DAT_1806FA600, *(u64*)(*DAT_1806F2D68 + 0x870), "total_result_root", 0)`
then `afp_layer_play(*(u32*)(result + 8))`. `FUN_18026EAE0` returns **0** on failure.
`+0x870` is the `scene_result` package slot — see §6. Same unchecked pattern in the row
builder. `main_root` comes from `*(*DAT_1806F2D68 + 0x30)`, the always-resident common
slot, so only `scene_result` is at risk.
**Caveat 2 (hang):** if the ShutterActor is *already closed* (state 4) on entry, state
6's `close(0)` request pushes it 4→5→…→0 (i.e. it **opens**) and state 7 never observes
4 → permanent hang. Entering from song select the shutter is **open (state 0)**, so
`close(0)` runs 0→…→4 and state 7 fires normally. ✔ **Do not** close the shutter
yourself before triggering.
**Caveat 3 (data, not stability):** the row builder skips every record with
`mcode == -1`. With Premium Free active there is only ever **one** record (every play
reuses the frozen slot), and the modpack's own stale-record fix virginizes it on entry
to song select — so a quick logout from song select will show an **empty** TOTAL
RESULTS screen. Valid and stable, just not informative. See §9.1.
**No dependence on a preceding `ResultSequence`** beyond the play records themselves —
no global written by `ResultSequence` is consumed. Also: `createNextSequence` case
`0x21` sets no globals of its own (unlike case `0x20`, which bumps the stage counter),
so jumping straight to `0x21` does not corrupt the stage count.
---
## 6. Scene resource residency — the one hard blocker
`LoadingSequence` ctor = **`FUN_18002D130(this, loadMask, unloadMask, minTime, kind)`**
→ fields `+0x68`, `+0x6C`, `+0x74`, `+0x7C`.
`LoadingSequence::onUpdate` = **`FUN_18002D200`**:
```c
if (!this->0x80 && (shutter == 0 || shutterState == 0 || shutterState == 4)) {
    FUN_1801ACEE0(DAT_1806F2D68, this->0x68, this->0x6C);   // apply the masks
    this->0x80 = 1;
}
if (!this->0x80 || *(char*)(DAT_1806F2D68 + 0x24)) return;   // still loading
... kind-specific background/movie setup ...
if (this->0x74 <= this->0x70 && ...) finish(this, 0);
```
`FUN_1801ACEE0(mgr, load, unload)` — the scene resource manager:
```c
mgr->resident = (mgr->resident & ~unload) | load;     // u32 at mgr+0x20
for (i = 0; i < 0x24; i++)
    if ((mgr->resident & table[i].mask) == 0) release(base + i*0x40), release(base + (i+0x24)*0x40);
for (i = 0; i < 0x24; i++)
    if ((table[i].mask & load) && handles free) load(table[i].name, base + i*0x40, ...);
```
Package table at **`0x18035AD40`** — 36 records × `0x18` = `{u32 mask; u32 pad;
const char* name; const char* dir}`. Slot `i`'s handle ids live at
`*(u32*)(*mgr + i*0x40 + 0x28)` and `+0x928`; the **package pointer** is at
`*(void**)(*mgr + i*0x40 + 0x30)`.
Decoded slots that matter (`i*0x40 + 0x30` in the right column):
| slot | name | mask | package ptr at |
|---|---|---|---|
| 0 | `common_operation_guide` | `0xFFFFFDFE` (effectively always resident) | `+0x030` |
| 18 | `scene_eamusement_window` | `0x00040400` | `+0x4B0` |
| 21 | `scene_caution` | `0x00000800` | `+0x570` |
| 22–25 | `select_music_folder/card/side/option` | `0x00002000` | `+0x5B0`…`+0x670` |
| 32 | `dance_howto` | `0x00001000` | `+0x830` |
| **33** | **`scene_result`** | **`0x00010000`** | **`+0x870`** ← TotalResult's `total_result_root` |
| 34 | `scene_game_over` | `0x00040000` | `+0x8B0` |
| 35 | `event_skill` | `0x00010000` | `+0x8F0` |
`0x870 = 33*0x40 + 0x30` ✔.
Now cross-reference the loader masks from §3.1:
- **`scene_result` (0x10000) is set only by the 0-idx 29 loader** (and its
  matching-chain twin at 0-idx 53).
- The 0-idx 24 loader — the one that runs *into* song select — unloads `0x31800`,
  which **includes** `0x10000`. So on the song-select screen `scene_result` is
  **definitively not resident**.
- The 0-idx 33 loader loads `0x40000`, which covers **both** `scene_game_over`
  (`0x40000`) and `scene_eamusement_window` (`0x40400 & 0x40000 != 0`) — so scenes 34
  and 35 are fully self-sufficient behind that one loader. ✔
**Consequence:** `finish(child, 0x21)` from song select → `TotalResultSequence` case 0
reads a stale/invalid `scene_result` package → `FUN_18026EAE0` returns 0 → unchecked
`afp_layer_play(*(u32*)(0 + 8))` → **crash on the first frame**. This is why Mechanism
A routes through the 0-idx 29 loader instead of jumping straight to 32, and why
Mechanism B (which only needs the 0-idx 33 loader) is the low-risk option.
Requesting the load manually (`FUN_1801ACEE0(mgr, 0x10000, 0)`) and then jumping is
*not* a shortcut: the load is asynchronous and only `LoadingSequence` waits on
`*(char*)(mgr + 0x24)`.
---
## 7. Candidate mechanisms
### Mechanism A — instant logout **with** the summary (recommended primary)
Trigger condition: current scene ∈ song select (`{25, 47, 49}` 0-indexed;
`TS+0x68 ∈ {0x1A, 0x30, 0x32}`) **and** at least one side has `PlayerWork+0x4 != 0`.
```
scene_manager::add_redirect_once(30, 32);        // ResultSequence -> TotalResultSequence
let ts    = scene_manager::current_transition_sequence()?;
let child = *(ts + 0x58);                        // SelectMusicSequence
sequence_finish(child, 30);                      // 1-indexed 30 == 0-idx 29 loader
```
Resulting chain:
```
29 loader (loads scene_result)
   → getNextID(0x1E) = 0x1F  --[redirect 30→32]-->  32 TOTAL RESULTS
   → finish(0) → getNextID(0x21) = 0x22 → 33 loader (loads gameover + eam window)
   → getNextID(0x22) = 0x23 → 34 EAmExitRoot  ** credit expire + LOGOUT SAVE **
   → 35 THANK YOU → 36 → scene 6 → attract
```
Why the redirect works cleanly: the modpack's `scene_manager` already rewrites `RDX`
in its `createNextSequence` detour, **and** its `advance_to_scene` detour repairs
`TS+0x68` to the redirected id — so `getNextID` continues correctly from `0x21`
afterwards. Both halves are already shipped and load-bearing for `skip_intros` and
`quick_restart_or_fail`.
Costs / caveats:
- The 0-idx 29 loader emits one POSEVT `"playmusic"` event log
  (`FUN_18001BAC0("playmusic")` at `0x18002FECD` → `arkCoreEventLog*("POSEVT", "",
  name, 1, 0)`). Cosmetic telemetry artifact; harmless on a private backend.
- The shutter is open on entry, so the cut into the loader has no wipe. Visually abrupt.
  (This is also what makes TotalResult's exit gate work — §5.4 caveat 2. Leave it.)
- The redirect must be one-shot and armed *only* by this trigger. `quick_restart_or_fail`
  also uses `add_redirect_once`, but keyed on scene **29**, not 30 — no collision, though
  the two features should be documented as mutually exclusive in-flight.
### Mechanism B — instant logout **without** the summary (ship this first)
```
sequence_finish(*(ts + 0x58), 34);    // 1-indexed 34 == 0-idx 33 loader
```
Chain: `33 loader → 34 EAmExitRoot (logout save) → 35 THANK YOU → attract`.
No `scene_result` dependency, no redirect, no POSEVT, no `TotalResultSequence` at all.
This is the minimal path that proves the whole hypothesis (especially the ark
entry-flow question in §5.3). Recommend implementing this as milestone 1 behind the
same trigger, confirming the save lands in the backend, then adding Mechanism A's two
extra lines.
### Mechanism C — "make this my last stage" (zero-risk complement)
```
*(i32*)(GameWork + FINAL_STAGE_OVERRIDE_OFF) = *(i32*)(GameWork + STAGE_COUNTER_OFF);
```
Pure data write into a field **no game code ever writes** (§5.2), tripping
`FUN_1800B7270`'s own early-out so the *game* decides the session is over and runs its
own `finish(0x21)`. Every downstream path is 100 % vanilla: correct resource loads,
correct shutter phase, legitimate POSEVT, real summary rows.
- From **GAMEPLAY**: press the combo → the song in progress becomes the final stage.
  Solves the stated problem directly with zero forced transitions.
- From **SONG SELECT**: the *next* song becomes the final stage (one more song than
  "instant", but free of every risk above).
- Self-clearing: `GameWork::reset` (`FUN_1801DCAB0`) restores `-1` at the start of each
  session, so no cleanup and no cross-session leakage.
- Side effect to expect: `FUN_1801DD620`/`FUN_1801DD660` also start returning true, which
  affects the extra-stage fade-flag selection in `createNextSequence` case `0x1E`
  (`0x30000` instead of `0x10000` — the extra bit maps to no slot, harmless) and
  suppresses `result::ExtraActor` creation (desirable). It may also make stage-indicator
  captions read "EXTRA STAGE"; writing the field only once the player is already in
  GAMEPLAY keeps that window closed.
- **Must write the 0-based counter value, not `counter + 1`** — see the warning in §5.2.
**Recommendation:** ship C and B together first (C is ~10 lines and cannot crash; B
proves the logout save), then layer A on top for the summary.
---
## 8. Signatures required
### 8.1 New — `sequence_finish` (`agcs::Sequence::finish`)
```
48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 59 08 48 8B F1 8B FA F6 43 20 20
```
`MOV [RSP+8],RBX; MOV [RSP+0x10],RSI; PUSH RDI; SUB RSP,0x20; MOV RBX,[RCX+8];
MOV RSI,RCX; MOV EDI,EDX; TEST byte [RBX+0x20],0x20`. **Unique on all three builds
tested.** (Optionally extend with `75 ?? 48 8B 03 44 8B C7 BA 01 02 00 00` to pin the
`0x201` message.) Signature: `unsafe extern "C" fn(*mut u8, i32)`.
### 8.2 New — `final_stage_override_probe` (`FUN_1801DD660`, for Mechanism C)
A 46-byte leaf that yields **every** constant Mechanism C needs, decoded from the
matched bytes in the `stage_record_accessor` / `bm2d_package` house style — nothing
hardcoded:
```
+0   48 8B 05 d32          MOV RAX,[rip+d32]      ; d32 @ +3  -> GameWork ptr-ptr global
+7   48 8B 08              MOV RCX,[RAX]          ; -> GameWork
+10  48 83 79 d8 00        CMP qword [RCX+d8],0   ; d8  @ +12 -> course field (0x70)
+15  8B 51 d8              MOV EDX,[RCX+d8]       ; d8  @ +16 -> STAGE COUNTER (0x0C)
+18  75 17                 JNZ
+20  8B 81 d32             MOV EAX,[RCX+d32]      ; d32 @ +21 -> event-mode field (0xD0)
+26  83 F8 01 / 74 0C / 83 F8 02 / 74 07
+37  3B 51 d8              CMP EDX,[RCX+d8]       ; d8  @ +37 -> FINAL-STAGE OVERRIDE (0x10)
+40  0F 94 C0 / C3 / 32 C0 / C3
```
Anchor pattern (matches at function start + 7; subtract 7, or use it as-is and read
the disp8s at the anchor-relative offsets):
```
48 8B 08 48 83 79 70 00 8B 51 0C 75 17 8B 81 D0 00 00 00 83 F8 01 74 0C 83 F8 02 74 07 3B 51 10 0F 94 C0 C3
```
**Unique on all three builds tested.** Sanity-check the decoded offsets against
`premium_free_stage_inc`'s own disp8 (stage counter) and `stage_record_accessor`'s
course disp8 — if they disagree, fail the mod closed.
### 8.3 Reused (already shipped, version-agnostic)
| name | resolution | used for |
|---|---|---|
| `scene_transition` | debug string `sequence::TransitionSequence::createNextSequence` | scene tracking + the `30 → 32` redirect |
| `advance_to_scene` | existing AOB | the `TS+0x68` repair that makes the redirect stick |
| `stage_record_accessor` | existing AOB | GameWork ptr-ptr global, course offset, player-work table (cross-check) |
| `premium_free_stage_inc` | existing AOB | stage-counter offset (cross-check) |
| `player_work_table` | existing derivation | `PlayerWork+0x4` "side entered" gate |
`TS + 0x58` (active gosub child) is a bare offset — already used by
`quick_restart_or_fail::ACTIVE_CHILD_OFFSET`; reuse the same constant.
---
## 9. Interactions with existing modpack features
### 9.1 Premium Free
- The stage counter is frozen, so **every play overwrites the same record slot** and
  the modpack's stale-record fix virginizes it (`mcode = -1`) on each entry to song
  select. Result: TOTAL RESULTS will be **empty** (or one row) after a Premium Free
  session. Stable, just uninformative. Worth surfacing in the doc/UI, and worth a
  future-work note: a Premium Free variant that lets the counter advance but neutralises
  the end-of-session predicate would give a real summary (and up to 5 rows, the record
  array's hard limit).
- The `savekind = 3` marshal loops stages `0 .. min(counter, 4)` and skips records with
  `mcode == -1` or `end_time == 0`. With a frozen, virginized record the logout payload
  carries **zero stage entries** — which is *safer* (no stale or duplicate submission),
  and the per-stage saves (`savekind = 2`) already delivered every score. Only the
  profile/customize fields ride on the logout save, which is exactly what the user wants.
  Open question for the backend: does bemani-buddy derive anything (playtime totals) from
  the logout payload's stage list?
- Quick logout does **not** need to un-freeze the counter first: the scene-31 Wait
  sequence (the only stage bump) is skipped by every mechanism here.
### 9.2 `score_guard`
**(Superseded at implementation — see §13.)** This section described the policy as it
stood when researched: `is_logout_suppressed(side)` read `SESSION_TAINTED[side]` and the
`save_sender` trampoline suppressed the whole `savekind = 3` save for a tainted side —
ending the session **without** saving the profile. The shipped feature replaced that
with **sanitise-and-forward** (user decisions D21–D26): the accessor is now
`logout_taint(side)`; on entry to scene 34 a sanitiser virginises the tainted side's
5 array records + course record (`mcode = -1`, via the shared `stage_records` service)
and the trampoline strips `<data><league>` (libavs **Ordinal 164** =
`property_node_remove`) before forwarding, so the profile/customize write-back
persists. Fail-closed: if the record layout didn't decode or Ordinal 164 didn't
resolve, the old full suppression applies. Quick logout itself still taints nothing —
it fabricates no score.
### 9.3 `quick_restart_or_fail`
Uses `add_redirect_once(29, target)`; Mechanism A uses `add_redirect_once(30, 32)`.
Different keys, no collision. Still, both should refuse to arm while the other is
in flight. Also: `quick_restart_or_fail` triggers only during GAMEPLAY, quick logout
(A/B) only during song select — naturally disjoint.
### 9.4 Input / trigger design
`input_manager` gives per-player edge-detected events for Start, the 4 menu directions
and all 12 numpad keys, and the numpad is ignored by the game on most screens (which is
why the modpack's gestures live there). Currently taken: triple-`0` (mod menu),
triple-`1` / triple-`3` (restart / fail, gameplay-gated).
Because a logout is **destructive and unrecoverable**, recommend:
- a **two-stage gesture**: e.g. triple-`9` arms and shows a confirmation (a
  `TextWidget`/`ImageWidget` prompt), a second triple-`9` within ~5 s commits, anything
  else disarms;
- a hard scene gate (song select only for A/B; GAMEPLAY only for C);
- a session-active gate (`PlayerWork+0x4` set on at least one side) so it can never fire
  in attract or the operator menu;
- config gating in `mod-config.json` under `mods["quick-logout"]`, plus a
  `quick_logout` block for the gesture key and the confirm window;
- an option row on the MODS tab is *not* appropriate for the trigger itself (it is an
  action, not a setting), but a `PersistMode::Full` enable toggle would fit the existing
  pattern.
### 9.5 Where the code would live
- `src/mods/quick_logout.rs` — new mod (`id = "quick-logout"`), registered in
  `src/lib.rs`.
- `src/core/signatures.rs` — the two new definitions from §8.
- `src/types/scenes.rs` — add the missing constants: `FINAL_RESULTS = 32`,
  `FINAL_TO_THANKS_INTERSTITIAL = 33`, `EAM_EXIT = 34`, `THANK_YOU = 35`. (Names for
  29/30 are slightly off in the current map: 29 is the post-song `LoadingSequence` that
  *renders* the pass/fail shutter, 30 is the actual `ResultSequence`. Worth a comment
  rather than a rename, to avoid churn.)
- No new service is needed — `scene_manager`, `input_manager` and `memory` cover it.
---
## 10. Cabinet validation checklist / open questions
Ordered by risk:
1. **Does a forced `EAmExitRootSequence` actually perform the logout save?** The chain
   depends on ark's per-side entry-flow scene becoming `0x1B` (GAMEMODE) in response to
   `arkExpireCredit`/`arkEACoinExpire`. All static evidence supports it (§5.3) but the
   flow itself lives in `arkmdxbio2.dll`. **Test with Mechanism B**: trigger from song
   select, confirm the LOGOUT screen shows, confirm the `SavePlayerDataActor:%dP GameEnd`
   log line, confirm `(&DAT_1806EC488)[side] == 0x13` transiently, and confirm the
   backend received the `savekind = 3` payload. Failure mode to watch for: both window
   actors immediately report scene `0x42` → the exit sequence finishes in ~1 frame →
   THANK YOU with **no save**. That would be a silent no-op, so instrument it.
2. **Does the profile/customize write-back actually land?** Verify the modpack's
   `PersistMode::SaveOnly` WebUI cosmetics and the workout-profile fields round-trip
   after a quick logout.
3. **Mechanism A: does `TotalResultSequence` render and exit?** Watch for (a) a crash in
   case 0 (means `scene_result` was still not resident — the redirect fired too early or
   the 0-idx 29 loader chose a different mask path), and (b) a hang in state 7 (shutter
   phase, §5.4 caveat 2).
4. **Shutter/visual quality of the cut** out of song select. If it is unacceptable,
   investigate requesting the shutter *before* the loader — but note the interaction with
   TotalResult's exit gate.
5. **Mechanism C: confirm the `+0x10` write ends the session at the intended stage** and
   that no "EXTRA STAGE" caption leaks onto the stage indicator / results.
6. **2P sessions.** Everything here is symmetric (both sides get a window actor, both
   get expired), but confirm a 2P session logs both players out and saves both.
7. **PASELI vs credit.** `PlayerWork+0x8 == 3` takes the `arkEACoinExpire` branch and
   sub-state `0x2C` waits on `arkEACoinQuerySessionState`. Verify a PASELI session closes
   cleanly rather than hanging on that wait.
8. **Course / Dan mode** (`GameWork+0x70 != 0`). `TotalResultSequence` short-circuits to
   state 7 (renders nothing, only waits for the shutter), and the course record has
   different semantics. Recommend simply **blocking quick logout in course mode** for v1.
9. **Event/special modes** (`GameWork+0xD0 ∈ {1,2}`) use `0x39` instead of `0x21` for
   total results. Either mirror the branch or block the feature in those modes for v1.
---
## 11. Address reference
### 11.1 Primary build (20260721), file-relative to `0x180000000`
| Symbol | Address |
|---|---|
| `TransitionSequence::createNextSequence` | `0x18002E240` |
| `TransitionSequence::advanceToScene` | `0x18002DC10` (TS vtable `0x18035D5B8` slot `+0x48`) |
| `TransitionSequence::getNextID` | `0x18002DD70` |
| `TransitionSequence::update` | `0x18002D7D0` (slot `+0x30`) |
| `agcs::Sequence::onMessage` | `0x18021E070` (`agcs::Sequence::vftable` `0x180389D08` slot `+0x18`) |
| **`agcs::Sequence::finish`** | **`0x18021DF70`** |
| install gosub child (writes `TS+0x58`) | `0x18021DEF0` |
| leave broadcast (`0x104`) | `0x18021DE20` |
| destruction reaper | `0x18022EBE0` (called from main loop `0x180003020` @ `0x180003147`) |
| mark subtree dead | `0x18022EB10` / `0x18022EB70` (all TS children) |
| `LoadingSequence` ctor | `0x18002D130` |
| `LoadingSequence::onUpdate` | `0x18002D200` |
| scene resource mask apply | `0x1801ACEE0` |
| scene resource table | `0x18035AD40` (36 × `0x18`) |
| `/gameOptions/max_stage/current` read | `0x18002E911` (out-ptr `DAT_18047E784`) |
| `GameWork::reset` | `0x1801DCAB0` (`+0x10 = -1` @ `0x1801DCAF4`) |
| stage-counter `INC` | `0x180030368` (`premium_free_stage_inc` patch site) |
| `isLastNormalStage(stage)` | `0x1801DD550` |
| `isCurrentStageExtra()` | `0x1801DD620` |
| **override probe (Mechanism C anchor)** | **`0x1801DD660`** (pattern matches at `0x1801DD667`) |
| extra-stage granter | `0x1801DD0B0` |
| `ResultSequence` ctor / update / setup | `0x1800B6400` / `0x1800BBC30` / `0x1800B7270` |
| `ResultSequence` `finish(0x21)` site | `0x1800BEA03` (state 30 block `0x1800BE940`) |
| `TotalResultSequence` ctor / update / rows / teardown | `0x1800C9680` / `0x1800C9A90` / `0x1800CB090` / `0x1800CAFD0` |
| `TotalResultSequence` `finish(0)` site | `0x1800CAD05` (state 7 `0x1800CACE6`) |
| `EAmExitRootSequence` ctor / onSetup / onUpdate | `0x1800A41B0` / `0x1800A4330` / `0x1800A47F0` |
| `EAmEntryRootSequence` ctor / onSetup / onUpdate | `0x180084A90` / `0x180084C10` / `0x1800857A0` (vtable `0x180364218`) |
| `EAmEntryWindowActor` ctor / update | `0x180086230` / `0x180088210` |
| ARK_ENTRYFLOW_GAMEMODE handler (record `0x1B`) | `0x180094A30` |
| `SavePlayerDataActor` ctor / onUpdate | `0x1800B4C80` / `0x1800B4E30` |
| `ark::network::PlayerDataSaveLogoutRequest` | `0x18001EE90` |
| `ReflectSavePlayerData(side, savekind)` | `0x180018E50` |
| `GameOverSequence` ctor / onUpdate | `0x1800B3830` / `0x1800B3C30` |
| `arkEamOff()` helper | `0x18001B870` |
| POSEVT emit helper | `0x18001BAC0` (`"playmusic"` @ `0x18002FECD`) |
### 11.2 Globals
| Global | Meaning |
|---|---|
| `DAT_1806F14F8` | GameWork ptr-ptr (`GameWork = **(void***)`) |
| `DAT_1806F2ED0` | 2-entry PlayerWork wrapper table |
| `DAT_18047E784` | `/gameOptions/max_stage/current` (image value 2) |
| `DAT_18047E788` | `/gameOptions/gameoverduringsong/current` |
| `DAT_1806F2D68` | scene resource / package manager (`+0x20` resident mask, `+0x24` loading flag) |
| `DAT_1806F2D40` | `ShutterActor` singleton |
| `DAT_1806FA600` | BM2D layer pool (0x400 × 0x240) |
| `(&DAT_1806EC488)[side]` | per-side network save step (`0x13` = logout save pending) |
| `(&DAT_1806EC480)[side]` | per-side entry-flow ack state |
GameWork fields used here: `+0x00` sides-visible, `+0x04`, `+0x08` primary side,
**`+0x0C` stage counter (0-based)**, **`+0x10` final-stage override (`-1`)**,
`+0x18` current music id, `+0x59`/`+0x5A` extra stage granted/consumed,
`+0x70` course, `+0xD0` event mode, `+0x13E`/`+0x13F` server-pushed booleans.
PlayerWork fields: `+0x04` side entered, `+0x05` has e-am data to save,
`+0x08` payment kind (0–2 credit, 3 PASELI), `+0x1598..0x15A0` played-music vector,
`+0x590 + stage*0x2B8` per-stage records (5 slots).
### 11.3 Cross-version
| Anchor | 20260324 | 20260616 | 20260721 |
|---|---|---|---|
| `sequence_finish` (§8.1) | `0x18021AF30` | `0x18021DB90` | `0x18021DF70` |
| override probe (§8.2, match addr) | `0x1801DBBB7` | `0x1801DD1B7` | `0x1801DD667` |
Both patterns returned exactly **one** match per build. Scene ids, `getNextID`
structure, the resource-table layout and the GameWork field offsets were read on
20260721 only — re-derive rather than hardcode wherever a signature can supply them,
per house style.
---
## 12. Dead ends / negative results
- **`arkEntryFlowGetGameOverFlag` / `arkEntryFlowGetGameOverState` are resolved but
  never called.** Full xref sets for `DAT_1806F2B10` / `DAT_1806F2B18` are the two
  resolver `LEA`s each. gamemdx polls **no** framework-owned "game over" signal, so
  there is nothing there for a hook to drive. (`arkResetShortBalanceRetryCount` /
  `DAT_1806F2B08` is likewise dead.)
- **`arkEntryFlowResetGameRequest` / `ResetGameWait`** re-arm the *entry* flow for a new
  play (sites: `0x180001727`, `0x180085F3D`, `0x180095657`, `0x1800B0984`). They are not
  a "terminate the current session" trigger.
- **`GameWork+0x13E` / `+0x13F`** are server-pushed booleans applied by the
  `ReflectPlayerWork` opcode-9999 table appliers (`0x180016C0E`, `0x180018C6F`; sub-codes
  `0x61/0x62` and `0x70/0x71`) and merely *reported* back. Not session-terminate flags.
- **Sending `0x201` directly to the `TransitionSequence` (or calling `advanceToScene`
  yourself) does not kill the outgoing child** — only `finish`, called *on the child*,
  marks it for destruction. `createNextSequence` case 1 is the only place that mass-kills
  TS children (`FUN_18022EB70`). Do not take that shortcut.
- **Patching `DAT_18047E784`** (max stage) is a worse lever than `GameWork+0x10`: it is
  re-read from AVS on every session start at `0x18002E911`, and `FUN_1801DD0B0` requires
  it to be exactly 2 for extra stages.
- **Manually requesting the `scene_result` load** and then jumping to scene 32 does not
  work — the load is asynchronous and only `LoadingSequence` waits on
  `*(char*)(DAT_1806F2D68 + 0x24)`.
---
## 13. Implementation & cabinet validation outcomes (2026-07-28)
Shipped as the `quick-logout` mod (`src/mods/quick_logout.rs`) plus the logout-save
sanitisation policy (`src/services/score_guard.rs` +
`src/services/custom_options_persistence.rs`, records via the new shared
`src/services/stage_records.rs`). Planning record:
`.agents/planning/20260727-quick-logout/`.
### 13.1 Validation results (single consolidated cabinet pass)
The maintainer ran the design's §7 checklist against the feature-complete build and
reported a clean pass across the board — in particular:
- **§10 item 1 (assumption A1) CONFIRMED:** a forced entry into `EAmExitRootSequence`
  performs the logout save. The full chain rendered and completed
  (song select → 29 loader → TOTAL RESULTS → LOGOUT window → THANK YOU → attract)
  with neither FR4 WARN firing (scene 34 present, dwell ≥ 500 ms).
- Mechanism A's TOTAL RESULTS rendered and exited normally — no `scene_result` crash
  (the redirect fired after the 29 loader), no shutter hang.
- Profile/customize write-back round-trips through a quick logout.
- **R3 answered: the triple-9 gesture DOES fire while the song-select options modal is
  open.** Accepted as-is (cosmetic; the modal is still music selection — the session
  ends normally from there).
### 13.2 Deviations from this document's recommendations
The implemented feature deliberately differs from §7/§9.4's suggestions (user
decisions in the planning register, D1–D26):
- **Mechanism A only.** B (no-summary) and C ("make this my last stage") were not
  shipped; C is recorded as future work. A stopped needing B as a proving milestone.
- **Bare triple-9 trigger** — no two-stage confirmation gesture, no on-screen prompt,
  no `quick_logout` config block, no option row (D3/D9/D10/D11). `mods["quick-logout"]`
  is the only switch.
- **No course-mode or event-mode gates** (§10 items 8–9 suggested blocking):
  verified unnecessary — TotalResult's course fork never touches the package, and
  `getNextID` merges the event summary scene into the same successor (design
  Appendix A).
- **Sanitise-don't-suppress** for tainted logout saves (D21–D26) — see §9.2. §9.1's
  open backend question was settled: bemani-buddy ignores regular-song results in the
  `savekind = 3` payload (they feed only Dan-course grades), and absent `<league>` is
  a no-op — which is what makes record-virginising + league-strip sufficient.
- The mod additionally requires `scene_manager`'s `advance_to_scene` m_currentID
  repair (`redirect_repair_available()`) to enable — without it the `30 → 32`
  redirect would leave `TS+0x68 = 0x1F` and the tail after TOTAL RESULTS would run
  the stage-bump Wait back to song select instead of the logout.