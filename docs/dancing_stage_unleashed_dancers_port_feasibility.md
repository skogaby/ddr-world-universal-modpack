# Dancing Stage Unleashed (Xbox) Dancers → DDR World — RE Notes & Port Feasibility (2026-09-28)

**Question.** Can the two 3D dancers of *Dancing Stage Unleashed* (the PAL release of DDR
ULTRAMIX, Xbox 2004), their animations and their toon shader be ported into DDR World through
this repo's Background Dancers pipeline, Blender add-on and shader synthesis? If so, what is the
approach?

**Status (2026-09-28): Path B and the DSU-exact cel/outline are IMPLEMENTED and cabinet-validated**
(`.agents/planning/2026-09-28-dsu-dancer-port/`; the dancers ship as
`data_mods/custom_models/dancers/Ultramix {Afro,Lady}`, built by
`tools/blender_ddr_addon/examples/port_character_ultramix.py`). CEL SHADING and SCENE OUTLINES now
use DSU's toon ramp and black hull for the whole scene. The rest of this note is the original
research.

**Scope.** RE of the game rip plus a feasibility and approach write-up. Decoders were written and
checked, and one Blender verification render was made (§3.4). Nothing is implemented in the DLL,
and nothing has been cabinet-tested.

**Sources.**
- The disc rip is at `~/Desktop/dancing_stage_unleashed/`. Assets were unpacked from
  `x_data_UK.bin` by `scripts/extract_ultramix_data.py` (config `ultramix_uk`) into
  `extracted_full/`.
- `dancing_stage_unleashed_default.xbe` was analysed in Ghidra 12 headless, with the `ghidra-xbe`
  loader, in a scratch project (the GUI project was left untouched).
- New decoder: `scripts/ultramix_k3d_dump.py`. It covers `.ddm`, `.ani`, `.xpu`, the embedded VS
  sources, CPU skinning to `.obj`, the hierarchy check and the same-take survey. §10 lists every
  command that reproduces a claim here.

**Address convention.** XBE code and data addresses are **virtual addresses** (image base
`0x10000`), as Ghidra shows them. Sections map to file offsets as follows:

| Section | VA | File offset |
|---|---|---|
| `.text` | `0x11000` | `0x1000` |
| `.rdata` | `0x1B1980` | `0x1A3000` |
| `.data` | `0x1D5420` | `0x1C7000` |

Constants read by the code are quoted with the VA they live at.

## TL;DR

**Feasible. Most of the work is content work the repo already has a proven playbook for.**

1. **All the formats are decoded and verified end to end.**
   - The dancer data is two models (`afro.ddm`, `lady.ddm`), 27 `.ani` clips, one DXT1 512²
     body texture each plus an eyes-closed **blink** texture, and a 2-band toon ramp (`toon.tga`).
   - `.ddm` and `.ani` are small flat formats (§2, §3). The decoded meshes, skinned by the game's
     own formula, render correctly (§3.4).
2. **The choreography is already in DDR World.**
   - 23 of the 27 DSU clips are **the same motion-capture takes** as World's stock
     `mc_male`/`mc_female` `_exec` clips.
   - DSU stores them at **30 Hz** (World: 60 Hz), trimmed to about 19 s.
   - Every tested joint's trajectory correlates at 0.97–0.998 after 2:1 decimation (§6).
   - Consequence: a DSU dancer rigged to World's stock skeleton performs DSU's dances with **no new
     animation work and no DLL change**, at twice DSU's frame rate.
   - Only `lady_hiphop01`, `lady_house02` and `lady_house03` have no World counterpart.
     `lady_house01` matches only the dead `ne01_loop` data.
3. **The toon shader is a strict subset of the repo's CEL style.** It is:
   - texture × a hard 2-band ramp on per-vertex N·L (0.557 below 0.5, 1.0 above), × light colour;
   - × 0.5 inside a self-shadow map;
   - plus a black inverted-hull outline (§5).

   Everything except the shadow map ports as a `DSU_TOON` define on `shaders/src/mdl_cel.hlsl`
   plus a variant column in `shader_layout`. The shadow map is not portable at reasonable cost:
   World's model pass binds no light matrix and has no depth pre-pass.
4. **The only real design decision is proportions.**
   - DSU's skeleton is its own. Its joint names are custom and it has longer upper arms and
     bigger feet (§6.2).
   - The stock-rig route (**Path A**) forces World's donor proportions, because stock clips carry
     translation tracks for every bone.
   - Keeping DSU's proportions takes either a small pure-Rust pose option (**Path A+**: ignore
     non-root translation tracks for a flagged custom dancer), or a native-rig port with its own
     motion pool (**Path B**, a medium DLL feature).

| Path | Gives | DLL work | Content work | Fidelity |
|---|---|---|---|---|
| **A — stock-rig port** | both dancers in World, dancing World's 60 Hz versions of the same takes | none | new `.ddm` source importer + one `port_lib` config per dancer | DSU look (with the §5 shader), World donor proportions |
| **A+ — own lengths on stock clips** | as A, keeping DSU's limb lengths | S: pose option in `core/anm/pose.rs` + sidecar flag + host tests | as A, conform on directions only | DSU proportions, World choreography |
| **B — native DSU rig + DSU clips** | DSU's exact skeleton, trims, and the 3 DSU-only takes | M: per-dancer motion source (`session.rs` / `pick.rs` / sidecar) | `.ani → .anm` converter; the rig ports 1:1 | exact DSU, but 30 Hz source |
| **Shader** (any path) | DSU toon + black ink | S: `_toon` variant family + selector | HLSL define + manifest lines + blobs | minus self-shadow |
| **Blink** (optional) | the 1/6 s eyes-closed swap every 0.5–3 s | S–M: texture-slot swap on the body item | ship `*_al02` | exact |

**Recommended order:**
1. Path A for both dancers, together with the `_toon` shader. This proves the importer, textures
   and look on the cabinet with zero engine risk.
2. A+ if the donor proportions read wrong. The flared trousers and platform shoes are the likely
   casualties.
3. Path B only if the three DSU-only takes or DSU's exact trims are wanted.
4. Blink last.

---

## 1. Asset inventory (what the dancer scene loads)

The in-game dancer UI is `K3DUIInGame` (vtable `0x1CDE1C`: `[0]` Update `0x6D5D0`, `[5]` Load
`0x6AA00`, `[6]` Render `0x6B2A0`, source path `NEW\3D\K3DUIInGame.cpp`). What its load state
machine (`FUN_0006AA00`) pulls from the `x_data` archive:

