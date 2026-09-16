# 3D Background Dancers in DDR World — Feasibility & Research

Status: **RESEARCH COMPLETE — GO (hybrid architecture), spike-gated** (2026-09-16).
No code written, no PDD docs yet; this is the design-input record for a future
PDD cycle.

Goal: bring back the pre-World "3D background dancers" — the animated
character(s) dancing on a 3D stage behind the lane, with the per-song camera
work — inside DDR World, using the DDR A3 character/stage/motion/camera data.

Addresses are file-relative to `gamemdx.dll` @ `0x180000000`. New RE in this
document was performed on **World 20260825** (the current build) and **A3
20240402** (final A3), with a presence check on **World 20250805** (the oldest
supported build). Builds are named per finding. Builds on:

- `docs/3d_model_format_research.md` — the complete on-disk formats (KTMDL,
  ANM family, B2IT, MRL0 rlists), the A3 runtime binding (§8), the camera
  re-projection (§6), and the in-game-verified Blender add-on. Every format
  fact this document relies on is there; nothing is re-derived here.
- `docs/custom_arrow_renderer_research.md` §3 — the 2D screen-command-list tag
  map (fallback renderer path).
- `docs/custom_shader_backgrounds_research.md` §6 — drawing under the 2D game
  content, hiding the stock background, movie suppression.
- `docs/custom_resolution.md` §3a — the per-frame present chain (where the 3D
  target sits relative to the 2D layers).
- `docs/shader_replacement_research.md` — GSPW containers / the model shaders.

---

## 1. Verdict

**Feasible, and substantially cheaper than "write a renderer from scratch".**
The working assumption going in — that World stripped all the code that
renders dancers — is only half right, and the half that survived is the half
that would have been expensive to rebuild:

