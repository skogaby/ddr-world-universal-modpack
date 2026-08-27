# Progress: live sourcing + texture pipeline (task-02)

Status: Complete (uncommitted — the maintainer handles all git)

Cabinet validation: round 1 (noteskin build) PASSED legs 1–3+5–6 with the
density finding; round 2 (bar-mode build, 2026-08-14) **ALL LEGS PASSED**
(maintainer-confirmed; log-verified fault leg: init resolved all three
anchors, `DDR_STRIP_FAULT=selector` produced exactly one WARN "row
selector/renderer unavailable -- flat tap coloring", flat colors visible
in-game, synthesis 4 ms for 428 notes, texture bound; reverse read live
in the snapshot line `reverse=false`).

## Cabinet probe round 1 (2026-08-14, noteskin build) — results

- PASS: strip appears with the real chart/noteskin/colors; per-song
  refresh across consecutive songs (risk-1 stem probe); no-session songs
  show no strip; performance unremarkable.
- NOT RUN: leg 4 (`DDR_STRIP_FAULT=selector`).
- FINDING: on real high-level charts the noteskin glyph rendering is so
  dense it is practically unusable → maintainer-directed aesthetic
  redesign (host-side A/B on the real casr Single Expert chart, three
  iteration rounds): **bar mode** — taps/heads 1-px quantization-colored
  bars; freeze bodies solid rects; shocks full-width / mines per-panel
  bright blue-white (`[190,230,255]`); freeze head+body same family
  color (approved as-is). Recorded as design R7 SECOND amendment.

## Bar-mode rework (2026-08-14)

- Pure layer: `render_strip_bars` + `row_bar_color` (max-luminance
  palette-row entry — live-palette-sourced bar colors, quantization-hack
  future-proofing intact) + `BAR_H`/`SHOCK_MINE_RGBA` + shared
  `pair_freeze_tails`. 5 new failure-first tests (30 strip_synth total).
  The noteskin rasterizer + sheet/lightning extraction remain (tested)
  for future views.
- strip_hud: synthesis now calls `render_strip_bars`; the sheet/strike/
  mine-frame disk loads, their caches, and the `read_arrow_shape` chain
  are REMOVED from the live path (bar mode needs none of them — the
  sheet-unreadable failure rung is gone). Fault knob values now:
  `selector | palette | synthesis | load`.
- Experiment artifacts (temp harness only, nothing committed): embedded
  casr Single Expert fixture (transient SSQ parse per ssq_format.md §3/
  §5 — 568 rows; expert charts live in `casr_3.ssq` chunk 0x0314) +
  `strip_experiment.rs` A/B renderer. Final side-by-side pair:
  `<temp>/ddr_strip_preview/casr_expert_{baseline,bars}.png`.

## Checklist

- [x] Setup + Explore + Plan (see context.md / plan.md)
- [x] Signatures: `arrow_row_selector` (3-build AOB) +
      `derive_strip_hud_anchors` (RTTI vtables)
- [x] strip_hud.rs: state machine, scene/judge wiring, render pump
- [x] Snapshot: side/notes/chart_end + selector-per-note + live palette
      walk + RTTI validation + fail-open ladder
- [x] Background synthesis + PNG write (bar mode)
- [x] asset_loader pipeline + ONE reused ImageWidget + visibility gate
- [x] mod.rs wiring + `DDR_STRIP_FAULT` injection
- [x] Cabinet probe round 1 (noteskin build): PASS except leg 4; density
      finding → bar-mode redesign
- [x] Bar-mode rework (pure layer + strip_hud) per the R7 second amendment
- [x] Gates after rework: harness 298/298 → check clean → fmt clean →
      ./build.sh (release DLL rebuilt)
- [x] Cabinet check round 2 (maintainer): ALL LEGS PASSED (bar-mode
      legibility, fault injection with visible flat colors — log-verified)
- [x] Reverse-scroll support (maintainer ask during round 2 prep):
      `StripLayout::with_reverse` + the live renderer vb-flag read
      (player_perspective's guarded read shape); 2 pure tests; cursor/
      marker math inherits the flip for free (task-03)
- [x] Close record

## Cabinet probe checklist (the task's deploy validation)

Deploy the probe build, then in one session:

1. **Song 1 (training-active — engage a bound row or fire a gesture):**
   - [ ] The strip appears on the RIGHT edge during gameplay showing the
         real chart (glyphs in the player's noteskin; quantization colors
         matching the lane's arrows; freeze bodies; guidelines per bar).
   - [ ] Log shows: `StripHud: snapshot gen=N side=S notes=… design=D` →
         `StripHud: synthesized … in …ms` → `StripHud: strip texture
         resolved and bound`.
2. **Song 2 (different song, same session):**
   - [ ] The strip shows SONG 2's chart (the risk-1 stem-refresh probe —
         per-song stems + paired release). Log shows a release line at
         song 1's exit and a new gen=N+1 chain.
3. **Untouched song (no session):** the strip must NOT appear (synthesis
   runs; visibility stays off).
4. **Mid-song activation:** start an untouched song, fire triple-4
   mid-song — the strip must appear immediately.
5. **Fault leg:** relaunch with `DDR_STRIP_FAULT=selector` — one WARN,
   flat-colored strip, song plays/judges normally. (Optionally
   `=sheet` ⇒ absent strip.)
6. **Performance:** a dense chart with the strip visible — no observable
   frame cost (the steady state is one static widget + an O(1) pump).

## Deviations

- Task-text correction (RE-verified, recorded in context.md): the
  ArrowRenderer is at actor+0x148, not +0x138. RTTI validation gates
  both objects.
- The load/resolve/bind/visibility path runs on a render-thread pump
  (toast.rs's self-requeueing model) rather than in the judge tick —
  the widget/texture threading rule (AGENTS.md rule 3); the judge tick
  keeps only the once-per-song snapshot (its inputs are judge-scoped).
- Carried from task-01's maintainer rounds (not in the original task
  text): measure guidelines, the stock shock strike, per-noteskin mine
  lightning — all wired into the synthesis inputs.

## Key facts

- New signature: `arrow_row_selector` (44-byte position-independent
  head; 0616 @0x180028130, 0721 @0x180027d10, 0324 @0x180027650).
- New derivation: `derive_strip_hud_anchors` (RTTI
  `.?AVArrowPalette@screen@@` / `.?AVArrowRenderer@screen@@`).
- Palette walk: table POINTER at mgr+0x28, end at +0x30 (count
  validated 8..=64); rows 8..15 → slot 7 rowArg=row−7; phase at
  mgr+0x18; evaluate = vtable slot 1 `(this, rowArg, col, phase) →
  0xAARRGGBB`.
- Strip cache: `./data_mods/_cache/training_hud/training_strip_<gen>.png`.
