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
  a 4:3 one top and bottom; songs without a movie show a black screen.

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
  atlas: the PSX 256×256 page on the left, and the untextured polygons' flat colours (PSX `0x80` =
  1.0, so doubled) as 16 px swatches on the right. It is upscaled 2× nearest-neighbour. PSX
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

### A DDR STRIKE (PS2) dancer (`examples/port_character_strike.py`)

DDR STRIKE's 45 polygon dancers (22 characters in two costumes, plus the gold RHYTHM3) are the
System 573 engine's data on the PS2, so the port is the 573 one (`port_character_sys573.port_loaded`)
with a different loader. All 45 ship as `data_mods/custom_models/dancers/Strike <Name><n>` (keys
`strk<name><n>00`), ported and round-trip previewed 2026-09-29; not yet cabinet-tested. RE:
`docs/ps2_ddr_filedata_research.md` §4.

* **Input.** The extraction of `scripts/extract_ps2_ddr_data.py extract strike_jp <disc>
  <out> --unpack` (`STRIKE_DIR`): `unpacked/<id>/001.cmd` meshes and `000.tcb` textures,
  `unpacked/0ca9..0cb1/*.cmm` motion sets, and `elf/chara.lst` / `elf/chara.pos`.
* **Differences from the 573 port.**
  - The texture is a 192×256 8 bpp TCB at the left of the 256² page the UVs address. Palette
    alpha 0 is transparent and anything else opaque.
  - The motion files use the PS2 key-block layout, which `sys573_dancer_dump.parse_cmm`
    detects. The 16 routines are the 5thMIX set; each is taken once from the 8 sets.
  - Names, the sex call and the model scale come from the game's character table (ELF VA
    0x2B0410), hard-coded in the script. The table's scale is uniform: BABY-LON is 0.4 and
    AKIRA 1.06. It goes into the sidecar's model_scale column.
  - Labels are `Strike ` + the game's name with its costume digit. Three names are shortened
    to fit 15 bytes: BABY-LON → BabyLon, PRINCESS-ZUKIN → P-Zukin, ROBO2001 → Robo.
* **Checks.** They are the 573 port's. The worst joint error over all 45 × 16 clips is
  0.11 mm, and a re-run is byte-identical. All 45 dancers share one rig, so their `.anm` files
  are identical.

```bash
DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_strike.py      # ~6 min for 45
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
