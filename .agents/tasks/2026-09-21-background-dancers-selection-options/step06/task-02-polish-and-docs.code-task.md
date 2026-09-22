# Task: Polish + documentation — WARN audit, research §5, AGENTS.md row, README, validation matrix hand-off

## Description
Ship-quality wrap-up (plan Step 6 items 2–4): audit every preview-path WARN for one-shot latching, record
the new engine facts in `docs/background_dancers_research.md` as a "§5 Viewport passes / RENDER_2D
compositing" section, extend the AGENTS.md Background Dancers row with the options + preview summary,
add the two options to the README's Background Dancers text, and write the cabinet validation matrix
(design §7 items 1–6, both platforms, 1280×720 / 1920×1080 / 640×480) into `progress.md` for the
maintainer to run. No behaviour change beyond WARN wording/latching.

## Background
The feature spans: `custom_options::ScalarFormat::Dynamic` (framework), `background_dancers/{catalog,
options, pick, instance_plan, scene_window, preview/*}.rs`, `scene3d/{camera_math, viewport_pass,
viewport_pass_layout}.rs`, `signatures.rs::scene3d_resolve_viewport` (nine AOBs, optional sub-group), the
dev knob `DDR_DANCERS_VIEWPORT_SMOKE`. Engine facts to document (research `preview-compositing.md`): the
target lists + dispatch (`display+0x38` RENDER_2D, attach = push+sort, detach = erase, the per-viewport
SetViewport + conditional camera upload, `0x4003a` terminator), the MODEL pass object layout (0xF8; rect
+0x38, flags +0x54 bit0 DISABLED / bit1 skip camera, proj +0x58, view +0x98, self +0xE0, items +0xE8,
filter +0x2C), the ClearViewport (0x40; gd Clear record `{0x00140000, flags, ARGB, z, stencil}` at
`*(workerCtx+0x218)`), the free node-mask bits (`0x08`/`0x20`/`0x80`), the camera math (LookAtRH row
vectors; D3D off-centre projection), pass-5 cull does not drop items, the tag-0x10 correction (SetTexture,
not a model draw), the cabinet-proven ARGB colour (the azure smoke). Repo rules: `docs/` addresses are
file-relative to `0x180000000` (20260825 unless stated), never a local username / absolute path in
tracked files (`git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no hits).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md`
  (§4.5, §4.6, §5.2, §6, §7, Appendix A)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/research/preview-compositing.md` (the §5 source)
- `.agents/planning/2026-09-21-background-dancers-selection-options/progress.md` (deploy log + deviations)
- `docs/background_dancers_research.md` (§1–§4 style), `AGENTS.md` "Background Dancers" row, `README.md`
  "Background Dancers" paragraph(s)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. WARN audit (`src/mods/background_dancers/preview/*.rs`, `scene_window.rs` preview paths,
   `viewport_pass.rs`): every WARN reachable per frame or per request is behind a one-shot latch
   (`Warn` bits / `AtomicBool`); per-identity WARNs (unknown key, > 16 parts) fire at most once per
   identity per boot (a small `Mutex<Vec<String>>` of seen keys is acceptable); `viewport_pass::create`
   refusals are already one-per-call and reached at most once per side per boot through the "attempted"
   rule — verify. Wording: every preview line starts `BackgroundDancers: preview P{n}` (the driver) or the
   window's tag.
2. `docs/background_dancers_research.md`: append "## 5. Viewport passes / RENDER_2D compositing (2026-09-21)"
   with subsections 5.1 target lists + dispatch, 5.2 MODEL pass object + clone recipe, 5.3 ClearViewport +
   gd Clear record (+ the ARGB finding), 5.4 camera matrices, 5.5 what the graph does NOT do (pass-5 cull,
   camera slot 0 uninvolved), 5.6 the tag-0x10 correction, 5.7 signatures (`scene3d_vp_*`, the nine AOBs
   and their cross-checks), 5.8 the preview scene shape (slot bases, private bits, priorities, crop rule).
3. AGENTS.md Background Dancers row (Key Entry Points): append the options + preview summary — row ids
   `background_dancer` / `background_stage` (`custom_options` scalar rows via the NEW `ScalarFormat::Dynamic(fn)`,
   labels from `catalog.rs`, `PersistMode::Local`, `.in_game_only()`, stage row `versus_mirror`ed, pick
   order pin → option → random), the compositor one-liner (`scene3d::viewport_pass`: byte-cloned MODEL
   passes + a mod-owned ClearViewport attached into RENDER_2D at 0x68–0x6A / 0x6B–0x6D with the box as
   their D3D viewport and their own view/proj; items stamped 0x08 / 0x20; frame-board slot bases 0 / 16;
   `reap()` ONCE per frame in `mod.rs`), the shared `scene_window::SceneWindow` lifecycle, the crop rule
   (170×150 box, §4.7 amendment), the fail-open rule (no derivation ⇒ rows work, badge only), the two dev
   knobs `DDR_DANCERS_PIN` / `DDR_DANCERS_VIEWPORT_SMOKE`, and the D3DCOLOR ARGB note.
4. README: in the Background Dancers section add two sentences on BACKGROUND DANCER / BACKGROUND STAGE
   (where they live, RANDOM, the live preview in the box, cabinet-wide stage in 2P, local-only persistence).
5. `progress.md`: a "Validation matrix (design §7)" table — items 1–6 × {Windows, CrossOver} × {1280×720,
   1920×1080, 640×480} where applicable — with expected log lines per item, left for the maintainer to
   fill in; tick Step 6 in `implementation/plan.md` when the code + docs are done (matrix pending).
6. `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no hits; `cargo check` clean; `cargo fmt`.

## Dependencies
- task-01 of this step (the badge), Steps 1–5.

## Implementation Approach
1. Audit + fix latches; 2. docs (research → AGENTS → README); 3. progress matrix + plan tick; 4. gates.

## Acceptance Criteria

1. **One-shot WARNs**
   - Given a preview request that fails every frame (compositor unavailable)
   - When 600 frames pass
   - Then exactly one WARN is logged for that class.

2. **Docs present**
   - Given `docs/background_dancers_research.md`, `AGENTS.md`, `README.md`
   - When read
   - Then §5 exists with the eight subsections, the AGENTS.md row names both option ids + the compositor +
     the two knobs, and the README names both options.

3. **Repo hygiene**
   - Given the working tree
   - When `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` runs
   - Then no new hits versus the pre-feature baseline.

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, docs, polish
- **Required Skills**: Rust, technical writing in the repo's house style
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 6: RANDOM badge, polish, documentation, validation matrix
