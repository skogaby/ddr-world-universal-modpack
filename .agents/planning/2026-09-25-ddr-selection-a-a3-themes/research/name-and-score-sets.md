# Player name and stage-panel score sets (2026-09-25)

Scope: the maintainer overrides of D12 (player name in the score frame) and D13 (the stage panel's
per-player score sets) on the three themes. Ghidra: A3 `gamemdx_20240402`, World 20260825.
Addresses are file-relative to `0x180000000`.

## 1. Gameplay player name (A3 ScoreActor init `FUN_180055390`)

- **Where it goes.** After creating `frame_difficulty_%dp%s` (the difficulty frame, `+0x80`), A3
  looks up its child **`name_usr`** (`FUN_1801b6e60`). All three theme score packages have it
  (`dance_score0000_v{0,1,2}` `frame_difficulty_*`: `difficulty_level_usr`, `level_base_tex`,
  `level_tex`, **`name_usr`**). The eras' frames have none, which is why the eras never showed a
  name.
- **What draws it.**
  - A game-heap 0x20 alloc (`FUN_18018da20(DAT_1802dbfb8, 0x20, 0)`), then
    **`agcs::BmpString::ctor(this, font 6, name)`** (`FUN_1801608c0`).
  - A3's font table `0x1802dba98`: 0 `2d_font_system`, 1 `ark_system`, 2 `ui`, 3 `songtitle_m`,
    4 `songtitle_s`, 5 `rival`, **6 `2d_font_player`** (World ships `2d_font_player.arc`; the table
    order is identical in World — `research/legacy-score.md` in the parent project).
  - Then:
    - push onto the gameplay 2D render list (`DAT_1802eee58 + 200`);
    - priority `0x7ffffffb`;
    - scale `0.8 × (1.x | 1.y)`: y = 0.8 × `DAT_180288c60`, x chosen by the PlayerWork byte `+1`;
      SD cabinets use another factor;
    - `vfunc+0x10(0xffffeb08)`;
    - `FUN_180100480(name_usr, text, 1, 3, 1.0)` binds the text to the placeholder every frame;
    - hide the placeholder.
