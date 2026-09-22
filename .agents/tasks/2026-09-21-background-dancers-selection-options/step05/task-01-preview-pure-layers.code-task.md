# Task: Preview pure layers — pick builders, box layout + camera extents, the settle state machine

## Description
The host-testable half of the preview driver (design §4.6, §5.3, §4.7 amendment): the two preview pick
builders on `Pick`, `preview/layout.rs` (the box in canvas / render-target space, the per-side constants,
the frustum EXTENT maths for the cropped stage camera and the fixed dancer camera) and `preview/state.rs`
(the per-side focus / wanted / 150 ms settle / live-identity state machine, generic over the identity
type). Everything here is std-only (or `super::selection` for `pick.rs`) so
`scripts/validate_background_dancers.sh` mounts and tests it; task-02 wires it to the engine.

## Background
`Pick` (`src/mods/background_dancers/pick.rs`, Step 4) can already describe stage-only / dancer-only
scenes through `assemble_pick_opt`; the previews need two named builders: STAGE `key` ⇒ a uniform row of
that key (the gameplay rule for a chosen stage — `selection::resolve_choice`'s second stage) with no
dancers; DANCER `key` ⇒ that candidate alone, `stage: None`, playlist shuffled from its sex pool, parts
present per `arc_exists`.

The options modal's preview panel sits at `CHROME_ORIGIN[side]` = P1 `(185, 463)`, P2 `(742, 463)` in
the 1280×720 canvas (the WebUI overlays' measured constant, currently in `viewport_smoke.rs`); the row's
template marker (default `(191, 11, 170, 150)` — design §4.7 amendment: 170×150, NOT 16:9) is relative to
it; `RtRect::from_canvas` maps canvas → render-target pixels. Camera aspect handling under the amendment:
the stage's `.camanm` frustum keeps its VERTICAL half-tangent and the horizontal becomes
`half_tangent_y × box_aspect` (a centre crop of the stock 16:9 frame); the dancer camera is
`eye (0, 1.05, 3.4) → target (0, 0.95, 0)`, up +y, `half_tangent_y = 0.32` (so `half_tangent_x = 0.32 ×
aspect`), near 0.1, far 100 — frames a 1.8 m figure with headroom; the no-camera-set fallback is the
gameplay fixed camera `(0, 1.6, 5) → (0, 0.9, 0)`, hFOV 76.8° AT 16:9, cropped the same way.

Preview events (design §4.6): `on_preview_request(side, id)` fires every focus tick for the focused row
(`id ∉ ours` ⇒ focus left); the value is read live; `0` (RANDOM) ⇒ no 3D; `k` ⇒ wanted = `(kind, key)`
with `settle_at = now + 150 ms` re-armed on every CHANGE of wanted; `on_menu_close(side)` / scene leaving
25 ⇒ everything cleared. `on_frame` starts the wanted preview once the settle elapsed and it differs from
the live identity, tearing the live one down first.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md`
  (§4.6, §4.7 incl. the 2026-09-21 amendment, §5.3, §7 "preview/layout.rs" tests)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md` (Step 5)
- `src/mods/background_dancers/pick.rs` (`assemble_pick_opt`, the fixtures used by its tests)
- `src/mods/background_dancers/selection.rs` (`resolve_choice`'s chosen-stage rule, `Rng::below`)
- `src/mods/background_dancers/viewport_smoke.rs` (`CHROME_ORIGIN`, `FALLBACK_MARKER` — move them)
- `src/services/scene3d/viewport_pass_layout.rs` (`RtRect::from_canvas`, `CANVAS_W/H`)
- `scripts/validate_background_dancers.sh` (mounting convention — `preview/*.rs` files mount as
  `preview_layout` / `preview_state`; they must not `use crate::` or `super::`)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `pick.rs`: `pub fn stage_only(rng: &mut Rng, stages: &[StageCandidate], camera_rows: &[(String,
   Vec<String>)], key: &str) -> Option<Pick>` (rows with `key` ⇒ `rng.below(rows.len())` picks one — the
   `resolve_choice` shape; none ⇒ `None`; `assemble_pick_opt(rng, Some(row), camera_rows, vec![], false,
   |_| false)`), `pub fn dancer_only(rng: &mut Rng, dancers: &[DancerCandidate], key: &str, arc_exists:
   impl Fn(&str) -> bool) -> Option<Pick>` (`assemble_pick_opt(rng, None, &[], vec![cand], false,
   arc_exists)`). Both stamp `source_stage` / `source_dancers` with `PickSource::Option`.
2. New pure `src/mods/background_dancers/preview/layout.rs` (std only):
   - `pub const CHROME_ORIGIN: [(f32, f32); 2] = [(185.0, 463.0), (742.0, 463.0)]`, `pub const
     FALLBACK_MARKER: (f32, f32, f32, f32) = (191.0, 11.0, 170.0, 150.0)`.
   - Constants (§5.3): `SETTLE_MS: u64 = 150`, `SLOT_BASE: [u32; 2] = [0, 16]`, `MAX_PREVIEW_INSTANCES:
     usize = 16`, `STAGE_CUT_PERIOD_S: f32 = 9.0`, dancer camera `DANCER_EYE: [f32; 3] = [0.0, 1.05, 3.4]`,
     `DANCER_TARGET: [f32; 3] = [0.0, 0.95, 0.0]`, `DANCER_HALF_TANGENT_Y: f32 = 0.32`, `DANCER_NEAR: f32 =
     0.1`, `DANCER_FAR: f32 = 100.0`; fallback `FALLBACK_EYE = [0.0, 1.6, 5.0]`, `FALLBACK_TARGET = [0.0,
     0.9, 0.0]`, `FALLBACK_HFOV_DEG: f32 = 76.8`, `FALLBACK_FAR: f32 = 500.0`; `BACKDROP_ARGB: u32 =
     0xFF0C_0C14` (the colour clear behind every preview — dark, near-black blue; a D3DCOLOR `0xAARRGGBB`).
   - `#[derive(Clone, Copy, PartialEq, Debug)] pub struct CanvasRect { x, y, w, h: f32 }` with
     `aspect()`; `pub fn box_rect(side: usize, marker: (f32, f32, f32, f32)) -> CanvasRect` =
     `CHROME_ORIGIN[side.min(1)] + marker`.
   - `#[derive(Clone, Copy, PartialEq, Debug)] pub struct Extents { l, r, b, t: f32 }` (half-tangents at
     `w = 1`); `pub fn crop_to_aspect(vertical_half_tangent: f32, aspect: f32) -> Extents` (`t = v, b = −v,
     r = v·aspect, l = −r`); `pub fn dancer_extents(aspect) -> Extents` (`crop_to_aspect(0.32, aspect)`);
     `pub fn fallback_extents(aspect) -> Extents` (`crop_to_aspect(tan(hfov/2) / (16/9), aspect)`).
3. New pure `src/mods/background_dancers/preview/state.rs` (std only), generic:
   ```rust
   pub struct SlotState<I: Clone + PartialEq> { focused: bool, wanted: Option<I>, settle_due: Option<u64>, live: Option<I> }
   pub enum Action<I> { None, Teardown, Start(I) }
   impl SlotState { new(); on_request(&mut self, wanted: Option<I>, now_ms: u64)  // Some = a non-RANDOM value of OUR row; None = RANDOM / another row
                    on_clear(&mut self)                                          // menu close / scene exit: focused=false, wanted=None, settle_due=None (live untouched — the driver tears it down)
                    poll(&mut self, now_ms: u64) -> Action<I>                    // settle elapsed ∧ wanted ≠ live ⇒ Teardown (live present) else Start(wanted) ; wanted None ∧ live present ⇒ Teardown
                    mark_started(&mut self, id: I); mark_torn_down(&mut self); live(&self) -> Option<&I>; is_focused(&self) -> bool }
   ```
   Semantics: `on_request(Some(w))` sets `focused = true`; if `w != wanted` ⇒ `wanted = Some(w)`,
   `settle_due = Some(now + 150)`; an unchanged `w` leaves the deadline alone. `on_request(None)` ⇒
   `focused` stays true only for a RANDOM value of our row (caller passes `focused_ours: bool`) —
   simplify: `on_request(focused_ours: bool, wanted: Option<I>, now_ms)`. `poll`: if `wanted.is_none()`
   ⇒ `Teardown` when live else `None`; if `Some(w)` and `w == live` ⇒ `None`; if the deadline has not
   passed ⇒ `None`; else `Teardown` when live (the driver calls `poll` again after the teardown finishes)
   else `Start(w)`.
4. `viewport_smoke.rs`: `CHROME_ORIGIN` / `FALLBACK_MARKER` become `pub use`/imports from
   `super::preview::layout` (task-02 creates `preview/mod.rs`; for THIS task add a minimal
   `src/mods/background_dancers/preview/mod.rs` declaring `pub mod layout; pub mod state;` and register
   `pub mod preview;` in `background_dancers/mod.rs`).
5. Harness: mount `preview_layout` (`src/mods/background_dancers/preview/layout.rs`) and `preview_state`
   (`src/mods/background_dancers/preview/state.rs`).
6. Host tests: `pick.rs` — `stage_only("boom00")` picks a boom00 row, no dancers, camera lists from that
   row, source `Option`; unknown key ⇒ `None`; `dancer_only("emi01")` ⇒ `stage None`, one dancer, source
   `Option`, `arcs_for(&PREVIEW)` without the shadow arc. `layout.rs` — `box_rect(0, FALLBACK_MARKER) ==
   (376, 474, 170, 150)`, `box_rect(1, …) == (933, 474, 170, 150)`, aspect `170/150`; `crop_to_aspect(0.5,
   2.0) == {l:-1, r:1, b:-0.5, t:0.5}`; `dancer_extents(170/150).r ≈ 0.3627`; `fallback_extents(16/9)`
   reproduces the gameplay frustum (`r == tan(38.4°)`, `t == r/(16/9)`) and at `170/150` keeps that `t`.
   `state.rs` — request → no Start before 150 ms → Start at ≥ 150 ms; a value change at 100 ms re-arms
   (Start only at 250 ms); repeated identical requests do not re-arm; `on_request(false, None)` with a live
   preview ⇒ `Teardown` at the next poll; `on_clear` ⇒ `Teardown`, then `mark_torn_down` ⇒ `None`; wanted
   == live ⇒ `None`; changing wanted while live ⇒ `Teardown`, then after `mark_torn_down` ⇒ `Start(new)`.

## Dependencies
- Step 4 (`pick::assemble_pick_opt`, `ParseOptions`).

## Implementation Approach
1. Tests first in `pick.rs`, `layout.rs`, `state.rs`; then the code; mount; harness green.
2. Move the two constants out of `viewport_smoke.rs`; `cargo check`; `cargo fmt`.

## Acceptance Criteria

1. **Pick builders**
   - Given the real fixture tables
   - When `Pick::stage_only(rng, stages, camera_rows, "boom00")` / `Pick::dancer_only(rng, dancers, "emi01", …)` run
   - Then the picks have the shapes in requirement 6 and unknown keys return `None`.

2. **Box geometry**
   - Given `FALLBACK_MARKER`
   - When `box_rect(0, …)` and `box_rect(1, …)` are computed
   - Then they are `(376, 474, 170, 150)` and `(933, 474, 170, 150)`.

3. **Extents**
   - Given `crop_to_aspect(v, a)`
   - When evaluated
   - Then `t == v`, `b == −v`, `r == v·a`, `l == −r`; `fallback_extents(16/9).r == tan(38.4°)` within 1e-6.

4. **Settle machine**
   - Given a fresh `SlotState`
   - When requests arrive at 0 ms (`A`), 100 ms (`B`) and polls run at 149 / 249 / 250 ms
   - Then the polls return `None`, `None`, `Start(B)`.

5. **Harness + build**
   - Given the crate
   - When `./scripts/validate_background_dancers.sh` and `cargo check --target x86_64-pc-windows-msvc` run
   - Then both are clean with `preview_layout` / `preview_state` mounted and `viewport_smoke.rs` reading
     `CHROME_ORIGIN` from `preview::layout`.

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, preview, pure-layer, host-tested
- **Required Skills**: Rust, the repo's pure-module harness convention
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 5: Preview driver — live dancer and stage previews
