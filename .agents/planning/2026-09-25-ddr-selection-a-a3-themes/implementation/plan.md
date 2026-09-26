# DDR SELECTION — DDR A / DDR A3 (White) / DDR A3 (Gold) — Implementation Plan

Status: Approved 2026-09-25

Design: `.agents/planning/2026-09-25-ddr-selection-a-a3-themes/design/detailed-design.md`
(approved 2026-09-25). Section references (§) point there; this plan does not restate them.

**Every step's gate:**

- `cargo check --target x86_64-pc-windows-msvc` clean;
- `cargo fmt` (whole crate);
- `./build.sh` clean;
- `scripts/validate_ddr_selection.sh` green, plus `scripts/validate_s_marvelous.sh` when S-Marvelous
  files change;
- when `src/core/signatures.rs` or a `match + N` reader changes: `./scripts/validate_signatures.sh
  <supported-builds folder>` green and a `scripts/sig_harness/shape_diff.py` review;
- a cabinet deploy (`./scripts/deploy.sh`, plus any new `data_mods/` content) with the step's Demo
  observed in spice2x `log.txt`;
- `progress.md` updated.

The maintainer commits.

**Rule for every step:** a theme's policy row for a base lands in the **same** step as the widening
of that base's adapter. A theme package handed to an adapter that still rejects skins 6..=8 would
meet World's unadapted actor, which NULL-derefs.

## Checklist

- [x] Step 1: Theme identity, whole-package swaps, song-info panel, stage frame — and the cross-generation texture spike — cabinet-proven 2026-09-25 (spike passed: no texture bleed across generations)
- [x] Step 2: Theme HUD: life gauge, combo, score, option icons — cabinet-proven 2026-09-25
- [x] Step 3: READY, end banners, announcer / crowd, AUTO with the cabinet read — cabinet-proven 2026-09-25
- [x] Step 4: Stage panel: theme variant and A3's skin-0 fill — cabinet-proven 2026-09-25
- [x] Step 5: Stage-panel score sets (RE first) — cabinet-proven 2026-09-26
- [x] Step 6: Gameplay player name (RE first) — cabinet-proven 2026-09-26
- [ ] Step 7: Danger on doubles (scoped patch)
- [ ] Step 8: S-Marvelous on the themes
- [ ] Step 9: Release integration and the cabinet matrix

---

Step 1: Theme identity, whole-package swaps, song-info panel, stage frame — and the cross-generation texture spike

- **Objective.**
  - Make the three themes selectable and armed end to end with the parts that need no new adapter
    logic.
  - Use them to test assumption AS1 (no texture-name bleed across generations, §2.4, §6.3) before
    anything else is built on it.
- **Guidance.**
  - **Trigger** (§4.1): `ROW_MAX` 9, labels 7..=9, clamp, dev knob `1..=SKIN_MAX`. AUTO is unchanged
    in this step, so themes are explicit-only for now.
  - **Policy** (§4.3, §5.2, §5.3):
    - `SKIN_MAX` / `ERA_MAX`, `Theme`, `is_era` / `is_theme`, `tex_number`, `engine_skin`,
      `skin_name` = row labels;
    - `Entry.skins: u16`;
    - `fixed_arc` → `Naming` with `ThemeArc`;
    - one `package_name` used by `package_helper.rs`;
    - theme rows for `dance_judge`, `dance_fast_slow`, `dance_fullcombo`, `dance_game_over`,
      `dance_danger`, `dance_score_compare`, `dance_stage` and `dance_song_info` only.
  - **`mod.rs`** (§4.4): `arm` writes `engine_skin`; the dev-knob WARN text changes; `movie_sel`
    plans `_sel` movies only for eras.
  - **Adapters, this step's part** (§4.5):
    - `stage_frame.rs`: slots 1..=8, prefix from `tex_number`, reach check to slot 8;
    - `song_info_logic::mode_for_skin(6..=8) = Panel`;
    - `marker_keys::root_name(6..=8)`.
  - **The unchanged paths**, confirmed: panel, banner, intro and option-icon code take no theme
    branch yet.
    - `panel_logic::packages` / `banner_logic::art` return `None` for 6..=8. With no panel session,
      the song shows World's stage panel and World's end banner.
    - `dance_message` has no theme row yet, so the song keeps World's intro.
  - Module-doc touch-ups where a range or name changed.
