# Progress — HOTTEST PARTY flight stages (take-off + tunnel flight)

Updated: 2026-10-06 (session 6, after cabinet run #6)
Status: DONE — flight stages fully working on the cabinet (run #6): take-off, READY hold, FX,
steady tunnel scroll, clean skybox. All work uncommitted (maintainer commits).
NEXT ACTION: none open. The run-#4/#5 diagnostics (`FlightDiag`, `frame_board::{now_ns,
reader_stats}`, `render_item::material_param_raw`) were removed after run #6 (gate green). P3
(z-write off on the blended grid) NOT needed — the WRAP build shows no visible smoke occlusion.
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

## Session 4 (2026-10-05) — cabinet run #1 feedback → fixes
- Tunnel lines "shuffled back and forth": (a) `.sanm` keys of UV scrolls whose period is not a
  whole number of frames (STG201 grid v 40.x f, u 482.x f) interpolated backwards across the wrap —
  `port_stage_hottest2.offset_keys` now continues the slope and pairs keys at the wrap frame
  (generic; ALL HP stages re-ported); (b) `fly_*` parts ran on dance time — now the real clock from
  the switch (`director::produce`).
- Take-off = the Wii intro on the REAL clock (`flight_fx::flight_schedule_time`, lifecycle passes
  schedule time + real time; `Session::{schedule_time, dance_at_switch}`); filmed by the intro
  shots `<key>_intro01..03` (port: `is_intro_camera`; DLL: `selection::intro_cameras`,
  `Pick::camera_intro`, `ParsedCameras::intro`, `director::intro_camera`).
- Sky burst: MUSIC FIT stage effect at COL `EFF_04_01` (table entry 227, name "04_01"), HP4 at every
  `EFF_03_*` (FUN_8004d7bc), both (0, 740, 2730); port writes `flight_fx/stage_fx.teb` + pool
  `fx_<key>_xa` + layout `stage_effect 180 x y z 0`; DLL `flight_fx::StageFx`, `InstanceKind::StageFx`.
  Gravity (0x2) RE'd + ported (Rust + teb_dump; cross-check on the burst within 7e-4 units).
- Sound: SE_DDR_BOSS (MF 0xF6 / HP4 0x115) = one RSEQ note on BNK_SEDDR prg 10 = wave 10 (9.94 s);
  port writes `flight_fx/burst_44k_mono.pcm`, layout `stage_sound 180 burst_44k_mono.pcm`; DLL
  encodes a one-cue XACT pair (parse thread) and plays it once at intro 3.0 s via the new slot-less
  `game_audio::{register_one_shot_bank, play_one_shot, stop_one_shot}` (gameplay only).
- HP2 intro camera RE'd (session 4b): every camera directly in a stage's `/#1/` is the camera
  controller's intro list (FFP `FUN_8003d234`, MF `FUN_800472d0`); FFP plays it before the song
  (`FUN_800302e8` state 4 waits for it + 1 s). `is_intro_camera` now uses that rule; HP2 STG102 /
  103 re-ported (`hp2stage10x_intro01`, 3 s); `director::intro_camera` returns None after the last
  shot so the stage cycle takes over before the switch.
- READY delay NOT done (needs live RE of the DPS dwell / panel / graph enable; design noted).

## Session 5 (2026-10-05) — cabinet run #3 feedback → fixes
- Tunnel grid still stutters (back and forth + slow forward creep) at 120 Hz. Offline: the
  `.sanm` (fly_ble, 8 frames, v 0→1) samples monotonic and `core/anm/sample.rs` interpolates
  fractional frames; the grid texture (`grid01nuki`, 128 px) is ONE bright line + three fading
  trail lines 21 px apart, scrolled 1/8 tile per 60 Hz frame (0.76 line spacings: the trail lines
  alias backwards on any coarse step, the head reads forward). Suspect = the clock: the i32-ms
  music count, one update stale, possibly 0 / 16.7 ms steps at 120 Hz (a cursor-driven clock under
  CrossOver: 11.6 ms staircase). Tried: `clock::SmoothClock` (2nd-order tracker of the count in
  wall time) driving `real_s` — deployed in runs #4 / #5 with NO visible effect, REMOVED in session 6
  (the real clock is the raw count again; the DLL never alters the game's clock). Diagnostic (kept): `FlightDiag` (developer_mode) logs 48 frames after the switch + a 1 s summary (dt,
  count steps, real steps, the grid `offV`, frame-board publish/read counts via the new
  `frame_board::reader_stats`).
- No tunnel scroll during the take-off: ROOT CAUSE = the stage port baked skinned meshes rigid.
  The mouth's grid tube (`OBJA_Z_hole0n` / `DRAW_B03_grid02`) is skinned to `Dummy_tube_*` and
  stretches from the mouth back through the origin around the platform over the opening; the
  flight tube (`DRAW_B03_grid01`) is skinned to `Dummy01..03` and BENDS (ends sway ~1400 units).
  `port_stage_hottest2`: `SKIN_ANCHOR` bones per skin joint (rest-world referenced, ≤ 4 weights;
  static skins baked deformed); verified by re-skinning the exported models offline (`.model` +
  `.anm` vs the zan deformation: identical extents at frames 0 / 60 / 134 / 300 and 500 / 1000).
  Only the flight tunnels and HP4 STG002 / 043 (filter props) carry skins on the four discs.
  Re-ported HP3 STG201 / 205 / 206, HP4 STG301 / 002 / 043, then `port_flight_fx.py`.
- READY hold implemented (`flight_hold_logic.rs` pure + 5 harness tests, `flight_hold.rs` engine,
  `lifecycle::hold_tick`): stock panel 4 s → panel layer hidden + dwell held at 4.0 → take-off on
  the real clock's free run → 0.30 s before its end panel shown + dwell seeded 1000 → song; the
  real clock is then `count + take-off`, `dance_at_switch = tau(0)`. Stands down for a foreign
  dwell seed (quick restart fresh DPS, DDR SELECTION legacy intro), a scene not ready in 4 s, no
  timer offset. Dev knob `DDR_DANCERS_NO_FLIGHT_HOLD`.

## Session 5b (2026-10-05) — cabinet run #4 feedback → fixes
- READY hold never engaged. A3 skin (log): `stood down: another driver seeds the READY? dwell`
  — DDR SELECTION's legacy panel seeds `DPS+0x130` to 1000 from the ShutterActor update (after our
  input-poll write, before the DPS update), every pre-song frame. World skin (no log kept):
  disassembly of 20260915 `DancePlaySequence::onUpdate` step 5 (+0x591E4..+0x592B5): the gate is
  `timer >= 5.0` ∧ shutter state ∈ {0, 4} ∧ bank prepared; ONLY THEN `0x1043`, the shutter reveal
  call (+0x5929A, when state 4), the SceneGraph enable bit (+0x592A9) and step++ — the graph is
  disabled for the whole dwell, so the hold's `scene_ready` (built ∧ graph enabled) never held and
  it stood down after its wait. Fixes: new `services::ready_hold` (the holder announces the next
  song's hold at the window start; DDR SELECTION's `seed_dwell` defers; quick restart still seeds
  and wins); the hold enables the SceneGraph itself (`scene_graph::set_enabled`) from the take-off
  start; it starts only once the panel settled (ShutterActor state 4 or no panel) — waits up to
  8 s; a one-shot `READY? step reached` state line.
