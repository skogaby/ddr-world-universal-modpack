# DDR SELECTION (legacy gameplay skins) in DDR World — Feasibility & Research

Status: **IN IMPLEMENTATION** (2026-09-22) — `src/mods/ddr_selection/`,
planning `.agents/planning/2026-09-22-ddr-selection/`. The P0 mechanism is
cabinet-proven (legacy judge / FAST/SLOW / full combo / game over / danger on
every skin). **Several statements below were corrected by the pre-design RE —
read §12 first**; the architecture actually built is §12.1, not §6.1.

Goal: bring back DDR A / A20 / A20 PLUS / A3's **DDR SELECTION** behaviour — the
gameplay UI (judgement words, combo digits, life gauge, score panel, stage
banner, READY/HERE WE GO messages, full-combo/danger/game-over art, and a few
era-specific sounds) re-skinned to look like the DDR generation the song came
from — inside DDR World, which removed the feature.

Addresses are file-relative to `gamemdx.dll` @ `0x180000000`. New RE in this
document was performed on **A3 20240402** (final A3, the specification) and
**World 20260825** (the current build), with a presence check on **World
20250805** (the oldest supported build). Builds are named per finding. Data
inventory was taken from the stock World install (`$DDR_WORLD_INSTALL`) and a
stock A3 final install side by side.

Builds on:

- `docs/afp_system.md`, `docs/afp_texture_pipeline.md` — the BM2D/AFP package
  and texture machinery the skins ride on (nothing re-derived here).
- `docs/folder_system_research.md` — World's folder/property system (the
  trigger side, §7.2).
- `docs/s_marvelous_judgement_research.md` — the one existing mod that edits
  `dance_judge` (an interaction to plan for, §8).
- `src/services/avs_layeredfs/` — the file-replacement layer every option in
  §6 is built on; `core/arc.rs` (arc reader + writer + member rename),
  `core/ap2/` (AFP parser/serializer), `ifs_textures` (texture injection +
  `purge_texture_replacement`), `afplist_ext` (afplist rewriting).

---

## 1. Verdict

**Feasible, and structurally much cheaper than the Background Dancers revival.**
The dancers needed a deleted engine layer re-implemented (animation runtime,
scene nodes, render items). DDR SELECTION needs *none* of that: every runtime
the skins use — the BM2D/AFP package loader, the versioned-arc probe, the
gameplay actors that play the clips — is alive in World, and **the legacy skin
assets themselves are still shipped in the stock World install, byte-identical
to A3's** (55 `dance_*000{1..5}_v0.arc`, 12.6 MB, plus the shutter/stage-banner
and folder-banner art). What Konami removed is thin:

