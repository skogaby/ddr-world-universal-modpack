# Progress: pure strip-synthesis layer (task-01)

Status: Complete (uncommitted — the maintainer handles all git; working
tree holds `src/mods/training_mode/strip_synth.rs` (new) + the
`pub mod strip_synth;` registration in `src/mods/training_mode/mod.rs`)

Maintainer visual review: **round 4 APPROVED 2026-08-14** ("the preview
renders look correct to me, now").

## Checklist

- [x] Setup: working dir, approval chain verified, maintainer directives recorded
- [x] Explore: formats verified against real assets (host probe), patterns read
- [x] Plan: plan.md (Status: Approved — upstream approval verified)
- [x] Fixtures + extraction (tests → impl)
- [x] Layout math (tests → impl, failure-first)
- [x] Rasterizer (tests → impl, failure-first)
- [x] PNG encode + env-gated real-asset preview test
- [x] Harness wiring (image dep, arc/avslz/log_warn mounts, strip_synth mount)
- [x] Gates: harness 292/292 → cargo check clean → cargo fmt clean
- [x] Real-sheet preview renders → maintainer review rounds 1–4
- [x] Maintainer visual review PASS (round 4)
- [x] Close record

## TDD cycles

1. **Extraction** — fixtures (synthetic DDS builder, hand-rolled ARC v1 with
   real `avslz::compress`) + `extract_sheet`/`decode_dds` landed together
   (single-layer cycle; the exact-pixel + exact-error-variant assertions are
   the strength here). 3 tests green.
2. **Layout** — tests first, 4 failed on stubs for the expected reasons
   (`layout_rejects_degenerate_params` can't distinguish the None stub —
   noted), then `StripLayout` + `format_mss` implemented. 8 green.
3. **Rasterizer + PNG** — full test set first (11 failing on stubs), then
   pairing/resolve/rotate/downscale/blend/render + `encode_png`. 19 green.
4. **Preview test** — env-gated `render_preview_from_real_sheets`
   ($DDR_WORLD_INSTALL directive); writes `<temp>/ddr_strip_preview/
   strip_arrow0{0..7}.png`. 20 green.
5. **Refinement** — shocks/mines now rotate per panel/column (unrotated
   shock cells read as a row of left taps in the real-art preview);
   suite stays green.
6. **Maintainer review round 1 (2026-08-14)** — renders "correct, for the
   most part"; shocks/mines flagged: they read as an ordinary 4-arrow
   jump, not silver-with-lightning avoid-me arrows. Root cause: I ran
   the shock cells through the palette like tap cells, but the game's
   shock pass binds the DEFAULT shader (research §2.2) — the shock
   cells are TRUE-COLOR art (silver + baked glow). Fix: `shock_glyph`
   true-color path (crop, no palette; direction variants x=192 = left
   art / x=288 = down art, 180°-flip for the opposite directions);
   mines use the same path per panel (the mine mod's "shock noteskin
   chopped per arrow" convention). `StripNote.tap_row` no longer feeds
   shock/mine. Test updated with true-color expectations per variant.
7. **Maintainer scope addition (same review): measure guidelines** —
   one line per measure/bar, matching the in-game guideline layer.
   Pure-layer shape: `render_strip` now takes a `StripScene` (notes +
   `guideline_ms: &[i32]` + guideline/background RGBA); lines draw
   full-width UNDER the notes via the factored-out `blend_px`. The
   tick→raw-ms measure enumeration is task-02's job (through the
   shipped `seek::raw_for_display` interpolation — this layer never
   duplicates the chart's time mapping). New test
   `guidelines_draw_full_width_under_the_notes`. 21 green.
8. **Maintainer review round 2 (2026-08-14)** — guidelines + silver
   shocks/mines approved; two asks: (a) composite the FIRST FRAME of
   the lightning animation over shocks/mines; (b) pick the
   noteskin-appropriate lightning size variant. Landed:
   `lightning_frame0(png_bytes)` (frame 0 of the mine mod's 192×384
   2×4 grid — mine_render.rs's documented layout; None on any other
   shape), additive compositing via a new `additive_stamp` (the game's
   BLEND_SRC_ONE overlay pass — premultiplied channel sums, clamped at
   coverage), and `shock_size_suffix(arrow_shape)` mirroring
   `texture_loader::SHOCK_SIZE_TABLE` ([2,2,2,2,1,0,0,2] → l/l/l/l/
   m/s/s/l; that module is engine-coupled so the pure layer carries a
   cross-referenced mirror + a table-pinning test). 24 green. (Tests
   and impl landed in adjacent edits this round — interleaved with the
   maintainer exchange.)
9. **Maintainer review round 3 (2026-08-14)** — mines approved; shocks
   flagged: in-game the shock lightning is ONE contiguous horizontal
   strike across all 4 arrows, not per-panel copies. Found the STOCK
   asset: `data/arc/2d/2d_shock_effect00.arc` →
   `shock_effect00_{s,m,l}.dds`, 768×384 A8R8G8B8 = 2×4 grid of
   **384×96 strike frames** (verified by extraction + view). Landed:
   `extract_shock_lightning(arc_bytes, suffix)` (variant entry by
   `_<suffix>.dds` name, `decode_dds` generalized to parameterized
   dims, frame-0 crop 384×96); `StripScene.lightning` split into
   `shock_lightning` (ONE additive stamp spanning the shocked side's 4
   columns) + `mine_lightning` (per-panel, unchanged); the shock arm
   now stamps the shocked SIDE's panels (doubles: a left-side shock
   lights columns 0..3, right-side 4..7 — more correct than the old
   all-columns loop). Tests: `shock_effect_arc_extracts_the_strike_frame`
   (multi-entry synthetic arc, per-variant tints, unknown-suffix
   refusal) + the composite test now uses a half/half strike proving
   contiguity (per-panel repetition would fail it). 25 green.

## Validation results

- Harness: **292 passed / 0 failed** (baseline 255 + 3 arc.rs inline +
  9 avslz.rs inline (new mounts) + 25 strip_synth).
- `cargo check --target x86_64-pc-windows-msvc`: clean.
- `cargo fmt --check`: clean.
- `./build.sh` deliberately deferred to the step's engine-facing tasks
  (task-01's approach names harness → check → fmt as its gates).
- Preview renders round 4 regenerated (contiguous stock strike over
  shocks, per-panel mine frame retained) — awaiting maintainer
  re-review.

## Deviations

- Freeze span from kind-2 tail pairing rather than durations[] units
  (context.md Ambiguities #1 — durations' domain unconfirmed; tail
  raw_time is the engine-verified raw-ms source; rebuild_expectations'
  own pairing rule reused).
- Shock/mine art is TRUE-COLOR (maintainer review round 1 — supersedes
  both the task text's implied palette treatment and my interim
  rotate-through-palette call; grounded in research §2.2's
  default-shader binding).
- Measure guidelines added to the pure layer (maintainer scope addition
  during review round 1; `StripScene.guideline_ms` — the live tick→ms
  enumeration lands in task-02).
- Cycle 1 landed tests+impl in one edit (recorded honestly; later cycles
  were strictly failure-first).

## Harness changes (temp-dir infra — recipe update needed in feature progress.md)

- Cargo.toml: `image = "0.25"` dependency added.
- main.rs: `#[macro_export] log_warn!` stub (format-and-discard);
  `core::arc` mount; `services::avs_layeredfs::avslz` mount;
  `mods::training_mode::strip_synth` mount beside section_math.

## Notes

- Real-asset probe verified the research §3 format claims byte-for-byte
  (DDS masks/dims/payload) and visually (cell layout, baked-LEFT art,
  shock variants at x=192/288, caps at col·96, bodies at 384+col·96).
- The env-gated preview test uses a labeled STAND-IN ramp palette; the
  live palette arrives in task-02 via the game's own generators.