- Stutter unchanged with the smoothed clock (since removed, session 6), and the run had no `flight diag` lines
  (developer_mode off) — the diagnostic now runs on every flight stage, bounded (48 frame lines +
  10 one-second summaries + every 10th after), and adds: when `visit(2)` copied the grid's slot
  (ms after the previous publish, shared `frame_board::now_ns`), and the item's material copy read
  back at the next frame's start (`render_item::material_param_raw`; a MISMATCH = another
  writer). Disassembly of the material-constant emit (`FUN_18026cce0` → `FUN_18026c440`, 20260825):
  the params are COPIED into the command stream (no deferred read of the item copy by the
  executor); a same-material state cache skips re-uploads within a pass.

## Session 6 (2026-10-06) — revert of the clock smoothing; RE before any new fix
- Maintainer's binding rules: nothing may alter the game's music or judgement clock (read-only use
  is fine); the READY hold's writes (`DPS+0x130`, the SceneGraph enable bit) are approved and
  kept; no new fix for the tunnel jitter or the skybox smears until the RE is written up and
  approved; the HP3 footage shows NO jitter / aliasing (a character flies through a tunnel passing
  grid lines), so any model of the Wii effect predicting aliasing is wrong.
- REVERTED: `clock::SmoothClock` + its constants and tests; `Window.song_clock` / `real_clock`
  (dance time `t` straight from the count via the tempo map; `real_s` = raw `mc_ms / 1000 +
  offset`); the held take-off's clock is wall time from an `Instant` stamp
  (`Window.takeoff_started`, `lifecycle::takeoff_elapsed`); `last_frame` / `dt` kept for the
  hold's wait budgets only. KEPT as read-only diagnostics: `FlightDiag` (on the raw count),
  `frame_board::{now_ns, reader_stats}`, `render_item::material_param_raw`. Docs updated
  (`director.rs::produce` comment, effects §5, research 2/3 §7.5).
