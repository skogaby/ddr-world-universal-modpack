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

## 2. `PlayerWork` / `GameWork` header (build-invariant)

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
| | `+0xC..+0x14` | char[8+1] | name — WRITTEN `BOT LV<n>`, restored |
| | `+0x1C` | i32 | "saves" flag the per-stage `SavePlayerDataActor` waits on |
| | `+0x50/+0x54/+0x5C` | i32 | style / committed mcode / selected difficulty — MIRRORED from the human |
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
