# DDR 3D Formats — Blender add-on

Imports and exports DanceDanceRevolution (A3 / World) 3D assets in Blender 4.2+.

| File | Import | Export |
|---|---|---|
| `.model` (KTMDL) + `.b2it`/`.grp2it` + `.dds` | armature with the game's exact joint frames (names from `.b2it`), one mesh object per KTMDL mesh with skin weights, UVs (`v = 1 − v_file`), vertex colours, custom split normals (exact file normals kept in a `ddr_normal` attribute), material node trees that reproduce the stock shaders (see below); blend / two-sided from the mesh flags | selected armature + child meshes (or plain meshes for a static prop): rest pose = bind pose, one KTMDL mesh per material slot (more when a slot needs more than 52 bones — per-mesh palette blocks), vertex layout picked from skinning/UV/colour presence, ≤ 4 weights, stock bone-AABB rule, `.b2it`/`.grp2it`, DDS copied from the source or written as uncompressed A8R8G8B8 with 3 mips (the game accepts it — 140 stock textures are that shape) |
| **Character** (body `.model` + part models) | body + every sibling `pl_<name>_face01..03 / _head00 / _chest00 / _forearm00 / _hips00` model attached to its bone as the game does (`Head`, `Head`, `Spine2`, `LeftForeArmRoll` + a mirrored linked duplicate on `RightForeArmRoll`, `Hips`), `face02/03` hidden, the per-character scale from `chara_resources.rlist` on the armature object | the game's `data/chara/` folder layout: `pl_<key>/` body, `pl_<key>_<part>/` per attached part, and a `chara_resources.rlist` with this character's row upserted into the source list |
| **Stage** (any `gm_<stage>_<part>.model`) | every part of that stage from `map_resources.rlist` (with its `_play_loop.anm`), optionally the stage's camera set from `stage_camera_resources.rlist` | — (export each part as a DDR Model; the `map_resources.rlist` row is a manual edit, `ktmdl_dump.write_rlist`) |
| `.anm` | action on the active armature, baked per frame from the game-equivalent evaluator (`scripts/anm_dump.py`) | scene frame range of the active armature → kind 0x1C rotation / 0x1D translation (/ kind 10 scale when animated) uniform tracks — what the stock choreography uses; channels that never change collapse to one key like stock (≈1.1× the stock file size) |
| `.camanm` | animated camera (position ×0.01 → metres, orientation, lens = the game's 16:9 re-projection, clip planes) | active camera → position in cm, orientation, FOV through the inverse of the game's projection (aspect 4:3), near/far |

Format reference: `docs/3d_model_format_research.md`. Codecs: `scripts/ktmdl_dump.py`,
`scripts/anm_dump.py` (loaded from `../../scripts` in a checkout, from `vendor/`
in a packaged zip). Both writers round-trip every stock file: `write_model` byte-
identically on all 284 models, `write_anm` decoded-key-identically on all 222
animations, `write_rlist` byte-identically on all four resource lists.

## Install

* Packaged: `scripts/build_blender_addon.sh` writes `release/blender_ddr_addon-<ver>.zip`;
  install via `Edit > Preferences > Get Extensions > ⌄ > Install from Disk…`.
* From a checkout: symlink `tools/blender_ddr_addon` into your Blender
  `scripts/addons/` (or `extensions/user_default/`) directory.
* Menu entries: `File > Import > DDR Model / DDR Character / DDR Stage / DDR Animation`,
  `File > Export > DDR Model / DDR Character / DDR Animation` (select the armature
  first for `.anm` and for the character export, the camera for `.camanm`).

## Validate

```
scripts/validate_blender_addon.sh <unpacked-data-root> [out.blend]
```
runs Blender headless (`/Applications/Blender.app` or `$BLENDER`) over three tests:
`smoke_test.py` imports `pl_emi00` + `mc_female_ne01_loop.anm` + one song camera,
checks bones/names/weights/UVs/materials and that the baked pose reproduces the
codec's world positions, then exports all three and re-parses them (the model must
match the stock file's triangles, vertex counts, bind matrices, AABBs, flags,
materials and `.b2it`; the animation and camera must re-evaluate to the stock
poses); `character_test.py` imports `pl_rinon00` with all seven parts, checks each
part lands at `v · E · Bind[bone]` in body space, exports the character folder and
compares every part's vertex data and the rlist against stock, re-imports, then imports the `boom00` stage set + cameras and renders the dancer on it;
`synthetic_test.py` exports a 64-bone rig (two meshes, 52-slot palette blocks) and a
stage-screen quad (texture name `offscreen1`, 8 × 8 placeholder DDS).
Export files land in `$DDR_3D_OUT_DIR` (default `<data-root>/../blender_export_test`);
the character test finds `chara_resources.rlist` under `<data-root>/../startup/data/chara/`
or `$DDR_3D_RLIST`.

## Materials

Every stock model shader is unlit; the node trees follow the decompiled programs
(doc §3.7): `Image Texture` × `Vertex Color` for the `_vc` shaders (alpha too),
an extra multiply by `vConstantColor` (+ add `vOffsetColor`) for the `_c` shaders,
a `Mapping` node for a non-identity `m_vTexAnime`. `mdl_*_lambert` materials are
NOT shipped in A3's `shader.arc` — the game draws them with `gs_model_default`
(texture × draw tint, vertex colours and parameters ignored), so they import as
texture-only and carry a `ddr_shader_note` saying so. The Principled BSDF is kept
(roughness 1, no specular) so Blender's viewport lights still shade the preview.

**COLOR0 rule (in-game finding, 2026-09-15):** the `_vc` programs read the vertex
colour and alpha-test the result — a mesh WITHOUT a colour attribute exported under a
`_vc` shader is invisible in the game (no error, just an empty stage). The exporter
now picks `mdl_ch_constant` / `mdl_bg_constant` for colourless meshes and warns when an
explicit `_vc` shader meets one; the proven stock combination for a flat-textured mesh
is a colour attribute filled with opaque white + `mdl_*_constant_vc` (that is what the
two ports below do).

**COLOR0 bytes (2026-10-04):** the exporter writes a BYTE_COLOR attribute's STORED bytes
(`color_srgb`) as the D3DCOLOR, and the game multiplies those bytes as is. So a port writes
the source's colour bytes with `attr.data.foreach_set('color_srgb', rgba)`. The `color`
accessor is linear: Blender sRGB-encodes what it gets, and a source 0.5 ships as 188 (0.74),
0.2 as 124. Alpha is stored linearly either way; white (1.0) and black are unaffected.
Every affected source (the HOTTEST PARTY 1–3 and SuperNova / X stages, the SN / X GUS glasses) was
re-ported with the fix, so all shipped COLOR0 bytes are the discs'. `scripts/fix_vertex_colour_srgb.py`
undoes the encoding in place for a model ported by an older checkout; never run it on current output.

## Authoring a new asset

* **Character:** `File > Import > DDR Character` on any stock body (e.g.
  `pl_emi00.model`) to get the 33-bone rig the game's dance loops address by bone
  index, replace/edit the body meshes (skin to the same bones), edit or replace the
  bone-parented part objects (a hat parented to `Head` exports as `head00`, an
  accessory on `Spine2` as `chest00`, on `Hips` as `hips00`, on `LeftForeArmRoll` as
  `forearm00` — the game mirrors it onto the right arm itself; never author a right
  forearm), set the character scale on the armature object, assign image textures
  named `[A-Za-z0-9_]` (≤ 20 alphanumerics, unique after lower-casing and dropping
  `_`), then `File > Export > DDR Character` into a folder: it writes the `pl_<key>*`
  directories and a `chara_resources.rlist` with your row. Each directory is the
  content of one `data/arc/<dir>.arc`; the rlist replaces the one inside `startup.arc`.
  Set `mat["ddr_shader"]` to change a shader (default `mdl_ch_constant_vc`; `_c`
  variants read the constant colour).
* **Stage prop:** static meshes need no armature (a root bone is synthesized);
  parent them to an armature only if they should animate. Vertex colours are
  exported when a colour attribute exists.
* **Mesh flags** come from `obj["ddr_flags"]/["ddr_flags2"]` when present (imported
  meshes keep theirs); otherwise from the material: backface culling off → two-
  sided, blended render method → transparent pass with alpha blending.
