# S-Marvelous — mod assets

**STATUS: PLACEHOLDER ART** — programmatic deep-violet colorizes (hue fixed
~280°, saturation floor 150 so white cores take the tint, value ×0.82) of the
stock Marvelous donors, generated 2026-08-29. Replace each file with the real
art, keeping the EXACT canvas size (the donor-anchored atlas clone requires
identical pixel rects so cloned geo UVs stay valid).

| file | donor (package · texture) | size | used by (plan step) |
|---|---|---|---|
| `dance_judge/smarvelous.png` | dance_judge0000 · `dance_judge0000_marvelous` | 344×61 | gameplay flash word (Step 4) |
| `dance_combo/smarvelous_0..9.png` | dance_combo0000 · `dance_combo0000_marvelous_0..9` | 72×75 | combo digits (Step 5) |
| `dance_combo/smarvelous_combo.png` | dance_combo0000 · `dance_combo0000_marvelous_combo` | 147×45 | combo caption (Step 5) |
| `dance_fullcombo/dafu_eff_smar.png` | dance_fullcombo0000 · `dafu_eff_mar` | 559×67 | FC splash (Step 6) |
| `dance_fullcombo/dafu_light_smarvelous.png` | dance_fullcombo0000 · `dafu_light_marvelous` | 122×122 | FC splash (Step 6) |
| `dance_fullcombo/dafu_ring_smarvelous.png` | dance_fullcombo0000 · `dafu_ring_marvelous` | 108×108 | FC splash (Step 6) |
| `dance_fullcombo/dafu_rsring01_smarvelous.png` | dance_fullcombo0000 · `dafu_rsring01_marvelous` | 108×108 | FC splash (Step 6) |
| `dance_fullcombo/dafu_side_light_smarvelous.png` | dance_fullcombo0000 · `dafu_side_light_marvelous` | 770×76 | FC splash (Step 6) |

Still needed later (deferred until the exact atlas regions are mapped during
those steps — the art lives inside larger sheets in the results packages):

- Results score-tab row label ("S-MARVELOUS" word, `scre_tab_detail_*`
  family) — Step 7.
- Per-stage results FC emblem `loop_smfc` frames' art — Step 9.
- Total-results emblem (`scre_total_player_*` family) — Step 9.

Regeneration (dev machine, `DDR_WORLD_INSTALL` + sibling bemaniutils): the
generator is a one-shot inline script recorded in
`.agents/planning/2026-08-29-s-marvelous-judgement/progress.md` (Step 4
prep); it decodes the donors from the game arcs via bemaniutils' IFS class
and applies an alpha-preserving HSV hue rotation.
