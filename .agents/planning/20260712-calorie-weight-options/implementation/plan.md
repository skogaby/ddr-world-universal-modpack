# Implementation Plan — In-Game Weight & "Display Burned Calories" Options

Spans **two repos that ship together**:
- **DLL** — `ddr-world-universal-modpack` (this repo)
- **Backend** — `bemani-buddy` (`~/Desktop/Projects/bemani-buddy`)

Read `../design/detailed-design.md` (design), `../idea-honing.md` (requirements),
and `docs/calorie_weight_profile_research.md` (RE facts) before starting. Steps are
incremental and each ends with integration — no orphaned code.

> **Progress tracking (per AGENTS.md):** maintain `../progress.md` in this feature
> directory throughout implementation — update it after each step and before any
> pause/handoff, with a `NEXT ACTION:` line and a deploy/test log.

## Checklist

- [ ] **Step 1 — Backend:** save-path detection for `mod_weight` / `mod_is_disp_weight` (+ schema entry, + test)
- [ ] **Step 2 — DLL:** `profile_fields` submodule — register parent/child rows + apply-on-change; wire into `webui_options::enable()`
- [ ] **Step 3 — DLL:** seed both rows from `PlayerWork` at SONG_SELECT (read-only, `0→60`)
- [ ] **Step 4 — End-to-end:** full round-trip validation (cabinet + backend) + docs (README / AGENTS.md)

---

## Step 1: Backend — persist `mod_weight` / `mod_is_disp_weight` on save

**Objective.** Make `bemani-buddy` store the two injected save fields into the
already-existing native `weight` / `is_disp_weight` profile columns, so the values
round-trip through the game's own `<common>` load. This is the independently
testable foundation of the round-trip.

**Guidance.**
- `models/ddr_world/playdata_3.json` — in the **save** `<option>` node (the one that
  already declares `mod_customize_*`, ~lines 375-384), add:
  ```json
  "mod_weight": "s32?",
  "mod_is_disp_weight": "s32?"
  ```
  Regenerate codegen if the workspace build requires it.
- `crates/game-server/src/handlers/ddr_world/playdata.rs` — in the save `<option>`
  applier, next to the `mod_customize_*` block (~lines 663-672), add:
  ```rust
  if let Some(v) = child_i32(option, "mod_weight")         { profile.weight = v; }
  if let Some(v) = child_i32(option, "mod_is_disp_weight") { profile.is_disp_weight = v != 0; }
  ```
  Keep the "only when present" (`if let Some`) policy — an un-hooked play or a
  web-UI edit between hooked sessions must not clobber the stored value.
- **No migration**, no `db` model / MySQL change, no load change — columns
  (migration 003), the `<common>` load emit (`playdata.rs:292/294`), and persistence
  already exist. Optionally clamp/sanitize `mod_weight` server-side, but the game
  already tolerates out-of-range weights, so a straight copy matches `mod_customize_*`.

**Tests.**
- Add a handler test following the crate's existing conventions (mirror any
  `mod_customize_*` coverage): a `playerdata_save` request carrying
  `mod_weight=68` / `mod_is_disp_weight=1` updates the profile row; a subsequent
  `playerdata_load` emits `<weight>68</weight>` and `<is_disp_weight>1</is_disp_weight>`
  in `<common>`. Add an absence case: a save without the fields leaves stored values
  unchanged. Reuse existing fixtures where possible
  (`crates/ddr-score-proxy/tests/fixtures/playerdata_load_response.xml`).
- `cargo build` + `cargo test` green in `bemani-buddy`.

**Integration.** Self-contained backend change; consumes the wire fields the DLL will
emit in later steps (framework-automatic — see Step 2).

**Demo.** Fire a crafted `playerdata_save` (test or packet replay) with the two
fields → DB row updates → a following `playerdata_load` echoes them in `<common>`.
A save without the fields leaves the row untouched.

---

## Step 2: DLL — `profile_fields` submodule: register rows + apply on change

