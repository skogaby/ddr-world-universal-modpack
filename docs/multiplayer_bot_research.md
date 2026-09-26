# Multiplayer Bot — RE research notes

Durable reverse-engineering facts behind `src/mods/multiplayer_bot/` and
`src/services/foot_panel_swap/`. Addresses are file-relative to `gamemdx.dll`'s base
`0x180000000`, build **20260825** unless stated (cross-checked on 20250805, the other end of the
supported range). Globals named here (`game_work_global`, `player_work_table`,
`audio_manager_global`, `auto_foot_panel_vtable`) are derived at runtime by
`stage_records` / `signatures.rs` — never hardcoded. The judge acceptance rule, the freeze
judge, `judge_submit`'s bookkeeping and the NORMAL gauge's exact integer formula are
transcribed in `docs/gauge_and_judge_scoring_research.md` (the `tools/bot_sim` oracle) and
are only summarised here.

Planning record: `.agents/planning/2026-09-13-multiplayer-bot/` (design Appendix A/B, the
per-step research notes, `progress.md`).

## 1. The two load-bearing facts

1. **Player count is one byte per side.** `TransitionSequence::createNextSequence` case
   `0x1d` (0-idx scene 28) builds, per side, `{entered = *(u8*)(PlayerWork+0x4), is_main =
   (GameWork+0x8 == side), is_double = (GameWork+0x4 == 1), difficulty = clamp(PlayerWork+0x5C),
   0}` (`FUN_18002e3b0` @ `0x18002fb03..`; byte-identical on 20250805 `FUN_18002e140`), hands both
   to `DancePlaySequence::ctor` (`0x1800570a0`, heap-copied to `DPS+0xF0/+0xF8`), and
   `DancePlaySequence::onUpdate` (`0x180057e10`) case 1 creates a `GamePlayActor` for each side
   whose `entered != 0` (`DPS+0x100/+0x108`; LayoutActor `DPS+0x110`). Everything else in the DPS
   (SSQ path build in `onSetup`, layout style array, song-info actors) runs for both sides
   regardless. ⇒ setting `PlayerWork[bot]+0x4 = 1` before scene 28 is created yields a second
   `GamePlayActor` natively.
2. **Autoplay is an input object, not a flag.** `AutoFootPanel` implements the `IFootPanel`
   interface the judge queries every frame; a DLL-owned object with the same vtable shape whose
   `getPressAge` returns `current_mc − planned_event` places every graded event exactly where a
   skill model decided — on every build (§5).

## 2. `PlayerWork` / `GameWork` header

Build-invariant except where marked (the chart-identity triple moved: see §12.1).

