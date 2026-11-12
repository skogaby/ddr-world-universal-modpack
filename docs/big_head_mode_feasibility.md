# Big Head Mode (Background Dancers) — Feasibility (2026-09-27)

**Question.** What would it take to add a **Big Head** row to the Background Dancers section of the
mod menu's GLOBAL SETTINGS tab that, when ON, draws the head of every background dancer (stock A3
and custom) at **3×** its size?

**Scope.** This began as a feasibility study and implementation plan. It builds on the shipped
Background Dancers machinery and its RE notes (`docs/background_dancers_research.md`,
`docs/3d_model_format_research.md`, `docs/background_dancers_feasibility.md`). Engine addresses
are file-relative to `gamemdx.dll` at `0x180000000`, World build **20260825** (the build the
research notes use). The geometry figures in §5 come from a read-only survey of the stock install
and the in-repo custom dancers, described in the appendix.

**Status (2026-09-27): implemented, not yet cabinet-tested.** The maintainer settled the §7
decisions after reviewing a Blender contact sheet of all 31 dancers at 1× and 3× (§7). The code
follows §6 with one deviation: the row is **live** rather than latched per song (§6.4). Host tests
pass (`scripts/validate_background_dancers.sh`) and `cargo check` is clean. The §9 cabinet pass is
still to do.

## TL;DR

**Feasible and small. No new RE, no new signatures, no detours and no data_mods assets are
needed.** A DLL-only deploy is enough.

- World has no dancer animation code. **The mod computes every dancer pose itself**, once per frame
  on the game thread: `director::produce` evaluates each dancer's clip into model-space bone
  matrices and publishes them on the frame board. So Big Head is a few lines between
  `evaluate_into` and `frame_board::publish`. Scale the `Head` bone matrix by 3 about its own
  joint: rows 0–2 × 3, translation untouched.
- That one edit reaches every place the head is drawn:
  - the skinned head,
  - the rigid head parts (`head00`, `face01`),
  - every outline hull twin,
  - the song-select previews.

  The floor shadow is unchanged, because it only reads the Head *translation*.
- On the engine side, nothing needs changing, and this was checked in the code:
  - The bone upload multiplies our matrices by the inverse bind, so the scale reaches the GPU.
  - The per-draw frustum cull tests skinned meshes **through our animated bone matrices**
    (`FUN_180260970`, §3.2), so a 3× head is culled correctly.
  - The lit and cel shaders renormalize normals, and the scale is uniform.
- There is a precedent. A3 itself ran a Rinon whose Head-weighted vertices were baked ×3 about the
  Head joint offline, and she "dances correctly" (`docs/3d_model_format_research.md` §10, line
  1176). The runtime transform proposed here gives exactly that result at rest (§1).
- **Work:**
  1. a `big_head` config key,
  2. one OFF/ON row in `style.rs`,
  3. a `Head` subtree resolved at parse time,
  4. a live scale factor read by the director every frame (§6.4),
  5. a pure helper in `director_math.rs` with host tests,
  6. README and module-doc updates.

  That is roughly 100–150 lines including tests. §6 has the file-by-file plan.
- **Cosmetic effects** (all accepted in §7):
  - The chin drops 0.1–0.2 m into the collar.
  - Long hair weighted to `Head` stretches downward. Yuni's reaches the waist, and Rinon's and the
    custom Miku's go through the floor.
  - 3× heads crop at the top of the song-select preview box and in the FULLSCREEN movie mode's
    medium shots.
  - In versus, the two heads can touch.

## 1. The transform

The dancer bones the director produces are **model-space bone matrices**. A bone matrix maps
bone-local space to model space. It is row-major, row-vector (`p' = p·M`), with the translation in
elements `[12..14]` (`src/core/anm/mod.rs`). At rest it equals the KTMDL bind matrix. The engine
skins each vertex with `invBind[i] · bone[i]`, blended over up to 4 weights.

**Scaling a bone by `k` about its own joint** is a local-space pre-multiply:

