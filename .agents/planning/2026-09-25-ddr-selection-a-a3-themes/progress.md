# Progress — DDR SELECTION: DDR A / DDR A3 (White) / DDR A3 (Gold)

Updated: 2026-09-26
Status: Step 9 of 9 — docs done, the §7.3 cabinet matrix pending (Steps 1–4 cabinet-proven 2026-09-25, Steps 5–8 2026-09-26; Steps 1–7 committed in `bb6cddc`; Step 8 onward uncommitted — the maintainer commits)
NEXT ACTION: Maintainer: run the "Step 9 cabinet matrix" below (deploy the DLL **and** `data_mods/` whole) and report per block. On a pass: tick plan Step 9 and add the deploy-log line; the feature is complete. Open question for the maintainer: refresh `.agents/summary` through codebase-summary, or hand-edit its component rows.

Resume protocol: read `implementation/plan.md` (checklist + current step), then
`design/detailed-design.md` (approved); decisions in `idea-honing.md` (D1–D26); research in
`research/` (`orientation.md`, `name-and-score-sets.md` incl. the §5 design-pass addendum). Task
files: `.agents/tasks/2026-09-25-ddr-selection-a-a3-themes/stepNN/`; working records:
`.agents/scratchpad/2026-09-25-ddr-selection-a-a3-themes/<task>/`.

## Done

