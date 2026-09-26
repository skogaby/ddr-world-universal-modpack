# Progress — DDR SELECTION: DDR A / DDR A3 (White) / DDR A3 (Gold)

Updated: 2026-09-26
Status: Step 7 of 9 — code done, cabinet demo pending (Steps 1–4 cabinet-proven 2026-09-25, Steps 5–6 2026-09-26; all code uncommitted — the maintainer commits)
NEXT ACTION: Maintainer: run the "Step 7 cabinet demo" below and report. On a pass: tick plan Step 7, add the deploy-log line, then start Step 8 (S-Marvelous): write `.agents/tasks/2026-09-25-ddr-selection-a-a3-themes/step08/` task file, then `src/mods/s_marvelous/targets.rs` (design §4.13).

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

## In flight

- Uncommitted: `src/mods/ddr_selection/**` (policy, trigger, marker_keys, song_info_logic, mod,
  package_helper, intro, stage_frame, movie_sel, song_info, options, gauge, gauge_math, combo,
  combo_math, score, score_math, option_icons, banner, banner_logic, panel_logic, sound/rules,
  sound/cues, sound/call_voice, panel, score_set (new), score_set_logic (new), score_name (new),
  score_name_logic (new), danger (new)), `src/services/widget_renderer.rs`, `src/widgets/text_widget.rs`,
  `src/services/{cabinet.rs (new), mod.rs}`, `src/mods/custom_resolution/debug_ui.rs`,
  `src/core/signatures.rs` (Step 5 / 6 / 7 additions only — the file also carries another feature's
  uncommitted edits), `scripts/validate_ddr_selection.sh`, `docs/ddr_selection_theme_score_sets.md`
  (new), plus the task / scratchpad records.

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

## Deviations & open questions

- `policy::skin_name` now returns the row labels (logs read `2013-2014`, not `2013-A`), per D18.
- The design assumed every theme cue was already in the `dsel` bank; DDR A's FAILED shutter
  (`common_shutter_v0`) embeds `ACE_TEPPAN3`, now added to the manifest (a sweep of every theme
  package found no other gap).
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
- Score sets (Step 5): the pure fill is `score_set_logic::fill_side`; the engine reads are
  `score_set::side_inputs`; the panel applies them in `panel.rs::fill_score_sets`. The theme
  session's only tickets are `common_texture_v0` and the area package; `art_ready` waits for them
  (at the latest until World's swap).
