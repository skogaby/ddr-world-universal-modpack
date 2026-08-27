# Training Mode (v1) — Implementation Plan

Status: Approved 2026-08-13 (Steps 6–9 restructured 2026-08-14 with the
R7/R12 amendments — chart-strip timeline + FF/RW pulled into v1;
maintainer-directed, feasibility in `docs/chart_strip_hud_research.md`)
Design: `design/detailed-design.md` (Approved 2026-08-13). Section
references (§) below are to the design unless noted.

- [x] Step 1: Identity arm + shifted serving in song_rate
- [x] Step 2: Seek-to-T in song_reset + A/B gestures + restart-from-A
- [x] Step 3: Bound rows, session persistence, silent skip-first start
- [x] Step 4: LOOP SONG — loop driver + early natural end
- [x] Step 5: Score containment (training + assist-tick taints)
- [x] Step 6: Chart-strip timeline HUD + placement row
- [x] Step 7: FF/RW scrobbling (pinpad 7/9)
- [x] Step 8: TRAINING OPTIONS header row + grouping
- [ ] Step 9: Docs, default config, regression pass

---

## Step 1: Identity arm + shifted serving in song_rate

**Objective**: the audio half of the design (§4.5) — the highest-risk piece,
front-loaded: identity-percent arming with `passthrough_plan`,
`IdentityPassthrough` serve mode, and the `{shift_blocks, lead_blocks}`
content mapping (bind-time and stop/replay-time settable).

**Guidance**: new arm-request atomic consumed by `lifecycle` at scene 26;
plan main entry via `passthrough_plan` at 100 % (§5.3 of
`docs/training_mode_research.md` — never `plan_entry(100)`); serving per
§4.5. Minimal `src/mods/training_mode/mod.rs` skeleton (mod id, enable,
eligibility latch, arm request) so the demo is real. One new
`DDR_SONG_RATE_FAULT` leg: bind-refused at identity arm.

**Tests (host)**: identity passthrough byte-identity at `{0,0}`; shifted
serving equals a reference slice + silent tail; lead-region silence;
mapping-change seqlock re-serve; passthrough-plan header equals stock
values. Extend `binding_tests.rs` / `virtual_bank` tests.

**Integrates**: nothing upstream; the mod skeleton registers like any mod.

**Demo**: mod enabled, eligible 100 % song → logs show identity binding
armed; song sounds byte-identical. A temporary log-gated test mapping
(removed in Step 2) plays the song starting mid-content with a silent lead.

## Step 2: Seek-to-T in song_reset + A/B gestures + restart-from-A

