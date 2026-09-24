# Research: legacy song intro, READY shutter, and the complete A3 skin surface (2026-09-22)

RE-only pass (no code, no Ghidra edits). Builds: **A3 = `gamemdx_20240402.dll`**
(the spec), **W = World `gamemdx_20260825.dll`**, **W805 =
`gamemdx_20250805_STOCK.dll`** (presence checks only). Addresses are
file-relative to `0x180000000`. Data: `$DDR_A3_INSTALL`, `$DDR_WORLD_INSTALL`
(`data/arc/bm2d/*.arc`, unpacked with `scripts/unpack_arc.py` + `ifstools`;
AFP label/child/sound names read from the BSI-descrambled AP2 string tables).

Units: chart ticks, **0x1000 = 1 measure, 0x400 = 1 beat** (4096/measure).
Message ids differ by build: **World = A3 − 3** for the song-intro/gameplay
family (A3 `0x1047` audio-play = W `0x1044`, A3 `0x1048` tick = W `0x1045`,
A3 `0x104A/B/C` = W `0x1047/48/49`) and **World = A3 − 1** for ShutterActor
control (A3 `0x1008` request = W `0x1007`, A3 `0x1009` = W `0x1008`,
**A3 `0x100D` = W `0x100c`**).

---

## 1. A3 intro timeline (song-select commit → first arrow)

Actors: **ShutterActor** (TS child, singleton `DAT_1802eee78`, vtable
`0x180268258`; update `FUN_18002f5f0`, msg `FUN_1800304b0`), **ReadyGoActor**
(`sequence::dance::ReadyGoActor`, ctor `FUN_180042000`, vtable `0x1802691a8`:
init `FUN_1800420d0`, update `FUN_1800423d0`, finalize `FUN_180042520`, msg
`FUN_180042570`), **ControlMessageActor** (CMA; ctor `FUN_1800373d0`, msg
`FUN_180037760`, send `FUN_180037940`; one per GamePlayActor, created in the
GPA ctor `FUN_18003a720`), **DancePlaySequence** update `FUN_180039650`.

