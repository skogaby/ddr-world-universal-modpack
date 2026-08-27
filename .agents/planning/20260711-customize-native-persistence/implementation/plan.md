# Implementation Plan — Native Customize Persistence

Design: `../design/detailed-design.md`. Requirements/decisions:
`../idea-honing.md`. Wire mapping: repo `docs/player_customization_system_research.md`.

This change is a **coordinated flip** across two repos that deploy together
(no backward-compatibility layer — D6). Each step below is per-repo
compile-complete and independently reviewable; the true end-to-end cabinet
verification is the final integration step, since a coordinated flip cannot be
end-to-end from a single-repo increment.

> **Pre-existing uncommitted work to reconcile:** bemani-buddy
> `crates/game-server/src/handlers/ddr_world/playdata.rs` already has an interim
> save write-through that sets BOTH `opt_mod_customize_*` and `cust_*`. Step 3
> supersedes it (final form writes only the renamed native columns). The DLL
> repo's `docs/player_customization_system_research.md` already carries the
> corrected mapping + a "Server-Side Persistence Mapping" section written for
> the OLD dual-channel design; Step 4 revises that section to the single-source
> design.

## Checklist

- [x] **Step 1 — DLL:** `PersistMode` enum + silent setter + persistence gates (framework, no behavior change)
- [x] **Step 2 — DLL:** WebUI Options → `SaveOnly` + scene-25 read-only seed (remove scene-20 apply)
- [x] **Step 3 — Server:** consolidation migration + model/query/protocol/handler updates + sqlx cache regen
- [x] **Step 4 — Docs:** revise DLL research doc to single-source; update README / AGENTS.md / summary components
- [x] **Step 5 — Integration:** deploy both, cabinet round-trip verify (1P/2P) + log checks

---

## Step 1 — DLL: `PersistMode` framework + silent setter

**Objective.** Give the custom-options framework the capability the WebUI flip
needs, with zero behavior change for existing options (all remain `Full`).

**Guidance.**
- `services/custom_options/api.rs`: add `pub enum PersistMode { Full, SaveOnly,
  None }`. Replace `RegisterSpec.persist: bool` with `persist: PersistMode`;
  builders (`bool_toggle`, `enum_values`, `scalar`) default to `Full`; add
  builder setter `persist_mode(PersistMode)`. Map any existing `persist:false`
  usage to `None`.
- `services/custom_options/registry.rs`: mirror the field type on the stored
  option (`persist: PersistMode`); carry it from the spec.
- `services/custom_options/mod.rs`:
  - `snapshot_for_save()` — filter `mode != None` (emits `Full` + `SaveOnly`).
  - `resolve_from_load()` — early-return when the option's `mode != Full`
    (single gate for network-load + JSON-prime).
  - add `pub fn set_value_silent(option_id, player_side, value)` — mutate the
    registry value and **discard** the callback tuple (no `dispatch_callback`).
  - add predicate `json_persisted(id) -> bool` (`mode == Full`).
- `services/custom_options_persistence.rs`: `write_json_cache` filters entries
  to `custom_options::json_persisted(id)`.

**Tests / validation.** `cargo check --target x86_64-pc-windows-msvc` clean.
Deploy; confirm existing `Full` options (autoplay, premium-free, PUS, timing
offsets) still round-trip network + JSON exactly as before (all are `Full`, so
the new gates are no-ops for them).

**Integration.** Pure capability addition consumed by Step 2. No option uses
`SaveOnly` yet.

**Demo.** The framework compiles with `PersistMode`; a log/behavior check shows
existing options unchanged. `set_value_silent` and `SaveOnly` gating exist but
are dormant until Step 2.

---

## Step 2 — DLL: WebUI Options → `SaveOnly` + scene-25 read-only seed

**Objective.** Flip the WebUI options to network-save-only and source their
menu state by reading the game's own `Customize` object at SONG_SELECT entry,
removing the scene-20 memory-overwrite (the second-writer contention).

**Guidance.** In `mods/webui_options/mod.rs`:
- Registration: add `.persist_mode(PersistMode::SaveOnly)` to each spec; keep
  `save_transform` (index → asset id); drop `load_transform` (no framework
  caller remains — the seed does its own reverse lookup). Keep
  `on_change(on_value_changed)`.
- Scene callback: register on **scene 25** (`scenes::scene::SONG_SELECT`) and
  call `seed_registry_from_game(0)` / `(1)`. **Remove** the scene-20
  `try_apply_all` calls.
- New `seed_registry_from_game(side)`: walk `player_work_table[side] → wrapper
  → PlayerWork + customize_offset` (same chain as `try_apply_all`); per category
  read the `Customize` field at `customize_field_offset` as `u32` asset id,
  reverse-map to a menu index (`asset_ids.iter().position(|&a| a == id)
  .unwrap_or(0)`), and call `custom_options::set_value_silent(option_id, side,
  index)`. Null-guard table/wrapper/player_work (skip an un-carded side).
  Panic-free (bounds-checked reads, `unwrap_or`).
- `try_apply_all`: unchanged; now invoked only from `on_value_changed`.

**Tests / validation.** `cargo check`. Deploy against a server whose native
columns hold real values (see Step 3 note); confirm via log: a seed line per
carded side at scene-25 entry, no scene-20 apply, save still emits
`mod_customize_*`, and no `resolve_from_load` for customize ids. (Full cabinet
round-trip is verified in Step 5, once the server half lands.)

**Integration.** Consumes Step 1's `set_value_silent` + `SaveOnly`. The DLL is
now native-load-only for customize; the only `Customize` writer is a user edit.

