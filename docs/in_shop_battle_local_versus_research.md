# In-Shop Battle UX for Local Versus — Feasibility Research

RE record for bringing the "in-shop battle mode" gameplay HUD — the per-player score
boards, the score-ratio bars, the live 1st/2nd rank badges and the score-margin
readout — to ordinary local 2-player (versus) play on ONE cabinet, without a second
LAN-linked cabinet, without the matching server, and without the 4-player session
the stock mode requires.

All addresses are file-relative to `gamemdx.dll`'s `0x180000000` base. Primary
build: **20260825**; the class layout and constructor were cross-checked
byte-for-byte on **20250805** (the oldest supported build). Research only — no code
this session. Companion doc: `docs/bpl_battle_mode_research.md` (the matching
NETWORK stack: hardware gate, LibComm, ESS broker protocol). This document covers
the DISPLAY side and the local re-hosting plan.

## TL;DR

- The whole battle HUD is ONE self-contained game actor,
  **`sequence::dance::MatchingBattleFrameActor`** (0x280 bytes), created by
  `MatchingDancePlaySequence::onUpdate` as a sibling of the two `GamePlayActor`s.
  Its art package `dance_matching` is **already resident during normal gameplay**
  (same loader-mask group as `dance_common`/`dance_stage`), and its screen anchor
  (`matching_usr` in `dance_root`) is registered by the SAME `LayoutActor` builder
  that lays out the normal HUD. Nothing about the actor's rendering depends on the
  network — only its DATA does.
- The actor pulls scores from `CNetworkManager`'s per-cabinet blocks
  (`DAT_1806f3ac8[cab] + 0x80 + slot*4`, filled by UDP `ScoreReport`/`ScoreNotice`)
  and player identity (ddrcode, dancer name, BPL team id) from the same blocks. In
  normal play those blocks are null, so a naïvely constructed frame shows two empty
  0-score boards.
- **Recommended approach (A):** construct the stock actor ourselves at gameplay
  entry, give the instance a **mod-owned vtable clone** whose `onInitialize` slot
  flips `GameWork+0x0` (versus flag) to 0 for the duration of the stock call (so it
  takes the 2-player `main_single` branch) and whose `onUpdate` slot feeds
  `BATTLE_INFO[i].score` straight from `GamePlayActor+0x1D4/+0x1D8`, then hands off
  to the stock rank/draw code. Zero detours, zero byte patches, one vtable clone,
  ~5 new signatures/derivations, all game-side code paths stock. Estimated
  implementation: one single-file mod + a handful of `signatures.rs` entries.
- Spoofing the matching mode (Approach C) is a dead end: the matching scene chain
  (0-idx 47–57) blocks on `CNetworkManager` state 4 (connected) and on the
  music-start sync handshake, and it flips `GameWork+0xD0 = 1` which re-routes the
  results/total-results/logout flow and forces EX-score rules. Re-implementing the
  HUD from scratch on the `dance_matching` assets (Approach B) is possible but ~10×
  the work for the same pixels.

## 1. What the stock battle HUD is

### 1.1 Mode entry and gating (why it is locked)

| Step | Where (20260825) | Gate |
|---|---|---|
| Hardware gate → `CNetworkManager` init | `FUN_180001420` → `FUN_1801a4b00(1)` | `arkMDXGetMachineType()==4 && PCType ∈ {2,3,4}` (gold cab) — see companion doc |
| Mode-select "In-shop battle" button | `SelectStyleSequence::onUpdate` = `FUN_1800b0bc0` | `matching_group != 0 && matching_ready && FUN_1801bb350()` (partner found) → `change_battle_usr` text `scmo_change_battle_on` |
| BPL flag | `SelectStyleSequence+0x288` (byte; `param_1[0x51]`) | set when the player toggles into battle |
| **Mode commit** | `FUN_1800b0bc0` case 1 | `*(GameWork+0xD0) = bpl_flag ? 1 : 0` — **this is the event-mode selector the rest of the repo already knows (`stage_records::event_mode()`; 1 = BPL battle, 2 = the other event chain)**; also `GameWork+0x0 = (2 players) ? 1 : 0`, `GameWork+0x4 = style`, `GameWork+0x8 = primary side` |
| Scene chain | `createNextSequence` (`FUN_18002e3b0`) | `GameWork+0xD0 ∈ {1,2}` selects the 0-idx 47–57 chain: `MatchingCautionSequence`, matching song-select variants (47/49, `matching_left/right_usr`, `no_matching_usr`), the 0-idx 51 stage loader (`case 0x34`, loads mask 0x8000 like the normal one), **`MatchingDancePlaySequence`** (`case 0x35`), results with `in_battle`/`battle_rank_usr`, total results with `loop_bpl_rank`/`header_usr/name_bpl_usr` |
| Score rule | `FUN_1801ea320()` ("use EX score") | returns 1 unconditionally when `GameWork+0xD0 ∈ {1,2}`; otherwise the operator option `/gameOptions/use_ex_score/current`. Cached per song in `GamePlayActor+0x1D0` |

