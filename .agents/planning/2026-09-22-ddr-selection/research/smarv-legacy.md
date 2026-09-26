# S-Marvelous on the legacy skins (Step 13 RE, 2026-09-25)

Design §4.9 P6. Templates dumped from the 20260915 World install
(`data/arc/bm2d/`, `arctool` + `ifstools`) and skin 5's combo from A3
(`gamemdx_20240402` install; World's copy is blanked). The DLL's real
recipes (`core/ap2`, `core/geo`) were run on every legacy template through
the `validate_s_marvelous.sh` `ap2check` binary.

## 1. What S-Marvelous does on World's art today

| surface | mechanism | code | on a legacy song today |
|---|---|---|---|
| judgement word | enable-time staging of `dance_judge_v3`: geo-first word chain (`find_word_shape_by_geo("in_marvelous", "marvelous")`), dry-run of `run_word_clone` (clone `in_marvelous` → `in_smarvelous` onto a fresh shape + sprite, additive-glow mutes), rewritten geo (region `daju_marvelous` → `daju_smarvelous`), donor-anchored atlas clone, per-image PNG, geo MD5 mapping, afplist `<geo>` extension. Patch fn (`afp_patcher`, key `dance_judge`) byte-gates on the staged stock bytes | `assets::stage`, `afp_patches.rs` | patch fn returns `None` on `legacy_package("dance_judge")` |
| word re-drive | post-original on every S-Marv event: NoteResultActor (RTTI) `+0xA0` wrapper → MC `+0x110` → `mc_op(0xF09, "in_smarvelous")` | `flash.rs` | returns after the FAST/SLOW hide on `legacy_package("dance_judge")` |
| S-MFC splash | staging of the four `dance_fullcombo_v3` templates: geo-first resolution of every shape whose region's last token starts `mar` (exactly 4 on World), `clone_segment_with_new_shapes("marbelous_in" → "s_marbelous_in")`, per-template geos, one donor-anchored atlas batch. Re-drive: post-original FullcomboActor msg `0x1034`, type 0 ∧ `combo_is_all_smarv(side)` ⇒ `mc_op(0xF09, "s_marbelous_in")` on `+0x98` | `assets::stage_fullcombo`, `splash.rs` | both the patch closure and the re-drive return on `legacy_package("dance_fullcombo")` |
| combo | refresh POST subscriber on `combo_hooks`: worst `+0x6C == 0` ∧ all-S ⇒ places 10/100/1000 → `daco_combo_smarvelous_%d` (FRESH set in `dance_combo_v3_ifs`), violet tint on roots 2/3 | `combo.rs`, `assets::stage_combo_digits` | never runs: the legacy refresh is an OVERRIDE, which skips the POST list (`combo_hooks::refresh_hook`) |
| FAST/SLOW | one-byte gate patch (`note_result_fast_slow_gate`, Marvelous shows FAST/SLOW) + `hide_for_smarvelous` on the `+0xA8` clip | `fast_slow.rs` | unchanged: code-level, package-independent; the hide runs before the legacy stand-down |
| receptor burst | `JudgeEffectRenderer::push` type 7 + `playfield_styling` fill-hook recolour | `receptor.rs` | unchanged: renderer-level; DDR SELECTION has no legacy `dance_effect` / receptor |
| results, lamps, upload | World's result scene, song select, `/data/s_marv` | `results_*`, `lamp*`, `upload*` | unchanged: DDR SELECTION is gameplay-only (results stay World's) |

## 2. Legacy templates

### 2.1 `dance_judge000N_v0` (skins 1–5)

Same family as World's `dance_judge_v3`: exports `aep_mask_dummy` (6),
`aeplibset` (3), `dance_judge`, the Marvelous word sprite (`marvelous`;
World `dance_marvelous`); root 600 frames plus an inner timeline sprite
with the same labels (`in_marvelous` 0, `in_perfect` 38, `in_great` 80,
`in_good` 124, `in_boo` 170, `in_miss` 212, `in_ok` 253, `in_ng` 292 —
World has no `in_boo` and different offsets from `in_miss`). `in_marvelous`
places the word sprite at depth 2 (object 32, removed f32) and a flash
shape at depth 3; `stop` DoAction at f33 — identical to World.

| skin | word sprite | word shape | region (imgrect px) | additive self-glow |
|---|---|---|---|---|
| 1 | 35 | 32 | `dance_judge0001_marvelous` 346×63 | yes: `NEW d3 blend 8`, mult α 0.5 → 0.25 → 0, 5-frame loop |
| 2 | 35 | 32 | `dance_judge0002_marvelous` 346×63 | yes (same) |
| 3 | 34 | 32 | `dance_judge0003_marvelous` 346×63 | **none** (one placement only) |
| 4 | 35 | 32 | `dance_judge0004_marvelous` 346×63 | yes (same) |
| 5 | 35 | 32 | `dance_judge0005_marvelous` 346×63 | yes (same) |

