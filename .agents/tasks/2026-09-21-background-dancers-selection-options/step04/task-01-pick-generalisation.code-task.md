# Task: `Pick` generalisation — optional stage, empty dancer list, `ParseOptions { shadow }`

## Description
Let a `Pick` describe a stage-only or a dancer-only scene (design §4.8) so the Step 5 preview driver can
reuse the gameplay load/parse/build machinery unchanged: `Pick.stage` becomes `Option<StageCandidate>`,
`Pick.dancers` may be empty, `Pick::arcs()` / `summary()` handle both, and `parse_pick` takes a
`ParseOptions { shadow: bool }` (gameplay `true`, previews `false`). Move the pick types + builders into a
dependency-free `pick.rs` so the host harness can test them. NO gameplay behaviour change: every existing
call site wraps its stage in `Some`, the per-song `pick.summary()` INFO keeps its existing tokens for a
`Some` stage, and `parse_pick(pick, ParseOptions { shadow: true })` produces the bundle it produces today.

## Background
`src/mods/background_dancers/session.rs` currently holds `Pick` / `make_pick` / `assemble_pick` (pure —
they only use `super::selection`) next to the engine-facing `Parsed` / `Session` / `build_one` code, so the
pure half cannot be mounted by `scripts/validate_background_dancers.sh` (the crate does not build on the
macOS ARM host — `retour`). `parse_pick` reads the stage arc unconditionally, the motion arcs per dancer,
and the `pl_shadow00` arc whenever any dancer has ground bones. A preview needs: stage-only (no dancers ⇒
no motion arcs, no shadow) and dancer-only (no stage arc, no camera sets, no shadow — FR-8 "no shadow").

Existing call sites of the pick builders: `lifecycle::window_entry` (three arms — pin, `option_pick`,
`make_pick`), `lifecycle::request_load` (`pick.arcs()`), `lifecycle::drive_live` (`pick.camera_main` /
`camera_non` in the camera-director INFO), `session::parse_pick`, `session::Session::new`
(`pick.seed`), `director` (none). `Pick::summary()` is the per-song INFO line a field log is read by —
the token shapes for a `Some` stage must not move.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md`
  (§4.6 "preview/scene.rs — pick construction", §4.8 `Session` / `Pick` changes, §7 host tests)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md` (Step 4)
- `src/mods/background_dancers/selection.rs` (`StageCandidate`, `DancerCandidate`, `playlist`,
  `camera_lists`, `dancer_x`, `PickSource`, the `#[cfg(test)] fixtures` module)
- `scripts/validate_background_dancers.sh` (how pure files mount: `super::selection` resolves because both
  files sit at the harness crate root)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New pure file `src/mods/background_dancers/pick.rs` (std + `super::selection` only — no `crate::`
   imports) holding `Pick`, `make_pick`, `assemble_pick` moved verbatim from `session.rs`, plus:
   - `pub struct ParseOptions { pub shadow: bool }` with `ParseOptions::GAMEPLAY` (`shadow: true`) and
     `ParseOptions::PREVIEW` (`shadow: false`) consts.
   - `Pick.stage: Option<StageCandidate>`.
   - `assemble_pick(rng, stage: StageCandidate, …)` keeps its signature and wraps the stage in `Some`;
     a new `assemble_pick_opt(rng, stage: Option<StageCandidate>, camera_rows, dancers, pinned,
     arc_exists) -> Pick` is the general form (`camera_row` empty and `camera_main/non` empty when the
     stage is `None`; `playlists`/`parts` empty when `dancers` is empty).
   - `Pick::arcs()` unchanged in output for a `Some` stage + non-empty dancers; a `None` stage omits the
     `mapset_*` arc; `Pick::arcs_for(&self, opts: &ParseOptions)` = `arcs()` minus the `pl_shadow00.arc`
     entry when `!opts.shadow` (gameplay passes `GAMEPLAY` ⇒ identical list).
   - `Pick::summary()`: for a `Some` stage the line is byte-identical to today's; for `None` the stage token
     reads `stage=none{<source>} parts=0`.