| Object | Offset | Type | Meaning / use |
|---|---|---|---|
| `GameWork` (`**game_work_global`) | `+0x0` | i32 | versus word: 0 solo/doubles, 1 versus — WRITTEN 1 at the flip, restored |
| | `+0x4` | i32 | style (0 single / 1 double) — gate |
| | `+0x8` | i32 | primary side |
| | `+0xC` | i32 | stage counter (`stage_records::stage_counter()`) |
| | `+0x59` | u8 | extra stage granted (§7) |
| | `+course_field_offset()` (0x70) | u64 | course pointer — gate |
| | `+0xD0` | i32 | event mode — gate |
| `PlayerWork[side]` (`*table[side]`) | `+0x4` | u8 | entered — WRITTEN 1 at the flip, restored |
| | `+0x5` | u8 | registered (carded) player — gates the network rival/event calls |
| | `+0x8` | i32 | payment (−1 never entered) — the EAM-exit settle-up requires `>= 0` |
| | `+0xC..+0x14` | char[8+1] | name — WRITTEN (`BOT LV<n>` / the Target Score target's name / `TARGET`), restored |
| | `+0x1C` | i32 | "saves" flag the per-stage `SavePlayerDataActor` waits on |
| | `+0x50/+0x54/+0x5C` (20260324+) · `+0x60/+0x64/+0x6C` (20250805, 20260224) | i32 | style / committed mcode / selected difficulty — MIRRORED from the human; **build-dependent**, derived (`pw_chart_*_off`, §12.1) |
| | `+player_option_offset()` (0xE0 new / 0xF0 old) | `ddr::player::Option` | `+0x08..=0x6C` COPIED from the human, `+0x18` gauge forced 0 |
| `record[stage]` (`stage_record(side, stage)`; base 0x590 new / 0x570 old, stride 0x2B8) | `+0x00/+0x04/+0x08` | i32 | mcode / difficulty / style — `+0x04/+0x08` MIRRORED |
| | `+0x50` | i32 | rank (extra-stage AAA test `>= 0xF`) |
| | `+0x54` | i32 | clear kind |

Only two writers of `PlayerWork+0x4` exist in the binary (byte pattern `C6 ?? 04 01`):
`EAmEntryWindowActor::update_ARK_ENTRYFLOW_CREDIT_READY` (`FUN_180093b60` @ `0x180093e0d`,
scene 19) and the BPL guest-join helper (`FUN_1800b2d10`, from the SelectStyle START handler when
VERSUS is chosen with an empty side) — the latter produces exactly the "cardless entered side"
shape the bot uses (`PlayerWork::reset` `FUN_1801e7fb0` leaves `+0x4 = 0, +0x5 = 0, +0x8 = −1,
+0x18 = 0, +0x1C = 0`, records `(mcode −1, diff 0, style 2)`, then `+0x4 = 1`). Nothing in
scenes 24–35 writes `+0x4`.

## 3. The versus word and its readers

`SelectStyleSequence::onUpdate` (`FUN_1800b0bc0`) STEP_WAIT case 2 is the ONLY writer besides
`GameWork::reset`: `count = 2 − sides that made no selection`; `count == 2 ⇒ GameWork+0 = 1,
GameWork+0x4 = 0` (versus forces SINGLE), else `GameWork+0 = 0, +0x4 = chosen style`. Every
play-window reader is a display/layout selector, so writing 1 at the flip and 0 at the restore is
sufficient:

| Reader | Effect of `GameWork+0 == 1` |
|---|---|
| `ResultSequence` build (`FUN_1800b9030`, scene 30) | side *i*'s info pane / profile / tab populated iff `GameWork+0 == 1 \|\| i == primary`; the tab's versus byte `tab+0x134` |
| `ResultSequence::onUpdate` (`FUN_1800bc120`) cases 2 / 0x11 | same predicate (BPL rank write-back, per-side detail rows) |
| `createNextSequence` cases 0x16 / 0x24 / 0x2e / 0x3b | `FUN_1801aa500(GameWork+0 == 1)` → the SE pan byte (§3.1) |
| `ReflectSavePlayerData` (`FUN_180018ee0`) | staging `+0x44` (`savekind==1 ? 2 : GameWork+0`) and `+0x12C` → wire `/data/mode`, `/data/battle_mode` — the human's per-stage save carries `1` in bot sessions; neither bemani-buddy nor bemaniutils reads them |
| `two_player_bpl_mode` (DLL) | eligibility gate + layout selector (engages against the bot by design) |

### 3.1 SE pan byte (cosmetic, D22)

`FUN_1801aa500(u8)`: `MOV RAX,[rip+audio_manager_global]; MOV byte [RAX+0x20C4],BL` —
byte-identical (disp32 `0x20C4`) on all four builds; `audio_manager_global` is
`derive_audio_manager_and_play`'s RIP decode (`+0x6F2D68` on 20260825) and `game_audio` holds
it (`game_audio::{versus_pan, set_versus_pan}`). Reader `FUN_1801aa220`: while the byte is set,
side 0 SEs pan `DAT_18035a704`, side 1 `_DAT_180359f64`, centre otherwise. Stock writes it only
at scene-chain boundaries the flip never crosses, so the impersonation writes 1 after the versus
word and restores the snapshot at the window exit (unavailable manager ⇒ stock pan, never a
refusal).

## 4. Song-select commit and the bot's record

Commit `FUN_1800fdc90`: for BOTH sides, entered or not, `rec = record[stage_counter]`; `diff =
FUN_1801a7880(model, *(seq + 4 + side*4))` — that side's OWN cursor; `if mcode != rec->mcode {
FUN_1801e6010(rec, mcode, diff, GameWork+4) /* header + wipe */; FUN_1800fcc70(seq, diff, side)
/* PW+0x5C */ }`. The non-entered side's cursor is unset in 1P ⇒ its record carries the WRONG
difficulty unless fixed, hence the flip mirrors `rec_bot+0x04/+0x08` and `PW_bot+0x50/+0x54/
+0x5C` from the human, and REFUSES when `rec_bot+0x00 != rec_h+0x00` (the commit did not prepare
both). Downstream readers of the bot's header: the READY panel (`rec+0x04`), the loader
(`PW+0x5C` → DPS struct → `build_ssq_path`), the results pane (`rec+0x04/+0x50/+0x54`).

## 5. `AutoFootPanel` and the judge's algebra

7-slot vtable on every build (`auto_foot_panel_vtable` by RTTI; `0x18035c938` on 20260825,
`0x18033d618` on 20250805; object 0x58 / 0x40):

| Slot | Body |
|---|---|
| 0 | dtor |
| 1 `update(this, &results, cur_beat, mc)` | presses panels whose state is 1 (TRG) or 4 (REP freeze head), holds `state ≥ 2` panels while `cur_beat < note.beat + max(length)`, avoids shocks by pressing the OTHER panels; stamps the press time (20260721+: libavs ordinal-45 clock back-dated so the event lands on `note.mc`, `qword[8]` at `+0x18`; older: `timeGetSystemTime().ms`, `dword[8]`) |
| 2 `wasJustPressed(this, panel)` | `*(u8*)(this+0x10+panel)` |
| 3 `isHeld(this, panel)` | `*(u8*)(this+0x08+panel)` |
| 4 | thunk → slot 5 |
| 5 `getPressAge(this, panel)` | `now − pressTime[panel]` (clock + stride differ by build) |
| 6 `consumePress(this, panel)` | zero the slot |

`judgeNotes` (`0x18005EC00`), per frame with `fp = *(actor + judge_hook::foot_panel_offset())`
(0x278 new / 0x270 old): `held_mask = OR isHeld(i) << i`; for each unjudged kind-0 note in order —
Miss when `note.mc + 160 <= mc` (grade 5, submit `0x102D`; shock 6 / `0x1030`); stop when `mc <
note.mc − 260`; for each held panel with `state ∈ {1,4}`: `event = mc − getPressAge(i)`, matched
to the EARLIEST unjudged note carrying that arrow WITHOUT a window test; jumps need a second
panel within 66 ms; shocks are N.G. on a `wasJustPressed` inside `[−34, +84]`. Grade = first
window containing `event − note.mc` from the table at `0x18035B9E0` (Marvelous ±17, Perfect ±34,
Great ±84, Good ±124, [±160]) — accepted only when `grade < min(best_this_frame, 4)`: **the ±160
row can never win (World has no Boo)**, a rejected match leaves the note unjudged and the press
unconsumed, and exactly ONE note is accepted per actor per frame. Nothing checks `event <= mc`.

Consequence (the controller): `event = mc − (mc − E) = E` for a panel whose `getPressAge` returns
`CURRENT_MC[side] − event_mc[panel]` — exact, build-independent, and offset-agnostic (JUDGEMENT
OFFSET / SOUND OFFSET shift `mc`, and `E` is expressed in the same base). The DLL clones the stock
vtable (COL at `[-1]`, slots 0–4 verbatim, 5/6 replaced) into an RWX region holding both
`BotFootPanel` objects (`services/foot_panel_swap/layout.rs`); the judge pre (`Priority::Late`) /
post (`Early`) pair swaps the side's slot for the duration of `judgeNotes`. The planner's two
non-negotiable rules follow from the judge: per-panel monotonic events with `blocked_until` after
a decided Miss (else the next same-panel press is attributed to the missed note), and one live
event per panel per frame.

## 6. `ddr::player::Option` (setters' debug strings, `FUN_1801e1b90..FUN_1801e23b0`)

`+0x08` SpeedType, `+0x0C` Hispeed, `+0x10` derived multiplier, `+0x14` ScrollSpeed, **`+0x18`
Gauge (0 = NORMAL)**, `+0x1C` ScrollDirection, `+0x20` TimingDisp, `+0x24` TimingMusic,
`+0x28` Visibility, `+0x2C` ConstantValue, `+0x30` LaneTransparency, `+0x34` LaneCover, `+0x38`
FastSlow, `+0x3C` Guideline, `+0x40` Stepzone, `+0x44..+0x54` draw order / layout / notice /
scroll-moving, `+0x58` ArrowPlacement, `+0x5C` ArrowColor, `+0x60` ArrowDesign, `+0x64/+0x68/
+0x6C` CutTiming / CutFreeze / CutJump (chart-altering — copying keeps chart parity), `+0x90`
f64 BPM. The vtable at `+0x00` is never copied.

## 7. Extra-stage grant (`extra_stage_grant`)

`FUN_1801ddcd0(int arg)` (`0x1801c6970` on 20250805, `0x1801ca7e0` on 20260224, `0x1801dd0b0` on
20260721): gated on `GameWork+0x59 == 0`, `arg == 0`, `GameWork+0x70 == 0`, `GameWork+0x4 != 1`,
`max_stage + 1 == 3`; then for EVERY side with `PlayerWork+0x4 != 0` requires `record[0]+0x50 >=
0xF`, `PlayerWork+0x1710 == 0`, gauge option ∈ {0, 0xC}, `record[0]+0x270 != 7`; on success
`GameWork+0x59 = 1`. Called from `ResultSequence::onUpdate` case `0x16` (results window-out)
when the stage counter is 0 — inside the bot's play window. The bot can never ADD a grant, only
block one, so `extra_stage_guard.rs` detours the entry (prologue AOB, unique on all four builds)
and clears the bot's entered byte around the original.

## 8. Saves for the cardless side

`SavePlayerDataActor::onUpdate` (`FUN_1800b53c0`) waits while `PW+0x1C != 0 && FUN_1801de420(2)
&& !(rec+0x1A4)`, then requires the ark entry-flow side-state check (`FUN_1800139a0`) before
sending — a never-entered side sits at ark scene 0 so the send never fires; the `ResultSequence`
ctor's network rival/event calls are gated on `PW+0x5 != 0` (the bot skips them). The bot side
additionally carries the autoplay score taint (`score_guard::set_autoplay_taint`) as the backstop:
`savekind==2` suppressed, `savekind==3` sanitised.

## 9. Phantom-player governance (2026-09-13 interaction audit)

During the window BOTH sides read `stage_records::side_entered == Some(true)`, but
`versus_mirror` never engages (it engages only at song select with both entered;
`scene_manager` updates `current_scene` BEFORE the callbacks fire, so the flip lands with the
scene already 26). Policies written for real versus — "P1 governs when both entered", which the
mirror makes value-neutral — therefore read the BOT side's rows (the JSON cache of whoever last
used that pad) whenever the human is on P2. Fixed by `multiplayer_bot::is_bot_side(side)`:

| Consumer | Without the fix (human on P2) | Now |
|---|---|---|
| `premium_free::effective_freeze` (re-resolved every scene change) | the stage-bump NOP during scene 31 followed P1's stale cache | bot side ⇒ not entered |
| `training_mode::pre_shift_side`, `bounds::try_resolve_row_bounds` | a stale P1 LOOP SONG cache pre-shifted / looped the human's song | bot side skipped |
| `timing_offsets::calibration` census | refused as "2P" | bot side ⇒ not entered |
| `assist_tick` GAMEPLAY latch | a stale bot-side `assist_tick = ON` played a clap track | bot side never enabled |
| `announcer_mute::effective_mute` | the bot side's cached mute governed | bot side ⇒ not entered |

Left as-is (cosmetic or intended): `training_mode::strip_hud::latch_placement` uses P1's timeline
placement; PUS may show a stats widget for the bot; S-Marvelous paints the bot's results pane;
`two_player_bpl_mode` engages against the bot with the bot's name (D9); `song_rate`'s scene-26
classifier fires BEFORE the flip on the same edge (service callbacks register before the mods')
so it sees one entered side; `per_song_judgement_offsets` writes the bot's `Option+0x24` at first
dispatch — harmless (§5). Quick restart (28→27→28) re-seeds the bot; quick fail / any exit from
{26..=30} restores.

## 10. Scene edges (0-indexed)

25 SONG_SELECT → {26 interstitial, 27 stage indicator, 28 GAMEPLAY}: flip (the scene callback
fires BEFORE the original `createNextSequence`, so the entered byte is set before the 27/28
loaders read it). Play window {26..=30}; 29 is the post-song loader, 30 the `ResultSequence`.
First scene ∉ window (31 stage-bump wait, 24 select loader, 32 TOTAL RESULTS, 34 EAM exit):
restore. Pinned by `mods/multiplayer_bot/session.rs` (host-tested) against `types::scenes::scene`.

## 11. Addendum 2026-09-14 — Target Score tier (ghost replay)

The eleventh value of the BOT LEVEL row replays the HUMAN's loaded pacemaker ghost note for
note. Research pass on gamemdx 20260825 (Ghidra) with per-build checks on 20260224 / 20250805;
the signature sweep covers 20260721.

### 11.1 The ghost is FINAL before the bot's first judge frame

`GamePlayActor::onUpdate` (`FUN_18005cc70` @20260825) state 2:

```
18005d186  48 8B 8F F8 01 00 00   MOV  RCX,[RDI+0x1F8]      ; GhostActor*
18005d18d  48 85 C9               TEST RCX,RCX
18005d190  74 0D                  JZ   advance
18005d192  E8 rel32               CALL GhostActor::isReady   ; FUN_1800569b0
18005d197  84 C0                  TEST AL,AL
18005d199  0F 84 ...              JZ   keep_waiting          ; stays in state 2
```

`isReady` = `state[idx] == 2` (state pairs `+0x58 + idx*8`, idx u16 `+0x82`), else a
`TIMEOUT_GHOST` clock (default `0x7fffffff`). `GhostActor::onUpdate` (`FUN_180056d10`) reaches
state 2 on download success (decodes the wire string into `+0x98`, raises the pacemaker
visibility byte `*(+0x88)+0xC0`), on download failure, AND on request failure. The actor
therefore cannot reach its judging state (4) until the ghost vector has its final shape — the
Target tier needs no "ghost arrives late" handling.

### 11.2 The GhostActor field is build-dependent — derive it

| build | wait site | `GamePlayActor` field |
|---|---|---|
| 20260825 | `0x18005d186` | `+0x1F8` |
| 20260721 | `0x18005d1f6` | `+0x1F8` |
| 20260224 | `0x180058dc6` | **`+0x1F0`** |
| 20250805 | `0x180059d86` | **`+0x1F0`** |

The GamePlayActor layout fork (AGENTS.md's "≥ ~0x208") actually sits at `+0x1F0`. Signature
`gpa_ghost_actor_probe` = `48 8B 8F ?? ?? 00 00 48 85 C9 74 ?? E8 ?? ?? ?? ?? 84 C0 0F 84`
(unique on all four); `derive_ghost_actor_probe` publishes the disp32 at match+3 as
`gpa_ghost_actor_off` ONLY IF the CALL target at match+12 contains, in its first 0x30 bytes,
`0F B7 81 82 00 00 00 48 8B D9 83 7C C1 58 02 74` (`MOVZX EAX,[RCX+0x82]; MOV RBX,RCX; CMP dword
[RCX+RAX*8+0x58],2; JZ` — byte-identical on 20250805 and 20260825), which both identifies
`isReady` and re-attests the state layout the consumer reads. RTTI
`.?AVGhostActor@dance@sequence@@` → `ghost_actor_vtable` is the runtime identity gate.

### 11.3 Ghost alphabet, alignment, backend

One grade-class byte per Results entry in chart order (the stage record's `+0xB8` stream,
written by the result commit from the `+0xB0` ring; wire `<ghost>` = `'0'+grade`): 0 Marvelous,
1 Perfect, 2 Great, 3 Good, 4 Boo (never produced by World's judge), 5 Miss, 6 O.K. (freeze held
/ shock avoided), 7 N.G. (freeze dropped / shock stepped). Freeze tails and shocks are Results
entries, so their bytes sit at their own indices. bemani-buddy hands EVERY score a ghost id (own
PBs via `playerdata_load` `score_str` field 6, rival/world/area/machine via `rivaldata_load`'s
last field) and `ghostdata_load(id)` returns the stored string — so any selectable target has a
ghost unless the saving play had none.

### 11.4 Reproduction rules (pure, host-tested: `ghost.rs`, `planner.rs`)

- Tap byte 0–3 ⇒ uniform |offset| inside that grade's inclusive window (Marvelous `[0,17]`,
  Perfect `[18,34]`, Great `[35,84]`, Good `[85,124]`), side from the skill model's sticky
  Markov chain; 5 ⇒ Miss; 4 and 7 ⇒ Miss (unreproducible); 6 on a tap ⇒ Marvelous.
- S-Marvelous floor: were the S-Marv mod armed at window `W` on the bot side, Marvelous
  samples `[W+1, 17]` (`W=16` ⇒ exactly 17). Since 2026-09-25 the side is excluded from
  classification instead (§12.4), so `W` reads 0 and the floor is only a guard. Levels 1–10
  untouched.
- Freeze N.G. (byte 7 at the tail): the planner drops the body hold on the head AND tail
  entries (both emit it) — the freeze judge (§2 of the gauge/judge RE) resolves N.G. as soon as
  any body panel was released. Head↔tail link: first later kind-2 entry at
  `head.beat + max(length)` sharing a body panel (bounded 512 entries).
- Shock N.G. (byte 7 at a shock): ONE `wasJustPressed` on the first shock panel from
  `note.mc` (inside the judge's `[mc−34, mc+84]`); `isHeld` stays 0 so the tap judge ignores it.
- Money score is a pure function of grade counts ⇒ a faithful replay ends on exactly the
  target's points (`repro_miss` in the restore INFO counts the planner-floor exceptions).

### 11.5 Fallback + persistence

No usable ghost at the first fill (empty / id 0 / failed download / `len ≠ results` / derivation
missing) ⇒ the song plays at Level 10, one WARN with the reason, 3 s toast `NO TARGET GHOST -
BOT LV10`; the plate keeps whatever the flip wrote (the target's name + TARGET BOT label, or
`TARGET` — §12). The impersonation is applied at song select — before the
GhostActor exists — so the flip cannot refuse ahead of time. Both bot rows are
`PersistMode::Local` (JSON cache in both directions, never on the wire; the load gate is split by
`LoadSource`), and the level row renders text via `ScalarFormat::Labeled` (`Level 1`…`Level 10`,
`Target Score`) on the scalar donor — no chip textures.

## 12. Addendum 2026-09-25 — Target Score presentation (target name, TARGET BOT label, S-Marvelous)

Research pass on gamemdx 20260825 (Ghidra) with per-build checks on 20250805 / 20260721 /
20260915; the signature sweep covers all five builds (20260224 included).

### 12.1 The game's TARGET-option lookup — and the PlayerWork chart fields

`i64 ghost_id(int side)` (`FUN_18001dc90` @20260825, `0x18001d6c0` @20250805, `0x18001dc00`
@20260721, `0x18001de70` @20260915) is what `ghost_actor_init` calls (see
`docs/premium_free_stale_record_bug.md`, 2026-09-16 addendum, for the switch). Byte-identical on
every build apart from its displacements, which make it the attested source of three
`PlayerWork` fields that turned out to be BUILD-DEPENDENT:

| field | 20260324+ | 20250805 / 20260224 | also read by |
|---|---|---|---|
| style (`CMP [PW+s],1` → doubles clamp difficulty ≥ 1) | `+0x50` | `+0x60` | the DPS loader's per-side difficulty getter (`FUN_1801d02f0` @20250805 reads `+0x60`/`+0x6C`) |
| committed mcode | `+0x54` | `+0x64` | |
| selected difficulty | `+0x5C` | `+0x6C` | createNextSequence case `0x1d` (the DPS ctor struct) |
| TARGET option | `+0x1328` | `+0x1308` | |
| rival codes 1..3 | TARGET `+4 .. +0xC` | same | |

Signature `ghost_id_lookup` (the function entry, 251 bytes through the rival scan);
`derive_target_name_sites` stage 1 cross-checks the image-base `LEA` and the table disp against
`player_work_table` and publishes `pw_chart_{style,mcode,diff}_off` — the impersonation's
chart-identity mirror now uses them (it hardcoded the 20260324+ triple, so on the two old builds
it wrote the human's chart into three unrelated bot fields that the restore never puts back, and
the bot's DPS read a stale difficulty). Stage 2 publishes `pw_target_select_off`, the rival-set
global and the score-entry callee (`CALL` at `+0x1C9` behind its exact argument setup, prologue
attested).

### 12.2 Rival / ranking sets

`G` (`rival_sets_global`, `DAT_1806f1500` @20260825) → owner → container `{vector<Set*> begin +0,
end +8, …, default set +0x20}` (built at boot, destroyed by `FUN_1801ef320`). A set:

| offset | meaning |
|---|---|
| `+0x0` | kind: 0 / 1 / 2 = the three ranking loads (`rivaldata_load` loadkind 1 / 2 / 3 — WORLD / AREA / MACHINE in bemani-buddy's naming), 3 = rival |
| `+0x8` | load timestamp |
| `+0x10/+0x18` | `map<mcode, ScoreRow>` (`_Isnil` +0x201); value +0x20: 10 × 0x30 entries indexed `style*5 + diff`: `+0` score, `+4` rank, `+8` clear kind, **`+0x10` ghost id** |
| `+0x30/+0x38` | ranking sets: `map<mcode, HolderRow>` (`_Isnil` +0xE5); value +0x1C: 10 × 0x14 `{ddr code, area, name[12]}` — the best score's holder (written only when the score improves) |
| `+0x50` | rival set: DDR code |
| `+0x54` | rival set: area |
| `+0x58..+0x60` | rival set: name (8 chars + NUL, `FUN_1801ee860`) |

The parser (`FUN_18001cce0` rankings, `FUN_18001d5d0` rivals) copies at most 8 name chars.
Getters (all pure readers): `score_entry(set, mcode, style, diff)` (`FUN_1801ee220`) and
`dancer_name(set, mcode, style, diff)` (`FUN_1801ee8c0`, signature `rival_set_dancer_name` —
kind 3 ⇒ `set+0x58`, else the holder row's name, `""` when absent; the song select's
`target=%d, label=%s, …, dancername=%s` lambdas use exactly this pair).

`target_name::resolve` (at the flip, before any write): read TARGET; own best ⇒ the human's name;
rival n ⇒ the kind-3 set whose code matches; 4..6 ⇒ the set of kind `n−4` — found by the SAME
first-match search the lookup does, over a fully probed container (the lookup walks it unchecked
and falls back to the default set / reads past the end when absent). Then `ghost_id(human)`
(0 ⇒ no pacemaker ⇒ `TARGET`), then `score_entry` must carry that same ghost id before
`dancer_name` is trusted.

### 12.3 Where the plate is drawn, and why a second label

The name buffer is `char[9]` at `PlayerWork+0xC` (`PlayerWork::reset` zeroes `+0xC..+0x14`;
`+0x18` is the next field), read inline by ~10 routines plus one `getName` (`FUN_1801e88a0`), so
it cannot grow. Every reader builds a `sequence::SpriteLayer` of per-character bitmaps through
`FUN_1801d3240(text, "<prefix>%s")` / `FUN_1801d2b50` (letters lower-cased; `& , $ . ! - ? % + /
~` spelled out; anything else `blank`). `common_texture_v3` ships only A–Z, 0–9, `ampersand
blank dollar exclamation hyphen period question` for both sets — no parentheses, no brackets.

| surface | owner | clip / anchor | glyphs | stock SpriteLayer setup |
|---|---|---|---|---|
| gameplay | `sequence::dance::ScoreActor` init (vslot 4, `FUN_1800775d0`) | `dance_name` clip at `+0x88`, anchor `name_usr`; SpriteLayer shared_ptr `+0x90` | `cote_edge_*` | priority 1, align (0,0), fit-to-anchor |
| stage results | `ResultSequence` setup (`FUN_1800b9030`) | main clip `RS+0x108`, anchor `player_Np_info_usr/profile_usr/player_name_usr`; SpriteLayer `RS+0x400+side*0x10` | `cote_shadow_*` | priority 0, align (1,1), fit |

(`FUN_1800f9a00`'s `cote_shadow` rows are the results ranking tab, not the plate.)
`plate_label.rs` adds one process-lifetime SpriteLayer per surface on the same parent + anchor
(ScoreActor found by a bounded DPS-tree walk, side `**(+0x58)`; the results clip by content),
fixed scale `0.55 ×` the anchor's `0x1016` height × `0x100D` y-scale (the layout's own fit
input, re-measured per frame), top-aligned 2 px above the box. SpriteLayer layout math
(`FUN_1801d38e0`): `x = (W − w)·align_x/2 + pos_x + off_x − W/2`, same for y; alpha from the
anchor's `0x100A`.

### 12.4 S-Marvelous on the replay side

`s_marvelous::state::set_excluded(bot, true)` for the whole Target session (both orders against
the play-scene arm on a direct 25 → 28 edge are handled: the arm clears an excluded side, and the
exclusion clears an armed one). The side is never classified; `last_armed_window` / the song
latch read 0, so the results tab, graph, emblems and upload producer are stock for it; the
data-feed tap re-hides the cabinet-wide FAST/SLOW gate patch on its Marvelous
(`flash::on_excluded_marvelous`); its results pane shows `scre_tab_num_minus` in the shared
7-row sheet's S-MARV slot (the label word is baked into ONE sheet texture both panes share). The
ghost's Marvelous floor then reads 0 — the full stock band.

### 12.5 The bot pane opens on DETAILS

`sequence::result::WindowActor` (ctor `FUN_1800c3a30` @20260825; `+0x5C` "main window" =
`GameWork+0 == 1 || side == GameWork+8`, set by the ResultSequence setup) builds its tab kinds in
vslot 4 (`FUN_1800c3c90`): [3 in the `GameWork+0x18 == 0x9733` event], 0 CALORIES (when
`PlayerWork+0x28`), 6 SIMPLE RESULTS (PlaydataTab `result`), 1 DETAILS (PlaydataTab
`detail_result`), 7 PLAY GRAPH, 4, 2 (not in battle modes / course), 5 (`PlayerWork+0x1678 == 1`).
It opens on the kind remembered in `PlayerWork` — main window `+0x60`, else `+0x64` (`+0x70` /
`+0x74` on 20250805 / 20260224) — with −1 (the `PlayerWork::reset` value) mapping to 0 CALORIES
for a main window and 6 for the other; the 0x9733 event forces 3 and course mode forces 1/0. The
window-out message `0x1003` (vslot 8) writes the current kind back. A bot window is always main
(versus), so a fresh pad opened on CALORIES. The flip snapshots the bot's main-window field,
writes 1, and the restore (after the window-out) puts the snapshot back. Signature
`results_tab_memory_writeback` (the vslot-8 store pair), `derive_results_tab_memory` publishes
`results_tab_{main,solo}_off` after checking the table LEA against `player_work_table`.