```
bone'  = diag(k, k, k, 1) · bone        rows 0–2 × k; row 3 (the joint position) unchanged
```

For a general subtree (the head bone plus any descendants), apply the same model-space scale about
the head joint `j` to every member:

```
bone_b' = bone_b · C,   C = [ k·I      0 ]      (row-vector; j = head bone translation)
                            [ j·(1−k)  1 ]
```

For the head bone itself (`t = j`) this reduces to the first form. In code, rows 0–2 are
multiplied by `k` and the translation becomes `t' = j + k·(t − j)`.

**At rest**, a body vertex `v` with Head weight `w` (and `1 − w` on Neck, etc.) lands at

```
v' = j + (v − j) · (1 + (k − 1)·w)
```

This is the formula of the A3 offline experiment, which was confirmed in-game on A3. It blends
smoothly across the neck. A rigid part hung off `Head` is scaled by `k` as an object about `j`,
which is exactly the offline experiment's part-transform baking.

**`k` must stay uniform.** Two things depend on it:

- The lit and cel vertex shaders recover the projection from `|World row 0|`, which assumes a
  uniform World scale (`shaders/src/mdl_common.hlsli` `view_frame`). A rigid part's World now
  carries the 3× (`part_world = bone · body_world`), and it stays uniform.
- Normal renormalization is exact only under uniform scale.

So there is no squash-and-stretch. Use a single `k` on all three axes.

## 2. Where it plugs in — the pose pipeline today

1. **Evaluate (game thread, every frame).** `director::produce` (`src/mods/background_dancers/director.rs:37`)
   loops over dancers (`:90–156`). For each one it:
   - picks the clip and local time from the schedule,
   - calls `core::anm::pose::evaluate_into` (`src/core/anm/pose.rs:159–205`), which samples the
     local TRS per bone, builds `diag(s)·R(q)` with Maya segment-scale compensation, and composes
     `local · world[parent]` into the session's shared `sess.bones` buffer (`session.rs:641–643`,
     sized once).
2. **Publish the body.** `frame_board::publish(body_slot, &body_world, …, &sess.bones[..n])`
   (`director.rs:147–156`; the seqlocked board is `src/services/scene3d/frame_board.rs:79`, 32
   slots × 64 bones).
3. **Derive the children from the same `sess.bones`** (`director.rs:158–213`):
   - Parts: `world = part_world(mirror, &bones[attach], &body_world)` = `E · bone · body_world`
     (`director_math.rs:115–121`), published with `[IDENTITY]` bones.
   - Shadow: reads only the ground bones' translations (`bones[g][12..14]`, Head is one of them)
     and the Hips height (`director.rs:183–197`).
4. **Copy into the render item (engine job thread).** The scene node's `visit(2)`
   (`src/services/scene3d/node.rs:94–125`) copies the slot's world, tint, bones and hidden flag
   into the item. Hull twins have no slot of their own. They read their body's or part's slot
   (`instance_plan.rs:57–60`), so they copy the same bones into their own item.
5. **Engine.** The engine uploads `invBind · bone` into the bone texture (`FUN_180261780`), skins
   the vertices on the GPU from sampler `s3`, and runs the per-draw frustum cull (§3.2).

| Consumer | Sees a scaled `bones[Head]`? | Why |
|---|---|---|
| Skinned body (head, and hair weighted to `Head`) | **Yes** | published bones |
| `head00` / `face01` rigid parts | **Yes** | `part_world` multiplies the full bone matrix, scale included. A3 rebuilt attachments from rotation + translation only, so this reach is a property of the mod, not of A3 |
| Outline hull twins (INK / LAYERED) | **Yes** | read the owner's slot |
| Floor shadow | No (correct) | translation-only read; scaling about the joint leaves row 3 alone |
| Song-select previews | **Yes**, if applied unconditionally | previews run the same `SceneWindow::publish → director::produce` path (`scene_window.rs:358–362`) |
| Movie modes (FULLSCREEN / MOVIE ONLY) | **Yes** | only the scene mask and camera set change |
| Stage props | No (correct) | separate loop (`director.rs:44–88`) |