- **Tests** (same step, host harness):
  - `trigger`: labels and budget for 0..=9, clamp, explicit 7..=9 → 6..=8, dev knob 1..=8;
    `auto_skin` still returns the era values (the AUTO change is Step 3).
  - `policy`:
    - every theme package name for this step's bases × three themes;
    - `tex_number`, `engine_skin`, `skin_name` ≡ labels;
    - extended invariants: no bare `0000`, every theme name has `_vN`, no overlapping rows, era
      rows never chosen for themes and vice versa;
    - bases without a theme row stay Stock for 6..=8 (gauge, combo, score, option, message).
  - `marker_keys`: theme roots. `song_info_logic`: Panel for 6..=8.
- **Integration.** Builds on the shipped mod unchanged. S-Marvelous stands down on skins 6..=8 by
  its existing `LEGACY_SKINS` rule, so theme songs show a plain Marvelous.
- **Demo, which is also the spike.**
  - **Setup.** On the cabinet, pick each theme in the row (and via `DDR_SELECTION_FORCE=6..8` in
    developer mode).
  - **What each theme song shows.**
    - The theme's judgement words, FAST / SLOW and full combo. DDR A's heavy font differs visibly
      from A3's condensed caps.
    - Game over and danger (singles) from `_v0`.
    - A3's pacemaker.
    - The theme's stage frame and A3 song-info panel at the theme root's positions.
    - The rest of the HUD is World's.
  - **Spike sequence.** Gold → White → DDR A → Gold → an era 3–5 song (`dance_song_info0000_v2`)
    → a skin-1 song (`stage_frame0000_*` in its stage-frame package) → Gold → stock → White.
  - **Pass.** The stage-frame and song-info frame colours (gold / silver / DDR A blue) switch
    correctly on every song, including quick restart and quick fail.
  - **Failure** (another generation's pixels) ⇒ stop; record which cause (lingering registration
    or global lookup) in `progress.md` and reopen the design (§6.3).
  - **Log.** `DDR SELECTION: armed skin 7 (DDR A3 (White)) …` and one `dance_* -> …_vN` INFO per
    package.

Step 2: Theme HUD: life gauge, combo, score, option icons

- **Objective.** The full A3 skin-0 gameplay HUD on all three themes (R8).
- **Guidance** (§4.5):
  - **Gauge:** `legacy_actor` 1..=8; explicit theme arm in `gauge_math::fill_mode`.
  - **Combo:** `PACKAGE_STATE` sized `SKIN_MAX + 1`; `package_usable` checks the policy's package
    name; `sheet_prefix` via `tex_number`.
  - **Score:** range 1..=8; `score_math` names via `tex_number`; the init POST keeps the `dance_name`
    stand-in. The name handoff is Step 6.
  - **Option icons:** `legacy_skin` 2..=8.
  - **Policy:** theme rows for `dance_gauge`, `dance_combo`, `dance_score`, `dance_option`, landing
    with their adapter changes.
- **Tests:**
  - `gauge_math`: theme fill = `Segmented {26, 17.0, partial}` outside FLARE labels, continuous on
    6..=16;
  - `combo_math`: theme sheet prefixes per worst grade, the `smarvelous` sheet name, growth /
    cell-width non-skin-1 paths;
  - `score_math`: `dance_score0000_*` digits, grey zeros, commas, `dance_score0000_lvNN`,
    priority 7;
  - `policy`: the four new rows per theme and the adapter they require.
- **Integration.** Step 1's rows and names; the adapters' existing scoped patches (gauge export,
  score init names) now also fire for themes.
- **Demo.**
  - **What each theme shows.**
    - A3's gauge (P2 mirrored; segmented with the partial cell).
    - A3's combo growing with the count and taking the worst-grade colour.
    - A3's score with grey zeros and commas.
    - The level texture in the difficulty frame.
    - A3's option icons.
    - World's name and BPM hidden.
  - **On A3 Gold,** one song each on FLARE, FLOATING FLARE, GRADE, LIFE4 and RISKY: real FLARE gauge
    art appears. On DDR A, the FLARE states behave like the eras, which also have no FLARE labels.
  - **Doubles and reverse** land at the theme root's positions.

Step 3: READY, end banners, announcer / crowd, AUTO with the cabinet read

- **Objective.** Complete the theme song outside the stage panel (R3, R7, R11, R12), and let AUTO
  reach the themes.