| Layer | A3 20240402 | World (20250805 … 20260825) |
|---|---|---|
| Skin id storage (`GameWork+0xB0` A3 / `+0xA8` World), reset to 0 per credit, copied into `LayoutActor+0x190`, identity-mapped through the play-sequence loaders | present | **PRESENT** (dead plumbing — nothing writes a non-zero value) |
| Skin id **writer**: song-select commit → folder category 13 ∧ folder id `0xB1..0xB5` → skin 1..5 (`FUN_180123360`) + msg `0x100B` broadcast | present | **GONE** (World's song select is a different system; no `sl0N` folders) |
| `LayoutActor::onInitialize` appending `%04d` to every gameplay package name (`dance_judge` → `dance_judge0001`) with a fall-back probe to skin 0 | present | **REMOVED** — the `"%04d"` `snprintf` is still executed into a dead stack buffer on every package (`FUN_18006b710` @ `0x18006b7b3`), but the string-append that followed it is gone |
| Actor-side name composition using the skin id (`dance_combo%04d_%s`, `dance_score%04d_*`, `stage_frame%04d_stage_%s`) | present | **REPLACED** by World's `da??_*` scheme (`daco_combo%s_%d`, `dasc_score_num_%d`, `dast_stage_*`) with no skin parameter |
| Skin-1 gates: no `OptionIconActor`, no `SongInfoActor` | present | **PRESENT** (three `CMP [GameWork+0xA8],1` sites still live) |
| Skin-1 option forcing in `ddr::player::CourseOption` getters (speed ×1.0, arrow shape 2, filter/guideline/boost off …) | present | **GONE** |
| Era-specific announcer / SE cues in `CallVoiceActor` (`sn2_dgm*`, `ACT6`, `2nd_BIG2`, `STG_APP0x`) | present, cues in `voice.xsb`/`se_normal.xsb` | code GONE (string table `0x1804653c0` orphaned); **cues ABSENT from World's banks** |
| Legacy skin arcs on disk (`dance_*000N_v0`, `common_choice000N_v0`, `common_choice_cutin000N_v0`, folder banners in `select_music_card_*_v0..2`) | 55 + 5 + 5 arcs | **PRESENT, byte-identical** (54/55 identical; `dance_combo0005_v0` re-stored compressed, §4) |
| World-side gameplay packages the skins must stand in for | — | 5 packages keep the A3 AFP structure (whole-package swap works), 6 were restructured/renamed (need texture-level adaptation), 1 is the layout root (keep World's) — §5 |

So the revival is **one detour plus a data step**: decide the skin per song,
re-add the missing name suffix at the package loader's resolver so the game
loads the `dance_<pkg>000N_v0.arc` files it already ships (untouched, under the
names they already have) for the "compatible" packages, and re-texture World's
own packages with the legacy art for the "restructured" ones. No engine ABI to
reproduce, no repacking, no cached copies; the per-song switch is a flag the
resolver detour reads, and the re-texture half rides the texture-restage/purge
mechanism S-Marvelous already uses. Estimated **1–2 weeks** for a v1 (§9)
after a **1–2 day spike** (§9.1) that proves the legacy packages load and play
under World's actors on a cabinet.

The parts that cannot come back from World's own data are the **era sounds**
(World's `voice.xwb`/`se_normal.xwb` dropped every legacy cue — A3's banks have
them; operator-supplied, §7.5) and A3's **option forcing for the 1st–5th skin**
(a gameplay change; recommend not porting it, or gating it behind its own
toggle, §7.4).

---

## 2. What DDR SELECTION was, mechanically (A3 20240402)

### 2.1 The five skins

The skin id is an integer 0..5. 0 = the current game's UI. The five legacy
skins map to the five DDR SELECTION sub-folders; the folder banners
(`select_music_card_lang_eng_v2` → `folder_ddrselection01..05.png`) name the
eras, and the judgement art confirms them:

| Skin | Folder key | Banner text | Judgement word style (from `dance_judge000N`) |
|---|---|---|---|
| 1 | `sl01` | `[1998-2001] DanceDanceRevolution 1st-5th` | pixel font `MARVELOUS!!! PERFECT!!! GREAT!! GOOD! BOO MISS` (the original arcade font); has FAST/SLOW textures |
| 2 | `sl02` | `[2001-2002] DDRMAX Series - DDR EXTREME` | bold italic `MARVELOUS!!! PERFECT!! GREAT! GOOD Boo Miss..` |
| 3 | `sl03` | `[2006-2007] DDR SuperNOVA Series` | small-caps `MARvELOUS!! PERFECT!! GREAT! GOOD BOO MISS...` |
| 4 | `sl04` | `[2008-2011] DDR X Series` | italic script `MARVELOUS!! PERFECT!! GREAT! GOOD! BOO MISS!`; has FAST/SLOW textures |
| 5 | `sl05` | `[2013-] DanceDanceRevolution - DDR A` | rounded `Marvelous!!! Perfect!! Great! Good BOO Miss...` |

(`sl00` → `folder_event_041` and `sl06` → `folder_event_042` in the same
key table — the DDR SELECTION category also hosted two event folders.)

### 2.2 Trigger: the folder you picked the song from

The skin is **not a musicdb field**. A3's `musicdb.xml` has no per-song skin
tag (tags present: `mcode basename title title_yomi artist bpmmax bpmmin series
bemaniflag limited_cha limited eventno region genreflag movie movieoffset
bgstage voice diffLv`). Instead, at the **song-select commit**
(`FUN_1800c53e0` @ `0x1800c5cb9..cd5`, and the second select path
`FUN_1800ec8a0` @ `0x1800ed4fc..517`) the game writes

```
GameWork+0x10 = mcode
GameWork+0x14 = folder CATEGORY   (FUN_1800f45f0)
GameWork+0x18 = folder ID         (FUN_1800f4630)
call FUN_180123360               ; the skin setter
```

and `FUN_180123360` is the whole rule:

```
if GameWork+0x14 == 0xD          ; category 13 = DDR SELECTION
   switch GameWork+0x18:
     0xB1 → GameWork+0xB0 = 1
     0xB2 → 2
     0xB3 → 3
     0xB4 → 4
     0xB5 → 5
     else → 0
else GameWork+0xB0 = 0
```

followed by `FUN_18002e660(skin)` = broadcast message **`0x100B`** (payload =
skin id) to the sequence actor tree (consumers: the stage-choice shutter,
§2.6). `GameWork+0xB0` is reset to 0 in the per-credit `GameWork` reset
(`FUN_180123060`). So the same song played from ALL MUSIC used the current UI;
played from a DDR SELECTION sub-folder it used that sub-folder's skin — the
skin follows the **folder**, and the folder's membership (which songs appear in
`sl01..05`) is the only thing that tied songs to eras. Membership presumably
follows `<series>`; the filter predicate for folder ids `0xB1..0xB5` was not
traced (open item §10).

### 2.3 Package selection: `%04d` on every gameplay package

`DancePlaySequence::onInitialize` (`FUN_180038b40`) constructs the gameplay
`LayoutActor` as `FUN_180049e00(this, p1Type, p2Type, skin)` with `skin =
table[GameWork+0xB0]` (an identity table `{0,1,2,3,4,5}` on the stack — the
same shape World still has, §3). The ctor stores it at `LayoutActor+0x190`.

`LayoutActor::onInitialize` (`FUN_18004a170`, vtable slot 4) builds the
package list. For each per-side package (`dance_combo`, `dance_gauge`,
`dance_judge`, `dance_fast_slow` inline; `dance_effect`, `dance_score`,
`dance_filter`, `dance_cover`, `dance_option_icon`, `dance_fullcombo`,
`dance_score_compare`, `dance_game_over`, `dance_danger` via `FUN_18004a070`)
and the shared ones (`dance_stage_frame`, `dance_message`, `dance_song_info`,
`dance_common`, side index 2):

1. `name = base + sprintf("%04d", skin)` (skipped for `dance_message` when
   skin == 0 — `dance_message` has no `0000` variant);
2. probe `FUN_1800fe370(name)`: for `v = DAT_1802eee98 .. 0`, `lstat
   data/arc/bm2d/<name>_v<v>.arc`; if none exists → **fall back to skin 0**
   (`name = base + "0000"`, record skin 0);
3. store a record `{name, skin @+0x28}` in the side's package list
   (`FUN_18004d780`) and push the name on the load list (`FUN_18004cab0`);
4. shared packages (`param_5 == 1`) are only registered when a variant
   actually exists (`local_28 != 0`) — the skin-0 copies are loaded by the
   scene itself.

The fall-back is why partial skins work: `dance_common0001_v0` does not exist
(1st–5th uses the current lane layout), `dance_song_info000N` exists only for
skin 2, `dance_cover/effect/filter/option_icon/score_compare` exist only as
`0000`.

### 2.4 Actors that compose texture names from the skin id

Everything inside an AFP clip (shape → texture) is self-contained per package,
so most actors only play labels. Three actors compose texture names in code
and read the skin from the package record (`FUN_18004d830(layout, "dance_combo")
→ record+0x28`):

- **ComboActor** (`FUN_180046a60`): `DAT_180265038 = {1,2,3}` — for those
  skins `ComboActor+0x94 = 0` and the sheet name is `dance_combo%04d`
  (one colour); skins 0/4/5 use `dance_combo%04d_%s` with
  `%s ∈ {marvelous, perfect, great, good}` (judgement-coloured digits).
  Glyph names `%s_combo`, `%s_%d`; digit spacing table `FUN_180046970`
  differs for skin 1, and skin 1 halves the measured digit width
  (`+0x80 /= 2`) — the 1st MIX digits are half-cells.
- **ScoreActor**: `dance_score%04d_score_num_%d`, `_score_comma%s`,
  `_lv%02d`, `_score_num_0_gray`.
- **StageFrameActor**: `stage_frame%04d_stage_%s` / `_howto` / `_checking`.

### 2.5 Skin-dependent behaviour beyond art

- **Hidden actors for skin 1** (1st–5th had neither): `GamePlayActor` init
  (`FUN_18003b490`) skips `OptionIconActor` when `GameWork+0xB0 == 1`;
  `DancePlaySequence::onUpdate` (`FUN_180039650` @ `0x180039a0d`) and the
  matching variant (`FUN_180040b60` @ `0x180040f66`) skip `SongInfoActor`.
- **Option forcing for skin 1.** Every per-player option object is a
  `ddr::player::CourseOption` (vtable `0x1802806E8`, installed at
  `PlayerWork+0xD0` by the `PlayerWork` ctor `FUN_1801284d0`; the base
  `Option` vtable `0x180280538` is only used by the two static
  course/event option singletons in `FUN_18012ac10`). Its getters override
  the stored value when `GameWork+0xB0 == 1` and no course-fixed option
  block (`this+0x90`) is present:

  | vslot | getter | raw field | value under skin 1 | meaning (by option-node order `speed boost appearance turn step_zone scroll arrow_color cut freeze jump arrow filter guideline …` — field names inferred, unverified) |
  |---|---|---|---|---|
  | `+0x20` | `FUN_180126ab0` | `+0x0C` | 3 | speed → ×1.0 (`(idx+1)·0.25`) |
  | `+0x30` | `FUN_180126b20` | `+0x10` | 0 | boost off |
  | `+0x40` | `FUN_1801267d0` | `+0x14` | 0 | appearance visible |
  | `+0x60` | `FUN_180126bc0` | `+0x1C` | 0 | step zone default |
  | `+0x70` | `FUN_180126c40` | `+0x20` | 0 | scroll normal |
  | `+0x80` | `FUN_180126c00` | `+0x24` | 2 | arrow colour scheme 2 |
  | `+0xC0` | `FUN_1801267f0` | `+0x34` | 2 | **arrow shape 2** (`2d_arrow02.arc` — the classic chevron arrow; `GamePlayActor` init loads `data/arc/2d/2d_arrow%02d.arc` from this getter) |
  | `+0xD0` | `FUN_180126810` | `+0x38` | 0 | filter off |
  | `+0xE0` | `FUN_180126830` | `+0x3C` | 0 | guideline off |

  The arrow **shapes** are otherwise the player's choice: the legacy skins
  ship no arrow sheets of their own (`2d_arrow00..07.arc` are the modern
  shape options, byte-identical A3 ↔ World; `2d_arrow02` is the one skin 1
  forces).
- **Era sounds** (`CallVoiceActor::onUpdate` `FUN_1800369e0`, ctor
  `FUN_180036890`): skin 1 — no combo call-outs, "high" state voice `ACT6`,
  combo-milestone SE `2nd_BIG2`; skins 2–3 — combo voices `sn2_dgm25..34`
  (`PTR_s_sn2_dgm25_1802db9c0`), states `sn2_dgm_middle`/`sn2_dgm_high`,
  milestone SE `2nd_KANSEI_B` (2) / `STG_APP03` (3); skins 4–5 — the
  current `vo_ingame_*` set with `STG_APP02`; skin 0 — `vo_ingame_*` +
  `STG_APP02/03/BOO`. All cues live in A3's `voice.xsb`/`se_normal.xsb`.

### 2.6 Beyond the play screen

- **Stage-choice shutter** (`FUN_180030d10`, the "1st STAGE" jacket panel):
  texture `scene_choice_stage%04d_{1st,2nd,final,extra}` with the skin from
  `actor+0x194` (set through msg `0x100B`). Data: `common_choice000N_v0.arc`
  (`scene_choice_stage000N_*.png`, e.g. skin 1's red-outlined "STAGE 1").
- **Cut-in**: `common_choice_cutin000N_v0.arc` (per-era letter sets
  `cochcu_bl_NN`/`cochcu_wh_NN`) — consumer not traced.
- **Folder UI**: `category_name_ddrselection[_b]`, `folder_version_ddrselection`,
  `folder_ddrselection01..05` in `select_music_card_lang_*_v0..v2`; folder
  key table `{"slNN", texture-name*}` at `0x180263d40` (16-byte stride,
  inside the larger folder-key table starting `0x180263c00`).

---

## 3. What survived in World — the evidence

Decompiled on 20260825 unless noted; 20250805 checked by strings + the
`LayoutActor` per-package function.

### 3.1 The skin id plumbing is intact but inert

- `GameWork` global `DAT_1806f14f8`; skin field **`+0xA8`** (A3 `+0xB0`; the
  World struct is 8 bytes shorter above it). Reset to 0 in the credit reset
  `FUN_1801dd6d0` (= A3 `FUN_180123060`).
- `DancePlaySequence::onInitialize` `FUN_1800573d0` @ `0x180057af4..b6b` and
  `MatchingDancePlaySequence` `FUN_180061520` @ `0x1800619dd..a27` still build
  the identity table `{0,1,2,3,4,5}`, index it with `GameWork+0xA8`, and pass
  the result as the 4th argument of the `LayoutActor` ctor `FUN_18006b3f0`,
  which stores it at `+0x190`.
- `LayoutActor::onInitialize` `FUN_18006b8b0` passes `+0x190` to
  `FUN_18006b710(this, side, name, skin, shared)` for all 18 packages
  (`dance_combo gauge judge fast_slow effect score bpm filter cover option
  fullcombo score_compare game_over danger` per side; `dance_stage message
  song_info common` shared). `FUN_18006b710` still formats `"%04d"`
  (`0x180361958`) with the skin into an 8-byte stack buffer at `0x18006b7b3`
  (and again with 0 at `0x18006b82f` on the fall-back path) — **and never
  reads it**. The A3 append (`FUN_18001ab30`) between the format and the
  version probe is gone. The record/list logic that follows is otherwise
  A3's (`shared` packages only registered when `skin != 0`).
- **20250805**: `FUN_180068000` is the same function, same dead format
  (`"%04d"` @ `0x180341dc8`), same probe.
- Version probe `FUN_1801ac3f0(dir, name)` (`"%s_v%d"` @ `0x180380468`):
  tries `<name>_v3`, `<name>_v0`, `<name>_lite` (pcType-gated), `<name>`,
  each as `avs_fs_lstat("data/arc/<dir>/<cand>.arc")`. **This probe order is
  what makes the legacy files loadable as-is**: asked for `dance_judge0001`,
  `_v3` misses and `_v0` hits `dance_judge0001_v0.arc` — the file World
  already ships.
- The load chain the `LayoutActor` list feeds (`FUN_1801aca30(dir, name, 3)`
  per package): dedupe by name in the global package vector
  (`FUN_1801ad560` over `DAT_1806f2d70`) → `FUN_1801ace90` → **`FUN_1801ac260(out, dir,
  name)`** repeats the `_v3/_v0/_lite/bare` resolution, registers the arc
  with the FileManager (`FUN_1801ac160`: lstat + `FUN_1801ff6b0(DAT_1806f2f50,
  path)`), and returns `{file index, FNV-1("<cand>.ifs")}` — the hash it later
  uses to find the ifs member inside the arc. The package vector entry keeps
  the **requested** name (`dance_judge`), so every downstream consumer is
  name-stable no matter which arc was resolved. For a legacy arc the member is
  `data/bm2d/dance_judge0001_v0.ifs`, i.e. it hashes correctly only when the
  resolver was asked for `dance_judge0001` — which is why the suffix has to
  be re-added at (or before) `FUN_1801ac260`, not faked at the file layer
  (§6.1 vs §6.2).
- Actors reach packages through the `LayoutActor` record indirection exactly
  as in A3: `FUN_18006ece0(layout, "dance_combo")` returns the record's value
  string (the name that was pushed on the load list), then
  `FUN_1801ad4a0(vector, end, value)` finds the package vector entry and
  reads its handle at `+0x30` (ComboActor `FUN_180066250`; the same shape in
  NoteResultActor `FUN_18007aa40`, LifeGaugeActor `FUN_1800706e0`,
  DanceDangerActor `FUN_180068ce0`, StageFrameActor `FUN_18007a190`). Either
  name (`dance_judge` or `dance_judge0001`) works end-to-end as long as the
  record value and the vector entry agree — they do by construction.
- Skin-1 gates are live: `FUN_180057e10` (DPS::onUpdate step 1) @
  `0x1800582b3` and `FUN_180061cc0` (Matching) @ `0x180062155` skip
  `SongInfoActor` (`FUN_180078e20`); `GamePlayActor` init `FUN_18005be20` @
  `0x18005c2fb` skips `OptionIconActor`. Writing `GameWork+0xA8 = 1` from the
  DLL therefore still reproduces A3's "no song-info panel, no option icons"
  for the 1st–5th skin for free.

### 3.2 What was removed

- The skin setter (`FUN_180123360`) and both call sites — World's song select
  has no folder ids in the `0xB1..0xB5` range and no category 13. Nothing in
  World writes `GameWork+0xA8` except the reset (verified by scanning every
  instruction within 40 of a `GameWork` load for a `+0xA8` store).
- The `CourseOption` skin overrides: no World option getter compares
  `GameWork+0xA8` (same scan, `CMP …+0xA8, 1` occurs only at the three actor
  gates above).
- `CallVoiceActor::onUpdate` `FUN_1800553b0`: a single `vo_ingame_*` /
  `se_kansei_*` table, no skin branches. The `sn2_dgm25..34`, `sn2_dgm_high`,
  `sn2_dgm_middle` strings survive only as an unreferenced pointer table at
  `0x1804653c0`.
- Skin-parameterised texture composition. World's actors compose
  `daco_combo%s_%d` / `combo_usr/number_usr/%d_usr` / `dance_combo_root%d`
  (ComboActor `FUN_180066250`), `dasc_score_num_%d` / `dasc_score_comma%s` /
  `dasc_dif_%s_level_%02d` (score), `dast_stage_*` (stage), `dabp_bpm_num_%d`
  (bpm), `daop_icon_%s_%s` (option) — no skin anywhere.

---

## 4. Data inventory (stock World install vs. stock A3 final)

`data/arc/bm2d/`: A3 has 106 `dance_*` arcs, World 142 — a strict superset
(World added the `_v3` generation: `dance_bpm`, `dance_measure`, `dance_option`,
`dance_stage`, `dance_subtitles_*`, and `_v3` versions of the rest). Every
legacy skin arc is present in World:

| Package | Skin variants present (`*000N_v0`) | Size total |
|---|---|---|
| `dance_combo` | 1 2 3 4 5 | 0.69 MB |
| `dance_common` (layout root + lane frames) | 2 3 4 5 | 1.01 MB |
| `dance_danger` | 1 2 3 4 5 | 3.04 MB |
| `dance_fast_slow` | 1 2 3 4 5 | 0.13 MB |
| `dance_fullcombo` | 1 2 3 4 5 | 3.34 MB |
| `dance_game_over` | 1 2 3 4 5 | 0.31 MB |
| `dance_gauge` | 1 2 3 4 5 | 1.53 MB |
| `dance_judge` | 1 2 3 4 5 | 0.53 MB |
| `dance_message` | 1 2 3 4 5 | 1.28 MB |
| `dance_score` | 1 2 3 4 5 | 0.26 MB |
| `dance_song_info` | 2 | 0.02 MB |
| `dance_stage_frame` | 1 2 3 4 5 | 0.44 MB |
| `common_choice` (shutter stage banner) | 1 2 3 4 5 | — |
| `common_choice_cutin` | 1 2 3 4 5 (+ `cutinbg`) | — |

54 of the 55 `dance_*000N_v0` arcs are **byte-identical** to A3's. The one
exception, `dance_combo0005_v0.arc`, has identical member listing and sizes but
World stores it AVSLZ-compressed (61 KB vs 518 KB); both offline unpackers in
this repo (`scripts/unpack_arc.py`, `scripts/arctool`) decompress it to zeros,
so either the file is damaged in World or the tools mishandle this flag —
resolve in the spike (§9.1; the runtime `avslz.rs` path is the one that
matters, and A3's uncompressed copy is a drop-in fallback).

Not present in World's data: the era **sound cues** — `2nd_BIG2`,
`2nd_KANSEI_B`, `STG_APP02`, `STG_APP03`, `STG_BOO`, `ACT6`, `sn2_dgm_*` are
in A3's `se_normal.xsb`/`voice.xsb` (`soundbanks.arc` + `voice.xwb`) and in
none of World's (World's banks were rebuilt: `voice.xwb` 61 MB vs 45 MB, no
overlap on these names). World's own `2d_arrow00..07.arc` are identical to
A3's, so the arrow shape skin 1 forced (`2d_arrow02`) is available.

The folder-UI art (`select_music_card_lang_*_v0..v2`) is present and identical
but World's redesigned song select (`_v3` assets, `muca_*`/`mufo_*` names)
never loads it; it is only useful if a folder-based trigger is built (§7.2).

---

## 5. Compatibility audit: legacy packages vs. World's actors

Because World's actors address packages by AFP export name, clip labels, child
`*_usr` names and (for a few) code-composed texture names, each legacy package
falls into one of three classes. Export names below are from `afplist.xml`
(`ifstools`) and `afputils parseafp` (bemaniutils).

| World package (`_v3` unless noted) | World AFP exports / what the actor asks for | Legacy skin AFP exports (`*000N_v0`) | Class |
|---|---|---|---|
| `dance_judge` | `dance_judge`, `dance_judge_for_freeze`, `dance_marvelous`; NoteResultActor `FUN_18007aa40` finds `dance_judge` / `dance_judge_for_freeze` and plays labels `in_marvelous in_perfect in_great in_good in_miss in_ok in_ng` (World has no Boo) | `dance_judge`, `dance_judge_for_freeze`, `marvelous`; labels `in_marvelous in_perfect in_great in_good in_boo in_miss in_ok in_ng` (superset; timeline offsets differ, which is fine — labels are looked up) | **A — whole-package swap** |
| `dance_fast_slow` | `dance_fast_slow` | `dance_fast_slow` (+ `_test`, `tri_*`) | A |
| `dance_game_over` | `game_over` (DanceDangerActor `FUN_180068ce0`: `game_over`) | `game_over` | A |
| `dance_danger` | `danger_single`, `danger_double`; actor asks `danger_single`/`danger_double`/`danger_gauge` | `dance_danger`, `dance_danger_gauge`, `dance_danger_side`, `danger_single`, `danger_double`, `danger_*_failed`, … | A (verify `danger_gauge` child) |
| `dance_fullcombo` | 16 exports (`01_fullcombo_single_normal` … `which_fullcombo_perfect`) | same 16 export names | A |
| `dance_message` (World resolves `_v0` — no `_v3` exists) | `00_here`, `00_howtoplay`, `00_ready` | `00_here`, `00_howtoplay`, `00_ready` | A |
| `dance_gauge` | `dance_gauge`, `dance_gauge_gaugeset`, `gauge_damage8`, `gauge_frame`; LifeGaugeActor `FUN_1800706e0` finds `dance_gauge`, plays `loop_%dlife`, children `gauge_frame_usr`, `fill _usr`, `damage_%d_usr`; World textures include the FLARE gauge set (`daga_gauge_flare1..9,ex`) | `00_dance_gauge`, `00_sd_dance_gauge`, `dance_gauge_gaugeset`, `dance_gauge_sd_gaugeset`, `game_over_gauge`, `gauge_damage8`, `gauge_frame`, `gauge_frame_sd`, `gauge_sd_damage8` | **B — swap with AFP rename** (`00_dance_gauge` → `dance_gauge` in the afplist + AP2 exported name; FLARE gauge frames absent — fall back to the World package when the gauge type is FLARE, or accept "no flare art") |
| `dance_combo` | `combo`, `dance_combo_root1..3`, `number`; ComboActor composes `dance_combo_root%d` (three roots), `combo_usr/number_usr/%d_usr`, textures `daco_combo%s_%d`, `daco_combo_dummy_%s` | `dance_combo`, `dance_combo_old`, `number`; textures `dance_combo000N[_%s]_{0..9,combo}` | **C — keep World AFP, re-texture** (`daco_combo_<grade>_<d>` ← `dance_combo000N[_<grade>]_<d>`; single-colour skins 1–3 map the one sheet onto all four grade names; the World ComboActor NULL-derefs when a `dance_combo_root%d` clip is missing, so a whole swap is not an option) |
| `dance_score` | `dance_difficulty`, `dance_name`, `dance_score`; textures `dasc_score_num_%d`, `dasc_score_comma%s`, `dasc_dif_%s_level_%02d`, `dasc_score_exscore` | `frame_score`, `frame_difficulty_{1p,2p}[_reverse]`, `difficulty_level[_base]`; textures `dance_score000N_score_num_%d`, `_score`, `_score_ex` | C |
| `dance_stage` (was `dance_stage_frame`) | `dance_stage`; StageFrameActor `FUN_18007a190` (`dance_stage`, marker `stage`); textures `dast_stage_{01..04,extra,final,encoreextra,howto,checking,galaxy}`, `dast_stage` | `stage_frame`, `stage_frame_sd`; textures `stage_frame000N_stage_{01..,extra,final}`, `stage_frame000N_stage` | C |
| `dance_song_info` | `dance_song_info_single`, `dance_song_info_double`; textures `daso_info_base_*` | `dance_song_info`, `dance_song_info_sd`; skin 2 only | C (skin 2) / stock |
| `dance_bpm`, `dance_option`, `dance_measure`, `dance_subtitles` | World-only packages | no legacy equivalent (A3's `dance_option_icon0000` is the option icons' ancestor) | stock (skin 1: hide via `GameWork+0xA8 = 1`) |
| `dance_common` (layout root) | `dance_root`, `lane_*`; World's root carries markers the new actors need (`bpm_%dp_usr`, `option_%dp_usr`, `matching_*`, `name_*`) | `dance_root`, `dance_root_sd`, `lane_*`, `combo_set_*`; textures `dam_gauge_hd_1p/2p` (the era lane-side gauge frame) | **keep World's**; optionally re-texture its gauge-frame art (`dam_gauge_hd_*` appear in both) |
| `dance_cover`, `dance_effect`, `dance_filter`, `dance_score_compare` | — | `0000` only | stock |

Class A packages were the A3 packages as well (World's `_v3` AFPs inherited
their export sets), which is why whole swaps are expected to just work. Class
C is where the modpack's existing texture-injection pipeline does the work:
World's AFP geometry stays, the legacy PNGs are served under World's texture
names. Size mismatches (e.g. 1st MIX half-width digits vs World's cells) are
handled at generation time by scaling/padding the PNG to World's `texturelist`
rect (the AFP shapes are fixed-size — a differently sized replacement is
stretched, not re-laid-out).

---

## 6. Architecture options

### 6.1 Option A — re-add the name suffix at the loader (one detour) (RECOMMENDED)

Everything A3 needed is still in place except the string append, so put the
append back at the one place all package loads funnel through and let the game
open the files it already ships, under the names they already have.

**Runtime (DLL)**:

1. **Skin resolver** (pure, host-tested): at song-select commit / GAMEPLAY
   loader entry decide `skin ∈ 0..5` from the option row and the song's
   `<series>` (§7.1). Versus: one skin per cabinet — `versus_mirror` shape.
   Publish it as an atomic the detour reads.
2. **Name-redirect detour on `FUN_1801ac260(out, dir, name)`** (the resolver every
   `FUN_1801aca30` load goes through, §3.1): when a skin is armed, `dir ==
   "bm2d"`, `name` is on the skin's allowlist (class A/B packages, §5) and
   `data/arc/bm2d/<name>%04d_v0.arc` exists (one `avs_fs_lstat`, cached per
   arm), call the original with `<name>%04d` instead. The original then
   resolves `_v3` (miss) → `_v0` (hit), registers `dance_judge0001_v0.arc`
   with the FileManager and hashes `dance_judge0001_v0.ifs` — the member that
   is actually inside the file. No bytes are touched, nothing is cached, and
   the package vector entry keeps the requested name so every actor lookup is
   unchanged. `FUN_1801ac260` is also called with the `"%s%s"` language variant
   (`<name>_lang_<xx>`) first (`FUN_1801ace90`); the allowlist check fails for it and it passes
   through. Disarmed (skin 0, or any other scene) the detour is a pure
   pass-through — an options-menu toggle never installs/removes it.
   Alternative site with identical effect: `FUN_18006b710` (the `LayoutActor`
   per-package helper) re-implemented with the append — more faithful (the
   record value becomes `dance_judge0001`, exactly A3) but it means rebuilding
   MSVC `std::string` handling for ~40 lines; the resolver detour is a pointer
   swap. Both are gameplay-loader-only paths, not hot.
3. **Class B (gauge)**: the legacy `dance_gauge000N_v0.ifs` exports
   `00_dance_gauge` where World's LifeGaugeActor asks for `dance_gauge`. Serve
   a rewritten `afplist.xml` (+ AP2 exported-name) for that ifs through the
   existing `afplist_ext` machinery, keyed on the legacy ifs path — a static,
   stateless data rewrite that only ever applies when the legacy package is
   loaded, so it needs no per-song bookkeeping.
4. **Class C (combo/score/stage/song_info)**: unchanged from the data route —
   World's AFP stays, the legacy PNGs are staged under World's texture names
   into `data_mods/ddr_selection/dance_<pkg>_v3_ifs/tex/` and
   `ifs_textures::purge_texture_replacement`'d per name at arm time (the
   `s_marvelous::assets::restage_word_art` mechanism); restore the stock set
   at disarm. This is the only part that carries per-song file state, and it
   can be deferred (v1 = classic judge/gauge/messages/danger/FC/game-over with
   World's combo/score/stage).
5. **`GameWork+0xA8 = skin`** (derive the field from the DPS loader's
   identity-table read — `MOVSXD RAX,[RCX+0xA8]` after the `GameWork` load @
   `0x180057b1b`; all-or-nothing, optional): reproduces A3's skin-1 "no
   song-info panel, no option icons" through World's own live gates, and
   makes the `LayoutActor` register the shared packages (`dance_message`,
   `dance_stage`, …) exactly as A3 did for a non-zero skin. Reset to 0 at
   disarm (the credit reset also zeroes it).
6. Disarm on the first scene outside the GAMEPLAY window.

**Why this layer:** it is the A3 mechanism itself, minus the folder trigger.
The files load through the game's own probe/register/hash path, so anything
the engine does with a package (afplist parsing, texture registration, ifs
mounting, release at scene exit) is stock behaviour on stock files. One
detour, no repacking, no cache, no LayeredFS state for class A/B.

**Costs/risks:** one new signature (`FUN_1801ac260` — anchored by its
`"%s_v%d"` / `"%s_lite"` / `"%s.ifs"` string references; its callers and
strings are present on 20250805, the function itself has not yet been
`shape_diff`'d across builds); the class-B afplist
rewrite must be proven (spike); World-only elements (FLARE gauge frames,
`daju_ng_shock`/`daju_ok_shock` shock-arrow words) have no legacy art —
fail-open to World's package for those gauge types or accept absence; any mod
that edits a swapped package's World template (S-Marvelous' `dance_judge`)
must be told the skin is active (§8).

### 6.2 Option B — serve renamed copies through LayeredFS (zero detours, stateful)

The file-layer equivalent: redirect the open of `dance_judge_v3.arc` to a
cached copy of `dance_judge0001_v0.arc` whose member was renamed to
`data/bm2d/dance_judge_v3.ifs` (`core::arc::rewrite_paths`), because the loader
hashes the *requested* name's `.ifs`. Works, and needs no new detour, but it
is a workaround for the missing append rather than the mechanism itself: a
per-skin `_cache` of repacked arcs, a dynamic override map in
`file_hooks::find_mod_replacement`, cache-fingerprint bookkeeping, and the
class-B rename done inside the copy. Keep as the fallback if the resolver
detour cannot be made build-stable, and as the code-free spike vehicle (§6.3).

### 6.3 Option C — static "classic UI" mod folder (no per-song switch, no code)

Option B done by hand for one skin: rename the member of the legacy arc World
already ships and place the result in the LayeredFS mod folder as
`data_mods/<mod>/data/arc/bm2d/dance_judge_v3.arc` (or drop the renamed ifs
through the arc-member overlay path,
`…/dance_judge_v3_arc/data/bm2d/dance_judge_v3.ifs`). No Konami data is
imported and no DLL change is needed, which makes it the fastest way to answer
the class-A/B compatibility questions (§5) on a cabinet before any code is
written. It is also a shippable "always 1st MIX UI" operator mod in its own
right. Once the Option A detour exists, the same test is a config flag.

### 6.4 Option D — transplant A3 code

Not applicable/needed: nothing engine-side is missing.

---

## 7. Game integration

### 7.1 Trigger and UX

World has no DDR SELECTION folder, so the A3 trigger cannot be reproduced
literally. Recommended v1: a PLAYER SETTINGS enum row **`classic_ui`** =
`OFF / AUTO / 1st-5th / MAX-EXTREME / SuperNOVA / X / 2013-A`
(`PersistMode::Local` until a backend column exists — the Multiplayer Bot
precedent). `AUTO` maps the song's `<series>` (World's `musicdb.xml` still
carries it, values 1..21) to an era. Anchors verified in A3's DB: 1 = 1st MIX
(`para`, `trip`, `make`), 2 = 2ndMIX (`puty`, `bril`), 3 = 3rdMIX (`afro`,
`dyna`), 5 = 5thMIX (`feal`), 6 = MAX (`maxx`, `cand`), 9 = SuperNOVA (`xeph`,
`chao`, `fasc`), 10 = SuperNOVA2 (`plur`), 12 = X2 (`poss`, `anti`, `valk`),
16 = A (`egoi`, `ovtp`), 17 = late A/A20 (`endy`, `newc`, `boss`), 18 = A20
PLUS (`aceo`). Proposed mapping (folder-banner year ranges): series 1–5 → skin
1, 6–8 → 2, 9–10 → 3, 11–13 → 4, 14–17 → 5, ≥18 → stock. Confirm the 11–17
boundaries against A3's folder predicates before shipping (§10).

Cabinet-wide effect: the gameplay packages are shared by both sides, so in
versus one skin applies — P1 governs via `services/versus_mirror` (the
song-speed/assist-tick convention).

### 7.2 Folder-based trigger (later)

The `folder_expansion` mod can define custom folders, but membership is
`<property>` bit-based (`docs/folder_system_research.md`), not series-based;
a "DDR SELECTION" folder set would need series-filtered custom folders plus a
"folder → skin" latch at commit (the A3 shape). The folder banner art exists
(§4). Defer.

### 7.3 Lifecycle

- Arm at the SONG_SELECT → GAMEPLAY loader transition (scene callbacks fire
  before `createNextSequence`, so redirects/restage land before the
  `LayoutActor` runs `FUN_1801aca30`). Quick restart (`finish` → fresh DPS)
  reloads packages: keep the arm through the restart; in-place `song_reset`
  keeps the loaded packages — nothing to do.
- Disarm on the first scene ∉ GAMEPLAY window; also restage canonical textures
  so results/next-song loads are stock.
- Training/course: per-stage identity comes from the SSQ-open / bank-create
  observers (`per_song_judgement_offsets::override_hook`); course stages
  batch-load, so the class-C restage would have to happen per stage before
  each stage's loader — v1 may restrict to normal play.

### 7.4 Skin-1 option forcing — recommend NOT porting by default

A3 silently forced speed ×1.0, boost/filter/guideline off and arrow shape 2
for the 1st–5th skin. That is a gameplay change modern players would not
expect; port it, if at all, behind its own toggle (`classic_ui_strict_1st`)
implemented as per-side `Option` field writes with restore, the shape
`per_song_judgement_offsets` uses for `Option+0x24`. The arrow shape part is
harmless and arguably the point of the skin — consider forcing only that.

### 7.5 Era sounds — operator-supplied

The cue audio is Konami data absent from World; the repo will not ship it.
Two viable shapes: (a) operators copy A3's `voice.xwb` + `soundbanks.arc`
banks into a mod folder and the DLL registers a second SE bank
(`game_audio::register_tick_bank` proves an immortal extra bank works; cues
by name through `se_play`), replaying A3's `CallVoiceActor` tables in a small
detour on World's `FUN_1800553b0`; or (b) skip — World's announcer plays. v1 =
(b).

### 7.6 Shutter / stage banner / cut-in

`common_choice000N_v0` (`scene_choice_stage000N_*`) and
`common_choice_cutin000N_v0` are swap candidates for the stage-choice shutter,
but World's shutter package (`common_choice_v2`: `shutter_choice_hd_root`,
`choice_stage`, …) was restructured and the World consumer was not traced.
Phase 2.

### 7.7 Custom Resolution / platform

All legacy art is 720p-era AFP content on the 1280×720 logical canvas — it
scales exactly like World's own packages under Custom Resolution. Nothing
GPU-side changes (same BM2D shaders). No CrossOver-specific concerns.

---

## 8. Interactions with existing mods

- **S-Marvelous** edits the `dance_judge` template at enable (word clone,
  `daju_smarvelous` restage into `dance_judge_v3_ifs/tex`). With a skin
  active the served `dance_judge` is the legacy AFP with `dance_judge000N_*`
  textures: S-Marv's staged texture is simply unused, but its AFP recipe
  (`run_word_clone` on `dance_judge`) targets World's template — the swap
  must either disable S-Marv's judge presentation for that song (fail-open
  to a plain white Marvelous) or run the clone recipe against the legacy AFP
  (its exports match: `dance_judge`, `marvelous`). Decide in design; the
  results-side S-Marv surfaces are unaffected.