Because `evaluate_into` rewrites every bone of the buffer each call, an in-place scale is
idempotent per frame and cannot leak across dancers or frames.

## 3. Engine side — nothing to change (verified)

### 3.1 Bone upload and skinning

The upload (`FUN_180261780`) reads `item+0x80`, which holds our bones, and `res+0x50`, the inverse
bind. It writes `invBind[i]·bone[i]` as three float4 rows per bone
(`docs/background_dancers_research.md` §2.3–§2.4). Those are full 3×4 affine matrices, so scale is
carried. Both the stock `gs_model_skinning_default` VS and our `_lit` / `_cel` variants blend 4
bones from those rows (`shaders/src/mdl_common.hlsli` `skin_rows` / `skin_apply`).

### 3.2 Frustum culling — self-correcting (closes an open question)

The earlier notes left one question open. `docs/3d_model_format_research.md` §3.3 says (A3
wording) "skinned meshes are culled by their palette bones' AABBs", while
`docs/background_dancers_research.md` §2.4 describes a sphere test on the mesh record. If World had
culled a skinned mesh by its static bind-pose sphere, a 3× head could pop out at frame edges.
Decompiling the World collector settles it.

- **`FUN_180263430`** (the model-pass collector):
  - Rigid resources (`res+0x1C bit0 == 0`) get `world = bone[0]·invBind[0]·item.world`. Skinned
    ones keep `item.world`.
  - **`FUN_18026ca40`** stores that world at `ctx+0xC0` and `World·ViewProj` at `ctx+0x100`, and
    binds them as VS c14 (cmd `0xE`) and c18 (cmd `0x12`).
  - Each draw record is then culled by `FUN_180260970(&WVP, rec, item)`. A non-zero result means
    culled.
- **`FUN_180260970`** builds the axis-aligned box of the mesh's bounding sphere, taken from
  `gpuRec+0x00..+0x0C` (centre, radius), then:
  - **Rigid:** it pushes the box's 8 corners through `WVP`. For a head part, `WVP` already contains
    `bone_Head' · body_world`, so the box grows 3× with the part.
  - **Skinned:** for **each palette bone** `b` (the `u8` list at `gpuRec+0x40`, count at
    `gpuRec+0x1C`) it forms `invBind[b] (res+0x50) · bone[b] (item+0x80) · WVP` and pushes the box
    through it. The record is drawn if **any** palette bone's box survives. The bone matrices are
    the ones we published, so the Head bone's box is scaled 3× about the joint with the head.

So with Big Head the cull volume follows the scaled head and nothing is culled early. For
example, Emi's head-only mesh 0 has a small bind sphere (centre y 1.51, r 0.15), but the box is
pushed through the *scaled* Head skin matrix, so it covers the 3× head. The depth sort key uses the
record centre through `WVP` and is unaffected. Only translucent draw order depends on it.

### 3.3 Shaders

| Shader | Effect of a uniform 3× scale |
|---|---|
| Stock (unlit) | uses no normals |
| `mdl_lambert` / `mdl_cel` | normalize the skinned or world normal before lighting (`mdl_lambert.hlsl:108`, `mdl_cel.hlsl:110`) |
| Outline hull | offsets in **screen pixels** along the renormalized projected normal, so the rim stays the configured width on a 3× head |

Across the neck, the linear blend of a 1× Neck matrix and a 3× Head matrix gives a smooth flare,
as on A3.

## 4. Finding the head

- **By name, through the body's `.b2it`.** It is the same lookup the mod already does for parts,
  ground bones and Hips (`session.rs:306–327`, `bone_index`). The constant `"Head"` already appears
  in `selection.rs` (`part_attach_bone`, `:213`; `GROUND_BONES`, `:226`). Add `HEAD_BONE = "Head"`
  beside `HIPS_BONE`.
