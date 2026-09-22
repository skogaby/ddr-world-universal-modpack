# Task: Option rows registration, persistence, mirror, and gameplay application

## Description
Wire the two rows into the running mod: `src/mods/background_dancers/options.rs` (engine-facing —
registration through `custom_options`, per-side value atomics, the `versus_mirror` tail on the
stage row, the `DynamicLabelFn`, the `*_choice` readers, availability flips), a
`lifecycle::tables_snapshot()` accessor, the enable/disable wiring, and the `window_entry` branch
that honours the rows at song-window entry (`selection::resolve_choice` → `assemble_pick`) with
the per-element source (`random|option|pin`) in the per-song `pick.summary()` INFO.

## Background
Registration follows the multiplayer_bot precedent (`src/mods/multiplayer_bot/mod.rs::register_rows`):
`RegisterSpec::scalar(..)` with `PersistMode::Local`, `.persist_transform(identity, clamp)`,
`Duplicate` ⇒ re-arm via `set_option_available(id, true)` + re-seed the atomics from
`get_value`. Mirroring follows premium_free / song_playback_speed: `versus_mirror::register(&[id])`
at enable, `mirror_edit(id, side, value)` at the `on_change` tail, `unregister` at disable. The
rows are `.in_game_only()` (their previews are the point; FR-7). Registration must happen AFTER
`lifecycle::init_tables()` (the catalog comes from the tables) and the base chrome must exist on
disk BEFORE `register_option` (`preview_gen::generate_chrome(option_id)` — widen it to
`pub(crate)`; the webui_options precedent at `src/mods/webui_options/mod.rs:186`). A live enable
registers the rows but their textures appear next launch (framework-wide one-shot atlas flush —
expected; log one INFO, do not "fix").

Gameplay application (design §4.3, FR-6): in `lifecycle::window_entry`, after the `DDR_DANCERS_PIN`
branch and before `make_pick`, build `stage_key = options::stage_choice()` and, for entered side
*i* (in entered order — dancer index *i* = the *i*-th entered side), `options::dancer_choice(side)`;
if any is `Some`, call `resolve_choice`; an unknown key (catalog drift) ⇒ one WARN naming the key
and that element falls back to RANDOM (call again with that key cleared). Then
`assemble_pick(rng, stage, camera_rows, dancers, pinned, arc_exists)` as today. The song seed
stays random. The `PLAYER work` entered flags come from `stage_records::side_entered(side)`.

