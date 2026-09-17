# Task: `core/anm` — dependency-free ANM/CAMANM/B2IT/MRL0/KTMDL codecs and the A3 pose math

## Description
Port the Python reference codecs (`scripts/anm_dump.py`, `scripts/ktmdl_dump.py`) to a pure, std-only Rust
module family `src/core/anm/` so the DLL can parse the stock `.anm` / `.camanm` / `.b2it` / `.rlist` files and
the `.model` bone table from bytes, sample tracks at fractional frames exactly like the game's evaluator, seed
untracked bones from the bind pose the way A3's `FUN_18013ba50` does, build the world-matrix chain with Maya
segment-scale compensation, and turn a `.camanm` frame into the camera sample the World camera slot needs.
Every file must be `#[path]`-mountable by the host harness: no `crate::` imports, no engine types, no
`unsafe`.

## Background
Steps 1–4 proved the engine draws hand-built render items from the stock A3 arcs (bind pose). Animating them
(Step 7) needs per-frame bone world matrices, and the camera director (Step 9) needs the `.camanm` recipe;
both are pure functions of file bytes + a frame number. The Python codecs are the verified reference
(`docs/3d_model_format_research.md` §5–§6 — camera recipe verified in-game on A3; pose chain verified against
`pl_emi00`'s bind matrices), so the Rust port is judged by byte/number equality with them, not by the game.

Facts that shape the port (see Reference Documentation):
- Header: magic `0xFF010001` @0; `frame_count` u16 @4; **loop flag = u16 @6 bit 0**; u32 @8 is the fps ONLY
  when a type-4 (camera) chunk exists, else 60.0; absolute chunk offsets u32[] @0x10, 0-terminated; chunk tag
  `0xFF010002 + type`; type 0 = bone tracks (u32 rel offsets @chunk+8, 0-terminated, relative to the CHUNK),
  type 4 = six u32 rel slots @chunk+8..+0x1C (0 = absent); every other type is ignored.
- Track (16 B): `u16 kind, u16 tag, u16 key_count, u8 target, u8 sub, u32 rel→u16 times[] (0 = uniform),
  u32 rel→values`, both relative to the TRACK start. Kinds: `0x1C` rot q48 (6 B), `0x1D` pos f32×3 (12 B),
  `10` scale f32×3(+pad) (16 B), `0x1E` 3×half (6 B), `0x1F` f32×3 base + 3×half per key (base at values+0,
  deltas at values+12+6i), camera `1` quat f32×4 (16 B), `4` f32×3 (16 B), `8` f32 (4 B); `0x1B`/`0x20` STEP.
- Sampling = `anm_dump.py::sample_track` exactly (uniform: `i = floor(frame)`, `n == 1 || i >= n−1` ⇒ last
  key; explicit: `fi = floor(frame)`, `fi >= times[n−1]` ⇒ last key, `i = max k with times[k] <= fi`,
  duplicate-time skip, `u = (frame − times[i]) / (times[i1] − times[i])`); rotation ⇒ slerp (negate b on
  `dot < 0`, lerp when `1 − dot <= 1e-5`), others lerp, step kinds pick `a` while `u < 1`.
- Pose chain = `evaluate_pose`: `local = diag(s)·R(q)` (row r scaled by s[r]), translation in row 3, non-roots
  divide the 3×3 COLUMNS by the parent's scale, `world = local · world[parent]` (row-vector matrices,
  translation in elements 12..14).
