# DDR SELECTION — S-Marvelous art for the legacy skins

S-Marvelous Judgement's presentation on DDR SELECTION's legacy skins. One
folder per **art set**, laid out like `data_mods/s_marvelous/`:

| Set | DDR SELECTION skin | Donor packages (`T` = texture number) |
|---|---|---|
| 1–5 | the eras: 1 = 1st-5th, 2 = MAX-EXTREME, 3 = SuperNOVA, 4 = X, 5 = 2013-A | `…000N_v0`, `T` = `000N` |
| 6 | 6 = DDR A | A3's skin-0 `…0000_v0`, `T` = `0000` |
| 7 | 7 = DDR A3 (White) and 8 = DDR A3 (Gold) | A3's skin-0 `…0000_v2` (White). Gold's `…0000_v1` Marvelous art is pixel-identical, so the DLL stages set 7 into both packages. |

| file | donor (package · texture) | size |
|---|---|---|
| `N/dance_judge/smarvelous_all_purple.png` | `dance_judge…` · `dance_judgeT_marvelous` | 346×63 (sets 1–6), 346×62 (7) |
| `N/dance_judge/smarvelous_purple_shadow.png` | same | same |
| `N/dance_fullcombo/dafu_eff_smar.png` | `dance_fullcombo…` · `dafu_eff_mar` ("MARVELOUS FULLCOMBO!!!") | 561×69 (sets 1, 6), 654×98 (2–5), 562×70 (7) |
| `N/dance_fullcombo/dafu_light_smarvelous.png` | · `dafu_light_marvelous` | 124×124 |
| `N/dance_fullcombo/dafu_ring_smarvelous.png` | · `dafu_ring_marvelous` | 110×110 |
| `N/dance_fullcombo/dafu_rsring01_smarvelous.png` | · `dafu_rsring01_marvelous` | 110×110 |
| `N/dance_fullcombo/dafu_side_light_smarvelous.png` | · `dafu_side_light_marvelous` | 772×78 |
| sets 4–7 only: `dance_combo/smarvelous_all_purple_{0..9,combo}.png` | `dance_combo…` · `dance_comboT_marvelous_{0..9,combo}` | 74×77 / 149×47 (4–6), 74×92 / 178×68 (7) |
| sets 4–7 only: `dance_combo/smarvelous_purple_shadow_{0..9,combo}.png` | same | same |

Every file must keep its donor's size (the imgrect, which is the size
`ifstools` extracts): the DLL clones the donor's atlas slot and serves the
image at the donor's position. `scripts/validate_s_marvelous.sh` (Leg H)
checks every size against the installed donors.

Skins 1–3 have no combo art: A3 drew one combo sheet on those skins whatever
the grade, so an all-S-Marvelous combo looks like any other there. The
themes are A3's own skin 0, which has a sheet per grade.

## How the DLL uses them

With both S-Marvelous Judgement and DDR SELECTION enabled, S-Marvelous stages
each skin whose art set has a folder here (once per boot, into
`data_mods/s_marvelous/<package>_ifs/` — generated, never committed):

- the word is cloned into an `in_smarvelous` segment of the skin's
  `dance_judge` template; the "Judgement Color" row picks
  `smarvelous_all_purple` or `smarvelous_purple_shadow`. A3's own MARVELOUS
  keeps its additive shimmer; the violet copy is static;
- the splash regions become the skin's `s_marbelous_in` full-combo segment;
- the combo sheet is loaded by DDR SELECTION's A3 combo while the combo is
  all S-Marvelous (skins 4–8). The "Judgement Color" row picks its variant
  too: ALL PURPLE violet digits, or PURPLE SHADOW — the skin's own digits
  with the word's violet outline / glow. These skins colour the combo by its
  worst judgement, so the outline keeps the S-Marvelous sheet from reading as
  one more grade colour. World's own combo has one violet variant.

A skin without its art set's folder keeps A3's presentation.

## Generator

The art is programmatic, like `data_mods/s_marvelous/`:
`python3 scripts/gen_ddr_selection_smarv_art.py` (World install from
`DDR_WORLD_INSTALL`; set 5's combo from the A3 import or `DDR_A3_INSTALL`)
rewrites every file here from the installed art. Add `--skins 3` for one art
set (`--skins 6,7` for the themes), or `--sheet <png>` for a donor/result
contact sheet. Set 7 first checks that every `_v1` donor is pixel-identical to
its `_v2` twin, and writes nothing if one is not.
`--check-world` proves the recipes reproduce `data_mods/s_marvelous/`.
`--review target/smarv_legacy_review` writes donor-vs-art review pages (one per
art set, all stacked, and a words-only grid) from the art here, writing no art.

Recipes (hue 280°):

- **ALL PURPLE** — every pixel: saturation floor 150/255, value ×0.82, alpha
  unchanged. Used for the ALL PURPLE word and combo sheet, and the splash.
- **PURPLE SHADOW** (the legacy words' own recipe, maintainer feedback
  2026-09-25) — the stock letters are kept; the dark outline / shadow around
  them is repainted in the skin's ALL PURPLE letter colour and grown by one
  pixel outward. Per-skin luminance thresholds split letter from outline
  (`OUTLINE` in the generator): 1st-5th's navy outline, MAX-EXTREME's brown
  outline and black drop shadow, SuperNOVA's grey shadow, X's black outline
  with its blur, DDR A3's dark olive outline (its violet darkened to 0.70 of
  the letter colour, maintainer 2026-09-26). 2013-A and DDR A instead keep
  their letters AND their dark outline and turn the light glow outside the
  outline violet (same alpha fall-off): 2013-A's white glow (its grey drop
  shadow stays grey), DDR A's white rim and yellow glow. (World's own PURPLE
  SHADOW word uses World's older recipe: neutral pixels kept, saturated
  pixels violet.)
- **PURPLE SHADOW combo sheet** (`COMBO_SHADOW` in the generator,
  maintainer 2026-09-26) — the word's recipes on the eleven images, with one
  violet for the whole sheet: X, 2013-A and DDR A keep their digits and bold
  dark outline and turn the light glow around it violet; DDR A3 gets the
  darkened violet outline, as its word does.

Hand edits are welcome: re-running the generator overwrites them, so run
it for the other art sets only (`--skins`).
