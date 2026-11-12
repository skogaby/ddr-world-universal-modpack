# zan particle effects (`CzanEff`, `.TEB`) — RE notes

Konami's Wii `zan` library draws its effects with a particle runtime, not with models. These
notes cover what the HOTTEST PARTY flight stages need: the flyer's orb, trails and stars
(`boss_ddr3.TEB`) and the intro's stage effect. Addresses are MUSIC FIT's main.dol
(`ddr_music_fit_main.dol` in Ghidra) unless marked HP4. SDA bases: MUSIC FIT r2 = 0x8032d840,
r13 = 0x8032c000; HP4 r2 = 0x802bf5a0. Reference decoder + simulator: `scripts/teb_dump.py`.
Feature context: `docs/wii_ddr_hottest_party_2_3_research.md` §7.5 and
`.agents/planning/2026-10-05-hottest-party-flight-stages/progress.md`.

## 1. Files, banks and who plays what

No REL modules on any zan disc (FuruFuru Party, MUSIC FIT, HP4, HP5): all code is in
`sys/main.dol` (no `.rel` / OSLink / objdll strings; HP1's Hudson engine has them).

The effect manager (`CzanEffMng`, getter `FUN_8012a104`, global at r13-0x72cc / r13-0x7188) holds
banks by CATEGORY, each registered from a `WII\0` archive {TEB, TPL, [model archive]} by
`FUN_8012a590(mng, category, archive)` (unregister `FUN_8012a7c4`):

| cat | file | registered by |
|---|---|---|
| 1 | `game/GAME_STG_EFF.bin` (ddr3_stage.TEB + smoke model) | stage, `FUN_8004090c` |
| 3 | `game/GAME_APL_EFF.bin` (concent.TEB) | `FUN_80040a58` |
| 5 | `stage/STG201_EFF.bin` (stage_effects01.teb, 1 effect) | stage, `FUN_8004090c` |
| 6 | `game/GAME_CHR_EFF.bin` (boss_ddr3.TEB, 24 effects, 17 textures) | characters, `FUN_8004b2ec` via `FUN_80033c20` |
| 0, 2, 4 | other game screens | `FUN_80091df4`, `FUN_80077d24`, `FUN_8003eb58` |

STG201_EFF and GAME_CHR_EFF are loaded only when the song's play setup has flag 0x200
(`FUN_800b44d8` / `FUN_800b4b70`) — the flight songs. HP4 / HP5 ship the flyer effects as
`character/CHR_EFF.bin` (the same boss_ddr3.TEB with 27 effects: MUSIC FIT's 24 + 3) and HP4 adds
a `CzanEffTail` class (not looked at).

API: play `FUN_8012aca8(timescale, mng, effect_no, category, group)` → handle; `FUN_8012af28`
set_mtx (copies the attach matrix and scales its ROTATION columns by the manager scale
`+0x294` = 10.0, `FUN_8012f5a8`); `FUN_8012aeec` set_f_draw; `FUN_8012ae74` stop (with fade);
`FUN_8012afe0` get_end; `FUN_80130734` start (optional fade-in). Per frame the manager ticks
`FUN_8012a970` (dt = 1/60 s × play speed) and draws `FUN_8012ab6c`.

### 1.1 The flyer's effects (`FUN_8004b5a8`, matrices `FUN_8004b360`)

Per player p (0..3) in mode m, three instances: effects `8m+2p`, `8m+2p+1`, `8m+2p+1`
(table 0x80218ad8), attached to dancer joints from table 0x80218b68 — modes 0 / 1: joints
[0, 1, 2]; mode 2: [5, 3, 4]. The dancer's joint table (+0x940, `FUN_800497a4`) is
[Hips, LeftHand, RightHand, LeftFoot, RightFoot, Hips]; joint 5 is used position-only. So:

- mode 2 (the leap, f 544 of the intro): effect 16+2p at the Hips (flash, one-shot 0.6 s), 17+2p at
  each foot (star sprays);
- mode 0 (after the switch; mode 1 when the play mode `+0xe4` == 4): effect 2p at the Hips = the
  ORB (ring sprite tex 5 with blue→gold→orange colour keys, world-space gold sparkles tex 1, a
  helix ribbon tex 7 on outward particles, the RAINBOW ribbon tex 3 on an immortal particle at the
  emitter), effect 2p+1 at each hand = an ORBITING STAR (a carrier node on a looping circular key
  path, r ≈ 1.3 effect units, 2.03 s; children: flip-book stars tex 0, a star sprite tex 1, a white
  trail ribbon tex 2).

Player colours: gold (P1), blue, pink/red, green. TPL indices: 0 / 9 / 10 / 12 small-star
flip-books, 1 / 8 / 11 / 13 big stars, 2 white flame gradient, 3 RAINBOW, 4 / 5 / 16 rings,
6 an empty 32×32 (carrier particles that only draw ribbons), 7 helix strip, 14 swirl, 15 flare.

### 1.2 The intro's stage effect and sound (the sky burst)

`FUN_80037354` (the flight intro, research 2/3 §7.5) plays category 5's effect `DAT_802892d0`
(effect 0 of the one-effect bank `stage/STG201_EFF.bin` = stage_effects01.teb) at f 180
(`r2-0x7ca0` = 180.0) via `FUN_800431a4`, attached to a COL node of the stage
(`FUN_80045b88(col, mtx, stage+0x6ba4)`). The node: the stage builder `FUN_8003fe78` matches every
COL node named `EFF_<xx_yy>` against a 0x104-entry table at 0x80288828 (12-byte entries `{char *name,
u32 effect, ...}`, filled by a static initializer at 0x800c7ef0) and stores the node index at
`stage+0x6100+12·i`; `+0x6ba4` is entry 227, name `"04_01"` (`r13-0x7b24`) — **`EFF_04_01`**, at
(0, 740, 2730) zan units with no rotation on STG201 / 205 / 206 alike: the tunnel mouth in the sky.
The same frame plays sound 0xF6 `SE_DDR_BOSS` (0xF7 `SE_DDR_BOSSMULTI` when play mode `+0xe4` != 1)
through `FUN_8001f6b8` (sound id < 10000 = the BRSAR sound index, `FUN_800c7e28`). Both are a
one-note RSEQ (`DDR_BOSS`: `prg 10, vol, ..., note 60 vel 127, fin`) on `BNK_SEDDR` program 10 =
wave 10 of its RWAR: 9.94 s stereo 32 kHz at its root key — one sound for the whole burst-to-leap
stretch (loud from 2.5 s into it, i.e. intro 5.5 s, fading out by 9 s ≈ the leap at 9.07 s).

HP4's intro is `FUN_80106298` (same script: f 180 burst + sound, here 0x115 / 0x116 = HP4's
`SE_DDR_BOSS` / `BOSSMULTI`, the same wave; f 544 leap; sky fade 390..420; a lighting preset switch
at f 390 `FUN_801030bc`). Its stage effect starter `FUN_8004d7bc` plays the stage's bank (STG301.bin's
own `STG301_EFF.bin`, 21 nodes, 4 s) at EVERY COL node whose name starts `EFF_03_` (`r13-0x7c14` =
`"03_XX"`, compared on 3 chars; the builder `FUN_8004e4dc` lists them at `+0x8ed4`): STG301's
`EFF_03_01`, again (0, 740, 2730).