- **playfield_styling / overlay_element_styling** capture clips by name
  (`dance_filter_*`, `hidden_cover_*`, `danger_single|double`) — the class-A
  legacy `dance_danger` keeps those names; filter/cover stay stock.
- **Power User Statistics** pacemaker→ms-error patch lives in
  `NoteResultActor` code, not art — unaffected.
- **Background Dancers / movie**: independent (background layer).
- **LayeredFS texture pipeline**: the legacy arcs load through the stock
  path, so LayeredFS sees them like any other stock arc (their
  `texturelist.xml` registers `dance_judge000N_*` as new global texture
  names — harmless). Only the class-C re-texture overlays add mod-folder
  state, and only for the skin currently staged.

---

## 9. Effort and phasing

### 9.1 Phase 0 — spike (1–2 days on a cabinet)

Two ways to run it; pick by what is faster to hand a tester.

- **Code-free (Option C):** rename the member of the World-shipped
  `dance_<pkg>0001_v0.arc` for `dance_judge`, `dance_fast_slow`,
  `dance_game_over`, `dance_message` and drop the results into the mod folder
  under the World names (§6.3).
- **Detour (Option A, preferred if a DLL build is on hand anyway):** add the
  `FUN_1801ac260` name-redirect with a dev-mode env knob (`DDR_CLASSIC_UI=1`)
  that arms skin 1 for every song, allowlist = the same four packages. This
  also proves the signature and the `_v0` probe rung on a real cabinet.