So in stock play the HUD is reachable only through `GameWork+0xD0 == 1`, which
drags in the entire network session. Nothing in the HUD actor itself checks 0xD0.

### 1.2 `MatchingDancePlaySequence::onUpdate` (`FUN_180061cc0`) — the creation site

The matching DPS mirrors the normal `DancePlaySequence::onUpdate` (`FUN_180057e10`)
with network waits spliced in. Relevant states:

| State | What |
|---|---|
| 1 | wait `LayoutActor` (this+0x138) reaches state 1 (layout built) |
| 2 | **requires `DAT_1806f2d48` (matching session actor) at state 4 = connected**; creates the two `GamePlayActor`s (ctor `FUN_18005ae30` with `param_10 = 1` = matching — that flag only adds a `MatchingBattleInfoActor` child and pre-fills the chart's EX max), the shared score/stage/song-info actors, **and then `MatchingBattleFrameActor` (alloc 0x280 via `agcs_heap_malloc`, ctor `FUN_180071740`, `Actor::addChild` `FUN_18021f230`)** |
| 3–6 | music-start sync handshake with the other cabinet (`FUN_1801bc890`, `MusicStartSyncNotice*`); "FAILED TO MUSIC START SYNC" fallback after 3 s |
| 0xB (in song) | every frame: `score = *(actor+0x1D0) ? *(actor+0x1D8) /*EX*/ : *(actor+0x1D4) /*money*/` per side → written to the LOCAL cabinet block `DAT_1806f3ac8[DAT_1806f391c] + 0x80 + side*4` (monotonic max); the network layer ships it (`CUDPFunctionManager::SendScoreReport` guest→host, `ScoreNotice` host→guests: `FUN_1801c3470` / `FUN_1801c3520`, 0x10/0x14-byte packets, stage-checked, monotonic max) |
| 0xE | copies both sides' stage result records (0x3C bytes each, from `PlayerWork+0x590+stage*0x2B8+…`) into the block at `+0x88 + (stage*2+slot)*0x3C` for `StageResultReport/Notice` |

Normal `DancePlaySequence::onUpdate` case 1 creates exactly the same children minus
the frame actor, with `param_10 = 0`. That symmetric gap is what the mod fills.

### 1.3 The frame actor

`sequence::dance::MatchingBattleFrameActor` — vftable `0x1803626f8` (20260825);
RTTI `.?AVMatchingBattleFrameActor@dance@sequence@@`. Standard agcs actor
(`agcs::Actor::vftable` `0x180389cc8` base; `+0x50` flags, `+0x58..` StackStep
pairs, children list at `+0x18`/`+0x10` — the same actor tree `song_reset` walks).

| Slot | Function | Role |
|---|---|---|
| 0 | `FUN_180071ba0` | deleting dtor (`flag&1` → `agcs_heap_free`) — **the parent's teardown frees it, so the instance MUST come from `agcs_heap_malloc`** |
| 3 | `FUN_18021d7d0` | system-message dispatcher (0x101 init → slot 4 once; 0x102 update → slot 6; 0x103 draw → slot 7; 0x104 finalize → slot 5) |
| 4 | `FUN_180071ce0` | **onInitialize** — builds the AFP layer and widgets |
| 5 | `FUN_1800735f0` | onFinalize — final rank + release layer |
| 6 | `FUN_180072e20` | **onUpdate** — pull scores from network blocks, smooth, rank |
| 7 | `FUN_180072f70` | onDraw — text/rank/gauge refresh |
| 8 | `0x180001000` | onReceiveMessage (ret 0 — it handles NO user messages) |

Constructor `FUN_180071740(this, layoutDesc, actorsArray, isEx, isDouble, mcode, diff)`:

```
this+0x88  = layoutDesc      // = LayoutActor + 0x98 (the layout descriptor: package map @+0x10, position map @+0x30)
this+0x90  = 0               // max score, computed in onInitialize (1,000,000 or the chart's EX max)
this+0x94  = isEx  (byte)    // FUN_1801ea320() at the call site
this+0x98  = isDouble        // either side's style==double
this+0x9C  = mcode           // record[stage]+0x0  (PW + 0x590 + stage*0x2B8 on 20260825; base is build-dependent — stage_records derives it)
this+0xA0  = diff            // record[stage]+0x4
this+0xA8  = layer           // BM2D layer (set in onInitialize)
this+0xB0  = BATTLE_INFO[4]  // 0x40 stride
this+0x1B0 = participants    // 2 if GameWork+0 == 0 else 4   <-- THE layout selector
this+0x1B8 = widgets[4][3]   // shared_ptr<SpriteLayer> ×3 per participant (name, score, point) — 0x30 stride
this+0x278 = actorsArray     // &GamePlayActor*[2] (DPS+0x128 in the matching DPS)
```

`BATTLE_INFO` (0x40, vftable `0x180362748`) — the first 0x1C after the vtable is a
verbatim copy of the network `PlayerInfo` record:

```
+0x00 vftable
+0x08 i32 player_index   (0..3 in the 4-player space; -1 = none)      <- record +0x00
+0x0C i32 ddrcode        (-1 = none; only logged by onInitialize)         <- record +0x04
+0x10 char name[16]      (8-char dancer name + NUL)                    <- record +0x08
+0x20 i32 team_id        (BPL team 100001..100007 → team art; else 0)   <- record +0x18
+0x24 i32 score_target   (written every onUpdate)
+0x28 i32 score_display  (smoothed: (display+target+1)/2, clamped ≤ target — eases up, snaps down)
+0x2C i32 score_diff     (vs best opponent; vs leader in 4P)
+0x30 i32 rank           (0 = 1st; -1 = none; ties share)
+0x34 f32 gauge_fraction (display / max_score)
+0x38 u32 position→player_index map (≥4 ⇒ unused slot)
+0x3C u8  isEx copy
```

The ctor's position mapping reads `CNetworkManager`'s blocks (`FUN_1801bccb0`
`GetPlayerInfo`, `FUN_1801bcbc0` `IsRemotePlayer`) and logs
`set player info : position=%d, player_index=%d, ddrcode=%d, dancername=%s`. With
null blocks every position stays `-1`/empty — harmless, we overwrite it.

#### onInitialize (`FUN_180071ce0`) — what the pixels are

1. Package: `FUN_18006ece0(layoutDesc, "dance_matching")` → package map miss →
   default slot `*(*DAT_1806f2d70 + 0x7F0)` = scene-resource-manager slot 31
   (`dance_matching`, see §2.1).
2. Layer: `FUN_180257af0(layer, pkg, GameWork+0 == 1 ? "main_tag" : "main_single", 5)`
   — **`main_single` = the 2-player layout, `main_tag` = the 2v2 (tag) layout**.
   A missing clip logs and leaves `layer == NULL`, then dereferences it (crash) —
   the package must be resident.
3. Position/scale from the position map entry `"dance_matching"` (layer vfuncs
   `+0x38` pos, `+0xC0` scale) — registered by the `LayoutActor` builder from the
   `matching_usr` marker of `dance_root` (§2.2). `base_usr` → texture `dama_base`.
4. `GameWork+0 == 0` only: hides `score_3p_usr`, `score_4p_usr`, `gauge_3p_usr`,
   `gauge_4p_usr` (msgs 0x1007/0x101E).
5. Per participant `i < this+0x1B0`:
   - `score_{i+1}p_usr/name_usr` ← a `sequence::SpriteLayer` (the SAME glyph-row
     class music_wheel_song_length drives) with `cote_normal_<char>` glyphs of the
     dancer name; `score_{i+1}p_usr/score_usr` ← SpriteLayer of `dama_score_diff_<digit>`
     glyphs; `score_base_usr` texture ← `dama_score_base_{1..4}p` or the BPL team
     art (`dama_score_base_{leisure,round,tradz,silk,game,gigo,apina}`) when
     `team_id ∈ 100001..100007`.
   - `gauge_{i+1}p_usr/main_gauge_usr` (positions 0/1) or `sub_gauge_usr` (2/3) ←
     `dama_gauge_{1..4}p_{single|tag|tag_sub}` (row by mode: single for
     `GameWork+0 == 0`), `gauge_{i+1}p_usr/point_usr` ← SpriteLayer (positions 0/1
     only), gauge clip paused at its start frame.
   - Logs `set score board %s` / `set gauge board %s, %s`.
6. Max score: for each live `GamePlayActor` in `this+0x278`: if
   `BATTLE_INFO[i].position_map (+0x38) == -1 || !isEx (+0x3C)` → 1,000,000; else the
   chart's EX max from the music DB entry (`FUN_1801b76f0` lookup,
   `entry+0x1B4 + (diff + mode*5)*4` — the same slot fast_bootup documents).