## 2. TEB format (big-endian, offsets relative to the TEB)

```
+0x00 "TEB\0", u32[3] 0, +0x10 u32 table (0x18), +0x14 u32 count << 16
table: count x {u32 node_list, u32, u8 nnodes, u8[3], u32}
node_list: nnodes x {u8 child, u8 next, u8 type, u8, u32 data}   (indices; 0 = none)
   type 0 root (CzanEffRoot), 1 part (CzanEffPart), 2 model (CzanEffMdl, unused here)
root data: {u32 loop, u32 tracks}            loop 1 = restart at the track end (§3.1)
tracks: {u32 n, u32 table} -> n x 0x14 {u32 nkeys, u32 keys, f32 t0, f32 t1, u8 spline}
   key 0x20 {f32 t, f32 pos[3], f32 quat[4] (x y z w; all 0 = identity)}
part data (`FUN_8012b1fc`):
   +0x00 u32 flags; +0x04.. u32 offsets of the blocks the flags select:
         [1] emitter (0x1), [2] gravity (0x2), [3] draw (0x4), [4] chain (0x8),
         [5] ribbon (0x10), [6] spin (0x20), [7] follow (0x40);
   +0x20 ellipsoid block, +0x24 ring block, +0x28 tracks, +0x2C u8 shape (0 box, 1 ellipsoid, 2 ring)
   behaviour flags: 0x80 world-space particles, 0x100 follow scale, 0x200 no depth test
   (Z func ALWAYS), 0x400 flipped default UVs, 0x800 immortal (age wraps to 0 instead of dying)
emitter: f32 life, life_rand, interval, spread[3] (box), rot_spread[3] (euler), speed, speed_rand;
   +0x2C s16 max particles, +0x2E s16 per spawn, +0x32 u8 billboard (0 camera, 1 lay flat)
draw: u32 flags (0x2 flip-book, 0x4 lit sphere mesh, 0x20 colour keys), u32 colours,
   f32 rect[4] (x0 y0 x1 y1: quad size = x1-x0, y1-y0), u32 scale_keys, u32 alpha_keys,
   +0x20 u32 flip-book, +0x24 u8 nscale, +0x25 u8 nalpha, +0x26 u8 blend (2 = additive SRCALPHA/ONE,
   else SRCALPHA/INVSRCALPHA), +0x28 s16 TPL index (<0 untextured), +0x2A u8 ncolour,
   +0x2B / +0x2C u8 size variation % (lo, hi)
   colours: flag 0x20 clear: one RGBA; set: ncolour x 8 {r, g, b, _, u8 t%, ...}
   scale / alpha keys: {f32 t (0..1 of life), f32 v}
flip-book: {u16 width, u16 cell, f32 frame_s, u8 frames}
ribbon: {f32 width, u8 segments, s16 TPL index}
spin: f32 [roll0, roll0_rand, roll_rate, rate_rand, roll_accel, accel_rand, orbit_rate, orbit_accel]
follow: {f32 follow (0 = stay in world, 1 = follow the emitter), f32 scale}
ellipsoid: {u32 flags (1 on the surface, 4 radial velocity), f32 radii[3]}
ring: {u32 flags (8 XY, 0x10 XZ, else YZ; 2 fill the disc; 4 radial velocity), f32 radii[3]}
```

