# Task: Python fixture generator, fixture-equality suite and the harness mount for `core/anm`

## Description
Prove the Rust `core/anm` port matches the Python reference on the REAL stock data: a generator script
(`scripts/gen_anm_fixtures.py`) reads the install's arcs in memory, evaluates every dance clip / stage loop /
stage camanm at 8 fractional frames with `anm_dump.py`, dumps the four `startup.arc` rlists, `pl_emi00.b2it`
and the `pl_emi00.model` bone table, and writes compact JSON under `tests/fixtures/anm/`; a `#[cfg(test)]`
suite in `src/core/anm/tests.rs` replays the same inputs through the Rust code and asserts equality (1e-5;
quaternions up to sign); `scripts/validate_background_dancers.sh` mounts the family and runs it.

## Background
The fixtures are values ONLY (no Konami bytes are committed): the Rust tests need the FILE bytes to parse,
so at test time the suite re-reads the same arcs from `$DDR_WORLD_INSTALL` through a tiny std-only arc
reader (`unpack_arc.py`'s format: magic `0x19751120`, u32 version, u32 file_count, u32 compression, 16-byte
cue entries `{path_off, data_off, decompressed_size, compressed_size}`; the members of interest are stored
UNCOMPRESSED — `decompressed_size == compressed_size` — so the reader refuses compressed members rather than
porting Konami LZ77). When `$DDR_WORLD_INSTALL` is unset the fixture tests SKIP with a printed note (the
synthetic tests of task 01 still run) — the harness prints which mode it ran in.

Python semantics the fixtures encode (and their limits):
- `evaluate_pose` seeds untracked channels with IDENTITY, A3/Rust seed from the BIND pose. The two agree only
  when every bone has every channel tracked; the generator therefore records per clip whether the clip is
  "fully tracked" (33 rotation + 33 translation tracks for dancers; stage loops vary) and the Rust
  equality test compares world matrices ONLY for fully-tracked clips, and for partially-tracked clips
  compares the per-track SAMPLES (q/t/s per tracked bone) instead — the pose chain is already pinned by
  the fully-tracked clips and the synthetic seed test.
- `parents` come from the type-1 hierarchy chunk (`hier["pairs"]`, `0xFF ⇒ −1`) — for dancers identical to
  the model's bone table; the generator records them so the Rust side can build the `Skeleton` for the
  world-matrix comparison from the SAME parents plus identity bind matrices (the Python chain never reads
  bind matrices).
- Camera fixtures = the six raw slot samples (`q`, `pos`, `fovV`, `near`, `far`, `aspect`) at each frame
  (absent slot ⇒ `null`) — the projection recipe itself is pinned by task 01's `half_tangent` tests and the
  §6 worked values; the Rust test derives `CamSample` from the same slots and checks the recipe algebra.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§7.1 items 1–4)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 5

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-16-enable-background-dancers/research/formats-and-data.md` §1 (which arcs hold
  what: `mc_male.arc` 16 clips incl. `ne01_loop`, `mc_female.arc` 14, `mapset_*` `_play_loop.anm` on some
  parts, `camera/stage_camera.arc` 93 camanms, `startup.arc` rlists)
- `scripts/unpack_arc.py` (`ARC(data).get_file(path)`, `list_files()`), `scripts/anm_dump.py`,
  `scripts/ktmdl_dump.py` (`parse_rlist`, `parse_b2it`, `parse_model`)
- Harness pattern: `scripts/validate_background_dancers.sh` (current), `scripts/validate_s_marvelous.sh`
  (mounts a `mod.rs` directory module via `#[path]`), `scripts/validate_song_playback_speed.sh` (a temp
  crate with `serde_json` as a dependency)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `scripts/gen_anm_fixtures.py` (Python 3, std + the two sibling codecs; `--install <dir>` overrides
   `$DDR_WORLD_INSTALL`; `--out tests/fixtures/anm`; exits non-zero with a clear message when the install
   is missing). Output files (JSON, 6-significant-digit floats via `round(x, 6)` to keep them small):
   - `dance_clips.json`: for every `.anm` in `mc_male.arc` + `mc_female.arc`: `{arc, path, frame_count,
     loops, fps, bone_count, parents, tracks: [{kind, target, key_count, uniform}], fully_tracked, frames:
     [8 fractional frames], world: [[16 floats]×bones per frame] (fully_tracked only), samples: [{target,
     channel, value}×tracks per frame] (partial only)}`.
   - `stage_loops.json`: same shape for every `*_play_loop.anm` of every `mapset_*.arc`.
   - `stage_cameras.json`: for every `.camanm` in `camera/stage_camera.arc`: `{path, frame_count, fps,
     loops, frames, slots: [[q|null, pos|null, fov|null, near|null, far|null, aspect|null] per frame]}`.
   - `rlists.json`: `{ "data/chara/chara_resources.rlist": [[key, [fields]]…], … }` for the four rlists.
   - `pl_emi00.json`: `{b2it: [[name, index]…], bones: {parents, bind: [[16]…], inverse: [[16]…]}}`.
   The 8 frames: `0, 0.25, 1.0, 7.5, frame_count/3, frame_count/2 + 0.75, frame_count − 1.5, frame_count`
   (clamped to `≥ 0`; duplicates allowed).