Steps and exit criteria are the same either way:

1. Play a song. Exit criterion: 1st MIX judgement words / READY! / HERE WE
   GO!! render, no WARNs, every `in_*` label resolves (BM2D logs a
   `movieclip is invalid` line for any that does not).
2. Add `dance_danger`, then the class-B `dance_gauge` (afplist + AP2 rename via
   `afplist_ext`) — exercise a LIFE4/RISKY gauge and a FLARE gauge to see the
   failure shape when the legacy clip lacks World's labels.
3. Re-texture `dance_combo_v3` with skin 1 digits by hand (PNG copies under
   `daco_*` names in a `dance_combo_v3_ifs/tex/` overlay) — confirm the
   fixed-geometry scaling look and decide whether class C ships in v1.
4. Resolve the `dance_combo0005_v0` compressed-arc question against the
   runtime `avslz` decoder.

### 9.2 Phase 1 — v1 (≈1–2 weeks after the spike)

- Signature + detour for `FUN_1801ac260` (name redirect), armed-skin atomics,
  per-arm existence cache, allowlist; `shape_diff` across all four builds.
- Pure resolver (`series → era`, option row semantics, versus governance),
  host-tested.
- Optional `GameWork+0xA8` write (derived from the DPS loader's identity-table
  read, all-or-nothing), scene lifecycle, diagnostics (one INFO per armed song
  naming skin + packages redirected; WARN per package that fell back to
  stock).
