# HUD actors — A3 legacy skins vs World actors (2026-09-22)

Scope: gauge, combo, score, stage frame, song info, layout root, the other
per-side packages, option icons. Goal = FULL fidelity with A3's legacy skins;
for each element: what differs, and whether to (a) adapt data or (b) re-host /
re-implement A3 behaviour inside World.

Builds: A3 final `gamemdx_20240402.dll` (the spec), World `20260825` (current),
World `20250805` (oldest supported; spot-checked). Addresses are file-relative
to `0x180000000`. Data: `$DDR_WORLD_INSTALL/data/arc/bm2d/*.arc`,
`$DDR_A3_INSTALL/data/arc/bm2d/*.arc`. Method: Ghidra decompile (read-only);
arcs unpacked with `scripts/unpack_arc.py`, ifs with `ifstools`, AFP trees
(exports, labels, placed-child names = `movie_name`, first-frame transforms)
from bemaniutils `afputils parseafp` (a sibling checkout) — throwaway scripts,
not committed. `strings` on AFP binaries is useless (scrambled string table).

---

## 0. Cross-cutting findings (read first)

**C1 — World's HUD actors still branch on the skin; they read it from the
LayoutActor record, not from `GameWork`.** `FUN_18006b710` stores the skin in
the record (`+0x28`) whenever its probe succeeds (so already today, if
`GameWork+0xA8` were non-zero). Live A3 skin logic found in World 20260825:

| Actor (World fn) | Skin branch still present |
|---|---|
| LifeGaugeActor init `FUN_1800706e0` | full-lives display state = `skin != 2` (A3 `FUN_18004f4a0` identical) |
| GaugeActor init `FUN_180073cf0` | skin 3 plays root label `1p_in`/`2p_in` (A3 `FUN_180052ec0`; the legacy 0003 gauge root has exactly those labels) |
| DanceDangerActor init `FUN_180068ce0` | skin 0: `danger_single/double` at the `filter` marker + 2nd clip; skins 1–2: centred (640,360); skins 3–5: at the `danger_gauge` marker |
| StageFrameActor `FUN_18007a190`, SongInfoActor `FUN_180078fd0`, marker builder `FUN_18006bd40` | skin 0 ⇒ loader-owned package slot (`*DAT_1806f2d70 + 0x6f0/0x730/0x6b0`), else the record's package — A3's shared-package shape |
| ScoreActor init `FUN_1800775d0` | stores skin at `+0x60` (A3 used it for layer/difficulty branches); no World consumer observed |

Consequence: writing `GameWork+0xA8` **without** restoring the `%04d`
append makes these branches fire on World art (danger flash moves to screen
centre on skins 1–2, etc.) *and* registers shared packages under their
unsuffixed names (orientation finding 4, dedupe onto loader entries). With the
append restored the shared names become `dance_common000N`,
`dance_stage_frame000N` … which never collide with loader entries, so the
orientation hazard disappears for suffixed names. **Append and `+0xA8` must
land together.**

**C2 — Every World HUD actor NULL-derefs when the export it asks for is
missing from the package it was handed.** Pattern (all inits): pool-slot
create `FUN_180257af0(slot, pkg, "<export>", prio)` (or `FUN_180257920`),
validity `vt+0x138` false ⇒ pointer set to 0 ⇒ unconditional `(*p)->vt+0xe8`.
Confirmed in LifeGauge/Gauge (`dance_gauge`), ComboActor
(`dance_combo_root1..3`), ScoreActor (`dance_score`, `dance_difficulty`,
`dance_name`), StageFrameActor (`dance_stage`), SongInfoActor
(`dance_song_info_single/_double`), BpmActor (`dance_bpm`), OptionIconActor
(`dance_option_root`), DPS movie layout (`movie_*_usr` on the loader root,
`FUN_180057e10` case 2). Legacy packages exist for gauge/combo/score/
stage_frame/song_info, so a blanket append would crash 5 actors. The append
must be **per-package allowlisted**, and a package may only be substituted when
its consumer is adapted (a) or re-hosted (b).