| File | Role | Notes |
|---|---|---|
| `afro.ddm`, `lady.ddm` | the two dancers | the name table at `0x1DDE48` is indexed by `this+0x154` |
| `afro_al.dds`, `lady_al.dds` | body textures | DXT1 512², **no mips**. Uncompressed `.tga` twins also ship |
| `afro_al02.*`, `lady_al02.*` | **blink** texture (eyes closed) | differs from `_al` only in the eye region (afro bbox 15..82 × 71..100) |
| `toon.tga` | toon ramp | 128×32 RGBA. Columns 0–63 = 142/255 grey with α 0; columns 64–127 = white with α 255 |
| `*.ani` (27) | skeletal clips | listed in `animations.csv` |
| `animations.csv` | clip DB | `NAME, DANCER, TYPE, SPEED, SKIP`. SPEED is 120 for every row. SKIP is 0 for every row. Parsed by `FUN_00098980` |
| `ToonLitShadowMapPixelShader.xpu`, `ShadowMapPixelShader.xpu` | pixel shaders | NV2A register-combiner programs (§5.2) |
| `<song>.csv` (47) | per-song **direction script** | `Bar, Note, Movie File, Movie Loops, Movie Speed, FF Motion, FF Period, FF Texture, FF Scale, Light Color, Light Position, Camera Position, Camera Transition Time, Render Style, Render Negative, Blur Style, Blur Amount`. Drives the `EF*` event classes (§4.5) |
| embedded in `default.xbe` | **vertex shaders** | 8 assembly *sources*, assembled at boot with `XGAssembleShader` (§5.1) |

- There are no stage models: DSU draws its dancers over the song movie or over a particle
  background.
- There are no per-dancer accessory parts.
- `.act` files are 2D (how-to-play and result lines). `.pd` files are particle definitions.
- The `.ddm` headers still carry DolphinSDK environment strings (`C:\DDR\xbox_port\new`,
  `DOLPHIN_PLATFORM=HW2`) in uninitialised padding, i.e. the format came over from a GameCube
  tool chain. That is consistent with the OpenGL-style CCW front faces (§4.4).

## 2. `.ddm` model format (`srdd`) — loader `FUN_00066950`

All little-endian. Offsets are absolute.

| Offset | Size | Field |
|---|---|---|
| `0x000` | 4 | magic `srdd` (`0x64647273`) |
| `0x004` | 0x44 | **`D3DMATERIAL8`**: Diffuse, Ambient, Specular, Emissive (RGBA f32 each), Power. Both files: every colour is `(0,0,0,1)`, Power 2.0. Copied to `ModelData+0x18`. **Unused by the toon path** |
| `0x048` | 0x100 | texture name. The tool overwrote the `.` with NUL (`afro_al\0bmp\0…`). The loader (`FUN_00068A80`) tries each extension in the table at `0x1DDE38`: `.dds`, `.tga`, `.bmp` |
| `0x148` | 4 | bone count *n* (afro 35, lady 32) |
| `0x14C` | 0x84·*n* | bones, see below |
| … | 4 + 2·*i* | `u32` index count *i*, then `u16` indices: **triangle list**, `i/3` triangles |
| … | 4 + 44·*v* | `u32` vertex count *v*, then vertices, see below. The file ends exactly here |

**Bone record** (0x84 bytes):

| Offset | Size | Field |
|---|---|---|
| `+0x00` | 0x40 | 4×4 f32 row-vector **inverse bind** matrix (model → bone) |
| `+0x40` | 0x40 | name |
| `+0x80` | 4 | `i32` **vertex-shader constant register** holding this bone's matrix |

- The registers run `-96, -92, … -4, 28, 32, …` in steps of 4. `c0..c27` are skipped because they
  hold the shader's own constants (§5.1). `-40` is also unused, for an unknown reason.
- **There is no parent index** (§3.2).

**Vertex** (44 bytes; the declaration at `0x1DDA38` gives `v0..v3` = FLOAT3, FLOAT3, FLOAT2,
FLOAT3):

| Offset | Field |
|---|---|
| `+0` | position f3 (bind space) |
| `+12` | normal f3 |
| `+24` | UV f2 (D3D: v = 0 is the top row) |
| `+32` | **bone 0 register** as f32 |
| `+36` | **bone 1 register** as f32 |
| `+40` | **bone 0 weight** f32 (bone 1 gets 1 − w) |

- At most **2 influences** per vertex, with w ∈ [0.5, 1].
- Afro: 31 % of vertices rigid, 69 % blended. Lady: 54 % rigid, 46 % blended.

**Facts about the two models:**

| | afro | lady |
|---|---|---|
| Vertices | 3071 | 2767 |
| Triangles | 4256 | 3965 |
| Mesh / material / texture | one each | one each |

- Bind space is **Z-up**, the dancer faces **+Y**, and the T-pose is authored with the pelvis near
  the origin.
- Units are ≈ **0.1026 m**: that scale puts DSU's pelvis at World's Hips height (0.97 m). Both
  models are about 17.4 units tall including the afro.
- **The two skeletons are identical.**
  - They share 30 bones. For each shared bone the rotations are bit-identical and the positions
    differ by one constant offset, `(0, 0.1316, 1.8167)`: lady is authored 1.82 units lower.
    Maximum deviation 1.6e-6.
  - Afro adds 5 `*_end` / `Head_End` leaves. Lady adds `BreastLeft` and `BreastRight`.
  - This is why the `mf` rows of `animations.csv` let lady play the male clips unchanged.

## 3. `.ani` clip format (`mina`) — loader `FUN_00065790`

| Offset | Size | Field |
|---|---|---|
| `0x00` | 4 | magic `mina` (`0x616E696D`) |
| `0x04` | 4 | track count (41 or 42) |
| `0x08` | 4 | frame count (557–661) |
| `0x0C` | … | tracks, back to back |

A track is:

| Offset | Size | Field |
|---|---|---|
| `+0x00` | 56 | name |
| `+0x38` | 4 | u32. Value `12` in the male clips, uninitialised garbage in the female clips |
| `+0x3C` | 4 | u32. Value `1` in the male clips, garbage in the female clips |
| `+0x40` | 28·F | one key per frame: quaternion `(x, y, z, w)` f32 ×4, then translation f32 ×3 |

The file ends exactly after the last track. The loader keeps every `(SKIP + 1)`-th frame; SKIP is
0 everywhere.

### 3.1 Semantics (verified)

- **Keys are WORLD-space joint transforms, not local TRS.** Each key is the joint's full
  model-to-world frame in the clip's space. The skinning matrix is
  `inverseBind · (R(q) with translation t)`, uploaded as is (`K3DSkinModel::Draw` `FUN_00067040`).