- **Bind seed (A3 `FUN_18013ba50`, re-verified in Ghidra 2026-09-16):** root ⇒ `T = row 3`, `s = row
  lengths`, `q = mat→quat(bindWorld[i])`; non-root ⇒ `M = bindWorld[i] · inverse(bindWorld[parent])`,
  `T = M.row3`, `P = diag(parentScale) · M` (rows scaled by the PARENT's seed scale, `FUN_18013c000`),
  `s = row lengths of P`, `q = mat→quat(P)` (`FUN_180190a90`, trace-based with the 3 diagonal fallbacks —
  applied to the UN-normalised matrix, exactly as A3 does; identical to the normalised result on the
  unit-scale stock rigs). The row prescale is what cancels the pose chain's column division for uniform
  parent scales.
- Camera (`docs/3d_model_format_research.md` §6): slots 0 quat, 1 pos (cm), 2 fovV deg, 3 near, 4 far,
  5 aspect_file; `R = quat_to_rowmat(q)`, `eye = pos·0.01`, `target = eye − 10.0·R.row2`, `up = normalize
  (R.row1)`, `t' = tan(½·atan2(2, 2·tan(fovV·π/360)·aspect_file·aspect_mul))`, `l/r = ∓t'`,
  `b/t = ∓t'/(16/9)`, `near = slot3·near_mul`, `far = slot4`; defaults 41.53°, 0.1, 10000, 4/3.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.1 — the API
  signatures; §7.1 items 1–4 — the tests)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 5

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-16-enable-background-dancers/research/formats-and-data.md` §2 (format facts with
  Python line references)
- `docs/3d_model_format_research.md` §3.1–§3.2 (KTMDL header + bone record), §4 (B2IT), §5–§5.3 (ANM
  container, track kinds, q48), §6 (`.camanm` + the in-game-verified projection recipe)
- `scripts/anm_dump.py` (`decode_q48`, `encode_q48`, `half_to_float`, `parse_anm`, `decode_key`,
  `sample_track`, `_slerp`, `quat_to_rowmat`, `_mat4`, `evaluate_pose`), `scripts/ktmdl_dump.py`
  (`parse_rlist`, `parse_b2it`, `parse_model` bone loop), `tools/blender_ddr_addon/import_anm.py::
  game_camera_half_tangent`
- Pattern for pure modules in this feature: `src/services/scene3d/render_item_layout.rs`,
  `src/services/scene3d/node_layout.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Files `src/core/anm/{mod.rs, anm.rs, sample.rs, pose.rs, camera.rs, b2it.rs, rlist.rs, ktmdl.rs}`;
   `pub mod anm;` added to `src/core/mod.rs`. `mod.rs` re-exports the public types and hosts the shared
   little-endian reader helpers + the `Mat4 = [f32; 16]` / `Vec3` / `Quat` aliases every sub-module uses.
   No `crate::` paths anywhere in the family (sub-modules use `super::`).
2. `anm::parse(&[u8]) -> Result<Anm, AnmError>` per design §4.1: `Anm { frame_count: u16, fps: f32, loops:
   bool, bone_tracks: Vec<Track>, camera_slots: [Option<Track>; 6] }`, `Track { kind, channel, target,
   key_count, times: Option<Vec<u16>>, values: Range<usize> }`, `Channel::{Rotation, Translation, Scale,
   CamQuat, CamPos, CamScalar}`; `duration_s()`. Every read is bounds-checked (`AnmError::Truncated`); an
   unknown kind is `AnmError::UnknownKind(kind)`; `values` is the byte range of exactly `key_count` keys
   (kind `0x1F`: `12 + 6·n`). Tracks whose kind has no channel mapping are rejected, not skipped.
3. `sample::{decode_q48, encode_q48 (test helper, pub), half_to_float, sample, clip_time}` with `Sample =
   Quat | Vec3 | Scalar`; the algorithm above, bit-for-bit the Python arithmetic order where it matters
   (`(x − 16383.5) / 23169.767578125`, `sqrt(max(0, 1 − Σ))`). `frame` is `f32`; `floor` via `as i64`
   after `floor()` (Python `int()` truncates toward zero — frames are never negative after `clip_time`, but
   guard negatives by clamping to 0).
4. `pose::{Skeleton { parents: Vec<i16>, bind_world: Vec<Mat4>, inverse_bind: Vec<Mat4> }, Trs { q, t, s },
   seed_local_trs(&Skeleton) -> Vec<Trs>, evaluate(&Anm, bytes, frame, &Skeleton, &[Trs], &mut [Mat4])}`
   exactly as described in Background (row-vector `mat_mul`, `quat_to_rowmat` with the Python element
   order, `mat_to_quat` = `FUN_180190a90`, 4×4 inverse for the seed). `evaluate` skips tracks with
   `target >= bone_count` and leaves untracked channels at the seed. Parents are processed in index order
   (bones are topologically ordered in every stock file — document the assumption; a parent index ≥ i is
   treated as root).
5. `camera::{CamSample, sample_camera(&Anm, bytes, frame, near_mul, aspect_mul) -> CamSample,
   half_tangent(fov_v_deg, aspect) -> f32}` per the recipe; absent slots use the defaults.
