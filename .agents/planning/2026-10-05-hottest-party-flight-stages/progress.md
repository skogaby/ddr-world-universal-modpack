# Progress — HOTTEST PARTY flight stages (take-off + tunnel flight)

Updated: 2026-10-05 (session 3)
Status: Step 7 of 7 — done (all uncommitted); FX F1–F6 implemented and gate-green; NOTHING
cabinet-tested yet (flight or FX).
NEXT ACTION: cabinet deploy of the DLL + the six flight stage folders (`data_mods/custom_models/
stages/HOTTEST PARTY {2,3,4}/Stage {102,103,201,205,206,301}` — they now carry `flight_fx/` + the
`fx_<key>_*` pool models; a DLL-only deploy flies WITHOUT effects) and walk the watch-list in
"Deploy & test log".
Resume protocol: this file → `docs/wii_ddr_zan_effects_research.md` (effects RE) →
`docs/wii_ddr_hottest_party_2_3_research.md` §7.5 (flight stages) →
docstrings of `port_character_hottest2.py` / `port_stage_hottest2.py` (`FLIGHT_*`) → `//!` headers of
`src/mods/background_dancers/{mod,selection,pick,schedule,director,director_math}.rs`.

## Session 2 findings (2026-10-05)
- No REL modules on the zan discs: FuruFuru Party / MUSIC FIT / HP4 / HP5 have only sys/main.dol and
  their DOLs carry no `.rel` / OSLink / objdll strings (HP1's Hudson engine does). The dumper writes
  every FST entry. HP4 / HP5 DOLs are in Ghidra as `ddr_hottest_party_{4,5}_main.dol` (the dump
  files were renamed; `extract_wii_ddr_data.main_dol` falls back to any `sys/*.dol`).
- HP4's intro script = `FUN_80106298` (r2 = 0x802bf5a0): same as MUSIC FIT except the sky fades
  over f 390..420 (not 360..420), a lighting-preset switch at f 390 (`FUN_801030bc`, lights not
  ported) and sounds 0x115 / 0x116. `intro_look` uses MUSIC FIT's timings for HP4 STG301 too (0.5 s
  difference on the sky fade; open).