* **Delivery** (in-game verified on DDR A3): pack each exported directory into
  `data/arc/<dir>.arc` — `scripts/arctool pack --output <dir>.arc <dir-parent>/data`
  with the files laid out as `data/chara/<dir>/…` — and, for a NEW character key, repack
  `startup.arc` with the exported `chara_resources.rlist` (unpack with
  `scripts/arc_tool.py unpack`, replace the file, pack). New arc names need no manifest.
  Row ORDER in the rlist matters: the attract HOW TO PLAY demo always shows **row 1**
  (stock `rage00`), so put a demo-visible test character there (or rename the export into
  Rage's slot); ordinary songs pick randomly from the sex/class pools of rows whose unlock
  id is `0.0`. Rigs with more than 52 influencing bones are fine — the exporter splits the
  mesh into 52-slot palette blocks (in-game verified).

## Stage screens (the song's movie on a TV / monitor)

The DDR World modpack's Background Movies = STAGE SCREENS plays the song's background movie
on the video screens inside a stage, like DDR A3's `monitor*` / `replicant*` stages. The game
keeps a 1280 × 1280 render target that it registers at boot as the texture **`offscreen1`**,
and a material textured `offscreen1` samples it — there is no per-stage code. To give a
custom stage a screen:

* Name the screen material's image **`offscreen1`** (case and `_` do not matter —
  `OffScreen_1` folds to the same key). The exporter then names the KTMDL texture
  `offscreen1` and writes a tiny black **`offscreen1.dds`** beside the part instead of
  the image's pixels: the game never binds it (the render target owns the name), but the
  DLL recognises a stage with screens by that file. Keep any Blender image you like on the
  material for the viewport preview.
* Use the unlit `mdl_bg_constant_vc` shader (a white colour attribute — the default the
  exporter picks for a coloured mesh). The DLL keeps screen materials unlit and without
  outlines under every Lighting Style.