## 3. Runtime

### 3.1 Nodes (`FUN_8012f788`, effect update `FUN_801302dc` / `FUN_8013119c`)

Each node has a clock (+0x34) advancing by dt while playing. Its local transform is the track
whose [t0, t1] holds the clock (positions linear, or Hermite with Catmull-Rom tangents when
`spline`; rotation slerp); no such track = inactive (a part stops spawning, live particles go on).
world = parent.world · local (column vectors, GX 3x4); the root's parent is the set_mtx matrix.
A looping root (loop = 1) resets every node clock to 0 when it passes its track's end; a
non-looping effect ends once all particles are gone.

### 3.2 Parts (`FUN_8012b5b8`; init `FUN_8012bf2c`; spawn PTMF by shape `FUN_8012c790` box /
`FUN_8012c92c` ellipsoid / `FUN_8012cb64` ring + common `FUN_8012c438`; step `FUN_8012ce18`)

- Emitter scale = the world matrix's column lengths (includes the ×10 manager scale).
- Spawning: an accumulator; when ≥ interval, spawn up to `per_spawn` into free slots (≤ max), the
  k-th catch-up round born at age interval·k; with interval 0 one round per frame.
- Spawn: position by shape (box ±spread; ellipsoid on/in a random direction; ring by a random
  angle), velocity direction = (Ry·Rz·Rx of random ±rot_spread)·(0,1,0) or radial; life =
  life ± life_rand (≥ 0); speed = speed ± speed_rand; spin values base ± rand; world-space
  particles store the emitter's position + rotation quaternion; size multiplier from the draw
  block's size variation; non-keyed colour from the colour entry.