- Run #5 data (120 Hz, CrossOver, `flight diag`): the game's frame dt and the count step alternate
  ~5 / ~12 ms (mean 8.3 ms); the frame board shows TWO `visit(2)` reads per publish (`board
  pub/read/lag (1, 2, 0)`, "120 frame(s) not read exactly once") — an earlier report of "read
  once" was wrong. Unexplained; part of the Task 2 RE (World's frame pacing under `fps_unlock`).
- Skybox smears (run #5 screenshots): offline raycaster — WRAP addressing renders `fly_bg` +
  `fly_add` + `fly_ble` clean; CLAMP on `fly_add`'s textures only reproduces the screenshots
  (grey bands with thin lines, radial streaks). Hypothesis (unconfirmed in-game): `fly_add`'s
  textures sample with CLAMP. World facts (20260915): gs texture registry stride 0xA0 (base
  RVA 0x6f1a90, spin 0x6f1a8c, index `id >> 0x11`), lookups 0x2375c0 / 0x236b20, init sites
  0x235f10 / 0x2364e0 write entry +0x34/+0x38/+0x3C = 3, +0x44 = 1, +0x48 = 1, +0x58 = 1 (the
  sampler desc per `docs/playfield_styling_research.md`). Second issue: `fly_ble` alpha-blends
  with z-write ON (mesh flags 0x2C0) under an alpha test GREATEREQUAL 0, so transparent texels
  write depth and may reject the additive smoke behind the tube. Candidate fix (0x400, z-write
  off) is for the plan, not now.
- RE findings of this session: see "Session 6 RE" below.

## Session 6 RE (2026-10-06) — findings (verified) and the proposed plan (NOT applied)
Full write-ups: `docs/3d_model_format_research.md` §3.8 (World sampler state = `texture.db`),
`docs/wii_ddr_hottest_party_2_3_research.md` §2.1 (MUSIC FIT texture-matrix update) and §7.5
("Session 6 RE"), `docs/wii_ddr_zan_effects_research.md` §3.4 (world offset re-verified).
Scratch (raycaster, 60 Hz grid emulation, PE disassembler helper):
`.agents/scratchpad/2026-10-06-flight-re/` (also in `$TMPDIR/opencode/s6`).

**Verified:**
1. Wii (MUSIC FIT main.dol): the grid's only per-frame effect is the GX texture matrix
   (`FUN_8010b98c`): `m13 += 0.125` repeat per 60 Hz tick (keys 0 → 1 over 0.1333 s win over
   the equal constant speed; key times in seconds, speeds per 60 Hz frame; dt = a fixed 1/60 s
   per update call × 1.2 on 50 Hz; no rate on objects 0 / 1 at the switch). Textures: every
   STG201 TPL image is `GX_REPEAT` / `GX_LINEAR`, no mips. The world offset translates camera,
   stage AND dancers identically — no relative motion. 60 Hz emulation with these values:
   seamless streaming, no aliasing (ring + 3 trails advance 16 px / frame as one group).
2. World (20260915): a texture's address mode / filters are fixed at CREATE from the `usage`
   word; for a `.dds` the loader takes it from `data/data/texture.db` (in `startup.arc`, FNV-1 of
   the registry key = basename lower-cased, `_` removed), default attr `0x55` = **CLAMP** /
   LINEAR / mip LINEAR when the key is absent. Stock textures are listed (318 / 345 names,
   attr `0x315` = WRAP, aniso 2); our ported textures are not (1 / 75 by coincidence) → every
   custom-model texture samples CLAMP. The KTMDL texture-entry bytes `+0x0A..0x0C` are
   indeed never read. No other writer of the registry's sampler desc exists (exhaustive sweep).
3. Why that is BOTH problems: the ported grid tube's v runs −2 → −0.5 → 1 per ring pair and
   repeats; under CLAMP each segment shows one ring cluster that slides a whole repeat as
   `offV` 0 → 1 and snaps back when the clip wraps — a 7.5 Hz sawtooth independent of frame
   rate and clock (explains: unchanged by the `.sanm` fix, by dance → real time, by the
   smoothed clock; sparse single rings in the screenshots vs the Wii's dense clusters). The
   same CLAMP smears the sky sphere (u −4..5) and `fly_add`'s strips/streaks; UV-in-range
   quads (planets, stars) are fine.
4. Run #5 pacing data: dt and count steps alternate 3 / 8 / 12 ms at 120 Hz under CrossOver
   (the game samples time where it runs, presentation is vsynced); the two `visit(2)` reads
   per publish both copy the same slot (lag 0) → no visual effect. Neither is the cause.

**Still assumed (not RE'd):** the Wii's z-write / alpha-compare for its "soft" (`0x83`)
blended materials (the grid). World's engine enum → `D3DTADDRESS_*` tables are identity (as
the 20260616 note found by live CE; not re-read on 20260915).

**Decision (maintainer, 2026-10-06): P2 (code patch) over P1 — P1 would rescan every custom
texture at boot to synthesise the db (IO cost); the intent is WRAP for all custom textures
anyway. APPLIED (session 6, uncommitted): signature `texture_db_default_attr_imm32` (unique +
byte-shape identical on all five builds; sweep + shape_diff green) and
`background_dancers::texture_wrap::apply` run from the mod's new `early_apply`: imm32
`0x55 → 0x15` (attr bits 7:6 cleared = WRAP; filter / mip / aniso unchanged), stock value
verified first, fail-open with one WARN. Boot-only (onBoot reads it once), no revert path;
skipped when the mod is config-disabled. Affects only db-absent textures: all mod content + 27
stock `2d_font_*` / license sheets (UVs inside [0, 1]).

**The plan as proposed (for the record):**
- P1 — WRAP for mod textures, data-only (preferred): generate an extended
  `data/data/texture.db` (stock 1604 records + one `0x315` record per
  `data_mods/custom_models/**/*.dds` registry key, sorted; field +4 = 0) and ship it through
  LayeredFS as `data_mods/background_dancers/data/arc/startup_arc/data/data/texture.db`
  (the `.arc` overlay path `arc_handler` already repacks; `startup.arc` is read in `onBoot`
  after LayeredFS installs). Open points: regenerate when models are added (a build step in the
  port scripts, or the DLL writing it to `_cache/` at LayeredFS install from the scanned
  model folders); keys of textures that need CLAMP (none known in our content; 2D sheets are
  not DDS-through-this-path). Verification: a read-only diagnostic logging each bound
  texture's desc (`entry+0x34..0x4C`) for the flight parts, then the cabinet.
- P2 — alternative if P1 cannot be served early enough: a one-byte patch of the default attr
  (`0x55` → `0x15`, `C7 44 24 ?? 55 00 00 00` in `onBoot`) — changes only db-absent textures
  (ours + 27 stock font / license sheets that stay inside [0, 1]) — or a detour on
  `FUN_1802058c0` returning WRAP for unknown keys. Both are code; P1 is not.
- P3 — `fly_ble` z-write: set mesh flag `0x400` on the alpha-blended grid / cutout meshes of
  the flight parts in `port_stage_hottest2` (stock A3 uses `0x6C0` for glows) so transparent
  grid texels stop occluding the additive smoke; alternatively keep z-write and accept the
  patches. Decide after P1 is seen on the cabinet (CLAMP → WRAP changes what is transparent).
- P4 — nothing to change in the clock path: the raw count is correct; the Wii's 1/60 s tick
  ⇔ World's `real_s` seconds at 7.5 repeats / s. `FlightDiag` removed after run #6.

## Deploy & test log
- Run #1 (2026-10-05, maintainer): HP3 flight stage + random HP3 dancer: take-off, platform, flight
  through the tunnel, orb / rainbow / stars all work. Issues → session 4 above.
- Run #2 watch-list: intro shots during the take-off (3 + 4 + 3 s) then the stage shots; the burst
  at the mouth ~3 s in (`flight intro stage effect -- effect 0 at intro frame 180 ...` INFO,
  `built ... N fx` includes `fx_<key>_xa`), `flight intro sound fxbXXXXXXXX at 3.0x s` INFO + audible;
  the mouth opening + its scroll at 5 s; tunnel lines stream steadily (any song tempo); no stage
  scroll elsewhere regressed (re-port); fast songs: take-off still 10 s real; dancers' flight loops
  still on the beat; rewind/quick restart re-arms the sound.
- Run #3 (2026-10-05, maintainer, 120 Hz): intro shots, sky burst + sound, flyer FX, take-off
  and flight work, HP2 intro shot works. Issues → session 5: grid lines still jitter back and
  forth (now with a slow forward creep); no tunnel scroll during the take-off; World's READY?
  over the take-off.
- Run #4 watch-list: (a) `flight diag` lines after the switch — `mc (d N ms)`: are there 0-ms /
  16-ms steps? `real (d …)` steps even (~8.3 ms at 120 Hz)? `board pub/read/lag` = (1, 1, 0)
  every frame? The 1 s summary's "frame(s) without a step" / "not read exactly once"; and does the
  grid now stream steadily? If the real steps are even AND every publish is read once but the grid
  still shuffles, the cause is downstream of the board (the item's material copy vs the draw) —
  next diagnostic there. (b) The take-off: the tube stretches out of the mouth from ~5 s and
  wraps the platform by ~7 s with the grid scrolling; the flight tunnel bends gently. (c) READY
  hold: `flight take-off before the song -- READY? dwell held ... stage panel layer 0x… hidden`,
  the take-off visible with the panel gone, then `... released (take-off end) at take-off 9.70 s`,
  the panel back and its reveal + READY? after the take-off; first count log `playing -- first
  count …` and whether the flight starts on the music; watch for a stand-down line instead
  (and why), lanes / HUD drawn during the take-off, and a panel that re-shows itself while hidden.
  Quick restart on a flight stage: in place = no take-off replay; fresh DPS = stand-down line.
- Run #4 (2026-10-05, maintainer, 120 Hz): the mouth's tube now stretches over the platform
  (skinning fix confirmed). Stutter looks the same. READY hold failed with both the A3 Gold and
  the World skin (READY as normal, the song starting with the take-off) → session 5b.
- Run #5 (2026-10-05, maintainer, 120 Hz): READY hold works with every DDR SELECTION skin (the
  take-off before the song, panel hidden / shown); stutter unchanged with the smoothed clock;
  skybox smears (screenshots `$DDR_WORLD_INSTALL/screenshots/20261006_0..3.png`); `flight diag`
  data → session 6.
- Run #6 (2026-10-06, maintainer, 120 Hz, P2 build): everything works — tunnel scroll streams
  exactly as intended, skybox streaks / stretching gone, "visually looks pretty great". Root
  cause confirmed: CLAMP addressing on mod textures (texture.db default), fixed by
  `texture_wrap` (session 6 RE).
- Run #5 watch-list (kept for reference): (a) World skin: `READY? step reached: timer …, scene built, stage panel state
  Some(4)`, then `before the song -- … stage panel layer 0x… hidden`, the take-off visible during
  the dwell, `released (take-off end) at take-off 9.70 s`, the panel back + its reveal + READY?;
  (b) A3 skin: the same, no `another driver seeds` stand-down (the legacy panel waits; its
  `frame_out` after the release); (c) stutter: `flight diag f…` and `1 s #…` — dt min..max (frame
  drops ⇒ 16.7 ms dt ⇒ the grid's trail lines alias even with a perfect clock), real-clock steps
  even?, board `(1, 1, 0)` and `read +x ms` stable?, any `MISMATCH` (another writer of the grid's
  material copy).
- Still open from the run #1 watch-list:
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