6. `b2it::{parse(&[u8]) -> Result<Vec<(String, u32)>, FormatError>, index_of(&[(String, u32)], &str) ->
   Option<u32>}` (binary search over the file order, which is sorted ordinally); `rlist::parse(&[u8]) ->
   Result<Vec<(String, Vec<String>)>, FormatError>` (MRL0 + LE, `total == len`, records from 0x10 with
   record-relative offsets, duplicate keys preserved); `ktmdl::bone_table(&[u8]) -> Result<Skeleton,
   FormatError>` (magic `"KTMDL"`, `bone_count @0x18`, `bone_off @0x1C`, records 0xB0: bind `+0x10`,
   inverse `+0x50`, parent i16 `+0xAC`).
7. Inline `#[cfg(test)]` tests that need NO external fixture: q48 encode/decode round trip over a sweep of
   unit quaternions (incl. each `m` branch); `half_to_float` vs known bit patterns (1.0, −2.0, 0.5, a
   denormal, inf); `clip_time` clamp/wrap/negative; loop flag from a hand-built header; `sample` on a
   hand-built uniform and explicit-time track (incl. duplicate times, `fi >= last`); `mat_to_quat ∘
   quat_to_rowmat` identity on random rotations (up to sign); `seed_local_trs` + `evaluate` with NO tracks
   reproduce `bind_world` (1e-5) on a synthetic 4-bone chain with unit scales; a synthetic PARTIAL clip
   (rotation track on bone 2 only) leaves every other bone at bind and rotates bone 2 and its child;
   `camera::half_tangent` monotone-decreasing over fovV ∈ [10°, 90°] and equal to the add-on's formula
   for the stock `(41.53, 1.333)`, `(37.85, 1.5)`, `(70.4, 1.333)` pairs (worked values in §6:
   hFOV 63.2°/62.8°/46.8°); `rlist`/`b2it` parse of hand-built minimal files.
8. Rust Quality Rules: no `unwrap`/`expect`/indexing panics on FILE bytes (every slice via `get`), `f32`
   math only (the game is f32), no allocation in `sample`/`evaluate` beyond the caller's buffers.

## Dependencies
- None new (std only). `serde_json` is NOT used in this task — fixture equality lives in task 02.

## Implementation Approach
1. `mod.rs`: aliases, `FormatError`/`AnmError`, the `Le` byte-reader helper (`u8/u16/u32/i16/f32` returning
   `Option`), `mat_mul`, `mat_inverse`, `quat_to_rowmat`, `mat_to_quat`, `normalize`.
2. `anm.rs` parser → `sample.rs` decoders + interpolation → `pose.rs` seed + evaluate → `camera.rs` →
   `b2it.rs` / `rlist.rs` / `ktmdl.rs`.
3. Tests as listed; `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`.

## Acceptance Criteria

1. **Parser fidelity on a hand-built file**
   - Given a synthetic `.anm` byte image with a type-1 chunk (ignored), a type-0 chunk carrying one uniform
     `0x1C` track and one explicit-time `0x1D` track, loop bit set
   - When `anm::parse` runs
   - Then `frame_count`, `fps == 60.0`, `loops == true`, two bone tracks with the right kinds/targets/times
     and value ranges are returned; the same image with a type-4 chunk reads `fps` from header `+8`

2. **Sampling equals the reference algorithm**
   - Given the synthetic tracks above
   - When sampled at `0.0`, `0.5`, `n − 1 + 0.25`, and (explicit) a frame inside a duplicate-time run
   - Then the results equal a hand computation following `sample_track` (last-key clamp, dup skip, slerp
     shortest path)

3. **Bind seeding**
   - Given a 4-bone synthetic skeleton with non-trivial rotations/translations and unit scales
   - When `evaluate` runs with an empty clip
   - Then every world matrix equals `bind_world[i]` within 1e-5; with a rotation track on bone 2 only, bones
     0/1/3-not-descendant stay at bind and bone 2 + its child move consistently

4. **Camera recipe**
   - Given fovV/aspect pairs `(41.53, 4/3)`, `(37.85, 1.5)`, `(70.4, 4/3)`
   - When `half_tangent` runs
   - Then `2·atan(t')` in degrees equals 63.2 / 62.8 / 46.8 (±0.1°) and `t'` decreases as fovV increases

5. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc` and `cargo fmt` run
   - Then both are clean (the harness run is task 02's gate)

## Metadata
- **Complexity**: Medium
- **Labels**: core, anm, background-dancers, step-5, pure, codecs
- **Required Skills**: Rust, binary formats, quaternion/matrix math (row-vector convention), reading the
  Python reference
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 5: Pure format layer `core/anm` with Python-generated fixtures