- Class-B afplist/AP2 rewrite for `dance_gauge000N` (static, via
  `afplist_ext`).
- Class-C texture sets: `scripts/gen_ddr_selection_textures.py` extracting
  the legacy PNGs from the stock World install under World's names, scaled to
  World's rects (no Konami bytes in the repo — generated into `_cache`), plus
  the restage/purge driver — or defer class C to Phase 2.
- Option row + `option_menu_settings` placement + textures via
  `scripts/gen_option_labels.py`.
- S-Marvelous coexistence decision implemented.

### 9.3 Phase 2 — polish / stretch

Class C if deferred, shutter stage banner + cut-in (§7.6), era sounds from
operator-supplied A3 banks (§7.5), skin-1 strict-options toggle (§7.4),
folder-based trigger (§7.2), `dance_common` gauge-frame re-texture,
course/training per-stage arming, a song-select preview of the active skin.

---

## 10. Open RE items

- A3 folder filters for ids `0xB1..0xB5` (which `<series>` values populate
  each `sl0N` folder) — the authoritative `AUTO` mapping. Start from the
  folder table at `0x180263c00` and the folder-property functor
  (`docs/folder_system_research.md` describes World's shape; A3's is the
  ancestor).
- `CourseOption` field names for the skin-1 overrides (§2.5 table is
  inferred from the option-node order; confirm against A3's profile
  load/save marshal).
