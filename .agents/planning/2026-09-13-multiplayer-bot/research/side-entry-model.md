# Side Entry / Versus Model (Step 2 orientation sub-report)

How gamemdx and this DLL model "a side is entered" and "this is a 2P versus session".
Addresses file-relative to `0x180000000`, build noted where the source doc does.

## `src/services/stage_records.rs` — the DLL's shared decode

| Item | Where | Fact |
|---|---|---|
| Decode source | `:1-31`, `:113-158` | From `stage_record_accessor` bytes: GameWork ptr-ptr global @ +3 RIP, `player_work_table` @ +16 RIP, course-field disp8 @ +23, course-record imm32 @ +36, stride @ +47, base @ +55; `_v1` shape on old builds |
| Build constants | `:10-24` | 20260324+: course field `GameWork+0x70`, course rec `PW+0x2D8`, stride `0x2B8`, base `PW+0x590`. 20250805/20260224: base `0x570`, course rec `0x2B8` |
| `game_work()` | `:377-397` | `**global` |
| `player_work(side)` | `:401-421` | `table[side] → wrapper → PlayerWork`; `side >= 2` ⇒ None |
| `side_entered(side)` | `:423-431` | **`PlayerWork+0x4` byte `!= 0`** |
| `stage_record(side, stage)` | `:435-446` | `PW + base + stage*stride`, `stage < 5`; `+0x00 == -1` = virgin |
| `course_record(side)` | `:450-457` | |
| `stage_counter()` | `:475-488` | `GameWork+0xC` (from the `INC` at `premium_free_stage_inc+3`) |
| `event_mode()` | `:501-508` | `GameWork+0xD0` |
| `final_stage_override()` / `max_stage_setting()` | `:514-536` | `GameWork+0x10` / gameOptions cache |
| `player_option_offset()` | `:86-96` | `Option` inline at `PW+0xE0` (20260324+) / `+0xF0` |

## PlayerWork layout (`ddr::player::Work`)

| Offset | Type | Meaning | Source |
|---|---|---|---|
| +0x00 | i32 | side index | `.agents/planning/20260610-suppress-score-submission/research/2p-options-load-side.md:45` |
| **+0x04** | **u8** | **entered flag** | `stage_records.rs:427-431`; `docs/quick_logout_research.md:281,334,720` |
| +0x05 | u8 | "has e-am data to save" — gates the SAVING window / result deltas, not actor creation | `quick_logout_research.md:301,342-344,368-369` |
| +0x08 | i32 | payment kind: 0–2 credit → `arkExpireCredit`, 3 PASELI → `arkEACoinExpire`; `>= 0` gates settle-up | `quick_logout_research.md:280-283,656,721` |
| +0x0C | `char[9]` | dancer name; getName `FUN_1801e88a0`: entered ∧ name[0]==0 ⇒ `PLAYER1/2` | `docs/in_shop_battle_local_versus_research.md:483-487`; `two_player_bpl_mode/logic.rs:77-100` |
| +0x18 | i32 | ddrcode (0 for guest; set only after profile load) | `2p-options-load-side.md:44,68-73` |
| +0x1C | ? | read by per-stage `SavePlayerDataActor` gating | `quick_logout_research.md:309` |
| +0x24 / +0x28 / +0x30 | s32 / u8 / u64 | weight / is_disp_weight / today_cal | `docs/calorie_weight_profile_research.md:29-31` |
| +0x4C | i32 | folder type / 10 = course | `…/20260727-quick-logout/research/savekind3-marshal.md:30-33` |
| +0x50 | i32 | style 0 single / 1 double (song-select commit) | `s_marvelous/lamp_badge.rs:116-120` |
| +0x54 | i32 | COMMITTED mcode | `docs/premium_free_stale_record_bug.md:136` |
| +0x5C | i32 | display/selected difficulty — cursor writes UNCONDITIONALLY; what the stage loader feeds the DPS | `docs/quick_restart_fail_speedup_research.md:845-861` |
| +0xE0 (0xF0 old) | inline | `ddr::player::Option` (JUDGMENT TIMING at `Option+0x24`) | `stage_records.rs:82-86` |
| +0x178 | obj | score-DB | `premium_free_stale_record_bug.md:158` |
| +0x2D8 (0x2B8) | record | course record | |
| +0x590 (0x570) + stage×0x2B8 | record[5] | per-stage play records | |
| +0x1790 (derived `customize_offset`) | obj | `ddr::player::Customize` | `docs/player_customization_system_research.md:210,218` |