- **Name source.** A3: `*PlayerWork + 8`.
- **World's own HUD name** (World ScoreActor init `FUN_1800775d0`): a separate `dance_name` clip at
  the `name` marker. Its child `name_usr` gets a glyph-texture string (`FUN_1801d3240(name,
  "cote_edge_%s")`, World's look) through a text object at `actor+0x90`.
  - World's name source: `*PlayerWork + 0xC`, or `PLAYER1` / `PLAYER2` (`PTR_s_PLAYER1_180387f00`)
    when the `+4` byte is set and `+0xC` is empty; else `PLAYER`.
  - The theme score package has no `dance_name` export. The mod's score adapter already substitutes
    the stand-in `difficulty_level_base`, whose missing `name_usr` makes World skip its name path,
    so World's name never draws on a theme.
- **Mod building block.** `widget_renderer::create_text_widget` already constructs exactly A3's
  object: `agcs::BmpString::ctor` on a game-heap 0x20 wrapper, registered in the native render
  list (`src/services/widget_renderer.rs:367-419`). The font id is hard-coded to 0; a font
  parameter makes it `2d_font_player`.
  - Position: follow `name_usr`'s screen position. Its parent clip sits at the difficulty marker;
    read it through `bm2d_api` as the marker post-pass and the song-info panel do.
  - Scale, colour and priority: A3's values.
  - Destroy the widget in the GAMEPLAY-exit callback (the layer-before-package rule).

## 2. Stage-panel score sets (A3 `FUN_180030d10` + `FUN_180032240`)

Per side N ∈ {1, 2}, the root child **`pN_score_set_mc`**:

- **Visible iff** the side is entered (`*PlayerWork` byte 0) ∧ (not a course ∨ the first course
  stage).
- **Contents** (`FUN_180032240(root, set, name, area, record)`):

| Child | A3 content | Texture source |
|---|---|---|
| `pN_highscore_usr/highscore_difficulty_usr` | `scene_choice_{beginner,basic,difficult,expert,challenge}` (`_edit_*` variants for edit data) | `common_choice_vN` |
| `…/choice_dancer_name_usr/highscore_name1..8_usr` | one texture per character: `playername_<c>` (a–z, 0–9, `blank`, `exclamation`, `question`, `doll`, `and`, `hifun`, `dot`; `FUN_1800ffe00`) | **`common_texture_v0`** (World loads `common_texture_v3`, which lacks them) |
| `…/choice_score_usr/highscore_000000N_usr` | 7 digits `scene_choice_num_%d`, leading zeros hidden (`FUN_1800ff9d0`) | `common_texture_v0` |
| `…/highscore_rank_usr` | `scene_choice_rank_{aaa, aa_p, aa, aa_m, a_p, a, a_m, b_p, b, b_m, c_p, c, c_m, d_p, d, e}` by record rank 0..15; hidden with no record | `common_choice_vN` |
| `…/fullcombo_mark_rotate_usr/fullcombo_mark_usr` | `scene_choice_fullcombomark_{good, great, perfect, marvelous}` for record FC type 7..10; hidden otherwise | `common_choice_vN` |
| `…/highscore_area_usr` | `dancer_region_*` by area code (`FUN_180100280`: Japan / America ranges by region setting, else a 0x77-entry table) | **`common_area_lang_<lang>_vN`** |

- **Best record.** A3 `FUN_1801271b0(PlayerWork + 0x168, mcode, style, difficulty)` → `{score,
  rank, fc_type}`, only when the chart's "edit" byte is 0. Name: `PlayerWork + 8`. Area:
  `PlayerWork + 0x1C` when the `+1` byte is set.
- **Target set** `pN_target_usr` (same `FUN_180032240` fill):
  - Hidden when the target type (`PlayerWork + 0xDC0`) is −1, or the chart is edit data, or
    `FUN_180124000` (a mode test).
  - Type 0 = the player's own record.
  - Types 1..=3 = rival `PlayerWork + 0xDC4 + (t−1)·4`; 4..=6 = other target kinds via a
    `{3,3,3,3,3,1,2}` class table (`FUN_180130700`).
  - The name comes from `FUN_18012f5d0`, the area from `FUN_18012f680`, the record from
    `FUN_18012eee0`.
- **Always hidden in A3:** `fullcombo_challenge_usr`, `caution_usr` (except the flare caution).

## 3. World's data for the same fields (World kind-3 fill `FUN_180035f00`)

World's own stage panel reads the same records with renumbered offsets:

- Per-stage chart pointer: `*PlayerWork + 0x590 + stage·0x2B8` (course `+0x2D8`). Difficulty at
  `+4`, edit byte at `+0xC`.
- **Best record:** `FUN_1801e2c40(*PlayerWork + 0x178, mcode, style, difficulty)` → a pointer into
  the song node:
  - singles: `node + 0x20 + d·0x30`;
  - doubles: `node + 0x20 + (d·3 + 0xF)·0x10`;

  a 0x30-byte chart record with the score at `+0`. World shows only the score. **The rank and
  full-combo / clear-lamp fields are not yet pinned**; World's song-select wheel draws both, so
  they are there.
- **Target:** type at `*PlayerWork + 0x1328` (−1 hidden; also hidden for edit data, `FUN_1801de5b0`,
  event modes 1 / 2), rivals at `+0x132C`. The same `{3,3,3,3,3,1,2}` class table →
  `FUN_1801efa00` → `FUN_1801ee220` record. World shows the target's score only; the target's
  name / area getters are not yet traced.
- **Name:** `*PlayerWork + 0xC` / `PLAYER1` / `PLAYER2`, as in §1. **Area: not yet found** in World's
  PlayerWork.

## 4. Feasibility and open RE (for the plan)

- **Name:** feasible with existing machinery. New: a font-id parameter on `create_text_widget`, and
  a per-frame follow of `name_usr`. Open: A3's exact scale constants and the `vfunc+0x10` argument
  (colour or outline); the World PlayerWork name path is already known.
- **Score sets:** feasible. Every texture ships in World (`common_choice_vN`, `common_texture_v0`,
  `common_area_lang_*_vN`). The fill is a port of `FUN_180032240` over World's records. Open RE,
  each field failing open (hidden) if unresolved:
  1. rank and lamp fields of World's 0x30-byte chart record, and World's lamp → A3 FC-mark mapping;
  2. the player's area field and World's region setting;
  3. the target's name and area getters (`FUN_1801efa00` result);
  4. the edit-data difficulty variants (`_edit_*`).
- **Packages to request with the panel session:** `common_texture_v0` (explicit name — the bare
  base resolves `_v3`) and `common_area_lang_<lang>_vN` (World's language suffix;
  `common_area_lang_kor` exists only at `_v1` / `_v2`).

## 5. Addendum (2026-09-25, design pass — Ghidra)

### 5.1 World's best record: rank and lamp are pinned

World's song-select side-info table (`RecordPanel::Refresh`, 20260825 `FUN_18019b9f0` — already the
mod's `selectmusic_record_panel_refresh` signature) draws BEST SCORE / CLEAR RANK / rank per
difficulty through three getters on the music manager (`DAT_1806f2d58`), each
`(mgr, chart_ref, side)`, each calling the same best-record lookup
`FUN_1801e2c40(*PW[side] + 0x178, mcode, style, difficulty)` and reading one field:

| Getter | Field of the 0x30-byte record | No record |
|---|---|---|
| score (`FUN_1800fe300`; flare skill `FUN_1800fe100` when the display mode is 1) | `+0x0` | shown as none (`0xFFFFFFFF` when the lamp getter says 0) |
| rank `FUN_1800ff5a0` | `+0x4`: 0..15, shown iff `< 0x10` | `0x11` |
| clear kind `FUN_1800ff4a0` | `+0x8` | `0` |

- **Rank order** (World's 16-name table, `musi_dif_rank_%s`): `aaa, aa_p, aa, aa_m, a_p, a, a_m,
  b_p, b, b_m, c_p, c, c_m, d_p, d, e` — **identical to A3's `scene_choice_rank_*` order**, so the
  rank maps 1:1.
- **Clear kind** (World's `DAT_1804bdc00` → `musi_dif_%s`): 2 `clear_assisted`, 3 `clear_normal`,
  4..6 `clear_life4`, **7 `fc_gofc`, 8 `fc_grfc`, 9 `fc_pfc`, 10 `fc_mfc`** — A3's FC types 7..10
  (good / great / perfect / marvelous) map 1:1. (S-Marvelous' `CLEAR_KIND_MFC = 10` agrees.)
- Difficulty for the lookup: chart `+0x70` (singles) or `+0x74 + style·4`; style from
  `*DAT_1806f14f8 + 4`. The per-stage record header (`PW + 0x590 + stage·0x2B8`) carries the same
  key: mcode `+0x0`, difficulty `+0x4`, style `+0x8` (premium_free's `REC_*` constants).

Still open: the target's name / area getters, a per-player area in World. (Edit-data difficulty
textures: moot — the maintainer confirms World has no edit charts.)

### 5.2 A3's gameplay name, fully decoded

- **Colour.** `vfunc+0x10` is `agcs::BmpString` slot 2 (`FUN_180160ad0`): bytes `{B, G, R, A}` / 255
  into `desc+0x20..+0x2C` (r, g, b, a). `0xFFFFEB08` ⇒ **(1.0, 0.922, 0.031, 1.0)** — yellow.
- **Scale.** `desc+0x58` (x) = `x_factor × y_base`, `desc+0x5C` (y) = `y_base × 0.8`, with
  `y_base` = 0.8 (`DAT_180288c60`; SD machine types 0/1: 0.576 `DAT_1802942ac`) and `x_factor` =
  1.16 (`DAT_1802942a8`) or 1.6 (`DAT_180294188`) when the `*PlayerWork + 1` byte is set. HD:
  **x 0.928 / 1.28, y 0.64**.
- **Binding** `FUN_180100480(placeholder, text, halign 1, valign 3, 1.0)` runs **once, at init**:
  - `x, y` = placeholder screen position (`0x1008`) + 0.5, truncated; `w`, `h` = size
    (`0x1015` / `0x1016`) × scale (`0x100D`);
  - centred box `desc+0x68 = x − w/2`, `desc+0x6C = x + w/2`;
  - `valign > 1` ⇒ `y += h/2`;
  - slot 0 (`FUN_180160a30`: `desc+0x4C/+0x50` = the mod's `TextWidget::set_position`) at `(x, y)`;
  - `desc+0xA8 = 1` (h-align), `desc+0xAC = 3` (v-align), `desc+0xB4 = 1`.
- **Gate.** Created only when `FUN_18014f310(6)` (font 6 resident) is non-null.
- **Render list.** Pushed onto `*(DAT_1802eee58 + 200)` — the same list layout the mod's
  `register_in_render_list` walks (free head `+0x18`, sentinel `+0x20`, head `+0x28`, tail `+0x30`,
  count `+0x3C`) but a **different list** from the mod's `*scene_manager + 0xB0`; node sort key
  `wrapper+0xC = 0x7FFFFFFB`. World's counterpart list is not yet identified (open for the name
  step).