#### onUpdate (`FUN_180072e20`) and rank (`FUN_180073650`)

```
for each participant p:
    target = 0
    if p.position_map < 4:
        for cab in 0..2: block = DAT_1806f3ac8[cab]; if block:
            for slot in 0..2: if record(block,slot).valid && record.player_index == p.position_map:
                target = *(block + 0x80 + slot*4)          // <-- the ONLY score source
    p.score_target = target                                // null blocks ⇒ 0 every frame
for each participant p (if max_score != 0):
    p.display = min((p.display + p.target + 1) / 2, p.target)
    p.gauge_fraction = display / max_score
tail-jump FUN_180073650(this)                              // rank
```

`FUN_180073650` collects participants whose `position_map (+0x38) != -1`, qsorts by
`score_display` descending, assigns `rank` (ties share), and `score_diff` =
own − best OTHER display (2P) / own − leader (leader: − second) (4P; for exactly two
participants both formulas coincide). onDraw (`FUN_180072f70`) then: score glyphs
(`%d` → `dama_score_diff_*`), point glyphs (`%+d` → `dama_score_diff_*` or the
`_1st` glyph set for the leader), gauge `SetFrame(in_0 + fraction*(in_500 − in_0))`,
rank badge on `main_rank_usr`/`sub_*_rank_usr` (`dama_score_1st`[rank] table
`0x1804658c0`, hidden while `rank == -1`).