- The quaternion is D3DX `(x, y, z, w)`. Of the four candidate conventions, only this one makes a
  child's offset constant in its parent's frame.
- **Clip space is Y-up, floor at y = 0, dancer facing −Z.** The dancer's left is +X. The data is
  numerically D3D left-handed.
- The game's model matrix is built from `RotationZ(π)` and `RotationX(π/2)` (`FUN_0006AA00`
  case 9). It stands the Y-up clip up inside DSU's Z-up scene.
- Bind → clip space is `(x, y, z) → (x, z, −y)`: bind +Z maps to clip +Y (up), and bind +Y
  (facing) maps to clip −Z.
- **Bone lengths are exactly constant over every frame** (e.g. Knee→Ankle 4.295 in bind and in
  every frame), and each local offset equals the bind local offset. The capture is a rigid-segment
  solve: no stretch and no scale.
- Tracks bind to model bones **by name**. Clips carry a superset of both rigs. The extra tracks
  are `AfroEnd`, `Breast*End` and `obj32`. `obj32` is not rigid to any joint and no model uses it.
- The root moves through the clip: `hh01_m` travels 11.6 units ≈ 1.2 m forward. Clips do not
  loop seamlessly.

### 3.2 Hierarchy (reconstructed)

No parent table exists anywhere. The game never composes local transforms, so it needs none.
`ultramix_k3d_dump.HIERARCHY` rebuilds it from the joint names. It is checked by
`ultramix_k3d_dump.py hierarchy *.ani`: every child's offset in its parent's frame is constant to
≤ 3e-5 units over all 27 clips, and ≤ 1.2e-3 for the `Breast*` and `AfroBone` jiggle bones.

```text
root ─┬ Spine_Low ─ Spine ─┬ Sternum ─┬ Neck_1 ─ Neck_2 ─ Head ─┬ Head_End
      │                    │          │                         └ AfroBone ─ AfroEnd
      │                    │          ├ Clav_L1 ─ Clav_L2 ─ shoulder_L ─ Elbow_L ─ Wrist_L ─ Wrist_L2 ─ Wrist_L_end
      │                    │          └ Clav_R1 ─ … (mirror)
      │                    ├ BreastLeft ─ BreastLeftEnd
      │                    └ BreastRight ─ BreastRightEnd
      ├ Hip_L_DUM ─ Leg_L ─ Knee_L ─ Ankle_L ─ Toe_L ─ Toe_L_end_site
      └ Hip_R_DUM ─ … (mirror)
```

- `root`, `Spine_Low` and both `Hip_*_DUM` bones sit at the same point.
- In the shipped clips the whole `root → Spine → Sternum → Neck_1` torso turns as one rigid body.
  The solve left the spine unbent.

### 3.3 Frame rate

- The data is **30 Hz**. §6 shows every DSU clip is a 2:1 decimation of a 60 Hz World take.
- The *playback* code runs each clip with a frame period of `frames · (1/35) / frames`:
  - `0.0285714` at `0x1D24F4`, passed as the duration to `PlayAnimation` `FUN_00067450` from the
    Load and Update paths;
  - `Advance(dt)` `FUN_00067630` accumulates `dt / period`, steps at most one frame per call, and
    lerps/slerps between frame *n* and *n+1*.
- Unless Update's `dt` is not in seconds (its caller was not traced), DSU played its dances
  **about 17 % fast**.
- This does not affect a World port: World's `bpm_sync` clock drives playback, and these are
  World's own 120 BPM takes.

### 3.4 Verification render

`skin_pose()` implements the formula above. With it the decoded meshes were CPU-skinned at frames
0/120/240/360 of `hh01_m` (afro) and `lady_soul01` (lady), then rendered in Blender 5.2 (EEVEE,
emission-only toon material, solidify-modifier inverted hull). Scratch project:
`~/blender-projects/dsu-dancer-port/`, not tracked.

- Geometry, UVs (`v_blender = 1 − v`), 2-bone weights, textures and poses were all correct: clean
  limbs, the feet on the floor at every frame, no tearing.
- The 2-band ramp and the black hull already give a recognisably ULTRAMIX look.
- **The first overview render is NOT what DSU shows.** It used an orthographic front camera, a
  light direction chosen by hand, and a solidify hull built *inward* (−0.06). That hull poked
  through the lady's eyes and lips as dark streaks.
  - Plain-texture close-ups of the same skinned mesh, in bind pose and posed, are clean. The
    face damage came from the render, not the data.
  - `render_dsu_camera.py` in the scratch project re-renders with DSU's own viewport, camera and
    light setup (§4.6), an outward hull at DSU's width, and a 4:3 frame.
- None of these renders use the World skeleton. Everything is DSU data skinned with DSU's formula.
- To repeat without that project: `ultramix_k3d_dump.py obj … <frame> out.obj`, then import the
  OBJ. The OBJ is right-handed (Z negated, winding reversed).

## 4. Runtime behaviour (the K3D engine)

### 4.1 Skinning and draw — `K3DSkinModel` (vtable `0x1CDDE0`: dtor `0x66E40`, Draw `0x67040`)

Per bone, Draw computes:
- `slerp(qA[frameA], qB[frameB], t)` and `lerp(tA, tB, t)`;
- `invBind · that`, transposed into `c[register]` via `FUN_00063A30` → `FUN_00063AB0`. The latter
  is a shadow copy of the 192 NV2A constants, indexed `reg + 96`.

It then issues `DrawIndexedVertices(TRIANGLELIST, tris·3)`. The `(A, B, t)` triple does double
duty:
- inside a clip it is `(frame n, frame n+1, sub-frame fraction)`;
- across a clip change it is a **0.3 s crossfade** (`0x1C56F0`) from A's current frame to B's
  start frame.

`PlayAnimation(anim, dur, 15, frames − 15, loop)` plays the window `[15, frames − 15]`. In-song
clips are **not** looped; at the end the next clip starts, with the crossfade.

### 4.2 Choreography sequencing — `FUN_0006AA00` (load) and `FUN_0006D5D0` (update)

1. **Dancer.** `this+0x154` toggles between afro and lady **every song**. One dancer is on screen
   at a time.
2. **Clip filter.** From the `animations.csv` rows whose DANCER is that dancer, the song record's
   per-dancer bitmask (`song+0x24C` afro, `+0x250` lady) keeps only the matching rows
   (`FUN_00098510`). A zero mask keeps all.
3. **Selection.** While more than 5 remain, a random one is removed (`GetTickCount() % n`). The
   survivors are shuffled into a playlist.
