# Idea Honing — Requirements Clarification

Interactive Q&A refining the rough idea into a concrete specification. One question
at a time; the chosen answer (plus notable alternatives) is recorded under each.

---

## Q1: Where should this feature live — inside the existing `webui-options` mod, or as a separate mod?

The two new values are **profile/PlayerWork-header** fields, not cosmetic
`Customize` fields. Unlike the existing WebUI Options categories they:

- do **not** use `discovery.rs` (no filesystem asset scan),
- do **not** use `customize_field_offset` / the `Customize` object,
- have **no preview art** (they'd render as a plain Scalar row + an OFF/ON Enum row).

They *do* share the same idea ("things only the web UI can normally set"), the same
`custom_options` registration + `SaveOnly` persistence, and the same
seed-at-SONG_SELECT pattern.

Options:
- **(a)** Inside `webui-options` as a new self-contained submodule (e.g.
  `webui_options/profile_fields.rs`) registered alongside the cosmetics under the
  same `webui-options` mod toggle. Pro: one user-facing toggle, shares the mod's
  init/scene plumbing. Con: stretches "WebUI Options" beyond the Customize object.
- **(b)** A separate small mod (e.g. `workout-options` / `calorie-options`) with its
  own toggle in `mod-config.json`. Pro: clean separation (different memory region,
  no previews/discovery). Con: another mod entry; duplicates the player-work chain +
  scene-seed plumbing.

**Answer:** **(a)** — implement inside the existing `webui-options` mod as a
self-contained submodule, registered under the same `webui-options` toggle,
reusing the mod's player-work chain, SONG_SELECT seed callback, and save-injection
path. Keeps all "web-UI-only settings" under one user-facing toggle.

---

## Q2: How should the WEIGHT row behave — UI kind, range, step, and unit/unset handling?

`weight` is an `s32` at `PlayerWork+0x24`. Open question from RE: the stored unit
isn't 100% pinned (kg vs a scaled value) and the calc's *unset* branch (`weight==0`)
behaves anomalously. Sub-decisions:

- **UI kind**: `Scalar` (integer, no preview) — assumed. (An Enum of preset weights
  is the alternative but seems worse.)
- **Range + step**: e.g. `30..=150` kg, fine step 1, coarse step 10? Or wider?
- **Unit source of truth**: hardcode a kg range now, or make the range/label
  **config-driven** (`mod-config.json`) so it can be tuned once the unit is
  confirmed at runtime without a rebuild?
- **Unset / `0` handling**: if the game returns `weight==0` (never set), what does
  the row show — clamp the *displayed* value to the range minimum, show `0`, or
  seed to a sensible default (e.g. 60)? (Note: writing any value >0 makes the calc
  use `weight/100`; leaving `0` triggers the anomalous 60.0 branch.)

Suggested default: `Scalar`, range `30..=150`, fine 1 / coarse 10, **config-driven
range** (so the unit can be calibrated later), and on seed treat `0` as "show the
range minimum but don't write until the user commits a value."

**Answer:** *(revised — dropped LB in favour of KG to avoid unit conversion)*

- **UI kind:** `Scalar` (integer row, no preview).
- **Unit:** **kilograms (KG)** — the same unit the game stores and the web UI
  uses, so **no unit conversion anywhere**. The value read from `PlayerWork+0x24`,
  the value written back, and the value sent to the backend are all the same
  integer kg.
- **Range/step:** kg range TBD (the earlier `25..=500` was lb-specific) — see Q4.
- **Unset handling:** if the game returns `weight == 0`, **seed the row to a
  default of 60** (kg).
- **Parent/child schema:** the calorie-display toggle is the **parent** row; the
  WEIGHT row is its **child**, shown **only when the toggle is ON**. When calories
  are OFF, the weight row is hidden entirely. *(See Q3/Q4.)*

**Caveats carried into design (not blockers):**
1. **Unit assumption.** Proceeding on "profile weight is plain kg." The RE
   unset-branch anomaly (`F=60.0`) still warrants a one-off runtime calibration
   (Cheat Engine) before shipping; does not block design.
2. **Conditional child visibility** depends on `custom_options`/mod-menu framework
   support — feasibility tracked in Q4.

---

## Q3: The DISPLAY BURNED CALORIES toggle (parent row) — labels, default, and when edits take effect

`is_disp_weight` is a `u8`/bool at `PlayerWork+0x28`.

**Answer:**
- **Kind/labels:** `Enum` OFF/ON, **modeled the same as other boolean options**
  (stock `seop_op_off` / `seop_op_on` ribbons).
- **Default:** when the profile reads `0`, the parent shows **OFF**; the child
  WEIGHT row stays hidden until the player turns it ON.
- **Edits take effect:** written to `PlayerWork+0x24`/`+0x28` on the song-select
  edit; takes effect on the **next play** and persists via save → backend → next
  card-in. No live mid-session toggle required.

---

## Q4: Concrete KG range/step for the WEIGHT row, and should the range be config-driven?

Now that the row is KG (the earlier `25..=500` was lb):

- **Range:** proposed **30..=200 kg** (covers essentially all players; game default
  is 60). Wider/narrower?
- **Step:** fine **1 kg**, coarse **10 kg**?
- **Config-driven?** With the unit now fixed as kg, a config-tunable range is less
  necessary than when calibration was open. Hardcode `30..=200` as constants
  (simplest), or expose `custom_options.weight_min` / `weight_max` in
  `mod-config.json` for operator tuning?

Suggested default: hardcoded **30..=200 kg**, fine 1 / coarse 10, not config-driven
(keep it simple; revisit if an operator asks).

**Answer:** Accepted the suggestion — WEIGHT row is `Scalar` **30..=200 kg**, fine
step **1 kg**, coarse step **10 kg**, **hardcoded** (not config-driven).

---

## Q5: Save injection — field names, wire types, and the backend contract

These are `PersistMode::SaveOnly` (network-save-only, like the WebUI cosmetics): the
DLL emits them on `playerdata_save`; the backend stores them; they come back through
the game's native `<common>` load on the next card-in; the menu re-seeds from
`PlayerWork`. No network load, no JSON cache.

The existing cosmetics append `mod_customize_*` **s32** children under the save
packet's `<option>` node (via `custom_options_persistence`'s `save_sender`). Proposed
mirror:

| Injected field (`<option>` child) | wire type | value | backend → native column |
|-----------------------------------|:---------:|-------|-------------------------|
| `mod_weight`          | s32 | kg integer (30..200) | `common.weight` (s32) |
| `mod_is_disp_weight`  | s32 | 0/1                  | `common.is_disp_weight` (bool) |

Notes:
- `is_disp_weight` is a **bool** in the native `<common>` load, but the DLL's save
  path emits **s32**, so it's sent as `0`/`1` and the backend casts to bool for its
  column.
- On `playerdata_load` the backend emits the native `<common><weight>` /
  `<is_disp_weight>` — nothing mod-specific — and the game's own reflect applies
  them; the DLL only seeds its menu from `PlayerWork`.

Questions:
- **Field names OK** as `mod_weight` / `mod_is_disp_weight` (consistent with the
  `mod_*` convention), or do you want different names the backend will key on?
- Confirm **`SaveOnly` / network-save-only** semantics (no JSON offline cache, no
  network load) — same as the cosmetics.

**Answer:** Confirmed — field names `mod_weight` / `mod_is_disp_weight` (s32
children under `<option>`), `PersistMode::SaveOnly` / network-save-only (no offline
JSON cache, no network load), matching the WebUI cosmetics.

---

## Q6: Parent-child conditional visibility — feasibility (from Q2/Q3)

**Resolved — no framework work needed.** The `custom_options` framework already
supports conditional row visibility via `ShowWhen::Equals { parent_id, value }`,
with a working precedent in `src/mods/power_user_statistics/mod.rs`:

```rust
let specs = [
    RegisterSpec::bool_toggle("pacemaker_to_mserror"),               // parent
    RegisterSpec::scalar("pacemaker_threshold", 1, 50, 1, ScalarFormat::Integer)
        .default_value(10)
        .show_when(ShowWhen::Equals { parent_id: "pacemaker_to_mserror".into(), value: 1 }),
];
```

So the feature registers:
1. **Parent** — `is_disp_weight` via `RegisterSpec::bool_toggle(...)` (OFF/ON).
2. **Child** — `weight` via `RegisterSpec::scalar("weight", 30, 200, 1, Integer)`
   `.default_value(60)`
   `.show_when(ShowWhen::Equals { parent_id: "is_disp_weight", value: 1 })`.

Constraint: the **parent must be registered before the child** (the framework
validates the reference synchronously, else `RegisterError::UnknownParent`).

**Answer:** Feasible as-is; adopt the `power_user_statistics` parent/child pattern.

---

## Q7: Offset resolution — hardcode `+0x24`/`+0x28`, or derive via signature?

The PlayerWork-header offsets are verified byte-identical on 20260324 and 20260616
(`docs/calorie_weight_profile_research.md` §7). Options:

- **(a)** Hardcode `const WEIGHT_OFFSET: usize = 0x24;` /
  `const IS_DISP_WEIGHT_OFFSET: usize = 0x28;`. **Consistent with how the mod
  already treats `customize_field_offset`** (hardcoded internal offsets) while the
  version-variable `player_work_table` base is derived at runtime. Simplest.
- **(b)** Add a signature on the calorie calc `FUN_180053430`
  (`66 0F 6E 42 24` → `MOVD XMM0,[RDX+0x24]`) and decode the `+0x24` displacement
  at runtime; `is_disp = weight + 4`. Update-resilient if the header ever shifts,
  but more machinery for offsets that have been stable.