- Random: `FUN_801322bc(x)` = (2x/10000)·(rand() % 10000) − x, `FUN_80132354(x)` = x/10000·(rand() %
  10000) (MSL rand).
- Step at age a, tn = a / life: pos = p0 + dir·speed·a; with spin, pos = Rot_Y(orbit_rate·a +
  orbit_accel·a²/2)·pos (orbits about the emitter's Y).
- Gravity (flag 0x2; block `{f32 dir[3], rot_spread[3], accel, accel_rand}`): at spawn the
  particle's gravity direction is `Ry·Rz·Rx(±rot_spread)·dir` (box `FUN_8012c790`, non-radial
  ellipsoid `FUN_8012c92c`) or `−normalise(pos)` (ellipsoid with radial velocity), its accel
  `accel ± accel_rand` (`FUN_8012c438`). Step (`FUN_8012ce18`): a RING with radial velocity
  (flag 4) falls in the emitter's frame, `pos += g·accel·a²/2` before the emitter transform; every
  other part falls in WORLD space after it, `wpos += g·a²·accel·(manager scale 10)/2` (the
  direction kept unnormalised).
  Local particles: world = emitter.world · pos. World-space (0x80): rotation = slerp(stored,
  emitter, follow), stored position += emitter displacement this frame × follow.
- Billboard (mode 0): the sprite's rotation = the camera basis (transpose of the view rotation,
  manager +0x1d8), columns 0 / 1 scaled by the emitter scale; mode 1: emitter rotation · lay-flat
  matrix (0x80274eb8). Spin then rolls it about its own z by roll0 + rate·a + accel·a²/2.
- Size: scale_key(tn) × size multiplier × rect width / height. Alpha: 255 × alpha_key(tn) (colour
  keys) or alpha_key(tn) × colour.a, × the effect fade. Colour keys interpolate on the life
  percentage. Flip-book frame = floor(a / frame_s) % frames in a grid of width / cell columns.
- Draw (`FUN_8012e6ac`, `FUN_8012ec68`): particles depth-sorted (`FUN_8012ddf4`), alpha parts
  first then additive; a sprite is the unit quad (±0.5) strip (0x80274f70) with UVs
  (0,1) (0,0) (1,1) (1,0) (0x400: (1,1) (0,1) (1,0) (0,0)), one RGBA for all corners.

### 3.3 Ribbons (flag 0x10; `FUN_80131658` init, `FUN_80131d08` push, `FUN_80131764` age,
`FUN_8013193c` draw)