- **Survey facts**, over all 26 stock bodies and the 5 in-repo custom dancers (Big Smoke, Carl
  Johnson, Hatsune Miku, Kasane Teto, Peter Griffin):
  - Every rig has 33 bones, with `Head` at index 16 and parent 11 (`Neck`).
  - `Head` has **no children** on any rig. Hair and face accessories are skinned or part models,
    not bones.
  - The Blender add-on enforces the donor 33-bone rigs
    (`tools/blender_ddr_addon/README.md`, "must not be moved, renamed, reordered, added or deleted").
- **Do not hard-code 16.** The runtime does not enforce 33 bones on custom content. Resolve by
  name, and precompute the Head *subtree* (Head plus descendants, a single topological pass over
  `Skeleton::parents`). On every known rig the subtree is just `[16]`, but the general form costs
  nothing and keeps a future rig with jaw or hair bones under `Head` coherent.
- **No `.b2it`:** the body already dances without parts or shadow (`session.rs:316–321`). Big Head
  should degrade the same way (empty subtree, no-op) and extend that WARN text.
- **Tracks don't interfere.** Dance clips are fully tracked (33 rotation + 33 translation tracks)
  and carry no scale tracks. Since the edit is applied to the *evaluated matrix*, it would override
  a scale track anyway.

## 5. What it will look like — geometry survey (3×, at rest)

These figures are in model units (metres before the rlist `model_scale`) unless marked world.
"Chin" is the lowest vertex with Head weight ≥ 0.9. "Lowest" is the lowest vertex with any Head
weight after scaling. Joint heights: Head ≈ 1.478–1.490, Neck ≈ 1.389 (1.43–1.44 on the class-B
mascots), shoulders (`LeftArm`) ≈ 1.33–1.35.

| Dancer | rlist scale | Head top 1× → 3× | **3× top, world** | Chin 1× → 3× | Lowest any-weight 3× (w) | Notes |
|---|---|---|---|---|---|---|
| emi00 | 0.9 | 1.67 → 2.07 | **1.86 m** | 1.41 → 1.29 | 1.29 (1.0) | typical female |
| rage00 | 1.0 | 1.73 → 2.23 | **2.23 m** | 1.40 → 1.23 | 1.23 (1.0) | typical male |
| afro00 / 01 | 1.0 | 1.73 → 2.23 | **2.23 m** (afro part to 2.47) | 1.40 → 1.21 | 1.21 | `head00` part |
| concent00 | 0.95 | 2.06 → 3.21 | **3.05 m** | 1.40 → 1.21 | 1.21 | tallest head |
| zukin00 | 0.9 | 1.83 → 2.54 | **2.29 m** | 1.40 → 1.25 | 1.25 | hood |
| babylon00 | 0.4 | 2.89 → 5.71 | **2.28 m** | 1.41 → 1.25 | 1.25 | class-B mascot, already big-headed; 3× half-width ≈ 0.79 m world |
| pix00 | 0.4 | 2.00 → 3.05 | **1.22 m** | 1.14 → **0.45** | 0.45 | class-B mascot, becomes nearly all head (half-width ≈ 0.92 m world) |
| yuni00–02 | 0.9 | 1.64 → 1.97 | **1.77 m** | 1.28 → **0.90** | 0.90 (1.0) | hair fully on `Head` reaches the waist |
| rinon00–02 | 0.65 | 1.81 → 2.48 | **1.61 m** | 1.32 → 1.04 | **−0.07 (0.2)** | long hair partly on `Head` reaches the floor |
| bonnie00 | 1.0 | 1.70 → 2.14 | **2.14 m** | 1.41 → 1.28 | 0.70 (0.12) | light hair weights |
| julio00 | 0.8 | 1.68 → 2.08 | **1.66 m** | 1.39 → 1.18 | 1.18 | — |
| *custom* Hatsune Miku | 0.9 | → 2.11 | **1.90 m** | → **−1.90** | **−1.90 (1.0)** | twin tails fully on `Head`, deep through the floor |
| *custom* Kasane Teto | 0.9 | → 2.39 | **2.15 m** | → 1.42 | 1.39 (0.12) | — |
| *custom* Big Smoke / CJ / Peter | 1.0 / 1.0 / default | → 2.10 / 1.88 / 2.05 | ≈ 1.9–2.1 m | → 1.52 / 1.30 / 1.31 | ≥ 1.29 | — |