Suggested default: **(a) hardcode**, matching existing `customize_field_offset`
practice, given the confirmed cross-version stability. (Add the signature later if
a future build moves the header.)

**Answer:** **(a)** — hardcode `const WEIGHT_OFFSET = 0x24` /
`const IS_DISP_WEIGHT_OFFSET = 0x28`; keep `player_work_table` runtime-derived as
today. Revisit with a signature only if a future build moves the header.

---

## Q8: Per-player (P1/P2) scope, and how we'll validate

**Scope.** `weight` / `is_disp_weight` are per-player profile fields, and
`custom_options` is inherently per-side (`on_change(player_side, …)`,
`get_value(side, id)`). So both rows are **per-player**: seeded independently for
side 0 and side 1 at SONG_SELECT, applied per side on change, written to the
correct side's `PlayerWork` — mirroring `webui_options::{seed_registry_from_game,
try_apply_all}` which already loop sides 0 and 1. Assumed correct.

**Validation** (this repo's only real gate is live cabinet deploy + log
observation, per AGENTS.md). Proposed success criteria:
1. On the Mods tab (or wherever the WebUI options render), the **DISPLAY BURNED
   CALORIES** toggle appears; the **WEIGHT** row appears **only when it's ON** and
   vanishes when OFF (per side).
2. Editing WEIGHT writes the value to `PlayerWork+0x24`; toggling calories writes
   `PlayerWork+0x28` (verify via log / read-back).
3. At SONG_SELECT the rows **seed** from the game's `PlayerWork` (server-loaded
   value shows; `weight==0` seeds to 60).
4. On card-out, `mod_weight` / `mod_is_disp_weight` are injected into
   `playerdata_save`; backend stores them; on next card-in the native `<common>`
   load applies them and the menu re-seeds to the saved values.
5. In-game **calorie display** reflects the toggle, and burned-calorie numbers
   track the set weight, on the **next play**.
6. **One-off unit calibration** before shipping: set a known weight via the web
   UI, read `PlayerWork+0x24` (Cheat Engine) to confirm it's plain kg (settles the
   RE anomaly). Non-blocking for implementation.

**Answer:** Per-player scope confirmed (seed/apply/save independently for P1 & P2,
mirroring `webui_options`). Success criteria accepted as listed. Item 6 (unit
calibration) is a separate manual check, not part of the code deliverable.

---

## Q9: Backend (bemani-buddy) scope — same-execution deliverable

**Requirement:** the DLL change and the **bemani-buddy backend** change ship in the
**same execution** (one coordinated deliverable across both repos). Backend repo:
`~/Desktop/Projects/bemani-buddy` (Rust; see `research/backend-bemani-buddy.md`).

**Discovered change surface (minimal — the columns & load already exist):**

1. **No DB migration.** `ddr_world_profiles` already has `weight INT NOT NULL
   DEFAULT 0` and `is_disp_weight BOOLEAN NOT NULL DEFAULT FALSE`
   (`migrations/003_ddr_world_profiles.sql`). *(Lesson from migration 009→010: do
   NOT add redundant `opt_mod_*` columns — write straight to the native columns.)*
2. **Load: already done.** `handlers/ddr_world/playdata.rs:292/294` already emits
   `weight` and `is_disp_weight` from the profile in the `<common>` block; the game
   applies them on card-in. No change.
3. **Save: add detection** in the `<option>` block of the save handler
   (`playdata.rs`, alongside the `mod_customize_*` lines ~663-672), mirroring the
   established "only when present" pattern:
   ```rust
   if let Some(v) = child_i32(option, "mod_weight")         { profile.weight = v; }
   if let Some(v) = child_i32(option, "mod_is_disp_weight") { profile.is_disp_weight = v != 0; }
   ```
   (`mod_is_disp_weight` arrives as s32 `0`/`1`; cast to bool.)
4. **Save-request schema**: declare `mod_weight` / `mod_is_disp_weight` as `s32?`
   in the save `<option>` node of `models/ddr_world/playdata_3.json` (mirroring the
   `mod_customize_*` entries at ~375-384) and regenerate codegen if required.
5. **Persistence layer**: `db` model + MySQL UPDATE already persist
   `weight`/`is_disp_weight` — no change.

**Round-trip (end state):** DLL sends `mod_weight`/`mod_is_disp_weight` on save →
handler writes native `weight`/`is_disp_weight` columns → next card-in the existing
load emits them in `<common>` → game reflect applies to PlayerWork → DLL menu
re-seeds. Fully closed loop; matches the cosmetic `mod_customize_*` design.

**Answer:** Backend is **in scope**, shipped together. Change surface confirmed as
above (save-handler detection + save-request schema entry; no migration, no
load/model/persistence changes). Both repos covered by one implementation plan.

---

## Requirements status

All questions resolved (Q1–Q9). Ready to proceed to **detailed design** covering
**both** the DLL (`webui-options` submodule) and the bemani-buddy backend
(save-handler detection + schema entry), so the two ship as one coordinated change.