Each particle owns a ring of `segments` points (0x44 bytes). Every frame with dt > 0 the particle's
world position is pushed; its edge offset = normalise(cross(camera_pos − p, p − p_prev)) ×
width × scale_key × emitter scale x (camera position = manager +0x288), edges p ± offset. A
point lives 1.0 s (ribbon +0x1c) and is dropped at ≤ 0. EVERY live point is moved each frame by
the manager's drift vector +0x298 (`FUN_80131764`). u = 0 / 1 across, v from 1 (oldest) to 0
(newest); one RGBA (the particle's) for the whole strip; blend by the draw block.

The manager drift is 0 in the constructor (`FUN_8012a10c`) and nothing ever writes it (MUSIC
FIT: direct `stw` / `stfs` / `psq_st` to +0x298 and every `addi rX, rY, 0x298` — only the
constructor and the ribbon update; the vector setters are `FUN_8012aca0` (+0x288 camera position)
and `FUN_8012ac98` (+0x198 view matrix)).

### 3.4 The rainbow trail: the flight scrolls the whole world

What makes the trails stream behind the flyer is not the effects but the play object's WORLD
OFFSET (+0x3ac, vec3; enable flag +0x3a8):

- the intro arm (`FUN_80035634` → `FUN_800fb07c(+0x3ac)`, +0x3a8 = 0) zeroes it;
- at the switch (`FUN_80037354`, the intro camera's end) it is set to (0, 0, −50000)
  (`FUN_800fb090`, r2-0x7c68 = −50000.0) and +0x3a8 = 1;
- every unpaused frame (`FUN_800377e0`) z += 3.0 (r2-0x7cac) while z < 50000 (r2-0x7c60) — 180
  zan units / s ≈ 20 m/s, 555 s to the end;
- the same function applies the offset to the camera (`FUN_800fb0b0(camera + 0x60, offset)`), the
  stage (`FUN_80042fa0`: a translation matrix) and every dancer (`FUN_800fb500(mtx, mtx, offset)`
  before the dancers' effect matrices are taken, `FUN_8004b360`).

On screen nothing moves — camera, tunnel and flyer translate together — except state the
effects keep in WORLD space: ribbon points (each lives 1 s, so the rainbow — ribbon width 0.3,
30 points, on an immortal particle at the Hips — is a ~180-unit / 20 m streak behind the flyer, and
the hands' stars draw long white trails) and world-space (0x80) particles (the orb's sparkles drop
behind). During the take-off the offset is 0, so the leap burst does not stream.

Re-verified 2026-10-06 (decompiled `FUN_800377e0`, `FUN_80042fa0` → `FUN_800459fc`,
`FUN_800478b8`): the camera's eye and target, every stage object's base matrix and every
dancer's matrix receive the SAME translation — the stage does not move relative to the camera;
the tunnel's visible motion is its texture matrix alone (`docs/wii_ddr_hottest_party_2_3_research.md`
§2.1).

A port reproduces it by running the effects in a world scrolling at (0, 0, 180) units / s from the
switch (`teb_dump.simulate(scroll=FLIGHT_SCROLL)`), or equivalently by moving every world-space
point / particle anchor by −scroll·dt each frame; World's camera and scene need not move.

## 4. Reference simulator (`scripts/teb_dump.py`)

`parse_teb`, `teb_members`, `simulate(effect, attach(i), frames, view_rot, cam_pos)` reproduce
§3 for the features boss_ddr3 and the stage burst use (shapes 0–2, flags 0x1 / 0x2 / 0x4 / 0x10 /
0x20 / 0x40 / 0x80 / 0x200 / 0x400 / 0x800, draw flags 0x2 / 0x20). Not implemented: chains (0x8),
follow scale (0x100), the lit sphere mesh (draw 0x4), model nodes (type 2), fades.
Blender check: `tools/blender_ddr_addon/examples/preview_flight_fx.py` (the orb ring, stars
orbiting the hands with trails, sparkles; the leap flash and foot sprays). Unit tests:
`scripts/test_teb_dump.py` (a synthetic bank). Until 2026-10-05 the preview attached the effects
to `C^T · m · C` of the Blender bone matrix (the joint's local axes turned 90° about x); the joint
frame is `C^T · m` (the ported rigs keep zan's joint frames, `convert.rowmat_to_blender` keeps
local axes) — which plane the hand stars orbit in is a cabinet / footage check.

## 5. The DLL port (`src/mods/background_dancers/flight_fx.rs`)

**Runtime:** a std-only Rust port of `teb_dump` (§2–§3; f32, column vectors, zan units),
numerically the reference (boss_ddr3 effects 0/1/2/7/16/17 over 90 frames with a moving attach and
the scroll: same particle counts, centres / colours / ribbon edges within 3e-5 of `simulate` run in
f32). Fixed 60 Hz steps (the game's tick, so ribbons keep 1 s = ≤ 61 points whatever World's frame
rate) on the REAL clock (the music count, not dance time); a catch-up runs ≤ 8 steps, a longer gap is
skipped, time going back resets. The world scroll is applied as a −v·dt shift of every world-space
point (ribbon points, world-space particle anchors, the emitters' previous positions) — the
caller's frame stays still. Who plays what is main.dol's table (§1.1, `mode_effects`): per flying
dancer i, player i mod 4; the leap (mode 2) at take-off frame 544 of 600 (dance time: the take-off
clip's length × 544 / 600), the flight (mode 0) from the switch; the scroll from the switch on (the
leap's world-space leftovers stream too). Joints: the evaluated body bones under the body's world,
rotation columns normalised, translation / `GAME_SCALE` (World's game space and zan's share axes).

**Drawing:** World's renderer has no particle path; the content port
(`tools/blender_ddr_addon/examples/port_flight_fx.py`) ships, inside each flight stage's arc,
`data/map/flight_fx/flight_fx.teb` (the bank verbatim: MUSIC FIT's, HP4's `CHR_EFF.bin` for HP4
Stage 301), the pool layout `flight_fx.txt` and per player POOL models (`fx_<key>_s<p><k>`: one unit
quad per particle — mesh, material, bone; `fx_<key>_r<p><k>`: one strip per ribbon, a vertex pair per
point, split at the 52-bone palette limit; ≤ 255 bones a model, KTMDL's byte blend indices). Sizes =
the exact peak: per (texture, flipped UVs, flip-book cell, blend, depth) group the sum of `max` ×
instances of the parts drawing it in the player's mode 0 + 2 effects (101 quads, 8 strips / 375
bones a player for boss_ddr3); a fully transparent texture (the 32×32 carriers) gets no pool. Mesh
flags: additive `0x06C1` / flags2 4 (the stage ports' `add`), alpha `0x02C1`; part flag 0x200 adds
`0x0020` (Z test off); huge bounding spheres / bone boxes (the quads go anywhere). Shader
`mdl_ch_constant_vc` (unlit, `tex × COLOR0 × c23`, applies `m_vTexAnime`). Per frame the director
publishes on each pool instance's frame-board slot: bone matrices (a sprite: the billboard axes ×
size at the centre, faced at this frame's camera; a ribbon point: its two edges; unused entries
collapse to zero), one colour per DRAW RECORD (`rec+0x00`, × tint → c23; additive colours are
premultiplied with alpha 1 — the collector forces an entry with alpha < 1 into the alpha group) and
`m_vTexAnime` writes (a flip-book quad's cell offset, a ribbon's v scale so its oldest live point
reads v = 1). The frame board grew for it: 64 slots, 256 bones, 128 material writes and 128 record
colours a slot; `visit(2)` copies straight into the item.

**The intro (2026-10-05, after the first cabinet run):** the take-off — everything the Wii runs on
its 60 Hz intro clock — runs on the REAL clock (the music count): the take-off clip, the platform
script (`intro_look`), the intro camera shots (`<key>_intro01..03` = STG201_CAM00_01..03 / HP4's
STG301_CAM.bin, played in order, 3 + 4 + 3 s), the leap and the sky burst; the flight after the
switch keeps dance time for the dancers (`flight_fx::flight_schedule_time`: continuous at the
switch) while the tunnel (`fly_*` parts) runs on the real clock from the switch, as the Wii's stage
objects do. The burst is the stage bank's effect 0 at the layout's `stage_effect` line (frame 180,
the node position), drawn through its own sprite pool (pseudo-player 4, instances
`InstanceKind::StageFx`); its gravity parts (nodes 10 / 11 / 15) use the gravity port above. The
sound: `port_flight_fx.burst_sound` renders the one-note RSEQ (the wave at its root pitch), mixed
to mono, 44.1 kHz, `flight_fx/burst_44k_mono.pcm`; the DLL encodes it into a one-cue MS-ADPCM XACT
pair on the parse thread and plays it once through a slot-less one-shot bank
(`game_audio::register_one_shot_bank`) when the real clock passes frame 180 (gameplay only;
rewind = stop + re-arm; teardown = stop).

**The real clock and the READY hold (2026-10-05, after the third cabinet run).** The tunnel grid
(MUSIC FIT `DRAW_B03_grid01` material 3, `grid01nuki`: a bright line and three fading trail lines
21 px apart in a 128 px tile, scrolled 0.125 texture / 60 Hz frame) is a temporal-aliasing probe:
any unevenness in its clock turns the trail lines into a back-and-forth shuffle while the head
creeps forward (what run #3 showed at 120 Hz). Offline the `.sanm` samples monotonic and the
sampler interpolates fractional frames, so the music count was suspected: an i32-ms value written
by `GamePlayActor::onUpdate`, read one update stale. A smoothed tracker of the count in wall time
(`clock::SmoothClock`, a critically damped second-order lock driving `real_s`, and later the dance
clock too) was tried and deployed (runs #4 / #5): NO visible change, so it was removed — the real
clock is the raw count again (+ the take-off length after a READY hold), and nothing in the DLL
smooths or substitutes the game's music or judgement clock (read-only use only). What remains is
a read-only diagnostic (`lifecycle::FlightDiag`, every flight stage, bounded): the first 48 frames
after the switch and one-second summaries — dt, count steps, real-clock steps, the grid's `offV`,
the item's material copy read back a frame later (another writer = MISMATCH), and whether / when
(ms after the publish) `visit(2)` copied each frame-board publish (`frame_board::reader_stats`).
Ruled out by disassembly: a deferred read of the item copy by the command-list executor — the
material constants are copied into the stream at build (`FUN_18026cce0` → `FUN_18026c440`,
20260825). Run #5 data (120 Hz, CrossOver): the game's frame dt and the count step alternate
~5 / ~12 ms (mean 8.3 ms) and the board shows two `visit(2)` reads per publish (`pub/read/lag
(1, 2, 0)`). Still open — and under RE before any further fix: how MUSIC FIT itself drives the
grid (texture-matrix update, world offset), and World's frame pacing / presentation under the
frame-rate hack. The maintainer's HP3 footage shows NO aliasing: a model of the Wii effect that
predicts visible aliasing is wrong.

The take-off can now run BEFORE the song, as in the games (`flight_hold_logic.rs`, engine side
`flight_hold.rs`; research 2/3 §7.5): the stock stage panel shows for 4 s of World's 5 s READY?
dwell, then the panel layer is hidden, the dwell timer (`DPS+0x130`) is held at 4.0 and the
take-off plays on the real clock's free run (no music count yet); 0.30 s before its end the panel
comes back and the dwell is released (seeded to 1000), so the song's music 0 is the switch: the
real clock is then `count + take-off length`, dance time at the switch = `tau(0)`. The take-off starts once
the scene is built and the panel settled (ShutterActor state 4), and the hold enables the
SceneGraph itself — World sets the bit only when the dwell gate passes (run #4). DDR SELECTION's
legacy panel, which seeds the timer every pre-song frame, defers to the hold
(`services::ready_hold`). It stands down (the take-off plays over the song as before) when quick
restart's fresh-DPS path seeds the dwell, when scene / panel are not ready within 8 s, or when the
dwell timer is unresolved; `DDR_DANCERS_NO_FLIGHT_HOLD` (developer_mode) disables it.

Not ported / deviations: chains (0x8), follow scale (0x100), the lit sphere mesh
(draw 0x4), model nodes (type 2), stop fades (none of them used by boss_ddr3's played effects);
particles depth-sorted (additive: order-free); ribbon v spacing linear (the game's first step is
1 / n, the last 2 / n); a drawable that finds no free pool entry is dropped (cannot happen with the
peak-sized pools). Assumed, to confirm on the cabinet: draw record i = mesh i (logged at build:
`[fx] N record(s) -> materials [0,1,2,…] (identity)`), the hand stars' orbit plane (above).