4. **Playback.** The playlist plays in order, cyclically (`this+0x158`).
5. **Scale.** The model scale is `1.25` for both dancers (`0x1DDE68`).

### 4.3 Blink

`Update` counts `this+0x144` down, starting from `0.5 + U(0,1)·2.5` s (`0x1D20A4`, `0x1D2280`).
At zero:
- it swaps the body texture (`ModelData+0x5C`) to the `_al02` texture for **1/6 s** (`0x1C5700`);
- then it restores the original and re-rolls the timer.

### 4.4 Render passes — `Render` `FUN_0006B2A0`, style = `this+0x148`

When flag `this+0x15D` is set, every style first renders the dancer into an offscreen target
(`this+0x11C`) for compositing and blur.

**NORMAL** (`FUN_0006C6F0`):
1. **Shadow pass** (`FUN_0006B250`, only when the shadow buffer exists). The skinned-unlit VS
   renders into a **1024² depth-only shadow buffer** from the light's position, with polygon
   offset `0.4 / 1.7` (`FUN_00064C70`). The buffer is created in the Load case 1 by
   `FUN_000648D0(…, 0x400, 0x400, 2)`.
2. **Body.**
   - Shaders: VS #6 "Toon + shadow" with **ToonLitShadowMap** PS.
   - Constants: `c16` = light direction in model space, `c17` = light colour (`FUN_00063C90`),
     `c19–c22` = shadow matrix (`FUN_00065420`).
   - State: render state `0x9C` = `D3DCMP_GREATER` (read as `SHADOWFUNC`) and render state
     `0x93` = `0x900` = **cull CW**. DSU's front faces are CCW.
3. **Outline.**
   - Shaders: VS #7 "Toon outline" with no pixel shader; the colour is `c3 = (0, 0, 0, 1)`.
   - Constant: `c18` = `{0.03, 0.3, 0, 0.001}` (`0x1DDE70`, the same for both dancers).
   - State: `0x93` = `0x901` = **cull CCW**, i.e. the classic inverted hull. `0x9C` = `NEVER`.

**Other styles.** The song CSV's RENDER STYLE column maps to `+0x148` (`FUN_00018A70`): `NORMAL`
= 1, `NEGATIVE` = 2, `ALPHA LIGHT` = 3, then `DOUBLE CAMERA`, `FILL IN/OUT`, `MATRIX IN/OUT` and
`OUTLINE` in declaration order (values 5–10, none is 4). They reuse the same shaders:
- `NEGATIVE` adds a fullscreen invert quad;
- `ALPHA LIGHT` uses VS #4 (toon without shadow);
- `OUTLINE` (`FUN_0006C7F0`) draws unlit plus hull, i.e. a silhouette;
- `MATRIX` (`FUN_0006C140`) runs three passes, the second and third with two parameters scaled
  ×2/×0.5 and ×4/×0.25. It is an echo/trail effect and was not decoded further.

The 1-to-1 value mapping is certain for 1–3. The rest are inferred.

### 4.5 Per-bar direction events

- The song CSV (47 songs) is parsed into `EF*` events: `EFLightColor`, `EFLightPosition`,
  `EFCameraPosition`, `EFRenderStyle`, `EFRenderNegative`, `EFBlurStyle`, `EFBlurAmount` and
  `EFFloodFill*`.
- **LIGHT COLOR changes almost every bar.** Indices 1–4 are used about equally (1596 rows), and 8
  once. The index lands in `this+0x68` (`FUN_000185B0`; value − 1) and selects the colour that
  becomes `c17`.
- **The light-colour table** is built in the ctor `FUN_00069380` → `FUN_0006A2D0`. Colours are
  RGBA:

  | Index | Colour |
  |---|---|
  | 1 | white `(1, 1, 1)` |
  | 2 | red `(1, 0.5, 0.5)` |
  | 3 | green `(0.5, 1, 0.5)` |
  | 4 | blue `(0.5, 0.5, 1)` |

- CAMERA POSITION and LIGHT POSITION index one shared, procedural table of **48 placements**
  (`FUN_0006A440`, stored at `this+0x7C`). Each placement is an offset rotated by `RotX(el)`, then
  `RotZ(k·45°)`, k = 0..7:

  | Indices | Offset | Target | Elevations `el` |
  |---|---|---|---|
  | 0–23 | `(0, −45, 0)` | `(0, 0, 10)` | −30°, −55°, +30° (30° above, 55° above, 30° **below**) |
  | 24–47 | `(0, −25, 0)` | `(0, 0, 16)` | the same three |

  - Values: `−45` at `0x1C570C`, `−25` at `0x1C5710`, the targets at `0x1E33D0` / `0x1E33DC`.
  - `FUN_0006CA00` picks the target by index > 23.
  - The mapping from CSV value to table index (1-based like LIGHT COLOR, or 0-based) is assumed,
    not traced.
  - The optional transition time slerps between placements (Update, `this+0x134`).

### 4.6 Viewport and projection

`FUN_00013E70` sets up the viewport:
- 640×480, **fovY 45°** (`0x1D2114`), aspect **4:3** (`0x3FAAAAAB`), near 1, far 1000.
- Every frame the aspect is overwritten from the Xbox video flags: `0x1D22F4` = **16:9** when the
  widescreen bit is set, else `0x1D22F0` = 4:3 (`FUN_00011EC0`). So with dashboard widescreen on,
  DSU renders an anamorphic 16:9 image into the 640×480 buffer.

Captures shown at the other aspect distort the dancers by about 33 % horizontally:
- widescreen capture shown at 4:3: dancers look tall and thin;
- 4:3 capture stretched to 16:9: dancers look short and wide.

The low `+30°` ring (camera below the floor looking up) and the 25-unit close ring give strong
perspective foreshortening: big feet, small heads.

## 5. Shaders

### 5.1 Vertex shaders

There are 8 sources embedded as text; `ultramix_k3d_dump.py vsh` prints them. They are assembled
by `FUN_00062B50` in `K3DVertexShaders.cpp` init `FUN_0006F570`. All are `vs.1.1` / `xvs.1.1`.

