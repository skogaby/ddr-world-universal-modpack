# R4 — Option-row texture generation (CONFIRMED)

## Generator
`scripts/gen_option_labels.py` (Pillow). Renders two texture families into the lang_eng
option IFS tex dir served via LayeredFS:
`data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`

- **Left-side row labels:** `seop_item_<option_id>.png` — 176×16 RGBA, **black**, left-
  justified, font Inclusive Sans SemiBold. Driven by the `LABELS` list of
  `(option_id, display_text)` tuples.
- **Value-ribbon chips:** `seop_op_<key>.png` — 132×24 RGBA, teal `#00ffbd`, centered.
  Stock ON/OFF ribbons (`seop_op_on`, `seop_op_off`) already exist in the game atlas and are
  **not** generated. Boolean toggles reuse them.

Over-long text is condensed horizontally only (height preserved), mirroring the game.

## What this mod needs

1. Pick the option id (snake_case, used everywhere): **`center_arrows_1p`**.
2. Add one entry to the `LABELS` list (near the "Mod toggles" group with `autoplay`,
   `premium_free`):
   ```python
   ("center_arrows_1p", "CENTER ARROWS (1P ONLY)"),
   ```
3. Run `python3 scripts/gen_option_labels.py` → writes
   `seop_item_center_arrows_1p.png` into the LayeredFS tex dir.
4. **No new value ribbon needed** — a boolean toggle uses the existing
   `seop_op_on` / `seop_op_off` chips.
5. "CENTER ARROWS (1P ONLY)" is ~23 chars; it will likely trip the horizontal-condense path
   (USABLE_WIDTH at 176×16). Expected and consistent with stock long labels
   (`EXPORT STEP DATA (CSV)`, `PACEMAKER -> MS ERROR`). Verify legibility on cabinet; shorten
   only if it reads poorly.

## How the id ties to the mod

`RegisterSpec::bool_toggle("center_arrows_1p")` makes the row look up
`seop_item_center_arrows_1p` for its label and `seop_op_on/off` for its value — exactly how
`premium_free` works. The texture name is derived from the option id by convention
(`seop_item_<id>`), so the id in code and the `LABELS` entry must match.

## Reference
- `scripts/gen_option_labels.py` (LABELS list + `main()` output naming).
- `src/services/custom_options/api.rs` (`seop_op_*` value-ribbon convention, `bool_toggle`).
- `src/mods/premium_free.rs` (reference: a registered bool toggle whose row label is
  `seop_item_premium_free`).
