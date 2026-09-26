# DDR SELECTION — A20 / A20 PLUS / A3 Gameplay Themes — Research & Plan (2026-09-25)

Status: **RESEARCH ONLY.** Nothing here is implemented or cabinet-tested.

**Question.** World's install still carries the UI data of the last few A-series releases. Can DDR
SELECTION offer those themes (A20, A20 PLUS, A3) wholesale during gameplay, next to its five
legacy eras?

**Method.**

- Every `dance_*` / `common_*` arc in the stock World install (`$DDR_WORLD_INSTALL`) was
  md5-compared against a stock A3 final install (`$DDR_A3_INSTALL`).
- The gameplay packages of every generation were unpacked (`scripts/unpack_arc.py` + `ifstools`).
  Their exports, labels, placed children and embedded `sound_play` cues were dumped with
  bemaniutils `afputils parseafp -d`.
- The texture art was compared visually, and the IFS header timestamps were read.
- A3 `gamemdx_20240402.dll` and World `gamemdx_20260825.dll` were disassembled (capstone) for
  the version probe, the cabinet class and the danger actor. Byte patterns were checked on all
  five supported World builds.

Addresses are file-relative to `0x180000000`. Tags:

- **[dis]** disassembled in this pass.
- **[data]** read from the install in this pass.
- **[notes]** stated in the DDR SELECTION research notes.
- **[inf]** inference.

Builds on `docs/ddr_selection_research.md`, `docs/ddr_selection_custom_skins_feasibility.md`,
and `.agents/planning/2026-09-22-ddr-selection/research/` (in particular `stage-panel.md`,
`intro-and-skin-surface.md`, `hud-actors.md`, `sounds-options-folder.md`).

---

## TL;DR

- **A3's own gameplay UI is in the data, in two colour schemes, plus DDR A's.** Beside World's
  `dance_*_v3` and the five DDR SELECTION eras, the install holds three complete A-series
  gameplay generations under A3's skin-0 names (`*0000_vN`, `dance_message_vN`,
  `common_choice_vN`, `common_shutter_vN`):

  | Suffix | What it is | Evidence |
  |---|---|---|
  | `_v1` | **A3, gold cabinet**: gold gauge / score / stage frames | A3's probe starts at `_v1` on cabinet class 6/7, the same test that picks `ddra3_bg_gold` [dis]; FLARE art; IFS dates 2023 |
  | `_v2` | **A3, every other cabinet**: silver frames | A3's probe starts at `_v2` otherwise [dis]; same design, same image rects as `_v1` |
  | `_v0` | **The DDR A generation**: blue gauge, heavy rounded fonts | IFS dates 2016-03 … 2018-09 (DDR A launched 2016-03); no FLARE art; on A3 it only served packages that have no `_v1` / `_v2` |

  All 171 `dance_*` / `common_*` arcs present in both installs are byte-identical. A3's install
  has no gameplay package that World lacks [data].
- **A20 and A20 PLUS have no gameplay UI of their own in the data.** A3 shipped only A3's art. If
  A20 / A20 PLUS looked different during gameplay, that art is not in A3's or World's install.
  What survives of them is four menu-background movies (`ddra20_bg[_gold]_hd.wmv`,
  `ddra20plus_bg[_gold]_hd.wmv`) and two song-select strings (§3). Whether A20 / A20 PLUS simply
  looked like A3's `_v1` / `_v2` cannot be told from the data.
- **Feasibility is high, and much cheaper than the five eras were.** A3's own packages already
  meet every adapter's contract: exports, labels, children and code-composed texture names were
  checked (§4). A3's gauge even carries the full FLARE label and art set. Most surfaces reduce to
  a package name plus "A3 skin 0" parameters that the adapters already implement for other skins.
- **Engine.** Registering the packages with a record skin ≥ 6 reproduces the skin-0 branches
  World kept from A3 (§5), with one exception: the danger clip. A 2-site scoped byte patch (unique
  on all five builds) closes that gap.
- **One genuinely new component.** The stage panel needs A3's skin-0 fill: the root's own
  `choice_stage` with `scene_choice_stage_{1st,…}` textures, no cut-in. Everything else is table
  rows and match arms.
