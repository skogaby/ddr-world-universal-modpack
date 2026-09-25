# DDR SELECTION — Implementation Plan

Status: Approved 2026-09-22 (user approved the plan request together with
autonomous execution up to the first cabinet-test gate)

Design: `.agents/planning/2026-09-22-ddr-selection/design/detailed-design.md`.
Every step ends with `cargo check` clean, `cargo fmt`, `./build.sh` clean,
the host validator green, and — when `signatures.rs` or a consumer-side
offset changed — `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`
green plus `shape_diff.py` review for every `match+N` reader.

## Checklist

- [x] Step 1: Package-helper restore + skin write + dev knob (P0 spike, class-A packages) — cabinet-proven 2026-09-22
- [x] Step 2: Options row, trigger, AUTO, governance, import scripts, docs (P1) — row cabinet-proven 2026-09-22; import scripts tested (bash + the bottle's cmd)
- [x] Step 3: Era sound bank + AFP cue routing (P2a) — cabinet-proven 2026-09-23 (`XAC_full_combo2` routed; World's `se_game_fullcombo` doubling fixed by `sound/code_se.rs`). The in-lane game-over clip (and its `Plate_spin3_st`) never appears on a fail even in stock World — moved to Step 6
- [x] Step 4: Legacy READY / HERE WE GO + World-intro suppression (P2b) — cabinet-proven 2026-09-23 (run #2). The READY? dwell skip is implemented but OFF until Step 5 (see progress.md)
- [x] Step 5: Legacy stage panel, cut-in, stage voices (P2c) — cabinet-proven 2026-09-23 (run #2, rev 2: just-in-time hosting at the song-select request, adoption-gated suppressions, old builds hosted); + GLOBAL SETTINGS "Era Cut-In" ON/OFF toggle (maintainer request). Score-set contents and the SD root are follow-ups (see progress.md)
- [x] Step 6: Legacy end banners + `_sel` movies (P2d) — cabinet-proven 2026-09-24 (runs #1–#3: CLEARED / FAILED on every skin, PRAY FOR ALL on skin 4 only — skin 1's clip has no art, maintainer decision; `_sel` movies incl. the 11 movie-less songs and the stage monitors; the stage-panel `afp_mc_get_param` log spam found in run #1 fixed). The in-lane game over needs no work (World shows it on EXTRA-stage fails only, A3 never on a normal stage — research `end-banners-sel-movies.md` §3)
- [x] Step 7: Legacy element positions, stage frame, danger 3–5, World-only HUD hiding (P3a)
- [x] Step 8: Legacy life gauge (P3b)
- [ ] Step 9: Legacy combo (P3c)
- [ ] Step 10: Legacy score + song info (P3d)
- [ ] Step 11: A3 announcer and crowd rules (P4)
- [ ] Step 12: 1st-5th option forcing (P5)
- [ ] Step 13: S-Marvelous legacy art (P6)
- [ ] Step 14: Release integration

---

Step 1: Package-helper restore + skin write + dev knob (P0 spike, class-A packages)

- **Objective.** Prove the core mechanism on a cabinet: World loads the
  legacy `dance_{judge,fast_slow,fullcombo,game_over}000N` (all skins) and
  `dance_danger000N` (skins 1–2) through the restored `%04d` append, with
  `GameWork+0xA8` holding the skin.
- **Guidance.** New signatures + all-or-nothing derivation for the
  `LayoutActor` per-package helper (callees assign / probe / record insert /
  list push / free / snprintf, the `"bm2d"` dir string, record/list offsets,
  identity gates on `"dance_message"` and `"%04d"`) and for
  `gamework_skin_off`. `src/mods/ddr_selection/` skeleton (`Mod` impl, id
  `ddr-selection`, `DEFAULT_OFF_MODS`), pure `policy.rs`, `package_helper.rs`
  detour (Stock ⇒ original with skin 0; Legacy ⇒ A3 append), arm/disarm scene
  callback (arm 25 → 26, `GameWork+0xA8` cleared on leaving {26..30}), dev knob
  `DDR_SELECTION_FORCE=<1..5>` as the only trigger, per-song INFO, the S-Marv
  bridge (`legacy_package`) wired into `flash.rs`/`splash.rs` so the spike
  cannot fire `in_smarvelous` on a legacy clip. Offline scan for any other
  reader of `LayoutActor+0x190`.
- **Tests.** `scripts/validate_ddr_selection.sh` (new temp-crate harness):
  policy invariants (no `0000` names, `dance_common` never legacy, per-skin
  availability), decision table for every base × skin. Signature sweep across
  the four builds.
- **Integration.** Registered in `src/lib.rs`; no other mod changes except the
  S-Marv stand-down query.
- **Demo.** Cabinet with developer mode + `DDR_SELECTION_FORCE=1`: 1st-5th
  judgement words, FAST/SLOW, full-combo splash, game over and centred danger
  render; song info panel and option icons hidden (skin-1 gates); consecutive
  forced → unforced → forced songs, quick restart and quick fail all show the
  right art; no WARN.

Step 2: Options row, trigger, AUTO, governance, import scripts, docs (P1)

- **Objective.** Players pick OFF / AUTO / an era in the in-game OPTIONS menu.
- **Guidance.** Pure `trigger.rs` (governing side, bot exclusion, course /
  event / attract exclusion, AUTO table), `music_series_vslot` publication
  from `flare_skill_classifier`, series read through `find_music_by_mcode`;
  Dynamic-format scalar row `ddr_selection` (`PersistMode::Local`,
  `.in_game_only()`, load clamp, Duplicate re-enable path), `versus_mirror`
  registration, per-side atomics; `seop_item_ddr_selection` strings in
  `scripts/option_strings.py` + regenerated eng/jpn/kor sets;
  `option_menu_settings` placement. `scripts/ddr_selection/import_a3_assets.{sh,bat}`
  + `a3_assets.manifest`. `docs/ddr_selection_research.md` corrections
  addendum; README section; AGENTS.md row.
- **Tests.** Trigger table (every row value × series × modes × versus/bot);
  import script against both installs (copies one file, second run no-op, bad
  paths refused).
- **Integration.** The dev knob stays as an override; the trigger replaces it
  as the default source.
- **Demo.** Cabinet: AUTO on a 1st MIX song shows skin 1, on a World song
  stock; an explicit era applies to a World song; versus follows P1.

Step 3: Era sound bank + AFP cue routing (P2a)

- **Objective.** A mod-owned `dsel` bank with every era cue is registered and
  playable; legacy clips' embedded `sound_play` cues resolve.
- **Guidance.** Focused RE first: World's AFP sound callback name resolution.
  `se_bank_synth` N-entry XWB writer + variation-capable XSB writer; pure
  `sound/bank_build.rs` (cue manifest from design §5.4, reads the `_n` files);
  background build at enable, registration via `game_audio::register_bank`
  slot 4 at the first arm; `game_audio::play_cue` / `is_cue_playing`; routing
  tap only if the RE shows it is needed.
- **Tests.** Builder round-trip (parse the built pair back), offline leg
  against `$DDR_WORLD_INSTALL`, cue-set completeness against every legacy
  clip's `sound_play` names.
- **Integration.** Step 1's legacy full-combo / game-over clips now play
  their embedded era SEs.
- **Demo.** Cabinet: forced skin 1 full combo plays `XAC_full_combo2`; a dev
  command plays each bank cue once without WARN.

Step 4: Legacy READY / HERE WE GO + World-intro suppression (P2b)

- **Objective.** A3's READY! / HERE WE GO!! clips play at the chart-derived
  ticks for every legacy skin.
- **Guidance.** Tap on the ControlMessageActor sender (`0x1047/48/49`), DLL
  layers from `dance_message000N` (policy entry), lesson-song variant, skin-1
  HERE voice; READY sends `0x100c` to the shutter (reusing quick-restart's
  drain unblock); suppress `vo_ingame_ready` and clear the 5.0 s dwell while
  armed; layers destroyed before the package is released.
- **Tests.** Pure message → action mapping (duplicate triggers, lesson song,
  final-stage voice).
- **Integration.** World's stage panel is dismissed at READY instead of
  parking; quick restart / quick fail still pass their shutter gates.
- **Demo.** Cabinet: every skin shows its READY / HERE WE GO with the era
  voice; restart and fail mid-intro recover.

Step 5: Legacy stage panel, cut-in, stage voices (P2c)

- **Objective.** A3's pre-song stage panel and era cut-in replace World's
  kind-3 panel for legacy skins.
- **Guidance.** Turn `intro::SKIP_WORLD_DWELL` on here (Step 4 left World's
  5 s dwell stock because World's panel still showed). Focused RE first
  pinning the kind-art loader, kind-3 fill and state seams on all four builds; then the kind-3 host (root from
  `common_choice_v2` by full name, legacy sub-clips, jacket rules incl. the
  skin-3 SN2 banner, available score fields), `frame_out` park, cut-in state
  with `sele_*` SE and skip, stage-voice detour.
- **Tests.** Pure jacket/banner/voice selection per skin and stage; shutter
  state-transition table.
- **Integration.** Step 4's READY dismiss closes the legacy panel.
- **Rev 2 (after cabinet run #1).** The stage panel is requested at the
  song-select confirm, so hosting happens just-in-time in the ShutterActor
  update that loads the stage art (pre-original; the row is patched for that
  one update); World's stage voice / READY? dwell / the intro's World-art
  dismissal stand down only once A3's root is adopted; the old layout's
  un-null-checked `jacket_usr` SetVisible is NOPed while hosted, so the
  panel reaches the 20250805 floor. RE: `research/stage-panel.md` §1.4–§1.5.
- **Demo.** Cabinet: every skin's panel, cut-in and stage call; quick restart
  and quick fail from every intro state.

Step 6: Legacy end banners + `_sel` movies (P2d)

- **Objective.** Legacy CLEARED / FAILED / PRAY FOR ALL banners; `_sel`
  movies on the 18 songs that have them.
- **As built (2026-09-24).** One-update CLEARED / FAILED row patch on the
  Step 5 ShutterActor detour (`banner.rs`, World's named-package path loads
  A3's root; World fills nothing in kinds 4/5) + our overlay layer from
  World's copy of `common_shutter000N`; `_sel` = one detour on
  `SceneManageActor::onInitialize` (`movie_sel.rs`: scoped "has a movie"
  byte for the 11 movie-less songs, `+0x149` on the new MovieActor). Any
  legacy-era song (no folder), all 18 songs, VIDEO SIZE OFF respected. RE +
  as-built: `research/end-banners-sel-movies.md` §7.
- **Guidance.** Kind 4/5 overrides with a DLL overlay layer and guarded World
  fills; MovieActor `+0x149` flag write post-ctor. Also the in-lane game-over
  clip (`dance_game_over000N` `game_over`, frame-1 `Plate_spin3_st`): World's
  DangerActor shows it on msg 0x103c, yet it is never visible on a fail even in
  stock World (maintainer, 2026-09-23) — find out whether the FAILED shutter
  covers it or it never advances, and how A3 sequenced clip vs banner, before
  deciding how the legacy fail sequence shows it.
- **Tests.** Pure banner selection; `_sel` existence rule.
- **Integration.** The shutter overrides are removed once the shutter is idle
  after the window.
- **Demo.** Cabinet: clear / fail / quick-fail banners per skin (fail: the
  legacy in-lane game over + `Plate_spin3_st` in the `legacy clip sounds`
  line); a 1st MIX song plays its `_sel` movie.

Step 7: Legacy element positions, stage frame, danger 3–5, World-only HUD hiding (P3a)

- **Objective.** HUD elements sit where A3 put them; the legacy stage frame
  shows; danger for skins 3–5 goes live.
- **Guidance.** Promote center-arrows' layout builder / marker setter /
  song-info card detours to `services/hud_layout_hooks` (behaviour-identical);
  marker post-pass reading the legacy root by explicit name; stage-frame
  detours + `dance_stage → dance_stage_frame` policy entry; BPM and name
  hiding.
- **Tests.** Pure marker-name mapping (nesting, reverse difficulty, World-only
  keys untouched); center-arrows regression via its own validator.
- **Integration.** Center-arrows' lane shift runs after the post-pass.
- **Demo.** Cabinet: positions per skin match A3 reference captures;
  center-arrows on and off.
- **As built (2026-09-24).** `research/hud-layout-stage-frame.md` §11. Stage
  frame = two checked code patches (export LEA + texture prefix) scoped to the
  `LayoutActor`'s legacy `dance_stage` record instead of detours; BPM / name
  hidden by parking their markers off screen (no detour); keys whose World
  art is replaced only in Steps 8–10 (score, difficulty, gauge, combo,
  song info) stay World's until their packages turn legacy; the song-info
  card detour stays in center_arrows_single until Step 10; no center-arrows
  validator exists (regression = cabinet).

Step 8: Legacy life gauge (P3b)

- **Objective.** Legacy gauges for every gauge type.
- **Guidance.** Clip-create export alias (promoting overlay-element-styling's
  capture to a shared dispatcher), P2 mirror, segmented / continuous fill
  port, `dance_gauge` policy entry.
- **Tests.** Pure cell math against A3 constants; alias map.
- **Integration.** `song_reset` gauge restore unchanged.
- **Demo.** Cabinet: NORMAL, LIFE4, RISKY, FLARE, FLOATING FLARE, GRADE per
  skin, 1P and versus.
- **As built (2026-09-24).** `research/legacy-gauge.md` §5. Export alias =
  a checked patch of each gauge init's clip-create LEA (→ `00_dance_gauge`)
  scoped to the legacy `dance_gauge` records, not a CMovieClip::Create
  dispatcher (overlay-element-styling's capture untouched); 2P mirror and the
  skin-3 LIFE intro = post-original init detours; fills = a full-replacement
  detour on World's fill. HD constants only.

Step 9: Legacy combo (P3c)

- **Objective.** A3 combo behaviour on legacy skins.
- **Guidance.** Promote S-Marv's combo-refresh detour to
  `services/combo_hooks`; A3 ComboActor port in World's
  init/msg/update/finalize; overlay-element-styling classifier learns the
  legacy clip; blanked `dance_combo0005` detection (skin 5 stock + WARN without
  the A3 import).
- **Tests.** Pure growth / cell / centring math.
- **Integration.** S-Marv combo repaint unchanged on stock songs.
- **Demo.** Cabinet: combo per skin incl. 1000+; skin 5 with and without the
  import.

Step 10: Legacy score + song info (P3d)

- **Objective.** A3 score / difficulty / EX display and song-info panels.
- **Guidance.** ScoreActor init / digits / difficulty ports; skin-2 band and
  skins 3–5 A3 `0000` panel via the shared song-info hook.
- **Tests.** Pure digit / smoothing / level-texture mapping.
- **Integration.** `song_reset` score sentinel honoured.
- **Demo.** Cabinet: score, EX mode, every difficulty, reverse scroll, per
  skin.

Step 11: A3 announcer and crowd rules (P4)

- **Objective.** A3's per-skin announcer and crowd behaviour.
- **Guidance.** Pure `sound/rules.rs`; `CallVoiceActor::onUpdate` detour via
  the RTTI vtable; skins 4–5 A3 `vo_ingame_*`.
- **Tests.** Fixture timelines → expected cue sequences (guard, mute, regain,
  milestones).
- **Integration.** Uses Step 3's bank; World announcer when the bank is
  missing.
- **Demo.** Cabinet: a long song per skin, voice mute songs, versus.

Step 12: 1st-5th option forcing (P5)

- **Objective.** A3's forced classic options for skin 1.
- **Guidance.** Field-offset publication; snapshot / force / re-assert /
  restore; save-node rewrite fallback; bot ordering.
- **Tests.** Pure value table against World enum orders; lifecycle transitions.
- **Integration.** Real-speed and per-song offsets unaffected.
- **Demo.** Cabinet: forced options visible in play, profile unchanged after
  logout, bot game.

Step 13: S-Marvelous legacy art (P6)

- **Objective.** S-Marvelous presentation on every legacy skin.
- **Guidance.** Draft generator script, hand-finished art, S-Marv recipes
  generalised to legacy templates, bridge flips from stand-down to enable per
  available art.
- **Tests.** S-Marv host legs extended to the legacy templates.
- **Integration.** Stock-song S-Marv unchanged.
- **Demo.** Cabinet: S-Marv word, S-MFC splash and combo treatment per skin.

Step 14: Release integration

- **Objective.** Ship.
- **Guidance.** Full cabinet matrix (design §7), remove from
  `DEFAULT_OFF_MODS` on the maintainer's call, README / AGENTS.md final pass,
  release archive check (import scripts included).
- **Tests.** Every validator + sweep green.
- **Demo.** Stock cabinet install + import script + AUTO on a mixed setlist.