| # | Handle | Source VA | Purpose | Inputs | Key math |
|---|---|---|---|---|---|
| 1 | `0x502FC8` | `0x1DDE90` | static toon | v0 pos, v1 nrm, v2 uv | `oT1.xy = N·c16` |
| 2 | `0x502FCC` | `0x1DE358` | static toon outline | v0, v1 | see #7 |
| 3 | `0x4FFA34` | `0x1DE980` | skinned unlit | v0, v2, v3 | shadow depth pass; OUTLINE style |
| 4 | `0x4FFA38` | `0x1DEFB0` | skinned toon | v0–v3 | skin pos + nrm; `oT1.xy = N·c16` |
| 5 | `0x4FFA3C` | `0x1DF7F8` | skinned + shadow coords | v0, v2, v3 | `oT2 = pos · c19..c22`, `w = max(w, 0)` (no references outside init) |
| 6 | `0x4FFA40` | `0x1DFF58` | **skinned toon + shadow** (the dancer body) | v0–v3 | #4 + #5 |
| 7 | `0x4FFA44` | `0x1E08C8` | **skinned toon outline** | v0, v1, v3 | `pos += N_skinned · (c18.x + c18.y·(w_clip·c18.w))`; `oD0 = c3` |
| 8 | `0x4FFA48` | `0x1E1178` | particles | — | point sprites (`oPts`) |

Skinning is `mov a0.x, v3.x; m4x3 r0, v0, c[a0.x]` for both bones, then a lerp by `v3.z`. The
normal is skinned with `m3x3` and **not renormalised**.

### 5.2 Pixel shaders (`.xpu` = `D3DPIXELSHADERDEF_FILE`)

An `.xpu` is `PSB0` followed by the 60-dword NV2A register-combiner program. The loader
(`FUN_00062AF0`) passes it straight to `CreatePixelShader`. Disassembly
(`ultramix_k3d_dump.py xpu`):

```text
ToonLitShadowMapPixelShader.xpu    t0,t1,t2 = project2d; 4 combiners; c0/c1 → D3D consts 0/1
  stage 0: r0.rgb = t2·c0 + (1−t2)·c1      c0 = ffffffff (lit), c1 = ff808080 (shadowed: ×0.5)
  stage 1: r1.rgb = t1 · v0                 t1 = toon.tga at (N·L, N·L); v0 = c17 light colour
  stage 2: r0.rgb = r0 · r1
  stage 3: r0.rgb = t0 · r0 ; r0.a = c0.a   t0 = body texture; alpha = constant
  final:   out = r0 (clamped)
ShadowMapPixelShader.xpu            same without stage 1 (no ramp)
```

- `c0`/`c1` are the file's defaults. The game never calls `SetPixelShaderConstant`: the only
  reference to it is inside D3D's `ApplyStateBlock`. So lit = ×1.0 and shadowed = ×0.5 are the
  effective values.
- `t2` samples the depth-format shadow buffer projectively. On NV2A that is the hardware shadow
  compare, so it returns 0 or 1.
- The equivalent HLSL, in gamma space (Xbox combiners and World's shaders both work on
  non-sRGB values, so the constants port 1:1):

  ```hlsl
  float ramp = (NdotL_vertex_interp < 0.5) ? 142.0/255.0 : 1.0;
  float shad = lerp(0.5, 1.0, shadow_visible);
  rgb = tex2D(s0, uv).rgb * ramp * light_rgb * shad;   // alpha = constant (texture alpha unused)
  ```

- The ramp coordinate is **N·L computed per vertex and interpolated**; the step is taken per
  pixel. That is exactly the "N·L in TEXCOORD, quantize in PS" structure of
  `shaders/src/mdl_cel.hlsl`.
- `toon.tga` flips at column 64 of 128 and is sampled bilinear. The edge is therefore one texel
  wide in N·L units: about 1/128.

### 5.3 Outline width in World units

- The offset is `0.03 + 0.0003·w` DSU units along the object-space normal, before the 1.25 model
  scale.
- In metres that is ≈ `1.25 · (0.0031 + 0.0003 · d_m)`: about 5–6 mm for a dancer 5 m away.
- That is about 1 px at DSU's 480p. The repo's default is 2 px at 720p, which is visually the
  same class.
- DSU's ink is pure black. The repo's `outline::INK_RGB` is 0.03 grey.

## 6. Cross-reference: DSU clips = World's stock takes

`ultramix_k3d_dump.py match <DSU dir> <unpacked mc_*.arc dir>` produces this table:
- each DSU clip's root height is upsampled 2×;
- it is correlated (Pearson, sliding window) against every World clip's Hips height (bone 1);
- "@" is the World frame that lines up with DSU frame 0.

| DSU clip | World clip | r | @ | | DSU clip | World clip | r | @ |
|---|---|---|---|---|---|---|---|---|
| brk02_m_t7 | mc_male_br01_exec | 0.95 | 24 | | lady_break01 | mc_female_br01_exec | 0.80 | −3 |
| brk03_m_t1 | mc_male_br02_exec | 0.92 | 0 | | lady_break02 | mc_female_br02_exec | 0.81 | 30 |
| brk03_m_t3 | mc_male_br03_exec | 0.92 | 0 | | lady_hiphop02 | mc_female_hh02_exec | 0.92 | 38 |
| hh01_m | mc_male_hh01_exec | 0.96 | 0 | | lady_hiphop03 | mc_female_hh03_exec | 0.92 | −12 |
| hh04_m | mc_male_hh02_exec | 0.89 | 26 | | lady_jazz01 | mc_female_ja01_exec | 0.82 | 32 |
| ht01_m | mc_male_ht01_exec | 0.97 | 42 | | lady_jazz02 | mc_female_ja02_exec | 0.94 | 26 |
| ht02_m | **mc_female**_ht02_exec | 0.95 | 0 | | lady_soul01 | mc_female_sf01_exec | 0.95 | 28 |
| ht03_m | mc_male_ht03_exec (ht02 scores the same) | 0.92 | 0 | | lady_soul02 | mc_female_sf02_exec | 0.94 | 30 |
| ht04_m | mc_male_ht04_exec | 0.95 | 0 | | lady_soul03 | mc_female_sf03_exec | 0.93 | 34 |
| jaz01_m | mc_male_ja01_exec | 0.94 | 20 | | lady_house01 | `ne01_loop` only | 0.76 | — |
| jaz02_m | mc_male_ja02_exec | 0.88 | −1 | | lady_hiphop01 | none (best 0.62) | | |
| sfd01_m / 02 / 03 | mc_male_sf01 / 02 / 03_exec | 0.94 / 0.90 / 0.92 | | | lady_house02 / 03 | none (best 0.58) | | |

**Per-joint check** (full World pose through `anm_dump.evaluate_pose` on the donor rigs). The
height of every tested joint follows World frame `@ + 2k`:

| DSU clip | World clip | LHand | RHand | LFoot | RFoot | Head | Hips |
|---|---|---|---|---|---|---|---|
| `hh01_m` | `mc_male_hh01_exec` | 0.993 | 0.995 | 0.998 | — | 0.997 | 0.997 |
| `lady_soul01` | `mc_female_sf01_exec` | 0.988 | 0.990 | 0.996 | 0.998 | 0.994 | — |
| `lady_jazz02` | `mc_female_ja02_exec` | 0.989 | 0.987 | 0.991 | 0.975 | 0.996 | — |