| Layer | A3 | World (20250805 … 20260825) |
|---|---|---|
| KTMDL `.model` loader → GPU resource (VB/IB/decls/materials/textures/bones) | present | **PRESENT, byte-shape identical** (`FUN_180275d40` binder with the `"KTMDL"` magic at `0x180275d51`, converter `FUN_180272f80` with the same 15 helpers as A3's `FUN_180189900`, registry `FUN_18026f140`; registered from `Application::onBoot` as `agcs::ModelFileCallback`) |
| DDS / ANM file callbacks | present | **PRESENT** (`agcs::DdsFileCallback`, `AnimeFileCallback` `FUN_1801b0a40` — magic check, byte-swap, registry by extension) |
| Model shaders (`gs_model_default`, `gs_model_skinning_default`, all 10 `mdl_*`) | 35 `.gsp` | **PRESENT, byte-identical `shader.arc` (70 entries, same set)**, compiled at boot like every other container |
| The four model render passes (`MODEL:DISTANTVIEW/OPACITY/LOWPRIO_TRANS/TRANS`), collector, sorter, bone-texture upload, draw loop, per-record render states | present | **PRESENT and LIVE**: created in `FUN_1801f6510`, three of them attached to the RENDER-3D target every frame at priorities 0x66/0x67/0x68 (`FUN_1801f2c30`), driven per frame with an item list that is simply empty |
| `agcs::scene::SceneGraph` + `SceneGraphManager` (node tree, per-frame update job, render-item list, 2 camera slots, frustum culling, deferred destroy) | present | **PRESENT and LIVE** (`FUN_1800238a0`/`FUN_180214570`), root + one `debug::CameraNode` |
| Camera object (`me::scene::camera::Camera`, 0x3F8, view/proj → passes each frame) | present | **PRESENT** (`FUN_180023fb0` copies slot 0 into all passes) |
| `agcs::scene::{ModelNode, TransformNode, AnimationNode}`, `scene::CameraNode`, `scene::EmotionController` | present | **GONE** (no RTTI, no code) |
| ANM object ctor + track evaluator + pose→matrix chain + animation player | present | **GONE** — only the orphaned per-kind decoder table (`0x18035a560..`, e.g. kind 0x1C = `FUN_1801f8980`) survives with zero code references |
| `sequence::dance::{CharaActor, StageActor, CameraActor}`, the 3D half of `SceneManageActor` | present | **GONE** (World's `SceneManageActor` `FUN_18007d480` only owns the background-movie actor; `StageFrameActor` `FUN_18007a100` only owns the 2D "1st STAGE" banner layer) |
| `musicdb.xml` `<bgstage>` field parsing | present | **PRESENT** (descriptor table next to `movieoffset`/`bemaniflag`, u16 → music entry `+0x1C8` on 20260825) — World's XML just never populates it |
| Dancer / stage / motion / camera arcs on disk | 170 arcs | **PRESENT in the stock World install, byte-identical to A3** (165/165 compared; `pl_*` 27 MB, `mapset_*` 24 MB, `mc_*` 28 MB, `camera/` 2.2 MB, rlists inside `startup.arc`) — never opened by the game |

So the engine (`gs`/`agcs`/`me` libraries) still contains the whole 3D
pipeline; Konami deleted the **game-side scene layer** (node types, animation
runtime, actors) and stopped shipping `<bgstage>` values. What a mod has to
supply is that scene layer: an animation evaluator (pure math, already
implemented and verified in Python in `scripts/anm_dump.py`), a render-item
builder matching the engine's ABI (fully readable in both binaries), one
scene-graph node type, a camera driver, and the game logic that picks
stage/dancers/choreography/camera per song. Estimated at **3–6 weeks** for a
v1 (§9), preceded by a **one-week spike** that proves the render-item ABI on
a cabinet (§9.1) — the single load-bearing unknown.

The recommended architecture (§5, "Option A — hybrid") lets the game's own
code do everything GPU-facing: the stock loader turns the A3 arcs into GPU
resources, the stock passes draw them with the stock skinning shaders, the
stock scene graph culls them and the stock camera projects them. The DLL only
produces scene state. A from-scratch renderer over the 2D command list stays
available as the fallback (§5, Option B) and as the tool for the one thing the
hybrid can't do: drawing dancers *over* a background movie.

---

## 2. What survived in World — the evidence

Everything in this section was decompiled on 20260825 unless noted; the
20250805 presence check used strings/RTTI/byte search only.

### 2.1 Model loading is intact

`Application::onBoot` (`FUN_180002060`) still registers the file callbacks
`agcs::ModelFileCallback`, `agcs::DdsFileCallback`, `AnimeFileCallback`,
`PngFileCallback`, `agcs::ShaderFileCallback`, `agcs::Bm2dFileCallback`. Any
arc registered with the FileManager (the `asset_loader::load` path the WebUI
preview already uses) has its `.model` members converted:

`FUN_180208fc0` (callback load) → `FUN_1802030b0` (FNV-1 of the name, dedupe
in the ResourceManager map at `DAT_1806f2f68+0x38`) → `FUN_18026f140` (pop a
free resource slot, bind, convert) → `FUN_180275d40` (KTMDL binder; checks
`"KTMDL\0\0\0"`, version 2.x) → `FUN_180272f80` (converter). The converter's
callee set is the A3 set one-for-one (bones, VB upload, IB upload,
blend-index remap, materials, textures, draw records, vertex decls, name
hashing), so `docs/3d_model_format_research.md` §3 describes World's loader as
well as A3's. `.dds` members register textures under FNV-1(lower-cased,
underscore-stripped stem) exactly as before; `.anm`/`.camanm` members register
their raw buffers by extension (`anm` / `vanm` / other) — the registries exist,
only their consumers are gone.

### 2.2 The render passes are intact and running

`FUN_1801f6510` builds the four `gs::Renders::Model::Viewport<Render>` pass
objects into `DAT_1806f1528..1540` with the same name hashes, node-mask
filters (`0x01 / 0x56 / 0x10 / 0x46`) and sort modes as A3's `FUN_180137670`.
The render-graph boot `FUN_1801f2c30` attaches OPACITY, LOWPRIO_TRANS and TRANS
to the RENDER-3D target (`*(display+0x28)`) at priorities 0x66/0x67/0x68 —
line-for-line A3's `FUN_180133c30`. (DISTANTVIEW is unattached in both games.)

Per frame the pass entry `FUN_1801f68a0` runs `FUN_1802606d0` (= A3
`FUN_180176b10`) when `pass+0xB8` (the item list) is non-null:
`FUN_180261430` dispatches the collector by mode (opaque `FUN_180263430`,
transparent `FUN_180262f00`, …) over every render item; `FUN_180261780`
performs the once-per-frame **bone-matrix → bone-texture upload** (locks
`item+0x78+frameParity*4`, writes `invBind[i]·bone[i]` as 3 float4 rows per
bone — the skinning contract of §3.6 in the format doc — guarded by the atomic
frame stamp `item+0xB4`); `FUN_180262310`/`FUN_1802624c0` sort; `FUN_180262670`
emits per record: world matrix, VS c22/PS c2 `ModelParameters` (`item+0x40`),
VS c23 tint, material bind (`FUN_1801f63f0`, incl. the `mdl_*` shader select),
vertex declaration, streams, bone texture on stage 3 when the draw record's
mask has `0x100`, `DrawIndexedPrimitive` per stream.

The list is empty because nothing constructs render items — not because the
passes were removed. Identical structure on 20250805 (`MODEL:OPACITY` string,
`SceneGraph@scene@agcs` RTTI, `KTMDL` magic all present).

### 2.3 The scene graph is intact and running

`SceneGraphManager` (`FUN_1800238a0`, global `DAT_1806f2d08`) is created at
boot (`FUN_180023da0` ← `FUN_18002c2b0`), registers `GraphUpdateJob` and
`DebugRenderJob` into the job graph exactly as A3's `FUN_18001cd80`, points all
four passes' `+0xE8` at the graph's render-item list (`*(graph+0x30)`), and each
frame (`FUN_180023fb0`) flushes the deferred-destroy queue (`FUN_180024250`)
and copies the active camera's view (`+0x08`) and projection (`+0x1C8`) into
the passes (`+0x98` / `+0x58`) and the packet passes.

`SceneGraph::update` (`FUN_180214570` = A3 `FUN_180159a90`) is the same
five-pass traversal:

1. pass 2 (`node+0xC & 0x10`): update, ctx = `&dt`
2. pass 3 (`& 0x4`): refresh, ctx = `{ptr, ptr, u8}`
3. reset the render-item list (`+0x30`) and the visible-node vector (`+0x58`)
4. pass 4 (`& 0x8`): renderables push THEMSELVES onto the visible vector
5. optional sort, then for every visible node push `*(node+0x78)` (its render
   item) onto the item list (`FUN_180267190`)
6. pass 5 per active camera (`+0x38`, 0x3F8 stride, active byte `+0x3F4`):
   frustum cull

Node protocol (`FUN_180215ad0` / `FUN_180215a60`): `+0x00` vtable (slot 0
`dtor(this, free)`, slot 1 `u32 visit(this, int pass, void* ctx)` — return 1 to
recurse into children), `+0x08` flags (bit0 enabled), `+0x0C` pass mask,
`+0x10` parent, `+0x18` first child, `+0x20` next sibling (singly linked, new
children are pushed at the head). Graph root = the SceneGraph object itself
(`+0x0C |= 2`, enabled `+0x08 = 1` from the ctor — in A3 the play sequence also
re-asserts bit0 at song start).

World's only node is the `debug::CameraNode` (`FUN_18001f920`, vtable
`0x18035c578`, update `FUN_18001fab0`), which writes camera **slot 1**
(`eye/target/up` at `+0x268/+0x274/+0x280`, frustum `+0x290..+0x2A4`,
near/far `+0x2A8/+0x2AC`, dirty bytes `+0x2B0/+0x2B1`). Both slots are
constructed active (`FUN_180214ce0` sets `+0x3F4 = 1`), so `FUN_1800243a0`
returns **slot 0** — the slot A3's `scene::CameraNode` (the `.camanm`
consumer) drove, and the one a mod must drive. Its field layout is the object
described in `docs/3d_model_format_research.md` §6/§8 (view `+0x08`, projection
`+0x1C8`, `+0x2B3` = projection-dirty → rebuild).

### 2.4 What was removed, precisely

- RTTI present in A3, absent in every World build: `ModelNode@scene@agcs`,
  `TransformNode@scene@agcs`, `AnimationNode@scene@agcs`, `CameraNode@scene`,
  `EmotionController@scene`, `CharaActor/StageActor/CameraActor@dance@sequence`.
- The ANM object constructor (A3 `FUN_18013a800`, chunk map keyed on
  `tag − 0xFF010002`), the track evaluator (`FUN_18013ab80`), the pose chain
  (`FUN_18013ba50/bc20/c000/be60`), `anim_player_apply_frame` (`FUN_180158ad0`)
  — none has a fuzzy match in World; the `ADD EAX,0xfefffe` chunk normaliser
  appears only in the byte-swapper (`FUN_1801b1c80`). The per-kind decoder
  table survived as dead data: no instruction references `0x18035a560..`.
