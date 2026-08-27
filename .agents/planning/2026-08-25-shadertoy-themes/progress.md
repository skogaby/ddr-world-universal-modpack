# Shadertoy Theme Pack — progress

Updated: 2026-08-25
Status: COMPLETE — cabinet-validated (uncommitted; maintainer commits
manually). Final shape: 11 shader themes + static MINIMAL, BUBBLES
default; arrows/wavefield/mandelbulb retired.
NEXT ACTION: none — feature closed out 2026-08-25.

Resume protocol: read `plan.md` in this directory; the lockstep invariant
and touch-point list live there.

## Done

Batch 1 (TERMINAL, WAVEFORM, SPECTRUM, TUNNEL, XMB):
- Ports + arrows/wavefield removal + count 3→6 generalization
  (`THEME_PROGRAM_COUNT`), fingerprint `v3`→`v4`, BUBBLES default.
- Cabinet-validated by the maintainer same session.

Batch 2 (SQUARES, MANDELBULB, CARD SWIRL, BLOBS, PS2):
- 5 more ports at `shaders/src/themes/theme_{squares,mandelbulb,
  card_swirl,blobs,ps2}.hlsl` (attribution headers with URLs).
- Count 6→11 (`shader_layout::THEME_PROGRAM_COUNT`), `THEME_BLOBS`
  [;12], overlay_draw `[u32; 11]`, 5 new `ThemeProgram` variants,
  THEMES table now 12 entries (MINIMAL still last/static). Fingerprint
  stays `v4` (never shipped; blob-list change re-fingerprints anyway).
- Blob stats (fxc): squares 434 instr, mandelbulb 608 (bulb iteration
  made a dynamic [loop] — unrolled it inlined 6× to 2293), card_swirl
  250, blobs 53, ps2 159.
- Validation: validate_mod_menu.sh 36 pass, validate_overlay_draw.sh 18
  pass, cargo check clean, cargo fmt, ./build.sh clean.

## Deploy & test log

- Batch 1 (2026-08-25): deployed; maintainer: "everything looks great on
  the first try" — all 5 themes approved as shipped.
- Batch 2 deploy #1 (2026-08-25): SQUARES / CARD SWIRL / BLOBS / PS2 OK.
  MANDELBULB froze the game on theme scroll — D3DMetal (CrossOver)
  logged "Fallback to SW fragment processing because buildPipelineState
  failed" + SW vertex/fragment fallback: the Metal pipeline compile for
  the program failed and the whole renderer dropped to software. Cause
  attributed to three-deep dynamic flow (the bulb's [loop] nested inside
  intersect/softshadow's [loop]s + if/else; PS2's plain loop-in-loop
  works fine). Flattened (bulb [unroll], 919 static instrs).
- Batch 2 deploy #2 (2026-08-25): MANDELBULB froze AGAIN with the
  flattened build — whatever D3DMetal chokes on in that marcher runs
  deeper than the obvious nesting (candidate suspects: the stateful
  over-relaxation if/else inside [loop], loop-carried `break` +
  early-out combination, or sheer complexity). Maintainer CUT the theme
  (it's slow even on Shadertoy fullscreen) and replaced it with PRIME
  CUBE (https://www.shadertoy.com/view/w3V3DG): shallow single [loop] +
  one top-level break + fully predicated body, isPrime collapsed to the
  exact {2,3,5,7} lookup (coords span ±10), 150×0.012 steps, 112 static
  instrs.
- Final deploy (2026-08-25): PRIME CUBE renders and performs fine;
  maintainer: "everything is perfect" — all 11 shader themes approved,
  feature closed out.

## Deviations & open questions

- D3DMetal constraint (cabinet-derived, 2026-08-25): theme shaders must
  keep dynamic flow control SHALLOW — top-level [loop]s with
  straight-line/predicated bodies (loop-in-loop as in PS2 is OK; a
  dynamic loop inside a conditional inside a loop is NOT). Violation =
  buildPipelineState failure = whole-renderer software fallback (freeze).
- Batch 2 port notes: MANDELBULB `continue` → guarded body, 6-tap normal
  → 4-tap tetrahedral (single-loop form), intersect 48→40 / shadow
  50→20 steps, bulb 7→5 iterations; CARD SWIRL bakes the permanently
  saturated ramp-in (`time = iTime+10` ⇒ `min(6,speed)` etc. constant);
  PS2 `float[11]` table → unrolled bracket chain, 24→12 trail segments,
  glsl_mod for negative past-times.
- Wrap-seamless status: MANDELBULB, CARD SWIRL, BLOBS, PS2 are FULLY
  seamless (all phases snapped); SQUARES' square-drift `frac(velX*rnd)`
  and fbm pans are non-periodic ⇒ hourly jump-cut (documented in
  header), spin/sway snapped.
- Batch 1 deviations still stand (see headers): WAVEFORM/TUNNEL/XMB
  jump-cuts; XMB 588 static instrs; SPECTRUM 48→32 bars, fake FFT.
- Old configs with `"theme": "arrows"`/`"wavefield"` degrade to BUBBLES
  with one WARN (existing unknown-id path; no migration).

## Key facts for a cold resume

- Lockstep invariant: `ThemeProgram::slot()` == `THEME_BLOBS` PS order ==
  `default_theme_indices` order == published `[u32;11]` order. Host tests
  pin it; SetShader has no bounds check.
- Blob rebuild needs the CrossOver `bemani` bottle (fxc golden path);
  normal DLL builds don't (blobs committed).
- Each theme PS ends with `col *= K` (0.55–0.85) — the menu-dim knob.
  CARD SWIRL is dimmed hardest (0.55; saturated paint).