| # | Trigger | A3 skin N (1..5) | A3 skin 0 |
|---|---|---|---|
| 1 | Song-select commit (`FUN_1800c53e0` / `FUN_1800ec8a0`) | `FUN_180123360` sets `GameWork+0xB0=N`; `FUN_18002e660` sends **msg `0x100B`(N) to the ShutterActor subtree only**. Shutter `FUN_1800328c0` (event mode ∉{1,2}): `+0x194=N`, names `common_choice%04d` (`+0x1A0`), **`common_shutter%04d`** (`+0x1C8`), `common_choice_cutin%04d` (`+0x1F0`); if the first two exist → request-load both (`FUN_1800fe770(name,3)`), cut-in + `common_choice_cutinbg` if present | same call, N=0 → names cleared |
| 2 | Kind-1 ("stage choice") request (`0x1008`) → shutter state 0 | root `shutter_choice_hd_root` from `common_choice` (A3 v2, slot 2) via `FUN_1800306c0`; fill `FUN_180030d10` **legacy branch** (both names non-empty): clip `choice_stage` from `common_choice000N` into `choice_stage_usr2`, texture `scene_choice_stage000N_{1st,2nd,final,extra}`; clip `choice_background` from `common_shutter000N` into `choice_background_usr`; skins 3–5 add `choice_jacket` (from `common_shutter000N`) into `choice_jacket_usr`; p1/p2 score sets / high score / target / full-combo challenge / rinon filled for every skin | fill modern branch: `scene_choice_stage_{1st,2nd,3rd,4th,final,extra}` (+ galaxy/encore/`fl%s`) into `choice_stage_usr/*`, `choice_background_%s` variants (`+0x198`) |
| 3 | state 0 → **1/2 (cut-in)** only if the cut-in package loaded | `choice_cutin` from `common_choice_cutin000N` (depth 7), code SE **`sele_1st` / `sele_ext` / `sele_sn2` / `sele_x2` / `sele_2013`** (bank 2); skippable by button after 0x3B frames; wait label `close` | skipped (state 0 → 3) |
| 4 | state 3 | jacket arc load; **skin 3 loads `data/arc/banner/banner_sn2_<basename>.arc`** (texture `banner_sn2_%s`) instead of the jacket | `data/arc/jacket/<name>.arc` |
| 5 | state 4 (swap + `in`) | jacket clip **hidden+paused for skins 1–2**; modern `choice_stage_usr` hidden; stage voice at the legacy clip's `voice` label (`FUN_180032a50` → `FUN_18002e210`): **skin 1 none; skins 2–3 `sn2_etc{a2..a5, a7 final, 73 extra}` (`FUN_18002e060`); 4–5 `vo_stage_NN/final/extra`** (bank 3). League `*_lg_%s` variants disabled | `choice_stage_usr2` hidden, `vo_stage_*` immediately |
| 6 | states 5→6→7 | wait `loop` + `data_release`, hold (`loop` label or `+0x280` min time) → **7 = parked/covered** | same |
| 7 | Stage loader → DPS state 0 | when LayoutActor ready and (`!DPS+0x12C` ∨ `GameWork+100` lesson): **create ReadyGoActor** (0xA8 bytes; `+0xA0` = no-HERE flag = `DPS+0x12C`) as DPS child. Its init reads the `dance_message` LayoutActor record: skin≠0 → package `dance_message000N`, skin 0 → scene package slot `*(*DAT_1802eee90+0x8F0)`; layers `00_ready` (or `00_howtoplay` in lesson mode) and `00_here`, draw layer/prio 5, parked at frame 0 | same (stock `dance_message_v2`) |
| 8 | DPS 1–4 | GPAs (+CMA each), SongInfoActor unless skin 1, background, readiness `0x1001`, bank register | same |
| 9 | DPS 5 (**no fixed dwell in A3**) | shutter ∈{0,7} ∧ bank ready → bcast `0x1046`; shutter 6/7 → `FUN_18002e460` = `0x1009` → shutter 8: kind 1 plays **`frame_out`** (doors open, stage band/jacket stay over the lanes) → **9 parked** | same |
| 10 | DPS 6 | shutter ∉{6,7} → `0x1047` = audio play (`HIGH_PRECISION_BEGIN_TICK`) | same |
| 11 | CMA tick `0x1048` ≥ **READY** | `0x104A` → ReadyGoActor plays `00_ready` (AFP-embedded voice: skin 1 `ACT2_1`+`2nd_BIG2`, 2 `sn2_mst09`+`2nd_KANSEI_B`, 3 `sn2_mst09`, 4–5 `vo_ingame_ready`) and sends **`0x100D`** → shutter (active kind 1) state 10: `frame_out`→`out` → 11 wait `end` → release → 0 | `00_ready` embeds `vo_ingame_ready` |
| 12 | tick ≥ **HERE** | `0x104B` → READY `out` + `00_here` play; **skin 1 plays voice `ACT3_1` (final stage `ACT4_2`)** (bank `DAT_1802eee88[8]`) | no voice |
| 13 | tick ≥ **OUT** | `0x104C` → `out` on HERE; ReadyGoActor waits `end` on both clips → dies | same |
| 14 | tick ≥ first note | first arrow | — |

CMA thresholds (identical A3 `FUN_1800373d0` / W `FUN_180055c80`), from the
chart event vector: `firstNote` = first entry with type ≥ 0; `HERE0` = tick of
event `0xFA` (else 0). If the song has a movie byte (`info+0xB0/+0xB1` not 0/5),
is in `music_camera_resources.rlist`, or is the lesson song (A3 mcode `0x9542`,
W `0x9733`): `READY` = event `0xFB` tick (else 0), `HERE = HERE0`. Otherwise
`HERE = max(firstNote − 2 measures, HERE0)`, `READY = HERE − 1 measure`. Then
`OUT = min(HERE + 3 beats, firstNote)`, `HERE = min(HERE, OUT − ½ beat)`.
`0x104D`/W `0x104A` = last note end, `0x104E`/W `0x104B` = event `0xF9` ms.
**Skin never changes the timing** — only art and sounds. Versus: the DPS
routes the non-governing side's tick to its own GPA (A3 `FUN_18003a3a0`), and
ReadyGoActor's state gates make duplicate triggers no-ops.