- The render-item constructor (A3 `FUN_180175ea0`, ~200 bytes/item, `(*alloc)(size,
  0x10)` from the gs allocator `DAT_1802ee010`) and its bone-texture creation
  (`FUN_1801765d0`: 4×bones `A32B32G32R32F` (fmt 0x74), usage 0x2001, two of
  them) have no World counterpart — but every consumer of the item is intact and
  readable, which is what makes the hybrid option possible (§5.1).
- `ModelNode::setModel` (A3 `FUN_18015afb0`: item create + bind-matrix copy
  into `node+0x80`) and the `ModelNode` visit (`FUN_18015b220`; pass 4 refresh
  `FUN_18015b300` copies node world RT · extra matrix → `item+0x00`, tint →
  `item+0x50`, bone matrices `node+0x80` → `item+0x80`, pass mask → `item+0xB0`,
  hidden bit → `item+0xAC`, then pushes the node onto the visible vector).

### 2.5 Data on disk

The World install (`$DDR_WORLD_INSTALL/data/arc/`) carries every dancer-era
arc: 115 `pl_*.arc` bodies/parts, 26 `mapset_*.arc` stages, 24 `mc_*.arc`
motion sets (11 song-specific pairs + the generic `mc_male`/`mc_female` pools
+ `mc_bpm120`), `camera/stage_camera.arc` (96 `.camanm`) and 11
`camera/camera_music_*.arc`; `startup.arc` still contains
`chara_resources.rlist` (26 rows), `map_resources.rlist`,
`stage_camera_resources.rlist`, `music_camera_resources.rlist`. All 165 arcs
common to the local A3 and World installs are byte-identical (the only
differences are this repo's own `griffin00`/`peter00` test arcs on the A3
side). Nothing needs to be "pulled in" from A3 for the stock roster; the
modpack would ship copies only as a fallback should a future World data update
drop them (~80 MB; see §7.6 on licensing).

World's `musicdb.xml` dropped `<bgstage>` (and `<bemaniflag>`,
`<limited_cha>`) from every entry, but the parser still knows the field. A
`<bgstage>` value supplied through the modpack's existing `startup.arc` overlay
would land in the game's own music entry; equivalently the DLL can carry its
own mcode → stage table generated from A3's `musicdb.xml` (1,469 World entries
vs. A3's assignments; 2/3 of A3's songs point at stages 2/3/14/15/16/17).

---

## 3. How A3 assembled the scene (what the mod must reproduce)

From A3 20240402 (`FUN_180039650` = `DancePlaySequence::onUpdate`,
`FUN_180060460` = `SceneManageActor::onUpdate`, `FUN_180060090` =
`SceneManageActor::onInitialize`, `FUN_18005d5d0` = `CharaActor::onUpdate`;
full character-assembly detail in `docs/3d_model_format_research.md` §8):

1. **Song start (DPS step 2):** `SceneManageActor(basename, ?, courseFlag)` is
   created for every song, movie or not. Its `onInitialize` decides the movie
   flag from the music entry (`+0xb1` movie byte; `(5,5)` = none), creates the
   background-movie actor when there is one, requests
   `data/arc/mc_male_<song>.arc` / `mc_female_<song>.arc`, and sets `+0x122`
   for one special mcode (`0x9439`).
2. **step 0:** `StageActor(bgstage)` — walks `map_resources.rlist` to row
   `bgstage` (clamped), loads `mapset_<key>.arc` (`_g` on gold cabs for
   `0x94e7`), creates one `ModelNode` per part (`gm_<key>_<part>`), starts each
   part's `_play_loop.anm`, applies the `:N` priority → LOWPRIO_TRANS mask;
   `CameraActor(basename)` — resolves the camera set: the song's row in
   `music_camera_resources.rlist` (song-specific `camera_music_<song>.arc`) else
   the stage's rows in `stage_camera_resources.rlist` (`stage_camera.arc`
   `stNNN_stNN` sets; cue/switch semantics **OPEN** — never traced, §10).
3. **step 1:** one `CharaActor` per entered side (both when `GameWork+0 == 2`
   or when a song-specific clip exists; mcode `0x94e7` = the HOW TO PLAY demo
   forces kind 4). Kinds: `-2` random male A, `-1` random female A, `0` any,
   `1`/`2` random by sex, `3+k` = rlist row `k`; the pool honours the row's
   unlock id (`chara_resources.rlist`, §8 of the format doc).
4. **step 2:** wait for arcs; per dancer pick the clip: the song-specific
   `mc_<sex>_<song>_<song>_exec` if registered, else a random member of the
   generic pool (`br01/02, hh01/02/03, ht01/02/03(/04), ja01/02, sf01/02/03`
   per sex; only `sf01`/`br03` when `+0x122`); space dancers along X by
   `(i − (n−1)/2)·spacing`; hand the stage the pair of colours from the
   `map_resources` row (`+0xF8/+0x108` — consumer untraced).
5. **CharaActor::onUpdate step 0:** body `ModelNode` + rlist scale, `.b2it`
   bone lookup (`Head/Hips/Spine2/LeftForeArmRoll/RightForeArmRoll`, ground set
   `Hips/Spine2/Head/LeftToeBase/RightToeBase`), `EmotionController` for the
   three faces, part models attached to bones (forearm mirrored via
   `diag(−s,−s,−s)`), `pl_shadow00` quad; every frame thereafter: shadow
   position = mean of the ground-contact bones, size/alpha from the tallest.
6. **Animation:** the `.anm` bone tracks are evaluated at `t·fps`, the pose
   chain applies Maya segment-scale compensation, roots decompose the bind
   world matrix; `.camanm` slots → camera object → the `atan2`-based 16:9
   re-projection with position ×0.01 (§6 of the format doc, in-game verified).

Everything in 1–6 is either already reproduced by the Blender add-on (formats,
pose chain, part attachment, camera projection — all in-game verified on A3 in
2026-09) or is plain game logic readable in the A3 binary.

---

## 4. Requirements decomposition