`MatchingBattleInfoActor` (per-GamePlayActor child, gated on ctor `param_10`) is
irrelevant: its only behaviour is caching msg-0x1036 scores into
`DAT_1806f2d50[side]`, which has no reader.

## 2. Why re-hosting is cheap: residency and layout are already there

### 2.1 `dance_matching` is resident in normal gameplay

Scene-resource package table `0x18035AD50` (36 × `{u64 mask, char* name, char* dir="bm2d"}`),
consumed by `FUN_1801ac930` with **`(resident_mask & entry.mask) != 0`** (bitwise
AND, not equality):

| slot | name | mask | pkg ptr |
|---|---|---|---|
| 26 | `dance_common` | 0x9000 | `+0x6B0` |
| 27 | `dance_stage` | 0x9000 | `+0x6F0` |
| 28 | `dance_song_info` | 0x9000 | `+0x730` |
| 29 | `dance_danger` | 0x9000 | `+0x770` |
| 30 | `dance_shock_arrow` | 0x9000 | `+0x7B0` |
| **31** | **`dance_matching`** | **0x9000** | **`+0x7F0`** |
| 32 | `dance_howto` | 0x1000 | `+0x830` |

The normal stage loader (`createNextSequence` case 0x1C, the `gameplay_loader_masks`
signature: `MOV EDX,0x8000`) loads mask 0x8000 → `0x8000 & 0x9000 ≠ 0` → slot 31
loads alongside `dance_common`. File: `data/arc/bm2d/dance_matching.arc`
(category `bm2d`). No package work needed; `bm2d_package` on-demand loading is NOT
required.

### 2.2 The anchor is registered by the shared layout builder

`sequence::dance::LayoutActor` (vftable `0x180361c98`, DPS child; `+0x98` = the
layout descriptor the frame ctor receives) runs `FUN_18006bd40` once its packages
are resident (state 0 → 1, `FUN_18006bb30`). The builder instantiates a throwaway
`dance_root` from `dance_common` and records marker positions into the descriptor's
position map — including `"dance_matching"` from marker **`matching_usr`** (both
sides single) / `matching_right_usr` (P1 double) / `matching_left_usr` (P2 double),
right before `"stage"`/`"song_info"`. It runs in BOTH DPS variants, so the anchor
exists in local versus. (A LayeredFS `dance_common` replacement must keep the
`matching_usr` marker — a missing marker yields position 0,0 and scale 0.)