**Demo.** In-game: the options modal shows the current server-loaded selections
(seed), a change applies immediately (on-change apply), and no JSON/network
load of customize occurs.

---

## Step 3 — Server (bemani-buddy): consolidation

**Objective.** Make the renamed native `cust_*` columns the sole customize
store: ingest `mod_customize_*` on save into them, emit them in `<customize>`
on load, drop the `opt_mod_customize_*` columns + the `<option>` load echo,
rename the opaque columns, and drop the inert `cust_3_0`.

**Guidance.**
- **Migration** `migrations/010_ddr_world_customize_consolidation.sql`:
  `RENAME COLUMN` the 10 `cust_<cat>_<pat>` → semantic names (see design Data
  Models; preserves data + defaults, incl. `cust_character_p2 DEFAULT 2`);
  `DROP COLUMN cust_3_0`; `DROP COLUMN` the 10 `opt_mod_customize_*`. Keep
  `opt_mod_autoplay`.
- **Model** `crates/db/src/models/ddr_world/profile.rs`: rename the 10 `cust_`
  fields, remove `cust_3_0`, remove the 10 `opt_mod_customize_*` fields (keep
  `opt_mod_autoplay`).
- **DAO** `crates/db/src/mysql/ddr_world/profile.rs`: update the `row_to_profile!`
  macro (renames, drop `cust_3_0` + `opt_mod_customize_*`), and the `update()`
  SQL column list + bind list (same). `create()` needs no change.
- **Protocol model** `models/ddr_world/playdata_3.json`: remove the 10
  `mod_customize_*` from `outputShapes/option` (the load echo). **Keep** them in
  `inputShapes/data/option` (save; the JSON doubles as protocol docs — Q5) and
  keep `mod_autoplay` in both. Re-run codegen:
  `cargo run -p codegen -- models/ddr_world/ crates/bemani-protocol/src/ddr_world/`.
- **Handler** `crates/game-server/src/handlers/ddr_world/playdata.rs`:
  - Save write-through (supersede the interim edit): when a `mod_customize_*`
    child is present, write it **only** into the matching renamed `cust_*` field
    (no `opt_mod_customize_*`). Keep the "only when present" guard.
  - Load builder: remove the `mod_customize_*` echo assignments from the
    `PlayerdataLoadOption` construction (fields no longer exist post-codegen);
    keep `mod_autoplay`.
  - `CUSTOMIZE_KEYS`: drop the `(3, 0)` entry. `build_customize_nodes`: rebuild
    the `values` array from the renamed fields (drop `cust_3_0`).
- **sqlx cache**: with local MySQL (`config.toml` →
  `mysql://bemani:bemani_dev@localhost/bemani_buddy`):
  `sqlx migrate run --source migrations/` then
  `cargo sqlx prepare --workspace`; commit `.sqlx/`.

**Tests / validation.** `SQLX_OFFLINE=true cargo check --workspace` clean.
`grep -rn 'cust_[0-9]\|opt_mod_customize' crates/ migrations/` returns only the
new migration's rename/drop clauses. DB round-trip: a hooked save populates the
renamed native columns; the next load emits them in `<customize>`; the
`<option>` block no longer contains `mod_customize_*`.

**Integration.** Completes the server half of the flip. With Step 2, the native
path now carries customize end-to-end.

**Demo.** Server-side: save a `mod_customize_*` payload → row's `cust_*` column
updates → load response's `<customize>` reflects it, `<option>` has no
`mod_customize_*`.

---

## Step 4 — Docs

**Objective.** Bring the durable docs in line with the single-source design.

**Guidance.**
- Repo `docs/player_customization_system_research.md`: revise the "Persistence"
  and "Server-Side Persistence Mapping" sections from the old dual-channel /
  keep-`opt_`-columns guidance to the single-source model (native columns
  authoritative; DLL sends on save, seeds from the `Customize` object on load;
  no echo; `opt_mod_customize_*` retired). Keep the `(category, pattern) →
  field` mapping table (still authoritative) and the renamed-column reference.
- `README.md`, `AGENTS.md`, `.agents/summary/components.md`: update the WebUI
  Options / `custom_options_persistence` descriptions that say customize values
  "persist via both network and local JSON" to reflect network-save-only +
  game-native load + scene-25 seed. Note `PersistMode` in the custom-options
  framework description.

**Tests / validation.** Prose review; ensure no doc still claims the DLL
network-loads or JSON-persists customize values.

**Integration.** Documentation only.

**Demo.** A reader of the research doc + AGENTS.md sees the current design and
the server contract other operators must implement.

---

## Step 5 — Integration: deploy + cabinet verification

**Objective.** Prove the coordinated flip end-to-end on hardware.

**Guidance.** Build + deploy the DLL (`./scripts/deploy.sh`); deploy the updated
server. Exercise the full loop on the maintainer's cabinet + server.

**Tests / validation (the acceptance gate).**
1. Card in → cosmetics apply via the game's own `<customize>` load (not a DLL
   overwrite); the options modal shows the **current** selections (seed worked).
2. Change several categories in-game → each applies immediately; card out →
   re-card → selections persisted (via native columns).
3. 2-player: both sides seed and persist independently.
4. Unknown-id edge (optional): a native column holding an id absent locally →
   menu shows item 1, game keeps rendering the stored value (read-only seed),
   no crash.
5. Logs: per-side seed at scene-25 entry; no scene-20 apply; save emits
   `mod_customize_*`; no customize `resolve_from_load`; server load has no
   `<option>` `mod_customize_*`.

**Integration.** Final; both repos live.

**Demo.** A player changes their background/character/lane in-game with no web
portal; it survives card-out/card-in through the game's native profile fields,
with the DLL acting only as the save-direction bridge.