## 2. World intro timeline

ShutterActor W: update `FUN_180033f60`, msg `FUN_180035170`, kind-art loader
`FUN_180035420` (kind table `0x18035e040`, 0x40 stride: `{pkg (NULL = default
common_shutter slot), root, close SE, open SE, voice, …}`), kind-3 fill
`FUN_180035f00`, stage voice `FUN_180033760`, singleton `DAT_1806f2d48`, kind
fields `+0x310/+0x314` (W805: `+0x2E0/+0x2E4`, stage kind 1, msg
`FUN_180034d10` — same handler shape). DPS update `FUN_180057e10`.

| # | Trigger | World |
|---|---|---|
| 1 | Song commit | no skin, no `0x100B` handler (W handler knows only `0x1007/08/0c/47/48`) |
| 2 | **SelectMusicTerminateSequence** (`FUN_180112480` @ `0x18011266a`; also `FUN_180154940`, `FUN_180156b50`, `FUN_180051660`) requests **kind 3** | root **`shutter_play`** from `common_shutter` (`_v3`). Kind table: 0–2/6/8 `shutter_root`, **3 `shutter_play`** (`se_start_game`), 4 `shutter_cleared` (`vo_stage_clear`), 5 `shutter_failed`, 7 `shutter_check`, 9–11 `event_shutter_brave` (named package) |
| 3 | state 1 | fill `FUN_180035f00`: `cosh_call_{1st,2nd,3rd,4th,final,extra,savior}` → `stage_usr`, `info_%dp_usr` tips (`cosh_play_tips_%s`), `cosh_dif_%s[_level_%02d]`, name, best/target score; jacket arc |
| 4 | state 2/3/4 | swap, label `in`, stage voice `vo_stage_*` (`FUN_180033760`); wait `loop` → **4 covered** |
| 5 | Stage loader (gate A: shutter 0/4) → DPS 0–4 | **no ReadyGoActor** (step 0 only advances) |
| 6 | DPS 5 | **5.0 s dwell** `DAT_18035a8c4 ≤ DPS+0x130` ∧ shutter 0/4 ∧ bank → `0x1043`; shutter 4 → `0x1008` (`FUN_180033a70`) → state 5: **`stage_out` + code voice `vo_ingame_ready`** → **6 parked** (the “READY?” panel: `ready_loop`, texture `cosh_call_ready`) |
| 7 | DPS 6 | shutter ≠ 4 → `0x1044` audio play + timing anchor |
| 8 | CMA `0x1047` (READY tick) | shutter: vestigial frame reads, **no action** |
| 9 | CMA `0x1048` (HERE tick) | shutter: goto **`ready_out`** (AFP continues `ready_here` → `here_out` → `end`); GPA msg `FUN_18005e190` also starts `*(GPA+0x130)+0x48=1` / `GPA+0x140` step 1 |
| 10 | `0x1049` (OUT) | no gameplay consumer found |
| 11 | song end | DPS 8 requests kind 4/5 (banner = drain of the parked 6) |

**Broadcast scope changed:** W's CMA sender `FUN_1800561f0` broadcasts from
`parent⁴` (CMA→GPA→DPS→TS→root), A3's from `parent²` (DPS). That is how the
TS-child ShutterActor hears `0x1048` in W — and it means **any DLL actor
anywhere in the tree receives `0x1047/48/49`**.

**World `0x100c` is A3's `0x100D`** (READY-time "dismiss the stage panel",
`active kind == stage kind` → drain): its only sender was ReadyGoActor, which
is why `docs/quick_restart_fail_speedup_research.md` §4a found no stock sender.

World package/actor ↔ A3 element:

| A3 element | World counterpart |
|---|---|
| ShutterActor kind 1 `shutter_choice_hd_root` (`common_choice` v2) + legacy sub-clips | kind 3 `shutter_play` (`common_shutter_v3`) — different root, children (`stage_usr`, `info_%dp_usr`, …) and labels (`in_stage loop_stage stage_out ready_loop ready_out ready_here here_out end`) |
| cut-in (`common_choice_cutin000N` + `common_choice_cutinbg`) | none |
| ReadyGoActor + `dance_message*` (`00_ready`/`00_here`/`00_howtoplay`) | `ready_loop`/`ready_out`/`ready_here` labels **inside `shutter_play`**; `dance_message` has no consumer |
| ControlMessageActor | identical (thresholds, lesson special-case), messages −3, root-wide broadcast |
| stage voice by skin | `FUN_180033760` `vo_stage_*` only |
| CLEARED/FAILED (A3 kinds 2/3/4, `common_shutter` v2 / legacy `common_shutter000N`) | kinds 4/5 `shutter_cleared`/`shutter_failed` (`common_shutter_v3`) |
| no dwell | 5.0 s READY? dwell |
| `ready_%dp_usr` | **not intro** — `sequence::selectmusic` lambda10 (`FUN_180115b90`), the song-select per-player READY badge |

All A3 intro art ships **byte-identical** in World: `common_choice_v2`,
`common_shutter_v2`, `common_choice000{1..5}_v0`, `common_shutter000{1..5}_v0`,
`common_choice_cutin000{1..5}_v0`, `common_choice_cutinbg_v0`,
`dance_message_v0..v2`, `dance_message000{1..5}_v0`, the 12
`data/arc/banner/banner_sn2_*.arc`. **Can it be data-swapped? No.** The A3
panel is a code-built composite of three packages plus a cut-in state World's
state machine does not have; World's kind 3 uses a different package/root/
children/labels. Resolver caveat: World's probe order is `_v3,_v0,_lite,bare`,
so requesting `common_choice` would load **`_v0`** (DDR A generation), not A3's
`_v2` — request the full name `common_choice_v2` (resolves on the bare rung,
FNV of `common_choice_v2.ifs` matches the member).

## 3. Element-by-element recommendation