The normal DPS waits for `LayoutActor` state 1 before its case-1 child creation, so
"DPS past case 1" ⇒ "anchor registered".

## 3. Approaches

### A — Re-host the stock actor with a mod-owned vtable (RECOMMENDED)

Create the real `MatchingBattleFrameActor` in the normal DPS and own only the two
behaviours that differ from stock: the layout selector and the score source.

```
at first frame where: scene == GAMEPLAY, live DPS (song_reset::live_dps), both
GamePlayActors present (gameplay_actors), both sides entered, GameWork+0 == 1
(local versus), no existing frame child, mod enabled, not already done this song:

  layout   = DPS child with LayoutActor vftable
  actors   = [gpa_side0, gpa_side1]  (order by GamePlayActor+0x84 side)
  is_ex    = *(gpa+0x1D0)            // the game's own cached FUN_1801ea320()
  mcode/diff = stage_records current record header (+0x0 / +0x4)

  a = agcs_heap_malloc(app_heap_handle, 0x280, 0, 0)
  ctor(a, layout+0x98, &actors[0], is_ex, 0 /*single*/, mcode, diff)   // stock ctor; GameWork untouched
  *a = MOD_VTABLE                    // clone of the stock vftable, slots 4 and 6 replaced
  a+0x1B0 = 2
  for i in 0..2: BATTLE_INFO[i] = { player_index=i, ddrcode=*(PW[i]+0x18) (or -1 for guest),
                                   name = PW[i]+0x0C or "PLAYER1/2", team_id=0, position_map=i, isEx }
  addChild(dps, a)                   // FUN_18021f230 — appended last, updated after LayoutActor/GamePlayActors
```

The vtable clone (0x48 bytes + RTTI locator at [-1], in mod-owned memory):

- **slot 4 (onInitialize)**: `saved = GameWork+0; GameWork+0 = 0; stock_onInitialize(this); GameWork+0 = saved`.
  This selects `main_single`, hides the 3p/4p boards and picks the `_single` gauge
  art. The call is synchronous on the game thread inside the DPS's 0x101 dispatch;
  no other game code observes the flipped word (the render thread does not read
  `GameWork`). Callees are BM2D/SpriteLayer/music-DB helpers — none read
  `GameWork+0` (verified by reading `FUN_180071ce0`'s call tree at one level; the
  design should re-verify with a `DAT_1806f14f8` xref check).
- **slot 6 (onUpdate)**: replace the network read with
  `BATTLE_INFO[i].score_target = *(gpa_i+0x1D0) ? *(gpa_i+0x1D8) : *(gpa_i+0x1D4)`
  (`gpa_i` re-read from `this+0x278` each frame, null-guarded), then the stock
  smoothing (5 lines) and `FUN_180073650(this)`. `GameWork+0 == 1` in the rank
  function is harmless for two participants (§1.3). Alternatively call the stock
  onUpdate and re-write `score_target` before the smoothing — not possible, the
  smoothing is inside the same function; the ~15-line re-implementation is the clean
  option.
- slots 0, 3, 5, 7, 8 = stock. Teardown is the parent's: when the DPS dies it
  runs each child's deleting dtor → `agcs_heap_free` on our allocation (allocator
  matched by construction). `song_reset` in-place restarts keep the actor alive;
  the snap-down smoothing handles the score reset; `max_score` is per-song anyway.

Why this and not the alternatives in the same family:

- A detour on the stock `onInitialize`/`onUpdate` with an identity gate would work
  too but adds two detours to functions shared with real matching play; the vtable
  clone touches only our instance.
- Writing `GameWork+0` around the ctor is unnecessary: with null network blocks the
  4-player ctor branch writes nothing we keep, and `+0x1B0`/`BATTLE_INFO` are
  overwritten afterwards.
- Faking `DAT_1806f3ac8[0]` (a mod-owned cabinet block with two valid records and
  live scores at +0x80) would let the STOCK onUpdate run unmodified, but the blocks
  are `CNetworkManager` state with other readers — `createNextSequence`
  (`FUN_18002e3b0` ×2), the `ScoreActor` rival-sync path (`FUN_180077d50`: with the
  matching flag it retargets the top score readout to the remote score), the result
  record copy — and they are freed/reset by the manager (`FUN_1801bbbe0`). Rejected.

Prerequisites (all resolvable with the existing scanner toolkit):