* **UVs** say which part of the square the screen shows. The movie is fitted into the
  square with its aspect kept and centred (A3's rule), so in D3D top-down space a 16:9
  movie covers **v 0.21875 – 0.78125**, a 4:3 one **v 0.125 – 0.875**, a square one the
  whole square; u runs 0 → 1 left to right as seen by the viewer (unmirrored). In Blender
  (bottom-up v) that is `v_blender = 1 − v_d3d`, i.e. the 16:9 band is also 0.21875 –
  0.78125. Mapping the 16:9 band onto the whole panel makes a 16:9 movie fill it and crops
  a 4:3 one top and bottom; songs without a movie show a black screen. A source whose
  screens were authored against a 2×2 video mosaic (HOTTEST PARTY 4/5 `quarter` stages)
  must have each quadrant unfolded onto 0..1 first — see
  `examples/port_stage_hottest2.py::unfold_quadrants`.

## Porting an existing model (playbook)

Both flows are in-game verified on DDR A3 (Peter Griffin in the Griffin living room,
2026-09-15); the scripts that did it are in `examples/` and are meant to be copied and
adapted, not run blind.

### A rigged character from another game (`examples/port_character_ue_rig.py`)

The source was a Fortnite rip (FBX, 372-bone UE5 rig, A-pose, 3 meshes, flat `_D`
diffuse maps). Every step below exists because of a rule of the game's format:

1. **Get the DDR rig from a stock body of the same sex** (`import_character.load_character`
   on `pl_rage00` for a male, `pl_emi00` for a female; delete the imported meshes and
   parts). The 33 bones must not be moved, renamed, reordered, added or deleted: dance
   clips address bones by index AND carry translation tracks for all 33 bones equal to
   that sex's bind offsets, so the game forces those joint positions at runtime.
2. **Conform the source rig to the DDR joints in pose mode, then bake.** Instead of
   re-skinning, pose the source armature so its main joints land exactly on the DDR
   joints (T-pose), scaling each chain bone along its axis so the mesh between two joints
   stretches to the DDR segment length (`pbone.matrix = T(joint) · R · S(1, k, 1)`,
   parents first, `inherit_scale = 'NONE'` on the chain bones so helpers still inherit),
   then copy the evaluated vertex positions back into the mesh and drop the source rig.
   The professional skin weights survive and the joints match the animation exactly.
   UE rigs carry TWO arm chains (control `upperarm/lowerarm/hand`, parent of the fingers,
   and `deform_*`, which carries the weights); the FBX loses the constraints tying them,
   so pose both. Head/neck bones were placed but not scaled (keeps the head's size); the
   spine absorbed the torso compression.
3. **Retarget the weights by class + position.** Map each source vertex group to a body
   class (hips / spine / neck / head / collar / upper arm / forearm / hand / thigh / calf /
   foot / toe, per side) and distribute each class over its DDR bones by the vertex's
   position along the segment — the stock rigs put roughly half of every limb segment on
   the mid-segment `*Roll` bone, so `Arm → ArmRoll → ForeArm` is a linear blend along X,
   `UpLeg → UpLegRoll → Leg` along Z, the spine `Spine → Spine1 → Spine2 → Neck` along Z,
   `pelvis → Hips` and `head`+face → `Head` outright. Log the groups you did not map (the
   Fortnite rig had belt/groin/lat helpers) and add them to a class rather than dropping
   them. Keep ≤ 4 influences, renormalise.
   **Keep the head rigid:** like the stock rigs, weight everything from the jaw up 1.00 to
   `Head` and confine the `Head`/`Neck` blend to a band under the chin and at the nape
   (stock ~1.39–1.45 m). Auto/heat weights on an unrigged rip spread the blend over the whole
   lower face, while inner features (eyes, teeth, glasses) stay at 1.00. That is invisible at
   rest, but any Head-vs-Neck difference then shears the skin off those features. The
   modpack's Big Head mode (Head ×3) pushed Big Smoke's eye whites and teeth through his
   face. A strong nod does the same, more subtly. Check: no vertex above the lips should
   carry a partial `Head` weight.
4. **Textures:** use the flat diffuse maps (the `_DShaded` bakes look muddy unlit, the
   `_DF` masks are not ink lines), downscale to ≤ 1024 (1024², 1024×512 and 512² all
   load), name the images `xx_stem` (stems must be unique after lower-casing and dropping
   `_`, ≤ 20 alphanumerics), one Image Texture node per material.
5. **Colour attribute → opaque white** (see the COLOR0 rule above), backface culling on,
   opaque materials.
6. `export_character(..., key='<name>00', write_rlist=False)`; sanity-check with the codec
   (33 bones, weight sums, `write_model(model_to_spec(m)) == data`); render the re-imported
   export with a stock clip (`import_anm.load_anm` of `mc_male_lesa_lesa_exec.anm`) before
   touching the game.
7. **Deliver:** `scripts/arctool pack --output pl_<key>.arc <dir>/data` (members
   `data/chara/pl_<key>/…`), repack `startup.arc` with the rlist row inserted — **row 1**
   if it should be the attract HOW TO PLAY dancer (kinds ≥ 3 index rows directly), the
   stock row moved to the end. No face-part arcs are needed (a body without `_face01..03`
   loads fine).

### A current Fortnite rip with a tail and a weapon (`examples/port_character_fortnite.py`)

Ironmouse (2026-10-04) ships as `data_mods/custom_models/dancers/Custom/Ironmouse` (`ironmouse00`,
no weapon) and `Custom/Ironmouse 2` (`ironmouse01`, holding her *Blade of Love*); both sidecars are
`<key>, pl, F, A, 0.9, 0.75, 0.0` on the `pl_emi00` donor. Round-trip previewed; not yet
cabinet-tested. The source is the same UE rig family as Peter's, from a newer Fortnite build: an
`F_MED` body FBX (304 bones, A-pose, 4 meshes: hair, body, head, tail) plus a separate weapon FBX.
It uses `port_lib`, the Peter steps above, and these differences:

* **Bone axes.** The raw UE bone Y axis runs across the limb, and Blender's automatic bone
  orientation points thigh and `spine_04` at a helper child. `conform` stretches along Y, so
  `port_lib.align_chain_bones(fa, next_of)` re-aims every chain bone at its child first, in edit
  mode. The mesh does not move: at rest the pose equals the rest pose in any frame.
* **A-pose hands.** The hand is a terminal bone. `conform` keeps a terminal bone in its rest
  orientation, so the hand would stay bent 45° down when the forearm swings up to the T-pose.
  `conform(..., follow_parent_rot=('hand_l', 'hand_r', 'ball_l', 'ball_r'))` gives each of these
  the rotation of its parent instead. A T-pose source does not need this.
* **Centimetre FBX.** The armature imports at object scale 0.01. Do not pass `terminal_len` (it
  divides world metres by the armature-space length); leave it out and `s` = 1 keeps the size.
* **Trunk.** The Fortnite pelvis sits 1 cm above the thigh sockets, while DDR's Hips sit 6 cm above.
  The trunk takes the linear z-map of the CJ port (thigh sockets → DDR UpLeg, `neck_01` → DDR
  Neck, kz ≈ 1.05), and the thighs keep the source socket width.
* **Tail.** It is on its own root (`C_Root_Main_Root_Jnt` → `C_Tail_A_Base/1..9_Jnt`), authored
  flat on the floor behind the feet. Before the bake, each joint's world matrix is set so the
  chain hangs from the lower back in a curve: `TAIL_ATTACH` (source y,z) and `TAIL_PITCH`
  (degrees below horizontal per segment). The tail is weighted rigidly to Hips.
* **Textures.** These are toon maps:
  - `*_ColorL`: the lit colour. This is the one used, downscaled from 2048² to 1024².
  - `_ColorS`: the shadow colour.
  - `_DFL` / `_DFLC`: an ink-line mask and its colour.
  - `_STT`: shading thresholds.
  - `_N`: normals.
  - `_FX`: an effect mask.

  The game's shaders are unlit, so only ColorL ships. Its lines are already enough: the brows and
  lashes are geometry. The `_FX`-masked body islands render flat light grey: the band around the
  waist and the patches on the sleeve tops. Fortnite draws an effect over them, which is not
  reproduced here.
* **Physics chains.** `dyn_skirt*` → `skirt`; jacket, bow, ribbon, hood and chest → `spine`;
  belt, heart and the tail → `hips`; hair and pigtails → `head`. `rigid_head` forces every
  vertex that started above the chin (jaw pivot − 4.5 cm) to `Head` 1.00. The 83 facial shape
  keys are cleared, and the basis is the neutral face.
* **Weapon** (`WEAPON=hand` + `WEAPON_SRC`). The right-hand fingers are curled into a fist
  (`FIST_CURL` degrees per phalanx, about the knuckle axis expressed in each bone's rest frame).
  The weapon's handle origin goes to the fist centre: the blade comes out of the thumb side and
  the edge faces the knuckles. It is weighted 1.0 to `RightHand` as an extra body material slot.
  A `forearm00` part would not work, because the game mirrors it onto the left arm too.
  `WEAPON=back` slings it across the back on `Spine2` instead. `WEAPON_SCALE` and `WEAPON_TILT`
  are optional. At full size the 1.08 m sword clips the legs in some moves.

```bash
# runbook (macOS paths; any scratch dir works)
W=$TMPDIR/ironmouse; A="$DDR_WORLD_INSTALL/data/arc"
for a in pl_emi00 mc_female mc_female_lovy; do python3 scripts/arc_tool.py unpack "$A/$a.arc" -o $W/game; done
python3 scripts/arc_tool.py unpack "$A/startup.arc" -o $W/startup
export DDR_3D_DATA=$W/game/data DDR_3D_RLIST=$W/startup/data/chara/chara_resources.rlist
export PREVIEW_ANM=$DDR_3D_DATA/chara/mc_female/mc_female_hh01_exec.anm:$DDR_3D_DATA/chara/mc_female_lovy/mc_female_lovy_lovy_exec.anm
B=/Applications/Blender.app/Contents/MacOS/Blender; S=tools/blender_ddr_addon/examples/port_character_fortnite.py
SRC=~/Desktop/Ironmouse/Model/"Ironmouse FN.fbx" OUT_DIR=$W/v1 $B -b --factory-startup --python-exit-code 1 --python $S
SRC=~/Desktop/Ironmouse/Model/"Ironmouse FN.fbx" OUT_DIR=$W/v2 CHARA_KEY=ironmouse01 WEAPON=hand \
  WEAPON_SRC=~/Desktop/Ironmouse/Weapon/"Blade of Love.fbx" $B -b --factory-startup --python-exit-code 1 --python $S
# check $W/v*/ironmouse0*_{front,side,close_*,mc_female_*}.png, then ship the export folders:
cp -R $W/v1/export/. data_mods/custom_models/dancers/Custom/Ironmouse/
cp -R $W/v2/export/. "data_mods/custom_models/dancers/Custom/Ironmouse 2/"
```

Adapting this to another Fortnite rip:

1. Inspect the FBX first (bone list, mesh and material names, texture set).
2. Edit `MAT_TEX` and `TWO_SIDED`.
3. Check the bone names in `classify`. The unknown-groups line must print `{}`.
4. Drop the tail block if there is no `C_Tail_*` chain.
5. For an `M_MED` body, use `DONOR=pl_rage00 SEX=M MODEL_SCALE=1.0`.

The folder name is the in-game label (≤ 15 bytes), and the key comes from `pl_<key>`. Two variants
need two keys.

### Anime characters: a Rigify-style GLB and an MMD model (`examples/port_lib.py` + two configs)

The Peter flow above, factored into a reusable module (`port_lib.py`: DDR-rig load, pose-conform
with a uniform pre-scale, world-space bake incl. evaluated normals, class/position weight retarget,
palette textures, export + codec checks, sidecar row, Workbench previews) and per-character
configs of ~150 lines. Both ports (Kasane Teto from a GLB, Project SEKAI Hatsune Miku from an
mmd_tools FBX, 2026-09-22) ship in `data_mods/custom_models/dancers/` — they are the modpack's
Background Dancers content, not an A3 startup.arc repack, so no arc packing and no rlist merge:
the export folder is copied as-is next to a one-row `chara_resources.rlist.txt` sidecar
(`<key>, pl, F, A, 0.9, 0.75, 0.0` — the SEX must match the donor rig, the modpack picks that sex's
dance loops + bind offsets). Run either with `SRC`, `OUT_DIR`, `DDR_3D_DATA`, `DDR_3D_RLIST`,
`TEX_DIR` (+ optional `PREVIEW_ANM`, `PRESCALE`, `MODEL_SCALE`) in the environment.

* **Female characters use `pl_emi00` as the donor** (default in `port_lib.read_env`); its arm
  joints sit a few cm inward of Rage's — always read the joints from the donor
  (`load_ddr_rig` returns them), never paste the male table.
* **Pre-scale `S`** (source units → metres: 0.37 for the ~4.5-unit Teto, 0.08 for MMD's 8 cm
  units) sets the head and hand size — everything between two DDR joints is stretched to the DDR
  segment anyway (`conform` scales X/Z by `S` and Y by `target_len / rest_len`). The conform log
  prints each bone's stretch relative to `S`; torsos come out ~1.2×, limbs 0.75–1.0 on both models.
* **Teto (`port_character_rigify_glb.py`):** Blender 5.2's glTF importer crashes on a shape-key
  animation aimed at a mesh without shape keys — `strip_glb_animations` writes an animation-free
  copy first. Inverted-hull outline shells (`Edge_Col`, half the triangles) are deleted
  (`delete_faces_by_material`; the modpack draws its own outlines). Untextured flat-colour hair
  materials become one 2-band palette texture (`palette_texture` — `pack()`ed, or the pixels are
  lost on save — + `set_face_uvs` to the band centre; glTF `baseColorFactor` is linear, convert
  with `linear_to_srgb`). The rig has a chest-height torso pivot (`spine.001`) that is the PARENT of
  the upward `spine` bone — `conform` uses rest data for every direction/length, so parent-before-
  child processing cannot leak an already-moved neighbour into a stretch factor.
* **Miku (`port_character_mmd_fbx.py`):** the FBX binds no textures — `examples/pmx_dump.py`
  reads the PMX material table (texture per material, and flag bit 0 = double-sided; this model
  is single-sided). Standard MMD names (`上半身/首/頭/肩/腕/ひじ/手首/足/ひざ/足首/足先EX`, `.L/.R`);
  the twist bones `腕捩`/`手捩` are IN the parent chain (`腕 → 腕捩 → ひじ → 手捩 → 手首`), so the
  whole chain is placed by arc length on Arm → ForeArm → Hand (`map_chain_arclength`) with the
  real joints pinned. The trunk has no bone at the DDR Hips point: `下半身`/`腰`/`上半身*` take a
  linear z-map from the leg joints to the neck (`y_scale` = that map's slope for the identity-
  rotation pelvis bones). 27 facial shape keys are cleared before the bake; only `UVMap` is kept
  of the 7 UV layers; mmd_tools rigid-body/joint helpers are deleted. Skirt physics bones map to the
  `skirt` class (Hips above the hip joints, up to 60 % handed to the nearer thigh down the hem).
* **Previews:** `preview_renders` (front/¾/back + frames of any `mc_female_*_exec.anm`) is the
  pre-cabinet check; a `_face`/`_hand`/`_feet` close-up pass caught nothing on these two but is
  where a wrong UV layer or a dropped texture shows first.

### A rigidly skinned game rip in ASCII FBX 6.1 (`examples/port_character_gta_fbx6.py`)

GTA San Andreas' Carl Johnson (2026-09-26, ships as `data_mods/custom_models/dancers/Carl Johnson/`,
sidecar `cj00, pl, M, A, 1.0, 0.8, 0.0` — a MALE on the `pl_rage00` donor). Same `port_lib` flow;
what was different:

* **ASCII FBX 6.1** (3ds Max / FBX SDK 2011): Blender refuses it. `examples/fbx6_ascii.py` is a pure-
  Python reader (node tree, mesh, UVs, per-polygon materials, skin clusters, bind pose) and
  `examples/fbx6_gta_source.py` builds the armature from the clusters' `TransformLink` matrices (FBX
  global Y-up → +90° about X) plus the skinned mesh. Coordinates are Max Z-up inches facing −Y, like
  Blender. Its `MAIN_CHILD` table names the GTA/XNALara-style bones; adapt it for another skeleton.
  The rar needed `bsdtar` (p7zip 17 reported "Unsupported Method").
* **Floor-root weights:** 30 heel vertices were skinned to `root ground`; they are moved to the nearer
  ankle before the conform, or the shoe heels stay on the floor.
* **Waist:** the DDR hip joints sit ~2 cm/side wider than CJ's, so dragging the thighs out to them
  made the jeans flare over the tank top at the belt. The thigh targets keep the source socket width
  (the trunk's linear map of the source joint), and the belt band is re-baked with the trunk map,
  ramped back to the limb bake over 3 in below the hip joints. `waist_report` prints baked ÷ source
  width per height slice, and that ratio should read 1.00.
* **Short neck:** stretched only `NECK_K` = 1.15× toward the DDR Head joint. The rigid head then rides
  2 cm below the joint, which is invisible, instead of sitting on a long thin neck.
* **Rigid skinning** (every vertex 1.00 on one bone): wherever two neighbouring bones get different
  conform transforms, the bake tears along the weight border. Here the DDR Collar sits 2 cm outboard
  and 2.5 cm higher than the source clavicle, and the clavicle's rigid region runs down the shirt
  sides, which gave 4–6 cm tears under the arms and steps at the strap tops. Two `port_lib` helpers
  repair this, both on `weld_graph` (the edge graph with UV-split duplicates welded, keyed on the
  source rest positions):
  * `relax_displacement` re-bakes a region harmonically. D = bake − a smooth reference (the trunk
    map) is held everywhere else and relaxed over the free nodes (the clavicle vertices). Positions
    only.
  * `blend_weights_across` softens hard DDR weight borders after `retarget_weights`. Every edge
    whose ends have different dominant groups involving a Collar gets 2 rings of Laplacian
    smoothing. Otherwise the Collar 1.0 | Arm 1.0 edge at the shoulder pivot stretches 3–5× when an
    arm lifts, which shows as a pointy flap.

  `seam_report` in the config lists the largest displacement jumps across an edge, which is where
  a rigid bake tears.
* **Small textures are fine:** 256², 128², 128×256 and 64² load at native size (no upscaling).

### A dancer with ITS OWN rig and clips (`examples/port_character_ultramix.py`)

This is the Omnimix path: no donor rig, no conform and no weight retarget. The source skeleton, its
proportions and its own choreography are kept as they are. First used for *Dancing Stage
Unleashed* / DDR ULTRAMIX (Xbox), 2026-09-28. It ships as `data_mods/custom_models/dancers/Ultramix
Afro` and `Ultramix Lady`. The formats are documented in
`docs/dancing_stage_unleashed_dancers_port_feasibility.md`.

* **Rig.** Build it from the source bind matrices in GAME space (Y-up metres, facing +Z, left at +X)
  and set each edit bone with `edit_bone.matrix = convert.rowmat_to_blender(bind)`. Blender makes
  every bone's local Y point along the bone, but that does not matter: the clips are converted
  against the EXPORTED bind frames (`Q = B_exported · B_source⁻¹`), so any rigid re-framing leaves
  the skinning product unchanged. Export as usual. Any bone count works up to 52 per mesh palette
  and 64 posed bones per instance (`frame_board`).
* **Clips** go to `pl_<key>/motion/<clip>.anm`. Write them as LOCAL TRS against the exported
  parents: `local = world · world_parent⁻¹`, kind `0x1C` rotations and `0x1D` translations, flag 0
  like the stock `_exec` clips. When the source runs slower than the game, place its keys on
  EXPLICIT times, e.g. `[0, 2, 4, …]` for a 30 Hz source on the 60 fps timeline. That needs no
  resampling: the evaluator slerps between keys. Every clip is checked with `anm_dump.evaluate_pose`
  against the source world transforms (< 1 mm).
* **The DLL** plays a body that carries `motion/*.anm` members from that pool only, ignoring the
  sex pool (`selection::DancerCandidate::motion`). The sidecar sex then only labels the dancer.
* **Role bones.** The shadow, BIG HEAD and the part attach points look up `Hips`, `Spine2`,
  `Head`, `LeftToeBase`, `RightToeBase` and `LeftForeArmRoll` BY NAME in the body's `.b2it`. Append
  ALIAS entries to it (extra name, same index; `K.write_b2it` keeps the table sorted). The World
  engine never opens a body `.b2it`, only the DLL does.
* **Winding.** A source in D3D left-handed space needs a mirror (negate Z) and a reversed triangle
  order to land in the game's right-handed convention. Quaternions follow as `(−x, −y, z, w)`,
  handled implicitly by conjugating every frame with the mirror.
* **Sequels on the same rig (`examples/port_character_ultramix2.py`).** DDR ULTRAMIX 2 /
  *Dancing Stage Unleashed 2* reuses the K3D formats. It ships as the six
  `data_mods/custom_models/dancers/UMX2 *` folders. Three differences are handled:
  - Each model leaves out some ancestor joints (afro has no `root`, robo has no `Sternum` /
    `Clav_*1`, …). The missing joints and the role-bone joints are added as UNWEIGHTED helper
    bones, with binds copied from a sibling model. All six binds are one skeleton, and every clip
    animates every joint.
  - The texture and clip pool come from the rip's `default_model.csv` (NORMAL P1) and
    `animations.csv` (GROUP) tables.
  - Clips play whole (`loop_in = 0`).
* **DSU3 (`examples/port_character_ultramix3.py`).** *Dancing Stage Unleashed 3* has eight new
  dancers, which ship as `data_mods/custom_models/dancers/UMX3 *`. This port handles more
  differences:
  - A multi-material `.ddm` revision (cloth / face / pants / shoes). `parse_ddm` reads both
    revisions, and each slot becomes a World material with the COSTUME1 texture from
    `<COSTUME>.csv`.
  - Maya-style joint names (`M_Root`, `L_Knee`, …) with no toe joints. The role aliases are
    `M_Root` / `M_Chest` / `M_Head` / `L_Ankle` / `R_Ankle`.
  - Separate male (`M_*.ani`) and female (`F_*.ani`) clip sets, picked by the GENDER column.
  - Per-model bind origins. A helper bone's bind is shifted by the offset between the two rigs.

### A System 573 polygon dancer (`examples/port_character_sys573.py`)

The same Omnimix path for the arcade dancers of DDR 3rdMIX PLUS / 4thMIX PLUS / 5thMIX (Konami
System 573, 49 characters, 16 shared dance routines). All 49 ship as
`data_mods/custom_models/dancers/<N>MIX <Name>` (keys `ddr<3|4|5><chara>00`). `3rdMIX Afro` was
cabinet-tested 2026-09-29; the other 48 were ported the same day with the same checks. The
decoders and all conversion math are pure numpy in `scripts/sys573_dancer_dump.py`
(`world_bones`, `world_binds`, `world_mesh`, `world_atlas`, `routine_samples`,
`routine_to_anm_spec`). Formats and RE: `docs/sys573_dancers_research.md`. Extract a mix first
with `scripts/extract_sys573_data.py`.

* **Rig.** 31 bones: `root` (the routine's travel), the 16 joints and 14 HELPER bones. The 573
  draws one of 5 hand shapes per hand and one of 4 faces per frame. Here every alternate is in the
  mesh, each on its own helper bone, and the clip scales the hidden ones' helpers to 1e-3 with
  kind-10 scale tracks (key pairs one frame apart at each change). Helpers are leaves, so no
  segment-scale compensation reaches anything else.
* **Mesh.** One rigid-skinned mesh; every PSX object rides one bone. The texture is a 512×256
  atlas: the PSX 256×256 page on the left, and the untextured polygons' flat colours (as is: an
  untextured lit PSX primitive draws its RGB unscaled) as 16 px swatches on the right. It is upscaled 2× nearest-neighbour. PSX
  triangles are clockwise from outside; the triangle order is reversed.
* **Clips.** One `.anm` per routine, not per 573 clip: a 573 clip is ONE measure and only
  flows into its successor when chained. 120 frames per measure (World's dance clock runs at
  120 BPM under `bpm_sync`), keys every 2nd frame. A routine is 13–14 measures, 26–28 s at
  120 BPM. The 1-measure `normal_*` idles are skipped.
* **Root travel.** The 573 re-bases the root on the previous measure's end pose, so a routine
  wanders up to ~3 m and ends turned. `ROOT_MODE`:
  - `recentre` (default): the 573 path, shifted so its bounding-box centre is the dancer's mark;
  - `travel`: exactly as the 573 plays it;
  - `inplace`: root x/z translation removed; turns, hops and bobbing are kept.
  Each routine ends somewhere else than it starts, so the next clip starts with a jump.
* **Checks** per clip: joint positions of the written `.anm` against the 573 pose (< 1 mm; the
  proof of concept measured ≤ 0.11 mm) and every helper's scale against the 573 draw selection
  (0 mismatches). Model: codec round trip, unique bone identities, palette ≤ 52, ≤ 64 bones.
* **Batch.** `DANCERS=all` ports every model of every extracted mix (~7 min for 49). Labels are
  `3rdMIX` / `4thMIX` / `5thMIX` + the character name, ≤ 15 bytes (`display_name` drops a trailing
  mix-number marker: `afro4` → `4thMIX Afro`, `zukin5a` → `5thMIX Zukin A`). Labels are
  `<3rd|4th|5th> <Name>` (≤ 15 bytes) and keys `ddr<3|4|5><name>00`. The sidecar sex comes from
  a hand-made table in the script (it only affects the shadow scale and the label).

```bash
SYS573_DIR=~/Desktop/ddr_573_extracted DANCERS=3rdmix_plus/afro PREVIEW=1 \
  /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_sys573.py
```

### A DDR STRIKE / FESTIVAL / PARTY COLLECTION (PS2) dancer (`examples/port_character_strike.py`)

DDR STRIKE's 45 polygon dancers (22 characters in two costumes, plus the gold RHYTHM3) are the
System 573 engine's data on the PS2, so the port is the 573 one (`port_character_sys573.port_loaded`)
with a different loader. All 45 ship as `data_mods/custom_models/dancers/DDR Strike/<Name> <n>` (keys
`strk<name><n>00`), ported and round-trip previewed 2026-09-29; not yet cabinet-tested. RE:
`docs/ps2_ddr_filedata_research.md` §4. DDR FESTIVAL (`GAME=festival`, 26 dancers) and DDR PARTY
COLLECTION (`GAME=pc`, 60: the dancers of 1st..7thMIX and the CS mixes) are the same engine again
(RE §6.1) and ship as `dancers/DDR FESTIVAL/<Name> <n>` (keys `fest<name><n>00`) and
`dancers/DDR PARTY COLLN/<Name> <mix>` (keys `pc<name><mix>00`), ported 2026-10-03; not yet
cabinet-tested. Neither disc has 3D stages.

* **Input.** The extraction of `scripts/extract_ps2_ddr_data.py extract strike_jp | festival_jp |
  party_collection_jp <disc> <out> --unpack` (`PS2_DIR`; `STRIKE_DIR` still works for STRIKE):
  `unpacked/<id>/001.cmd` meshes and `000.tcb` textures, the 8 `unpacked/<set>/*.cmm` motion
  sets, and `elf/chara.lst` / `chara20.lst` / `chara.pos`. `GAMES` in the script has each disc's
  ids and table address.
* **Differences from the 573 port.**
  - The texture is a 192×256 8 bpp TCB at the left of the 256² page the UVs address. Palette
    alpha 0 is transparent and anything else opaque.
  - The motion files use the PS2 key-block layout, which `sys573_dancer_dump.parse_cmm`
    detects. The 16 routines are the 5thMIX set; each is taken once from the 8 sets.
  - A 20-object mesh (six of Party Collection's) uses `chara20.lst`: one hand shape per hand, so
    the rig has no hand helpers (21 bones).
  - Names, the sex call and the model scale come from the game's character table, hard-coded in
    the script and checked against the ELF when it sits beside the extraction. The table's
    scale is uniform: BABY-LON is 0.4 and AKIRA 1.06. It goes into the sidecar's model_scale
    column. The sex comes from the record's motion-set list (male / female routine sets);
    STRIKE's is judged from the models.
  - The script writes `OUT_BASE/<source>/<label>/` directly: `DDR Strike`, `DDR FESTIVAL`,
    `DDR PARTY COLLN`. Labels are the game's names with the costume digit (`Blues 1`) or the mix
    (`Afro 1st`), shortened to 15 bytes where needed: BABY-LON → Baby-Lon, PRINCESS-ZUKIN →
    P-Zukin, OSHARE-ZUKIN → O-Zukin, ROBO2001 → Robo (STRIKE).
* **Checks.** They are the 573 port's. The worst joint error over all 131 × 16 clips is
  0.11 mm, and a re-run is byte-identical. The clips depend only on the object table: every
  28-object dancer of the three discs has byte-identical `.anm` files, and the six 20-object ones
  share a second set.

```bash
DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_strike.py      # ~6 min for 45
GAME=festival DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_strike.py      # ~4 min for 26
GAME=pc DANCERS='AFRO(1st),BUS(7th)' /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_strike.py      # names as in the table
```

### A DDR SuperNova / SuperNova 2 / X / X2 (PS2) dancer (`examples/port_character_supernova.py`)

SuperNova's eight dancers (AFRO, BABYLON, EMI, GUS, JENNY, RAGE, ROBOZUKIN, RUBY) are a new,
XSI-exported engine: skinned strip meshes with up to three weights per vertex on a 22-joint
HumanIK-named skeleton, and 30 Hz quaternion routines. Decoders and the World-space math are in
`scripts/tzm_dump.py`; formats and RE in `docs/ps2_ddr_filedata_research.md` §7.4. Ported
2026-09-30 and round-trip previewed (all eight re-import and render, rest and mid-clip). Staged as
one folder per character (`Afro`, `Baby-Lon`, ...; keys `sn<name>00`) under `OUT_BASE` (default
`~/Desktop/SuperNova Dancers`); shipped under `data_mods/custom_models/dancers/DDR SUPRNVA 1+2/`
as `Afro 1` .. `Ruby 1` beside the SuperNova 2 content below.

**SuperNova 2** (`GAME=sn2`, 2026-10-03): twelve characters × two costumes (`<skin>01` / `02`:
the eight above + YUNI, ALICE, CONCENT, JULIO) dancing the same 29 routine packs byte for byte.
The costume-01 bodies of the returning eight ARE the SuperNova skins (same sheets — EMI's and
ROBOZUKIN's recoloured — same rigs, same geometry), so `DANCERS=all` ports the sixteen that are
new (every costume 02 + both costumes of the four new characters; `DANCERS=afro01` still works),
labelled `<Name> 2` / `<Name> 1`, keys `sn2<skin>`. Two things changed in the packs:
* the body meshes have **no face** — the eyes / mouth triangles moved to `<skin>_face.TZM`, an
  unskinned mask (`face01` neutral, `face02` smile, `face03` eyes shut; three 128² sheets) hung
  off the Head joint through a `trans_null` whose rotation is the Head bind rotation's inverse.
  `tzm_dump.face_overlay` puts the chosen sheet (`FACE`, default `face01`) into the body's game
  space (`Head_bind · W_object · v`, verified on all 24 packs) and the port joins it to the body
  mesh as its own material slot (`sn2<skin>_face`), weighted 1.0 to `Head`;
* the root node is named after the pack (`afro01`, `concent01`) instead of `globalSRT` while the
  clips still carry the original rig's static root track — `tzm_dump.clip_worlds` matches the
  rig root by position (CONCENT's root carries a 0.41 pose offset the game discards; with it the
  character would hover 4 cm). JULIO 1 (`SCALE` 0.9) and BABY-LON 2 (0.4, a chibi) fold their
  scale like BABYLON. Not ported: the two other expressions.
* CONCENT's chest fan (`body01 > body_trans_null > fan01` in its face pack, hung off `Spine1`,
  spun -720° per 4 s loop by the pack's own `ddr_concent_fan` record) is ported SPINNING: the fan
  object becomes a 24th joint `fan01` under `Spine1` (`tzm_dump.attach_part_bone`: pose = the
  chain below the pack root, bind = Spine1's bind × that chain — exactly where
  `tzm_dump.part_overlay` puts its vertices, which weight 1.0 to it, slot `sn2<skin>_body01`), and
  every dance clip gets a `fan01` rotation track = the chain's static tilt × the spin sampled at
  the clip's own key frames, wrapping over the 240-frame loop (`tzm_dump.part_spin_track`). The
  `.anm` evaluates to one revolution per 2 s relative to Spine1. `SPIN=0` joins it static instead.

**DDR X / X2** (`GAME=x` / `GAME=x2`, 2026-10-03): the same engine, routines and face-pack layout
once more (RE doc §7.6). Both games bundle SuperNova 2's skins (X's costumes 02 / 03, X2's 03) and
add their own — X's costume 01 for all twelve characters, `babylon02`, BONNIE, ZERO; X2's costume
02 (X's 01 bodies on new sheets), `bonnie02` / `zero02` and the four PIX pigs (`SCALE` 0.45).
`DANCERS=all` ports only what SuperNova 2 does not have (15 + 18), and the two games ship into ONE
source folder, `data_mods/custom_models/dancers/DDR X + X2/` (`Afro 1` = X's 01, `Afro 2` = X2's
recolour, `Baby-Lon 1/2` X, `Baby-Lon 3` X2, `Pix 1..4`; keys `x<skin>` / `x2<skin>`). The X
tables carry what SuperNova 2's did not: the per-costume face root (`face01`; PIX
`jx_pixNN_face1`) and face pack (X2's bonnie02 / zero02 reuse the 01 masks), the sex flag (PIX
dances a mixed MM/FF six), the game's own shadow scale (+0x8C: BABYLON / PIX 0.35 .. CONCENT
0.85 — the sidecar's), and CONCENT's fan from `parts/convent01_body01.tzm` on `Spine1`
(`x<skin>_body01`). Not ported: the DISK-A/B/? ring mannequins (`wakka_*`, zero-area strips) and
X2's `dmm/` board-game chibis (another rig). Routine lists are long here — 12–14 per character.

* **Input.** The extraction of `scripts/extract_ps2_ddr_data.py extract supernova_jp |
  supernova2_jp <disc> <out>` (`SN_DIR`): `files/IMAGE/model/chara/skin/<name>.TZM` and
  `chara/motion/<clip>.TZM`.
* **Rig.** The TZM's own bones, bind = the file's GLOBAL bind pose (`T2`, `R2`), in game space
  through one scale (`tzm_dump.GAME_SCALE` = 0.970 / 9.655: the Hip at World's Hips height; the
  TZM frame already faces +Z with its left at +X, so no mirror). BABYLON's `SCALE` node (0.6) is
  folded into that scale and dropped. The exported rest pose is the T-pose lifted onto the floor.
* **Mesh.** Every mesh of every object joined into one, object transforms applied (AFRO's muffler
  is authored in its own frame), the file's normals via `ddr_normal`, strip winding made
  consistent with the normals (the GS never culled, so the strips face either way), the 512² CLUT
  sheet as `sn<name>_tex`. Vertex colours are kept where a mesh has them: GUS's 60 %-alpha glasses
  go to a second, alpha-blended material slot (`MESH_FLAG_TRANSPARENT`).
* **Clips.** The character's OWN routine list from the ELF character table (AFRO jazz + soul-funk,
  RAGE break + hip-hop + house, ...; the 4 s `*_NE_01` idles are left out), one `.anm` per
  routine, 30 Hz keys on even frames of the 60 fps timeline (the clips are authored at 120 BPM
  like World's — `MM_NE_01` is 241 frames, `mc_*_ne01_loop` 242). Each clip is checked against
  the TZM pose (< 0.15 mm per joint). Role aliases `Hips` / `LeftToeBase` / `RightToeBase` →
  `Hip` / `LeftToes` / `RightToes` (`Spine2` and `Head` are named alike).
* **Sidecar.** `sn<name>00, pl, <sex>, A, 1.0, <0.75 F / 0.8 M>, 0.0`; the sex is the routine
  family's (`FF_*` / `MM_*`, which is what the table's flag says too).

```bash
DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~25 s for 8
GAME=sn2 DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~60 s for the 16 SuperNova 2 ones
GAME=x DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~2 min for the 15 X-only ones
GAME=x2 DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~2 min for the 18 X2-only ones
```

### The DDR SuperNova / SuperNova 2 / X (PS2) stages (`examples/port_stage_supernova.py`)

SuperNova's 20 stages (+ `system_bg001`; SuperNova 2 ships the same packs byte for byte and adds
only `system_bg002` — `GAME=sn2`, 2026-10-03) are XSI scenes: objects grouped under blend-layer roots
(`dec` opaque, `add` additive, `sub` subtractive, `glo` glow, `ble` alpha), coloured strip meshes
(format 0x152), a `stageNNN` MOTION record with SRT tracks on the animated parts (a 4 s / 8-beat
loop at 60 fps, 8 s at 29.97) and material fcurves, and a `cameraNNN` record with ten shots + a
neutral one. World's own stage parts carry the same layer names (`gm_dawnstreet00_{dec,ble,glo}` —
`dawnstreet00` is DDR X's `st005`), so one layer becomes one part with the stock flag conventions.
Ported 2026-09-30 (geometry, object loops, material animation, cameras) and round-trip previewed
(every part re-imports; the Workbench render shows the additive layers opaque); not yet
cabinet-tested.
Staged as `Stage 01` .. `Stage 20` + `System BG 1` (keys `snstage001`.., `snsystembg001`) under
`OUT_BASE` (default `~/Desktop/SuperNova Stages`; `GAME=sn2`: `System BG 2`, `snsystembg002`, under
`~/Desktop/SuperNova 2 Stages`); shipped together under `data_mods/custom_models/stages/DDR SUPRNVA
1+2/` (the source folder supplies the label prefix).

**DDR X** (`GAME=x`, 2026-10-03): six stages of its own (`stage001..006`; X2 ships them again plus
SuperNova 2's `system_bg002` byte for byte, so there is no `GAME=x2` here), the same layer / record
layout with 29.97 fps records on 001 / 002 and camera records named `jx_stNNN_cam_FIX2`
(`Camera_001..008`, `Camera_neu`, `Camera_non_chara01..04` → `_st01..08`, `_non01..05`, then the
shared close-ups `_non06..15`; `is_camera_record` now classifies by content). `Stage 01` is read
from `stage001_2play.TZM` — the 1P pack plus the eighteen beat-pulsing speakers the game overlays
from `stage001_speaker.TZM`, same cameras; the other `_2play` packs are reduced two-player dressings
and are skipped. Shipped as `data_mods/custom_models/stages/DDR X + X2/Stage 01..06` (keys
`xstage001..006`, texture stems `x001...`). **Stage 02's TVs are movie screens**: X drew its
sub-monitor feed on the `Render*` objects (`RenderBIGTV`, `RenderSBTOPTV`, `RenderSBTVa/b/c`), so
the port textures them `offscreen1` (the "Stage screens" section above — World's STAGE SCREENS mode
then plays the song's movie on them), remaps each surface's authored v band onto the square's 16:9
band (0.21875–0.78125; u already runs left to right as seen), paints them white and skips material
animation on them; `RenderBIGTV2`, a coplanar glass over the big TV with a gradient placeholder, is
dropped. `CFG['screens']` / `CFG['drop']` carry the per-game object-name rules.

* **Parts / flags.** `bg` (the skydome subtree of `dec`, `:-2`) and `dec` two-sided opaque
  `0x0001`; `ble` alpha `0x02C1` (`:-1`); `add` additive `0x06C1` + `flags2 4`; `sub` subtractive
  `0x06C1` + `flags2 8`; `glo` = an opaque copy with the `_t` sheet PLUS an additive copy with the
  `_g` glow sheet. Two-sided everywhere because the GS never culled. Vertex RGBA colours are kept
  (`_vc`), the MATERIALLIST maps each mesh's (0x18-byte, truncated) material name to its textures.
* **Rig / loop.** Per part a `root` bone plus one FLAT bone per animated object (a mesh's anchor
  is its deepest animated ancestor; the static sub-chain is baked into the vertices; bind = the
  rigid part of the anchor's rest world; a zero rest scale — flattened decals — is baked too).
  `gm_<key>_<part>_play_loop.anm` (loop bit) carries q / t / relative scale per key with a wrap
  key, checked against the TZM object worlds (rotation < 3e-4, translation < 1e-4 relative). Flat
  bones side-step World's segment-scale compensation.
* **Foot panel.** `FOOTPANEL=1` adds `model/footpanel.TZM`'s unlit `ftpnl` mesh as `footpanel`
  (texture `snfootpanel`; its arrow layout matches the stock `gm_boom00_footpanel`). Off by
  default: stock World stages carry that part only on the lesson-only `boom00`, and the shipped
  SuperNova stages dropped theirs.
* **Material animation.** The `stageNNN` record's material fcurves (`tzm_dump.material_animation`:
  kind 503 texture translation, 504 colour, 1302 glow strength — RE doc §7.4) become one
  `gm_<key>_<part>_play_loop.sanm` per part in World's own material-clip layout (`anm_dump.write_anm`
  `material_tracks` / `material_targets`, format doc §7): one kind-8 key per record frame plus a
  wrap key, on parameter floats 2 / 3 (`m_vTexAnime` offU / offV — the `_uvani` conveyors, water and
  light strips scroll), 4..6 (`vConstatntColor` rgb: the 504 pulse on the base pass, the 1302 glow
  on the `_g` additive copy). Materials driven on 4..6 are exported with `mdl_ch_constant_c_vc` and
  their frame-0 values in `ddr_params` (`mat["ddr_sn_material"]` / `["ddr_sn_pass"]` record the
  source). The hook DLL samples the `.sanm` on the stage clock into the part's private material
  copies (`src/core/anm/sanm.rs`); without that build the model shows the frame-0 state.
* **Cameras.** Each camera record's `Camera_001..010` → `camera/<key>_st01..10.camanm` (the
  director's main rotation), `Camera_neu` → `<key>_non01`, and — `CHARA_CAMERAS=1` (default) — the
  shared `stage_chara_camera.TZM` close-ups → `_non02..11` (`tzm_dump.camera_to_camanm_spec`): one key
  per record frame at 60 fps (`60 / fps` apart: 4 s clips, 8 s for the 29.97 fps stages), position
  × `GAME_SCALE` in cm, the look-at orientation with roll, near 0.1 / far 32768 / aspect 1.333, and
  the FOV through the inverse of the game's projection. SuperNova's kind-7 FOV is XSI's HORIZONTAL
  angle of the 4:3 frame (0.93616 rad = 53.638°, XSI's default — and exactly A3's stock 41.53°
  vertical at 1.333); `FOV_KEEP=vertical` (default) keeps that vertical extent on 16:9 (game hFOV
  68°: what SuperNova showed top to bottom stays in frame, the sides widen), `horizontal` keeps the
  horizontal extent (top / bottom cropped). Static shots collapse to single keys. With `PREVIEW=1`
  the stage and a ported dancer (`PREVIEW_DANCER`, default the SuperNova `Afro 1` under
  `data_mods/custom_models/dancers/DDR SUPRNVA 1+2/`) are rendered through the written clips as the game projects them
  (`import_anm.load_camanm`): `<key>_cam_non01_f0.png`, `<key>_cam_st01..03_f<mid>.png`.
* **Sidecar** `map_resources.rlist.txt`: `<key>, 000000, 000000, bg:-2, dec, glo, add, sub, ble:-1`
  (present parts; `footpanel` last when `FOOTPANEL=1`).
* **Not ported:** the `_conf.PTF` lighting; the material diffuse colour (0.7 grey on 19 materials —
  whether SuperNova multiplies it into the draw is untraced); a texture scale / rotation fcurve
  would be reported as UNSUPPORTED (none exist).

```bash
STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_stage_supernova.py   # ~40 s for 21
GAME=sn2 STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_stage_supernova.py   # system_bg002 only
GAME=x STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_stage_supernova.py   # ~1 min for the 6 DDR X stages
```

### The Dancing Stage / DDR HOTTEST PARTY (Wii) dancers (`examples/port_character_hottest.py`)

Hottest Party runs on Hudson's Mario Party engine, so its models are **HSFV037** (decoder and
World-space math: `scripts/hsf_dump.py`; formats and RE: `docs/wii_ddr_hottest_party_research.md`).
Extract the disc first with `scripts/extract_wii_ddr_data.py extract <unpacked disc> <out> --png`
(`HP_DIR`). Ported 2026-10-03 and round-trip previewed; not yet cabinet-tested. **Retired
2026-10-04**: MUSIC FIT remakes this cast on its own rig, and the shipped HOTTEST PARTY dancers are
that port (`HOTTSTPARTY 1-3`, next section; this one stays as the HSF reference). The 40 were
`<Label> <costume>`, keys `hp<stem><costume>` (who they are: the research note §6):
- Emi / Jenny / Afro / Rage;
- `Dancer A..D` (the four new characters, models `hispanic`, `black_f`, `korea_m`, `jamaika`;
  neutral labels because nothing on the disc maps the EU names to them);
- `Backup F / M`.

Each comes in costumes 1..4.

* **Rig.** All 40 models share one 26-joint Maya skeleton (Hips .. `Head*end`, `*Wrist*end`,
  `*Toe*end`; `*` becomes `_` in bone names).
  - MayaConverter's `<J>*root` / `<J>*leaf` helper objects are identity and are dropped
    (`hsf_dump.rig_joints`, parents first: the file lists children first).
  - The file frame is World's. One scale (`hsf_dump.GAME_SCALE`) puts the Hips at 0.97 m.
  - Role alias `Spine2` -> `Spine1`.
* **Mesh.** One vertex per distinct (position, normal, colour, st) corner, since HSF indexes them
  separately.
  - Envelope weights (single / dual / multi, up to 5 influences; the exporter keeps 4).
  - Two-sided (the materials carry NOCULL). Winding is made consistent with the normals, because
    GX's (0, 2, 1) corner order is mirrored.
  - A 512^2 body sheet plus the 128 x 64 open-eyes sheet (the game swaps in blink sprites).
* **Clips.** Every character dances ONE library: `data/c_000.bin`'s 256 clips.
  - Pieces whose end pose is the next one's start are joined into takes (`hsf_dump.chain_clips`).
    The 34 takes of >= 3 bars are kept; "Lesson by DJ"'s step demonstrations are left out.
  - Each piece's length in bars comes from the dance viewer's table (`dll/danceviewDll.rel`,
    `extract_wii_ddr_data.danceview_clip_bars`). The library mixes 120 / 145 / 177 / 70 BPM
    takes, and each is retimed so a bar is 120 frames (World's `bpm_sync` clock).
  - Keys every 2nd frame; Hips x/z re-centred (`ROOT_MODE`); < 0.1 mm per joint.
  - The takes are **dealt** across a character's four costumes (~154 bars, ~1.7 MB each), and
    the four together dance all 617.5 bars. Giving every dancer the full library would take
    ~6.9 MB each.

```bash
DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_hottest.py   # ~14 min for 40
DANCERS=emi PREVIEW=1 ...                                                # one character's 4 costumes
```

### The Dancing Stage / DDR HOTTEST PARTY (Wii) stages (`examples/port_stage_hottest.py`)

42 stages (`data/stgNN.bin`, minus the `stg05` test stub and its three copies) ship as
`data_mods/custom_models/stages/HOTTEST PARTY 1/Stage NN` (keys `hpstageNN`). Ported
2026-10-03 and previewed through their own cameras; not yet cabinet-tested.

* **Input.** A pack is a list of (model, motion) pairs: the BG backdrop, the floor pieces and
  the props, each with its own loop of 180..6000 frames. Then come the dancers' `chr*` / `look*`
  formation markers (not ported) and six camera motions.
* **Colour is COLOR0.** Most stage textures are white alpha masks; the vertex colours carry the
  colour (vtxMode 5). The `_vc` shader multiplies them in. A Workbench render shows them white,
  so `preview` rewires the materials to emit texture x vertex colour (EEVEE).
* **Blend groups.** ADDCOL -> `add` (0x06C1/4), INVCOL -> `sub` (0x06C1/8), a translucent pass
  with real partial alpha -> `ble:-1` (0x02C1), else `dec` alpha-tested. The backdrop model's
  opaque meshes -> `bg:-2`. All meshes are two-sided.
* **Parts.** Models sharing a loop length, or one dividing it (sampled at `t mod L`), share a
  part per blend group. A part is split at 63 animated anchors (frame board: 64 bones per
  instance). That keeps stages at 2..9 parts.
* **Rig and loops.** One flat bone per animated anchor (the SuperNova stage scheme).
  - The geometry is baked at the frame-0 pose: a constant HSF track re-poses its object.
  - `_play_loop.anm` keys every 2nd frame plus a wrap key.
  - `_play_loop.sanm`: UV scroll = -(attribute T), unwrapped across the repeats of a shorter
    loop, on params 2 / 3; litColor tracks on 4..6 (`mdl_ch_constant_c_vc`).
  - **The curve evaluator must scan, not bisect.** MayaConverter writes pre-roll keys at negative
    times after key 0 (`hsf_dump.sample_curve`).
* **Cameras.** The pack's shots -> `_st01..06`; `data/ddrcam.bin`'s 59 dance cameras ->
  `_non01..59`. Position / aim / roll; the FOV is vertical, its extent kept on 16:9.

```bash
STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_stage_hottest.py   # ~40 min for 42
```

### The DDR FuruFuru Party / MUSIC FIT (Wii JP = HOTTEST PARTY 2 / 3) dancers (`examples/port_character_hottest2.py`)

Both games run on Konami's `zan` library (decoder `scripts/zan_dump.py`; formats, Ghidra findings
and the naming evidence: `docs/wii_ddr_hottest_party_2_3_research.md`). Dump the discs with
`scripts/extract_wii_ddr_data.py disc` (`HP3_GAME` / `HP2_GAME`). MUSIC FIT re-ships FuruFuru
Party's whole cast and remakes HOTTEST PARTY 1's in their HP1 outfits, so ONE source folder
`data_mods/custom_models/dancers/HOTTSTPARTY 1-3/<Name> <n>` (keys `hp<person><nn>`) holds all
137 from MUSIC FIT's costume files: the eight leads, NAOKI / U1 / jun, Dyna / Bridget / Ceja and
the eight back-ups Pia, Gliss, Forte, Sharp, Bossa, Nova, Hip, Hop (a back-up costume file holds
two of them: variants 1/2 and 3/4). A person's variants run HP1 outfit → HP2 → HP3.

* **Rig / mesh** as the HP1 port: one 37-bone rig rebuilt from the rest worlds, the head rigid on
  `mii_head`, skin weights by joint name, the eye / mouth overlay layers baked into the face
  texture (`zan_dump.bake_overlay`), accessories on their joints, role alias `Spine2` -> `Spine1`.
* **Clips.** One library: MUSIC FIT's song motions plus the FuruFuru Party pieces it lacks (1022
  ~8-bar clips), retimed to 120 frames a bar, dealt 12 per dancer.

```bash
DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_hottest2.py   # ~10 min for 137
DANCERS=pia PREVIEW=1 ...                                                 # one person's costumes
```

### The DDR FuruFuru Party / MUSIC FIT stages (`examples/port_stage_hottest2.py`)

`stages/HOTTEST PARTY 2/Stage NN` (FuruFuru Party, 59, keys `hp2stageNNN`) and `stages/HOTTEST
PARTY 3/` (MUSIC FIT's 17 own stages; its STG000 / 041–055 are FuruFuru Party's again and ship
once). The part / flat-rig / camera scheme is the HP1 port's; what is new:

* **Movie screens.** The `root` quad of a `*_MOV*` prop is where the Wii plays a stage movie or
  the song's PV: textured `offscreen1` ("Stage screens" above), v remapped onto the 16:9 band.
* **Texture flip-books -> atlases.** World's `.sanm` animates shader parameters, never the
  texture, so a material cycling TPL images gets its frames side by side along the axis that does
  not scroll (wrap gutters, triangles clipped at that axis' tile lines into one cell) and the UV
  offset steps from cell to cell. Steps are two keys on one frame (the sampler takes the later),
  so nothing blends.
* **UV scrolls** are the texture matrix's translation `(-u, +v)` of the zan keys (per-axis key
  counts and holds: `zan_dump.sample_uv`). Every part with animated materials gets a `.sanm`
  with its own clip length, static parts too; parts split at 48 animated material floats.
* **Binds** are the nearest rotation of the anchor's rest world and the keys `bind · rest⁻¹ ·
  world(t)`; a node rotating under a non-uniformly scaled parent shears, which TRS bones cannot
  carry (logged `SHEAR`, ≤ 0.55 m at the tips of STG049's light cones).

```bash
GAME=hp2 STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_stage_hottest2.py   # 59 stages; GAME=hp3 for 17
```

### A room / stage from a .blend (`examples/port_room_stage.py`)

1. Evaluate every mesh with its modifiers (`bpy.data.meshes.new_from_object`), bake the
   world transform and the room transform (`Scale(s) · Translation(−origin)`), join into
   one object. Pick `s` from real-world cues (doors ≈ 2 m, couch back ≈ 1.2 m — the DDR
   dancer is ~1.7 m) and put the spot the dancer stands on at the origin; the dancer
   faces −Y (Blender).
2. **Plain-colour materials → one palette texture**: an 8×8 grid of 16 px swatches in a
   128×128 image, each material's sRGB base colour in one swatch, every face's UVs pinned
   to its swatch centre (`v_blender = 1 − v_file`); materials with images keep them,
   resized to power-of-two. Rebuild the material list afterwards and re-apply the
   polygon indices — `mesh.materials.clear()` resets every `material_index` to 0.
3. White colour attribute, two-sided materials (`use_backface_culling = False`) so
   single-sided interiors stay visible; culling ON for a ceiling gives a see-through
   ceiling for high cameras.
4. `export_model.export_model(path, None, [room])` → `gm_<stage>_<part>.model`
   (37 k triangles in one part loads fine).
5. **Stage registration:** `StageActor(index)` walks `map_resources.rlist` to row
   `index` (the song's `<bgstage>` in `musicdb.xml`); the row key names the arc
   (`data/arc/mapset_<key>.arc`, `mapset_<key>_g.arc` for the lesson demo on a gold
   cabinet) and its parts become `gm_<key>_<part>`. Append a row (`griffin00 →
   ['000000','000000','room','footpanel']`), append the same index to
   `stage_camera_resources.rlist`, set the song's `bgstage` to the new index, and copy the
   stock `gm_boom00_footpanel` model + `footPanel.dds` (`g/footPanel.dds` in the `_g` arc)
   renamed to `gm_<key>_footpanel` for the dance pad. Ship both arcs.
6. **Camera:** the stock lesson camera spends most of the demo 2–5 m behind the dancer,
   which lands inside a couch — author a `.camanm` for the room instead (keyframed camera,
   `export_anm.export_camanm(cam, 0, 6238)`; two keys one frame apart = a hard cut;
   Blender ≥ 4.4 has no `action.fcurves`, set
   `preferences.edit.keyframe_new_interpolation_type = 'LINEAR'` before keying) and pack
   it as `camera/camera_music_<song>.arc` — the song set wins over the stage set.

## Conventions (see `convert.py`)

Game: right-handed Y-up metres, row-vector matrices, quaternions `(x, y, z, w)`.
Blender: Z-up. One +90° rotation about X converts world quantities; bone and
camera local frames are kept as-is (the game camera already looks down −Z with
+Y up, like Blender's). Blender re-orders `armature.bones` after edit mode, so
bones are always addressed by name; `arm["ddr_bone_order"]` keeps the file order.
Character PARTS are the exception: their mesh data stays in the file's raw
coordinates (the attach bone's frame) and the bone-parented object supplies the
axes — that is how the game composes them.

## Not yet done

* An arc-packing step inside the add-on (today: `scripts/arctool`, see Delivery above).
* A stage (`map_resources.rlist`) export helper — the rlist writer exists.
* A DXT encoder (uncompressed textures are accepted, just larger).