`Pick` gains per-element provenance for the log: extend `Pick` with `pub source_stage: PickSource`
and `pub source_dancers: Vec<PickSource>` (`enum PickSource { Random, Option, Pin }`) — or an
equivalent minimal shape — and render them in `summary()` as `stage=boom00[0]{option}` /
`emi01(F) x=+0.0{option}`; the existing ` (PINNED)` suffix stays when `pinned`.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md` (§4.2, §4.3, §6)

**Additional References (if relevant to this task):**
- `src/mods/multiplayer_bot/mod.rs` (`PersistMode::Local` scalar row precedent, `Duplicate` handling)
- `src/services/versus_mirror.rs` (`register` / `mirror_edit` / `unregister`)
- `src/mods/background_dancers/lifecycle.rs` (`Tables`, `init_tables`, `window_entry`, the pin branch)
- `src/mods/background_dancers/session.rs` (`Pick`, `assemble_pick`, `summary`)
- `src/mods/webui_options/preview_gen.rs::generate_chrome`
- `.agents/planning/2026-09-21-background-dancers-selection-options/research/orientation.md` §1.2

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `options.rs`: `pub const OPT_DANCER: &str = "background_dancer"`, `OPT_STAGE = "background_stage"`;
   `static CATALOG: OnceLock<Catalog>`; `static DANCER_VALUE: [AtomicI32; 2]`,
   `STAGE_VALUE: [AtomicI32; 2]`; `pub fn register(catalog: Catalog) -> bool`;
   `pub fn set_available(available: bool)`; `pub fn stage_choice() -> Option<String>`
   (`None` = RANDOM / rows unavailable; reads P1's value — the mirror keeps both equal while
   engaged, and outside versus the entered side's value is read: take `side` as a parameter,
   `stage_choice(side)`); `pub fn dancer_choice(side: u8) -> Option<String>`; `fn label(id, value)
   -> Option<String>` (the `DynamicLabelFn`; value 0 ⇒ `"RANDOM"`, out of range ⇒ `None`);
   `on_dancer_change` / `on_stage_change` (store atomics; stage calls `versus_mirror::mirror_edit`);
   `fn clamp_load(id, value)` load transform via `catalog::clamp_to_catalog` against the stored
   catalog (unknown catalog ⇒ RANDOM).
2. `RegisterSpec::scalar(id, 0, n, 1, ScalarFormat::Dynamic(label)).step_coarse(5)
   .default_value(RANDOM).persist_mode(PersistMode::Local).persist_transform(identity, clamp_load)
   .in_game_only().display_name("Background Dancer" / "Background Stage")
   .description("…").on_change(..)`; gated on `custom_options::row_injection_available()`
   (WARN + skip otherwise). Both descriptions present (the `validate_custom_options.sh` display-string lint).
3. `register` calls `preview_gen::generate_chrome(id)` for both ids first; `Duplicate` ⇒ success
   path (re-seed atomics from `get_value`, `set_option_available(id, true)`); other errors ⇒ WARN,
   return `false`. Logs one INFO naming both rows and the catalog sizes; one INFO noting the
   next-launch texture caveat when the registration happened after boot (`lib.rs`'s atlas flush —
   detect via a `custom_options`-side flag if one exists, else log unconditionally as INFO).
4. `lifecycle.rs`: `pub(super) fn tables_snapshot() -> Option<(Vec<StageCandidate>,
   Vec<(String, Vec<String>)>, Vec<DancerCandidate>)>`; `window_entry` gains the choice branch;
   `mod.rs::enable()` calls `options::register(catalog::build_catalog(..))` after the tables are
   ready and `versus_mirror::register(&[OPT_STAGE])`; `disable()` calls
   `versus_mirror::unregister(&[OPT_STAGE])` + `options::set_available(false)`.
5. `session.rs`: `Pick` provenance fields + `summary()` rendering; `make_pick`/`assemble_pick`
   fill them (`assemble_pick` takes the sources or the caller sets them after — keep the gameplay
   log line's existing tokens intact, only ADD the `{source}` suffixes).
6. `cargo check --target x86_64-pc-windows-msvc` clean; `./scripts/validate_background_dancers.sh`
   and `./scripts/validate_custom_options.sh` green; `cargo fmt`.

## Dependencies
- task-01 (`ScalarFormat::Dynamic`), task-02 (`catalog.rs`, `resolve_choice`).

## Implementation Approach
1. Host-testable bits first (tests in `session.rs`: `summary()` renders the `{option}` /
   `{pin}` / `{random}` suffixes; a `Pick` built by `assemble_pick` defaults to `Random`).
2. `options.rs` with the registration + readers; `mod.rs` wiring; `lifecycle` accessor + branch.
3. `generate_chrome` → `pub(crate)`.
4. Gates.

## Acceptance Criteria

1. **Registration**
   - Given the mod enables with tables ready and `row_injection_available()`
   - When `options::register` runs
   - Then both rows are registered (`custom_options: registered "background_dancer"` /
     `"background_stage"` INFO), max = catalog count, default RANDOM, `PersistMode::Local`,
     in-game only, and `versus_mirror` holds `background_stage`

2. **Re-enable**
   - Given the rows were registered earlier this boot and the mod is toggled off then on
   - When `register` runs again
   - Then `Duplicate` is treated as success: atomics re-seeded from the registry, rows re-shown

3. **Label function**
   - Given the stored catalog
   - When `label("background_dancer", 0)` / `(…, 2)` / `(…, 999)` are called
   - Then they return `Some("RANDOM")`, `Some(catalog.dancers[1].label)`, `None`

4. **Choice application**
   - Given P1 entered with `background_dancer = k` and `background_stage = m` (both ≠ 0)
   - When a song window opens
   - Then the pick's stage key is `stages[m−1].key` (row uniform over that key), dancer 0 is
     `dancers[k−1].key`, and the summary INFO shows `{option}` on both elements; RANDOM values
     leave the existing random path and `{random}` suffixes

5. **Pin precedence**
   - Given developer mode with `DDR_DANCERS_PIN` set and non-RANDOM row values
   - When a song window opens
   - Then the pin wins (unchanged behaviour) and the summary shows `{pin}` + ` (PINNED)`

6. **Disable**
   - Given the mod disables
   - When `disable()` runs
   - Then both rows are unavailable and the stage row is no longer mirrored

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, custom_options, versus_mirror, lifecycle
- **Required Skills**: Rust, the custom_options framework, the mod lifecycle
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 1: Option rows end-to-end
