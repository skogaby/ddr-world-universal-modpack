# Task: Pure strip-synthesis layer (extraction, layout, rasterizer, PNG)

## Description
The host-testable half of the Step-6 chart-strip timeline (design R7 as
amended 2026-08-14): a pure module that turns (note vector, arrow sheet,
palette, layout params) into the finished strip image bytes — ARC/DDS
sheet extraction, content-time→pixel layout math, the glyph rasterizer,
and PNG encoding. Zero engine calls; everything injected; fully
host-tested in the harness.

## Background
The strip is pre-rendered ONCE per song (performance is the binding
constraint — per-frame cost must be constant w.r.t. chart density) and
must be noteskin-accurate: the player's chosen `2d_arrowNN` sheet plus
the game's real quantization coloring. This task builds the pure layers;
task-02 wires them to live game state (chosen design, live palette
evaluators, the FileManager texture pipeline).

Key format facts (verified 2026-08-14, research §3):
- `data/arc/2d/2d_arrowNN.arc` (N=00..07): ARC v1, one entry
  `data/2d/arrowNN/arrowNN.dds`, AVSLZ-compressed.
- The DDS payload is **uncompressed A8R8G8B8, 768×192** (128-byte
  header; masks R=0x00ff0000 G=0x0000ff00 B=0x000000ff A=0xff000000).
  The RGB channels carry PALETTE INDICES (red = palette U), not colors.
- Cell layout (research §3): 96×96 cells — tap `[0..96]×[0..96]`,
  freeze head `[96..192]×[0..96]`, freeze bottom-cap art rows
  `[96..192]`, freeze body columns at `x = col·96 + 384` (col =
  `{0,0,1,2,2,1,3,3}[dir·2+reverse] & 3`), body art tiles vertically.
  One direction baked; rotation is per-sprite (the rasterizer rotates).
- Color resolution: `rgba = palette[row][atlas.red]` — the palette is
  injected as a `[[u32; 256]; 32]`-shaped table (task-02 fills it from
  the live generators); the note's palette row is injected per note
  (task-02 supplies the selector). Alpha composes `atlas.a · palette.a`.

Note-vector input: the pure layer consumes a caller-provided slice of
decoded notes (timestamp ms, beat count, kind, per-panel state, per-panel
freeze length — mirror `seek::NoteView`/`GameNote` fields as a small
input struct so the harness needs no engine types).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (R7 as amended 2026-08-14, §4.1)

**Additional References (if relevant to this task):**
- docs/chart_strip_hud_research.md (§2 pipeline, §3 sheets/cells, §4 palette model, §5 risk 4 legibility)
- src/core/arc.rs (`parse`/`extract`), src/services/avs_layeredfs/avslz.rs (`decompress`)
- src/mods/webui_options/preview_gen.rs (`image`-crate usage precedent)
- src/mods/training_mode/section_math.rs (pure-module + inline-test house style; harness mount precedent)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New pure module under `src/mods/training_mode/` (e.g. `strip_synth.rs`)
   mounted in the host harness (one `#[path]` line beside section_math's).
   No engine imports; `image` crate allowed (already a dependency).
2. Sheet extraction: `arc bytes → RgbaImage` (ARC parse → AVSLZ decompress
   → DDS header validation (dims/masks; reject anything but 768×192
   A8R8G8B8 with one error) → RGBA). Pure function over byte slices.
3. Layout math: strip geometry (width, height, per-column x, glyph size),
   `content_ms → y` (linear over 0..chart_end, top = start), doubles
   (8 columns) vs single (4), `m:ss` formatting, and marker-fraction
   helpers (reused by task-03's cursor math). Degenerate chart_end ⇒ None.
4. Rasterizer: for each note, per set panel: select cell (tap/shock/mine
   by kind; freeze head + body bar + cap when `length[panel] > 0` —
   body as a vertical bar of body-column art scaled to the ms span),
   rotate per direction, downscale to glyph size (box filter), resolve
   color through the injected palette + per-note row, alpha-blend into
   the strip. Overlaps draw in time order (later on top). Shock = the
   full-width row treatment; mines only when the chart carries them.
5. PNG encode via `image` (RGBA8) returning bytes (caller writes the
   file in task-02).
6. Host tests: DDS extraction against a synthetic fixture (constructed
   header + payload — do NOT commit game assets); layout math edge cases
   (0/EOF clamp, doubles columns, m:ss rounding); rasterizer against
   synthetic notes + a tiny synthetic sheet + fixed palette (assert
   specific pixels: position, rotation, freeze span, palette lookup,
   alpha composition). Follow section_math's inline-test style.

## Dependencies
- None new (image crate already in Cargo.toml). Steps 1–5 shipped.

## Implementation Approach
1. TDD in the harness: extraction fixture tests → layout tests →
   rasterizer pixel tests.
2. Implement extraction, layout, rasterizer, encode as separate pure fns.
3. Gates (harness → check → fmt).

## Acceptance Criteria

1. **Sheet extraction round-trip**
   - Given a synthetic ARC containing an AVSLZ-compressed 768×192
     A8R8G8B8 DDS
   - When the extraction fn runs
   - Then the RGBA image matches the synthetic pixels exactly, and a
     wrong-format DDS returns an error (no panic)
2. **Layout math**
   - Given chart_end = 120000 ms and a 600 px strip
   - When mapping 0 / 60000 / 120000 ms
   - Then y = 0 / 300 / 600 (clamped), m:ss renders "0:00"/"1:00"/"2:00",
     and chart_end ≤ 0 yields None
3. **Rasterizer fidelity**
   - Given synthetic notes (tap on panel 0, a 2-beat freeze on panel 2,
     a jump) with known palette rows and a synthetic sheet
   - When the strip renders
   - Then glyphs appear at the mapped positions/columns with the
     palette-resolved colors, the freeze body spans its ms range, and
     the jump renders both panels
4. **Zero engine coupling**
   - Given the host harness
   - When the full suite runs
   - Then the new module compiles and passes on aarch64 (pure Rust only)

## Metadata
- **Complexity**: Medium
- **Labels**: training-mode, hud, pure-layer, host-tested
- **Required Skills**: Rust, image processing, the repo's pure-module conventions
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 6: Chart-strip timeline HUD + placement row
