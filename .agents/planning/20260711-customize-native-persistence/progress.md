# Progress — Native Customize Persistence

Updated: 2026-07-11
Status: **COMPLETE — all 5 steps done; live-verified on cabinet 2026-07-11.**

**NEXT ACTION:** none — feature complete. Maintainer to commit both repos
together (coordinated flip). Residual (low-risk, unexercised): explicit
2-player simultaneous seed/persist test and the unknown-asset-id display
fallback (both code-reviewed, neither cabinet-exercised).

Resume protocol: read `implementation/plan.md` (step list + checklist),
`design/detailed-design.md` (architecture, both repos), and `idea-honing.md`
(decisions D1–D7, findings F1–F2). Wire mapping is authoritative in the repo's
`docs/player_customization_system_research.md`. This file is the live resume
point; update it after each step.

## Done

- Full PDD planning pass (rough-idea, idea-honing, detailed-design, plan,
  summary). Requirements clarified with maintainer; design reviewed.
- RE already complete in `docs/player_customization_system_research.md` (the
  `(category,pattern)→field` mapping) — refactor work only, no further RE.
- **Step 1 (DLL framework)** — `PersistMode { Full, SaveOnly, None }` replaces
  `persist: bool` on `RegisterSpec` + registry (builders default `Full`;
  `no_persist()` now maps to `None`; new `persist_mode()` setter; new
  save-only `save_transform()` setter). `snapshot_for_save` filters
  `mode != None`; `resolve_from_load` early-returns unless `Full` (single gate
  for network load + JSON prime); new `set_value_silent()` (mutates registry,
  discards callback tuple — no dispatch); new `json_persisted(id)` predicate;
  `write_json_cache` filters by it. `cargo check` clean; zero behavior change
  (all pre-existing options are `Full`).
- **Step 2 (DLL WebUI flip)** — WebUI options register
  `.persist_mode(PersistMode::SaveOnly)` + `.save_transform(persist_save_transform)`;
  `persist_load_transform` deleted. Scene callback moved 20 → 25
  (`scene::SONG_SELECT`), now calls new `seed_registry_from_game(0/1)`:
  walks `player_work_table[side] → wrapper → PlayerWork + customize_offset`
  (null-guarded; un-carded side skipped), reads each category's `Customize`
  field as u32 asset id, reverse-maps via `asset_ids.position(id).unwrap_or(0)`,
  seeds via `set_value_silent` (read-only — never writes Customize).
  `try_apply_all` unchanged, now invoked only from `on_value_changed`. One
  INFO seed line per carded side per scene-25 entry. `cargo check` clean.
- **Step 3 (server consolidation)** — bemani-buddy:
  - `migrations/010_ddr_world_customize_consolidation.sql`: RENAMEd the 10
    `cust_<cat>_<pat>` → semantic names (`cust_appeal_board`,
    `cust_character_p1/p2`, `cust_background`, `cust_background_gameplay`,
    `cust_lane_single/double`, `cust_lanecover_single/double`,
    `cust_movie_size`; data + defaults preserved), DROPped `cust_3_0` + the 10
    `opt_mod_customize_*` (kept `opt_mod_autoplay`). Applied to the local dev
    DB (MySQL 9.6).
  - Model (`crates/db/src/models/ddr_world/profile.rs`) + DAO
    (`crates/db/src/mysql/ddr_world/profile.rs`: `row_to_profile!`, `update()`
    SQL + binds) updated to the renamed/dropped columns.
  - Protocol JSON (`models/ddr_world/playdata_3.json`): removed the 10
    `mod_customize_*` from `outputShapes/option` (load echo) ONLY — kept in
    `inputShapes/data/option` (save; protocol docs, Q5) and `mod_autoplay` in
    both. Re-ran codegen (playdata_3.rs −20 lines, exactly the echo fields).
  - Handler (`playdata.rs`): save write-through now writes incoming
    `mod_customize_*` ONLY into the renamed native `cust_*` fields (interim
    dual-write superseded; "only when present" guard kept); load builder echo
    removed from both `PlayerdataLoadOption` sites; `CUSTOMIZE_KEYS` dropped
    `(3,0)`; `build_customize_nodes` rebuilt from the renamed fields.
  - `sqlx migrate run` + `cargo sqlx prepare --workspace` (validates every
    query against the live post-migration schema); `.sqlx/` regenerated.
    `SQLX_OFFLINE=true cargo check --workspace` clean.
    `rg 'cust_[0-9]|opt_mod_customize' crates/` → no hits.