World's word sprite nests one level deeper (sprite 46 → sprite → shape 41)
and its pulse is α 0.20 → 0.098 → 0. The descrambled AFPs differ per skin
(five distinct md5s). One atlas `tex000` per IFS, served per image
(`tex/md5(image)`), like World.

### 2.2 `dance_fullcombo000N_v0`

Same four templates as World (`01_fullcombo_single_{normal,reverse}`,
`02_fullcombo_double_{normal,reverse}`) and the same labels (`marbelous_in`
1, `perfect_in` 87, `great_in` 179, `good_in` 281, root + inner timeline).
**Difference:** the legacy `marbelous_in` segment plays its own sound —
`asdlib.sound_play("XAC_full_combo2")` at f2 and `stop` at f81 (World plays
`se_game_fullcombo` from code; DDR SELECTION's `code_se` silences that on
legacy songs). The placements-only segment clone copies non-definition tags,
DoActions included, so `s_marbelous_in` plays the same cue once.

Marvelous-coloured regions (`mar*` last token): five on every skin —
`dafu_eff_mar` (the "MARVELOUS FULLCOMBO!!!" text), `dafu_light_marvelous`,
`dafu_ring_marvelous`, `dafu_rsring01_marvelous`,
`dafu_side_light_marvelous` (World: four — eff, light, rocket, side light).
Texture names are the same on every skin and World's, so staging must be
per IFS (it is: `atlas_cloner` caches and merged texturelists are keyed by
IFS mod path).

| skin | `dafu_eff_mar` | light | ring | rsring01 | side light |
|---|---|---|---|---|---|
| 1 | 561×69 (own) | 124² (own) | 110² (own) | 110² | 772×78 (own) |
| 2–5 | 654×98 (per skin) | 124² (shared 2–5) | 110² (shared 2–5) | 110² (shared 1–5) | 772×78 (shared 2–5 = World's) |

The fullcombo AFPs are byte-identical between skins 2 / 3 and 4 / 5, so the
patch fn **cannot identify the skin from the bytes**: it must use the armed
skin (`ddr_selection::armed_skin()` while `legacy_package("dance_fullcombo")`)
and that skin's staged entry — a patch whose new shapes have no geo in the
streaming IFS would draw nothing.

### 2.3 `dance_combo000N_v0`

Per `legacy-combo.md` §3: skins 1–3 one sheet (`dance_combo000N_{0..9,combo}`),
skins 4–5 one per worst grade (`dance_combo000N_{marvelous,perfect,great,good}_*`;
5 also an unused `gray`). Digits 74×77, word 149×47. Skin 5 needs the A3
import. A3's texture write (`combo.rs::texture_write`) runs on every combo
message ≥ 4 and at init — the natural place for an all-S-Marvelous sheet.

## 3. Recipe generalisation (run on the real templates)

- **Word clone:** `find_word_shape_by_geo` + `run_word_clone` succeed on all
  five skins (word shape 32 → new shape 54 / sprite 55, `in_smarvelous` at
  f600, section 638 frames, outputs 10.3–11.7 KB). The region stem rule gives
  `dance_judge000N_smarvelous`. Skins 1/2/4/5: both mutes apply (3 + 3
  records); skin 3: nothing to mute (first ladder rung still succeeds with 0
  records — only the harness's "stock pulse must exist" assertion is
  World-specific).
- **Splash clone:** `clone_segment_with_new_shapes` succeeds on all 20
  legacy templates with **5** shapes (single 129/132/135/138/147 → 175..179;
  double 172/175/178/181/190 → 232..236; 3 sprite clones, label tables
  sorted in every section). The DLL's staging gate `shape_ids.len() != 4`
  is World-specific ⇒ generalise to "the template's expected count" (4 on
  World, 5 on legacy) so a surprise shape still refuses.
- **Combo:** no AP2 edit — a FRESH texture set bound by name
  (`afp_mc_load_bitmap`), exactly like World's `daco_combo_smarvelous_%d`.

## 4. Mechanism as built (Step 13)

No new signatures, detours or code patches; World's code paths and patch
outputs are unchanged (`validate_s_marvelous.sh` Legs A–G still green).

- **Targets** (`s_marvelous/targets.rs`, pure, 5 tests): legacy package
  bases / arc candidates / IFS names / mod paths, per-skin art paths
  (`data_mods/ddr_selection/s_marvelous/N/`), the splash rename rule (moved
  here from `assets.rs`, shared with the emblem), `word_region`, expected
  splash shape count (World 4, legacy 5), `legacy_combo_has_grade_sheets`
  {4, 5}, combo texture names, `target_skin(legacy, armed_skin)`,
  `mute_stock_glow(skin)` (World only).
- **Staging** (`assets.rs`): `Target` (skin, arc path, IFS name, IFS mod
  path); `legacy_target(kind, skin)` resolves the arc World's probe opens
  (LayeredFS mod file first, then `data/`). `stage_word` / `stage_fullcombo_for`
  are World's staging taking a target (World calls them with World's
  target — same logs); `run_word_clone(doc, shape, mute_stock)` skips the
  both-mutes rung for legacy targets; `stage_legacy_combo` stages the eleven
  `dance_combo000N_smarvelous_*` textures as a FRESH set (quiet INFO when the
  IFS is not readable — skin 5 without the A3 import). Output under
  `data_mods/s_marvelous/<ifs>_ifs/`.