The remaining stock bodies fall inside these ranges: alice, emi01/02, gus, jenny, rage01, ruby and
zero have 3× world tops of 1.76–2.17 m.

**What this means on screen:**

- **Chin.** Scaling about the joint pushes the jaw down by 2 × (joint − chin) ≈ 0.13–0.2 m. It
  ends at or below shoulder-joint height, so the head sits directly on the shoulders and the jaw
  clips into the upper chest (the "NBA Jam" look). §7 D1 covers a chin-anchored pivot if the
  maintainer prefers a stretched neck instead.
- **Hair weighted to `Head`** grows downward by the same factor. The skinning is per bone, so there
  is no runtime way to tell hair vertices from face vertices on one bone. Yuni's hair reaches the
  waist, Rinon's reaches the floor, and Miku's tails go through it (the floor depth-occludes them,
  so they vanish into it). §7 D4.
- **Stage cameras** are unaffected. The stock sets frame from 5–17 m with 4–12 m of visible height
  at the dancer (`docs/background_dancers_research.md` §7.4). The nearest stock eye comes about
  1.4 m from the head, which makes a big close-up, but near planes are 0.1 / 0.01 m, so nothing
  clips.
- **Song-select preview.** The fixed dancer camera (eye (0, 1.05, 3.4) → (0, 0.95, 0),
  half-tangent 0.32; `preview/layout.rs:30–35`) shows up to about **2.04 m**. Emi-class heads fit,
  male and tall heads crop at the top, and concent00 crops heavily. §7 D2.
- **FULLSCREEN movie camera set** (`scripts/gen_movie_cameras.py`, framing check assumes a 0.24 m
  Head extent). The frame top is about 1.9 m on medium shots and about 2.05 m on full shots, so
  most big heads crop in the mediums. §10.
- **Versus.** Dancers stand at ±0.8 m and the hips wander ±0.55–0.75 m. With 3× head half-widths
  of 0.3–0.9 m (world), the two heads can intersect when the choreography brings the dancers
  together. This is accepted as part of the look.

## 6. Implementation plan

Everything below is inside `src/mods/background_dancers/` plus `src/mods/config.rs` and `README.md`.

### 6.1 Config — `src/mods/config.rs`

Add `big_head: bool` to `BackgroundDancersConfig` (`:447–523`) with `#[serde(default)]` (false),
and set it in `Default` (`:534–549`). The doc comment should say it applies live and is shown
under Background Dancers in GLOBAL SETTINGS.

There is no `big_head_scale` key: D3 settled on ON = 3× with no size options.

### 6.2 Row, live value, persistence — `style.rs`

1. Add `ROW_KEY_BIG_HEAD = "background-dancers-big-head"` next to the other keys (`:37–48`).
2. Add `static LIVE_BIG_HEAD: AtomicBool` (`:57–75`) and a getter:
   `pub fn head_scale() -> f32 { if LIVE_BIG_HEAD.load(Relaxed) { BIG_HEAD_SCALE } else { 1.0 } }`.
3. `init_from_config` (`:214–321`): seed the value from `bd.big_head` and add it to the scene-style
   INFO line.
4. `persist_section` (`:324–345`): **emit `"big_head"`**. The section is rewritten whole on every
   edit (AGENTS.md cross-cutting rule), so a missing key is silently reset by the next edit of any
   Background Dancers row.
5. `register_rows` (`:438–555`): add one enum row (`values [0, 1]`, labels `OFF`/`ON`), with
   `parent_row_key = MOD_ID`, label `"Big Head"`, the hint *"Every dancer's head at 3x size, hair
   and head accessories included (previews too). Applies immediately."*, and
   `on_change: set_big_head` (store, persist, INFO). Rows render in registration order
   (`mod_menu/model.rs::build_global_tab`), so per D5 it is registered **last**, after CUSTOM
   DANCERS & STAGES.