- **Guidance.**
  - **Intro:** `intro.rs` takes its package name from `policy::package_name` (§4.6); the theme row
    for `dance_message` (adapter ReadyGo).
  - **Banners** (§4.8): `PACKAGES` per skin 1..=8 (static), `art` / `package` 1..=8,
    `has_pray_for_all` 4 | themes.
  - **Sound** (§4.7): `rules::step` 1..=8, the theme crowd arm, a 4-slot `plays` array (executed in
    order by `call_voice.rs`), `all_cues` additions; `panel_logic::stage_voice` theme branch (used in
    Step 4).
  - **Cabinet** (§4.2): new `src/services/cabinet.rs` `machine_type()`, promoted from
    `custom_resolution/debug_ui.rs`, which now calls it.
  - **AUTO** (§4.1): `Inputs.gold_cabinet`, the new `auto_skin`; `resolve_song` fills
    `gold_cabinet` for AUTO rows only.
- **Tests:**
  - `trigger`: the full AUTO table incl. 14–16 → 5, 17 → 6, 18–20 → 7 / 8 by `gold_cabinet`, 21+
    → 0; explicit values ignore `gold_cabinet`.
  - `banner_logic`: theme packages and PRAY FOR ALL on 6..=8 (the eras unchanged).
  - `sound::rules`: the four R11 crowd branches, cheer / boo guarding, callouts as skins 4–5,
    `all_cues` ⊆ the bank manifest.
  - `policy`: `dance_message_vN` per theme.
- **Integration.**
  - The intro's World-panel dismissal already covers songs without a legacy panel, which is still
    every theme song until Step 4.
  - The banner host, the AFP sound route and `call_voice` are unchanged apart from the widened
    ranges.
  - The cabinet helper replaces the private resolver in `debug_ui.rs` (no behaviour change there).
- **Demo.**
  - **Intro and end.** Each theme plays its own READY (with `vo_ingame_ready`) and ends on its
    CLEARED / FAILED shutter. Tohoku EVOLVED shows PRAY FOR ALL on every theme.
  - **Sound.** The announcer and crowd follow A3 (cheer at a high gauge, boo near failing).
  - **AUTO:**
    - a DDR A song → DDR A;
    - an A20 / A20 PLUS / A3 song → White on a white cabinet and Gold with the SMX GOLD force;
    - a 2013 / 2014 song → `2013-2014`;
    - a World song → stock.
  - **Log.** `AUTO series N` in the arm INFO.

Step 4: Stage panel: theme variant and A3's skin-0 fill

- **Objective.** A3's skin-0 stage panel before every theme song (R4, R5). The score sets stay
  hidden in this step.
- **Guidance** (§4.9):
  - `panel_logic::Variant`, `variant(skin)`, `root_package(skin)`, `theme_stage_texture`,
    `special_stage`;
  - `panel.rs`:
    - static root CStrs for the row patch;
    - the theme session: no era packages, no cut-in, no `common_shutter_vN` ticket;
    - variant-aware adoption hides; the theme fill;
    - `stage_mc = choice_stage_usr` with `voice_label = 0`;
  - `arm` probes the theme's own root package.
  - The pending-banner check (World still holding `common_shutter000N`) applies only to eras.
- **Tests** (`panel_logic`):
  - `variant` / `root_package` per skin;
  - `theme_stage_texture` over stages 0..4, the final override, the extra stage, special stages
    (override index, past `max + 1`), course / event;
  - `stage_voice` for themes;
  - the machine's action sequence for a theme session (no cut-in: `Adopt, Fill → SetJacket →
    Showing`, voice due on the first Showing frame).
- **Integration.**
  - The hosting, row patch, READY dismissal, dwell skip and release queue are the eras' machinery.
  - `panel::handles_dismissal` now covers theme songs, so the intro stands its World-panel
    dismissal down, as for the eras.
- **Demo.**
  - **Each theme panel** (DDR A blue, White teal, Gold purple) shows:
    - the band `1st` / `2nd` / `3rd` / `4th` / `FINAL` / `EXTRA` matching the stage;
    - the song jacket in the root's frame;
    - the `vo_stage_*` call at once;
    - no cut-in, even with Era Cut-In ON (eras still cut in);
    - dismissal at READY.
  - **AS1 re-check.** Back-to-back theme songs (Gold → White → DDR A) and an era song between them
    show the right band art every time.
  - **Stability.** Quick restart and a song-select back-out after confirming stay clean (no stuck
    shutter, no WARN spam).

Step 5: Stage-panel score sets (RE first)

- **Objective.** A3's per-player high-score and target sets on the theme panel, from World's
  records, each field failing open (R6).
- **Guidance.**
  1. **RE task** (Appendix C rows 1–4). Record the results in the new module's `//!` doc and in a
     `docs/ddr_selection_theme_score_sets.md` note:
     - the best-record lookup anchor, either an AOB for `FUN_1801e2c40` or a derivation from the
       call in the rank getter via `selectmusic_record_panel_refresh`;
     - the target resolver's calling convention and record shape, plus the target name / area
       getters;
     - whether World keeps a per-player area, and its region / language setting;
     - A3's no-record display and `FUN_1800ffe00`'s handling of unmapped characters and unused
       slots.
  2. **Signatures:** `ddr_sel_best_record`, plus the target getters the RE pins, as optional
     derivations in `src/core/signatures.rs`. Then the sweep and `shape_diff.py`.
  3. **`score_set_logic.rs`** (pure, §4.10, §5.4) and **`score_set.rs`** (engine: reads, `Record`
     building, texture writes).
  4. **Theme session tickets:** `common_texture_v0`, and `common_area_lang_<lang>_vN` only if the
     area resolves.
  5. **Panel:** the theme fill calls the score-set fill; the eras keep hiding `pN_score_set_mc`.
