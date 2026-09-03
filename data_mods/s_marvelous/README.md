# S-Marvelous — mod assets

**STATUS: PLACEHOLDER ART** — programmatic deep-violet colorizes (hue fixed
~280°, saturation floor 150 so white cores take the tint, value ×0.82) of the
stock Marvelous donors, generated 2026-08-29. Replace each file with the real
art, keeping the EXACT canvas size (the donor-anchored atlas clone requires
identical pixel rects so cloned geo UVs stay valid).

| file | donor (package · texture) | size | used by (plan step) |
|---|---|---|---|
| `dance_judge/smarvelous_all_purple.png` | dance_judge_v3 · `daju_marvelous` | 260×90 | gameplay flash word, "Judgement Color" = ALL PURPLE (full violet colorize) |
| `dance_judge/smarvelous_purple_shadow.png` | dance_judge_v3 · `daju_marvelous` | 260×90 | gameplay flash word, "Judgement Color" = PURPLE SHADOW (default; see below) |
| `dance_combo/smarvelous_0..9.png` | dance_combo0000 · `dance_combo0000_marvelous_0..9` | 72×75 | combo digits (Step 5) |
| `dance_combo/smarvelous_combo.png` | dance_combo0000 · `dance_combo0000_marvelous_combo` | 147×45 | combo caption (Step 5) |
| `dance_fullcombo/dafu_eff_smar.png` | dance_fullcombo0000 · `dafu_eff_mar` | 559×67 | FC splash (Step 6) |
| `dance_fullcombo/dafu_light_smarvelous.png` | dance_fullcombo0000 · `dafu_light_marvelous` | 122×122 | FC splash (Step 6) |
| `dance_fullcombo/dafu_ring_smarvelous.png` | dance_fullcombo0000 · `dafu_ring_marvelous` | 108×108 | FC splash (Step 6) |
| `dance_fullcombo/dafu_rsring01_smarvelous.png` | dance_fullcombo0000 · `dafu_rsring01_marvelous` | 108×108 | FC splash (Step 6) |
| `dance_fullcombo/dafu_side_light_smarvelous.png` | dance_fullcombo0000 · `dafu_side_light_marvelous` | 770×76 | FC splash (Step 6) |
| `scene_result/scre_tab_detail_judge.png` | scene_result_v3 · `scre_tab_detail_judge` | 108×118 | results row-label sheet, Details tab (Step 7) |
| `scene_result/scre_tab_detail_base.png` | scene_result_v3 · `scre_tab_detail_base` | 108×118 | results row-label sheet, Simple results tab (Step 7) |
| `scene_result/scre_fc_smarvelous.png` | scene_result_v3 · `scre_fc_marvelous` | 232×18 | per-stage S-MFC emblem caption (Step 9) |
| `scene_result/scre_total_player_fc_smfc.png` | scene_result_v3 · `scre_total_player_fc_mfc` | 30×12 | total-results S-MFC badge (Step 9) |

**Gameplay flash word (`dance_judge/smarvelous_*.png`) and the stock additive
glow (2026-09-03).** The two variants are selected by the overlay menu's
"Judgement Color" row (GLOBAL SETTINGS › S-MARVELOUS JUDGEMENT, persisted
as `s_marvelous.judgement_color` = `all_purple` | `purple_shadow`); the DLL
copies the chosen PNG to `dance_judge_v3_ifs/tex/daju_smarvelous.png` at
enable and again on every row edit (purging LayeredFS's converted copy), so
the change lands when the game next loads the dance_judge package
(normally the next song). Both files MUST exist and share the 260×90 rect. The stock `dance_marvelous` sprite (sprite 46 in
`dance_judge_v3`) stamps a SECOND copy of the word over itself in ADDITIVE
blend (blend 8, instance `marvelous_ef`, mult alpha 0.20 → 0.098 → 0
looping every ~4–5 frames). On the stock white letters the addition clamps
at 255 and is invisible; on ANY coloured pixel it reads as a ~12–15 Hz
brightness/hue flicker. Two-part fix: (1) the DLL's word clone now MUTES
that layer in the cloned chain (`WordCloneOpts::mute_additive_glow` —
every record of the additive object gets mult alpha 0; the stock segment
is untouched; `assets::run_word_clone` falls back to an unmuted clone with
one WARN on a template where the mute can't apply) — the mute is
UNCONDITIONAL, independent of the colour choice, so ALL PURPLE renders
static too; (2) the PURPLE SHADOW PNG keeps the
donor's neutral pixels (saturation < 0.12 — white fill, black outline, AA
greys) untouched and recolours only the saturated shadow/highlight pixels
(hue 280°, saturation floor 0.55, value ×0.90) — maintainer-picked look.
Generator: inline PIL/numpy HSV
script (donor decoded from the live `dance_judge_v3.ifs` via bemaniutils'
IFS class), recorded in
`.agents/planning/2026-08-29-s-marvelous-judgement/progress.md`
(2026-09-03 entries).

The two `scene_result` sheets are FULL REPLACEMENTS of the stock 6-row
judgement-label sheets: the stock block is uniform-scaled ×16/19 (pitch
19px → 16px), right-aligned to the original margin, and a violet colorize of
the Marvelous row is stacked on top as S-MARVELOUS — 7 rows at 16px pitch,
centers at sheet-y 11/27/43/59/75/91/107. Hand edits are welcome (they're
source art, not generated output) but MUST keep the 108×118 canvas and the
16px row grid — the `body_tab_detail_result` AP2 patch moves the six stock
`*_num_usr` number instances to exactly that grid (maintainer note
2026-08-30: the base sheet's violet row carries a sliver of Perfect's shadow
from the crop — hand cleanup planned).

The two Step-9 emblem textures are straight violet colorizes of their donors
(same canvas — `scre_fc_smarvelous` is donor-anchored in the atlas, so the
232×18 rect is load-bearing; the `..._fc_smfc` badge is a FRESH texturelist
entry sized from the PNG). The per-stage emblem keeps the stock "Marvelous
Fullcombo!!!" wording per the maintainer's art language (violet hue, no "S-"
prefix); it renders STATIC violet — the AP2 patch drops the stock rainbow
hue-rotation records, so no hue variation is needed in the art.

Regeneration (dev machine, `DDR_WORLD_INSTALL` + sibling bemaniutils): the
generators are one-shot inline scripts recorded in
`.agents/planning/2026-08-29-s-marvelous-judgement/progress.md` (Step 4
prep for the gameplay art; Step 7 prep for the results sheets; Step 9 prep
for the emblem colorizes); they decode
the donors from the game arcs via bemaniutils' IFS class and apply an
alpha-preserving HSV hue rotation (hue 280°, saturation floor 150,
value ×0.82).