6. Update the module `//!` header.

Mod-menu rows are plain strings, so there are **no generated label PNGs** (unlike `custom_options`
rows) and nothing new in `data_mods/`.

### 6.3 Parse — `session.rs`, `selection.rs`

- `selection.rs`: `pub const HEAD_BONE: &str = "Head";`.
- `ParsedDancer` (`session.rs:107–124`): add `head_subtree: Vec<usize>`, empty for no Head or no
  `.b2it`. In `parse_pick`, set it next to `ground` / `hips` (`:376–377`):
  `bone_index(HEAD_BONE).map(|h| subtree_of(&skeleton.parents, h)).unwrap_or_default()`.
  Extend the `.b2it`-missing WARN text (`:312`, `:318`) to name Big Head.
- This runs on the parse thread, is std-only, and allocates once per song.

### 6.4 Live factor — no session state (as built)

The original plan latched the factor per session (a `Session::head_scale` field set through a
`with_head_scale` builder in `lifecycle.rs::drive_live` and `preview/scene.rs::make_session`),
like every other "next song" row. D6 chose **live** instead, which is simpler:

- There is no `Session` field and no builder.
- `director::produce` reads `style::head_scale()` (one relaxed atomic load) once at the top of
  every call.
- Gameplay (`lifecycle.rs`) and both song-select previews (`scene_window.rs:358–362`) all go
  through `produce`, so a mod-menu toggle shows on the next frame everywhere. That satisfies D2
  with no extra code.

Toggling mid-frame is safe because `evaluate_into` rewrites every bone each call, so the scale is
re-applied (or not) from scratch every frame.

### 6.5 Director — `director.rs`

At the top of `produce`, and then inside the dancer block directly after `evaluate_into`
(`:137–145`) and before the body publish (`:147`):

```rust
let head_scale = super::style::head_scale();   // live: once per produce() call
// ...
let Session { scratch, bones, .. } = sess;
evaluate_into(&clip.anm, &clip.bytes, frame, &d.skeleton, &d.seed, scratch, bones);
if head_scale != 1.0 {
    scale_subtree_about_root(bones, &d.head_subtree, head_scale);
}
```

The body publish, the parts and the shadow then read the edited buffer unchanged. The edit is one
12-multiply pass per dancer per frame, with no allocation, no engine call and no lock, so it is
fine on the hot path. There are no `unwrap` or indexing panics: the helper uses `get`/`get_mut`.

### 6.6 Pure helper + tests — `director_math.rs` (already harness-mounted)

```rust
/// Bones of the subtree rooted at `root` in topological order (root first):
/// the stock rigs are parent-first (`parent < index`), so one pass suffices.
pub fn subtree_of(parents: &[i16], root: usize) -> Vec<usize>;

/// Uniform scale `k` about the subtree root's joint (MODEL space) applied to
/// every listed bone: `M' = M · [k·I 0; j(1−k) 1]`. For the root itself the
/// translation is unchanged. No-op on an empty list / out-of-range indices.
pub fn scale_subtree_about_root(bones: &mut [Mat4], subtree: &[usize], k: f32);

pub const BIG_HEAD_SCALE: f32 = 3.0;
```

Host tests, run by `scripts/validate_background_dancers.sh`, which already mounts
`director_math.rs`:

1. The root's translation row is preserved, and rows 0–2 are exactly × k.
2. A bone-local point `p` maps to `j + k·(p·M − j)`.
3. Rest-pose parity with the A3 offline formula: for a vertex blended `w` Head / `1−w` Neck,
   `Σ wᵢ · v·invBindᵢ·boneᵢ'` equals `j + (v − j)(1 + (k−1)w)`.
4. `part_world(false, &scaled_head, &body)` scales a part point by `k` about `j`, and
   `|row 0|` of the result is `k · s_body`. That uniform-scale property is what `view_frame`
   needs.
