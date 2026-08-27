# Backend Research — bemani-buddy (weight / is_disp_weight save path)

Backend repo: `~/Desktop/Projects/bemani-buddy` (Rust workspace). Findings from
reading the DDR World playerdata handlers, profile model, migrations, and packet
schema. This feature must ship **together** with the DLL change.

## Repo layout (relevant crates)

```
bemani-buddy/
├── migrations/                       # SQL schema (MySQL/InnoDB)
│   ├── 003_ddr_world_profiles.sql    # ddr_world_profiles table (has weight + is_disp_weight)
│   ├── 009_ddr_world_mod_customize.sql# added opt_mod_customize_* cols (later dropped)
│   └── 010_ddr_world_customize_consolidation.sql # dropped 009's cols; write native cust_* instead
├── models/ddr_world/playdata_3.json  # packet schema (codegen source) for playdata_3
├── crates/
│   ├── bemani-protocol/src/ddr_world/playdata_3.rs  # generated/typed structs
│   ├── db/src/models/ddr_world/profile.rs           # Profile model
│   ├── db/src/mysql/ddr_world/profile.rs            # load/save SQL
│   └── game-server/src/handlers/ddr_world/playdata.rs # load builder + save applier
```

## What already exists (no work needed)

- **Columns** — `migrations/003_ddr_world_profiles.sql`:
  - `weight INT NOT NULL DEFAULT 0` (line 11)
  - `is_disp_weight BOOLEAN NOT NULL DEFAULT FALSE` (line 13)
- **Load emit** — `handlers/ddr_world/playdata.rs` builds the `<common>` block with:
  - `weight: profile.weight` (line 292)
  - `is_disp_weight: profile.is_disp_weight` (line 294)
  So on card-in the backend already sends these; the game's reflect applies them.
- **Load schema** — `models/ddr_world/playdata_3.json`: `"weight": "s32"` (line 57),
  `"is_disp_weight": "bool"` (line 59) in the load `<common>` node.
- **Persistence** — `db` profile model + MySQL UPDATE already include
  `weight`/`is_disp_weight`.

## What's missing — the SAVE path

The game never sends `weight`/`is_disp_weight` on save (web-UI-only fields), so the
save handler has no code to persist them. The **modpack DLL** will now inject
`mod_weight` / `mod_is_disp_weight` into the save `<option>` block; the backend must
detect and store them.

### Precedent: `mod_customize_*` (the pattern to mirror)

`handlers/ddr_world/playdata.rs` (~lines 641-673) reads modpack fields from the save
`<option>` node with `child_i32(option, "...")` and writes them straight into native
columns, **only when present** (so an un-hooked play never clobbers stored values):

```rust
if let Some(v) = child_i32(option, "mod_customize_appeal_board") { profile.cust_appeal_board = v; }
// ... 10 cosmetic fields ...
```

The `mod_customize_*` fields are also declared in the save `<option>` node of
`models/ddr_world/playdata_3.json` (~lines 375-384) as `s32?`.

> History: migration 009 added redundant `opt_mod_customize_*` columns; migration
> 010 dropped them in favour of writing the native `cust_*` columns directly. The
> takeaway for this feature: **write native `weight`/`is_disp_weight` directly — do
> not add new columns.**

## Required backend changes (minimal)

1. **`models/ddr_world/playdata_3.json`** — in the **save** `<option>` node (the one
   at ~line 343, containing the `mod_customize_*` entries), add:
   ```json
   "mod_weight": "s32?",
   "mod_is_disp_weight": "s32?"
   ```
   Regenerate codegen if the build requires it.
2. **`crates/game-server/src/handlers/ddr_world/playdata.rs`** — in the save
   `<option>` applier, alongside the `mod_customize_*` block:
   ```rust
   if let Some(v) = child_i32(option, "mod_weight")         { profile.weight = v; }
   if let Some(v) = child_i32(option, "mod_is_disp_weight") { profile.is_disp_weight = v != 0; }
   ```
   `mod_is_disp_weight` is sent as s32 `0`/`1`; cast to bool for the column.
3. **No migration**, **no `db` model/SQL change**, **no load change** — columns,
   load emit, and persistence already exist.

## Verification (backend side)

- Unit/integration: a `playerdata_save` request carrying `mod_weight=68` /
  `mod_is_disp_weight=1` updates the profile row; a subsequent `playerdata_load`
  emits `<weight>68</weight>` / `<is_disp_weight>1</is_disp_weight>` in `<common>`.
- Absence guard: a save WITHOUT the fields leaves the stored values unchanged.
- There are existing fixtures (e.g.
  `crates/ddr-score-proxy/tests/fixtures/playerdata_load_response.xml`) showing the
  `<common>` shape with `weight` / `is_disp_weight`.