**Objective.** Add the two option rows (parent OFF/ON toggle + conditional WEIGHT
child) to the in-game menu and write edits into `PlayerWork`. After this step the
rows are visible, the parent/child visibility works, editing changes game memory,
and — because they're `SaveOnly` — they are already emitted on `playerdata_save`
(consumed by Step 1's backend).

**Guidance.**
- New file `src/mods/webui_options/profile_fields.rs`; add `pub mod profile_fields;`
  to `src/mods/webui_options/mod.rs`.
- Constants: `WEIGHT_OFFSET = 0x24`, `IS_DISP_WEIGHT_OFFSET = 0x28`,
  `WEIGHT_MIN = 30`, `WEIGHT_MAX = 200`, `WEIGHT_DEFAULT_WHEN_UNSET = 60`, and the
  ids `"weight"` / `"is_disp_weight"`.
- Reuse the mod's player-work chain: a `player_work(side) -> Option<*mut u8>` helper
  identical to the pointer walk in `mod.rs::{seed_registry_from_game, try_apply_all}`
  (`player_work_table[side]` → `*wrapper` = PlayerWork), fully null-guarded. Read the
  resolved `player_work_table` from the same `SharedState` the mod already holds
  (no new signature — `player_work_table` is already a `required_signature`).
- `register()`:
  - **Parent first:** `RegisterSpec::bool_toggle("is_disp_weight").default_value(0)
    .on_change(on_is_disp_changed).persist_mode(PersistMode::SaveOnly)`.
  - **Child:** `RegisterSpec::scalar("weight", 30, 200, 1, ScalarFormat::Integer)
    .step_coarse(10).default_value(60).on_change(on_weight_changed)
    .show_when(ShowWhen::Equals { parent_id: "is_disp_weight".into(), value: 1 })
    .persist_mode(PersistMode::SaveOnly)`.
  - Log + skip on `register_option` error (do not abort cosmetics).
- `on_weight_changed(side, v)` writes `PlayerWork+0x24` as `i32`;
  `on_is_disp_changed(side, v)` writes `PlayerWork+0x28` as `u8` (`0`/`1`). Both
  null-guard the chain (side not carded in → no-op).
- Wire into `WebUiOptionsMod::enable()` after the existing
  `custom_options::is_available()` guard and the cosmetic registration loop:
  `profile_fields::register();`.

**Tests.**
- `cargo check --target x86_64-pc-windows-msvc` clean; `./build.sh` release build
  clean.
- Live deploy (`./scripts/deploy.sh`) + logs/DebugView:
  - Mods tab shows **DISPLAY BURNED CALORIES**; **WEIGHT** shows only when it's ON,
    hidden when OFF — per side (P1 & P2 independent).
  - Editing WEIGHT / toggling logs the write; a read-back of `PlayerWork+0x24`/`+0x28`
    confirms the value landed on the correct side.

**Integration.** Registered under the existing `webui-options` toggle; no new mod
entry. `SaveOnly` means `custom_options_persistence` now auto-emits
`<mod_weight>` / `<mod_is_disp_weight>` on save → Step 1 backend stores them.

**Demo.** In song-select options: toggle calories ON → WEIGHT row appears; set a
weight → next play's calorie display/scaling uses it; toggle OFF → WEIGHT row
disappears. (Values reset to defaults on a fresh session until Step 3.)

---

## Step 3: DLL — seed both rows from `PlayerWork` at SONG_SELECT

**Objective.** Make the rows reflect the player's actual stored values (server-loaded
via the game's native `<common>` load) on every song-select entry, so a carded-in
player sees their real weight / toggle rather than defaults.

**Guidance.**
- Add `pub fn seed(player_side: u8)` to `profile_fields.rs`: read `PlayerWork+0x24`
  (i32) and `+0x28` (u8) via the same chain; `set_value_silent("weight", side, w)`
  with `w = if raw == 0 { 60 } else { raw.clamp(30, 200) }`, and
  `set_value_silent("is_disp_weight", side, if raw_u8 != 0 {1} else {0})`. Read-only
  w.r.t. game memory; `set_value_silent` fires no `on_change` (no write-back loop).
- Hook into the **existing** SONG_SELECT (scene 25) callback in
  `WebUiOptionsMod::enable()` — the same closure that calls
  `seed_registry_from_game(0/1)` — adding `profile_fields::seed(0)` /
  `profile_fields::seed(1)`. One scene subscription for both concerns.

**Tests.**
- `cargo check` / `./build.sh` clean.
- Live deploy + logs:
  - Card in a profile with a known server weight → WEIGHT shows that value; toggle
    reflects the stored `is_disp_weight`.
  - A profile with `weight==0` → WEIGHT seeds to 60.
  - Edit a value, leave and re-enter song-select → the row re-seeds to the same
    (just-written) value (idempotent; edits persist in memory within the session).

**Integration.** Completes the in-game half of the loop: load (game-native) → seed
(read PlayerWork) → edit (write PlayerWork) → save (auto-emit) → Step 1 backend.

**Demo.** Card in → options show the profile's real weight + calorie toggle
(server-loaded), not defaults.

---

## Step 4: End-to-end round-trip validation + documentation

**Objective.** Verify the full cross-repo loop on a live cabinet against the updated
backend, and record the feature in user/agent docs. Ship both repos together.

**Guidance.**
- Deploy the DLL (Steps 2-3) and run the backend (Step 1) together.
- Confirm the packet path: card-out `playerdata_save` carries `mod_weight` /
  `mod_is_disp_weight` (packet log) → backend writes native columns → next card-in
  `playerdata_load` `<common>` carries the saved values → game reflect →
  `profile_fields::seed` shows them.
- Docs:
  - **README.md** — extend the **WebUI Options** entry to mention the new
    non-cosmetic rows (DISPLAY BURNED CALORIES + WEIGHT, kg, parent/child,
    network-save-only, backend round-trip).
  - **AGENTS.md** — add a "Key Entry Points" row pointing at
    `src/mods/webui_options/profile_fields.rs`, and note the two new
    `mod_weight`/`mod_is_disp_weight` save fields + their backend columns.
  - Cross-link `docs/calorie_weight_profile_research.md` from the README/AGENTS
    entry if useful.
- Perform the **one-off unit calibration** (non-blocking): set a known weight via the
  web UI, read `PlayerWork+0x24` (Cheat Engine) to confirm plain kg. If it proves
  scaled, adjust only the range constants / add a scale factor in
  `profile_fields.rs` (localized).

**Tests.**
- Final `./build.sh` (DLL) + `cargo test`/`cargo build` (backend) green.
- Live end-to-end on cabinet: the full round-trip and the calorie display reflect
  the settings on the next play, per side.
- Degradation check: with `custom_options` unavailable, the mod still enables
  (cosmetics only), no crash, rows simply absent.

**Integration.** Both repos coordinated; documentation reflects the shipped feature.

**Demo.** Full loop on a live cabinet: set weight + turn calories ON in-game →
card-out → card-in on a fresh session → the values persist and the in-game calorie
display tracks the set weight.