- **Step 4 (docs)** — research doc's "Persistence" section rewritten to the
  single-source model (user-edit-only Customize write, save injection,
  game-native load + scene-25 read-only seed); "Server-Side Persistence
  Mapping" updated (no `<option>` echo — required behavior; bemani-buddy
  reference table now shows semantic columns + (cat,pat) emitted on load;
  stale scene-20 reference in open-question 5 fixed). README.md: WebUI
  Options feature row + "Custom-option persistence" section (SaveOnly
  call-out box; example no longer shows customize keys in p1/p2). AGENTS.md
  `custom_options` config bullet. `.agents/summary/`: components.md
  (custom_options api list + persistence service + webui_options sections),
  workflows.md (Config Persistence), interfaces.md (p1/p2 comment),
  data_models.md (legacy-migration note).

## In flight / uncommitted working-tree state (start-here truth)

Nothing is committed (maintainer commits). All Steps 1–4 changes are in the
working trees:

**ddr-world-universal-modpack:** `src/services/custom_options/{api,registry,mod}.rs`,
`src/services/custom_options_persistence.rs`, `src/mods/webui_options/mod.rs`,
`docs/player_customization_system_research.md`, `README.md`, `AGENTS.md`,
`.agents/summary/{components,workflows,interfaces,data_models}.md`, these
planning docs (untracked).

**bemani-buddy:** `migrations/010_...sql` (new), `crates/db/src/models/ddr_world/profile.rs`,
`crates/db/src/mysql/ddr_world/profile.rs`,
`crates/game-server/src/handlers/ddr_world/playdata.rs`,
`models/ddr_world/playdata_3.json`, `crates/bemani-protocol/src/ddr_world/playdata_3.rs`
(regenerated), `.sqlx/` (regenerated; one query file added, one deleted).
`config.toml` / `packet-logger.toml` remain the maintainer's local-env edits —
NOT part of this feature.

## Deploy & test log

- **2026-07-11 live test #1 (load direction — PASS).** Latest server (with
  migration 010) + latest DLL on cabinet. Maintainer set the 9 non-movie-size
  native columns to `11` in the DB and carded in: all 9 rows showed `11` in
  the in-game menu. (Initial report of LANE COVER (DOUBLE) defaulting was
  operator error — its DB column was actually still `1`; after correcting to
  `11` and reloading, all rows seed correctly.) Log confirms: 123 assets
  discovered for lanecover_double, `seeded 10 option(s) from game Customize`
  for both sides at scene-25 entry, no scene-20 apply, no customize
  `resolve_from_load`. This validates acceptance-gate item 1 (native load +
  seed) and the DB → `<customize>` → game → seed chain.
- **2026-07-11 live test #2 (save direction — PASS).** Maintainer played a
  session exercising in-game cosmetic edits: selections applied immediately,
  the save path emitted `mod_customize_*`, the server wrote them into the
  renamed native `cust_*` columns, and the values round-tripped back through
  the game's own `<customize>` load. Maintainer confirms everything works as
  expected. **Acceptance gate met — feature complete.**
- Residual (unexercised, low-risk): explicit 2P simultaneous test (gate item
  3) and the unknown-asset-id read-only fallback (item 4, optional). Both are
  straight-line code paths reviewed in Steps 1–2; verify opportunistically in
  normal play.

## Deviations & open questions

- Step 1 added a `save_transform(fn)` builder setter (save-half only) beyond
  the plan's letter — `persist_transform` sets both halves and Step 2 must
  not register a dead `load_transform`. `persist_transform` kept (unused).
- Migration 010 was applied to the throwaway local dev DB. The maintainer's
  production DB gets it at Step 5 deploy time (`sqlx migrate run` on the
  server host, or bemani-buddy's normal migration-on-boot if configured).
- Otherwise none. All design questions resolved in `idea-honing.md` (Q1–Q5).

## Key facts for a cold resume

- Goal achieved in code: native `<customize>` profile fields = single source
  of truth. DLL keeps only game→server save; menu seeds by READING the game's
  `Customize` at scene 25 (silent setter — never writes back); no
  network-load / JSON persist for customize ids.
- Acceptance gate (plan Step 5): (1) card-in applies cosmetics via native
  load + menu shows current selections; (2) edits apply immediately + survive
  card-out/in; (3) 2P independent; (4) unknown-id shows item 1, doesn't
  clobber; (5) logs — per-side `WebUiOptions: seeded N option(s) from game
  Customize (side=S)` at scene-25 entry, no scene-20 apply, save emits
  `mod_customize_*`, no customize `resolve_from_load`, server load response
  has no `<option>` `mod_customize_*`.
- Build/validate DLL: `cargo check --target x86_64-pc-windows-msvc`;
  `./build.sh`; `./scripts/deploy.sh`; observe `[DDR-Hook]` logs. Server:
  `SQLX_OFFLINE=true cargo check --workspace`.
- Do NOT commit — maintainer commits. Do NOT touch bemani-buddy
  `config.toml` / `packet-logger.toml`.