5. `subtree_of`:
   - a leaf gives `[root]`;
   - a synthetic chain gives all descendants;
   - a malformed parent (`>= i`) is excluded;
   - an out-of-range root gives an empty result.
6. `k = 1.0` is the identity.

### 6.7 Docs

- `README.md`: the `background_dancers` row of the config table (`:242`) gains `big_head`, and
  "All eight are also rows…" becomes nine. Add a feature-table mention (`:189`).
- `src/mods/background_dancers/mod.rs` `//!` Config list.
- `.agents/summary/data_models.md` (config ownership) is generated. The next codebase-summary
  refresh picks up the key, so do not hand-edit it.

### 6.8 Explicitly not needed

| Not needed | Why |
|---|---|
| New signatures or a signature sweep | nothing new is scanned |
| New detours or hooks | the pose is ours; one detour per target is untouched |
| New shader variants or blobs | the stock, lit and cel paths all handle a uniform bone scale |
| `data_mods/` assets or label PNGs | mod-menu rows are plain strings |
| A `score_guard` taint | purely cosmetic, does not affect a song's outcome |
| `multiplayer_bot` phantom-side handling | the row is cabinet-wide and not folded from per-player values |
| A relaunch or a song change | live: the next frame, gameplay and previews alike (§6.4) |

## 7. Decisions (settled 2026-09-27)

