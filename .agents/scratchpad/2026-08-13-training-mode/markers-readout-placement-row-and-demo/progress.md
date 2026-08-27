# Progress: markers, readout, placement row, backend + demo (task-03)

Status: Complete (uncommitted — the maintainer handles all git)

Cabinet validation: 5 demo rounds (2026-08-15), round 5 **ALL LEGS
PASSED** (maintainer-confirmed: "all legs passed and everything looks
great in-game") — plan Step 6 TICKED. Round history: R1 = 6 findings
(stock ribbons, stale fault env, B-line fallback, cursor, veil,
readout); R2 = fixes verified + reverse scroll + 4 follow-ups (loop
early-fire, A/veil always-on, peak-phase palette); R3 = overlay z-order
race + ramp-palette directive; R4 = ALL PASSED incl. placement
card-out/in backend round-trip + the OFF/LEFT/RIGHT sole-visibility UX
amendment; R5 = amendment verified, everything green.

## Round 4 (2026-08-15) — results + UX amendment

PASSED: everything from rounds 1–3 (z-order, ramp colors, loop-at-
marker, always-on A/B/veil) + the placement card-out/in backend
round-trip. LOOP SONG backend question answered: `training_loop_song`
is `PersistMode::Session` BY DESIGN (Step 4 — session-scoped like the
START/END rows; resets each session) — no backend column expected, not
a gap.

UX amendment (maintainer): the TIMELINE PLACEMENT enum becomes
**OFF / LEFT / RIGHT** and is the SOLE visibility control for the HUD —
replacing the "any training feature active" session predicate. No
backwards compat needed (maintainer is the only user; old stored 0 =
RIGHT now reads as OFF and can be re-set).

Implementation:

- `mod.rs`: `PLACEMENT_OFF=0 / LEFT=1 / RIGHT=2`, default OFF (fresh
  profiles opt in); the OFF ribbon is the stock `seop_op_off`;
  `on_progress_pos_change` passes the raw value to
  `strip_hud::set_placement(side, value)`.
- `strip_hud.rs`: tri-state `PLACEMENT`/`LATCHED_PLACEMENT` AtomicI32s
  (out-of-range → OFF, fail-safe hidden); GAMEPLAY-entry arm gate skips
  the ENTIRE per-song pipeline when the latched placement is OFF (no
  snapshot/synthesis/texture — one debug line); both visibility
  predicates (strip widget + overlay) now read `latched_visible()`
  instead of `bounds::training_session_active()`. Placement still
  latches per song at entry (edits apply next song).
- `gen_option_labels.py`: placement preview copy updated ("OFF hides
  it"); textures regenerated — `seop_image_training_progress_pos.png`
  CHANGED (cabinet sync needed).
- Backend: no change needed — the wire value 0/1/2 rides the existing
  nullable-INT `opt_mod_training_progress_pos` verbatim.

Gates: harness 301/301 → check clean → fmt clean → ./build.sh.

## Round-5 verification checklist (ALL PASSED 2026-08-15)

- [x] Placement row shows OFF / LEFT / RIGHT (stock ribbons); default
      OFF on a fresh/reset profile
- [x] OFF: no timeline during play (even with training features
      active); LEFT/RIGHT: timeline always shown on that edge (even
      with NO training features active)
- [x] Log on an OFF song: "placement OFF -- HUD idle this song" (no
      snapshot/synthesis lines)
- [x] Backend round-trip of the new values (card-out with LEFT/RIGHT/
      OFF, card back in)
- [x] Sync `data_mods/.../tex/seop_image_training_progress_pos.png`
      (regenerated preview copy)

## Round 3 (2026-08-15) — results + fixes

Round-2 follow-ups verified working EXCEPT:

1. **Overlay drawn BEHIND the strip** (veil/A/B/cursor only peeking at
   the overhang edges — screenshot-confirmed). Not a code change:
   widget z = CREATION order, and the strip widget (created at first
   strip-texture resolve) RACES the overlay widgets (created at
   marker-texture resolve, which only happens once a session is
   active). Rounds 1–2 happened to create the strip first; round 3
   engaged the session before the strip texture landed, parking the
   whole overlay under the strip for the process lifetime. Fixed
   deterministically: `ensure_strip_widget()` force-creates the (hidden,
   unbound) strip widget immediately BEFORE the overlay's image widgets
   are first created (and the resolve path shares the same helper) — the
   strip is now always below, overlay always above.
2. **Colors still flat with the live palette** (even peak-phase swept).
   Maintainer directive: ship the OFFLINE ramp — the approved
   host-render recipe, which `flat_ramp_palette` already reproduces
   verbatim (TINTS: 4th [255,90,130], 16th [255,215,80], 8th
   [110,150,255], other [140,255,120], freeze [130,230,140]). The live
   `walk_palette` machinery (RTTI validation + peak-phase sweep) stays
   implemented behind `USE_LIVE_PALETTE = false` for a future revisit.
   Per-note ROW classification stays LIVE (the game's own quantization
   selector — `taps=live`); snapshot INFO now reports `palette=ramp`.

Gates: harness 301/301 → check clean → fmt clean → ./build.sh.

## Round-4 verification checklist

- [ ] Veil + A/B lines + cursor draw ON TOP of the strip (engage the
      session early in song 1 — the old race's worst case)
- [ ] Bar colors match the approved offline renders (`palette=ramp` in
      the snapshot log)
- [ ] Loop with a row-set end marker fires AT the marker (round-2 fix —
      verify if not yet observed)
- [ ] A line at song start / B at end / whole-strip veil with no markers
- [ ] Placement card-out/in server round-trip (still unconfirmed)

## Re-demo round 2 (2026-08-15) — results + follow-ups

PASSED: findings 1–6 fixes verified working ("everything mostly working
now"); reverse scroll confirmed (tested live, log `reverse=true` at
01:20:38); log shows `taps=live palette=live` on all three songs — the
live palette machinery works (round 1's flat colors WERE the stale env
var).

Follow-ups (all fixed):

1. **Loop fired ~2 s before the end marker** (B at 70 s looped at ~68 s).
   Log-verified: row B resolved to 69999 ms (grid quantization, −1 ms)
   and `loop fire bound 68999 ms … margin 1000 ms` — the driver
   subtracted the 1000 ms end-margin from the USER'S marker.
   Fixed in `section_math::loop_fire_bound`: the margin now applies
   ONLY to the stock-threshold terms —
   `min(b_live, min(t94_raw, t98_raw) − margin)` — so the loop fires AT
   the marker while the cascade guard stays authoritative (a marker
   inside the threshold window still clamps to threshold − margin).
   Tests updated (+2 cases: exact-marker fire, marker-near-end clamp).
2. **A line always renders** — falls back to song start (0) when no
   start marker, mirroring B's chart-end fallback; the shared top clamp
   keeps it fully visible at the strip edge.
3. **Veil always shows** — `strip_synth::section_veil` now returns the
   active region unconditionally (`[a or 0, b or chart_end]`); no
   markers = whole song active = whole strip shaded. Test updated.
4. **Colors "a little flat" vs the host renders** — the palette
   generators ANIMATE on `phase` (borders blink on beat quarters, body
   cells pulse); a peak-phase sweep was added — then SUPERSEDED by
   round 3's ramp directive (the sweep remains in the parked live path).

## Demo round 1 (2026-08-15) — 6 findings + fixes

1. **LEFT/RIGHT ribbons didn't render** — the generated
   `seop_op_left/right.png` collided with the game's STOCK ribbon
   lookup at atlas injection. Fixed: RIBBONS entries reverted in
   `gen_option_labels.py` (+ a "never add stock ribbon names" comment),
   both PNGs deleted from the repo output dir, script re-run (now 32
   labels + 4 ribbons + 28 previews). Row registration keeps the
   `seop_op_right`/`seop_op_left` names — they resolve to the stock
   atlas entries. **Maintainer must also delete the two PNGs from the
   cabinet's data_mods copy.** [VERIFIED round 2]
2. **Flat strip colors** — ROOT-CAUSED as a stale
   `DDR_STRIP_FAULT=selector` env var left set from the round-2 fault
   leg (maintainer-confirmed). NOT an RTTI bug: static RE on 20260721
   shows actor+0x130/+0x148 unchanged (actor init `FUN_18005cce0`,
   ArrowRenderer ctor `FUN_1800264e0` stores vptr 0x18035cbd8 = exactly
   the resolved `arrow_renderer_vtable` +0x35CBD8), and
   playfield_styling's fill hook classified the live arrow renderer
   with the SAME vtable in the same second as the WARN. Hardening
   shipped: loud "DDR_STRIP_FAULT=... is SET" WARN at snapshot;
   distinct one-shot WARNs per color rung; snapshot INFO carries
   `taps=live|flat palette=live|flat`. [VERIFIED round 2: live/live]
3. **B line hidden with no end marker** — falls back to `chart_end_ms`
   (always drawn); line tops clamp to `[0, height-h]`. [VERIFIED;
   extended to the A line in round 2]
4. **Cursor** — 6 px tall, 5 px overhang, yellow (`0xFF00FFFF`).
   [VERIFIED round 2]
5. **Veil imperceptible** — `0xA0FF7828` mostly-opaque blue tint.
   [VERIFIED round 2; now always-on]
6. **Readout huge + clipped** — scale 0.4; Center alignment verified
   (per-line about x, the toast model); center-x clamped on-screen via
   estimated glyph width (17 px/char at scale 1.0). [VERIFIED round 2]

## Checklist

- [x] Setup + Explore (bounds accessors verified: rows feed A_MS/B_MS —
      the veil predicate covers rows and gestures uniformly)
- [x] Visual plan maintainer-APPROVED (in-session) with one amendment:
      the active-section veil shows whenever EITHER marker is set
      (superseded round 2: the veil ALWAYS shows the active region —
      whole song when unbounded)
- [x] Plan: plan.md (Status: Approved)
- [x] Pure veil-span helper `strip_synth::section_veil` (failure-first;
      33 strip_synth tests → suite 301)
- [x] Marker asset PNG (data_mods/training_mode/tex/training_marker.png,
      4x4 outline-baked, hand-rolled writer — committed, 79 bytes)
- [x] Placement plumbing: strip_hud per-side atomics + per-song entered-
      side latch + strip_origin (strip x now follows placement too)
- [x] Overlay: OVERLAY widget set (track/veil/A/B/cursor ImageWidgets by
      creation-order z + readout TextWidget), marker asset loaded once
      via asset_loader (never released — process-lifetime chrome), UV
      center-row sampling for veil/track (outline rows stay thin only on
      lines), overlay_update() on the existing render pump (<=5 position
      writes + occasional text re-layout), readout "m:ss / m:ss" updated
      on second change, fail-open ladder (no strip -> translucent track;
      no marker asset -> readout only)
- [x] TIMELINE PLACEMENT row (enum RIGHT=0 default / LEFT=1,
      PersistMode::Full builder default -> wire mod_training_progress_pos)
      registered after LOOP SONG + seeding + availability toggles
- [x] Textures: label + preview via gen_option_labels.py (LEFT/RIGHT
      value ribbons are STOCK — never generate stock ribbon names)
- [x] bemani-buddy (SEPARATE repo, maintainer commits): migration 015 +
      db model/mysql row-map/UPDATE/bind + protocol structs (load
      serialize + save deserialize) + playdata load/save plumbing +
      5 verbatim-storage tests (44/44 green); sqlx cache regenerated
      (migration applied to the local dev DB); clippy clean; my edits
      follow local file style (pre-existing whole-repo fmt drift left
      untouched)
- [x] Gates: harness 301/301 -> check clean -> fmt clean -> ./build.sh
- [x] Step-6 demo round 1 (maintainer, 2026-08-15): 6 findings
- [x] Fix round for the 6 findings + gates re-run
- [x] Re-demo round 2 (maintainer): fixes verified + reverse scroll
      PASS + 4 follow-ups (loop early-fire, A-line/veil always-on,
      palette peak-phase bake) — all fixed, gates re-run
- [x] Round 3 (maintainer): 2 findings (overlay z-order race, ramp
      palette directive) — fixed, gates re-run
- [x] Round 4 (maintainer): ALL PASSED incl. placement card-out/in
      round-trip; UX amendment (OFF/LEFT/RIGHT sole-visibility) —
      implemented, gates re-run
- [x] Round 5 (maintainer): ALL LEGS PASSED -> plan Step 6 TICKED
- [x] Close record

## TDD cycles

- loop_fire_bound margin reshape: updated
  `loop_fire_bound_composes_min_and_margin` first (exact-marker fire +
  marker-near-end clamp + degenerate-marker cases), then the
  implementation; suite 301/301.
- section_veil always-on: updated `section_veil_spans_the_active_region`
  (whole-song span for no markers), then the one-line predicate change.

## Deviations

- Veil semantics per the round-2 maintainer amendment: ALWAYS shade the
  active region (supersedes both the task text's loop-gated veil AND
  the round-1 "either marker set" amendment).
- Placement latched per song at GAMEPLAY entry (context.md ambiguity 2).
- LEFT/RIGHT value ribbons are the game's stock atlas entries, not
  generated (demo finding 1 — generated stock names collide).
- Loop fire margin applies to the stock-threshold terms only (round-2
  follow-up 1) — design §4.3's "min(...) − margin" shape amended.
- walk_palette bakes the PEAK-phase palette (16-phase sweep) rather
  than one instant (round-2 follow-up 4) — the "call the game's own
  generators" constraint holds; the sweep just samples the game's own
  animation across a beat.
- Bar COLORS ship from the fixed offline ramp (round-3 maintainer
  directive — the approved host-render recipe; the live walk, sweep
  included, is parked behind `USE_LIVE_PALETTE = false`). The
  "never replicate the color math" constraint is CONSCIOUSLY relaxed
  for bar colors by the maintainer ("I can revisit later if needed");
  per-note ROW classification remains live.
- Strip widget force-created before the overlay's image widgets
  (round-3 finding: widget z = creation order and the two texture
  resolves race — the session-engaged-early case parked the overlay
  under the strip for the process lifetime).
- TIMELINE PLACEMENT is OFF/LEFT/RIGHT and the SOLE HUD-visibility
  control (round-4 UX amendment — supersedes the design's
  session-active visibility predicate and the RIGHT default; default
  is now OFF). Enum value 0 changed meaning (RIGHT→OFF) with no
  compat shim (maintainer-approved).
