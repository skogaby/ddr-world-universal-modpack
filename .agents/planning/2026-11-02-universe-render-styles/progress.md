# Progress — DDR Universe render styles as Lighting Style options

Updated: 2026-11-02
Status: Phase 2b (4 follow-ups) — code + host gates done; awaiting deploy #2. Uncommitted
(maintainer commits manually).
NEXT ACTION: deploy #2 WITH `data_mods/` (27 `mdl_*uni*` blobs incl. the new `mdl_uni_inv*` and
the changed `mdl_uni_ste*`, + `bdfxuniverse.arc`), then the deploy #2 watch-list below.
Resume protocol: this file → the RE note (`docs/ddr_universe_render_styles_research.md`) →
`shaders/src/mdl_universe.hlsl` header → `shader_layout.rs` (`SceneStyle` / `StyleFamily` docs).

## Decisions (2026-11-02, maintainer)
- Port BOTH groups: the shader-only styles (A) and the ones that need the DLL to bind a style
  texture and/or draw additively (B).
- Dancers only: the Universe styles restyle dancers (bodies, parts, Winx twins); stage props fall
  back to CEL in those styles.
- One menu row: LIGHTING STYLE grows from 3 to 17 values (STOCK / SMOOTH / CEL + 14 Universe).

## Style set (Universe id → World style)
| World | Universe (CSV names) | Look | Needs |
|---|---|---|---|
| CONTRAST | 10 CONTRAST | white where N·L > 0.5 else black | — |
| HIGHLIGHT | 11 HIGHLIGHT | white lit side only, rest discarded | — |
| SHADOW | 12 SHADOW | white unlit side only, rest discarded | — |
| NEGATIVE | 19 NEGATIVE / BLACKGLOSS | 1 − toon-shaded colour, + outline | — |
| MONOCHROME | 20 MONOCHROME | luma of toon-shaded, + outline | — |
| SEPIA | 21 SEPIA | YCbCr sepia of toon-shaded, + outline | — |
| STENCIL | 8 STENCIL / FILL IN | flat white fill (most common params −16 = white) | — |
| STENCIL RAINBOW | 9 STENCIL RAINBOW | screen-space `rainbow.tga` fill | style texture |
| THERMO | 13 THERMO | light-space sphere map `sphere_thermograph` | style texture |
| MERCURY | 15 MERCURY / T1000 | light-space sphere map `sphmap2` (chrome photo) | style texture |
| CANDY | 14 CANDY | additive thermograph sphere map | texture + additive |
| BUBBLE | 16 BUBBLE / STONE | additive `sphmap2` sphere map | texture + additive |
| CRYSTAL | 17 CRYSTAL | additive `crystal` ring sphere map | texture + additive |
| XRAY | 18 XRAY / ALPHA LIGHT | additive `xray1` sphere map | texture + additive |
Not ported: 1 NORMAL / 2 TOON (≈ the existing CEL), 4 OUTLINE (needs colour-write-off body
pass), 6 SILLHOUETTE / FILL OUT and 7 SILLHOUETTE RAINBOW (full-screen stencil fill OUTSIDE the
dancer).

## Plan
1. RE note `docs/ddr_universe_render_styles_research.md`.
2. `shaders/src/mdl_universe.hlsl` (one VS per shape, PS families) + `build_shaders.sh` manifest +
   blobs.
3. Style textures: `scripts/gen_universe_style_textures.py` → `data_mods/background_dancers/arc/
   bdfxuniverse.arc` (5 DDS from the rip).
4. `shader_layout.rs` (`SceneStyle` + families + container names, tests) + `shader_synthesis.rs`
   (family containers, fingerprint v9).
5. `style.rs` rows / effective / outline rule; stage + wipe fallbacks.
6. `session.rs` / `render_item.rs` / `scene_window.rs`: per-instance style, style-texture injection
   into material slot 1 (+ per-frame retry), additive record flags, fx arc load.
7. Gates: cargo check, fmt, validate_overlay_draw.sh, validate_background_dancers.sh, build.sh;
   docs.