| Need | Source |
|---|---|
| `MatchingBattleFrameActor` ctor | new AOB (prologue is byte-identical 20250805 ↔ 20260825: `MOV [RSP+8],RCX; PUSH RSI/RDI/R12/R13/R14; SUB RSP,0x40; MOV [RSP+0x30],-2; … MOVZX R10D,R9B; MOV RBX,R8; MOV R11,RDX; MOV R14,RCX`, then the `agcs::Actor` prologue and `MOV word [R8],5`); also yields the class vftable (2nd `LEA RAX,[rip]` + `MOV [RCX],RAX`) and `BATTLE_INFO::vftable` (`LEA R8,[rip]` before the ×4 init loop) |
| stock vtable slots 4/5/6/7 | read from the derived vftable |
| rank fn `FUN_180073650` | derive: last instruction of stock slot 6 is `JMP rel32` to it |
| `Actor::addChild` `FUN_18021f230` | new AOB (small, distinctive: `CMP RCX,RDX; JZ; TEST RDX,RDX; …; CMP dword [R?+0x24], dword [RDX+0x28]` sorted insert) or derive from the matching-DPS call site |
| `agcs_heap_malloc` + `app_heap_handle` | existing signatures |
| `GameWork` | existing (`stage_records::game_work()`) |
| `GamePlayActor` vftable, live DPS, child walk | existing (`song_reset`) |
| `LayoutActor` vftable | RTTI walk for `.?AVLayoutActor@dance@sequence@@` (repo has the RTTI helpers) — or the vftable that the normal DPS's case 0 compares state against |
| `GamePlayActor+0x1D0/+0x1D4/+0x1D8` | ≤ `+0x1E9` ⇒ identical on all four builds (AGENTS layout rule); still run `shape_diff.py` on the matching-DPS case-0xB read as the attestation |
| `PlayerWork+0x0C` name, `+0x18` ddrcode | existing (`custom_options_persistence` uses `+0x18`); name semantics from `FUN_1801e88a0` (`+0x0C` inline, empty ⇒ `"PLAYER1"/"PLAYER2"`) |
| mcode/diff | `stage_records` (current stage record header `+0x0` mcode / `+0x4` difficulty — the same `PW+base+stage*0x2B8` read the matching DPS does; the base `0x590`/`0x570` is already derived by `stage_record_accessor`) |

### B — Re-implement the HUD on the `dance_matching` assets

Create the `main_single` clip ourselves (the repo's `bm2d_api` + `SpriteLayer`
machinery covers every primitive: layer create by name, find child, set texture,
SetFrame, glyph rows) and drive it from a mod-owned per-frame update. Same pixels,
full control over what is shown (e.g. hide names for guests, custom colours), no
`GameWork` flip. Cost: reproducing ~40 BM2D operations and the widget plumbing of
`onInitialize` (name glyph lists via `cote_normal_%s`, three SpriteLayers per side,
gauge frame math, rank/diff rules) — roughly the size of music_wheel_song_length
plus s_marvelous's results_score. Keep as the fallback if the vtable-clone approach
hits an unforeseen dependency inside `onInitialize`.

### C — Spoof battle mode / fake the network (REJECTED)

Setting `GameWork+0xD0 = 1` (or the `SelectStyleSequence+0x288` flag) routes the
session through the matching scene chain, which:

- blocks in `MatchingDancePlaySequence` state 2 until `DAT_1806f2d48` (the matching
  session actor) reports state 4 (connected) — requires a live `CNetworkManager`
  session (LibComm TCP/UDP on 6198, host/guest handshake, `DAT_1806f3918` role);