**C3 — Fallback trap.** World's probe `FUN_1801ac3f0` tries only `_v3`, `_v0`,
`_lite` (pcType-gated), bare. A3's fallback appends `"0000"`; in World
`<base>0000` resolves to the **early-World `*0000_v3`** arcs (A3-structured
names) or A3's **oldest `_v0`** — never World's `<base>_v3` and never A3's
gold-cab `_v2`. `*0000_v3` exist for bpm, combo, common, filter, gauge, judge,
option, score, song_info, stage_frame, and several export the wrong names for
World's actors: `dance_bpm0000_v3` → `dance_filter` (!), `dance_gauge0000_v3` →
`00_dance_gauge`, `dance_combo0000_v3` → `dance_combo`, `dance_score0000_v3` →
`frame_score`…, `dance_song_info0000_v3` → `dance_song_info`,
`dance_filter0000_v3` → `dance_filter`, `dance_stage_frame0000_v3` →
`stage_frame`. **A restored append must fall back to the unsuffixed base (World
stock), not to `0000`.** (If a design wants "A3 skin-0 art" somewhere, it must
name the arc explicitly — the probe cannot reach `_v2`.)

**C4 — The layout root is positions only.** `dance_common000N` textures are
placeholders (`combo_dummy`, `dam_gauge_hd_1p`, `score_dummy`, … identical
name/size set in 0000_v2 and 0002..0005). There is no gauge-frame art to
re-texture there (the research doc §5 "dance_common" row and §9.3 "gauge-frame
re-texture" are wrong). The legacy look carried by the root = marker
coordinates (§6).

**C5 — Re-host shape.** `song_reset` identifies the gauge family, ScoreActor,
NoteResultActor by **stock RTTI vtable identity** (`signatures.rs:8049`
`find_gauge_vtables`) and pokes stock fields (`song_reset/mod.rs:143`
gauge `+0x90`, `:193` LifeGauge `+0x90`, `:355` ScoreActor `+0x68/+0x6C`).
So a (b) re-host should keep World's actor objects **and** vtables and detour
individual World functions with a skin gate (A3 behaviour ported inside the
detour), not install cloned vtables (the two_player_bpl_mode precedent clones a
vtable — fine there, wrong here). Where a target already has a detour
(center_arrows_single: marker builder + setter + song-info card; s_marvelous:
combo digit refresh; overlay_element_styling: CMovieClip::Create) the new
behaviour must join a shared dispatcher (one-detour rule).

**C6 — A3 itself relied on "missing label / missing texture" behaviour** for
legacy skins: FLARE/GRADE labels absent from legacy `gauge_usr`, stage textures
`_03/_04/_howto/_checking/_encoreextra/_galaxy` absent from most skins, skin-1
score lacks `score_num_0_gray` and `lv%02d`. A (b) port that issues A3's exact
calls on A3's exact data reproduces A3 by construction, whatever libafp does
(`SetFrameLabel` 0xf09 / `load_bitmap` Ordinal_112 on an unknown name) — but
the visible result must be observed once on a cabinet (open Q1).

---

## 1. Life gauge (gauge family)

### A3 (spec)
- Gauge type → actor (GamePlayActor init `FUN_18003b490`, Option vslot `+0xF0`):
  0 NORMAL → NormalGaugeActor; 1/2/3 RISKY/LIFE4/**LIFE8** → LifeGaugeActor
  (lives `FUN_180126420`: 1/4/8); 4 → GradeGaugeActor (`FUN_180054e50`); 5 →
  ImmortalGaugeActor; 6..15 → FlareGaugeActor(level 0..9) = FLARE I–IX, EX
  (`FUN_180055010`). **A3 has FLARE** (no FLOATING FLARE).
- Percent family init `FUN_180052ec0` (shared by all 5 GaugeActor vtables):
  export `00_dance_gauge` (HD) / `00_sd_dance_gauge` (machine type 0/1), marker
  `gauge`, **P2 mirrored** (`SetScale(-1,1)` for side 1, `DAT_1802626d4`),
  `gauge_frame_usr`/`gauge_usr` ← `loop_normal`, `damage_1..8_usr` hidden,
  skin 3 ⇒ root `1p_in`/`2p_in`.
- Update `FUN_180053600`: state → `gauge_usr` label (`loop_normal/rainbow/
  danger/grade/check/fl1..fl9/flex/flare_danger`), danger msgs 0x103a/b/c; fill:
  **skins 2,3,4 or any FLARE state → continuous** (`FUN_1800544b0`), **else
  segmented** (`FUN_180054050`: skin 1 = 63 cells × 6.98 px HD, no partial
  cell; skins 0/5 = 26 cells × 17 px with `fill _2_usr` partial cell) — both
  mirror the scissor for P2.
- LifeGauge init `FUN_18004f4a0`: same exports, marker scale, `gauge_frame_usr`
  ← `loop_%dlife` (4 or 8), `gauge_usr` state via `FUN_180050480`; skin 2 ⇒
  full lives shows `loop_normal` not `loop_rainbow`.
- FLARE/GRADE with a legacy skin: legacy `gauge_usr` has only `loop_danger/
  normal/rainbow` ⇒ the `loop_fl*`/`loop_grade`/`loop_check`/
  `loop_flare_danger` calls hit missing labels (C6); fill continuous.

### World
- Option vslot `+0x230` (`FUN_18005be20`): 0 NORMAL; 1..11 FlareGaugeActor
  (`FUN_1800758d0`) = FLARE I–IX, EX, **11 = FLOATING FLARE (no A3 spec)**;
  12/13 LifeGauge (LIFE4/RISKY, lives vslot `+0x1c8`; ctor `FUN_180070590`,
  frames 4 or 8 exactly like A3; **no LIFE8 gauge**); 14 GradeGauge
  (`FUN_1800756b0`); 15 Immortal.
- GaugeActor init `FUN_180073cf0` = A3 minus SD export, **minus P2 mirror**
  (`SetScale(1,1)` both sides, verified in disassembly @ `0x180073e9b`), export
  fixed `dance_gauge`, msg ids renumbered (0x103e). Update `FUN_1800743d0`: same
  label table (state split into vslots +0x50/+0x58), **fill always continuous**
  `FUN_180074e10` (no segmented path, no P2 scissor mirror). LifeGauge init
  `FUN_1800706e0` = A3 minus SD and minus the skin-3 root label; state labels
  `FUN_1800715d0` ≡ A3.
- 20250805: GaugeActor init `0x180070550`, update `0x180070c30`, fill
  `0x180071650`, LifeGauge init `0x18006cf20`; same shape.

### Legacy data (`dance_gauge000{1..5}_v0`, byte-identical to A3)
Exports `00_dance_gauge`, `00_sd_dance_gauge`, `dance_gauge_gaugeset`,
`dance_gauge_sd_gaugeset`, `gauge_damage8`, `gauge_sd_damage8`, `gauge_frame`,
`gauge_frame_sd`, `game_over_gauge`. `00_dance_gauge` children:
`gauge_frame_usr` (labels `in/loop/out_{normal,1life,4life,8life}`),
`damage_1..8_usr`, `fill _usr`, `fill _2_usr`, `gauge_usr` (`loop_danger`,
`loop_normal`, `loop_rainbow` only). No FLARE/grade/check art. Skin 3 root has
`1p_in`/`2p_in`. World `dance_gauge_v3`: export `dance_gauge`, same child
names, `gauge_usr` has all 16 labels, frame only `normal/4life`.

### Diff
Only one hard incompatibility (export name `00_dance_gauge` vs `dance_gauge`).
Behavioural gaps World removed: P2 mirror, segmented fill (skins 1, 5).
Behaviours World kept (skin-3 intro, skin-2 full-life) work once the record
carries the skin (C1).

### Recommendation — (a) + small (b)
- (a) export alias `00_dance_gauge` → `dance_gauge` for the legacy packages
  (AFP rename in the served ifs — afplist + AP2 exported name — or an export-name
  map inside the shared CMovieClip::Create dispatcher when `pkg` is a legacy
  handle). All other names already match.
- (b) two gated detours: GaugeActor init post-original `SetScale(-1,1)` on side
  1 (legacy skins, percent family only — A3 LifeGauge uses the marker scale,
  no mirror); GaugeActor fill: port A3 `FUN_180054050` (segmented, P2-mirrored)
  for skins 1/5 non-FLARE states and A3's mirrored continuous fill for 2/3/4 and
  FLARE. ~60 + 40 lines, uses the same libafp ordinals (0x1015/0x1016/0x1023).
- FLARE (and GRADE) on legacy skins: A3 behaviour = legacy bar with missing
  labels (C6). FLOATING FLARE has no A3 spec — design choice: same as FLARE
  (mechanically identical actor) or stock World gauge for FLARE types.
- Effort 2–3 days. Risks: song_reset gauge restore (keep stock objects/vtables;
  it relabels through the stock update — our fill detour must stay
  side-effect-free on its latches +0x94/+0x98/+0x9C); World `FUN_180074e10` is a
  new detour target (needs a signature).

---

## 2. Combo

### A3
ComboActor init `FUN_180046a60` (vtable `0x180269488`: init, finalize
`FUN_180046f00`, update `FUN_180046e40`, msg `FUN_180046f70`): ONE clip,
export `dance_combo`, marker `combo`; layer by option vslot `+0x100` (0 ⇒
side+2 prio 10, else layer 0 prio 1 — the combo-priority option). Skin list
`{1,2,3}` (`DAT_180265038`) ⇒ single sheet `dance_combo%04d_{d,combo}`, else
`dance_combo%04d_<grade>_{d,combo}` (worst grade so far). Texture writes
(`FUN_1800470e0`): `combo_usr`, `number_usr/{0001,0010,0100,1000}_usr`,
leading places hidden, cap 9999. **Code-driven layout** (`FUN_180047460`,
`FUN_180047570`): digit cell width from `number_usr/0001_usr` (skin 1 halves
it), `number_usr` scaled by a count-dependent growth factor `FUN_180046970`
(skin 1: 1.0 → 1.1..1.99 for 10–99 → 2.1 ≥100; others: 1.0 → 1.04..1.35 →
1.5 (100–999) → 1.25 (1000+)), clip re-centred per digit count, msg 0x1038
with the new centre; shown only at combo ≥ 4, `GotoAndPlay(0)` replay on each
increment.

### World
ComboActor init `FUN_180066250`: THREE roots `dance_combo_root1..3` (`+0x70`,
`+0x78`, `+0x80`; root2/3 = tinted underlays), marker `combo`, option vslot
`+0x288`. Digit refresh `FUN_180066930` (`combo_digit_refresh`): digit count
via AFP labels `loop_1/10/100/1000` on `combo_usr/number_usr`, textures
`daco_combo_<grade>_%d` (root1) / `daco_combo_%d` + `daco_combo_dummy_*` +
per-grade tint pairs (roots 2/3). Msg `FUN_180066770` (0x1033) calls
`vt+0x90` on all three roots unconditionally. No growth scaling, no skin.
20250805: init `0x180062b40`, refresh `0x180063220`.

### Legacy data
`dance_combo000N_v0`: exports `dance_combo` (children `combo_usr`,
`number_usr/0001..1000_usr`, labels `in/loop`), `dance_combo_old`, `number`;
skins 1–3 `dance_combo000N_{0..9,combo}` (74×77 / 149×47), 4–5 per-grade sets
(+ `gray` in 5). **World's `dance_combo0005_v0.arc` fails to unpack in
`ifstools` too** (not just the repo tools; file mtime differs from the other 54)
— A3's copy is fine; treat World's as damaged.

### Recommendation — (b) re-host
Structural mismatch (1 clip + code layout vs 3 label-driven roots; missing
roots NULL-deref) rules out (a). Port A3 ComboActor behaviour into gated
detours on World's init/msg/update/finalize (keep the World object; use World's
own counters `+0x68` combo / `+0x6C` worst grade, keep the legacy clip in
`+0x70`, keep `+0x78/+0x80` null and never let World's msg/refresh run for a
legacy side). Effort 3–4 days. Risks: **s_marvelous** post-original detour on
`FUN_180066930` (`s_marvelous/combo.rs:6`, fields `:56-57`) must stand down
(and must not be reached); **overlay_element_styling** classifies combo by
`dance_combo_root` prefix (`capture.rs:216`) ⇒ legacy combo escapes the
per-player COMBO scale/opacity rows unless the classifier learns legacy
`dance_combo`; s_marvelous stages `dance_combo_v3_ifs` (`assets.rs:525`) —
inert for legacy packages; combo position depends on §6.

---

## 3. Score (incl. EX score, difficulty/level)

### A3
ScoreActor init `FUN_180055390`: exports `frame_score` (marker `score`, marker
scale applied) and `frame_difficulty_%dp%s` (%s = reverse suffix, marker
`difficulty`; layer 3 for skin 2 else 7); EX flag (`FUN_18012ae70`) toggles
`ex_tex` visibility; name = font text in `name_usr` of the difficulty frame.
Digits `FUN_180055be0`: 7 places `0000001_usr…1000000_usr`, smoothing
`(t+d+1)/2`, `dance_score%04d_score_num_%d`, leading zeros
`_score_num_0_gray` (hidden in EX), `comma1/2_usr` ←
`dance_score%04d_score_comma[_gray]`. Difficulty msg `FUN_180056080`: skin 2
⇒ label `<diff><side>` + base `<diff>_in`; else label `<diff><1|2>` (level ≥
10) + `difficulty_level_usr/level_tex` ← `dance_score%04d_lv%02d`.

### World
Init `FUN_1800775d0`: exports `dance_score`, `dance_difficulty`, `dance_name`
(markers `score`, `difficulty`, `name`); EX ⇒ `score_usr` ←
`dasc_score_exscore`; name via text renderer (`cote_edge_%s`). Digits
`FUN_180077eb0` = A3's routine with names `dasc_score_num_%d` /
`dasc_score_score_num_0_off` / `dasc_score_comma%s`; same fields (`+0x68`
target, `+0x6C` displayed). Difficulty msg `FUN_180078320` (0x104f): label =
difficulty name, `level_<abbr>_usr` ← `dasc_dif_<abbr>_level_%02d`.
20250805: init `0x1800738a0`, digits `0x180074150`.

### Legacy data
`frame_score` (7 digit children; `ex_tex` only in skin 2; no comma children in
1/2/4), `frame_difficulty_{1p,2p}[_reverse]` with `difficulty_level_usr`
(`beginner1/2`…) and (skin 2) `difficulty_level_base_usr` (`<diff>_in`);
**no `name_usr`** in skins 1–5 (legacy UIs showed no player name). Skin 1 has
no `score_num_0_gray` and no `lv%02d` textures; skin 2 carries A3's
`dance_score0000_*` art internally (texture-name overlap with World's
`dance_score0000_v*` arcs — open Q3).

### Recommendation — (b) re-host
Three World exports missing (C2), different difficulty structure, different EX
indicator, name must be absent. Keep the World object (song_reset pokes
`+0x68/+0x6C`, `song_reset/mod.rs:355`; identical in A3) and detour
init/digits/difficulty-msg for legacy skins with the A3 port. Effort 3–4 days.
Risks: song_reset score sentinel must keep meaning "repaint all digits" (the
port's digit loop must honour `+0x6C == -1`); scale from markers (§6).

---

## 4. Stage frame ("1st STAGE" …)

A3 `FUN_180057f60` / `FUN_1800581a0`; World `FUN_18007a190` / `FUN_18007a390`
(20250805 `0x180076460` / `0x180076690`). **Logic identical** (skin 0 ⇒ loader
slot; stage index → `01..04/final/extra/encoreextra/galaxy`, howto, checking;
`stage_number_usr` ← texture). Only names differ:

| | A3 | World |
|---|---|---|
| package key | `dance_stage_frame` | `dance_stage` |
| export | `stage_frame` / `stage_frame_sd` | `dance_stage` |
| marker key (from `stage_frame_usr`) | `stage_frame` | `stage` |
| texture | `stage_frame%04d_stage_%s` | `dast_stage_%s` |

Legacy textures: skin 1 `01,02,extra,final` (+ A3 0000 set), 2/3/5
`01,02,extra,final`, 4 adds `03`. No `howto/checking/encoreextra/galaxy`
(C6). **Recommendation — (b-light):** gated detours on the two World functions
running the A3 code (package via the resolver's `dance_stage` →
`dance_stage_frame000N` map, export `stage_frame`, A3 texture format). (a)
would need a package-name map + export alias + texture-name alias (texture
names are code-composed; aliasing them means rewriting the ifs texturelist).
Effort ~1 day. Risk: the World export `dance_stage` missing ⇒ crash if the map
lands without the detour.

---

## 5. Song info

A3 `FUN_180056cc0`: package `dance_song_info` (skin 0 ⇒ loader slot), export
`dance_song_info` (+ SD suffix for skin 2 on SD), layer 9 for skin 2 else 5,
marker `song_info`, child SongInfoChild `FUN_180057070` writing
`music_name_usr`/`artist_name_usr`. Skin 1 hidden (World gate live:
`FUN_180057e10` @ `0x1800582b3`, `FUN_180061cc0` @ `0x180062155`). Skin 2 =
`dance_song_info0002` (exports `dance_song_info`, `dance_song_info_sd`;
1282-px base + black band; **no text children** ⇒ band only). Skins 3–5 fell
back to **A3's own 0000 panel** (`music_name_usr`/`artist_name_usr`, 378-px
base).

World `FUN_180078fd0` (ctor `FUN_180078e20`, 20250805 init `0x180075270`):
exports `dance_song_info_single/_double`, `jacket_usr` + SongInfoChild
`FUN_1800792e0` (`music_usr/artist_usr/source_usr`); center_arrows_single
detours this builder (`song_info_card_style`, `signatures.rs:818`).

**Recommendation:** skin 1 — `GameWork+0xA8` gate (free). Skin 2 — (b-light)
gated init detour creating the legacy band (no SongInfoChild), must share the
center_arrows detour. Skins 3–5 — **design choice**: World's card (the World
analogue of "current UI") or A3's 0000 panel (needs the explicit
`dance_song_info0000_v2` arc — probe can't reach `_v2`, C3 — plus a port of A3
SongInfoChild's text layout). Effort 0.5 day (skin 2) / +2 days (A3 panel).

---

## 6. Layout root / lanes (markers)

Marker map: shared at `LayoutActor+0x98`, per side `+0xE0/+0x108 + side*0x48`,
entries `{x, y, w, h, sx, sy}`, getter `FUN_18006f100` (missing key ⇒ static
`{0,0,…,1,1}` — **graceful, element lands at (0,0)**), setter `FUN_18006f020`
(`hud_layout_setter`). Builder: A3 `FUN_18004ace0` (`dance_root` /
`dance_root_sd`), World `FUN_18006bd40` (`dance_root`; 20250805 `0x180068630`),
both release the root after reading it.

| Key (consumer) | World builder reads | A3 builder reads | in legacy roots 2–5 |
|---|---|---|---|
| `dance_matching` (MatchingBattleFrame/Info `0x180071d22`/`0x180072156`) | `matching_{usr,left_usr,right_usr}` | same | **no** |
| `stage` / A3 `stage_frame` (StageFrameActor) | `stage_frame_usr` | same | yes |
| `song_info` (SongInfoActor) | `song_info_usr` | same | yes |
| `score` (ScoreActor) | `score_%dp_usr` | same | yes |
| `bpm` (BpmActor `FUN_180065960`) | `bpm_%dp_usr` | — (World-only) | **no** |
| `difficulty` (ScoreActor) | `difficuty_{normal,reverse}_%dp_usr` | `difficuty_normal_%dp[_reverse]_usr` | normal yes / reverse **name differs** |
| `name` (ScoreActor `dance_name`) | `name_{normal,reverse}_%dp_usr` | — | **no** |
| `gauge` | `gauge_%dp_usr` | same | yes |
| `danger_gauge` (DanceDanger skins 3–5) | `danger_gauge_%dp_usr` | same | yes (**absent in World's own root**) |
| `gameover` | `%dp_gameover_usr` | same | yes |
| `option` | `option_%dp_usr` | `option_icon_%dp[_reverse]_usr` | World name **no** |
| `option_icon` (OptionIconActor) | `option_icon_%dp_usr` | — | yes |
| `fullcombo` | `%dp_lane_usr` / `double_lane_usr` | same | yes |
| `judge`, `filter`, `score_compare`, `freeze_judge`, `arrow_raw`/`arrow` (GamePlayActor note renderer + shock lane) | `<lane>/<x>_usr` | same | yes |
| `combo`, `fast_slow` | `<lane>/combo_usr`, `<lane>/fast_slow_usr` | `<lane>/combo_set_usr/{combo,fast_slow}_usr` | **nested — World names miss** |
| movie placement (DPS `FUN_180057e10`, loader root only, **NULL-deref on miss**) | `movie_*_usr` | — | no (irrelevant while the loader root stays World's) |

Per-skin coordinates (1P, first frame; A3 0000_v2 = A3 skin 0/1; World =
`dance_common_v3`): receptor `arrow_usr` (456,477) and `freeze_judge` are
**identical everywhere** (the receptor row never moves). Legacy 2–5 vs World:
score (192,663) vs (281,672); gauge (278,36) vs (280,37); difficulty
(193,622–630) vs (235,630); stage frame (640,36) vs (641,23); judge
(601,601.5) vs (601,611); combo under `combo_set_usr` vs (601,714); option
icon (17.5,~606; skin 2 345.5,655) vs (34,607). A3 0000_v2 differs from
legacy 2–5 only slightly (gauge 289,32; difficulty 208,626; stage 640,42;
danger 451,34). Early-World `dance_common0000_v3` is a third layout (A3 names +
`bpm`/`option`). `fast_usr`/`slow_usr` and the `combo_set_*` exports in legacy
roots have no A3 consumer. Lane art: both builders attach `lane_%s_%s` from the
root package into the transient root (no lasting effect traced; World's lane
background is fill quads + `dance_filter`, `docs/playfield_styling_research.md`
§4b).

**Recommendation — (b):** never hand World's builder a legacy root as-is
(combo/fast_slow/bpm/name/matching/reverse-difficulty land at (0,0)). Keep the
World root for the builder, then in a post-pass read A3-named markers from the
legacy root (`dance_common000N`, 2–5) and overwrite the A3-defined keys via the
setter: score, difficulty, gauge, danger_gauge, gameover, fullcombo, judge,
combo, fast_slow, filter, score_compare, arrow_raw/arrow, freeze_judge, stage,
song_info (World-only keys bpm/name/option/option_icon/dance_matching stay
World's; decide per key whether a hidden element keeps its marker). Skin 1:
A3 used A3's 0000 root — design choice (A3 `dance_common0000_v2` via explicit
path, or World's root). Effort 2–3 days. Risks: **center_arrows_single** owns
detours on the builder entry and the setter (`signatures.rs:769`/`:796`) ⇒
shared dispatcher, and its lane shift must apply after our overwrite;
**two_player_bpl_mode** feeds `LayoutActor+0x98` to the battle frame
(`two_player_bpl_mode/mod.rs:80`) ⇒ keep `dance_matching`; any change that
swaps the loader-owned `dance_common` breaks the DPS movie layout (NULL-deref).

---

## 7. Other per-side / shared packages

| Package (World consumer) | Legacy variants | A3 legacy skins showed | Recommendation |
|---|---|---|---|
| `dance_effect` (NoteResultActor hit flash) | 0000 only | A3 0000 art | keep World stock (design note: A3 = A3 current art) |
| `dance_filter` (LaneFilterActor) | 0000 only (`dance_filter`/`_double`; World `_single/_double`) | A3 0000 | keep World |
| `dance_cover` (CoverActor) | 0000 only (`hidden_cover`/`sudden_cover`; World `_single/_double`) | A3 0000 | keep World |
| `dance_score_compare` | 0000 only | A3 0000 | keep World (PUS pacemaker swap depends on it) |
| `dance_option` (World OptionIconActor, `dance_option_root`) | — (A3 `dance_option_icon0000_v0` has no afplist) | skins 2–5 A3 option icons; skin 1 none | keep World; skin-1 gate free (§8) |
| `dance_bpm` (BpmActor, World-only) | — | nothing | hide for legacy skins (fidelity) or keep — design choice; its `bpm` marker is absent from legacy roots |
| `dance_measure`, `dance_subtitles_*` (CaptionActor) | — | measure consumer not traced; A3 also has a CaptionActor | keep World |
| `dance_danger`, `dance_game_over`, `dance_fullcombo`, `dance_fast_slow`, `dance_judge` | 1–5 | legacy | class-A swaps (research doc); DanceDanger skin placement needs §6 `danger_gauge` |
| `dance_message` | 1–5 | A3 **ReadyGoActor** (`FUN_1800420d0`: `00_ready`/`00_howtoplay`, `00_here`) | World has no ReadyGoActor (RTTI absent); READY lives in the ShutterActor (`FUN_180035170`, labels `ready_loop`/`ready_out`, msgs 0x1047/0x1048) ⇒ (b) re-host ReadyGoActor + suppress the shutter READY for legacy skins (separate design item) |

---

## 8. Option icons (skin-1 gate)

World `FUN_18005be20` @ `0x18005c2fb`: `CMP dword [GameWork+0xA8],1; JZ
0x18005c3a8` skips constructing `sequence::dance::OptionIconActor` (name
"DanceOptionIconActor", 0x68 bytes; init `FUN_180076c80`: package
`dance_option`, export `dance_option_root`, marker `option_icon`, icons
`daop_icon_%s_%s` for gauge/turn/boost/scroll/appearance/…). Confirmed: the
gate still suppresses World's option display for skin 1 (20250805 init
`0x180073110`).

---

## Summary

| Element | World vs legacy | Rec. | Effort | Main risk |
|---|---|---|---|---|
| Gauge (all types) | export name; World dropped P2 mirror + segmented fill | (a) alias + small (b) | 2–3 d | song_reset latches; FLOATING FLARE has no A3 spec |
| Combo | 1 clip + code layout/growth vs 3 label-driven roots | (b) | 3–4 d | s_marvelous combo detour, overlay_element_styling classifier |
| Score/difficulty/EX | 3 exports missing, difficulty/EX/name structure differ | (b) | 3–4 d | song_reset `+0x6C` sentinel |
| Stage frame | same logic, 4 names differ | (b-light) | 1 d | crash if package map lands alone |
| Song info | skin 2 band vs World card; 3–5 A3 panel | (b-light) / design | 0.5–2.5 d | center_arrows card detour |
| Layout markers | name mismatches, World-only keys, nested combo | (b) post-pass | 2–3 d | center_arrows builder/setter detours, BPL matching marker |
| Others | 0000-only / World-only | keep World; ReadyGo re-host separate | — | — |
| Option icons | gate live | free | — | — |

Plus the loader-level prerequisites C1–C3 (append + `+0xA8` together,
per-package allowlist, unsuffixed fallback).

## Open questions

1. libafp behaviour for `SetFrameLabel` (0xf09) with an unknown label and
   `load_bitmap` (Ordinal_112) with an unknown texture — A3's legacy FLARE,
   stage-3/4, skin-1 score paths depend on it. One cabinet observation settles
   it; the port reproduces A3 either way.
2. A3 `frame_difficulty_%dp%s` / `difficuty_normal_%dp%s_usr` suffix argument
   (inferred `""`/`"_reverse"` from the data; not read from the call).
3. BM2D texture namespace: legacy packages embed other skins' names
   (`dance_score0000_*`, `gauge0000_*`, `stage_frame0000_*`) that also exist in
   World's `*0000_v*` arcs — global vs per-package resolution untested.
4. Whether the World `dance_combo0005_v0.arc` is damaged in the install or
   only on disk here (A3's copy extracts; World's fails in `ifstools`).
5. World gauge 14 (GradeGaugeActor) semantics and A3 gauge 4 equivalence —
   actor classes match, option meaning not traced.
6. `dance_measure` consumer and whether A3 legacy skins showed a measure
   display.
7. Lane frame art: both builders attach `lane_%s_%s` into a root they then
   release — whether anything of it survives on screen was not traced.