**Objective**: the gameplay-state half (§4.4): `request_reset(t_ms != 0)`
real — clamped gates, mapping set between stop/replay, back-dated anchor,
record rebuild at `t_q`, spanning-freeze neutralization, accumulator
policy param. Plus `bounds.rs` A/B state, gestures (triple-4/5/6 =
A/clear/B, the pinpad's middle row — D3 as amended 2026-08-13), and
restart-from-A with the 2.5 s lead.

**Guidance**: transaction order per `docs/training_mode_research.md` §5.4;
anchor math §6 ibid.; freeze pass §3.3 ibid. Audio seeks at rate use the
fresh-seeded WSOLA model per the design §4.5 amendment (2026-08-13:
O(1), frame-exact, byte alignment unpinned across epochs). Gestures via
the GestureBuffer precedent; ControlMessageActor located per §4.2 (RTTI
child walk) for `chart_end_raw` + the seek clamp.

**Tests (host)**: block quantization + anchor-value math in a pure helper;
freeze-neutralization record transform against synthetic note/record
vectors.

**Integrates**: consumes Step 1's mapping API; hooks
`quick_restart_or_fail::trigger_restart` for restart-from-A.

**Demo**: mid-song, triple-4 sets A; triple-1 restarts at A after the
silent approach lead — combo/score reset, claps aligned, works at 75 % and
125 % rate too.

## Step 3: Bound rows, session persistence, silent skip-first start

**Objective**: SKIP FIRST / OMIT LAST rows with `PersistMode::Session`
(new framework variant), select-time audio-length clamp (wavebank-create
publication, §4.6), gameplay-entry bound resolution, and the R15 silent
first start (bind-time pre-shift + first-anchored-frame adjust, §4.3).

**Guidance**: rows via the existing scalar-row machinery + label textures
(`scripts/gen_option_labels.py`); publication cell per §4.6; driver
skeleton (`driver.rs`) hosts the first-start adjust.

**Tests (host)**: clamp math incl. effective-clamp truncation; bound
resolution (A/B vs `chart_end_raw`, margins); publication generation/torn
-read guard.

**Integrates**: pre-shift uses Step 1's bind-time mapping; the adjust
reuses Step 2's anchor/rebuild block (no stop/replay).

**Demo**: set SKIP FIRST 60 at song select (row clamped to the highlighted
song's length) → song starts in silence with notes approaching, music
enters exactly at 1:00; the true beginning is never heard. OMIT LAST set →
bounds visible in logs.

## Step 4: LOOP SONG — loop driver + early natural end

**Objective**: the LOOP SONG row (session-persist, default OFF); LOOP ON =
driver-fired `request_reset(a_ms, 2500, …)` at `b_ms` with accumulator
zeroing; LOOP OFF = ControlMessageActor threshold writes (`+0x94`/`+0x98`)
with the mod-side display-domain converter (§4.2).

**Guidance**: LOOP ON must never write thresholds; LOOP OFF writes at
entry / on live B-set (B behind cursor ⇒ immediate end, accepted). Clamp
loop fire strictly below thresholds per
`docs/training_mode_research.md` §4.3.

**Tests (host)**: display-domain interpolation against synthetic note
vectors; threshold/loop-bound mutual-exclusion state machine.

**Integrates**: driver from Step 3; seek from Step 2; bounds from Step 3.

**Demo**: LOOP ON + section set → grinds the section until triple-3;
LOOP OFF → section end runs the stock banner → results with partial stats.

## Step 5: Score containment (training + assist-tick taints)

**Objective**: R5 — `score_guard::set_training_taint` wired from bound
engagement/gestures/seeks; assist-tick per-side enable latched at gameplay
entry as a taint source (behavior change to the shipped mod, deliberate).

**Guidance**: reuse the existing per-stage suppression + sanitised-logout
machinery verbatim; taint latching per §4.1's session-active predicate.

**Tests**: none host-viable; cabinet + server verification.

**Integrates**: consumes Steps 2–4's engagement signals.

**Demo**: suppression matrix verified against the server: assist tick
alone, bounds alone, seek alone, loop, LOOP OFF partial results — none
submit; a clean untouched song still submits.

## Step 6: Chart-strip timeline HUD + placement row

**Objective**: the amended R7 HUD — a vertical chart-strip timeline on
the screen edge: noteskin-accurate glyphs (player's `2d_arrowNN` sheet,
live palette/quantization coloring, freeze bodies, shocks/mines) at
content-time positions, pre-rendered once per song into ONE texture on a
background thread; ONE static ImageWidget + dynamic cursor / A/B / loop
markers + a small `m:ss / m:ss` readout; TIMELINE PLACEMENT row
(LEFT/RIGHT, default RIGHT, `PersistMode::Full`, wire
`mod_training_progress_pos`).

**Guidance**: full pipeline + RE in `docs/chart_strip_hud_research.md` —
ARC/DDS extract (`core/arc.rs` + `avslz`, uncompressed A8R8G8B8
768×192), rasterize → `image`-crate PNG → the mine-texture
FileManager pipeline (`texture_loader.rs` precedent, lazy poll,
refcounted release); chart from `song_reset::decoded_notes`; color by
READING the game (palette snapshot / row-selector call — never
replicate); markers per frame from `current_raw_music_count` /
`chart_end_raw` / bounds accessors; assist_tick's `CACHED_ARROW_SHAPE`
chain for the chosen design. Fail-open ladder per the amendment.
Backend: bemani-buddy migration 015 + playdata applier for the wire
field.

**Tests (host)**: strip layout math (ms→y, column mapping, glyph
placement, m:ss format); rasterizer against synthetic note vectors +
fixture sheet/palette (pure layers).

**Integrates**: bounds/driver state (Steps 3–4); Step 5's taint is
untouched (the HUD is read-only).

**Demo**: strip shows the real chart in the player's noteskin at the
correct positions/colors; cursor tracks play; A/B markers appear on
gesture; cursor jumps correctly on loop and restart-from-A; placement
row moves the strip left/right; a HUD-failure song still plays clean
(fail-open).

**As landed** (CLOSED 2026-08-15 after 5 demo rounds — details in the
task records under `.agents/scratchpad/2026-08-13-training-mode/`):
BAR MODE instead of noteskin glyphs (R7 second amendment — glyphs
unreadable on expert charts; 1-px quantization-colored bars, colors
from the fixed offline ramp with the live generator walk parked behind
`USE_LIVE_PALETTE=false`; per-note ROW selection stays live); the
placement row is **OFF/LEFT/RIGHT, default OFF, and the SOLE
HUD-visibility control** (round-4 UX amendment — replaces the
session-active predicate; OFF skips the whole per-song pipeline);
A/B lines + veil always render (song-start/end fallbacks, whole-song
veil); loop fire margin guards only the stock thresholds (fires AT the
user's marker); strip widget force-created before the overlay widgets
(z = creation order); reverse scroll, placement backend round-trip
(migration 015), and score containment all cabinet-verified.

## Step 7: FF/RW scrobbling (pinpad 7/9)

**Objective**: the amended R12 — single-press pinpad 7 = rewind / 9 =
fast-forward by `training_mode.{rw,ff}_increment_ms` (default 5000)
during eligible gameplay, via the shipped seek transactions; the
timeline cursor is the feedback.

**Guidance**: gesture surface beside the existing triple-4/5/6 handling
in `bounds::on_input_event` (single-press, GAMEPLAY-gated, mod-active
gated); target = `current + increment` clamped to `[0, min(b_live?,
chart_end) − margin]` block-quantized (the marker-clamp math); fire
through `song_reset::request_reset(t, lead, Zero, None)` with a cooling
latch (one in-flight, the loop driver's pattern); Step 5's
`on_song_reset(t>0 || session_active)` subscriber taints automatically;
config keys parsed beside `quick_restart`'s pattern.

**Tests (host)**: clamp/quantize math; config parse/defaults.

**Integrates**: timeline from Step 6 (works without it — cursor is just
the visible feedback); score containment from Step 5 for free.

**Demo**: 7/9 skip backward/forward by the configured increment at 100%
and at rate; claps/judging stay aligned after every skip; skips near the
chart end clamp; the skipped song's score is suppressed, and an
untouched song (no skips) still submits.

**As landed** (CLOSED 2026-08-15 after 4 demo rounds — details in the
task record `.agents/scratchpad/2026-08-13-training-mode/ff-rw-scrobbling/`):
scrub dispatches with **NO approach lead** (`request_reset(t_q, 0, Zero,
None)` — a pure music-player timeline adjuster; rewind-past-start = the
instant t=0 restart; TRAINING_LEAD_MS stays section-practice-only);
RW/FF **indicator icons** flash left/right mid-height with the toast
fade (repo-shipped PNGs `training_scrub_{rw,ff}.png`, generator
`scripts/gen_training_scrub_icons.py`, new `scrub_indicator.rs`);
`SCRUB_COOLING` clears via the new `song_reset::reset_in_flight()`.
Round 2/3 surfaced + fixed three shipped-machinery interactions:
(1) assist_tick's `on_song_reset` full-resynthesis (the checkpoint-4
loop gap) replaced by the Playing→Ready **re-shift demotion** — claps
resume within a frame of any reset; (2) a **reset clap floor**
(`mute_head_bytes` on `rewrite_tick_wave`) keeps consumed pre-target
notes clap-silent through the loop's approach lead (and the R15
skip-first lead); (3) **LOOP ON bypasses natural death** — the loop
latch arms the engine's own instant-death gate (`GamePlayActor+0x2B7`,
stash-restored at disarm/session end), the driver fires the loop on
`any_actor_dead()` ("death revive"), the reset's shipped flag-clear +
gauge restore is the revive; quick-fail remains the deliberate exit,
stock death on every degraded path. All round-4 legs PASSED
(lead-in silent, first post-A clap exact, death revive instant,
LOOP OFF death still fails out).

## Step 8: TRAINING OPTIONS header row + grouping

**Objective**: `UiKind::Header` in custom_options (§4.8): donor clone +
`+0x28` vtable swap `{return 0, no-op}` + half-height `+0xA8` +
label-only render; the R10 policy (headers injected only when listed in
`row_order`); the `header_training_options` label asset.

**Guidance**: mechanism per `docs/option_header_rows_research.md` —
zero new signatures; render/asset per the existing enum-row pipeline.

**Tests (host)**: ordering-policy unit tests (header unlisted ⇒ absent;
listed ⇒ positioned; normal rows unchanged).

**Integrates**: independent of Steps 1–7 (pure options UI); the grouped
ordering lands in Step 9's default config.

**Demo**: MODS tab shows the slim full-width TRAINING OPTIONS header;
cursor skips it in both directions and on tab-open; scrolls with its
group; removing it from `row_order` hides it.

**As landed** (CLOSED 2026-08-15 after 6 demo rounds — details in the
task record
`.agents/scratchpad/2026-08-13-training-mode/header-rows-and-grouping/`):
two deviations from the objective as written. (1) **Full-height row, not
half-height** — the `+0xA8` halving shrinks only the LAYOUT slot, not
the clip art (next row overlapped the bar); clip Y-scaling stretched or
displaced the label; full-box label bitmaps (32px/35px + measured pin
nudge) always bled below because the label-zone origin sits under the
row's box grid line. Final look: standard full-height slot, the header's
entire art is the OPAQUE 352x16 label texture (dark blue `#182860`,
white centered text at 70% label size) rendered centered in the text
zone with margin on all sides; `render_header` hides `choice_usr` (value
box/marker/arrows) AND `invalid_usr` (the clip's default-state gray
cover — ordinary rows clear it via the donor `onFocusChanged(false)`
that headers deliberately stub). (2) **Cursor skip lives in
`options_scroll`, not the native scan** — the mod scroll driver replaces
the native directional step whenever the Mods tab overflows the 7-row
viewport (always, at 33 rows), so the research §2 native `+0x28`
predicate never runs there; `RowHandle` gained `selectable` (false for
headers) and `predict_target` + tab-strip entry now skip unselectable
rows. Everything else per the objective: shared `+0x28` mod vtable
`{return 0, no-op}`, R10 (unlisted header EXCLUDED, identity fast-path
byte-identical for normal rows), `RegisterError::HeaderCarriesState`
validation, zero new signatures/detours; harness 324/324 (18 new
tests, `ordering.rs` + `header_rows_tests.rs` newly mounted).

## Step 9: Docs, default config, regression pass

**Objective**: README feature row + config section (`training_mode` block
reserved, grouped `row_order` example incl. `step_data_export`), AGENTS.md
entries (feature row + config bullet), assist-tick README note (taint
change), design §7 cabinet checklist executed end-to-end.

**Guidance**: follow the repo's README/AGENTS.md conventions; note v2/v3
sketches as future work.

**Tests**: the full cabinet regression checklist (design §7), including
composition runs (rate + assist tick + training simultaneously).

**Integrates**: closes the feature; readiness gates (`cargo check`, fmt,
`./build.sh`) per repo convention.

**Demo**: a fresh operator following only the README reproduces the
grouped options and a full training workflow.