The joint names correspond `Wrist_L ↔ LeftHand`, `Ankle_L ↔ LeftFoot`, `Head ↔ Head`,
`root ↔ Hips`. The ranges in metres agree too, e.g. LeftHand 0.66–1.49 m against 0.70–1.46 m.

**The same library.** DSU's genre codes map one-to-one onto World's pool names: `brk ↔ br`,
`hh ↔ hh`, `ht ↔ ht`, `jaz ↔ ja`, `sfd ↔ sf`. Konami reused one mocap library, captured at
≥ 60 Hz, from the ULTRAMIX era through A3 and World (World's motion arcs are byte-identical to
A3's). DSU exported it at 30 Hz. `SPEED = 120`
in `animations.csv` agrees with this repo's finding that World's clips are authored at 120 BPM.

### 6.2 Skeleton comparison

At 0.1026 m/unit (pelvis height matched), a T-pose-to-bind comparison:

| Segment | World `pl_rage00` | World `pl_emi00` | DSU afro/lady | Δ vs rage |
|---|---|---|---|---|
| Hips height | 0.970 | 0.970 | 0.969 | — |
| Thigh | 0.376 | 0.376 | 0.409 | +9 % |
| Shin | 0.423 | 0.423 | 0.440 | +4 % |
| Foot (ankle→toe) | 0.120 | 0.120 | 0.172 | **+43 %** (platform shoes) |
| Upper arm | 0.207 | 0.229 | 0.287 | **+39 %** (shoulder joint sits more medial) |
| Forearm | 0.235 | 0.211 | 0.223 | −5 % |
| Hip width | 0.196 | 0.196 | 0.221 | +13 % |
| Pelvis→neck | 0.419 | 0.419 | 0.466 | +11 % |

- The rigs differ in topology: World has 33 HumanIK bones with `*Roll` twist bones; DSU has
  32/35 custom bones, doubled clavicles, and no twist bones.
- The rigs differ in rest orientation too: DSU bones point along +X of their own frame, Maya
  style.
- So a retarget is always required to use World clips on DSU meshes. It is the same kind of
  retarget `port_lib` already does.

## 7. Target constraints (World side) vs. the DSU assets

All World-side facts are from `docs/3d_model_format_research.md`,
`docs/background_dancers_research.md` and `tools/blender_ddr_addon/README.md`.

| Constraint | World | DSU asset | OK? |
|---|---|---|---|
| Influences / vertex | ≤ 4 | 2 | ✓ |
| Bones / mesh (palette) | ≤ 52 per block | 32–35 (41 as a superset) | ✓ |
| Posed bones per instance (`frame_board`) | ≤ 64 | ≤ 41 | ✓ |
| Vertices / mesh | ≤ 65 535 | 3071 / 2767 | ✓ |
| Texture | DDS, stem ≤ 20 alphanumerics, globally unique key, first registration wins | `afro_al` / `lady_al` DXT1 512², **no mips** | rename (e.g. `dsuafro_al`). Keep DXT1 if a mip-less DDS loads, else A8R8G8B8 + 3 mips via the add-on (§9 R4) |
| Units / axes | metres, Y-up, dancer faces +Z (game) / −Y (Blender) | 0.1026 m units; clip Y-up facing −Z, LH numerics | **mirror Z** (quaternion `(x,y,z,w) → (−x,−y,z,w)`) and reverse the winding |
| Front faces | the add-on's export convention | CCW (DSU culls CW) | handled by the Blender round-trip |
| Clip rate | 60 fps skeletal | 30 Hz | World already has 60 Hz versions (§6). Path B upsamples 2× (slerp midpoints) |
| Clip binding | **by bone index**. Stock clips carry translation tracks for all 33 bones | by name, world space | Paths A/A+ use the stock rig. Path B needs its own clips |
| Clip pool | code lists per sex (`selection::POOL_MALE`/`POOL_FEMALE`), loaded from `mc_<sex>.arc` | per-dancer CSV rows | A/A+: the sidecar sex picks the pool. B: new motion source |
| Lighting | none bound. The repo's lit/cel reconstruct from World and WVP, key light `(0.7, 0.6, 0.5)` | directional light in model space + per-bar colour | fixed key + optional per-frame tint (§8.4) |

## 8. Approach

### 8.1 Common front end: a `.ddm` source importer for the add-on

New `tools/blender_ddr_addon/examples/ddm_source.py`, modelled on `fbx6_gta_source.py`. It
imports `scripts/ultramix_k3d_dump.py` (import-safe) and:

1. Builds an armature from `HIERARCHY` and each bone's `inverse(inverse_bind)`. It applies the
   bind→clip map (§3.1), scales by 0.1026, and converts to Blender with `convert.vec_to_blender`
   after the Z mirror. Bone heads sit at the joints; tails point at the first child.
2. Builds the mesh: positions and normals the same way, `v = 1 − v`, the triangle order reversed
   by the mirror. Vertex groups come from the two `(register, weight)` pairs through the
   register→name map.
3. Assigns one material, whose image is the renamed body texture.
4. For lady, optionally adds the bind offset `(0, 0.1316, 1.8167)` so both dancers share one rest
   skeleton. Path B needs this.
5. Offers a quick check: pose from an `.ani` frame by setting each pose bone's matrix to the
   clip's world matrix, so the result must match `skin_pose()`.

### 8.2 Path A — stock-rig port (content only, no DLL change)

This follows `README.md` "Porting an existing model", the flow already used for Peter, Teto,
Miku, CJ and Big Smoke. There is one config per dancer:
`examples/port_character_ultramix_ddm.py`, driven by env `SRC=afro|lady` and
`KEY=dsuafro00|dsulady00`. Keys must be `[a-z0-9_]` and not collide with stock keys.

1. **Donor.** `pl_rage00` for afro and `pl_emi00` for lady (`port_lib.load_ddr_rig`). The sidecar
   sex must match: `M` for afro, `F` for lady.
2. **Conform** (`port_lib.conform`, pre-scale S = 0.1026). Joint targets:

   | DSU | World |
   |---|---|
   | `root` / `Spine_Low` | `Hips` |
   | `Leg_*` | `*UpLeg` |
   | `Knee_*` | `*Leg` |
   | `Ankle_*` | `*Foot` |
   | `Toe_*` | `*ToeBase` |
   | `Toe_*_end_site` | `*Toe_end` |
   | `Spine` | `Spine1` |
   | `Sternum` | `Spine2` |
   | `Neck_1` | `Neck` |
   | `Head` | `Head` |
   | `Clav_*1` | `*Collar` |
   | `shoulder_*` | `*Arm` |
   | `Elbow_*` | `*ForeArm` |
   | `Wrist_*` | `*Hand` |

   `Neck_2`, `Clav_*2`, `Wrist_*2` / `_end`, `AfroBone` and the `Breast*` bones ride along as
   helpers.
3. **Weights** (`retarget_weights` with a class map):

   | DSU bones | Class |
   |---|---|
   | `root`, `Spine_Low`, `Hip_*_DUM` | hips |
   | `Spine` | spine low |
   | `Sternum`, `Breast*` | chest |
   | `Neck_1`, `Neck_2` | neck |
   | `Head`, `Head_End`, `AfroBone` | head (rigid) |
   | `Clav_*` | collar |
   | `shoulder_*` | upper arm |
   | `Elbow_*` | forearm |
   | `Wrist_*` | hand |
   | `Leg_*` | thigh |
   | `Knee_*` | calf |
   | `Ankle_*` | foot |
   | `Toe_*` | toe |

   The class/position blend then feeds the `*Roll` bones. **Head rule:** nothing above the chin may
   carry a partial `Head` weight. Check this with the add-on's report; DSU's own 2-bone blend at
   the neck may need tightening.
4. **Texture.** Copy the DXT1 file renamed (`dsuafro_al.dds`), or write A8R8G8B8 with 3 mips.
   Give the colour attribute an opaque white; set backface culling on.
5. **Export and check** (`port_lib.export_and_check`). Preview with the **matching** stock clip,
   for example `mc_male_hh01_exec.anm` against DSU `hh01_m`. The preview must show the DSU
   choreography.
6. **Deliver.** Put the export folder at `data_mods/custom_models/dancers/<Friendly Name>/pl_<key>/`
   with a sidecar such as `dsuafro00, pl, M, A, 1.0, 0.8, 0.0`. The DLL mounts it with no code
   change. Whether the result is **committed** or stays a personal install is the maintainer's
   call: these are ripped Konami assets (see the stock-mesh rule in
   `docs/ps1_style_dancers_feasibility.md` row 9).

**Cost of Path A.** The conform stretches DSU to the donor's lengths (§6.2):
- upper arms shortened by about 28 % (0.287 → 0.207 m);
- platform shoes shortened by about 30 %;
- torso shortened by about 10 %.

The head, hands and clothing keep their DSU size and shape. `port_lib`'s `waist_report` /
`seam_report` apply as for CJ.

### 8.3 Path A+ — own lengths on stock clips (small, pure DLL change)

Stock clips force donor proportions *only* through their per-bone translation tracks
(`README.md` step 1).

**Change.** Add a sidecar flag, e.g. a 7th rlist column `keep_lengths` (design choice). A flagged
custom dancer's evaluator **drops translation tracks for every bone except the root and Hips**
(`core/anm/pose.rs::evaluate_into`, the `(Channel::Translation, …)` arm). Those bones then keep
their seed, i.e. their own bind local offset.

**Content side.** Build the 33-bone World topology with the **donor's local rest orientations**
but DSU segment lengths. Offsets run along the donor's local offset direction, so a stock rotation
means the same thing. The conform then only corrects joint *directions*.

**Properties:**
- It is pure and host-testable. The existing `core/anm` tests already cover seeds and untracked
  channels.
- It is a one-line branch on the hot path (per bone per frame; negligible cost).
- The Hips translation (bounce and travel) stays the donor's. Choosing the pre-scale so the hips
  heights match (S = 0.1026) means it needs no scaling.
- A cheap preview check: the dancer's feet must stay planted at the donor's contact frames. DSU's
  legs are about 6 % longer (thigh + shin), but its hip sockets sit lower under the pelvis, so
  whether the feet dip or float is empirical. If they drift, scale the Hips translation by the
  standing-hip-height ratio.

### 8.4 Path B — native DSU rig with DSU clips (medium DLL feature)

**Model.** `ddm_source.py` builds the unified 41-bone superset rig (lady shifted) and exports it
as-is through `export_model` / `export_character`. Every value fits: 41 ≤ 52 palette slots,
41 ≤ 64 pose slots, 2 weights per vertex.

**Clips.** New `scripts/ultramix_ani_to_anm.py`:
1. Compute local TRS per bone: `local = W_child · W_parent⁻¹` using `HIERARCHY` and the superset
   index order, which is parent-first as KTMDL requires.
2. Apply the Z mirror (§7) and the 0.1026 scale.
3. Upsample 30 → 60 Hz with slerp/lerp midpoints.
4. Encode with `anm_dump.write_anm`: kind `0x1C` rotations, `0x1D` translations; translations only
   on `root`. Other bones' offsets are constant (§3.1), so the seed covers them.
5. Validate with the Python evaluator against `skin_pose()` joint positions (target < 1e-4 m).

   **Better source for the 23 shared takes:** retarget World's own 60 Hz clips onto the DSU rig,
   baking world joints and solving local rotations with rest-frame corrections. They are longer
   and captured at 2× the rate. DSU's `.ani` then only supplies the three DSU-only takes.

**DLL.** The pools are code lists keyed by `Sex` (`selection.rs`), and `session.rs` loads
`data/arc/mc_<sex>.arc` (`d.sex.arc_stem()`). Generalise that to a per-dancer **motion source**:
- `Stock(Sex)`, or `Custom { arc, clips }`;
- declared by the sidecar, e.g. `motion = mc_dsu.arc` plus a clip list or a folder scan in
  `custom_content.rs`;
- `custom_scan.rs` packs `pl_<key>/motion/*.anm` like the body arc and mounts it next to it;
- `pick.rs` builds the playlist from that pool.

`bpm_sync` / `stop_slow` apply unchanged, because these are 120 BPM takes. The size is similar to
the 2026-09-22 custom-content work (pure planner + host tests + the impure scan), and the hot path
is untouched.

**Gains.** Exact DSU proportions, DSU's exact clip trims, the three DSU-only takes, and DSU's
per-dancer pool (afro 14 clips; lady 13 + 9 `mf`).

### 8.5 Shader: a `_toon` variant family (any path)

1. **HLSL.** Add a `DSU_TOON` define to `shaders/src/mdl_cel.hlsl`:
   - two bands: `CEL_THRESH = 0.5`, levels `142/255` and `1.0`, `CEL_SOFT ≈ 1/256` (one ramp texel
     of bilinear);
   - no rim ink;
   - N·L per vertex into the TEXCOORD, quantised in the PS: the structure the file already has.

   Keep the stipple `texkill` and CCOLOR/NOTEX handling exactly as the cel PS does. Add manifest
   lines in `scripts/build_shaders.sh` (`mdl_{bg,ch}_toon[_vc].vs`, `mdl_toon[_c|_notex].ps`) and
   commit the blobs under `data_mods/shader_fixes/blobs/`.
2. **Containers.** Add a third variant column to `shader_layout::MODEL_VARIANTS`: `<name>_toon`
   with the outline pair at program 0. Bump the synthesis fingerprint (the same recipe as the
   `_ps1` plan in `docs/ps1_style_dancers_feasibility.md` §3.1).
3. **Selection.** Either add a fourth scene style `TOON (ULTRAMIX)` in `style.rs`, or add a
   per-dancer override: a sidecar flag, with the builder re-pointing the dancer's private material
   copies at `_toon` (`render_item.rs`, the existing re-point). The override keeps DSU dancers
   looking like DSU under any scene style. Both are cheap.
4. **Outline.** Reuse the hull machinery as is. Give DSU dancers **black** ink (`INK_RGB` → 0)
   and 1.5–2 px width (§5.3).
5. **Light colour** (optional, no shader change). The frame board republishes each item's **tint**
   every frame, and the PS multiplies COLOR0. Driving the tint from a 4-colour palette on bar
   boundaries (the tempo clock already knows measures) reproduces DSU's per-bar `c17` colour
   cycling. The palette is §4.5: white, red, green, blue.
6. **Dropped: the self-shadow map.**
   - World binds no light or shadow matrix to the model pass.
   - There is no depth pre-pass or depth-texture sampler to reuse.
   - Adding one means a new render target, pass and constants: the Tier-2-class engine work that
     `docs/ps1_style_dancers_feasibility.md` §4 describes.
   - Visually it is the "× 0.5 where occluded" term. The ramp's dark band covers most of what it
     did on a front-lit dancer.

### 8.6 Blink (optional, DLL)

DSU swaps the whole body texture for 1/6 s every 0.5–3 s. In World the equivalent is writing the
body render item's private material texture slot to the `_al02` texture and back, driven from the
director's per-frame tick. Both textures must be resident (§9 R5). This is engine-facing code: plan
a diagnostic deploy with one-shot logs on every fallback, per the repo rules.

### 8.7 Scene-level parity (no new work)

| DSU | World equivalent |
|---|---|
| `MOVIE + DANCER` | `background_dancers.movie_mode = FULLSCREEN` (dancers over the song movie, stage hidden) |
| One dancer per song, alternating | pick the DSU dancer(s) in the BACKGROUND DANCER row |
| DSU cameras | an authored `.camanm` set (`split_camanm.py` / `gen_movie_cameras.py`) built from the 48-placement table (§4.5), fovY 45°, one DSU unit = 0.1026 m × 1.25 model scale; stock cameras otherwise |

The other per-bar render styles (NEGATIVE, FILL, MATRIX, BLUR) are out of scope.

## 9. Open items / risks

| # | Item | Blocks | How to close |
|---|---|---|---|
| R1 | Head / neck weight band after retarget (Big Head / nod shear rule) | A, A+ | add-on weight report; preview a nod-heavy clip (`ja01`) |
| R2 | Whether A+'s dropped translations keep the feet planted with longer legs | A+ | preview renders, then cabinet; fallback: scale the Hips translation by the leg ratio |
| R3 | ~~light-colour / camera tables~~ decoded (§4.5, §4.6). Left open: whether CSV indices are 1-based, and whether DSU cancels root motion (the dancer walks up to 1.2 m during a clip) | §8.7 camera authoring | xemu capture against a known song bar |
| R4 | Does World's DDS loader accept a mip-less DXT1? (stock ships 3 mips) | texture choice | one-off test, or write A8R8G8B8 + 3 mips (add-on default path) |
| R5 | Whether a second body texture in the dancer arc is registered and resident without a material referencing it (blink) | §8.6 | add a dummy material or part that references `_al02`, or resolve the name through the texture registry at build |
| R6 | DSU playback-rate constant 1/35 vs 30 Hz data (DSU ran ~17 % fast?) | nothing (curiosity) | trace Update's `dt` source or time it in xemu |
| R7 | Texture-key collisions (`afroal`/`ladyal`) with World-resident textures | A/A+/B | rename stems with a `dsu` prefix (§7) |
| R8 | Redistribution of ported Konami meshes/textures | shipping | maintainer policy (personal install vs committed content) |

## 10. Reproduction

```bash
# unpack the archive (config for the PAL/UK disc)
scripts/extract_ultramix_data.py ultramix_uk ~/Desktop/dancing_stage_unleashed ~/Desktop/dancing_stage_unleashed/extracted_full
D=~/Desktop/dancing_stage_unleashed/extracted_full
scripts/ultramix_k3d_dump.py ddm $D/afro.ddm                        # §2
scripts/ultramix_k3d_dump.py ani $D/lady_soul01.ani                 # §3
scripts/ultramix_k3d_dump.py hierarchy $D/*.ani                     # §3.2
scripts/ultramix_k3d_dump.py xpu $D/ToonLitShadowMapPixelShader.xpu # §5.2
scripts/ultramix_k3d_dump.py vsh ~/Desktop/dancing_stage_unleashed/dancing_stage_unleashed_default.xbe  # §5.1
scripts/ultramix_k3d_dump.py obj $D/afro.ddm $D/hh01_m.ani 240 /tmp/afro240.obj   # §3.4
# §6: unpack World's motion arcs, then survey
for a in "$DDR_WORLD_INSTALL"/data/arc/mc_*.arc; do scripts/unpack_arc.py -o /tmp/mc "$a"; done
scripts/ultramix_k3d_dump.py match $D /tmp/mc
```

XBE functions worth reopening in Ghidra:

| Address | Function |
|---|---|
| `0x66950` | `.ddm` loader |
| `0x65790` | `.ani` loader |
| `0x67040` | `K3DSkinModel::Draw` |
| `0x67450` / `0x67630` | Play / Advance |
| `0x6AA00` | `K3DUIInGame` load |
| `0x6D5D0` | `K3DUIInGame` update (blink, clip chaining) |
| `0x6B2A0` / `0x6C6F0` | render / NORMAL pass list |
| `0x6F570` / `0x62B50` | VS assembly |
| `0x62020` / `0x62AF0` | PS load |
| `0x98980` | `animations.csv` parser |
| `0x18A70` | RENDER STYLE parser |