- Planning (PDD Steps 1–8): register D1–D26 (D12 / D13 overridden → included), Readiness Confirmed,
  design approved (World has no edit charts — A3's edit-data handling dropped), plan approved
  (9 steps), `summary.md`.
- Plan Step 1 code (2026-09-25, uncommitted):
  - task-01 `theme-policy-and-trigger` — row values 7..=9 (`DDR A`, `DDR A3 (White)`,
    `DDR A3 (Gold)`), dev knob 1..=8, theme table / `Naming` / `ThemeArc` / `tex_number` /
    `engine_skin` / `skin_name` = row labels, theme rows for judge, FAST/SLOW, full combo,
    game over, danger, pacemaker, stage frame, song-info panel; theme layout roots; host tests
    151 passed (was 142).
  - task-02 `theme-arming-and-engine-wiring` — `GameWork+0xA8` = 0 for a theme, the package
    helper and READY on `policy::package_name`, stage-frame prefix slots 1..=8
    (`stage_frame0000_stage_`), `_sel` movies eras-only. `cargo check`, `cargo fmt`, `./build.sh`
    clean. No signature changes.
  - Theme package exports verified against the stock install (identical export lists to skin 1's
    for the whole-package swaps; song-info / stage-frame children and textures present).
- Plan Step 1 cabinet-proven 2026-09-25 (see the log).
- Plan Step 2 code (2026-09-25, uncommitted): task-01 `theme-hud-adapters` — theme rows for
  `dance_gauge` / `dance_combo` / `dance_score` / `dance_option`; gauge / combo / score / option-icon
  adapters accept skins 6..=8; texture names via `tex_number` (`dance_combo0000_*`,
  `dance_score0000_*`); the combo's blanked-package check reads the policy's package name; the theme
  difficulty frame's `name_usr` placeholder hidden (A3). Host tests 155; build clean. Theme gauge /
  combo / score packages and every theme root marker verified against the stock install.
- Plan Step 2 cabinet-proven 2026-09-25.
- Plan Step 3 code (2026-09-25, uncommitted): task-01 `theme-intro-banners-sound-auto` — READY from
  `dance_message_vN`; CLEARED / FAILED / PRAY FOR ALL from `common_shutter_vN` (PRAY FOR ALL on all
  three themes); A3's skin-0 announcer + crowd (cheer above 0.7, boo below 0.2 on a short combo,
  transcribed from A3 `FUN_1800369e0`); the theme stage-call rule (for Step 4); AUTO 17 → DDR A,
  18–20 → A3 Gold on machine type 4 else White, via the new `services/cabinet.rs` (also used by
  `custom_resolution/debug_ui.rs`); `ACE_TEPPAN3` added to the era bank (DDR A's FAILED shutter).
  Host tests 162; build clean.
- Plan Step 3 cabinet-proven 2026-09-25.
- Plan Step 4 code (2026-09-25, uncommitted): task-01 `theme-stage-panel` — the theme session hosts
  its own root (`common_choice_v0` / `_v2` / `_v1`) with no packages and no cut-in; A3's skin-0 fill:
  `choice_stage_usr2` hidden, band `scene_choice_stage_{1st,2nd,3rd,4th,final,extra}` on the root's
  own `choice_stage_usr` (special stages → `extra`), the song jacket, `vo_stage_*` at once; READY
  dismissal / dwell / release unchanged. Host tests 164; build clean.
- Plan Step 4 cabinet-proven 2026-09-25.
- Plan Step 5 code (2026-09-26, uncommitted):
  - task-01 `score-set-re` — Appendix C rows 1–4 closed in
    `docs/ddr_selection_theme_score_sets.md` (see Deviations for what differs from the design).
  - task-02 `theme-panel-score-sets` — `score_set_logic.rs` (pure) + `score_set.rs` (engine);
    `derive_ddr_sel_score_set` in `signatures.rs` (additions only); `cabinet.rs`
    `licence_key_version()` / `game_language()`; the theme session requests `common_texture_v0`
    and `common_area_lang_<lang>_vN` (when the arc exists); `fill_theme` fills both sides' sets.
    Harness 181 (was 164); build clean; sweep ALL GREEN; `shape_diff.py` reviewed.
- Plan Step 5 cabinet-proven 2026-09-26.
- Plan Step 6 code (2026-09-26, uncommitted): task-01 `theme-gameplay-name` — RE in
  `score_name.rs`'s module doc; `score_name_logic.rs` (pure) + `score_name.rs` (engine);
  `ddr_sel_gameplay_list_push` + `ddr_sel_font_by_id` + `derive_ddr_sel_name` (additions only);
  `widget_renderer::create_text_widget_with_font(font, WidgetStyle, RenderList)` +
  `TextWidget::{set_box, set_vertical_alignment}`; `score.rs` hands the theme `name_usr` over.
  Harness 184; build clean; sweep ALL GREEN; `shape_diff.py` identical.
- Plan Step 6 cabinet-proven 2026-09-26.
- Plan Step 7 code (2026-09-26, uncommitted): task-01 `theme-danger-doubles` — site A verified in
  Ghidra and on all five builds; `ddr_sel_danger_double_skip` + `derive_ddr_sel_danger` (RTTI slot-4
  range, both strings, `75 07`) → `ddr_sel_danger_double_jnz` (additions only);
  `policy::wants_danger_doubles(base, decision)`; `danger.rs` (`init` stock check, idempotent
  `sync`, `restore`); the helper syncs every registered `dance_danger` and restores on a stock one;
  `disarm` restores. Harness 185; build clean; sweep ALL GREEN; `shape_diff.py` identical (`75 07` at
  the JNZ on every build).
- Plan Step 7 cabinet-proven 2026-09-26. Steps 1–7 committed by the maintainer (`bb6cddc`).
- Plan Step 8 code (2026-09-26, uncommitted): task-01 `theme-s-marvelous`.
  - `s_marvelous/targets.rs`: skins 1..=8, `tex_number`, `art_set` (8 → 7), grade sheets 4..=8,
    `u16` bits.
  - `policy::theme_package` names the theme targets in `assets.rs`, with a single `<name>.arc`
    candidate.
  - `u16` masks; the combo sheet name is checked against `combo_math::sheet_prefix`.
  - Generator (the maintainer asked to extend it): art sets 6 (DDR A, `_v0`, `violet_glow`) and 7
    (A3, `_v2`, `violet_outline`), plus the `_v1` ≡ `_v2` guard. It generated
    `data_mods/ddr_selection/s_marvelous/{6,7}/` (36 files); the eras regenerate pixel-identical.
  - Leg H covers skins 6..=8.
  - ddr_selection harness 186; s_marvelous harness OK (111 art files match their donors);
    `--check-world` OK; build clean.
- Maintainer art review 2026-09-26: A3's PURPLE SHADOW word was too light, so `OUTLINE[7]` gained a
  shade of 0.70. The generator gained `--review DIR` (the review pages in
  `target/smarv_legacy_review/`).
- Plan Step 8 task-01 cabinet-proven 2026-09-26 (the word, splash and combo on all three themes).
- Plan Step 8 task-02 code (2026-09-26, uncommitted; maintainer request):
  `smarv-combo-purple-shadow`. On the per-grade combo skins (4..=8) the S-Marvelous combo sheet
  follows the Judgement Color: ALL PURPLE digits, or PURPLE SHADOW (the skin's digits with the
  word's violet glow / outline).
  - Art is `dance_combo/smarvelous_{all_purple,purple_shadow}_{key}.png`; the old
    `smarvelous_{key}.png` files became the `all_purple` ones, byte-identical.
  - `COMBO_SHADOW`: glow on X / 2013-A / DDR A (the bold outline kept; maintainer review), the
    darkened outline on A3.
  - DLL: `assets::{serve_image, combo_pngs_for, restage_legacy_combo}`,
    `combo::set_legacy_color`, the Judgement Color row swaps the combo too; the word's enable-time
    copy re-serves on a byte change.
  - Leg H: 166 files match their donors. A full regeneration matches the shipped art (137 files).
- Plan Step 8 cabinet-proven 2026-09-26 (task-01 and task-02).
- Plan Step 9 code / docs (2026-09-26, uncommitted): task-01 `release-docs`.
  - README Highlights sweep (maintainer request): at most two end-user paragraphs per mod; Versus
    Bot, Background Dancer Revival, Gameplay Timing Fixes, Power User Statistics and Custom
    Resolution trimmed; Assist Tick loses a release-note sentence; the 2-Player BPL link to the
    Versus Bot anchor is fixed.
  - DDR SELECTION rewritten: the nine choices by their row labels, AUTO → DDR A / DDR A3
    (Gold on a gold cabinet), the theme panel's best score / target, the dancer name, the era
    cut-in, the A3 import one-liner. Its Full Feature List row and the S-Marvelous mention are
    updated too.
  - `option_strings.py` `ddr_selection` preview in en / ja / ko regenerated (no overflow; the
    unchanged `seop_item_ddr_selection.png` re-encode was reverted). The `options.rs` row
    description and the mod description are updated.
  - `ddr_selection/mod.rs` Surfaces (theme panel + score sets, dancer name, theme packages).
  - The research-note status line.
  - Gate: `cargo check` / `cargo fmt` / `./build.sh` clean; harnesses ddr_selection 186,
    s_marvelous 172 (+ Leg H), custom_options 59, custom_resolution 24, mod_menu 40,
    multiplayer_bot 98; signature sweep ALL GREEN.

## In flight

- Uncommitted (Steps 8–9): `src/mods/s_marvelous/{targets, assets, legacy, afp_patches, splash, combo,
  flash, mod}.rs`, `src/mods/ddr_selection/{policy, mod, options}.rs`,
  `scripts/gen_ddr_selection_smarv_art.py`, `scripts/validate_s_marvelous.sh`, `scripts/option_strings.py`,
  `data_mods/ddr_selection/s_marvelous/**` (README, sets 4–7 combo files renamed / added, 6/ and 7/
  new), `data_mods/custom_options/*/tex/seop_image_ddr_selection.png` (en / ja / ko), `README.md`,
  `docs/ddr_selection_a3_themes_research.md`, plus the step08 / step09 task files, the
  `theme-s-marvelous` / `smarv-combo-purple-shadow` / `release-docs` scratchpad records and this
  file / `plan.md`.

## Step 9 cabinet matrix — procedure (maintainer; design §7.3)

Deploy: `./scripts/deploy.sh` (the DLL) and copy `data_mods/` **whole**. That covers the option
preview textures, the S-Marvelous art (sets 4–7 renamed / added) and the A3 import if you use it.

Setup: DDR SELECTION on, S-Marvelous on. For block D, also turn on Center Arrows, Overlay Element
Styling and Playfield Styling.

**A. Boot and menus.**
- `log.txt` has no DDR SELECTION / S-Marvelous WARN. Look for these lines:
  - `DDR SELECTION: LayoutActor package-helper detour installed`;
  - `theme score sets ready`;
  - `theme dancer name ready`;
  - `[+] ddr_sel_danger_double_jnz (derived)`;
  - `SMarvelous: DDR SELECTION legacy art staged … (word [1, …, 8] …)`.
- The DDR SELECTION option row offers OFF, AUTO, the five eras, DDR A, DDR A3 (White) and DDR A3
  (Gold).
- Its preview reads "… an earlier DDR, from 1stMIX to DDR A3." Check ja / ko too if the cabinet
  language can be switched.
- The mod menu shows the new description.

**B. Each theme (DDR A, DDR A3 (White), DDR A3 (Gold)).** Play one song for each row. A theme
passes when the stage panel, HUD, S-Marvelous, danger, dancer name and banner look right, with
nothing from World or another skin mixed in:

| Play | Check |
|---|---|
| 1P singles, normal scroll | The panel (band, jacket, score sets), READY!, the HUD, the dancer name, S-Marvelous word / combo, CLEARED |
| 1P singles, reverse | The HUD and the name follow the reverse layout |
| 1P doubles | The danger flash spans the doubles playfield; the name is in the single frame |
| Versus (two players) | Both sides' HUD, names and score sets; one skin for both |
| Versus Bot (BOT OPPONENT) | Both sides render; the bot side is never the one picking the skin |
| Quick restart, then quick fail | Clean re-arm and exit: no stuck panel, no stale name, no WARN spam |

**C. Transitions.** For each theme, play stock → theme → era (any of 1–5) → stock, back-to-back.
Every song shows only its own skin: World's HUD / panel / banner on the stock songs, and the era's
art on the era song.

**D. Mods together.** On one theme, play a singles song and a doubles song with S-Marvelous, Center
Arrows, Overlay Element Styling and Playfield Styling all on. The styled elements scale and move as
on stock songs; nothing is misplaced or doubled.

**E. Gauges on DDR A3 (Gold).** Play one song each with FLARE, FLOATING FLARE, GRADE, LIFE4 and
RISKY. The real FLARE gauge art appears where it applies.

**F. AUTO.** Set the row to AUTO:
- a DDR A song → DDR A;
- A20 / A20 PLUS / A3 songs → White on a white cabinet and Gold with the SMX GOLD force (or a gold
  cabinet);
- a DDR 2014 song → 2013-2014;
- a World song → stock.

Log: `armed skin N … source=AUTO series …`.

**G. Tohoku EVOLVED.** On each theme it ends on PRAY FOR ALL.

**H. Names and records.**
- A profile player and a guest (`PLAYER1` / `PLAYER2`).
- A chart with a best score / full combo and TARGET set, and one never played with TARGET OFF.
- The panel's score sets and the in-song name follow the Step 5 / Step 6 expectations.

**I. Eras and stock unchanged.** A few era songs (1stMIX-5thMIX, X-X3 vs 2ndMIX, 2013-2014) and
stock songs look exactly as before this feature. This is the feature's completion gate.

Report per block: pass / what looked wrong (photos help), plus any WARN lines.

## Step 8 task-02 cabinet demo — procedure (maintainer)

Deploy: `./scripts/deploy.sh` (the DLL), and copy `data_mods/ddr_selection/s_marvelous/` **whole**
to the cabinet. On the cabinet, delete the old `4/` and `5/` `dance_combo/smarvelous_<key>.png`
files (renamed to `smarvelous_all_purple_<key>.png`), or replace the folder.

Setup: S-Marvelous and DDR SELECTION on. Judgement Color defaults to PURPLE SHADOW.

1. **Boot log.** Each per-grade skin's line reads `… S-Marvelous combo sheet staged (11 images,
   purple_shadow, fresh atlas)`, for `dance_combo0004_v0`, `0005_v0` (with the A3 import),
   `0000_v0`, `0000_v2` and `0000_v1`. There should be no `combo art incomplete` or `combo art
   missing` WARN.
2. **PURPLE SHADOW.** Keep an all-S-Marvelous combo going on X, 2013-A, DDR A and DDR A3 (White or
   Gold).
   - X / 2013-A / DDR A: the skin's own cream digits, their bold dark outline kept, with a violet
     glow around it.
   - DDR A3: the cream digits with a dark violet outline.
   - On the first loose Marvelous the combo switches to the grade colour.
   - Log: `DDR SELECTION: legacy combo S-Marvelous sheet (skin N, …)`.
3. **ALL PURPLE, live.** Change Judgement Color to ALL PURPLE in the mod menu.
   - Log: `skin N combo sheet all_purple staged (11 image(s) changed); applies when dance_combo
     next loads` for each skin, alongside the word lines.
   - The next song's all-S-Marvelous combo has violet digits (the Step 8 look).
   - Switch back to PURPLE SHADOW and check that it follows the same way.
4. **Across a reboot.** Leave ALL PURPLE set, reboot, and play a song: the combo is still ALL
   PURPLE, and so is the word.
5. **World.** A stock song keeps World's violet S-Marvelous combo digits in both settings.

Report: pass / what looked wrong (a photo of each skin's S-Marvelous combo helps).

## Step 8 cabinet demo — procedure (maintainer)

Deploy: `./scripts/deploy.sh` (the DLL), and **copy** `data_mods/ddr_selection/s_marvelous/6/` and
`7/` into the cabinet's `data_mods/ddr_selection/s_marvelous/`. Without them the themes keep a plain
Marvelous.

Setup: S-Marvelous Judgement and DDR SELECTION on. The first boot stages the new art into
`data_mods/s_marvelous/<package>_ifs/`, which takes a little longer; later boots are cached.

1. **Boot log.**
   - `SMarvelous: DDR SELECTION legacy art staged in … ms (word [1, …, 6, 7, 8], S-MFC splash [1,
     …, 6, 7, 8], combo sheet [4, …, 6, 7, 8])`. Skin 5's combo sheet needs the A3 import.
   - One `… splash staged (4 template(s), 5 region(s))` and one `… S-Marvelous combo sheet staged
     (11 images, fresh atlas)` each for `dance_fullcombo0000_v0/_v2/_v1` and
     `dance_combo0000_v0/_v2/_v1`.
   - No `resolved N art shapes (want 5)`, `differs from DDR SELECTION's` or `art missing` WARN.
2. **Word, each theme.** On DDR A, White and Gold, hit S-Marvelous steps:
   - PURPLE SHADOW (default): DDR A keeps its letters and black outline with a violet rim / glow;
     A3 keeps its letters with a violet outline;
   - ALL PURPLE (Judgement Color row): the whole word is violet;
   - A plain Marvelous (outside the window) keeps the theme's own shimmer.
   - Log: `dance_judge0000_vN (skin N) patched (…)` and `flash live on DDR SELECTION skin N`.
3. **S-MFC splash.** An all-S-Marvelous full combo on each theme shows the violet "MARVELOUS
   FULLCOMBO!!!" splash (rings / light / side light violet). Log:
   `dance_fullcombo0000_vN <template> (skin N) patched`. A normal MFC keeps the stock splash.
4. **Combo.** While the combo is all S-Marvelous, the theme's combo digits and "combo" word are
   violet. They go back to the grade colour on the first loose Marvelous. Log: `DDR SELECTION:
   legacy combo S-Marvelous sheet (skin N, …, dance_combo0000_smarvelous)`.
5. **Generations.** DDR A → White → Gold → an era 4–5 song back-to-back: each shows its own
   generation's violet art (DDR A's heavy font vs A3's condensed caps), with no bleed.
6. **Doubles / reverse.** A doubles and a reverse full combo use the matching splash template.
7. **Stand-down (optional).** Rename the cabinet's `s_marvelous/6/` and reboot.
   - Log: `no S-Marvelous art for DDR SELECTION skin 6`.
   - DDR A shows a plain Marvelous; White / Gold still dress.
   - Restore the folder afterwards.

Report: pass / what looked wrong (a photo or clip of each theme's word and splash helps).

## Step 7 cabinet demo — procedure (maintainer)

Deploy: `./scripts/deploy.sh` (DLL only; no new `data_mods/` content).

1. **Boot log.** You should see `[+] ddr_sel_danger_double_jnz (derived) @ +0x… (onInitialize+0xC4)`.
   There should be no `DDR SELECTION: danger doubles site …` WARN.
2. **Doubles, each theme.** Play a doubles chart on DDR A, White and Gold, and let the gauge drop
   into danger.
   - The flash spans the whole doubles playfield (`danger_double`), not one pad's lane.
   - Log: `dance_danger -> dance_danger0000_v0 (skin N, …)` then `danger doubles on (the theme's
     danger_double on doubles)`.
3. **Singles.** A theme singles song (1P and versus) shows the single-lane flash, as in Step 1.
4. **Era after theme.** Play a theme doubles song, then a doubles era song (any of 1–5).
   - The era shows its single-lane flash (A3's behaviour).
   - Log: `danger doubles restored (World's record-skin rule)` once, at the theme song's disarm.
5. **Stock.** A stock doubles song shows World's own full-width flash (unchanged).
6. **Restart.** On a theme doubles song, quick restart and quick fail keep the full-width flash.
   - Log: no second `on` line within the song, one `restored` after it.
7. **With Playfield Styling** lane width on, the theme doubles flash sizes like World's.

Report: pass / what looked wrong (a photo of the doubles flash helps).

## Step 6 cabinet demo — procedure (maintainer)

Deploy: `./scripts/deploy.sh` (DLL only; no new `data_mods/` content).

1. **Boot log.** You should see:
   - `[+] ddr_sel_screen_graph_global (derived) @ +0x… (gameplay list +0xC8)`;
   - `DDR SELECTION: theme dancer name ready (gameplay list +0xC8, font 6)`.
2. **Profile player, each theme.** Play a song on DDR A, White and Gold.
   - The yellow dancer name shows centred in the difficulty frame, in A3's wide profile scale.
   - Log: `theme dancer name bound (1P, N chars, profile)`, plus once per boot `theme dancer-name
     widgets created (font 6, gameplay list +0xC8, key 0x7FFFFFFB)`.
3. **Guest.** A guest shows `PLAYER1` / `PLAYER2` in the narrower guest scale. In versus, both
   sides show their own names.
4. **Reverse / doubles.** Reverse scroll: the name follows the reverse difficulty frame. Doubles:
   the single frame shows the name.
5. **Layering.**
   - The stage panel at song start covers the name.
   - The end shutter (CLEARED / FAILED) covers the name.
   - The name sits above the frame art (not hidden behind it).
6. **Scope.**
   - Era and stock songs show no name.
   - After the song (results onward) nothing lingers.
   - Quick restart and quick fail rebind cleanly (no stale name at the old position).
7. **Budget.** After 20+ consecutive songs:
   - no `render list node pool exhausted` WARN;
   - no second `widgets created` line (the two widgets are reused).

Report: pass / what looked wrong. For the position, a photo of the difficulty frame helps.

## Step 5 cabinet demo — procedure (maintainer)

Deploy: `./scripts/deploy.sh` (DLL only; Step 5 adds no `data_mods/` content). Every package the
panel requests ships in the stock install.

1. **Boot log.** One `[+] ddr_sel_best_record (derived) @ +0x… (PlayerWork score db +0x178)`
   (`+0x188` on 20250805 / 20260224) and `DDR SELECTION: theme score sets ready (score db
   +0x…, TARGET +0x…)`. A `theme score sets partial (…)` WARN names what is missing.
2. **Arm.** A theme song logs `legacy stage panel armed at … (skin N: common_choice_vN -- A3's
   own skin-0 panel, no cut-in; score sets common_texture_v0 + common_area_lang_<lang>_vN)`.
   On a Chinese-language cabinet, or Korean on DDR A, it reads `no area package`.
3. **High score.** Profile player, a chart you have full-combo'd, on each theme (DDR A / White /
   Gold). The panel's P1 set shows:
   - the difficulty word;
   - your name in A3's glyphs;
   - the 7-digit best score with the leading zeros hidden;
   - the rank letter;
   - the FC mark matching the lamp (good / great / perfect / marvelous);
   - your region.

   Log: `theme score sets filled (P1 diff Some(n) <score> rank <r> clear <k>, target …; P2 hidden)`.
4. **No record.** A chart never played: score `0`, no rank, no mark, name and region shown.
   Log: `no record`.
5. **Targets.** Set TARGET to each of these in turn:
   - own best: the target set repeats your record;
   - a rival: the rival's name, score and region;
   - a ranking (e.g. machine best): the record holder's name and score, blank name if none;
   - OFF: the target set is hidden.
6. **Guest.** A guest shows `PLAYER1` / `PLAYER2` glyphs and region `unknown`. In **versus**, both
   sides fill at their own positions.
7. **Eras / stock.** An era song's panel shows no score sets (as before); a stock song is World's
   panel. No `could not request common_texture_v0` or `… unreadable` WARN.
8. **Stability.** Back-to-back theme panels and a quick restart release the packages (`stage-panel
   packages released [common_texture_v0, common_area_lang_…]`) with no stuck shutter.

Report: pass / what looked wrong (a photo of a panel with a record helps), plus the
`theme score sets filled` lines.

## Deploy & test log

- 2026-09-25, Step 1 build (maintainer): everything works — themes arm, the whole-package swaps, stage frame and song-info panel show the right generation; the back-to-back spike (Gold → White → DDR A → Gold → era 3–5 → 1stMIX-5thMIX → Gold → OFF → White) shows no texture bleed. **AS1 holds.**
- 2026-09-25, Step 2 build (maintainer): everything worked correctly in-game (theme gauge incl. FLARE art, combo, score / difficulty, option icons).
- 2026-09-25, Step 3 build (maintainer): everything looks good in-game (READY, banners incl. PRAY FOR ALL, announcer / crowd, AUTO).
- 2026-09-25, Step 4 build (maintainer): everything looked good (theme stage panels, bands, stage call, no cut-in).
- 2026-09-26, Step 5 build (maintainer): everything worked (theme score sets: records, no-record, targets, guest / versus, eras unchanged).
- 2026-09-26, Step 6 build (maintainer): everything passed (theme dancer name: profile / guest / versus, reverse / doubles, layering under panel and shutter, scope, widget reuse).
- 2026-09-26, Step 7 build (maintainer): everything worked (theme doubles danger full-width, singles / eras / stock unchanged, restore after a theme song).
- 2026-09-26, Step 8 task-01 build (maintainer): the theme S-Marvelous word / splash / combo worked as expected. The art review asked for a darker A3 PURPLE SHADOW word (done) and for the combo sheet to follow the Judgement Color (task-02). The review of that asked for X's combo to keep its bold black outline (now `glow`).
- 2026-09-26, Step 8 task-02 build (maintainer): everything looks good in-game (the S-Marvelous combo follows the Judgement Color on the per-grade legacy skins). **Step 8 cabinet-proven.**

## Deviations & open questions

- `policy::skin_name` now returns the row labels (logs read `2013-2014`, not `2013-A`), per D18.
- The design assumed every theme cue was already in the `dsel` bank; DDR A's FAILED shutter
  (`common_shutter_v0`) embeds `ACE_TEPPAN3`, now added to the manifest (a sweep of every theme
  package found no other gap).
- Step 8:
  - `fc_expected_shapes` needs no theme branch: every theme `dance_fullcombo0000_vN` template has
    the eras' 5 Marvelous shapes.
  - The theme target names come from a new `policy::theme_package(base, skin)`, a thin wrapper on
    `package_name` for the row whatever the adapter availability.
  - The PURPLE SHADOW recipes are `violet_glow` (0.40, 0.70) for DDR A, which is shaped like
    2013-A, and `violet_outline` (0.20, 0.45, shade 0.70) for A3, whose thick dark outline is
    like X's. The shade darkens the violet outline (maintainer art review 2026-09-26: too light at
    1.0). Review pages: `python3 scripts/gen_ddr_selection_smarv_art.py --review
    target/smarv_legacy_review`.
  - `targets::tex_number` duplicates DDR SELECTION's rule (the file is harness-mounted). The combo
    sheet name is cross-checked at staging.
- Step 8 task-02 is beyond the design (maintainer request): the S-Marvelous combo sheet follows the
  Judgement Color on the per-grade legacy skins. World's combo keeps its one violet variant. The
  word's enable-time staging now re-serves its PNG only on a byte change and then purges
  LayeredFS's converted copy. Before, a colour changed while the game was off could keep serving
  the old conversion, since the cache freshness test is by mtime.
- Step 7: `policy::wants_danger_doubles` takes the base as well as the decision (`(base,
  decision)`), so "only `dance_danger`" is part of the tested rule. The derivation also pins the
  site to DanceDangerActor's RTTI slot 4 (init + 0xAC on every build). There is no design change.
- Open RE (design Appendix C): none left. Rows 1–4 were closed by Step 5 and row 5 by Step 6.
- Step 6 deviations from the design (RE in `score_name.rs`'s module doc):
  - **World keeps A3's render list.** The screen graph is identical; slot 8 (`*G + 0xC8`) is
    still World's gameplay list, and its ScreenRoot draws by the `+0xC` key. So the name uses A3's
    list and key `0x7FFFFFFB`, and §6.2's READY-to-shutter fallback is not used. The name shows
    while the difficulty layer lives and is visible, and the shutter (slot 7) covers it.
  - **`widget_renderer::create_text_widget_with_font`** also takes a `RenderList` (overlay or
    a graph list + sort key). The existing constructors are unchanged.
  - **SD cabinets** (machine types 0 / 1) get A3's SD base 0.576; the design only listed HD.
- Step 5 deviations from the design (all in `docs/ddr_selection_theme_score_sets.md`):
  - **Best-record anchor:** neither a new AOB nor the rank-getter path, but the case-0 call inside
    the already-swept `ghost_id_lookup` (bytes the pattern pins). Score db `+0x178`, old builds
    `+0x188`.
  - **Target class table** is `{3,3,3,3,0,1,2}`: the design's `{3,3,3,3,3,1,2}` misread an 8-byte
    store. World's resolver `FUN_1801efa00` is not called (its walk is unchecked); the set is
    found with `target_name`'s probed search, reusing `target_name_sites()`.
  - **Area exists** (`PlayerWork+0x20` behind the profile byte `+5`). There is no World target
    area getter, so a rival reads `set+0x54` and a ranking holder reads the dword before its
    name. Region and language come from ark exports (`arkMDXGetLicenceKeyVersion`,
    `arkMDXGetGameOptionsLanguage`) via `cabinet.rs`, with no signature.
  - **"Record" means clear kind ≠ 0.** This is World's own played test; A3 did not check it.
  - **Name** uses World's rule (`PLAYER1` / `PLAYER2` for an empty name). A3 used the raw name.
  - A **TARGET value outside -1..=6** hides the set; A3 left it visible and unfilled.
- Assumption AS1 (no texture-name bleed across generations) is tested by the Step 1 spike; a
  failure stops implementation and reopens the design (design §6.3).

## Key facts for a cold resume

- Row 7 / 8 / 9 = `DDR A` / `DDR A3 (White)` / `DDR A3 (Gold)` → skins 6 / 7 / 8 → suffixes `_v0` /
  `_v2` / `_v1`. Record skin = the skin; `GameWork+0xA8` = 0 for themes (`policy::engine_skin`).
- AUTO (Step 3): 14–16 → `2013-2014`, 17 → DDR A, 18–20 → Gold on machine type 4 else White.
- `policy::tex_number(skin)` = 0 for themes (`dance_combo0000_*`, `dance_score0000_*`,
  `stage_frame0000_stage_*`).
- A theme row for a base lands only with its adapter's range widening (gauge / combo / score /
  option icons: Step 2; `dance_message`: Step 3).
- Never produce a bare `<base>0000`; theme names always carry `_vN` (World's probe reaches them via
  its bare rung).
- Dancer name (Step 6): `score.rs` init POST → `score_name::bind(side, difficulty layer,
  name_usr)`. A self-requeuing render-thread job re-binds each frame; `hide_all()` runs at the
  GAMEPLAY exit, disarm and disable. The two widgets are created once and never destroyed.
- Danger (Step 7): `danger::sync(want)` flips the JNZ at `ddr_sel_danger_double_jnz` (`75 07` ↔
  `90 90`). The helper calls it for every registered `dance_danger` (`true` only for a theme), and
  `after_stock("dance_danger")` and `disarm` restore it.
- S-Marvelous (Step 8): skins 6 / 7 / 8 stage into `…0000_v0` / `_v2` / `_v1` from art sets 6 / 7 /
  7 (`targets::art_set`). The art is generated by `scripts/gen_ddr_selection_smarv_art.py --skins
  6,7`; the set-7 twin guard refuses a `_v1` that is not pixel-identical to `_v2`.
- Score sets (Step 5): the pure fill is `score_set_logic::fill_side`; the engine reads are
  `score_set::side_inputs`; the panel applies them in `panel.rs::fill_score_sets`. The theme
  session's only tickets are `common_texture_v0` and the area package; `art_ready` waits for them
  (at the latest until World's swap).