- Msg `0x100B` consumers beyond `FUN_180030d10` (shutter): search the
  `onMessage` switch tables of the select-music sequence actors.
- World consumer of `common_choice*` shutter art (for §7.6).
- World `dance_gauge` FLARE frame/label set vs. the legacy `dance_gauge000N`
  clips — the exact fail-open shape when `loop_%dlife`/flare labels are
  missing.
- Whether World's BM2D texture registry tolerates two packages exporting the
  same texture name across a song boundary (legacy `dance_judge0001_*` this
  song, none the next) — expected yes (packages are released at scene exit),
  verify in the spike log.

---

## 11. Key addresses

### A3 20240402 (the specification)

| What | Address |
|---|---|
| `GameWork` global / skin field / credit reset | `DAT_1802ed6d0` / `+0xB0` / `FUN_180123060` |
| Skin setter (category 13 ∧ folder `0xB1..0xB5` → 1..5) | `FUN_180123360` |
| Setter call sites (song-select commit; writes `+0x10/+0x14/+0x18` first) | `FUN_1800c53e0` @ `0x1800c5cd5`, `FUN_1800ec8a0` @ `0x1800ed517` |
| Folder category / id getters used at commit | `FUN_1800f45f0` / `FUN_1800f4630` |
| Skin broadcast msg `0x100B` | `FUN_18002e660` (also `FUN_18002e6d0`, `FUN_1800abbd0`) |
| Folder key table (`"slNN"` → `folder_ddrselectionNN`) | `0x180263d40` (inside `0x180263c00..`) |
| Strings `category_name_ddrselection` / `folder_ddrselection0N` / `folder_selection` / `semuca_selection%02d_bnr` | `0x180275e68` / `0x1802770c8..128` / `0x180278490` / `0x180278758` |
| `DancePlaySequence::onInitialize` (LayoutActor ctor with skin) | `FUN_180038b40` |
| `LayoutActor` ctor (`+0x190` = skin) / onInitialize / per-package helper / version probe | `FUN_180049e00` (vtable `0x180269b78`) / `FUN_18004a170` / `FUN_18004a070` / `FUN_1800fe370` (`"%s_v%d"`, `"data/arc/bm2d/%s.arc"`, version count `DAT_1802eee98`) |
| Package record lookup by name (record `+0x28` = skin) | `FUN_18004d830` |
| `ComboActor` init / digit spacing / skin list `{1,2,3}` | `FUN_180046a60` / `FUN_180046970` / `DAT_180265038` |
| Strings `dance_combo%04d[_%s]`, `dance_score%04d_*`, `stage_frame%04d_stage_%s`, `scene_choice_stage%04d_*` | `0x1802693d0/e8`, `0x18026ab30..ac10`, `0x18026ae80..aef0`, `0x180267e10..e70` |
| `GamePlayActor` init (`2d_arrow%02d` from Option `+0xC0`; skin-1 → no OptionIconActor) | `FUN_18003b490` |
| DPS / Matching DPS onUpdate skin-1 SongInfoActor gate | `FUN_180039650` @ `0x180039a0d` / `FUN_180040b60` @ `0x180040f66` |
| `CallVoiceActor` ctor / onUpdate (era voices/SEs) / `sn2_dgm25..` table | `FUN_180036890` / `FUN_1800369e0` / `PTR_s_sn2_dgm25_1802db9c0` |
| `ddr::player::Option` vtable / `CourseOption` vtable / `PlayerWork` ctor installing it at `+0xD0` / option resolver | `0x180280538` / `0x1802806E8` / `FUN_1801284d0` / `FUN_18012ac10` |
| Skin-1 override getters (CourseOption slots `+0x20 +0x30 +0x40 +0x60 +0x70 +0x80 +0xC0 +0xD0 +0xE0`) | `FUN_180126ab0`, `FUN_180126b20`, `FUN_1801267d0`, `FUN_180126bc0`, `FUN_180126c40`, `FUN_180126c00`, `FUN_1801267f0`, `FUN_180126810`, `FUN_180126830` |
| Stage-choice shutter skin art (`actor+0x194`) | `FUN_180030d10` @ `0x180031787..7b7` |

