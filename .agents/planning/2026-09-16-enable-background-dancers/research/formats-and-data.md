# Formats, Data Inventory and Python Reference Semantics

Compiled 2026-09-16 from `docs/3d_model_format_research.md`, `scripts/anm_dump.py`,
`scripts/ktmdl_dump.py`, `tools/blender_ddr_addon/`, and read-only probes of the stock World install
(`$DDR_WORLD_INSTALL/data/arc/`). The render-item ABI and node protocol are NOT repeated here — use
`docs/background_dancers_feasibility.md` §5.1.1–§5.1.3 (authoritative tables).

## 1. Stock data (World install; byte-identical to A3)

- `data/arc/`: 115 `pl_*.arc`, 26 `mapset_*.arc`, 24 `mc_*.arc`, `camera/stage_camera.arc` (93 `.camanm`:
  `etc/in/chara_in01..03`, `etc/out/chara_out01..03`, `long/floor/*` 8, `long/st001..st006/*` 12–15 each),
  11 `camera/camera_music_{butt2,dace,dacr,danf,drem,insp,iyhr,lesa,lovy,tare,will}.arc`.
- `mapset_*`: `boom00 boom00_g boom01..06 club00 crystaldium00 cyber00 dawnstreet00 disco00 floor00
  lovesweets00 monitor00..03 replicant00..05 speaker00`.
