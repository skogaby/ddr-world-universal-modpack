# Backend pattern: bemani-buddy player-option persistence

Source: the **uncommitted in-flight changes** in the sibling `bemani-buddy`
checkout (working tree at commit `04ddbc2`), which add exactly the kind of
option we need — `mod_song_speed` (migration 012) and
`mod_assist_tick_volume` (migration 013). Our `mod_preserve_pitch` change
follows the identical pattern, stacked on top of those in-flight changes.

Paths below are relative to the bemani-buddy repository root.

## The pipeline (JSON model → codegen → Rust)

1. **JSON protocol model** — `models/ddr_world/playdata_3.json` is the source
   of truth. Add `"mod_preserve_pitch": "s32?"` in **two** shapes:
   - the `PlayerdataLoadOption` shape (load response `<option>` block,
     alongside `mod_song_speed` at ~line 105-110)
   - the `PlayerdataSaveOption` shape (save request `<option>` block,
     ~line 396-400)
   The trailing `?` = optional → `Option<i32>` with
   `skip_serializing_if = "Option::is_none"` (load) / `#[serde(default)]`
   (save).

2. **Codegen** — `crates/codegen` (standalone crate,
   `crates/codegen/src/main.rs`):
   ```
   cargo run -p codegen -- models/ddr_world/ crates/bemani-protocol/src/ddr_world/
   ```
   (per CONTRIBUTING.md:87) regenerates
   `crates/bemani-protocol/src/ddr_world/playdata_3.rs` — marked
   `// @generated — do not edit`. Never hand-edit that file.

3. **Migration** — next free number is **014** (012 song_speed and 013
   assist_tick_volume are in flight):
   `migrations/014_ddr_world_preserve_pitch.sql`
   ```sql
   ALTER TABLE ddr_world_profiles
       ADD COLUMN opt_mod_preserve_pitch INT NULL DEFAULT NULL;
   ```
   With the house doc-comment (convention from 008/011, verbatim in 012/013):
   stored **verbatim** — the client DLL owns the value domain; column stays
   **nullable with no default** because stock clients never send the field and
   echoing a default `<option>` child back could crash an un-hooked game.

4. **DB model** — `crates/db/src/models/ddr_world/profile.rs`: add
   `pub opt_mod_preserve_pitch: Option<i32>` to `DdrWorldProfile` (next to
   `opt_mod_song_speed`).

5. **MySQL DAO** — `crates/db/src/mysql/ddr_world/profile.rs`: three spots —
   the `row_to_profile!` macro, the `UPDATE ... SET` column list, and the
   bind-parameter list (in-flight diff shows the exact placement).

6. **Handler** — `crates/game-server/src/handlers/ddr_world/playdata.rs`:
   - `handle_playerdata_load` (~line 340): map
     `profile.opt_mod_preserve_pitch` → `mod_preserve_pitch` in the load
     option struct.
   - `build_new_player_response` (~line 436): `mod_preserve_pitch: None`.
   - `handle_save_profile` (~line 701): only-when-present verbatim write:
     ```rust
     if let Some(v) = child_i32(option, "mod_preserve_pitch") { profile.opt_mod_preserve_pitch = Some(v); }
     ```
   - **Tests** (same file's `mod tests`, pattern from the in-flight
     `mod_song_speed` tests at ~line 1488+): present-is-parsed,
     absent-is-None (un-hooked client must not clobber), malformed-is-None,
     None-is-skipped-on-load (never echo to stock clients),
     Some-is-echoed-on-load. The in-flight `load_option_all_none()` test
     helper must gain the new field.

7. **sqlx offline cache** — after the migration + query changes
   (AGENTS.md:89-94):
   ```
   sqlx migrate run --source migrations/
   cargo sqlx prepare --workspace
   ```
   The regenerated `.sqlx/query-*.json` files are committed (build works with
   no DB).

8. **Validation**: `cargo build` + `cargo test` (offline, deterministic —
   `.sqlx/` committed).

## Notes

- The DLL side sends the wire field automatically for a `PersistMode::Full`
  option (`mod_<id>`), and the game's own `<option>` load applies it back —
  the server's only jobs are store-verbatim and echo-when-present.
- **Maintainer instruction:** if `cargo fmt` produces widespread churn in
  bemani-buddy, leave it in the working tree — the maintainer will fold
  everything into one commit.
- Our change **stacks on uncommitted work** (migrations 012/013 + their model
  and handler edits). Do not renumber or touch those; extend alongside.