2. Commit the generated JSON under `tests/fixtures/anm/` (values only; regenerate with the script — never
   hand-edit). Keep the total well under 5 MB: if `dance_clips.json` exceeds ~3 MB, keep `world` for
   every clip but only 4 of the 8 frames for clips beyond the first 6 per arc (record the frames actually
   used per clip).
3. `src/core/anm/tests.rs` (`#[cfg(test)] mod tests;` from `mod.rs`): a minimal std-only `arc.rs`-style
   reader lives INSIDE the test module (or a `#[cfg(test)]` sub-module) — it is test-only, not shipped;
   fixtures are read from `ANM_FIXTURE_DIR` (env, runtime) else `tests/fixtures/anm` relative to the
   current dir; arcs from `$DDR_WORLD_INSTALL/data/arc/`; `serde_json::Value` for the JSON. Tests:
   `dance_clips_match_reference`, `stage_loops_match_reference`, `stage_cameras_match_reference`
   (slots + `sample_camera` algebra: `eye == pos·0.01`, `|target − eye| == 10`, `|up| == 1`,
   `l == −r`, `t == −b`, `r/t == 16/9`, `near == slot3·near_mul`), `rlists_match_reference`,
   `pl_emi00_b2it_and_bone_table_match_reference` (+ `bind·inverse ≈ I` and `seed_local_trs` → `evaluate`
   with no tracks reproduces `bind_world` on the REAL 33-bone rig within 1e-4), `loop_flag_matches_names`
   (`*_loop` ⇒ true, `*_exec` ⇒ false across every clip). Each test SKIPS (returns after an `eprintln!`)
   when the install or fixtures are absent.
4. `scripts/validate_background_dancers.sh`: add `#[path = "$REPO_ROOT/src/core/anm/mod.rs"] pub mod anm;`
   (a directory module — mount `mod.rs`, NOT the leaf files), `serde_json = "1"` under `[dependencies]` of
   the temp crate, export `ANM_FIXTURE_DIR="$REPO_ROOT/tests/fixtures/anm"`, and print whether
   `DDR_WORLD_INSTALL` was available (fixture tests ran) or not (skipped). Keep the existing three mounts.
5. No production code changes beyond what task 01 delivered, except `#[cfg(test)] mod tests;` in `mod.rs`.

## Dependencies
- Task 01 (`src/core/anm/*`).
- Host: `python3`, `$DDR_WORLD_INSTALL` pointing at a stock World install (for generation and for the
  fixture tests; the suite degrades to skips without it).

## Implementation Approach
1. Generator script → run it → inspect sizes → commit fixtures.
2. `tests.rs` with the arc reader + the five fixture tests.
3. Harness edits → `./scripts/validate_background_dancers.sh` green (both with and without the install).
4. `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`.

## Acceptance Criteria

1. **Every stock clip matches**
   - Given `$DDR_WORLD_INSTALL` and the committed fixtures
   - When `./scripts/validate_background_dancers.sh` runs
   - Then `dance_clips_match_reference` / `stage_loops_match_reference` pass over every clip (30 dance clips,
     every stage `_play_loop`) at all recorded frames within 1e-5 (quaternion samples up to sign) and
     `stage_cameras_match_reference` passes over all 93 camanms

2. **Static tables match**
   - Given the same
   - When the rlist / b2it / bone-table tests run
   - Then all four rlists (26/34/34/12 rows), the `pl_emi00` b2it (33 names) and bone table (33 bones,
     parents + bind + inverse) equal the Python dumps exactly (strings) / within 1e-6 (floats), and the real
     rig's bind pose is reproduced by the seed path

3. **Graceful without the install**
   - Given `DDR_WORLD_INSTALL` unset
   - When the harness runs
   - Then the fixture tests skip with a printed note, the synthetic tests pass, and the script exits 0

4. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`,
     `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: core, anm, background-dancers, step-5, fixtures, harness, python
- **Required Skills**: Python 3, Rust tests, the repo's temp-crate harness pattern
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 5: Pure format layer `core/anm` with Python-generated fixtures