- `mc_*`: `mc_male.arc` (16 exec: the 13 pool + `ht04 br03` (in pool) + `tu01` (NOT in A3's pool) + `ne01_loop`),
  `mc_female.arc` (13 exec + `ne01_loop`), `mc_bpm120.arc` (dead), 10 song-specific pairs + `mc_male_lesa`.
- **Member order inside arcs is `.model` first, then `.dds`** (e.g. `pl_emi00.model @256`, `mdx_emi01.dds
  @90752`) — the feasibility doc's "dds first" premise is wrong; FileManager dispatch order vs. table order is
  unverified (loader-gating item).
- `mapset_boom00.arc` (34 members): parts `gm_boom00_{bg,ripple,sp,spot,stage,footpanel}` each with
  `.model`, `.b2it`, `.grp2it`, DDS textures (`jx_st001_NN.dds`, `footPanel.dds`), `_play_loop.anm` on
  `ripple/sp/spot/stage` (+ dead `.tanm/.sanm` on `bg`/`stage`).
- `pl_emi00.arc`: `pl_emi00.model` (139 KB), `mdx_emi01.dds` (1.3 MB), `.grp2it`, `.b2it`.

### rlists (from World `startup.arc`, parsed with `scripts/ktmdl_dump.py::parse_rlist`)

`data/chara/chara_resources.rlist` — 26 rows `key → [pl, sex, class, model_scale, shadow_scale, unlock_id]`:
```
[0] yuni00 F A 0.9 0.75 0.0    [1] rage00 M A 1.0 0.8 0.0     [2] afro00 M A 1.0 1.0 0.0
[3] jenny00 F A 1.0 0.8 0.0    [4] emi01 F A 0.9 0.75 0.0     [5] babylon00 M B 0.4 0.5 0.0
[6] gus00 M A 1.0 0.9 0.0      [7] ruby00 F A 0.9 0.75 0.0    [8] alice00 F A 0.9 0.85 0.0
[9] julio00 M C 0.8 0.75 0.0   [10] bonnie00 F A 1.0 0.8 0.0  [11] zero00 M A 1.0 0.8 0.0
[12] rinon00 F C 0.65 0.65 0.0 [13] emi02 (16.0) [14] alice01 (17.0) [15] rinon02 (18.0) [16] yuni02 (19.0)
[17] rinon01 (-1.0) [18] concent00 M A 0.95 1.15 -1.0 [19] zukin00 F A 0.9 1.0 -1.0 [20] pix00 M B 0.4 0.45 -1.0
[21] emi00 F A 0.9 0.75 -1.0 [22] yuni01 (-1.0) [23] rage01 (-1.0) [24] afro01 (-1.0) [25] jenny01 (-1.0)
```
`data/map/map_resources.rlist` — 34 rows `key → [rgb_hex, rgb_hex, part[:prio]…]`; rows 6, 8–13 are `dummy00`;
`boom00` at rows 0 and 32 (32 adds `footpanel`); `monitor00` at 18 and 24; `replicant00..05` at 19–21, 25, 26, 33;
`boom01..06` at 14, 27–31 (same part list). Every colour field is `000000`. Priorities seen: `bg:-2`, `stage:-1`,
`back:-5 … stage4:-4`, `pole:-1`, `ble:-1`, `star2:-2`, `chain:-1`.
`data/camera/stage_camera_resources.rlist` — 34 rows parallel to the stage rows, 10–13 set names each
(`stNNN_stNN`, `stNNN_nonNN`, `stNNNx2_stNN`, `floor_stNN`); the `.camanm` lives at
`data/camera/long/<name[:5]>/<name>.camanm` (`st001x2_st06` → dir `st001`).
`data/camera/music_camera_resources.rlist` — 12 rows: `butt2 [0 1 1 music_butt2]`, `dace [531 1 1 music_dace]`,
`dacr [605 1 1]`, `danf [461 0.1 1]`, `drem [664 1 1]`, `insp [648 1 1]`, `iyhr [0 0.1 1]`, `lovy [1 1 1]`,
`tare [695 0.01 1]`, `will [0 0.01 1]`, `lesa [0 1 1]`, `mawa [0 1 0.75 st006_non04:2.8 … st006_st05:100.0]`.
(Field semantics: see `a3-runtime-rules.md` §4.)

### A3 `musicdb.xml` `<bgstage>` prior (bottle sibling install, 1099/1221 songs)
Stages 2/3/14/15/16/17 = 77 %; never used in stock: 0, 1, 6–13 (`dummy00`), 24, 32. Irrelevant under the
random-stage requirement; useful only as a "which stages were shown most" hint.

## 2. Format facts a Rust port must reproduce (with Python reference)

- **q48 rotation keys** (`anm_dump.py:83-104` `decode_q48`/`encode_q48`): `a=(v>>32)&0x7FFF, b=(v>>17)&0x7FFF,
  c=(v>>2)&0x7FFF, m=v&3`; `f(x)=(x−16383.5)/23169.767578125`; `D=sqrt(max(0,1−(A²+B²+C²)))`; component order
  by `m`: `(D,A,B,C)`,`(A,D,B,C)`,`(A,B,D,C)`,`(A,B,C,D)` as `(x,y,z,w)`.
- **Track sampling** (`sample_track`, `anm_dump.py:411-431`): `frame = t·fps`; uniform keys `i=floor(frame)`,
  clamp at `n−1`; explicit times `times[i] ≤ floor(frame) < times[i1]` with duplicate-time skip; `u` linear;
  rotation → `_slerp` (negate on `dot<0`; lerp when `1−dot ≤ 1e-5`), others lerp; `STEP_KINDS={0x1B,0x20}`.
  Kinds used by stock dance clips: `0x1C` rot (6 B q48), `0x1D` pos (12 B f32×3), `10` scale (16 B), plus
  `0x1E` (3×half) and `0x1F` (f32×3 base + half deltas) on a few. `.camanm` slots use kinds `1`,`4`,`8`.
- **Pose chain** (`evaluate_pose`, `anm_dump.py:437-486`): `local = S·R(q)` (rows scaled), translation in
  row 3; for non-roots divide the 3×3 COLUMNS by the parent's scale (Maya segment-scale compensation), then
  `world = local · world[parent]` (row-vector). A3 seeds bones WITHOUT tracks from
  `bindWorld[i]·inverse(bindWorld[parent])` (`FUN_18013ba50`) — the port must do the same (Python defaults
  to identity; fine for the 33-track dance clips, wrong for partial clips).
- **Part attachment** (`tools/blender_ddr_addon/import_character.py`, verified in-game on A3):
  `part_world = E · animatedBone[attach] · body_world`, `E = diag(s,s,s,1)` (rlist `model_scale`), mirrored
  right forearm `E = diag(−s,−s,−s,1)` as a second instance of the LEFT forearm model. Attach bones:
  `head/face → Head`, `hips → Hips`, `chest → Spine2`, `forearm → LeftForeArmRoll` (+ mirror on
  `RightForeArmRoll`). Bone indices come from the body's `.b2it` (`parse_b2it`, `ktmdl_dump.py:753-760`:
  sorted `(name, index)` pairs, binary-searchable).
- **Camera** (`docs/3d_model_format_research.md` §6; `import_anm.py:145-149`): from the six `.camanm` slots
  `q, pos_cm, fovV_deg, near, far, aspect_file`: `R = quat_to_rowmat(q)`; `eye = pos·0.01`;
  `target = eye − 1000·R.row2·0.01` (same shift); `up = normalize(R.row1)`;
  `t' = tan(½·atan2(2, 2·tan(fovV/2)·aspect_file))`; frustum `l/r = ∓t'`, `b/t = ∓t'/(16/9)`; near/far
  unscaled (then × the music-camera `f1` multiplier); projection `m00=2w/(r−l), m11=2w/(t−b),
  m22=−far/(far−near), m23=−1, m32=−far·near/(far−near)`. Monotonically DECREASING in file FOV.
- **Skinning contract** (§3.6): bone texture = 3 float4 rows per bone at v = 0.125/0.375/0.625 of
  `invBind[i]·bone[i]` (row = output component); `ModelParameters.x` = bone count; palette-local
  `BLENDINDICES` are remapped to global indices by the loader.
- **Mesh flag → render state** table (§3.3) is applied by the ENGINE from the GPU draw record; the DLL only
  sets node pass masks (2 dancers/shadow, 4 stage, `0x10` `:N` parts) and per-record colour alpha (< 1 forces
  the TRANS path).
- **KTMDL header** fields the DLL may need to read from the FILE (if not reading them from the GPU resource):
  `bone_count @0x18`, `bone_off @0x1C` (records 0xB0: bind `+0x10`, inverse bind `+0x50`, parent `i16 @0xAC`),
  `palette_count @0x20`, `mesh_count @0x28`. The GPU resource exposes the same via `res+0x20/+0x48/+0x50`
  (feasibility §5.1.1).

## 3. Present chain placement (from `docs/custom_resolution.md` §3a)

Viewport order `0x65 OFFSCREEN1 → 0x66 RENDER (3D) → 0x67 AFTER RENDER 3D → 0x68 RENDER_2D → 0x69 DISPLAY
→ 0x6a PRESENT`. The three attached model passes sit inside `0x66` (priorities 0x66/0x67/0x68). RENDER is
cleared (colour+depth+stencil) every frame; RENDER_2D clears DEPTH ONLY, so 3D colour survives under the 2D.
Mode 3 (stock HD) draws straight into `display` and runs one in-place `sys_copy_aa` — World already pays the
3D AA pass on an empty scene.

## 4. Texture API (engine dynamic textures) — implementation-time RE

Documented on 20260616/20260721 (`docs/chart_strip_hud_research.md:91-109`): `create(w, h, mips, fmt, usage)`
= `FUN_1802488e0`, lock `FUN_180248eb0` → `(ptr, pitch)`, unlock `FUN_1802492e0`; the model pass's own
lock/unlock on 20260825 are `FUN_18024a1f0` / `FUN_18024a620`. Bone textures: `4 × bone_count`,
`A32B32G32R32F` (fmt 0x74), usage 0x2001, two per skinned item (frame parity). **No DLL wrapper exists and the
RELEASE call for a created handle is undocumented** — must be RE'd from the A3 render-item dtor
(`FUN_180175ea0` family) / World's texture manager before the skinned spike step.