### World 20260825

| What | Address |
|---|---|
| `GameWork` global / skin field / credit reset | `DAT_1806f14f8` / `+0xA8` / `FUN_1801dd6d0` |
| DPS onInitialize identity table + `LayoutActor` ctor call / Matching | `FUN_1800573d0` @ `0x180057af4..b6b` / `FUN_180061520` @ `0x1800619dd..a27` |
| `LayoutActor` ctor / onInitialize / per-package (dead `"%04d"` @ `0x18006b7b3`, `0x18006b82f`; string `0x180361958`) | `FUN_18006b3f0` / `FUN_18006b8b0` / `FUN_18006b710` |
| Version probe (`_v3`, `_v0`, `_lite`, bare) / package register → resolve+hash → arc register | `FUN_1801ac3f0` / `FUN_1801aca30` → `FUN_1801ace90` → `FUN_1801ac260` → `FUN_1801ac160` (`"data/arc/%s/%s.arc"`, `avs_fs_lstat` = `Ordinal_100`, FileManager `DAT_1806f2f50`, `FUN_1801ff6b0`) |
| Skin-1 gates: SongInfoActor (DPS / Matching) / OptionIconActor (GamePlayActor init) | `FUN_180057e10` @ `0x1800582b3` / `FUN_180061cc0` @ `0x180062155` / `FUN_18005be20` @ `0x18005c2fb` |
| `SongInfoActor` ctor / `OptionIconActor` (inline ctor) | `FUN_180078e20` / in `FUN_18005be20` |
| `CallVoiceActor` ctor / onUpdate (no skin) / orphaned `sn2_dgm*` table | `FUN_180055260` / `FUN_1800553b0` / `0x1804653c0` |
| `ComboActor` init (`dance_combo_root%d`) | `FUN_180066250` |
| `NoteResultActor` init (`dance_judge`, `dance_judge_for_freeze`, `dance_fast_slow`, `dance_score_compare`, `dance_effect`) | `FUN_18007aa40` |
| `LifeGaugeActor` init (`dance_gauge`, `loop_%dlife`, `gauge_frame_usr`, `damage_%d_usr`) | `FUN_1800706e0` |
| `DanceDangerActor` init (`danger_single/double`, `danger_gauge`, `game_over`) | `FUN_180068ce0` |
| `StageFrameActor` init (`dance_stage`, marker `stage`) | `FUN_18007a190` |
| Code-composed texture-name strings (`daco_combo%s_%d`, `dasc_score_num_%d`, `dast_stage_`, `dabp_bpm_num_%d`, `daop_icon_%s_%s`) | `0x1803613e0`, `0x180362d40`, `0x180363020`, `0x1803611f8`, `0x180362b90` |

