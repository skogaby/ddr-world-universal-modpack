# Research: label prototype (D12 look)

Throwaway prototype: `prototypes/series_label_sheet.py` (not implementation code). Outputs in
`prototypes/out/`: `contact_sheet.png` (all five `num_columns` layouts, 20 canonical rows,
stock GROUP tabs, 216×266 panel outline, red line = scroll boundary), `stock_vs_prototype_2col.png`,
`labels/sefi_version_<key>_<N>col.png`.

## Findings

- **Ink limits per canvas** (widest stock labels of each size): 104 → 98 px (`version_maxex`),
  64 → 60 (`cl_noplay`), 44 → 42 (`title_mno`), 32 → 30 (`rank_aaa`); 220 has no wide stock
  example (`event_league` ends at 115) → 216 assumed. Stock ink starts at x = 0–3. The label canvas
  sits right of the 12-px check mark, so the last few canvas pixels overrun the cell; the stock art
  simply never inks them. The generator must fit to these limits, not the canvas width.
- **Inclusive Sans SemiBold at 16 px** gives the stock 12-px cap height (baseline y = 16) but is
  wider than the stock condensed grotesque (`WORLD`: 58 px vs 52 px stock).
- **Its zero is dotted** and the font has no alternate zero glyph (GSUB features: aalt, case,
  ccmp, locl, ordn, rvrn, ss01). Visible in 2013, 2014, A20, A20 PLUS.
- **Fitting rule that reads well:** condense horizontally down to 0.70; below that, stack on two
  lines (11.5 px, baselines 9/19, second line right-aligned, as stock `SuperNOVA–/SuperNOVA2`)
  **only at a natural break** — a space or a lower→upper / digit→letter boundary. Single words
  are never split mid-word; they condense instead (`WORLD` 0.50 and `EXTREME` 0.41 at 5 columns
  are the worst cases).
- **Layout fit with 20 rows:** 1 column (20 grid rows) and 2 columns (10) overflow the nine
  visible rows and scroll; 3 columns (7), 4 (5) and 5 (4) fit.
- 1–3 columns: every canonical label is legible at full size or lightly condensed, except
  3-col SuperNOVA/SuperNOVA2/X3 VS 2ndMIX (stacked). 4–5 columns: most labels stack or condense
  hard — readable, but small; candidates for per-width text overrides (abbreviations) if wanted.

## Font override (2026-09-27)

The maintainer supplied FOT-TsukuGo Pro B (copied to `scripts/fonts/FOT-TSUKUGOPRO-B.OTF`) as a
closer match to the stock face. Calibration against stock `sefi_version_*`:
- 15 px gives the stock 12-px cap height (16 px gives 13).
- At natural width TsukuGo B is ~12 % wider (`WORLD` 59 px vs 52 stock) and lighter; a base
  horizontal scale of 0.90 plus a 0.375-px faux-bold (stroke rendered at 8× and downsampled) lands
  within a pixel or two of stock width and visibly matches the weight
  (`prototypes/out/stock_vs_prototype_2col.png`).
- Zeros are plain (no slash/dot).
- Two-line stack size 10.5 px (≈ 8-px caps, stock `SuperNOVA–/SuperNOVA2` scale).
- With the narrower face, 4 columns now stacks only A20 PLUS / X3 VS 2ndMIX / SuperNOVA /
  SuperNOVA2; 5 columns still stacks most `…MIX` labels and condenses EXTREME to 0.43.
- Faux-bold dropped at the user's request (2026-09-27): the final prototype renders TsukuGo B at
  its own weight (`EMBOLDEN = 0.0` in the prototype).