- HP4 / HP5 ship the flyer effects as `character/CHR_EFF.bin` (boss_ddr3.TEB, 27 effects: MUSIC
  FIT's 24 + 3). The ribbon drift (manager +0x298) is never written; the trail comes from the world scroll (§3.4 of the effects doc).
- `scripts/teb_dump.py` (parser + reference simulator, uncommitted, no tests yet) + the Blender
  check `tools/blender_ddr_addon/examples/preview_flight_fx.py -- <mode 0|2> <out> [frames]` render the effects on a
  ported dancer: orb ring, hand-orbiting stars with trails, sparkles; leap flash + foot sparks.
- "Tunnel invisible" was a PREVIEW artefact: `_unlit` made every material BLENDED, EEVEE sorted the
  opaque space sphere (fly_dec, 508 m) over the additive grid tube. Opaque parts now DITHERED. The
  port was right (grid tube = DRAW_B03_grid01, r 11–29 m, z ±338 m).
- MUSIC FIT main.dol flight intro (play-setup flag 0x200 = the song is a flight song): `FUN_8003729c`
  arms (stage objects = the stage file's zmbs in file order: 0 DRAW_STG201 tunnel, 1 BG_STG201,
  2 COL, 3 OBJA_Z_BG109 sky+sea, 4 OBJA_Z_BG201 plain space, 5 OBJA_Z_DRAW109 platform, 6 hole):
  0, 1, 6 hidden; 3, 4, 5 shown. `FUN_80037354` per frame f (60 fps): platform colour dims
  1 → 0.15 over f 60..210; f 180 stage effect (cat 5 = STG201_EFF, `FUN_800431a4`, at a COL node)
  + sound 0xf6/0xf7; hole fades in f 300..360 and starts its motion at 300; sky fades out f 360..420;
  f 544 char effects mode 2 (`FUN_8004b5a8(.., 2)`) = the leap; a stage-controller value (+0xea0,
  light level?) ramps 1 → 0.5 → 0.15 → 0.5 (not ported); the switch = end of the intro camera
  STG201_CAM00_01..03 (3+4+3 s = 600 f): 0, 1 shown, 3..6 hidden, char effects mode 0 (1 when
  play mode +0xe4 == 4). r2 = 0x8032d840 (SDA2) for the float constants.
- So the hole + BG201 are INTRO parts, BG_STG201 is a FLY part (the old split was wrong). Port now
  names parts by role (`pre_plat` / `pre_sky` / `pre_space` / `pre_hole` / `fly`), the DLL replays the
  script (`director_math::intro_look`, tint + clock), one-shot honoured on pre_ parts too.
- FX: zan `CzanEff` particle engine. Banks by category: 0 ?, 1 GAME_STG_EFF, 2 ?, 3 GAME_APL_EFF,
  4 ?, 5 STG201_EFF, 6 GAME_CHR_EFF (boss_ddr3.TEB: 24 effects = 3 modes × 4 players × {A, B}; player
  p mode m plays effects 8m+2p, 8m+2p+1 ×2 at dancer attach joints table DAT_80218b68 = modes 0/1:
  [0,1,2], mode 2: [5,3,4]; joint 5 position-only). Textures: per-player star colours (gold / blue /
  pink / green), rainbow strip, rings, flares. TEB = effects → node trees (type 0 root, 1 CzanEffPart
  emitter, 2 CzanEffMdl); part block = flag mask of sub-blocks (emitter, draw, child-chain trail,
  ribbon, key lists). Runtime: `FUN_8012b5b8` part update, `FUN_8012ce18` particle step,
  `FUN_8012ddf4` draw, `FUN_80131d08/80131764` ribbon. Effect API: `FUN_8012aca8` play,
  `FUN_8012af28` set_mtx, `FUN_8012aeec` set_f_draw, `FUN_8012ae74` stop.

## Goal (maintainer, 2026-10-05)
On a FLIGHT stage (HP2 STG102 / 103, HP3 STG201 / 205 / 206, HP4 STG301) the dancers take off
from the launch platform and fly through the tunnel for the rest of the song — whatever World song
plays (the stage selects the behaviour). Only flight-capable dancers appear there: a non-capable
pick is replaced by a random capable one; a capable pick is kept. Flight poses never appear on
normal stages or in the options-menu preview. ALSO (maintainer): the flight FX the games show — a
yellow light orb around the flyer, a rainbow trail behind, stars orbiting the orb leaving light
trails — must be ported FROM THE GAME'S OWN EFFECT ASSETS (no hand-authored stand-ins), before any
cabinet test.

## Design (implemented unless noted)
- Dancer data: flight songs (`zan_dump.piece_class` = 'flight' pieces: Hips near the origin, torso
  level) leave the dance library. Every zan dancer gets `pl_<key>/motion/flight/takeoff.anm` (the
  600-frame take-off, 1:1 = 10 s dance time, run + leap where authored) + FLIGHT_PER_DANCER (4)
  `motion/flight/fly_<song>_<n>.anm` (flight pieces ~8 bars, 1 bar per 120 frames of the piece,
  Hips lifted by HIPS_UNITS*GAME_SCALE = 0.97 m). Normal playlists read direct children of
  `motion/` only.
- Stage data: `mapset_<key>/flight.txt` (member `data/map/flight.txt`) marks a flight stage. Parts
  by role (main.dol's intro objects): `pre_plat_*` (OBJA_Z_DRAW109, dims 1 → 0.15 at 1–3.5 s),
  `pre_sky_*` (OBJA_Z_BG109, `_bg`, fades out at 6–7 s), `pre_space_*` (OBJA_Z_BG201, `_bg`),
  `pre_hole_*` (OBJA_Z_hole0n, fades in at 5–6 s, its one-shot opening clocked from 5 s) until the
  take-off ends; `fly_*` (DRAW_STG*, BG_STG* = `fly_bg`) after it on their own clock. A phased entry
  whose motion doesn't return to its first pose is a ONE-SHOT part (`.anm` flag 0, last key = end
  pose). HP2 STG102 / 103 (no platform): marker only.
- DLL: `DancerCandidate.flight` (+ `can_fly`, `takeoff_clips`, `flight_loops`),
  `StageCandidate.flight`, `PickSource::Flight`, `pick::fly_pick` + `FlightOutcome`,
  `Pick.flight`, `DanceSchedule::with_intro` / `intro_end`, `director_math::{part_phase,
  phase_clock, DEFAULT_TAKEOFF_S}`, `Session::{flight_stage, flight_switch}`,
  `session::flight_switch`, director phase gating + shadows off after the switch, lifecycle calls
  `fly_pick` after the pick (logs replacements), stage-only preview calls it too;
  `director_math::intro_look` replays the intro script on `pre_*` parts (instance tint + clock),
  one-shot clips honoured on pre_ and fly_ parts.

## Steps
1. DONE `zan_dump.piece_class` + test (`test_zan_formats.TestChoreography.test_piece_class`).
2. DONE character port; re-ported in the repo: HP1-3 137, HP4 32, HP5 18 dancers, each 12 dance
   clips + `motion/flight/takeoff.anm` + 4 `fly_*` (libraries: hp3 9 clips from 6 songs, hp4 8 from
   3, hp5 18 from 15 difficulty variants of HP4U 053–055).
3. DONE stage port (roles above); re-ported HP2 102 / 103, HP3 201 / 205 / 206, HP4 301; format check
   over all 158 HP1–HP5 stages 0 problems. Previews (`$TMPDIR/opencode/hpfix/fly_prev5.py`): platform
   → mouth in the sky → space → leap → grid tunnel with planets, as in the game.
4. DONE flight FX (session 3) — "FX implementation plan" F1–F6 below; RE + DLL record:
   `docs/wii_ddr_zan_effects_research.md` §5.
5. DONE DLL (+ `intro_look`, test `intro_script_dims_fades_and_opens`; 235 harness tests OK;
   `cargo fmt`, `cargo check`, `./build.sh` clean).
6. DONE re-ports (2, 3).
7. DONE docs: research 2/3 §7.5 + §8, README "Flight stages", module docs, both progress files.

## FX implementation plan (2026-10-05; resume here)
Reference: `scripts/teb_dump.py` (simulator), `docs/wii_ddr_zan_effects_research.md`.
- F1 DONE (session 3): `src/mods/background_dancers/flight_fx.rs` (harness `flight_fx`, 17 tests on a
  synthetic TEB; ad-hoc cross-check vs `teb_dump.simulate` on the real boss_ddr3 effects 0/1/2/7/16/17,
  90 frames with a moving attach + scroll: same counts, centres / colours / ribbon edges within 3e-5
  once the reference is rounded to f32). API: `parse_teb`, `EffectSim::{new, step, advance (fixed
  60 Hz catch-up, ≤ 8 steps), emit(FxSink), finished}`, `Affine::from_world_row`, `Camera::look_at_world`,
  `sprite_bone` / `ribbon_bone` / `ribbon_v_scale` / `additive_colour`. The scroll is applied as a
  −v·dt shift of all world-space state (ribbon points, anchors, prev emitter pos).
  WAS: — pure std-only (harness-mounted as
  `flight_fx` in `scripts/validate_background_dancers.sh`): TEB parser + the simulator, a port of
  `teb_dump.py` (f32, column-vector math, zan units, own small vec/mat types; LCG Rng). Output per
  frame: sprites {centre, axes[3], uv[4], rgba, tex, blend, depth} + ribbons {points (l, r), v,
  rgba, tex}. Scroll per `FLIGHT_SCROLL`. Tests on a synthetic TEB built in the test.
- F2 DONE ad hoc (not shipped): the F1 cross-check above.
- F3 DONE: `tools/blender_ddr_addon/examples/port_flight_fx.py` (plain python3, no Blender): per flight
  stage (any `mapset_*/flight.txt`) `flight_fx/{flight_fx.teb, flight_fx.txt}` + per player
  `fx_<key>_s<p>a` (101 quads, 6 groups) and `fx_<key>_r<p>{a,b}` (8 strips, 244 + 131 bones) with
  the textures `<key>fxNN.dds`; bank: MUSIC FIT GAME_CHR_EFF, HP4 CHR_EFF for Stage 301. Loading
  route: inside the stage arc (dirs not named `gm_<key>_*` are no parts; the engine converts every
  `.model` member on the stage load). Re-ported all six stages.
- F4 DONE: frame board 64 slots / 256 bones / 128 mat writes / 128 per-record colours
  (`publish_full`, `read_slot_into_item_raw`); `node_visit` copies straight into the item
  (`render_item::record_colours_raw`), no stack snapshot; test
  `record_colours_ride_the_slot_and_land_in_the_item`.
- F5 DONE: `InstanceKind::Fx { dancer, pool }` (after the shadows, no restyle / hulls); `Parsed::
  flight_fx` (`session::parse_flight_fx`, pool bone counts from the `.model`s), `ParsedDancer::
  fx_joints`, `Session::{fx, fx_leap, fx_camera}`; `director::tick_flight_fx` + Fx publishes;
  `produce(.., real_s, ..)` (music count seconds); the lifecycle runs `camera_tick` BEFORE the
  publish and stores `fx_camera`; build log `[fx] N record(s) -> materials [...] (identity)`.
- F6 DONE: gate (fmt / check / build.sh / harness 258 / py 72 incl. new `scripts/test_teb_dump.py`),
  docs (effects §4–§5, README, module docs), preview attach fix (`C^T·m`, was `C^T·m·C`).

## FX plan (for the maintainer's decision)
The orb / rainbow trail / orbiting stars are `boss_ddr3.TEB` effects run by the zan `CzanEff`
PARTICLE runtime (emitters with spawn rate, life, random spread, velocity / gravity, camera-facing
billboards, size / alpha / colour keys, flip-book UVs, child-chain trails and ribbon strips). There
is no geometry to port; the TEB is emitter parameters. A faithful port needs:
(a) the rest of the RE: spawn init, key-list evaluation (`FUN_80132120` / `FUN_8013212c`), draw /
    billboard modes (`FUN_8012ddf4`), ribbons (`FUN_80131xxx`), effect root (`FUN_8012f458` /
    `FUN_8012f788`), the RNG and the time scale;
(b) a Rust port of the runtime (pure, harness-tested against hand-checked frames) reading the TEB +
    TPL shipped as content (e.g. `pl_<key>`-independent `fx/` arc made by a port script);
(c) a World draw path the DLL does not have yet: per-frame sprites and ribbons. Workable with the
    existing scene3d pieces — a pooled quad model per emitter (one bone per particle, ≤ 64 / slot),
    bones set per frame to camera-facing matrices (the DLL owns the camera), per-particle colour /
    alpha through per-quad materials (`frame_board` material params, today ≤ 48 floats / slot ⇒
    raise it) — plus frame-board slots (32 total; a flight scene already uses ~26).
Estimate: several sessions (RE ~1, runtime ~1–2, renderer + content ~1–2) + cabinet iterations.
An offline bake along each flight clip is possible but camera-independent (no true billboards) —
an approximation, so not offered as the port.

## Deploy & test log
- (none yet). Watch-list for the first flight deploy:
  - take-off timing; the switch at the take-off's end; the intro fades (HP4 STG301 sky 0.5 s early);
  - `flight stage -- dancer N (x) cannot fly: y flies instead` when a non-flyer is picked;
  - no flight poses on normal stages / in the options preview; culled stages gone; delete
    `HOTTEST PARTY 5/Stage 12` on the cabinet;
  - FX: `flight effects -- k of n dancer(s), m pool instance(s), leap at 9.07 s` INFO; every
    `fx_<key>_*` built (`built ... N fx`), `[fx] ... (identity)` for the sprite pools (if NOT
    identity, quads are mis-coloured: fix the record mapping); textures `<key>fxNN` resolved;
    the leap burst at ~9 s at the hips / feet; after the switch the orb ring, the rainbow streak
    behind (20 m), hand stars with white trails, sparkles dropping behind; per-player colours
    (P1 gold, P2 blue, P3 pink, P4 green); billboards face the camera across cuts; no hitch;
  - frame-board growth (1.2 MB static): normal songs unchanged.

## Deviations & open questions
- FX: the hand stars' orbit plane depends on the joint frame (DLL: the World joint frame = zan's);
  compare with footage. Ribbon v spacing linear; Z-test-off parts honoured (mesh flag 0x20).