### World 20250805 (presence check)

`LayoutActor` per-package helper `FUN_180068000` (dead `"%04d"` @
`0x180341dc8`), `"%s_v%d"` @ `0x18035ddf4`, `dance_combo_root%d` @
`0x180341800`, `daco_combo%s_%d` @ `0x180341858`, `dance_stage` @
`0x180341e58`, `sn2_dgm_high` @ `0x18035dfa8` — same shape as 20260825.

No Ghidra symbols were added in this pass.

---

## 12. Corrections (2026-09-22 pre-design RE + P0 cabinet test)

Detail: `.agents/planning/2026-09-22-ddr-selection/research/` (`orientation.md`,
`hud-actors.md`, `intro-and-skin-surface.md`, `sounds-options-folder.md`).

### 12.1 Mechanism actually built

The suffix is restored in the **`LayoutActor` per-package helper**
(`FUN_18006b710`, AOB `layout_package_helper`), not at the arc resolver
(§6.1): the helper is fully replaced; stock packages run the original with
skin **0**, legacy packages are registered A3-style under `<base>000N` with
record skin N. `GameWork+0xA8` IS written (§6.1 step 5) — safe only together
with the helper append, because suffixed names never collide with the stage
loader's stock entries (a non-zero skin makes the `LayoutActor` register the
shared set; with unsuffixed names those would dedupe onto loader-owned entries
the `LayoutActor` then erases at finalize). A resolver-level swap would share
package keys and forbid the write. Cabinet-proven 2026-09-22 on all five skins.

### 12.2 Corrected facts

- **`dance_message` has no World consumer.** A3's READY / HERE WE GO player is
  `sequence::dance::ReadyGoActor` (ctor `FUN_180042000` on A3), which World
  deleted; World's READY lives inside the kind-3 `shutter_play` panel. The §5
  "class A" row is wrong — it needs a re-implemented ReadyGoActor. World msg
  `0x100c` is A3's `0x100D` (its only stock sender was ReadyGoActor).
- **`dance_danger` is a SHARED package on World** (`LayoutActor::onInitialize`
  passes `shared = 1`), loaded by the stage loader under skin 0.
- **World's HUD actors still carry A3's skin branches**, keyed on the
  `LayoutActor` record skin (`+0x28`): DanceDangerActor placement (skins 1–2
  centred, 3–5 at the `danger_gauge` marker), gauge intro label (skin 3),
  full-life display (skin 2), StageFrame / SongInfo / layout-builder package
  choice.
- **A missing export NULL-derefs every World HUD actor**, so a package may only
  turn legacy once its consumer is adapted, and the fallback must be the
  unsuffixed World base — `<base>0000` resolves to early-World `*0000_v3` or
  A3-oldest `_v0` arcs with the wrong export names.
- **`dance_common` roots carry positions only** — their textures are
  placeholders; there is no gauge-frame art to re-texture (§5, §9.3).
- **The era sound cues ship in World**: `data/sound/win/voice_n.xwb`
  (byte-identical to A3's), `data/arc/soundbanks_n.arc`,
  `data/arc/se_normal_n.arc` — the A3-generation banks World never loads (§4,
  §7.5 "absent" is wrong; no operator audio needed).
- **Raw series numbering**: 14 = 2013, 15–16 = 2014, **17 = DDR A, 18 = A20,
  19 = A20 PLUS, 20 = A3** (§7.1's 16 = A / 17 = A20 labels are off by one).
  A3's DDR SELECTION membership is a curated list of 54 songs bucketed 1–5 /
  6–8 / 9–10 / 11–13 / **14–17**; the shipped AUTO rule uses those buckets over
  every song.
- **World's `dance_combo0005_v0.arc` is blanked** (decompresses to 518 016
  zero bytes; A3's copy is intact) — `scripts/ddr_selection/import_a3_assets.{sh,bat}`
  copies it from an operator's A3 install into `data_mods/ddr_selection_a3/`.
- **The end-of-song STAGE CLEARED / FAILED banners are the ShutterActor**
  (World kinds 4/5, `common_shutter_v3`), not `dance_game_over` (which is the
  in-lane game-over clip). Legacy banners = `common_shutter000N` + an overlay
  layer — a separate phase.
- Beyond §2: the skin also selected the stage-choice panel's cut-in
  (`common_choice_cutin000N`, SE `sele_*`), the skin-3 SuperNOVA 2 banner as
  jacket, per-skin stage voices, the skin-1 HERE WE GO voice (`ACT3_1` /
  `ACT4_2`), the legacy CLEARED / FAILED / PRAY FOR ALL banners, and `_sel`
  background movies (World MovieActor flag `+0x149`, never set).

