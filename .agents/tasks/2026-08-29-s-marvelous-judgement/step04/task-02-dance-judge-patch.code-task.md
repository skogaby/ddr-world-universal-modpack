# Task: dance_judge asset synthesis and AFP patch

## Description
The client-side synthesis chain for the gameplay flash: inject the
S-Marvelous word texture (donor-anchored atlas clone), synthesize the cloned
geo (region-name rewrite), register the `afp_patcher` patch that gives the
`dance_judge` template its `in_smarvelous` segment, and prove the whole
chain offline by rendering the patched template with the new texture.

## Background
Full structural map in
.agents/planning/2026-08-29-s-marvelous-judgement/research/display-side-re.md §10.
The chain, per that map (all local, no import surgery):

1. **Texture**: donor-anchored atlas clone of region
   `dance_judge0000_marvelous` → new region (choose the name; suffix
   convention `dance_judge0000_smarvelous`) at the donor's exact pixel rect,
   art from `data_mods/s_marvelous/dance_judge/smarvelous.png` (344×61
   placeholder, maintainer will re-drop real art).
2. **Geo**: byte-clone of `dance_judge_shape32` with its region-name label
   rewritten to the new region; serve under the name the new shape id
   demands (`dance_judge_shape{N}`) via the AFP geo mapping registration.
   ⚠ The rewritten name is LONGER than the donor's — verify the shipped
   GE2D label rewriter handles length-changing rebuilds; extend it if not
   (promote to a shared helper while touching it — it currently lives in
   folder_expansion).
3. **AP2 patch** (`afp_patcher::register_patch("dance_judge", fn)`, fn gets
   descrambled bytes): `add_shape` (id N, geo binding by name) →
   `clone_sprite_definition(35-analog, remap {32-analog → N})` → placements-
   only `clone_labeled_segment("in_marvelous" → "in_smarvelous", remap
   {word-sprite → cloned sprite})`. Resolve ALL ids DYNAMICALLY (walk the
   segment: word placement = the PlaceObject whose source sprite internally
   places a shape whose geo references the `*_marvelous` region) — never
   hardcode 32/35 (skin packages and future builds differ). Unknown
   structure ⇒ return None (stock bytes stream), one WARN.
4. **Confidence flag**: expose `patch_ready()` (patch registered + assets
   staged) and `patch_applied()` (the fn actually produced output this
   session) for task-03's re-drive gating.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-29-s-marvelous-judgement/design/detailed-design.md (§4.2, §6)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-29-s-marvelous-judgement/research/display-side-re.md §10 (the structural map — read FIRST)
- .agents/planning/2026-08-29-s-marvelous-judgement/research/afp-tooling.md §3 (shipped pipeline: atlas_cloner donor mode, write_merged_texturelist, load_stock_texturelist, register_afp_geo_mapping, afp_patcher contract)
- src/mods/music_wheel_song_length.rs (generate_glyph_atlas — the atlas-injection recipe to mirror; note the first-boot rescan + reboot-once rule)
- src/mods/folder_expansion.rs (patch_ge2d_labels + donor-clone precedent)
- src/services/afp_patcher.rs (register_patch semantics, buffer lifetime)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New mod files: `src/mods/s_marvelous/afp_patches.rs` (patch fns, pure
   AP2 logic separated from IO so the doc-transform core is host-testable)
   and `src/mods/s_marvelous/assets.rs` (atlas injection + geo synthesis +
   mapping registration, run at enable; cache-guarded like the shipped
   recipes). Enable-time wiring in `mod.rs`; disable ⇒ patch fn returns None.
2. GE2D label rewrite: promote to a shared location (e.g. `core/geo.rs` if
   it can be pure, else a shared service fn) and support length-changing
   label rebuilds; folder_expansion updated to call the promoted fn
   (behavior-identical for its equal-length case — do not regress it).
3. Skin scope (v1): default skin 0000. The patch fn reads the actual
   region/texture names from the template+geo it receives; when the IFS
   isn't the one we injected into (unknown skin), return None with one WARN.
4. Host tests: the doc-transform core against a synthetic template shaped
   like §10 (definitions at frame 0, word chain sprite→shape→geo-name);
   assert the output has the new shape id, cloned sprite, placements-only
   segment, in_smarvelous label. GE2D rewriter: length-change round-trip
   tests.
5. Dev-leg proof (extend Leg C or add Leg D in
   `scripts/validate_s_marvelous.sh`): run the REAL patch fn (via ap2check
   linking the mod's pure transform, or by invoking the same core steps) on
   the real dance_judge, then render `in_smarvelous` with the renderer's
   texture dict carrying `data_mods/s_marvelous/dance_judge/smarvelous.png`
   under the new region name → the preview GIF must show the VIOLET word.
6. Gates: validate script fully green; `cargo check` msvc clean;
   `cargo fmt`; `./build.sh` clean.

## Dependencies
- step04 task-01 (definition-aware cloning primitives).

## Implementation Approach
1. Pure transform + host tests first; then the GE2D rewriter work; then
   enable-time asset wiring; then the dev-leg render proof.
2. Keep every name (region, geo, label) in one constants block with the §10
   derivation documented.

## Acceptance Criteria

1. **Pure transform**
   - Given a synthetic §10-shaped template
   - When the patch transform runs
   - Then the output re-parses with in_smarvelous, a new shape, a cloned
     word sprite, no duplicated definitions — and unknown shapes ⇒ None

2. **Offline render proof**
   - Given the real dance_judge + the placeholder art on the dev machine
   - When the extended dev leg runs
   - Then the preview GIF shows the deep-violet word playing under
     in_smarvelous

3. **Fail-open**
   - Given missing assets, unknown skin, or a transform failure
   - When the game would load dance_judge
   - Then stock bytes stream, exactly one WARN names the reason, and
     patch_applied() reports false

## Metadata
- **Complexity**: High
- **Labels**: s-marvelous, afp-patch, atlas-injection, geo, dev-validation
- **Required Skills**: Rust, repo LayeredFS/atlas pipeline, core/ap2
- **Generated By**: code-task-generator 2026-08-29
- **Source Plan**: .agents/planning/2026-08-29-s-marvelous-judgement/implementation/plan.md
- **Plan Step**: Step 4