- **Coordinator** (`s_marvelous/legacy.rs`): `stage_if_ready(color)` — both
  mods enabled, once per skin per boot, skins whose art folder exists — calls
  `afp_patches::add_legacy`, `splash::add_legacy`, `combo::add_legacy` and
  logs one summary line with the time taken. Called from DDR SELECTION's
  `enable` (`s_marvelous::on_ddr_selection_enabled`; S-Marvelous enables
  first at boot) and from S-Marvelous' `enable` (live order).
- **Patch fns**: `afp_patches` keeps a `Vec<StagedPatch>`, `splash` a
  `Vec<StagedFcPatch>` (closures registered for all four templates at the
  first activate); both pick the entry by `target_skin(legacy_package(..),
  armed_skin())` and byte-gate on it (legacy variant ⇒ one WARN per skin).
  Applied bits per target skin.
- **Re-drives**: `flash` drives only when `patch_applied_for(skin)` of this
  song's target (World: bit 0 = the old latch); `splash` World keeps
  `WORLD_READY`, legacy needs `LEGACY_APPLIED` for the armed skin. First
  legacy word re-drive logs once per skin.
- **Combo**: `combo_math::sheet_prefix(skin, worst, all_smarvelous)` —
  per-grade skin ∧ worst 0 ∧ flag ⇒ `dance_combo000N_smarvelous`
  (`SMARV_SHEET`). `combo.rs::texture_write` asks
  `s_marvelous::legacy_combo_smarv(skin, side)` (mod enabled ∧ sheet staged ∧
  `combo_is_all_smarv(side)`, atomics only) on every write; one INFO per song
  on the first S-Marvelous sheet.
- **Judgement Color**: `set_judgement_color` restages every staged target's
  word (names copied out of the lock before the file IO).
- **Validation**: `validate_s_marvelous.sh` mounts `targets.rs`; Leg H runs
  the word recipe (`smarv-legacy-word`: S-Marv copy silent, A3's pulse
  intact, sorted labels) + a bemaniutils render with the shipped art, the
  splash recipe (`smarv-fc … <skin>`, five shapes) on all 20 templates, and a
  size check of all 57 art files against their donors' imgrects.
  `validate_ddr_selection.sh`: `combo_math` `smarvelous_sheet` test (142).

## 5. Art spec

Per skin N under `data_mods/ddr_selection/s_marvelous/N/`, each file at its
donor's imgrect size (the `ifstools` extraction size — the donor-anchored
clone composites at the donor imgrect origin):

- `dance_judge/smarvelous_{all_purple,purple_shadow}.png` — 346×63
  (donor `dance_judge000N_marvelous`);
- `dance_fullcombo/dafu_{eff_smar,light_smarvelous,ring_smarvelous,rsring01_smarvelous,side_light_smarvelous}.png`
  (sizes in §2.2);
- skins 4–5: `dance_combo/smarvelous_{0..9,combo}.png` — 74×77 / 149×47
  (donor `dance_combo000N_marvelous_*`).

Generated programmatically from the installed legacy art by
`scripts/gen_ddr_selection_smarv_art.py`, with World's S-Marvelous recipes,
and committed like `data_mods/s_marvelous/` (maintainer decision below — the
design's "drafts, finished by hand" phase is dropped). Art review 1
(2026-09-25): World's PURPLE SHADOW recipe (saturated pixels violet) was
barely visible / artifacted on the legacy words; replaced for them by
`violet_outline` — stock letters kept, the dark outline / shadow repainted
in the skin's ALL PURPLE letter colour and grown 1 px (per-skin luminance
split). Art review 2: skins 1–4 approved; skin 5 keeps its black outline
and its white glow outside the outline turns violet instead (`violet_glow`;
the grey drop shadow stays).

## 6. Maintainer choices (2026-09-25)

1. **Legacy MARVELOUS pulse:** keep A3's additive pulse on the stock legacy
   MARVELOUS word; mute it on the violet S-Marvelous copy
   (`WordCloneOpts { mute_additive_glow: true, mute_source_additive_glow:
   false }` for legacy targets; World's stays both-muted).
2. **Judgement Color:** two variants per skin (ALL PURPLE / PURPLE SHADOW);
   the row applies on legacy songs too.
3. **Combo on skins 1–3:** nothing — A3's single sheet (design default).
   Skins 4–5 get the violet all-S-Marvelous sheet.
4. **S-MFC splash:** all five Marvelous regions recoloured, per skin.
5. **Assets:** no draft phase — the art is generated programmatically (as
   World's S-Marvelous art was), iterated with the maintainer until it looks
   right, and committed as the shipped art. Runtime and assets are both in
   Step 13's scope.