Record header: `+0x00` mcode, `+0x04` difficulty, `+0x08` style, `+0x10/+0x14` score/EX, `+0x28..+0x4C` judge counts, `+0x54` clearkind, `+0x1A4` uploaded, `+0x268` end-time, `+0x270` folder (`premium_free_stale_record_bug.md:23-37`).

Both PlayerWork objects exist in a 1P session — the song-select commit writes BOTH sides' `record[0]` "regardless of who is entered" (`premium_free_stale_record_bug.md:220-222`); the non-entered side is a live PlayerWork with `+0x4 == 0`.

## GameWork layout (`**DAT_1806F14F8`)

| Offset | Meaning | Source |
|---|---|---|
| **+0x00** | **1 = two players (versus), 0 = solo OR doubles**; also "sides visible" in TotalResult | `two_player_bpl_mode/logic.rs:13`; `in_shop_battle…:57,108,141,148`; `quick_logout_research.md:349-350` |
| +0x04 | style (0 single / 1 double) | `in_shop_battle…:57`; (conflict: `random_song_entry_research.md:75` says "side") |
| +0x08 | primary side — used UNCHECKED as a `DAT_1806F2ED0[…]` index | `in_shop_battle…:489-491` |
| +0x0C | 0-based stage counter | |
| +0x10 | final-stage override | |
| +0x18 | current mcode | |
| +0x1C | song-select UI mode | |
| +0x59/+0x5A | extra stage granted/consumed | |
| +0x70 | course pointer | |
| +0xD0 | event mode (1 BPL / 2 other) | |

**Writer of `GameWork+0`:** `SelectStyleSequence::onUpdate` (`FUN_1800b0bc0` on 20260825) case 1 — the mode-select commit (0-idx scene 20): `GameWork+0 = (2 players) ? 1 : 0`, `+4 = style`, `+8 = primary side`, `+0xD0 = bpl ? 1 : 0` (`in_shop_battle…:55-57`). What "(2 players)" is computed from is undocumented.

## Entry flow

Scene chain (0-idx): 14 title → 40/41 language → 18 → 19 `EAmEntryRootSequence` (login/card/credit) → 20 `SelectStyleSequence` → 21 CAUTION → 24 loader → 25 song select (`quick_logout_research.md:76-77`; `docs/ddr_world_scene_ids.md`).

- The ark owns the entry flow (`arkEntryFlowGetCurrentScene(side)`); a side that never entered stays at ark scene 0 (`ARK_ENTRYFLOW_NOENTRY`) all session (`quick_logout_research.md:339-341`).
- Card scanning stays armed through song select; the stage loader logs `STOP SCAN EAMUSEMENTPASS` (`quick_restart_fail_speedup_research.md:130-132`).
- Per-side state set around entry: `+0x8` payment, `+0x4`, `+0x5`; name/ddrcode/weight AFTER the profile load; `Option` from `<option>`, `Customize` from `<customize>`; records reset by `PlayerWork reset FUN_1801e7000` + `GameWork::reset FUN_1801DCAB0` at session start (`premium_free_stale_record_bug.md:63`).
- **The store to `PlayerWork+0x4` is NOT located in any note.**

## DPS / GamePlayActor count

- `song_reset::live_dps()` (`src/services/song_reset/mod.rs:1274-1287`): `TS → *(TS+0x58)`, rejected if `+0x20 & 0x24`. `gameplay_actors(dps)` (`:1290-1306`) walks `+0x18` first child / `+0x10` sibling, keeps `gameplay_actor_vtable` children; side `+0x84`, style `+0x88`.
- Loader builds the DPS ctor's per-side 16-byte `{entered, is_main, is_double, pad, i32 difficulty, u64}` with `difficulty = FUN_1801e89b0(wrapper) = PW+0x5C` (`quick_restart_fail_speedup_research.md:857-861`).
- `DancePlaySequence::onSetup` iterates present sides (`split_ssq_research.md:190-193`); `onUpdate` case 1 creates the actors (`in_shop_battle…:64-78`); the matching variant computes `isDouble` from per-side structs at `DPS+0x118/+0x120` (`…/2026-09-09-two-player-bpl-mode/research/re-findings.md:98`); normal DPS actors at `+0x100/+0x108`, LayoutActor `+0x110` (`in_shop_battle…:389-393`).
- Cabinet: solo P1 ⇒ no P2 actor (`learnings.md:740-741`); attract demo ⇒ two actors (`premium_free_stale_record_bug.md:217-219`).
- The LayoutActor per-side style array `root+0x84+side*4` reads `[0,0]` in BOTH real 1P and the 2P demo — NOT a presence signal (`…/20260612-center-arrows-single/research/r2-singleplayer-active-side.md:67-75`).