| # | Requirement | Verdict | Mechanism |
|---|---|---|---|
| R1 | Load A3 `.model`/`.dds`/`.anm`/`.camanm` in World | **Proven-adjacent** | stock file callbacks via `asset_loader`-style arc registration (§2.1) |
| R2 | Draw skinned characters + stage geometry with correct materials/states | **Feasible, engine-owned** | hand-built render items fed to the stock passes (§5.1); shaders already resident |
| R3 | Animate dancers from `.anm` in sync with the music | **Proven math** | Rust port of `anm_dump.evaluate_pose` + the content-domain music clock |
| R4 | Per-song camera (`.camanm`) | **Proven math** | write camera slot 0 (§2.3) with the §6 re-projection |
| R5 | Stage/dancer/choreography selection like A3 | **Feasible** | rlist parsing (`parse_rlist` exists in Python), A3 logic in §3 |
| R6 | Compose beneath the lane/HUD, replacing the stock 2D background | **Feasible** | the 3D target renders before all 2D (§6.1); transparent placeholder background + movie suppression (§6.2) |
| R7 | Dancers OVER a background movie | **Not with Option A** | needs Option B or a re-attached pass (§6.3) |
| R8 | Survive song rate / restart / loop / training scrubs | **Feasible** | drive time from `song_reset`/`song_rate` clocks like assist-tick |
| R9 | Player-facing toggle, per-player character choice, persistence | **Proven** | custom_options rows (`PersistMode::Local` / `Full`) |
| R10 | Custom characters/stages via the Blender add-on | **Proven pipeline** | arcs from `tools/blender_ddr_addon` + the same loader (§7.5) |

---

## 5. Architecture options

### 5.1 Option A — hybrid: DLL scene layer on the stock 3D pipeline (RECOMMENDED)

The DLL writes the layer World deleted and nothing below it.

**Per song (GAMEPLAY entry, or the DPS init step the World
`SceneManageActor::onInitialize` `FUN_18007d700` represents):**

1. Decide stage/dancers/clips/camera set (§3) from the rlists (read once from
   `startup.arc` via `core::arc`) and the song identity the DLL already tracks.
2. Register the arcs with the FileManager (`asset_loader::load` shape —
   `agcs::FileManager::Load(path)`; set the arc's category string like A3 did
   with `"3d-motion"` if the pump filters on it — verify in the spike). The
   stock callbacks produce: KTMDL GPU resources (registry lookup by
   FNV-1(name) — World `FUN_180202d50`-family, the `FUN_180146590` analog),
   registered DDS textures, raw ANM buffers (or read the ANM bytes ourselves
   from the arc — simpler and thread-safe; the engine registry is not needed
   for animation once the evaluator is ours).
3. For every model instance build a **render item** (§5.1.1) and a **node**
   (§5.1.2); attach the node under the scene graph root
   (`root+0x18` head insertion under the manager's lock, as A3
   `FUN_18001c300` does).
4. Write **camera slot 0** (§5.1.3).

**Per frame (inside our node's `visit`):** pass 2 — sample the music clock,
evaluate the `.anm` pose (`anm_dump.evaluate_pose` semantics: local TRS,
segment-scale compensation, roots from bind world) into the node's bone-matrix
array; compute part transforms (`partExtra · bone[attach] · body`, format doc
§8) and the shadow quad; pass 4 — refresh the item exactly as
`FUN_18015b300`: world matrix, tint, `memcpy` bones → `item+0x80`, pass mask,
hidden bit, then push the node onto the visible vector. Pass 5 — return 0 (let
the engine's sphere/AABB cull run) or reproduce the ModelNode cull mask update.
The engine then composes the skinning matrices, uploads the bone textures,
sorts, binds, draws.

**Per song end:** queue the nodes in `SceneGraphManager+0x08` (the
deferred-destroy vector `FUN_180024250` flushes next frame — unlinks children
and calls our dtor), free items/textures in our dtor, `FileManager::Free` the
arcs (the `asset_loader::release` shape).

#### 5.1.1 The render-item ABI (what the DLL must build)

Read from World's consumers (collector `FUN_180263430`, upload `FUN_180261780`,
draw `FUN_180262670`) and A3's constructor (`FUN_180175ea0` + `FUN_1801762a0`
/`176360`/`1763f0`/`1764f0`/`1765d0`); the two agree field for field:

| Item offset | Content | Written by |
|---|---|---|
| `+0x00` | f32[16] world matrix (row-vector) | our visit(4) |
| `+0x40` | f32[4] `ModelParameters` = `{bone_count, 1, bone_count-or-0, 0}` → VS c22 / PS c2 (the PS `.y` is the stipple dissolve — keep 1.0) | ctor |
| `+0x50` | f32[4] tint → per-record VS c23 multiplier | our visit(4) |
| `+0x60` | GPU model resource* (`res+0x1C` bit0 skinned, `+0x20` bones, `+0x24` draw records, `+0x28` materials, `+0x30` palettes, `+0x48` bind, `+0x50` inverse bind, `+0x68` draw records ×0x48, `+0x78` materials ×0x168, `+0x88` palettes ×200) | ctor |
| `+0x68` | allocation base of the trailing arrays | ctor |
| `+0x70` | → draw records ×0x30: `{f32[4] color, gpuDrawRec* @0x10, materialCopy* @0x18, paletteCopy* @0x20, u32 flags @0x28 (bit27 hidden; `0xE0` blend bits copied from `gpuRec+0x10 & 0xF00000FF`), u32 passmask @0x2C (0xFFFFFFFF)}` | ctor (+ per-frame edits for node visibility) |
| `+0x78` | u32[2] bone-texture handles (frame parity) — `4 × bone_count` `A32B32G32R32F`, 1 mip, dynamic; created via the engine texture API (`create(w,h,mips,fmt,usage)` — `FUN_1802488e0` on 20260721 per `docs/chart_strip_hud_research.md`; lock/unlock `FUN_18024a1f0`/`FUN_18024a620` on 20260825 are what the pass uses) | ctor (skinned only) |
| `+0x80` | → f32[16] × bone_count MODEL-space animated bone matrices (seeded with bind) | our visit(2) |
| `+0x88` | → scratch matrices (only with flag `0x10` "extra" mode — not needed) | — |
| `+0x98` | → private material copies (0x168 each, from `res+0x78`; the `.sanm` write target) | ctor |
| `+0xA0` | → private palette copies (200 B each, from `res+0x88`; draw records point into them) | ctor |
| `+0xA8` | u32 mode flags — the ctor's argument: `1` base, `\|6` when the resource is skinned (bone textures), `\|8` when the node supplies a bone array (`setModel` passes 9), `0x10` = extra mode | ctor |
| `+0xAC` | u32 flags, bit0 = hidden | our visit(4) |
| `+0xB0` | u32 node pass mask (2 = OPACITY+TRANS for dancers/shadow, 4 = stage parts, 0x10 = `:N` low-priority parts) | our visit(4) |
| `+0xB4` | u32 frame stamp (atomic; the pass compares it with its own frame id to upload bone textures once) | engine |