- **Tests** (`score_set_logic`):
  - visibility;
  - difficulty textures 0..4;
  - the glyph map (letters, digits, every symbol, unmapped characters, short names);
  - 7-digit leading-zero hiding, incl. 0 and 1,000,000;
  - rank 0..15 and none (≥ 16 / `0x11`);
  - clear kind → FC mark (7..=10, and 0..6 hidden);
  - the target's hidden rules (type −1, event modes 1 / 2) and type 0 = own record;
  - a missing field hides only itself.
- **Integration.**
  - Runs post-original in the same ShutterActor update as World's kind-3 fill (the same data).
  - Its tickets join the session's release queue (layer before package).
  - A missing signature hides the affected fields with one WARN.
- **Demo.**
  - **A charted, full-combo'd song** on a theme panel shows the difficulty, the dancer name in
    A3's glyphs, the 7-digit best score, the rank and the right FC mark.
  - **A never-played chart** shows A3's empty record.
  - **Targets.** With TARGET set (own best / a rival / machine best), the target set shows; with
    TARGET OFF it is hidden.
  - **Versus** fills both sides. **Guest** shows `PLAYER1` / `PLAYER2` glyphs, or whatever World's
    rule yields.
  - **Area** shows only if the RE found one.

Step 6: Gameplay player name (RE first)

- **Objective.** The dancer name in each theme difficulty frame, A3's way (R9).
- **Guidance.**
  1. **RE task** (Appendix C row 5):
     - World's counterpart of A3's gameplay 2D render list `*(scene_manager + 0xC8)` and the node
       key `wrapper+0xC`;
     - World's font-residency getter (A3 `FUN_18014f310`) and whether font 6 is resident during
       gameplay;
     - World's equivalent of A3's `*PlayerWork+1` (the x-scale choice).

     Record the results in `score_name.rs`'s `//!` doc. Add signatures only if needed, then sweep.
  2. **`widget_renderer::create_text_widget_with_font(font, WidgetStyle)`** and the narrow
     `TextWidget` setters (§4.11). `create_text_widget()` is unchanged.
  3. **`score_name_logic.rs`** (pure) and **`score_name.rs`**:
     - two lazily created, reused widgets via `run_on_render_thread`;
     - binding from the `name_usr` placeholder, re-synced each frame while the clip's layer lives;
     - the visibility window (§6.2 decides between the RE's render list and the READY-to-shutter
       fallback);
     - hidden at the GAMEPLAY exit, disarm and disable.
  4. **`score.rs`:** the theme init POST hands the difficulty clip to `score_name` and hides the
     placeholder.
- **Tests** (`score_name_logic`):
  - the D22 text rule (profile, entered guest P1 / P2, not entered, the bot side's `PlayerWork`
    as-is);
  - binding maths against hand-computed A3 values (centre, box, `valign 3` shift);
  - style constants bit-exact (colour from `0xFFFFEB08`; scale 0.928 / 1.28 × 0.64).
- **Integration.**
  - The only change to the shared service is the added constructor.
  - The widgets are hidden outside theme gameplay, so no other scene ever shows them.
  - World's own `dance_name` element stays hidden (markers `HIDDEN_KEYS`).
- **Demo.**
  - Each theme shows the yellow dancer name centred in the difficulty frame, both sides in versus.
  - A guest shows `PLAYER1` / `PLAYER2`.
  - Reverse scroll follows the reverse frame.
  - The name never draws over the stage panel or the end shutter.
  - Eras and stock songs show no name.
  - After 20+ consecutive songs no WARN about render-list nodes appears (the widgets are reused).

Step 7: Danger on doubles (scoped patch)

