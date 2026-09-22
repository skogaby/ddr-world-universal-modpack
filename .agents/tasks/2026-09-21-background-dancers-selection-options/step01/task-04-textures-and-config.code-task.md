# Task: Option textures, templates, and shipped config placement

## Description
Author the row label strings (en/ja/ko) and the SPLIT-layout preview-chrome templates for
`background_dancer` / `background_stage` in `scripts/option_strings.py`, regenerate every
language's texture set with `scripts/gen_option_labels.py`, keep `scripts/check_option_takeover.py`
tolerant of templates that have no hand-authored reference, and insert the two ids into the
shipped `mod-config.json` `option_menu_settings` after `arrow_opacity` with `overlay: false`.

## Background
Row labels are `seop_item_<id>.png` textures generated from `LABELS`; the options preview box
(368×172) shows `seop_image_<id>.png`, which for template-driven rows is generated at runtime by
`preview_gen::generate_chrome` from `seop_image_<id>_TEMPLATE.png` (marker boxes cleared to
transparent). The DLL reads the green marker rect from the template at runtime
(`preview_gen::marker_rect_for`) — that rect is where the live 3D preview (Step 5) will render,
so its geometry is load-bearing. Maintainer decision 2026-09-21 (design §4.7 amendment): keep the
shipped SPLIT layout — description text in the left column, the marker filling the RIGHT column;
the box need not be 16:9 (the stage preview will be a cropped view). Right-column geometry from
the shipped customize templates: ink right of the divider starts at x ≥ 186; `customize_appeal_board`
uses `(191, 33, 170, 22)`+`(191, 67, 170, 70)`, `customize_character_p1` `(209, 11, 134, 150)`.
Use `(191, 11, 170, 150)` — the full right column with the same 11-px top margin, 170 wide.
Templates are byte-identical across languages in their marker region (the DLL parses eng first).

`option_menu_settings` order is display order for both menus; placement under PLAYFIELD STYLING
OPTIONS is purely position in that list (after `arrow_opacity`, before `header_training_options`).
The updater's header-scoped merge delivers new ids to existing installs.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md` (§4.7 incl. the 2026-09-21 amendment, FR-7)

**Additional References (if relevant to this task):**
- `scripts/option_strings.py` (`LABELS`, `TemplateSpec`, `TEMPLATES`)
- `scripts/gen_option_labels.py::render_template` (SPLIT divider, `PREVIEW_SPLIT_RIGHT` = 173 text column limit)
- `scripts/check_option_takeover.py` (iterates `TEMPLATES` against a `--reference` dir)
- `mod-config.json` lines ~130–170 (the PLAYFIELD STYLING block)
- AGENTS.md "Options-menu texture localization" row

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `LABELS["background_dancer"]` = en `BACKGROUND DANCER`, ja `背景ダンサー`, ko `배경 댄서`;
   `LABELS["background_stage"]` = en `BACKGROUND STAGE`, ja `背景ステージ`, ko `배경 스테이지`.
2. `TEMPLATES["background_dancer"]` / `["background_stage"]`: `TemplateSpec(id, [(191, 11, 170,
   150, (0, 255, 0, 255))], lines)` with pre-broken lines per language that fit the 173-px text
   column (the generator hard-fails on overflow/kinsoku): en dancer
   `["Chooses the 3D dancer", "shown behind your lane.", "", "RANDOM picks a", "different one each song."]`-style
   copy (blank line = paragraph gap; keep ≤ 8 lines so the last baseline stays ≤ 168), stage
   likewise for the stage; ja/ko translations of the same two sentences.
3. Regenerate: `python3 scripts/gen_option_labels.py` (all three languages) — commits the new
   `seop_item_background_{dancer,stage}.png` and `seop_image_background_{dancer,stage}_TEMPLATE.png`
   under `data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/`; no other
   generated file may change (diff the tex dirs — a changed unrelated PNG means a generator drift).
   Never hand-edit a generated PNG.
4. `scripts/check_option_takeover.py`: skip a template whose reference file does not exist
   (`if not (reference / name).exists(): continue` with a one-line note) — new templates have no
   hand-authored original.
5. `mod-config.json` `option_menu_settings`: insert `{ "id": "background_dancer", "overlay": false,
   "in_game": true }` then `{ "id": "background_stage", "overlay": false, "in_game": true }` right
   after the `arrow_opacity` entry. JSON stays valid (run it through `python3 -m json.tool`).
6. `README.md`: nothing in this task (Step 6 owns the docs).

## Dependencies
- None on code; task-03 registers the ids these textures serve.

## Implementation Approach
1. Add the strings and templates; run the generator; inspect the two eng templates (`open` /
   image viewer) — text in the left column, a solid green 170×150 box on the right, no overlap.
2. Guard the takeover script; verify it still parses (`python3 -c "import ast; …"` or run it
   against an empty reference dir to confirm the skip path).
3. Edit `mod-config.json`; validate JSON.
4. `git status` — only the intended files changed.

## Acceptance Criteria

1. **Labels generated**
   - Given the new `LABELS` entries
   - When `gen_option_labels.py` runs
   - Then `seop_item_background_dancer.png` and `seop_item_background_stage.png` exist in all three language tex dirs

2. **Templates generated with the marker**
   - Given the new `TEMPLATES` entries
   - When `gen_option_labels.py` runs
   - Then each language's `seop_image_background_{dancer,stage}_TEMPLATE.png` is 368×172 with a solid
     `(0,255,0,255)` rectangle exactly at `(191, 11, 170, 150)`, byte-identical across languages in
     that region, and description text left of x = 173

3. **No collateral regeneration**
   - Given the regenerated tex dirs
   - When `git status` is inspected
   - Then only the four new files per language (2 labels + 2 templates) are added; no existing PNG changed

4. **Config placement**
   - Given the edited `mod-config.json`
   - When parsed
   - Then `background_dancer` immediately follows `arrow_opacity`, `background_stage` follows it,
     both `overlay: false, in_game: true`, and `header_training_options` follows them

5. **Takeover check tolerant**
   - Given a reference dir lacking the new templates
   - When `check_option_takeover.py --reference <dir>` runs
   - Then the new templates are skipped, not reported as failures

## Metadata
- **Complexity**: Low
- **Labels**: textures, localization, config
- **Required Skills**: Python (Pillow generator), JSON
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 1: Option rows end-to-end