## Phase 2b (2026-11-02, maintainer feedback after the first test)
Requests: (1) the MODS-tab dancer preview rebuilds on every LIGHTING STYLE change (it reads the
style only at start); (2) STENCIL cycles Universe palette colours (white-only read as a bug —
faithful, but Universe changes it per bar); (3) port OUTLINE (Universe's "pencil drawing": body
invisible but writing depth, then the black hull); (4) new first row value RANDOM (EVERY BAR):
the dancers' style re-picked every chart bar from every other preset.
World RE for (3) (`gamemdx_20260825`, `FUN_1801f2c30` / `FUN_180261430` / `FUN_180261c80`):
pass order OPACITY 0x66 → LOWPRIO_TRANS 0x67 → TRANS 0x68; OPACITY sorts front-to-back
(`FUN_180262310`), TRANS back-to-front (`FUN_1802624c0`), LOWPRIO does NOT sort (node sort-key
order); record bit 0 ⇒ the TRANS-type collector (`FUN_180262f00`, also LOWPRIO), else OPACITY
(`FUN_1802629d0`); blend group from the record flags in every pass (0x20 alpha, 0x40 additive,
0x60 subtract, 0x80 = no blend command). ⇒ gameplay OUTLINE body: additive + rgb 0 + z-write ON,
bit 0, node mask 0x10 (LOWPRIO only), high sort key (after the `:N` stage parts); hull records
bit 0 (TRANS, after it). Preview (no LOWPRIO clone, no stage): body bit 0 CLEAR (OPACITY clone).
Plan: per-instance runtime style switching (captured at build: container per family per
restyled material, restyled records' base flags, hull flags) applied by `Session::style_tick(bar)`
every frame — fixed styles apply once, RANDOM on every bar change; STENCIL's colour rides the
item tint (PS outputs COLOR0). Pure parts in `universe_logic.rs` (palette, stencil cycle,
random pick) — harness-mounted.

## Done
- Phase 2b (2026-11-02): `SceneStyle::Random` (row 0) + `Outline`; `UniFamily::Invisible`
  (`_uinv`, `mdl_uni_inv[_notex].ps`); STENCIL PS outputs COLOR0 (tint); `universe_logic.rs`
  (palette, STENCIL cycle, no-repeat per-bar pick, bars) and `style_rig.rs` (runtime look capture)
  — both harness-mounted; `render_item.rs` `restyle_candidates` + raw setters (shader object,
  record flags), `restyle_materials` / `make_records_additive` removed; `render_item_layout.rs`
  `invisible_depth_record_flags` / `trans_record_flags`; `node.rs` `set_item_pass_mask` /
  `set_sort_key`; `style.rs` `Effective { style, outlines, served }`, `gate_style`,
  `hull_plan(eff, dancer_preview)`, `generation()`; `HullPlan.stage`, `plan_instances(.., hull_stage,
  hull_dancers, ..)`; `Session::style_tick` + `look_for` / `apply_look` (session.rs), driven from
  `SceneWindow::publish` with `style_bar` set by lifecycle (tempo-map measures) / preview (2-s
  bars); director multiplies `dancer_tint`; arc loaded with every dancer pick. Gates: cargo check
  (+ tests), fmt, validate_background_dancers.sh (298), validate_overlay_draw.sh (29), style.rs
  gate tests in a throwaway harness (3), `./build.sh`.
- Step 1: RE note `docs/ddr_universe_render_styles_research.md` (§4.1 = how the DLL applies them).
- Step 2: `shaders/src/mdl_universe.hlsl` + 25 manifest lines in `scripts/build_shaders.sh` → 25
  new blobs; every existing blob byte-identical.
- Step 3: `scripts/gen_universe_style_textures.py` → `bdfxuniverse.arc` (5 DDS, 367 KB).
- Step 4: `shader_layout.rs` (`SceneStyle` ×17, `StyleFamily` / `UniFamily` / `StyleTexture`,
  `uni_vs` / `uni_ps`, `family_container_name`, `is_universe_blob`, `StyleFamily::served`);
  `shader_synthesis.rs` (`Plan.universe` soft-degrade, `universe_available()`, 9 shapes × 11
  families = 99 model containers, fingerprint "v9").
- Step 5: `style.rs` — 17-value LIGHTING STYLE row from `SceneStyle::ALL`; `effective_for(req,
  Served)` (Universe → CEL when the containers or the fx arc are missing); `Effective.dancer_hulls`;
  `hull_plan` → `HullPlan::ink_stage_only()` for Universe styles without an outline.
- Step 6: `outline.rs` `HullPlan.dancers`; `instance_plan.rs` `plan_instances(.., hull_dancers, ..)`
  + `Instance.style_tex_pending / style_tex_mats`; `render_item_layout.rs`
  `additive_record_flags`, slot helpers, `MAT_TEX_INDEX_NONE`, `STYLE_TEXTURE_SLOT`;
  `render_item.rs` `RestyleStats.material_restyled`, `make_records_additive`,
  `bind_style_texture`, `registered_texture`; `session.rs` `instance_style` (stage → CEL), the
  build gate waits for the style texture, `universe_extras` after the restyle; `scene_window.rs`
  loads the fx arc first + per-frame bind retry.
- Step 7: cargo check (+ `--tests`), cargo fmt, validate_overlay_draw.sh (29),
  validate_background_dancers.sh (290), style.rs gate tests in a throwaway harness (2), `./build.sh`
  (no warnings), path grep clean. README (LIGHTING STYLE paragraph + config table) updated.
- RE (Ghidra, `default_ddr_universe.xex`): style parser `0x820FEC58`, dispatcher `0x8209B818`,
  draw routines, kind→VS `0x8224CFF0`, kind→PS `0x82242F90`, PS setter `0x8224C9E0`, shader
  sources (HLSL text in `.data` `0x82507930..0x8251c188`, dumped outside the repo to
  `~/Desktop/ddr_universe/xex_shaders/`), stencil fill `0x8224E708`, palette `0x82180978`.
  Ghidra project edits: no-return flags cleared on the `__savegprlr` chain
  (`0x82487250..0x82487288`) and per-call `CALL_RETURN` overrides cleared in the functions above.

## In flight
- (none)

## Deploy & test log
- Deploy #1 (2026-11-02, maintainer, local CrossOver): every Universe style drew in gameplay;
  `universe styles=true`; slot-1 textures bound (`style texture 'bdfxunithermo' bound to 8
  material(s)`), additive flags `0x00 → 0x4B`. Feedback: STENCIL "pure white" read as a bug
  (faithful to its most common setting); the MODS-tab dancer preview did not follow LIGHTING STYLE
  (it read the style only at start). → Phase 2b.
- Deploy #2 watch-list (Phase 2b):
  - Preview (song select → options → MODS → BACKGROUND DANCER row): every LIGHTING STYLE edit
    changes the live dancer at once (visible after the overlay menu closes); RANDOM changes every
    2 s; CEL previews now show the ink outline when SCENE OUTLINES is on (dancer hulls in previews).
  - Gameplay: `dancers' look -> <style> (bar N, RANDOM (EVERY BAR))` lines; RANDOM changes on bars
    and never repeats a style back to back; STENCIL changes colour each bar.
  - OUTLINE ONLY: only black ink lines, stage visible through the dancer (gameplay: body in
    LOWPRIO_TRANS, hull in TRANS — if the dancer shows as a solid black shape the pass order is
    wrong; if nothing shows, the hull is hidden or the body hides it); preview: the same on the
    dark backdrop.
  - A mid-song LIGHTING STYLE edit applies at once (no hitch).
- Deploy #1 watch-list (kept for reference):
  - Boot log: `shader_synthesis: synthesizing (... universe styles=true ...)` and 99 model-variant
    container lines (was 18); boot time vs before (shader.arc gains 81 containers).
  - `BackgroundDancers: scene style -- … DDR Universe styles served`.
  - Per dancer item: `style <key> -- materials restyled=N`; texture styles: `style texture
    'bdfxuni…' bound to N material(s) (slot 1)` (or the retry line); additive styles: `N record(s)
    additive (flags 0x… -> 0x…)`.
  - Visual: sphere styles actually sample s1 (slot 1 → sampler 1 is an assumption); additive
    dancers land in the TRANS pass (flag bit 0 assumption) and glow; STENCIL RAINBOW's band is
    screen-fixed; NEGATIVE / MONO / SEPIA keep their hull; stage stays CEL with outlines.
  - The 87/113-bone UNIVERSE dancers (phase 1) in a Universe style.

## Deviations & open questions
- Light: Universe's c16 / c23 light direction is not set in the dancer module (not traced); World
  uses CEL's `TOON_LIGHT_DIR` for N·L and the sphere-map light basis.
- Light colour c17 (per-song LIGHT COLOR) = white.
- Outline for NEGATIVE / MONOCHROME / SEPIA uses World's existing DSU hull (Universe's c18 =
  {0.005, 0.5, 0, 0.001} is thinner); the SCENE OUTLINES row still gates it.