- **Objective.** `danger_double` on doubles for the themes; the eras keep `danger_single` (R10).
- **Guidance** (§4.12):
  - signature `ddr_sel_danger_double_skip` (optional member of the DDR SELECTION derivations; stock
    shape verified at init);
  - `danger.rs` `sync(want)`;
  - `package_helper.rs` calls it for every `dance_danger` request (the theme decision ⇒ `true`;
    stock / era ⇒ `false`);
  - restore at disarm and disable;
  - a pure `policy::wants_danger_doubles(decision)` helper for the host test.
- **Tests:**
  - `policy::wants_danger_doubles` true only for theme `dance_danger` decisions;
  - the signature sweep green on all five builds, with the per-build offsets matching §4.12;
  - `shape_diff.py` confirms `75 07` at the patch offset on every build.
- **Integration.** Mirrors the stage-frame / gauge / song-info patch scoping. A missing site leaves
  the Step 1 behaviour (single-lane on doubles) with one WARN.
- **Demo.**
  - Doubles on each theme: the full-width danger flash when the gauge drops.
  - Singles unchanged.
  - A doubles era song right after a theme doubles song shows the era's single-lane flash (patch
    restored).
  - The log shows apply / restore once per song.

Step 8: S-Marvelous on the themes

- **Objective.** Theme-styled S-Marvelous (R13).
- **Guidance** (§4.13):
  - **`s_marvelous/targets.rs`:** `LEGACY_SKINS` 1..=8, `u16` masks and `skin_bit`, the grade-sheet
    skins, tex-number texture names, `art_set`.
  - **`s_marvelous/assets.rs`:** theme targets named through `ddr_selection::policy::package_name`;
    set 7 staged into both the `_v1` and `_v2` IFS mod paths.
  - **Shape count.** Count the marvelous shapes in the theme `dance_fullcombo0000_vN` clips and set
    `fc_expected_shapes`.
  - **Generator.** Extend `scripts/gen_ddr_selection_smarv_art.py` (sets 6 and 7, donors, the
    `_v1` ≡ `_v2` pixel guard), then generate `data_mods/ddr_selection/s_marvelous/{6,7}/`.
- **Tests:**
  - `validate_s_marvelous.sh`: `LEGACY_SKINS`, `skin_bit(8)`, `art_set`, theme texture names, the
    grade-sheet rule, `target_skin(true, 6..=8)`;
  - the generator's own `--check-world` still passes, and the new `_v1` ≡ `_v2` guard passes.
- **Integration.**
  - DDR SELECTION's `legacy_package` / `armed_skin` seam is unchanged.
  - `on_ddr_selection_enabled` stages the new sets.
  - The deploy must copy `data_mods/ddr_selection/s_marvelous/`.
- **Demo.** With S-Marvelous on, each theme shows:
  - a violet S-MARVELOUS word (both Judgement Color settings);
  - a violet S-MFC splash on an all-S-Marvelous full combo;
  - a violet combo while the combo is all S-Marvelous;
  - a plain Marvelous where art is absent (delete one set to check the stand-down).

Step 9: Release integration and the cabinet matrix

- **Objective.** Ship-ready documentation and the full cabinet pass (R14, §7.3).
- **Guidance.**
  - **README:** the DDR SELECTION section and the mods-table row (new values; AUTO reaching DDR A
    and A3 White / Gold by cabinet; the theme panel and name).
  - **Option preview:** `scripts/option_strings.py` `ddr_selection` text in en / ja / ko,
    regenerated via `scripts/gen_option_labels.py` (never hand-edit the PNGs).
  - **Docs:** the module `//!` docs (`mod.rs` Surfaces / Decision, `options.rs` value list); the
    status line of `docs/ddr_selection_a3_themes_research.md`.
  - **Knowledge base:** `.agents/summary` component rows. The maintainer may prefer a
    codebase-summary refresh instead of a hand edit (the summary is generated).
- **Tests.** The whole host suite and the signature sweep green. The full readiness gate.
- **Integration.** Deploy the DLL plus `data_mods/` (the option preview texture and the S-Marvelous
  art).
- **Demo.** The §7.3 matrix:
  - each theme × {1P, versus, bot} × {normal, reverse, doubles} × {quick restart, quick fail} ×
    {stock → theme → era → stock};
  - S-Marvelous, Center Arrows, overlay element styling and playfield styling together;
  - AUTO on both cabinet looks;
  - Tohoku EVOLVED;
  - guest / profile;
  - charts with and without records / targets.

  Results go in `progress.md`'s deploy log. The feature is complete when the matrix shows no
  regressions on eras or stock songs.
