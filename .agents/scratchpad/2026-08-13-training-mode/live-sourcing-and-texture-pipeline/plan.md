# Plan: live sourcing + texture pipeline (Training Mode Step 6, task-02)

Status: Approved 2026-08-14 (verified upstream approval — same chain as
task-01; auto mode per the code-assist sop)

## Shape

Engine-facing driver `src/mods/training_mode/strip_hud.rs` + signature
work. Five pieces, implemented in order:

### 1. Signatures (`src/core/signatures.rs`)

- `arrow_row_selector` SignatureDefinition: the 44-byte
  position-independent head (context.md), verified unique on
  0616/0721 (+0324 check during implementation). NOT in
  `required_signatures` — strip_hud degrades.
- `derive_strip_hud_anchors(&mut self)`: resolve
  `.?AVArrowPalette@screen@@` → `arrow_palette_vtable` and
  `.?AVArrowRenderer@screen@@` → `arrow_renderer_vtable` via
  `find_vtable_by_rtti`; store as named addresses (both optional,
  fail-open with the [-] log the finder already emits).

### 2. strip_hud state machine (assist_tick's model)

Module statics: `Mutex<SongState>` {phase, generation, warned_this_song},
resolved fn ptrs/vtables (AtomicPtr, set at init), the widget slot
(OnceLock-ish Mutex<Option<ImageWidget>>), the loaded-asset slot
(Mutex<Option<AssetHandle>> + pending stem/path), background thread
handle implicit (detached, generation-tokened).

Phases: `Idle → Armed (gameplay entry) → Snapshotted (bg thread running)
→ PngReady (path+stem) → Loading (fm handle issued) → Shown/Resolved`.
Exit at any phase: hide widget, release handle, delete file, bump
generation.

- Scene callback: entry ⇒ arm + reset; exit ⇒ teardown (mirrors
  assist_tick's `on_scene_change`).
- Judge callback (per frame, game thread): O(1) steady state.
  - Armed: run the snapshot (once), spawn synthesis, → Snapshotted.
  - PngReady: `asset_loader::load(path, stem)` → Loading.
  - Loading: `asset_loader::resolve_hash` poll → set_texture_id →
    Resolved.
  - Every tick: visibility = Resolved && training_session_active() &&
    scene==GAMEPLAY → show/hide (idempotent widget calls guarded by a
    last-state bool).

### 3. The snapshot (game thread, once per song)

From the dispatching actor:
side (actor+0x84, validated 0/1) → decoded_notes(side) +
chart_end_raw(side); arrow shape via the player_option_table chain
(fallback 0 + WARN); renderer = *(actor+0x148) RTTI-validated → per-note
tap rows via the selector (fallback: flat rows by beat mod — NO, fallback
= flat palette; rows still 1..4 via… see fail ladder below); palette mgr
= *(actor+0x130) RTTI-validated → evaluate rows {1,2,3,4,8} × 256 cols at
phase mgr+0x18 → Box<StripPalette> (ARGB→RGBA).

Fail ladder application (one WARN per song, design §6):
- selector OR renderer invalid ⇒ per-note rows default: tap_row from a
  REPLICATED-nothing… tap_row = 1 for all (flat look), freeze_row = 8.
- manager invalid ⇒ flat ramp palette (task-01's preview recipe) for
  rows 1..4+8.
- notes/chart_end missing ⇒ no strip this song.

### 4. Background synthesis (per song, detached thread)

Inputs (owned): generation, notes Vec, chart_end, shape, palette Box,
per-note rows Vec. Thread body: read+extract sheet (cache per shape in a
static Mutex<HashMap>), strike + mine-frame (cache per suffix; mine
frame only if any kind-20), enumerate measures (display ticks k·4096 →
raw_for_display), build StripNote vec, StripLayout (columns 8 iff any
panel 4..7 participates else 4; column_px 14; height 620), render_strip,
encode_png, create dir + write
`./data_mods/_cache/training_hud/training_strip_<gen>.png`, then post
{generation, path, stem} to the state (PngReady) if the generation still
matches. catch_unwind → WARN + no strip.

### 5. Widget + mod wiring

- Lazy-create ONE ImageWidget on first PngReady (hidden; RIGHT-edge
  default: x = 1280 − width − 8, y centered; width/height from the
  layout used for that song — set_size per song).
- `mod.rs`: `strip_hud::init(signatures)` from enable() (after the
  existing wiring), `strip_hud::shutdown()` on disable (hide + release).
  Scene + judge subscriptions owned by strip_hud (registered at init,
  mirroring assist_tick's handle pattern).

## Validation

- No new host tests (engine-facing); harness suite must stay 292/292.
- Gates: harness → `cargo check` → `cargo fmt` → `./build.sh` (this task
  produces a deployable probe build).
- Cabinet probe (maintainer deploy): two consecutive training songs —
  (a) strip appears with the real chart in the live noteskin/colors;
  (b) song 2 shows ITS chart (stem refresh, risk 1); (c) log lines:
  snapshot, synthesis ms, load→resolve frames, release on exit; (d) a
  forced-failure leg via `DDR_STRIP_FAULT=selector` env (dev-mode
  injection mirroring song_rate's fault knob) ⇒ one WARN + flat/absent
  strip + clean song.

## Risks

- The +0x148 renderer / +0x130 manager offsets could drift on a future
  build — RTTI vtable validation turns drift into the flat-color ladder,
  never a wild call.
- The selector could relocate — AOB verified on 2 builds (3rd checked at
  implementation); resolution failure ⇒ ladder.
- PngFileCallback tolerance of image-crate output: near-zero (research
  risk 2; the pipeline already consumes arbitrary mod PNGs).
- Widget-node consumption: exactly one create (reused), matching the
  preview_overlay discipline.