| Element | Rec | World seams | Must suppress | Effort / risk |
|---|---|---|---|---|
| **Legacy READY / HERE WE GO** (ReadyGoActor) | **(c) re-implement** (small) | DLL actor attached to the live DPS (the `two_player_bpl_mode` vtable-clone + `actor_add_child` shape) at/after DPS step 1, or a per-frame poller of the governing CMA's StackStep; drive on W `0x1047` (READY) / `0x1048` (HERE) / `0x1049` (OUT), die at `end`. Package `dance_message000N` via `bm2d_package::request_load` (resolves `_v0`); layers via the World layer create (`FUN_180257920(&DAT_1806f9b20, pkg, clip, 0, 1)` + vt `+0xE8`/`+0xE0` = 5) — `bm2d_api` wrappers exist. Lesson: `00_howtoplay`, no HERE. Skin 1 HERE voice `ACT3_1`/`ACT4_2` (operator bank). At READY send W **`0x100c`** to the shutter iff the legacy panel is hosted on kind 3 | World READY?/`ready_out`/`ready_here` (goes away with the panel); shutter state-5 **`vo_ingame_ready`** code voice (legacy `00_ready` embeds its own) | 2–3 d / low-med |
| **Legacy stage-choice panel** (common_choice/common_shutter/jacket/banner/score sets) | **(c) re-implement**, hosted in **World's ShutterActor kind 3** (keeps loader gate A, DPS 1/5/6 gates and quick-restart's `0x100c` dismiss valid) | patch kind-3 table row (pkg `common_choice_v2`, root `shutter_choice_hd_root`) at arm, restore at disarm; detour kind-3 fill `FUN_180035f00` → port `FUN_180030d10` legacy branch + score-set fill; state 5 must play `frame_out` not `stage_out` (labels absent in the A3 root: `in loop loop_end frame_out out end data_release`) — detour/guard; READY-time `0x100c` → state 7 `out` → 8 → idle (A3 semantics). Alternative: DLL shutter actor as a TS child with World kind 3 never requested — decouples from W internals but loses the loader-gate cover and needs its own quick-restart dismiss | World fill (`cosh_*`), stage voice `FUN_180033760` (replace with skin voice at the `voice` label), 5.0 s dwell (seed `DPS+0x130` like quick-restart), `vo_ingame_ready` | 1–2 wk / high (shutter state machine is the limbo class of the QR research) |
| **Era cut-in** | (c) | insert between W state 1 (art loaded) and state 2 (swap): hold the jacket-load gate or play from a DLL actor over the covered screen; SE `sele_*` (operator bank) | none | 2–3 d / med |
| **Legacy CLEARED/FAILED end banners** (A3 kinds 2/3/4, `shutter_clear`/`shutter_failed` + overlay `00_cleared`/`00_failed`/`00_prayforall` from `common_shutter000N`) | (c) | kind 4/5 table rows → `common_shutter000N`/`shutter_clear|shutter_failed` (labels `in loop loop_end out end` match W's `in`/`loop`/`out`/`end` waits) + DLL-created overlay layer (W rows have no overlay slot); guard W's kind-4/5 fills (`score`, `cleared_star` children absent) | World banner art/voice for the song | 3–5 d / med |
| **`_sel` background movies** | (b) flip a dead flag | W MovieActor ctor `FUN_18007c960` never writes **`+0x149`** (A3 `+0x119`, set from `category==13 ∧ <movie>_sel exists`); init `FUN_18007cc20`→`FUN_18007cd70` still tries `FUN_18007c890` (`_sel`, `litp`→`_w`) first when set. Write 1 post-ctor for DDR-SELECTION songs | — (coordinate `movie_policy`/`movie_sync`) | 0.5 d / low |
| **Skin-branch geometry in surviving actors** (danger/gauges, below) | (b) feed the record skin | branches key on the LayoutActor record `+0x28` skin, i.e. `LayoutActor+0x190` ≠ 0 (orientation.md: a `GameWork+0xA8` write also registers shared packages — needs a narrower seam, e.g. set the record skin in the per-package helper) | — | design item |
| Era sounds (code + AFP-embedded) | (a) operator-supplied banks | see §4; AFP `sound_play` routing (which bank aeplib's callback searches) not traced | — | med |

## 4. Complete A3 skin-surface enumeration

Method: every instruction within 48 of a `DAT_1802ed6d0` (GameWork) load that
touches `+0xB0` (then hand-filtered), every `+0x14 == 0xD` test, every
`"%04d"` string, every consumer of the LayoutActor record skin (`FUN_18004d830`
callers reading `+0x28`) and the fields they store, every `0x100B` sender/
consumer, plus AFP-embedded `sound_play` cues in every legacy arc. "Doc" =
already in `docs/ddr_selection_research.md`.

| # | Behaviour | A3 site | World status | Doc |
|---|---|---|---|---|
| 1 | Skin setter (cat 13 ∧ folder `0xB1..B5`) | `FUN_180123360` | gone | yes |
| 2 | Credit reset | `FUN_180123060` | alive (`FUN_1801dd6d0`) | yes |
| 3 | **Reset at ResultSequence finalize + `0x100B(0)`** (skin lives commit → end of stage results) | `FUN_1800abbd0` (ResultSequence vt slot 5) | gone | **new** |
| 4 | `0x100B(0)` from BabylonsGalaxy SelectMusic update | `FUN_18002e6d0` ← `FUN_1800bde30` | gone | **new** |
| 5 | **`0x100B` consumer = ShutterActor only** (sent to its subtree), ignored in event mode 1/2 | `FUN_1800304b0` → `FUN_1800328c0` | gone | partial |
| 6 | Shutter loads **`common_shutter000N`** + `common_choice000N` + `common_choice_cutin000N` + `common_choice_cutinbg` | `FUN_1800328c0` | — | **new** (shutter pkg, cutinbg) |
| 7 | Legacy stage panel composite (choice_stage / choice_background / choice_jacket), `scene_choice_stage%04d_*` | `FUN_180030d10` | gone | partial |
| 8 | **Era cut-in state + SE `sele_*`** (skippable) | `FUN_18002f5f0` states 1–2 | gone | **new** (consumer) |
| 9 | **Skin 3: SN2 banner `banner_sn2_<basename>` as jacket** (`data/arc/banner/`) | `FUN_180030d10`, state 3 | gone | **new** |
| 10 | **Skins 1–2: no jacket on the panel** | state 4 | gone | **new** |
| 11 | **Stage voice by skin** (1 none, 2–3 `sn2_etc*`, else `vo_stage_*`) at the legacy `voice` label | `FUN_18002e210`, `FUN_18002e060`, `FUN_180032a50` | gone | **new** |
| 12 | League `*_lg_%s` shutter variants only for skin 0 | `FUN_1800306c0` | n/a | **new** |
| 13 | **Legacy CLEARED / FAILED / PRAY FOR ALL banners** from `common_shutter000N` (kinds 2/3/4 legacy flag) | `FUN_1800306c0` | gone | **new** |
| 14 | `%04d` package suffix + fall-back probe | `FUN_18004a170`, `FUN_18004a070` | dead format | yes |
| 15 | **ReadyGoActor** (dance_message by record skin) | `FUN_1800420d0` | actor deleted | partial (doc said class A) |
| 16 | **Skin-1 HERE voice `ACT3_1` / `ACT4_2` (final)** | `FUN_180042570` (`0x104B`) | gone | **new** |
| 17 | ComboActor skin list `{1,2,3}`, `dance_combo%04d[_%s]`, skin-1 half digits | `FUN_180046a60` | gone | yes |
| 18 | **DanceDangerActor: skin 0 per-style clip at `filter` marker + `_failed` overlay; skins 1–2 full-screen at (640,360); skins 3–5 at `danger_gauge` marker, depth side+2** | `FUN_180048630` | **ALIVE** (`FUN_180068ce0`) | **new** |
| 19 | **LifeGaugeActor: skin 3 plays `1p_in`/`2p_in`; skin 2 full-battery state** | `FUN_18004f4a0`, `FUN_18004fc90` | skin-2 **alive** (`FUN_1800706e0`, `FUN_180070de0`); skin-3 gone | **new** |
| 20 | **FlareGaugeActor: skin 3 `1p_in`/`2p_in`; skin 1 fill-segment/`fill _2_usr` style** | `FUN_180052ec0`, `FUN_180053600`, `FUN_180054050` | **ALIVE** (`FUN_180073cf0`, `FUN_1800743d0`) | **new** |
| 21 | ScoreActor `dance_score%04d_*`; **skin 2: difficulty frame prio 3, `%s%d`/`%s_in` labels** | `FUN_180055390`, `FUN_180055be0`, `FUN_180056080` | gone (stores skin `+0x60` only) | partial |
| 22 | StageFrameActor `stage_frame%04d_stage_*` | `FUN_180057f60`, `FUN_1800581a0` | gone (stores `+0x68`) | yes |
| 23 | Package-choice-only skin readers: SongInfo, layout root `dance_common`, MatchingBattleFrame (`dance_matching`) | `FUN_180056cc0`, `FUN_18004ace0`, `FUN_180050a70` | alive (`FUN_180078fd0`, `FUN_18006bd40`, `FUN_180071ce0`) | partial |
| 24 | Skin 1: no SongInfoActor / OptionIconActor | `FUN_180039650`, `FUN_180040b60`, `FUN_18003b490` | alive | yes |
| 25 | Skin 1: CourseOption forcing (9 getters) | `FUN_1801267d0…FUN_180126c40` | gone | yes |
| 26 | CallVoiceActor era voices/SEs | `FUN_180036890`, `FUN_1800369e0` | gone | yes |
| 27 | **Category 13 → `<movie>_sel` background videos** (18 files = exactly the sl01+sl02 songs; `litp` uses `_w`) | `FUN_180060090` @ `0x18006011a`, `FUN_18005f100`, `FUN_18005f370`(+0x119), `FUN_18005f030` | dead flag (W `+0x149`) | **new** |
| 28 | Folder UI: `category_name_ddrselection`, `folder_ddrselection0N`, card kind 6 `folder_selection`, banner **`semuca_selection0N_bnr`** (in `select_music_card_v2`), `banner_event_041/042` for `0xB0/0xB6` | `FUN_1800df610`, `FUN_1800e25e0` (banner string @ `0x1800e3087`) | art present, unused | partial |
| 29 | **AFP-embedded era sounds** (fire on clip play; all absent from World banks except `vo_ingame_ready`/`vo_stage_clear`): `common_shutter0001` `2nd_BIG2 ACT9 STG_APP02 STG_APP03 STG_CLOSE01 se_shutter_in/out vo_stage_clear`; `0002` `2nd_KANSEI_B ext_failed sn2_gov`; `0003` `STG_APP03 STG_CLOSE01 end_door sn2_gov`; `0004/0005` `Plate_spin4_st STG_APP02 STG_CLOSE01 banner_in`; `common_choice0001` `ACE_shutter_choice_exc`, `0004/5` `Plate_spin4_st`; `dance_message` (row 11); `dance_fullcombo000N` `XAC_full_combo2`; `dance_game_over000N` `Plate_spin3_st` | data | — | **new** |

Not skin: `daopic%04d_%dp`, `TS%04d`, `eam_popup_%04d%s`. No reader of skin in
results / total results / song select beyond rows 3–5, 28.

## 5. DDR SELECTION folder predicate (A3 `FUN_1800f2460`)

Membership is a **curated mcode allowlist mapped by raw series**, not a series
filter: `default → 0x118` (not a DDR SELECTION folder); `info+0xED == 0x29`
(event 41, inferred) → `0xB0` (`sl00`, `banner_event_041`); mcode ∈
`DAT_1802630d0` (54 entries) → by series (vslot `+0x78`): **1–5 → `0xB1`,
6–8 → `0xB2`, 9–10 → `0xB3`, 11–13 → `0xB4`, 14–17 → `0xB5`**; mcode ∈
`DAT_1802631a8` (4) → `0xB6` (`sl06`, `banner_event_042`, skin 0).

| Folder | Songs (A3 basename, raw series) |
|---|---|
| `0xB1` 1st–5th | trip, para (1); bril, para2 (2); afte, afro (3); bfor, burn (4); stil (5) |
| `0xB2` MAX–EXTREME | cand, maxx (6); drte, roll, kaku (7); radu, bagg, ichi, bom2 (8) |
| `0xB3` SuperNOVA | cach, gate, hana, flow2, chao, fasc (9); vemb, fway, alth, sunk, arra, plur (10) — = the 12 `banner_sn2_*` arcs |
| `0xB4` X | geis, rint, ontb, sabe (11); smoo, delt, poss (12); litp, tasf, alst, fwer, toho1 (13) |
| `0xB5` 2013–A | mobu, bedr, synf, anot, ostt (14); dind, syak, endr (15); **huia, hope, obor, coli (17)** |
| `0xB6` event 42 | anni, syur, ddrm, suns2 (all 17) |

**Settled: `0xB5` includes series 17 — and 17 is DDR A, not A20.** A3 raw
series (musicdb anchors): 14 2013 (mobu, anot), 15/16 DDR (2014) era (16:
egoi, ovtp; max mcode 38065), **17 A** (endy, newc, boss, huia), **18 A20**
(aceo, sill), **19 A20 PLUS** (kyoh, schw), **20 A3** (meh4, toyo). This matches
World's flare classifier (≥14 WHITE, ≥18 GOLD). **Correction:**
orientation.md / research doc §7.1 labels (16 = A, 17 = A20, 18 = A20 PLUS)
are off by one from 16 up. Note `0xB5` = exactly flare WHITE, `0xB1..B4` split
CLASSIC. An `AUTO` era mapping that must match A3 should use the allowlist;
a series-only rule (14–17 → skin 5) is a superset.

## 6. Open questions

1. AFP `sound_play` routing in World (which bank(s) aeplib's callback
   resolves) — decides whether operator-supplied A3 `se_normal`/`voice` banks
   make the embedded cues (§4 row 29) play without code.
2. A3 `DPS+0x12C` semantics (inferred demo/no-intro flag; gates ReadyGoActor,
   HERE and SongInfoActor) — World keeps the same byte.
3. What World's `0x1048` GPA side-effect (`*(GPA+0x130)+0x48`, `GPA+0x140`
   step) drives — must stay stock under a legacy intro.
4. A3 `FUN_1800327b0` (state-0 readiness: waits the skin packages?) and the
   `+0x280` hold writer `FUN_18002e820` (10.0 s / 0) — only needed if the
   panel hold is ported exactly.
5. Score-set / rival / target data sources for porting `FUN_180030d10`'s
   `p%d_score_set_mc` fill onto World's PlayerWork/score DB.
6. W805 checked for shutter msg handler (`FUN_180034d10`, stage kind 1) and CMA
   handler (`FUN_180052bd0`); danger/gauge skin branches, MovieActor `+0x149`
   and the parent⁴ broadcast verified on 20260825 only.
7. `banner_in` (`common_shutter0004/5`) is in no A3 `.xsb` either — likely a
   dead cue in A3 too.

## 7. Address index

**A3 20240402:** DPS update `FUN_180039650`; DPS msg `FUN_18003a3a0`; GPA
ctor `FUN_18003a720`; GPA tick send `FUN_18003de80`; CMA `FUN_1800373d0` /
`FUN_180037760` / `FUN_180037940`, vtable `0x180268a38`; ReadyGoActor
`FUN_180042000` / `0x1802691a8` / `FUN_1800420d0` / `FUN_1800423d0` /
`FUN_180042520` / `FUN_180042570`, strings `0x180269148..188`; ShutterActor ctor
`FUN_18002e870`, vtable `0x180268258`, update `FUN_18002f5f0`, msg
`FUN_1800304b0`, skin `FUN_1800328c0` (name builder `FUN_18002dfc0`), kind art
`FUN_1800306c0`, stage fill `FUN_180030d10`, legacy voice gate `FUN_180032a50`,
stage voices `FUN_18002e210` / `FUN_18002e060`, `0x1009` `FUN_18002e460`,
`0x100B` `FUN_18002e660` / `FUN_18002e6d0`; ResultSequence finalize
`FUN_1800abbd0`; folder predicate `FUN_1800f2460` (tables `0x1802630d0`,
`0x1802631a8`); folder card `FUN_1800df610`, card banners `FUN_1800e25e0`;
MovieActor `FUN_18005f370`, `_sel` `FUN_18005f100` / `FUN_18005f030`, bg init
`FUN_180060090`; actors in §4.

**World 20260825:** CMA ctor `FUN_180055c80`, msg `FUN_180056010`, send
`FUN_1800561f0`; GPA ctor `FUN_18005ae30`, msg `FUN_18005e190`; DPS update
`FUN_180057e10` (dwell `DAT_18035a8c4`); ShutterActor update `FUN_180033f60`,
msg `FUN_180035170`, kind art `FUN_180035420` (table `0x18035e040`), kind-3
fill `FUN_180035f00`, stage voice `FUN_180033760`, close request
`FUN_180033a00`, force-5 `FUN_180033a70`, singleton `DAT_1806f2d48`; labels
`ready_loop`/`ready_out` `0x18035e018`/`0x18035e028`; MovieActor ctor
`FUN_18007c960`, init `FUN_18007cc20`/`FUN_18007cd70`, `_sel` `FUN_18007c890`,
creator `FUN_18007d700`; Danger `FUN_180068ce0`; LifeGauge `FUN_1800706e0` /
`FUN_180070de0`; FlareGauge `FUN_180073cf0` / `FUN_1800743d0`; SelectMusic
READY badge `FUN_180115b90`; orphaned `ACT4_2`/`sn2_etc*` tables
`0x180465360..0x180465400`.

**World 20250805:** ShutterActor msg `FUN_180034d10`; CMA msg `FUN_180052bd0`;
`ready_out` `0x18033ecb0`.