Size 0xC8, allocated in A3 through the gs allocator function pointer
(`(*DAT_1802ee010)(size, align)` under a spin flag) — the World equivalent is
the pointer `FUN_180213dc0` uses for the item list (`DAT_1806f2070`). The engine
never frees an item on its own (A3's `ModelNode` dtor did), so the DLL may
allocate items from its own `memory::alloc_zeroed` region as long as it never
hands them to an engine free — the only engine-side ownership is the bone
TEXTURE handles, which must come from the engine's texture registry.

For non-skinned models the opaque collector builds the world matrix as
`bone[0] · invBind[0] · item.world` (rigid single-node animation — how stage
props with `_play_loop.anm` move); skinned models get the identity chain and
the VS does the rest through the bone texture.

#### 5.1.2 The node (what the DLL owns)

A 0x100-byte object with a mod-owned 2-slot-plus vtable (the
`custom_options/rows.rs::build_mod_vtable` / `foot_panel_swap::layout.rs`
shape): slot 0 dtor, slot 1 `visit(pass, ctx)`. `+0x08 = 1`, `+0x0C = 0x10 |
0x8` (update + renderable; add `0x4` only if we want the engine's pass-3
refresh), `+0x78` = the render item (REQUIRED — `SceneGraph::update` reads it
for every node on the visible vector), private fields after `+0x80` for the
bone array, part transforms, animation state. Insert under the root while
holding the manager lock (`mgr+0x28` count / `Ordinal_16/17` — A3
`FUN_18001c300` shows the exact sequence); remove through the manager's
deferred-destroy vector, never by unlinking mid-frame.

Threading: `SceneGraph::update` runs on a job-graph worker
(`GraphUpdateJob::run` `FUN_180024430`); the passes run later on the render
walker. Bone matrices are written only inside visit(2)/(4) on the update
thread and consumed by the upload under the frame stamp — the same
single-producer discipline A3 relied on. All DLL writes to item/node memory
happen inside `visit`; the game-thread side (scene callbacks) only enqueues
work.

#### 5.1.3 Camera

Slot 0 of `graph+0x38` (`SceneGraphManager` → `*mgr` → `+0x38`): write `eye`,
`target`, `up`, the frustum `l/r/b/t`, `near/far`, set the dirty bytes and let
the engine rebuild view/proj (`FUN_180220b80` ×2 + `FUN_1802376e0` in
`FUN_180023fb0`), or write `+0x08`/`+0x1C8` directly. Sample the `.camanm`
slots per frame (`anm_dump` semantics), apply the §6 re-projection
(`eye *= 0.01`, `fov' = atan2(2, w·(r−l))`, aspect 16/9). The debug camera node
keeps writing slot 1 and is irrelevant. Custom Resolution does not touch the
3D projection (the 3D target follows the output size; the aspect stays 16:9 for
16:9 outputs — the 4:3 SD plan renders at 1280×720 anyway).

#### 5.1.4 Signatures needed (all four builds)

Roughly a dozen, all in the `gs`/`agcs` layer, which was stable A3 → World
and across World builds (the `FUN_1801f6510` pass builder and
`FUN_180214570` update are byte-shape twins of A3's): `scene_graph_manager`
global (via `FUN_180023fb0`'s camera copy or the `GraphUpdateJob` vtable),
`file_manager_load/free` (exist), model-resource lookup by hash, texture
create/lock/unlock (chart-strip HUD has create+lock+unlock on 20260721),
`model_pass_globals` (optional, diagnostics), the gs allocator pointer
(optional). No detours are required for rendering at all; the feature is
"construct objects and hand them to the engine". The only detours are game
integration (scene callbacks already exist; a `SceneManageActor::onInitialize`
tap is optional).

#### 5.1.5 Why this is the right layer

- Every hard rendering problem (bone texture format, skinning shader,
  render-state table, transparency sorting, frustum culling, AA pass,
  resolution independence, present-chain placement) is the engine's, and the
  engine's answer is the one A3 shipped.
- The DLL code is almost entirely **pure and host-testable**: ANM evaluation
  (fixtures = the 222 stock `.anm`/`.camanm` decoded by `anm_dump.py`), pose
  chain, part attachment (add-on-verified maths), rlist parsing, selection
  logic, camera math. Only the item/node builders and the arc/registry glue are
  engine-facing.
- It reuses the exact data the add-on exports, so custom characters/stages
  made for A3 work unchanged (§7.5).

Costs/risks: an ABI we do not own (mitigated by the spike and by
`shape_diff.py` over the consumers on all four builds), and no z-placement
freedom (3D is always the frame floor — §6).

### 5.2 Option B — custom renderer over the 2D screen command list

The path the maintainer assumed: parse KTMDL ourselves (the Python parser is
complete), upload geometry as tag-0x05/0x06 `DrawVertices` records with real z
(`docs/custom_arrow_renderer_research.md` §4.3), bind DDS textures by registry
id (tag 0x11), skin on the CPU or in a custom `vs_3_0` via the c48+ constant
window (33 bones × 3 rows = 99 registers — fits), our own depth strategy
(gd tag 0x11 `ZENABLE` exists; the 2D lists share a depth buffer per
segment — untested), our own render-state/blend handling per mesh flag, our
own culling and sorting, shaders delivered through `shader_synthesis`.

- Pros: no engine ABI; z-placement anywhere in the 2D stack (dancers over a
  movie, §6.3); fully independent of the model passes.
- Cons: ~2–3× the engine-facing code of Option A, per-vertex CPU work or a
  bespoke skinning VS, duplicated material semantics (the `_c/_vc/_notex`
  variants, alpha test ref 0x7F, additive/subtractive), depth-buffer
  behaviour inside the 2D translator unverified, and D3DMetal shader-compile
  risk for a new VS. Correctness bar = "looks like A3", with no engine code
  to compare against.

Keep as the fallback if the spike shows the item ABI is unusable, and as the
targeted tool for R7.

### 5.3 Option C — tag-0x10 "model draw" records on the 2D list

The walker still handles a tag-0x10 model-draw record (`FUN_1802693a0 →
FUN_18026c730` on 20260616). If its payload is "render item + matrices", it
would give Option A's draw code with Option B's z-placement. Not investigated
(the arrow study deprioritised it as heavier than tag-5); worth one Ghidra
session before designing R7.

### 5.4 Option D — transplant A3 code

Not viable: the deleted functions reference A3 globals/layouts (SceneGraph
manager, resource manager, string tables) and are MSVC code with SEH/CRT
dependencies; the hook DLL is Rust. Reading them is the value (they are the
spec); executing them is not.

---

## 6. Compositing and game integration

### 6.1 Where the 3D lands in the frame

The model passes render into the RENDER-3D target before every 2D layer
(`docs/custom_resolution.md` §3a: mode 3 "direct" draws 3D into `display`,
runs `sys_copy_aa` in place — World still pays for the 3D AA pass on an empty
scene — then RENDER_2D draws on top; mode 0 on SD cabinets does the same
through the offscreen composite). The viewport clear of the RENDER target
happens every frame. So dancers/stage are automatically the **frame floor**,
below the lane, HUD, and every AFP layer — exactly where A3 put them and where
`docs/custom_shader_backgrounds_research.md` §6 wanted its shader quad.

### 6.2 Making them visible: the stock 2D background

World's gameplay background is 2D and opaque: the customize `background_gameplay`
package (`BackgroundFrame`) and/or the DirectShow movie (`BgMovieActor`, owned
by World's `SceneManageActor`), plus the "1st STAGE" banner (`StageFrameActor`,
`dance_stage` layer at the layout's `stage` marker; msg `0x104F` sets its
`stage_number_usr` label, msg `0x1052` — broadcast by the play sequence with
`{basename, stage}` — drives the layer through its vfunc `+0x20`).
For the 3D scene to show through, that content must be transparent or absent:

- Background package: the shader-background research already settled the
  mechanism — a **placeholder background arc** the game loads through its own
  backdrop manager (§6.4 there; a black `bg_root` clip in that design). For
  dancers the placeholder must be **fully transparent**, not black (black
  would cover the 3D). Same trick, alpha 0.
- Movie: `services/movie_policy`'s `MovieSuppressor` already has per-song
  contributors (`SongRate`, `NonNativeOs`); a `Dancers` contributor is a few
  lines and gives the `fake_opened` "no movie" state.
- Banner: leave it; it is a small element, and A3 showed it over the 3D too.

Song select / results are untouched — the scene graph items exist only while
our nodes are attached (GAMEPLAY ∪ ATTRACT_DEMO if we want the demo to dance).

### 6.3 Dancers over a movie (R7)

A3's own answer was "movie songs show the movie" (the movie actor is a 2D layer
above the 3D target; the dancers it created underneath were invisible). Offering
"dancers in front of the movie" as an option needs either Option B/C or a
second attachment of the model passes at a priority inside the 2D stack
(`FUN_1802666c0(targetList, viewportBase, priority)` is the registration API;
the RENDER_2D target's depth handling would have to be verified). Defer;
v1 = "3D STAGE replaces the movie/background for that song" as a user choice.

### 6.4 Song lifecycle

- **Entry:** GAMEPLAY scene callback (the DPS init steps are observable —
  `song_reset::live_dps`, `gameplay_actors`; dancer arcs can be requested at
  SONG_SELECT commit to hide load latency: `pl_*` ≈ 1 MB, `mapset_*` 1–2 MB,
  `mc_*` 2.8 MB per sex).
- **Clock:** the content-domain music clock the DLL already publishes
  (`song_reset::voice_origin`, `audio_clock`, the GamePlayActor music count) —
  `anm` time = `mc / 1000` seconds at 60 fps clips; `song_rate` follows for
  free (the clip plays at the rate the music plays). Assist-tick and
  movie_sync already consume the same feeds.
- **Restart / loop / scrub:** subscribe to `song_reset::on_song_reset` like
  movie_sync — re-seek is a time assignment, nothing to rebuild.
- **Quick-fail / exit:** scene-exit callback → deferred destroy.
- **Versus:** two dancers (A3's rule: one per entered side; `GameWork+0 == 2`
  forces two). The Multiplayer Bot's phantom side counts as entered —
  `multiplayer_bot::is_bot_side` governs whether the bot gets a dancer (fun) or
  not (A3-faithful) — a product decision.
- **Attract demo:** A3 dances the HOW TO PLAY lesson (mcode `0x94e7`, kind 4 =
  rlist row 1, stage row 32, `camera_music_lesa.arc`); reproducible if the
  demo's DPS is treated like GAMEPLAY.

### 6.5 Options and persistence

- Mods tab: `3d-background-dancers` toggle (default OFF at first, ON once
  cabinet-proven).
- PLAYER SETTINGS: `dancer_character` enum (rlist rows + RANDOM by sex/class,
  the A3 pools), `PersistMode::Full` → wire `mod_dancer_character` (bemani-buddy
  migration) or `Local` (JSON cache) until the backend exists — the same choice
  the bot rows made.
- GLOBAL SETTINGS: `dancers_background_mode` = REPLACE MOVIE / ONLY WHEN NO
  MOVIE (stock A3 behaviour) / OFF; `dancers_stage` override (AUTO = bgstage,
  or a fixed stage) for cabinets that want one look.
- Song → stage table: the modpack ships `bgstage` for the A3 catalogue
  (generated from A3's `musicdb.xml`), defaulting World-era songs to a
  deterministic pick (hash(mcode) over the stage list) or a configured stage.

### 6.6 Custom Resolution / platform notes

- 1080p/4K: the 3D target is output-sized (`render_surface_hoist` makes
  `render_depth` output-sized); the stock projection is aspect-driven, so
  nothing to patch. Post-AA `sys_copy_aa` runs regardless.
- CrossOver/D3DMetal: the model shaders are stock `vs_3_0/ps_3_0` and already
  compile at boot on every platform the modpack runs on; the skinning VS uses a
  vertex-texture fetch from an `A32B32G32R32F` texture — supported by every
  D3D9 SM3 device, but the CrossOver bottle has never exercised it under
  World. Spike item.
- Fill/vertex cost is negligible (dancers ~5–10 k tris, stages 30–50 k;
  A3 ran the same on the same cabinet class). The bone-texture lock per
  dancer per frame is the only new CPU→GPU traffic.

---

## 7. Open questions and unknowns (ranked)

1. **Render-item ABI acceptance (HIGH, spike-resolved):** does a hand-built
   item + node survive the collector/upload/draw on a real cabinet? Everything
   read says yes; nothing has run. The spike (§9.1) answers it in a week.
2. **Camera set semantics (MEDIUM):** `stage_camera_resources.rlist` lists
   several `stNNN_stNN`/`…_nonNN` sets per stage and `music_camera_resources`
   rows carry three numbers or a `name:time` cue list; the switching rules live
   in A3's `CameraActor` (`FUN_180059a60`, `FUN_180059d60` chooses the song
   set) — one Ghidra session; v0 can use the first set.
3. **Stage details (MEDIUM/LOW):** the two `map_resources` colours
   (`StageActor+0xF8/+0x108`, consumer untraced), the `:N` draw priorities, the
   `.sanm` fades (dead in A3 anyway), `EmotionController` face switching
   (cosmetic; face01 always visible is the A3 default).
4. **Loader gating (LOW):** whether the FileManager pump needs the `"3d-motion"`
   category string A3 set on motion arcs (`FUN_1801094f0(entry+0x90, …)`), and
   whether `.dds` members are guaranteed registered before the `.model` of the
   same arc is converted (A3 relied on the same arc order; member order in the
   stock arcs is `.dds` first).
5. **Transparent placeholder background (LOW):** the shader-background plan
   used black; alpha-0 art through the same backdrop path is expected to work
   but is untested. Fallback: hide the `BackgroundFrame` layer directly
   (fighting the backdrop manager's lifecycle — the known crash class from
   `docs/bm2d_background_preview_research.md`, so placeholder first).
6. **Draw-record blend semantics for tint alpha < 1 (LOW):** the runtime forces
   alpha blending + TRANS pass when the per-record colour alpha is below 1
   (format doc §3.3) — the dancer fade-in A3 shows at song start (the
   `ModelParameters.y` stipple) is optional polish.

---

## 7.5 Custom content

Because the loader and formats are A3's, everything `tools/blender_ddr_addon`
produces (bodies, parts, choreography `.anm`, stages, `.camanm`) is directly
usable: a character arc set + an rlist row (the DLL reads the rlists, so the
row can live in a mod-side rlist merged over the stock one — no `startup.arc`
repack), a `mapset_<key>.arc` + `map_resources` row, a `camera_music_<song>.arc`.
The add-on's own README playbook (UE5/Fortnite rig retarget, room → stage)
applies unchanged. World-specific bonus: the modpack can key choreography and
stage per mcode from its own tables, so custom songs get custom scenes without
touching Konami data.

## 7.6 Data delivery and licensing

The stock World install already contains the arcs (§2.5), so v1 ships **no
Konami assets** — the mod only opens files the user already has. If a data
update removes them, the fallback is the same as any other data-mod content
(operators copy their A3 `data/arc/{pl_,mapset_,mc_,camera}` into a LayeredFS
mod folder; the DLL resolves paths through `find_first_modfile` like every
other asset). The repo itself should never commit the arcs (the same rule that
keeps stock bytecode out of `data_mods/shader_fixes/blobs`).

---

## 8. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Item/node ABI differs subtly from A3 (an unread field, an unread flag) | medium | crash in the collector/upload (render-walker thread) | spike with a static prop first; `memory::is_readable` gates; every field defaulted from A3's ctor; item memory mod-owned so nothing dangles |
| Cross-build drift in the `gs` layer | low (A3→World identical; 20250805 has the same shapes) | wrong offsets on one build | `shape_diff.py` over `FUN_180263430`/`FUN_180261780`/`FUN_180262670` twins on all four builds; all-or-nothing derivation |
| Update-thread vs render-thread races | low | torn bone textures (visual) or crash | writes only inside `visit`; engine's frame stamp already serialises the upload; node removal only through the deferred-destroy queue |
| Texture registry ownership (bone textures) | medium | leak per song / registry exhaustion | create via the engine API, release in our dtor; count in the boot-log diagnostics |
| D3DMetal vertex-texture fetch | low | dancers not skinned on CrossOver | spike on the bottle; fallback = CPU skinning through the same item (write pre-skinned verts is NOT possible — fallback is Option B) |
| Memory: 5–8 arcs per song (~10 MB) on top of the song bank | low | 32-bit address pressure is not a concern (x64); load latency is | request at song-select commit; free at exit |
| Music-clock sync on rate-played / scrubbed songs | low | dancer drift | reuse `song_reset`/`audio_clock` publications (assist-tick precedent) |
| Legal: redistributing Konami arcs | — | — | don't; read from the install (§7.6) |

---

## 9. Effort and phasing

Reference points from this repo: the S-Marvelous and Song Playback Speed
features were multi-week efforts with a comparable mix of pure code and engine
glue; this one has LESS RE risk (every consumer readable, formats done) and
MORE pure code (evaluator, selection logic).

### 9.1 Phase 0 — spike (≈1 week, decides Option A vs B)

1. Register `data/arc/mapset_boom00.arc` through the FileManager at song
   start; log the model registry: `gm_boom00_footpanel` etc. become GPU
   resources (`FUN_18026f140` succeeded).
2. Hand-build ONE render item + node for the static footpanel, attach under
   the root, write camera slot 0 with a fixed camera — the footpanel appears
   behind the lane (with the background placeholder/hide in place).
3. `pl_emi00` in bind pose (bone matrices = bind; bone textures created via the
   engine API) — the skinned path renders on Windows AND CrossOver.
4. Animate with `mc_female_ne01_loop.anm` through the Rust evaluator (port of
   `anm_dump.evaluate_pose`, fixture-tested against the Python output).
5. Camera from `stage_camera.arc` `st001_st05` + music-clock sync + a
   `song_reset` re-seek.

Exit criterion: a dancing Emi on boom00 under the lane on both platforms with
zero WARNs, plus `shape_diff` green on the consumer functions across builds.

### 9.2 Phase 1 — v1 (≈3–5 weeks after the spike)

- Pure layer (host-tested): ANM/CAMANM sampler + pose chain (~800 LOC),
  rlist parser + selection logic (~400), camera re-projection (~100),
  part-attachment math (~150), bgstage table generation script.
- Engine layer: arc lifecycle, item/node builders and dtor, camera writer,
  ~12 signatures + derivations, diagnostics (~1,200 LOC).
- Game layer: song lifecycle (entry/exit/reset/versus), background
  placeholder + movie suppressor contributor, option rows + persistence,
  attract demo (~800 LOC).
- Stage props: `_play_loop.anm` on rigid parts, `:N` priorities, shadow quad.
- Validation: `scripts/validate_background_dancers.sh` (evaluator vs the 222
  stock clips, camera math vs the add-on's numbers, rlist round-trip), cabinet
  deploys per step.

### 9.3 Phase 2 — polish / stretch

Face `EmotionController` switching, dancer fade-in stipple, camera cue lists,
dancers-over-movie (Option B/C), per-song custom scenes for modded songs,
an in-menu character preview (the `bg_preview_overlay` machinery).

---

## 10. Open RE items (before or during Phase 1)

- A3 `CameraActor` (`FUN_180059a60`; set choice `FUN_180059d60`; the
  `music_camera_resources` numeric fields and `name:time` cues).
- A3 `StageActor` (`FUN_180061f30`/`FUN_180062450`): the colour pair consumer,
  per-part `_play_loop.anm` binding, `:N` priority → `node+0xE8`.
- World `SceneManageActor` step semantics (`FUN_18007d850` writes
  `DAT_1806f2d38+0x58` `+0x2D0/+0x378/+0x128` — the background/movie readiness
  gates the shader-background research also cares about).
- World texture-create API address on 20260825 (chart-strip HUD documents
  20260721's `FUN_1802488e0`); confirm the `A32B32G32R32F` (0x74) + usage
  0x2001 combination is accepted by `FUN_180164800`'s World twin.
- Option C payload (`FUN_18026c730` on 20260616) — one session.

---

## 11. Key addresses

### World 20260825

| What | Address |
|---|---|
| `Application::onBoot` (registers Model/Dds/Anime/Png/Shader/Bm2d file callbacks) | `FUN_180002060` |
| ModelFileCallback load → registry acquire → KTMDL binder → converter | `FUN_180208fc0` → `FUN_1802030b0` → `FUN_18026f140` → `FUN_180275d40` (magic @ `0x180275d51`) → `FUN_180272f80` |
| ResourceManager global / hash lookups | `DAT_1806f2f68` / `FUN_180202d50`, `FUN_180202e30` family |
| AnimeFileCallback load / byte-swap | `FUN_1801b0a40` / `FUN_1801b1c80` |
| Orphaned ANM decoder table (kind → fn) | `0x18035a560..0x18035a600` (kind 0x1C `FUN_1801f8980`, 48-bit helper `FUN_1801f7840`) |
| Render-graph boot (pass attachment, priorities 0x66–0x68) | `FUN_1801f2c30` |
| Model pass builder / pass globals | `FUN_1801f6510` / `DAT_1806f1528..1540` (DISTANTVIEW, OPACITY, LOWPRIO_TRANS, TRANS) |
| Pass draw entry / driver / collect dispatch / opaque collector | `FUN_1801f68a0` / `FUN_1802606d0` / `FUN_180261430` / `FUN_180263430` |
| Bone-texture upload / sorts / draw-sorted / material bind / pass begin | `FUN_180261780` / `FUN_180262310`,`FUN_1802624c0` / `FUN_180262670` / `FUN_1801f63f0` / `FUN_1801f6210` |
| Texture lock / unlock (used by the upload) | `FUN_18024a1f0` / `FUN_18024a620` |
| SceneGraphManager ctor / global / creation / per-frame camera copy / destroy flush / dtor | `FUN_1800238a0` / `DAT_1806f2d08` / `FUN_180023da0` (← `FUN_18002c2b0`) / `FUN_180023fb0` / `FUN_180024250` / `FUN_180023b30` |
| GraphUpdateJob::run → SceneGraph::update | `FUN_180024430` → `FUN_180214570` |
| SceneGraph ctor (item list `+0x30` cap 512, cameras `+0x38` ×2, visible `+0x58`) | `FUN_180213dc0` |
| Child recursion / frustum recursion / detach / item push | `FUN_180215ad0` / `FUN_180215a60` / `FUN_1802159e0` / `FUN_180267190` |
| Camera vector sizing (2 × 0x3F8, active by default) / active-camera find | `FUN_1802148f0`,`FUN_180214ce0` / `FUN_1800243a0` |
| debug::CameraNode ctor / vtable / visit / update (writes slot 1) | `FUN_18001f920` / `0x18035c578` / `FUN_18001fa60` / `FUN_18001fab0` |
| musicdb field descriptor table (`bgstage` u16 → entry `+0x1C8`) | `0x18047e148` (entry), strings `0x180381a70` `bgstage`, `0x180381a98` `movieoffset` |
| `StageFrameActor` ctor / init (`dance_stage` @ marker `stage`) / msgs 0x104F,0x1052 | `FUN_18007a100` / `FUN_18007a190` / `FUN_18007a360` |
| `SceneManageActor` ctor / init (movie actor) / update / msg | `FUN_18007d480` / `FUN_18007d700` / `FUN_18007d850` / `FUN_18007d970` |
| `MODEL:*` pass name strings | `0x180388520..0x180388560` |

### A3 20240402 (the specification to port)

| What | Address |
|---|---|
| `DancePlaySequence::onUpdate` (creates SceneManageActor at step 2, enables the graph at step 5) | `FUN_180039650` |
| `SceneManageActor` ctor / onInitialize / onUpdate (stage, camera, dancers, clip choice) | `FUN_18005fe00` / `FUN_180060090` / `FUN_180060460` |
| `CharaActor` ctor (kinds, pools) / onUpdate (assembly, shadow) / part attach | `FUN_18005c720` / `FUN_18005d5d0` / `FUN_18005e560` |
| `StageActor` ctor / parts / `CameraActor` ctor / set choice | `FUN_180061f30` / `FUN_180062450` / `FUN_180059a60` / `FUN_180059d60` |
| Model handle create (ModelNode + 8 AnimationNodes) / play clip by name | `FUN_18001c300` / `FUN_18001c750` |
| `ModelNode` ctor / setModel / visit / pass-4 refresh | `FUN_18015ae10` (vtable `0x180281fc0`) / `FUN_18015afb0` / `FUN_18015b220` / `FUN_18015b300` |
| Render item ctor + helpers (bone textures `FUN_1801765d0`) | `FUN_180175ea0`, `FUN_1801762a0`, `FUN_180176360`, `FUN_1801763f0`, `FUN_1801764f0`, `FUN_1801765d0` |
| SceneGraph update / ctor / manager ctor | `FUN_180159a90` / `FUN_1801592c0` / `FUN_18001cd80` |
| Model passes builder / driver / draw-sorted / material bind | `FUN_180137670` / `FUN_180176b10` / `FUN_180178aa0` / `FUN_180137550` |
| ANM object ctor / evaluator / pose chain / player | `FUN_18013a800` / `FUN_18013ab80` / `FUN_18013ba50`,`FUN_18013bc20`,`FUN_18013c000`,`FUN_18013be60` / `FUN_180158ad0` |
| Camera record apply / perspective / projection | `FUN_18001cb50` / `FUN_18001a790` / `FUN_1801a6860` |

No Ghidra symbols were added in this pass.