## Results (0-idx 30 / 32)

- `ResultSequence` ctor: `+0xE9 = (GameWork+0x70 != 0)`, `+0xEC = GameWork+0xC`, `+0xF0 = GameWork+4`, `+0xF4 = GameWork+8` (`quick_logout_research.md:232-248`).
- `FUN_1800b8aa0` creates two `WindowActor`s (one per side); tabs carry `+0x128 ResultSequence*`, `+0x130 side`, `+0x134 versus`, `+0x148 record side`, `+0x14C stage` (`…/2026-08-29-s-marvelous-judgement/research/display-side-re.md:34-46`). Tab kind 1 "Simple" `loop_guest` / 6 "Details" `loop_registered`.
- DLL results detours bail on `side_entered(side) == Some(false)` (`s_marvelous/results_score.rs:105-108`, `results_graph.rs:123-128`).
- `TotalResultSequence`: reads `GameWork+0x8` unchecked, `GameWork+0` (both visible), `PW+0x5 == 0` zeroes deltas, `PW+0x4` selects the name (`quick_logout_research.md:345-370`).
- Save actors: `SavePlayerDataActor(side, stage)` per side "unconditionally once results are reached" (`…/20260610-suppress-score-submission/research/score-submission-re.md:171-186`); gates `PW+0x1C`, `FUN_1801DD800`, `rec+0x1A4`.

## `versus_mirror` / `score_guard` / persistence

- `versus_mirror`: both entered = `side_entered(0)==Some(true) ∧ side_entered(1)==Some(true)` (`:132-133`), evaluated per frame; engages at the first SONG_SELECT frame with both entered (`:138-152`); disengages the instant either drops.
- `song_rate::lifecycle::classify_scene26` (`:211-228`): `(true,true)` ⇒ P1 governs, mask `0b11`.
- `score_guard`: per-song taints + rate ledger gate `savekind==2`; session-sticky gates `savekind==3` (`score_guard.rs:13-61, 802-871`).
- ess `save_sender` detour: side `*(*(job+0x10)+0x90)`, savekind `+0x74` (1/2/3) (`custom_options_persistence.rs:102-116, 864-890`); policy `:945-1007`. Fires once per CARDED-IN side (`score-submission-re.md:46-59`); gated on `*(savedata+0xF0) != 0` (`:83-84`).
- ddrcode → side routing never matches a ddrcode-less side (`custom_options_persistence.rs:1464-1471`).

## Traps (`.agents/learnings/learnings.md`)

`:731-750` option values outlive the player; `:1025-1064` stage bump = save-integrity boundary, two chart-identity writers; `:1313-1319` `PW+0x54` committed vs highlight; `:664-687` per-side UI mutators; `:1305-1311` staging slot ≠ wire field; `premium_free_stale_record_bug.md:224-240` cross-side `set_value` on load; `r2-singleplayer-active-side.md:67-94` layout style array ≠ presence.

## Open unknowns (Ghidra)

1. Who writes `PW+0x4` and when; whether `+0x5`/`+0x8` are set at the same site.
2. What "(2 players)" is in `FUN_1800b0bc0` case 1.
3. Where the loader (case 0x1d) reads the per-side `entered` byte; normal DPS side-info offsets; `is_main` source.
4. How the attract demo builds a 2P DPS with nobody entered.
5. `GameWork+4` style vs side.
6. Which gate suppresses a cardless side's `savekind==2`; `FUN_1800b6670` per-side loop gate; whether a `savekind==1` fires for a credit-only guest.
7. What hides the non-entered results pane in 1P; tab kind 1/6 key.
8. Versus vs solo song-select confirm selection; two-cursor handling.
9. Lamp/credit code `FUN_1800102a0` side effects of a fake-entered P2; `arkExpireCredit` at scene 34.
10. Extra-stage / max-stage predicates under `GameWork+0 == 1`.