- **Effort:** ≈ 1–1.5 weeks for "A3 GOLD", "A3 WHITE" and "DDR A" as three new DDR SELECTION
  values (§6). Saved option values stay valid, because new values are appended.
- **Correction.** The earlier notes call `_v2` "HD" and `_v1` "SD". Both are HD, with identical
  image rects; `_v1` is the gold-cabinet set (§8).

---

## 1. What the install contains

Gameplay packages by generation. ✓ = present. "World" = World's own `<base>_v3`.

| Package | `0000_v0` | `0000_v1` | `0000_v2` | `0000_v3` (early World) | World |
|---|---|---|---|---|---|
| `dance_judge` | ✓ (+ Boo word) | ✓ | ✓ | ✓ ("Mervelous!!!") | ✓ |
| `dance_fast_slow` | ✓ | ✓ | ✓ | — | ✓ |
| `dance_combo` | ✓ | ✓ | ✓ | ✓ | ✓ |
| `dance_gauge` | ✓ (no FLARE) | ✓ (FLARE) | ✓ (FLARE) | ✓ | ✓ |
| `dance_score` | ✓ | ✓ | ✓ | ✓ | ✓ |
| `dance_stage_frame` | ✓ | ✓ | ✓ | ✓ | (`dance_stage_v3`) |
| `dance_song_info` | ✓ | ✓ | ✓ | ✓ | ✓ |
| `dance_fullcombo` | ✓ | ✓ | ✓ | — | ✓ |
| `dance_common` (layout root) | ✓ | ✓ | ✓ | ✓ | ✓ |
| `dance_cover` | ✓ | ✓ | ✓ | — | ✓ |
| `dance_danger`, `dance_game_over`, `dance_effect`, `dance_filter`, `dance_measure`, `dance_score_compare`, `dance_option_icon` | ✓ (the only copy; A3 used these on every cabinet) | — | — | filter only | ✓ (World names) |
| `dance_message` (READY / HERE WE GO) | `dance_message_v0` | `_v1` | `_v2` | — | none (World's READY lives in the shutter) |
| `common_choice` (stage panel) | `common_choice_v0` | `_v1` | `_v2` | — | (`common_shutter_v3` kind 3) |
| `common_shutter` (CLEARED / FAILED) | `common_shutter_v0` | `_v1` | `_v2` | — | `common_shutter_v3` |

Art, compared on contact sheets built from the extracted PNGs (not committed):

- **`_v1` / `_v2`:** the same design with the same image rects (for example judge 346×62, gauge
  frame 552×74, score frame 386×48, stage frame 459×80). They differ only in the frame metal:
  `_v1` gold, `_v2` silver. The shutter backgrounds are purple (`_v1`) and teal (`_v2`). The
  judgement words, combo digits, READY / HERE WE GO and CLEARED / FAILED art are identical
  condensed-caps artwork in both.
- **`_v0`:** a different design throughout. Blue chevron gauge, heavy outlined rounded fonts, a
  cyan "READY", heavy "CLEARED", and a `Boo` judgement texture.

**Per-generation IFS header dates** [data], from `data/bm2d/<stem>.ifs`:

| Generation | Dates |
|---|---|
| `_v0` | judge / combo / danger / effect / fullcombo / game_over 2016-03-17..18; message 2017-06; stage frame 2017-07; score 2017-11; common 2018-08; gauge 2018-09 |
| `_v1` / `_v2` | 2023-06-15 for most packages; gauge / stage frame 2023-10; `common_*` 2023-10..12 |
| `0000_v3` | 2024-03 |
| World `_v3` | 2024-05 … 2025-02 |

**FLARE as an era marker.** FLARE is an A3 feature, so it separates the generations [data]:

- `dance_gauge0000_v1` / `_v2`: `00_dance_gauge` has 30 labels, including `loop_fl1..9`,
  `loop_flex`, `loop_flare_danger`, `loop_grade` and `loop_check`, and the matching
  `gauge0000_gauge_flare*` textures.
- `_v0`: 17 labels, none of them FLARE.

---

## 2. How A3 chose between them

**The version probe.**

- A3's arc probe (`FUN_1800fe370` / `FUN_1800fe260`) tries `<name>_v<N>` from `N =
  DAT_1802eee98` down to 0.
- `FUN_1800fe420` sets that start value [dis]:

  ```
  1800fe42d call 0x180011b40 ; cmp eax,6 ; je → DAT = 1
  1800fe437 call 0x180011b40 ; cmp eax,7 ; je → DAT = 1
  1800fe44b mov [DAT_1802eee98], 2
  ```

**The cabinet class.**

- `FUN_180011b40` combines two ark calls. It returns **6 or 7 only when the first is 4**, with
  the second 2 → 6 or 3 → 7 [dis].
- Machine type 4 is the gold cabinet (`docs/bpl_battle_mode_research.md`,
  `docs/3d_model_format_research.md`) [notes].
- Independent confirmation: A3's `sequence::common::BgMovieActor` init (vtable `0x180266d98`,
  slot 4 `FUN_1800294d0`) runs **the same class-6/7 test** to choose `ddra3_bg_gold` over
  `ddra3_bg` [dis].

**Result:**

- A gold cabinet loaded `_v1` → `_v0`.
- Every other cabinet loaded `_v2` → `_v1` → `_v0`.
- Packages with only `_v0` (danger, effect, filter, game over, measure, pacemaker, option icons)
  were the DDR A-generation art on every cabinet.
- `_v0` copies of packages that also have `_v1` / `_v2` were never loaded by A3.

**World** probes `_v3`, `_v0`, `_lite` and then the bare name, so it never loads `_v1` / `_v2` by
itself. An explicit name (`dance_judge0000_v1`) resolves through the bare rung. The inner member
`data/bm2d/dance_judge0000_v1.ifs` hashes correctly. This is the mechanism DDR SELECTION already
uses for `dance_song_info0000_v2`, `dance_common0000_v2` and `common_choice_v2`.

**Menu backgrounds** (`data/mdb_apx/movie/background/`, byte-identical in both installs):

- A3's `BgMovieActor` names only `ddra3_bg`, `ddra3_bg_gold`, `ddra3_bg_galaxy` and
  `ddra3_bg_galaxy_ending` [dis].
- The A20 / A20 PLUS movies are unreferenced leftovers.
- World's binary names none of them. They are **menu** backgrounds [inf: the actor lives in
  `sequence::common`]; theming menus is out of scope here.

---

## 3. A20 and A20 PLUS

- **No gameplay package in either install is specific to A20 or A20 PLUS.** Every suffix is
  accounted for: `_v0` is DDR A-era, `_v1` / `_v2` are A3's 2023 builds, `0000_v3` and `_v3` are
  World. A3's install is a strict subset of World's.
- **What does survive:**
  - the four `ddra20*_bg*_hd.wmv` menu movies;
  - the song-select strings `folder_version_ddra20` / `folder_version_ddra20_plus` (A3 binary).
- **So "A20" or "A20 PLUS" gameplay themes cannot come from World's data.** Three honest options:
  1. **Present A3's sets as what they are.** "A3 GOLD" / "A3 WHITE". If A20 / A20 PLUS shared the
     design, that already is their look; the data cannot say.
  2. **Import from an operator's A20 / A20 PLUS install**, like `import_a3_assets`.
     - Their files would carry the same names as A3's (`dance_judge0000_v1.arc`, …), so the import
       must alias them. Example: `dance_judge0000_v1` → `dance_judge_a20p_v1`, with the inner IFS
       renamed via `core::arc::rewrite_paths` (the `bg_preview_overlay::ensure_alias_arc`
       precedent).
     - The texture names inside stay `dance_judge0000_*` (see §7 risk 1).
     - That their packages have A3's structure is [inf]. It needs the export preflight from
       `docs/ddr_selection_custom_skins_feasibility.md` §4 before registering.
  3. **User re-skins** of A3's packages (the Tier 0 route of the custom-skins doc).

---

## 4. A3's skin-0 surface, element by element

"Skin 0" is how A3 labelled its own UI. For each element: what A3 did on skin 0, the package, what
DDR SELECTION does today for its eras, and what an A3 theme needs.

| Element | A3 skin 0 | Package (`vN` = the theme's suffix) | DDR SELECTION today | Needed for the theme |
|---|---|---|---|---|
| Judgement words | whole package | `dance_judge0000_vN` (exports `dance_judge`, `dance_judge_for_freeze`; labels `in_*` incl. `in_boo`) | whole-package swap | fixed-arc row |
| FAST / SLOW | whole package | `dance_fast_slow0000_vN` (`dance_fast_slow`, `in_fast` / `in_slow`) | swap | fixed-arc row |
| Full combo | whole package; clip plays `XAC_full_combo2` | `dance_fullcombo0000_vN` (the 16 World export names) | swap + `code_se` flip | fixed-arc row (the flip applies unchanged) |
| Game over / danger | whole package | `dance_game_over0000_v0`, `dance_danger0000_v0` (`danger_single` / `_double`) | swap; placement from the record skin | fixed-arc rows + the §5 danger patch |
| Life gauge | export `00_dance_gauge`; **P2 mirrored**; **segmented 26 cells × 17 px with a partial cell** (A3 `FUN_180054050`: "skins 0/5"); continuous in FLARE states; eased; rainbow at full lives; no intro [notes] | `dance_gauge0000_vN`: 30 labels incl. every FLARE state (`_v0`: 17, no FLARE); `fill _usr`, `fill _2_usr`, `damage_1..8_usr`; base 442 px = 26 × 17 | gauge adapter (export alias, mirror, fill) | the 2013-A fill profile; engine class ≥ 6 (§5). **First legacy-structured gauge with real FLARE art** |
| Combo | one clip; per-grade sheets (A3's skin list `{1,2,3}` is single-sheet, so skins 0/4/5 are per-grade); standard growth; full cells [notes] | `dance_combo0000_vN` (`dance_combo`: `in` / `loop`, `combo_usr`, `number_usr/0001..1000_usr`; textures `dance_combo0000_{marvelous,perfect,great,good}_{0..9,combo}`) | combo adapter | the X / 2013-A profile with texture prefix `dance_combo0000` |
| Score / difficulty / EX | `frame_score` 7 places, commas, grey zeros, `ex_tex`; level-texture difficulty (prio 7); **player name in `name_usr`** [notes] | `dance_score0000_vN` (`frame_score` incl. `comma1/2_usr`, `ex_tex`; `frame_difficulty_{1,2}p[_reverse]` with `difficulty_level_usr`, `level_tex`, **`name_usr`**; `difficulty_level_base`; `lv00..20`, `score_num_0_gray`) | score adapter (no name: the eras' frames have no `name_usr`) | non-skin-2 profile, prefix `dance_score0000`. Name text = follow-up (§6.3) |
| Stage frame | `stage_frame%04d_stage_%s` | `dance_stage_frame0000_vN` (`stage_frame`: `stage_frame_usr`, `stage_number_usr`; textures `stage_frame0000_stage_{01..14, final, extra, encoreextra, howto, checking, galaxy}`) — **every suffix World uses** (`_v0` lacks `checking` / `galaxy`) | stage-frame adapter (prefix patch) | one more near-buffer prefix `stage_frame0000_stage_` (22 chars, same length) |
| Song info | A3's panel with title / artist | `dance_song_info0000_vN` (`music_name_usr`, `artist_name_usr`) | panel mode for skins 3–5 (`_v2` only) | panel mode with the theme's suffix |
| Layout (positions) | A3's root | `dance_common0000_vN` | markers post-pass (skin 1 already uses `_v2`) | root name per theme; BPM / name keys hidden, as today |
| Option icons | A3's icon row | `dance_option_icon0000_v0` | skins 2–5 | same package, skin range extended |
| Pacemaker | A3's | `dance_score_compare0000_v0` | every era | unchanged |
| Hit flash | A3's | `dance_effect0000_v0`: export `dance_effect` **matches World's** | World's | optional swap (children not checked; `playfield_styling` captures it by export name) |
| Measure | A3's | `dance_measure0000_v0`: exports match World's `dance_measure_big` / `_small` | World's | optional (consumer not traced) |
| Lane filter / cover | A3's | `dance_filter0000_v0` (`dance_filter`, `dance_filter_double`), `dance_cover0000_vN` (`hidden_cover`, `sudden_cover`) — **World asks for `*_single` / `*_double`** | World's | keep World's (a missing export crashes the actor) |
| READY / HERE WE GO | ReadyGoActor, stock `dance_message`; no HERE voice; `00_ready` embeds `vo_ingame_ready` [notes] | `dance_message_vN` (`00_ready`, `00_here`, `00_howtoplay`; labels `in` / `loop` / `loop_end` / `out` / `end`) | ReadyGo adapter (`dance_message000N`) | fixed-arc package name; no HERE voice (already so for skins ≠ 1) |
| Stage panel | root `shutter_choice_hd_root` from `common_choice`. **Skin-0 fill**: `choice_stage_usr2` hidden; `choice_stage_usr` ← the root package's own `choice_stage` with `choice_stage_usr/scene_choice_stage_usr` ← `scene_choice_stage_{1st,2nd,3rd,4th,final,extra}` (special stages: `_fl%s`, `_galaxy`, `_encore` into `scene_choice_stage1/2_usr`); `choice_background_usr` ← `choice_background[_%s]`; song jacket; `vo_stage_*` at once; **no cut-in** [dis: string references of A3 `FUN_180030d10` `0x180031ae1..0x1800320e1`; control flow not traced] | `common_choice_vN`: same root children as `_v2` incl. the adoption check `choice_stage_usr2`; stage textures in the package; sounds `Plate_spin3_st`, `STG_APP02`, `banner_in`, `se_shutter_in/out` | hosts `common_choice_v2` with the eras' sub-clips (legacy fill) | **new "A3-own" fill** (§6.2); per-theme root package string |
| End banners | kinds CLEARED / PRAY FOR ALL / FAILED; root + overlay | `common_shutter_vN`: roots `shutter_clear` / `shutter_failed` (`se_shutter_in/out`), overlays `00_cleared` (`STG_APP02`, `STG_APP03`, `vo_stage_clear`), `00_failed`, **`00_prayforall` with art** | banner adapter (`common_shutter000N`) | package per theme; PRAY FOR ALL on (A3's Tohoku EVOLVED rule) |
| Announcer / crowd | A3's skin-0 column (`sounds-options-folder.md` §A.1): combo calls as skins 4–5; crowd: `g > 0.7` `STG_APP02` + `vo_ingame_cheer`, low `STG_BOO` + `vo_ingame_boo`, else `STG_APP03` + `vo_ingame_cheer` | cues already in the `dsel` bank (`sound/cues.rs`) | skins 1–5 profiles | one more `rules.rs` profile |
| Stage voice | `vo_stage_*` | dsel | skins 4–5 profile | same profile |
| `_sel` movies | only from the DDR SELECTION folder | — | any armed era | off for the themes (A3's rule; a maintainer call) |
| 1st-5th option forcing | none | — | skin 1 only | none |

**Sounds.** Every cue embedded in these clips is already in the `dsel` era bank: `XAC_full_combo2`,
`Plate_spin3_st`, `STG_APP02` / `03`, `vo_stage_clear`, `vo_ingame_ready`, `se_shutter_in` /
`out`, `banner_in`. So are the announcer cues. **The bank needs no change.**

**`_v0` (DDR A).** It has the same structure throughout: the same exports and children, plus
`name_usr` and `ex_tex` in the score frames, and `choice_stage_usr2` in its panel root. Its gauge
lacks the FLARE labels, as the five eras' gauges do. How DDR A's own code behaved is unknown:
DDR A's binary is not available, so the theme would use A3's skin-0 rules [inf].

---

## 5. Engine behaviour: the record skin for a theme

`docs/ddr_selection_custom_skins_feasibility.md` §2.2–2.3 lists World's readers of the package
record's skin. **Each actor reads the record of its own package**, so the value can be chosen per
package. For an A3 theme, a record skin ≥ 6 (the "neutral" class) gives:

| Actor | Skin 0 (World's code, which kept A3's branches) | Record skin ≥ 6 | Match |
|---|---|---|---|
| Percent gauge | eased, no intro | eased, no intro | ✓ |
| LIFE gauge | rainbow at full lives | rainbow | ✓ |
| Stage frame / song info | the stage loader's package: A3's own art on A3, **World's art on World** | the record's package (the theme's) | ✓ — a record skin ≠ 0 is required, or the theme gets World's art |
| Danger | `filter` marker, layer 0 / prio 6; `danger_double` on doubles; a **second copy** of the clip at the marker on layer side+2 | `filter` marker, layer 0 / prio 6 — but `danger_single` always, and no second copy | ✗ (doubles art, second copy) |

A3's own second danger clip was the `*_failed` variant (`research/intro-and-skin-surface.md` §4
row 18) [notes]. World's skin-0 path creates the same export twice. The patch below reproduces
World's skin-0 path, not A3's `_failed` overlay.

**The danger gap closes with two scoped patches** in DanceDangerActor init (`FUN_180068ce0` on
20260825) [dis]:

| Site | Bytes (20260825) | What it does | Patch |
|---|---|---|---|
| A | `0x180068d8c`: `CMP [RDI+0xB4],0; LEA RSI,["danger_single"]; LEA R15,["danger_double"]; MOV R8,RSI; JNZ +7; TEST R13B,R13B; CMOVNE R8,R15` | skin ≠ 0 skips the doubles choice | `JNZ +7` (`75 07`) → `90 90` |
| B | `0x180068eb6`: `CMP [RDI+0xB4],0; JNZ rel32; TEST R13B,R13B; MOV RDX,RBP; MOV RCX,R14; CMOVNE RSI,R15` | skin ≠ 0 skips the second clip | `JNZ rel32` (6 bytes) → 6 × `90` |

- **Uniqueness.** Each pattern (with the `+0xB4` displacement pinned) is unique on all five builds
  [dis]:

  | Build | Site A | Site B |
  |---|---|---|
  | 20250805 | file `0x64b3c` | `0x64c66` |
  | 20260224 | `0x63b8c` | `0x63cb6` |
  | 20260721 | `0x681ac` | `0x682d6` |
  | 20260825 | `0x6818c` | `0x682b6` |
  | 20260915 | `0x6895c` | `0x68a86` |

- **Why this reproduces skin 0 exactly.** For a record skin ≥ 6, the position branch
  (`test; jle` / `cmp 2; jle` / `cmp 5; jg`) and the layer branch (`add -3; cmp 2; ja`) already
  land on skin 0's paths. With both jumps removed, the record's package behaves as skin 0.
- **Scoping.** Apply the patches only while an A3 theme's `dance_danger` is registered: set them in
  the package helper before the record insert, and restore them on a stock request, at disarm and
  at disable. This is the `stage_frame` / `gauge` / `song_info` pattern. The five eras keep
  A3's `danger_single`-only behaviour, which is authentic for them.

**`GameWork+0xA8`** must stay in 0..=5 (the DPS table) and must not be 1, or the song info and
option icons disappear. Write 0 (or 2): once a skin is armed, nothing else reads the field, because
the helper ignores `LayoutActor+0x190`.

---

## 6. Implementation plan

### 6.1 Minimal route: three appended skins

This route needs no catalog refactor. It is compatible with the refactor proposed in
`docs/ddr_selection_custom_skins_feasibility.md` §6, where these entries later become catalog
rows. Doing the themes first also proves the neutral engine class and fixed-arc naming that the
catalog needs.

| Row value | Label | Internal skin | Suffix |
|---|---|---|---|
| 7 | `A3 WHITE` | 6 | `_v2` |
| 8 | `A3 GOLD` | 7 | `_v1` |
| 9 | `DDR A` | 8 | `_v0` |

The row maps value → skin as `value − 1`, exactly as today. Appending leaves 0..=6 meaning what
they mean, so existing JSON caches load unchanged; an older DLL clamps 7..=9 to OFF.

| Area | Change |
|---|---|
| `trigger.rs` | `ROW_MAX` 9, three labels (≤ 15 bytes), dev knob 1..=8 |
| `policy.rs` | `SKIN_MAX` 8; `Entry.skins` → `u16` (bit 8 overflows `u8`); a theme helper computing each base's fixed arc: `<arc_base>0000<suffix>`, `_v0` for danger / game over / option icons / pacemaker, `dance_message<suffix>`. The existing "no bare `0000`" test already accepts `0000_vN` fixed arcs |
| `mod.rs` | `write_skin`: skins 6..=8 → `GameWork+0xA8 = 0`; the record skin stays 6..=8 |
| Texture numbering | new `tex_number(skin)`: 1..=5 → itself, 6..=8 → 0. Used by `combo_math::sheet_prefix`, `score_math` and the stage-frame prefix (option icons are `0000` already) |
| Adapters' `1..=5` checks | `gauge.rs:246`, `combo.rs:561` (+ `PACKAGE_STATE` → 9 entries), `score.rs:325`, `stage_frame.rs:193` (slots up to 8, reach check to the last slot), `option_icons.rs:231` (2..=8) |
| Per-skin arms | `gauge_math::fill_mode` 6..=8 = segmented 26 + partial (today's `else` branch; make it explicit and test it); `combo_math` standard / full / per-grade (already the defaults for non-1..3); `score_math` non-skin-2 (already the default); `song_info_logic::mode_for_skin` 6..=8 → Panel; `marker_keys::root_name` → `dance_common0000<suffix>`; `sound/rules.rs` skin-0 profile; `banner_logic::has_pray_for_all` true for 6..=8; `movie_sel` skipped for 6..=8 |
| `intro.rs` | take the `dance_message` package name from the policy (today it builds `legacy_name("dance_message", skin)` itself) |
| `panel_logic.rs` / `panel.rs` | theme variant: root package `common_choice<suffix>` (static per-theme strings for the row patch), no era packages, no cut-in, jacket = song, stage voice `vo`, **A3-own fill** (§6.2) |
| `banner.rs` | `PACKAGES` → one entry per skin (`common_shutter<suffix>` for 6..=8) |
| Danger | new signatures for §5's two sites (sweep + `shape_diff.py`), scoped apply / restore |
| S-Marvelous | nothing: it stands down on any legacy judge / full-combo package whose skin is not in `LEGACY_SKINS` (the current fail-safe). Theme art is a later add-on |
| Tests | new arms in `scripts/validate_ddr_selection.sh`: policy names per theme, `tex_number`, fill / combo / score profiles, rules profile, row labels |

### 6.2 The A3-own stage-panel fill

Only the stage panel needs code that does not exist yet:

1. **RE (≈ 0.5 day):** trace the control flow of A3 `FUN_180030d10`'s skin-0 branch around
   `0x180031ae1..0x1800320e1`. Needed: which stage index picks which `scene_choice_stage_*`
   texture, when `scene_choice_stage1/2_usr` and the `choice_background_%s` variants are used,
   and what `caution_usr` does.
2. **Implementation (≈ 1.5 days):** in the adopted root:
   - hide `choice_stage_usr2`;
   - `load_movie` `choice_stage` from the root's package into `choice_stage_usr` and set its
     texture;
   - leave `choice_background_usr` at the root's default, or its variant;
   - set the song jacket (the existing `Jacket::Song` path);
   - play the stage call at once. The existing voice gate already fires at once when
     `choice_stage_usr2` has no `voice` label.

   The session, row patch, READY dismissal and release logic are the existing ones.

The score sets (high score, rank, FC mark, dancer name, target) are hidden today for the eras.
A3 showed them on every skin, so the themes inherit the same follow-up.

### 6.3 Follow-ups (not needed for a first cut)

- **Player name on the score frame.** A3 drew it as font text in `frame_difficulty_*/name_usr`.
  This needs a text child, like the song-info panel's SongInfoChild patch, or a DLL text widget.
  1–2 days.
- **Panel score sets** (shared with the eras' follow-up). 2–4 days.
- **Optional whole swaps:** `dance_effect0000_v0` (hit flash), after checking its children and
  labels against NoteResultActor's use; `dance_measure0000_v0`, after tracing its consumer.
- **Gold / white for the eras' shared A3 pieces.** On a gold cabinet, A3 drew the five eras' panel
  root, the skins 3–5 song-info panel and the skin-1 layout root from `_v1`; DDR SELECTION always
  uses `_v2`. A global "A3 style: gold / white" setting could drive both the eras and the themes.
- **S-Marvelous art for the themes**, generated like the eras' art
  (`scripts/gen_ddr_selection_smarv_art.py` recipes).

### 6.4 Effort

| Piece | Days |
|---|---|
| Policy, trigger, texture numbering, adapter ranges and arms, banners, announcer profile, tests | 2–3 |
| Danger patch (signatures, sweep, scoped apply) | 1 |
| A3-own panel fill (RE + implementation) | 2 |
| Cabinet pass (§7 matrix) | 1 |
| **Core total** | **≈ 1–1.5 weeks** |
| Follow-ups (§6.3) | +1 week |

---

## 7. Risks and cabinet checks

| # | Risk | Check |
|---|---|---|
| 1 | **Identical texture names across generations.** `_v0`, `_v1`, `_v2` and `0000_v3` all name their textures `dance_judge0000_*`, `gauge0000_*`, `dance_score0000_*`, … If BM2D texture names resolve globally and a registration outlives its package, consecutive songs on different themes could show the previous theme's pixels (`research/hud-actors.md` open Q3) | Play A3 GOLD → A3 WHITE → DDR A → A3 GOLD back-to-back, plus a skins 3–5 song (which loads `dance_song_info0000_v2`) between them; the frame colours must switch every song |
| 2 | Panel fill conditions known from strings only | §6.2 RE step first |
| 3 | Danger patch is a new code-byte patch | sweep + `shape_diff.py`; singles and doubles danger on a theme song and on an era song after it |
| 4 | Any A3 skin-0 behaviour World removed that the eras never needed | the ports already cover A3's removed gauge, combo and score logic "for skins 0/…" (`hud-actors.md`); watch the first cabinet run for surprises |
| 5 | `_v0` = DDR A is dated, not proven by code | label it "DDR A" only if the maintainer accepts the evidence (§1) |
| 6 | FLARE / FLOATING FLARE / GRADE / LIFE4 / RISKY on the A3 gauge | one song per gauge type on A3 GOLD; the FLARE art should appear (it never does on the eras) |

Standard matrix: each theme × {1P, versus, bot} × {normal, reverse, doubles} × {quick restart,
quick fail} × {stock → theme → era → stock}; S-Marvelous, Center Arrows, overlay element styling
and playfield styling enabled together.

---

## 8. Corrections to earlier notes

- **`_v1` is the gold-cabinet set, not SD.**
  - `research/legacy-score.md` §7.1 ("HD … `_v2` (SD: `_v1`)") and `research/hud-actors.md` C3
    ("A3's gold-cab `_v2`") are wrong: `FUN_180011b40`'s classes 6/7 are the gold cabinet (§2).
  - Both sets are HD; their HD image rects are identical. Only the SD sub-textures
    (`gauge0000sd_*`) differ.
  - Consequence for the shipped eras: DDR SELECTION's `_v2` choices (panel root, skins 3–5 song
    info, skin-1 layout) are the **white**-cabinet look. That is authentic for a white cabinet,
    not for a gold one (§6.3).
- `docs/ddr_selection_custom_skins_feasibility.md` §5.4 has the same error and names `_v0`'s era
  as unknown. Both are corrected there, with a pointer to this document.

---

## 9. Key addresses

| What | Address |
|---|---|
| A3 version-start setter (gold → 1, else 2) / probe / start global | `FUN_1800fe420` / `FUN_1800fe370`, `FUN_1800fe260` / `DAT_1802eee98` |
| A3 cabinet class (6/7 = machine type 4 with PC type 2/3) | `FUN_180011b40` |
| A3 `sequence::common::BgMovieActor` (vtable / slot-4 init; `ddra3_bg` vs `ddra3_bg_gold`) | `0x180266d98` / `FUN_1800294d0` |
| A3 stage-panel fill; skin-0 branch string refs | `FUN_180030d10`; `0x180031ae1..0x1800320e1` |
| World DanceDangerActor init (20260825); danger sites A / B | `FUN_180068ce0`; `0x180068d8c` / `0x180068eb6` |