The maintainer decided after reviewing a Blender contact sheet of all 26 stock and 5 custom
dancers at 1× and 3×. The sheet used the joint pivot and the rlist scales, rendered with
`tools/blender_ddr_addon` (the rig, parts and scale match the game; the lighting is Blender's).

| # | Decision | Options considered | **Decided** |
|---|---|---|---|
| D1 | **Pivot** | (a) the Head joint, simplest and the A3-confirmed formula; the chin drops 0.13–0.2 m. (b) a chin-anchored pivot (a Head-local point from the bind pose), which keeps the jaw in place and stretches the neck | **(a) Head joint**, as on the contact sheet |
| D2 | **Previews** | apply / apply and widen the preview frustum / exclude | **Apply.** The previews show Big Head too; no frustum change |
| D3 | **Factor exposure** | OFF/ON = 3× / an enum of sizes / a config-only scale | **OFF/ON = 3×**, no size options, no config scale |
| D4 | **Long hair on `Head` and the class-B mascots** | accept / per-dancer cap / exclude | **Accept.** Everything on `Head` scales; clipping through the floor or other geometry is fine |
| D5 | **Row placement** | after the outline rows / after STOP SLOW-MOTION / last | **Last row** of BACKGROUND DANCERS under GLOBAL SETTINGS in the operator (000) mod menu. It is cabinet-wide, with no in-game option row |
| D6 | **Live vs next song** | latched per session / live | **Live** (§6.4) |

## 8. Alternatives considered

| Approach | Verdict |
|---|---|
| **Pose-side matrix edit in the director** (§6) | **Chosen.** One place, pure and host-testable, reaches every consumer, no engine contact |
| Set `scratch[Head].s = 3` inside `core::anm::pose::evaluate_into` | Rejected. `core::anm` is a fixture-pinned port of A3's evaluator shared with stage parts. Maya segment-scale compensation (`pose.rs:192–199`) divides children by the parent's scale, so a future Head subtree would *not* grow. And it mixes a presentation effect into the codec |
| Offline geometry baking (the A3 experiment): repack every body and Head part with Head-weighted vertices × 3 into cached arcs | Rejected. It needs a pack step per stock and custom dancer (disk cache, enable-time cost), separate model keys to toggle it per song, and double ResourceManager residency. It is useful only as proof that the look works |
| VS-side scale keyed on bone index | Rejected. It needs a stock-style VS variant as well, and does not reach the rigid parts' World or the cull volume |
| Detour the bone-texture upload `FUN_180261780` | Rejected. It would be a detour on an engine hot path to change data we already own |

## 9. Validation plan

- **Host:** `./scripts/validate_background_dancers.sh` (the new `director_math` tests of §6.6).
- **Readiness gate:**
  1. `cargo check --target x86_64-pc-windows-msvc`
  2. `cargo fmt` (whole crate)
  3. `./build.sh`

  No signature sweep, since no signatures change.
- **Cabinet** (DLL-only deploy; `data_mods/` unchanged). Logs:
  - At enable: `BackgroundDancers: big head -- on (x3, live)` (or `off`).
  - On a toggle: `BackgroundDancers: BIG HEAD set to ON (live)`.
  - No new WARNs on stock picks.
  - A custom body without a `.b2it` WARNs once (`… -- no parts/shadow/big head`).
- **Visual checklist:**
  1. A solo stock dancer on a stage-camera stage in STOCK, SMOOTH and CEL + outlines (INK and
     LAYERED rims on the head).
  2. A `head00` wearer (afro / bonnie / rinon): the part follows and scales, and its hull too.
  3. Versus (two dancers).
  4. FULLSCREEN and STAGE SCREENS movie modes.
  5. A long-hair dancer (yuni / rinon / custom Miku) and a class-B mascot (babylon00 / pix00).
  6. Both song-select previews, including toggling while a preview is showing.
  7. Head at frame edges during camera cuts: no popping (confirms §3.2).
  8. **Live toggle mid-song**: open the mod menu during a song and flip BIG HEAD. The heads
     change on the next frame, and OFF restores them.
  9. The row is the last one under BACKGROUND DANCERS, and its value survives a relaunch and an
     edit of any other Background Dancers row (whole-section rewrite).

## 10. Open items / risks

1. **Chin and neck look** at 3× under the lit and cel lights. This needs eyes on a cabinet.
2. **Preview-box crop** for male and tall heads. Accepted (D2); revisit only if it reads badly.
3. **FULLSCREEN medium shots** crop most big heads. If that matters, generate a Big Head variant
   of the movie camera set: in `gen_movie_cameras.py`, raise `POINTS`' Head extent from 0.24 m to
   about 0.72 m and re-run the framing check. Out of scope.
4. The culling verification (§3.2) was done on World 20260825, the build the scene3d notes use.
   The collector is the same one the shipped feature already relies on for every build it
   supports, and Big Head adds no new engine dependency.
5. **Custom-content weights (resolved 2026-09-27).** Big Head amplifies any Head-vs-Neck weight
   mismatch. Big Smoke's heat-weighted port had partial Head weights across the whole lower face
   (w ≈ 0.4–0.8), while his eye patches and teeth sat at 1.00. At 3× the eye whites and teeth
   poked through the skin. The fix is in the port, not the DLL: the face is now rigid on Head, with
   the Neck blend confined under the chin, like the stock rigs (`tools/blender_ddr_addon/README.md`,
   "Keep the head rigid"). The other custom dancers have no partial weights from the lips up at the
   front of the face.

## Appendix — survey method

The table in §5 was produced read-only, with nothing written to the repo or the install:

1. `scripts/ktmdl_dump.py` (`parse_model`, `read_vertices`, `parse_b2it`, `parse_rlist`) and
   `scripts/unpack_arc.py` (`ARC`) read, from `$DDR_WORLD_INSTALL/data/arc/`:
   - each `pl_<key>.arc` (body `.model` + `.b2it`),
   - each `pl_<key>_head00.arc`,
   - `startup.arc`'s `data/chara/chara_resources.rlist`, for the model scales.

   The custom dancers were read from `data_mods/custom_models/dancers/*/pl_*/`.
2. For every vertex, the Head weight is `w = Σ weightsᵢ` over the blend indices whose palette
   entry is the `.b2it` `Head` index. The rest-pose 3× position is `j + (v − j)(1 + 2w)`, with
   `j` the Head bind translation.
3. Head parts are placed as `v · Bind[Head]` and scaled 3× about `j`.
4. The Head-children check is `parent == Head` over each bone table.