- The switch IS reversed now (end of the 600-frame intro camera = the take-off's end); the port
  switches at the take-off's end on dance time, so the script follows the song's tempo.
- Not ported from the intro: the light-level ramp (+0xea0), the f 180 stage effect + sound, the
  intro cameras as a fixed sequence (STG201_CAM00_01..03 are just three of the `_st` shots).
- FuruFuru Party's own switch for STG102 / 103 is not looked at (marker only).
- HP4 STG301 is assumed to run MUSIC FIT's script (HP4 main.dol not imported).
- The override draws from the global capable pool (not the side's DANCER SOURCE).
- Stage main cameras of STG201 are 10–40 m establishing shots; the flyer is small in them.

## Key facts for a cold resume
- Flight songs: MUSIC FIT 046–049, 051; HP4 053–055 (HP5 re-ships as `DANCE_HP4U_05x_*`); HP2 049.
  The same 600-frame take-off piece in all MUSIC FIT / HP4 ones.
- Effect files: MUSIC FIT `stage/STG201_EFF.bin` (/stage_effects01.teb + .tpl, loaded when a
  play-setup flag 0x200 is set, `FUN_800b44d8`), `game/GAME_CHR_EFF.bin` (boss_ddr3.TEB/.tpl),
  `game/GAME_STG_EFF.bin` (ddr3_stage.TEB/.tpl + smoke model), `game/GAME_APL_EFF.bin`
  (concent.TEB/.tpl); FuruFuru Party / HP4 / HP5 have their own GAME_*_EFF.bin.