- Material texture alpha is kept for cut-outs in every style (Universe replaces stage 0 with the
  sphere map and drops the body alpha; World's stock hair cards would turn into solid quads).
- Additive record flags use the A3 loader mask (0x01 transparent, 0x02 two-sided, 0x08 z-write
  off, 0x40 additive) — verify on the cabinet (INFO log of before/after flags).
- Stage props keep CEL's outlines in every Universe style (`HullPlan::ink_stage_only`); only
  dancer hulls are dropped for the styles Universe draws without an outline.
- A transforming Winx dancer draws through the CEL wipe containers (no Universe effect).
- Only blend-group-0 materials restyle, so alpha-blended hair / veils keep the stock look.
- A missing `mdl_uni_*` blob degrades only the Universe styles (→ CEL), never lit/cel.

## Key facts for a cold resume
- World model passes (RE 2b): OPACITY 0x66 → LOWPRIO_TRANS 0x67 → TRANS 0x68; record bit 0 ⇒
  TRANS-type collector; LOWPRIO unsorted (node sort-key order); blend group applies in every pass.
- Look switching runs on the game thread mid-frame (`SceneWindow::publish` → `style_tick`), the
  same contract as the texture retry; hull node-hidden is owned by `style_tick` once published.
- Container suffixes: `_u<tag>`, tags `con hil sha neg mon sep ste scr sph`; all six sphere styles
  share `_usph`, STENCIL RAINBOW is `_uscr`. Program layout = lit/cel (outline pair at 0).
- Style texture registry key = `pure::fnv1_name_hash(stem)` (stems lower-case, no `_`).
- Universe draw kinds (`.ddm` material flag word / `mat+0x5c`): 0xa0000001 gouraud-unlit,
  0xa0000002 toon, 0xa0000003 outline, 0xa0000005 sphere-mapped, 0xa0000010/11/12 contrast /
  highlight / shadow, 0xa0000019/1a/1b negative / mono / sepia.
- Ramp/sphere textures (x_data root): `toon`, `rainbow`, `specular`, `crystal`, `sphmap2`,
  `xray1`, `xray2`, `sphere_thermograph` (`.tga`).