2. `session.rs`: `pub use super::pick::{assemble_pick, assemble_pick_opt, make_pick, ParseOptions,
   Pick};` so every existing import path (`use super::session::{assemble_pick, make_pick, …, Pick}`)
   keeps compiling. `parse_pick(pick: &Pick, opts: &ParseOptions) -> Parsed`:
   - stage `None` ⇒ skip the stage section entirely (no warning, `stage_parts = []`);
   - `dancers` empty ⇒ the motion-arc loop runs zero times (already), and the shadow block is skipped;
   - `opts.shadow == false` ⇒ `shadow = None` WITHOUT opening `pl_shadow00.arc` (no warning);
   - cameras: with a `None` stage push NO "has no camera set" warning and set `cameras = None`; with a
     `Some` stage the existing logic (incl. its warnings) is unchanged.
3. `lifecycle.rs`: `parse_pick(&pick_for_thread, &ParseOptions::GAMEPLAY)`; `pick.arcs()` may stay or
   become `arcs_for(&ParseOptions::GAMEPLAY)` (identical output). No other logic change.
4. `scripts/validate_background_dancers.sh`: mount `pick` (`src/mods/background_dancers/pick.rs`) in
   `MODULE_NAMES` / `MODULE_PATHS` with a one-line comment in the header list.
5. Host tests in `pick.rs` (`#[cfg(test)]`, using `super::selection::fixtures`): arc lists for
   stage+dancers (stage first, body, motion, parts, shadow), stage-only (no body/motion/shadow arcs),
   dancer-only (no `mapset_` arc; shadow present under `GAMEPLAY`, absent under `PREVIEW`); `summary()`
   contains `stage=none{random}` for a `None` stage and the existing `stage=<key>[<row>]{…}` shape for
   `Some`; `assemble_pick` output equals `assemble_pick_opt(Some(stage))` under the same seed.

## Dependencies
- Steps 1–3 as shipped (uncommitted on `v1_4`). No new signatures, no engine code.

## Implementation Approach
1. Create `pick.rs` by moving the `Pick` section of `session.rs` (lines "Pick" through `assemble_pick`);
   add `ParseOptions`, `assemble_pick_opt`, `arcs_for`; write the tests first against the fixtures.
2. Make `Pick.stage` optional; fix `summary()` / `arcs()`; re-export from `session.rs`.
3. Thread `ParseOptions` through `parse_pick`; branch on `pick.stage` / `pick.dancers.is_empty()` /
   `opts.shadow`.
4. `cargo check --target x86_64-pc-windows-msvc`, `./scripts/validate_background_dancers.sh` (after
   mounting `pick`), `cargo fmt` (whole crate).

## Acceptance Criteria

1. **Gameplay pick unchanged**
   - Given a stage candidate + two dancer candidates and a seed
   - When `assemble_pick(rng, stage, rows, dancers, false, exists)` runs
   - Then `pick.stage == Some(stage)`, `pick.arcs()` lists `data/arc/mapset_<key>.arc` first and
     `data/arc/pl_shadow00.arc` last, `arcs_for(&GAMEPLAY) == arcs()`, and `summary()` starts with
     `stage=<key>[<row>]{random} parts=<n> dancers=[`.

2. **Stage-only pick**
   - Given `assemble_pick_opt(rng, Some(stage), rows, vec![], false, exists)`
   - When `arcs()` / `summary()` are read
   - Then the arc list is exactly `[data/arc/mapset_<key>.arc]`, `playlists`/`parts` are empty and the
     summary reads `dancers=[] clips=[] wear=[]`.

3. **Dancer-only pick**
   - Given `assemble_pick_opt(rng, None, rows, vec![dancer], false, exists)`
   - When `arcs()` and `arcs_for(&PREVIEW)` are read
   - Then neither contains a `mapset_` arc, `arcs()` ends with `data/arc/pl_shadow00.arc`,
     `arcs_for(&PREVIEW)` does not contain it, `camera_main`/`camera_non` are empty and the summary contains
     `stage=none{random} parts=0`.

4. **`parse_pick` honours the options**
   - Given a dancer-only pick and `ParseOptions::PREVIEW`
   - When `parse_pick` runs (cabinet / reading: by inspection — engine-facing, no host harness)
   - Then no `mapset_`, no `pl_shadow00` arc is opened, `stage_parts` is empty, `shadow` is `None`,
     `cameras` is `None`, and the warnings carry no "has no camera set" / "no shadows" entries.

5. **Harness**
   - Given `scripts/validate_background_dancers.sh`
   - When it runs
   - Then `pick` is mounted and every test (existing 127 + the new ones) passes.

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, refactor, pure-layer, host-tested
- **Required Skills**: Rust, the repo's pure-module harness convention
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 4: `Pick`/`Session` generalisation + `scene_window.rs` extraction
