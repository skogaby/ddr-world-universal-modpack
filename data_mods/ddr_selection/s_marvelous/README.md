# DDR SELECTION — S-Marvelous art for the legacy skins

S-Marvelous Judgement's presentation on DDR SELECTION's five legacy skins
(1 = 1st-5th, 2 = MAX-EXTREME, 3 = SuperNOVA, 4 = X, 5 = 2013-A). One folder
per skin, laid out like `data_mods/s_marvelous/`:

| file | donor (package · texture) | size |
|---|---|---|
| `N/dance_judge/smarvelous_all_purple.png` | `dance_judge000N_v0` · `dance_judge000N_marvelous` | 346×63 |
| `N/dance_judge/smarvelous_purple_shadow.png` | same | 346×63 |
| `N/dance_fullcombo/dafu_eff_smar.png` | `dance_fullcombo000N_v0` · `dafu_eff_mar` ("MARVELOUS FULLCOMBO!!!") | 561×69 (skin 1), 654×98 (2–5) |
| `N/dance_fullcombo/dafu_light_smarvelous.png` | · `dafu_light_marvelous` | 124×124 |
| `N/dance_fullcombo/dafu_ring_smarvelous.png` | · `dafu_ring_marvelous` | 110×110 |
| `N/dance_fullcombo/dafu_rsring01_smarvelous.png` | · `dafu_rsring01_marvelous` | 110×110 |
| `N/dance_fullcombo/dafu_side_light_smarvelous.png` | · `dafu_side_light_marvelous` | 772×78 |
| `4/`, `5/` only: `dance_combo/smarvelous_{0..9,combo}.png` | `dance_combo000N_v0` · `dance_combo000N_marvelous_{0..9,combo}` | 74×77 / 149×47 |

Every file must keep its donor's size (the imgrect, which is the size
`ifstools` extracts): the DLL clones the donor's atlas slot and serves the
image at the donor's position. `scripts/validate_s_marvelous.sh` (Leg H)
checks every size against the installed donors.

Skins 1–3 have no combo art: A3 drew one combo sheet on those skins whatever
the grade, so an all-S-Marvelous combo looks like any other there.

## How the DLL uses them

With both S-Marvelous Judgement and DDR SELECTION enabled, S-Marvelous stages
each skin that has a folder here (once per boot, into
`data_mods/s_marvelous/<package>_ifs/` — generated, never committed):

- the word is cloned into an `in_smarvelous` segment of the skin's
  `dance_judge` template; the "Judgement Color" row picks
  `smarvelous_all_purple` or `smarvelous_purple_shadow`. A3's own MARVELOUS
  keeps its additive shimmer; the violet copy is static;
- the splash regions become the skin's `s_marbelous_in` full-combo segment;
- the combo sheet is loaded by DDR SELECTION's A3 combo while the combo is
  all S-Marvelous (skins 4–5).

A skin without a folder keeps A3's presentation.

## Generator

The art is programmatic, like `data_mods/s_marvelous/`:
`python3 scripts/gen_ddr_selection_smarv_art.py` (World install from
`DDR_WORLD_INSTALL`; skin 5's combo from the A3 import or `DDR_A3_INSTALL`)
rewrites every file here from the installed legacy art. Add `--skins 3` for one
skin, or `--sheet <png>` for a donor/result contact sheet.
`--check-world` proves the recipes reproduce `data_mods/s_marvelous/`.

Recipes (hue 280°):

- **ALL PURPLE** — every pixel: saturation floor 150/255, value ×0.82, alpha
  unchanged. Used for the ALL PURPLE word, the splash and the combo sheet.
- **PURPLE SHADOW** (the legacy words' own recipe, maintainer feedback
  2026-09-25) — the stock letters are kept; the dark outline / shadow around
  them is repainted in the skin's ALL PURPLE letter colour and grown by one
  pixel outward. Per-skin luminance thresholds split letter from outline
  (`OUTLINE` in the generator): 1st-5th's navy outline, MAX-EXTREME's brown
  outline and black drop shadow, SuperNOVA's grey shadow, X's black outline
  with its blur. 2013-A instead keeps its letters AND its thin black outline
  and turns the white glow outside the outline violet (same alpha fall-off;
  the grey drop shadow stays grey). (World's own PURPLE SHADOW word uses
  World's older recipe: neutral pixels kept, saturated pixels violet.)

Hand edits are welcome: re-running the generator overwrites them, so run
it for the other skins only (`--skins`).