- runs the music-start sync handshake (`FUN_1801bc890`, 3 s timeout, then "FAILED
  TO MUSIC START SYNC" degraded start);
- forces EX-score rules (`FUN_1801ea320` → 1) and the `+0xD0`-keyed result /
  total-result / logout chain (`quick_logout_research.md` §3), plus BPL headers,
  `bgm_bpl`, `in_battle` result badges;
- fires `/coin/match` billing client-side once a match is confirmed.

A loopback second "cabinet" (running the LibComm protocol against ourselves) would
make all of that work but is a network-emulator project, not a HUD mod, and it
changes score semantics. Not worth it for the display.

## 4. Behaviour and interaction notes (for the design phase)

- **Gating:** local versus only — `GameWork+0 == 1` AND both `stage_records::side_entered`.
  Single-player, doubles (`GameWork+0 == 0`) and course/`event_mode()` sessions:
  inert. Training-mode and song-rate sessions are display-only affected (scores are
  whatever the actors hold). Autoplay taint does not matter (pure display).
- **Score type follows the cabinet:** the frame shows money score (max 1,000,000)
  unless the operator enabled `use_ex_score`, in which case it shows EX vs the
  chart's EX max exactly like BPL play — because we pass the game's own
  `GamePlayActor+0x1D0`.
- **Toggle semantics:** enable/disable applies at the next song (creation happens
  once per song). Mid-song disable could send the actor 0x104 (finalize) and unlink
  it from the DPS child list; simpler to leave it until song end.
- **Failure modes are all fail-open:** any missing signature ⇒ mod absent; ctor
  returns fine with null network blocks; a missing `matching_usr` marker (custom
  `dance_common`) ⇒ scale-0 frame (invisible), no crash; a missing
  `dance_matching.arc` ⇒ `main_single` clip NULL ⇒ **crash in stock onInitialize**
  — the slot-4 wrapper must pre-check `*(*DAT_1806f2d70 + 0x7F0) != 0` (slot 31
  package pointer) and skip the stock call (and mark the actor finished) when the
  package is not resident.
- **Overlap with other mod-owned HUD:** the frame sits at the `matching_usr` anchor
  (stock layout puts it between/above the lanes). power_user_statistics widgets
  (P1 x=80 / P2 x=1200, y=425), the training strip and the S-Marv flashes may
  overlap — cabinet check. In stock BPL play the normal per-player score readouts
  stay visible alongside the frame (only `ScoreActor`'s pacemaker target changes via
  the rival-sync path when a matching flag is set — which we do not set).
- **`overlay_element_styling` / `playfield_styling` captures** key on
  `dance_judge`/`dance_filter_*`/etc. clip names and `note_result_setup`; the frame's
  clips (`score_*p_usr`, `gauge_*p_usr`, `base_usr`) do not collide.
- **Clip-name registry:** `onInitialize` registers `score_%dp_usr`/`gauge_%dp_usr`
  ids in the global 0x400-entry name table (`DAT_180c2b340`/`DAT_180caf340`) that
  the `LayoutActor` also registers `score_1p_usr`… into. Stock BPL play has the
  same duplication, so it is tolerated by design.
- **Per-side timing offsets, S-Marv, judge hooks:** untouched — the frame reads
  scores post-hoc, never participates in judging.
- **Versus mirror / 2P training:** nothing to mirror; there is no option row
  beyond the on/off toggle (a `PersistMode::None` cabinet-wide bool in the Mods tab).

## 5. Cross-build notes

- Constructor, `BATTLE_INFO` layout, `+0x1B0`, `+0x1B8` widgets, `+0x278`: byte-
  and offset-identical on 20250805 (`FUN_18006df80`, GameWork global `DAT_1806b42a8`,
  `CNetworkManager` at `DAT_1806b6670`) and 20260825. The only build-varying parts
  are the RIP-relative globals, which the AOB decodes.
- `GamePlayActor+0x1D0/1D4/1D8` are below the `+0x208` layout fork (AGENTS.md
  "Build-dependent actor layouts") ⇒ stable; the DPS child offsets
  (`+0x100/+0x108` actors, `+0x110` LayoutActor on the normal DPS; `+0x128/+0x130`,
  `+0x138` on the matching DPS) are NOT to be hardcoded — walk the child list by
  vftable as `song_reset` does.
- `dance_matching` mask 0x9000 and the normal loader's 0x8000 are the same on all
  builds the `gameplay_loader_masks` signature already validates.

## 6. Open questions / cabinet verification list

1. Visual: where exactly `matching_usr` places the frame in the stock `dance_root`
   layout, and whether the stock per-player score readouts should stay (stock BPL
   keeps them) or be hidden for a cleaner look (would be a second, optional step —
   the `ScoreActor` clips are already captured by other mods).
2. Confirm on cabinet that `onInitialize`'s callees do not read `GameWork+0`
   (a boot-time `DAT_1806f14f8` xref scan over the callee set is cheap insurance)
   and that the flip leaves no log noise.
3. Guest handling: `ddrcode` is only logged, so guests (`PlayerWork+0x18 == 0`)
   can carry `-1` safely; the name falls back to `"PLAYER1"/"PLAYER2"` exactly like
   the stock HUD (`FUN_1801e88a0`). Decide whether to blank the name board for
   guests instead.
4. Does the `main_single` layout expect the local player at position
   `GameWork+0x8` (stock: local side) — i.e. is board 1 always drawn on the left?
   If the art is symmetric this is moot; else map position 0 ← side 0 explicitly
   (what the plan above does).
5. Phase-2 candidates (not required for the HUD): the result-screen rank badge
   (`ResultSequence` `FUN_1800cb570`, `in_battle`/`battle_rank_usr`, gated on
   `GameWork+0xD0 ∈ {1,2}`; it indexes texture tables by the record fields
   `rec+0x5E0/+0x5E4` — the first two fields the matching DPS copies into the
   network stage-result record, presumably rank/result — not decoded further) and
   the `vo_battle_style_*` announcer lines.

## 7. Address reference (20260825)

| Item | Address |
|---|---|
| `MatchingDancePlaySequence::onUpdate` | `FUN_180061cc0` (vtable `+0x30`); frame alloc/ctor at case 2 tail; local score write case 0xB |
| `DancePlaySequence::onUpdate` (normal) | `FUN_180057e10` — case 1 creates `GamePlayActor`s with `param_10 = 0` |
| `GamePlayActor` ctor | `FUN_18005ae30`; `+0x84` side, `+0x1D0` isEx, `+0x1D4` money score, `+0x1D8` EX score, `+0x2B7` matching flag (build-forked region) |
| `MatchingBattleFrameActor` ctor / vftable | `FUN_180071740` / `0x1803626f8` (`BATTLE_INFO::vftable` `0x180362748`) |
| onInitialize / onUpdate / onDraw / onFinalize | `FUN_180071ce0` / `FUN_180072e20` / `FUN_180072f70` / `FUN_1800735f0` |
| rank + diff | `FUN_180073650` (qsort cmp `LAB_180073630`) |
| `MatchingBattleInfoActor` ctor / vftable | `FUN_180073a30` / `0x180362778` (msg 0x1036 → `DAT_1806f2d50[side]`, unread) |
| `LayoutActor` vftable / onUpdate / layout builder | `0x180361c98` / `FUN_18006bb30` / `FUN_18006bd40` |
| layout descriptor lookups | `FUN_18006ece0` (package map `+0x10`), `FUN_18006f100` (position map `+0x30`), `FUN_18006f020` (insert) |
| scene package table / loader | `0x18035AD50` / `FUN_1801ac930`; slot 31 pkg ptr `*(*DAT_1806f2d70 + 0x7F0)` |
| `agcs::Actor::vftable` / system dispatcher / addChild / broadcast | `0x180389cc8` / `FUN_18021d7d0` / `FUN_18021f230` / `FUN_18021eeb0` |
| `agcs_heap_malloc` / `agcs_heap_free` / app heap handle | `FUN_18021f300` / `FUN_1801de6e0` / `DAT_180466030` |
| `GameWork` global | `DAT_1806f14f8` (`+0` versus, `+4` style, `+8` primary side, `+0xC` stage, `+0x18` mcode, `+0x70` course, `+0xD0` event mode) |
| `FUN_1801ea320` use-EX-score | reads `GameWork+0xD0` then `/gameOptions/use_ex_score/current` |
| `PlayerWork` name / ddrcode / getName | `+0x0C` / `+0x18` / `FUN_1801e88a0` |
| `SelectStyleSequence::onUpdate` (BPL flag → `GameWork+0xD0`) | `FUN_1800b0bc0` |
| `CNetworkManager` | `DAT_1806f38f0` (+4 sync state `DAT_1806f38f4`); role `DAT_1806f3918` (0 host / 1 guest); local cabinet idx `DAT_1806f391c`; stage `DAT_1806f3920`; cabinet blocks `DAT_1806f3ac8`/`DAT_1806f3ad0`; init `FUN_1801bbbe0`; `GetPlayerInfo` `FUN_1801bccb0`; `StartLocalMatchingSearch` `FUN_1801bc4d0` (local record built by `MatchingCautionSequence`'s ChildActor `FUN_1800a7410` case 1) |
| UDP score sync | `ReceiveScoreReport` `FUN_1801c3470`, `ReceiveScoreNotice` `FUN_1801c3520`, `ReceiveMusicStartSyncReport` `FUN_1801c3370` |
| matching session actor (state 4 = connected) | `DAT_1806f2d48` |
| art tables | `dama_score_base_*` `0x180465aa0`, team `0x180465a60`; `dama_gauge_*` `0x1804659e0`, team `0x1804658e0`; `dama_score_1st..` `0x1804658c0` |
