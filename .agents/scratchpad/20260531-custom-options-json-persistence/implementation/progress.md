# Progress — Custom-Options JSON Persistence

Auto mode. Steps 1–8 this session; Step 9 = maintainer cabinet test.

## Checklist

- [x] Step 1 — Config schema: split `persist` → `persist_network` + `persist_json`
- [x] Step 2 — Gate detours + network emission on `persist_network`
- [x] Step 3 — Dirty-checked `custom_options` block writer
- [x] Step 4 — JSON save on `save_sender` (gated on `persist_json`)
- [x] Step 5 — One-shot lazy JSON load timer
- [x] Step 6 — One-time `webui_options` → `custom_options` migration
- [x] Step 7 — Slim WebUI mod (remove JSON I/O)
- [x] Step 8 — Remove `ConfigFile.webui_options` field + docs/config
- [x] Final — Docs refresh + on-cabinet test plan (see `cabinet-test-plan.md`)

## Steps 6–8 + Final

- **Step 6** — `config::migrate_webui_options_to_custom_options()`: raw-JSON
  read-modify-write; moves `webui_options.{p1,p2}` → `custom_options.{p1,p2}`,
  deletes legacy key; idempotent; malformed (non-object) → skip + warn. Called in
  persistence `init()` after gate flags stored, before timer spawn. `cargo check` ✅.
- **Step 7** — `webui_options/mod.rs` slimmed: dropped `config`/`HashMap` imports,
  `CONFIG_KEY`, `load_from_json`/`save_to_json`/`side_key`, the saved-default
  computation, and P2 priming. Registers plain `default_value(0)`; `try_apply_all`
  keeps only the `Customize` field writes. Transforms + discovery + scene-20 wiring
  retained. No dead-code/unused-import warnings. `cargo check` ✅.
- **Step 8** — Removed `ConfigFile.webui_options` field + 2 `None` inits (migration
  uses raw JSON, unaffected). Bundled `mod-config.json` → new flat `custom_options`
  shape (gates + p1/p2). Updated README (new persistence subsection + migration
  note), AGENTS config bullet. `cargo check` ✅; `mod-config.json` valid JSON ✅.
- **Final** — Refreshed `.agents/summary/{components,interfaces,data_models,workflows}.md`;
  updated planning `plan.md` checklist + `summary.md` status. Wrote
  `cabinet-test-plan.md` (8-check matrix + ready-to-use mod-config.json).

## Not committed
Left for maintainer review/commit per solo-repo convention.

## Log

- **Step 1** — `config.rs`: `CustomOptionsConfig { persist }` → `{ persist_network,
  persist_json }` (both `default_true`). Reader in persistence `init()` reads
  `persist_network`. Module doc updated. `cargo check` ✅.
- **Step 2** — Added `PERSIST_NETWORK`/`PERSIST_JSON` `AtomicBool` statics. `init()`
  reads both gates, stores them, early-returns only if both false; detours install
  if either on; success log shows both gate states. Extracted network emission into
  `emit_network_children()` (its internal early-returns now bail network-only, not
  the whole trampoline) — gives Step 4 a clean insertion point. Load receiver
  early-returns when `!PERSIST_NETWORK`. `cargo check` ✅.
- **Step 3** — `config::save_custom_options_values(side, values)` — per-side
  read-modify-write, preserves gate keys + other side + other top-level keys,
  dirty-checked (skips write if `custom_options` block byte-identical), returns
  `bool` (wrote/skipped). `cargo check` ✅.
- **Step 4** — Save trampoline: after network branch, `if PERSIST_JSON` →
  `write_json_cache(&snapshot, side)` builds `{id: wire_value}` from the shared
  `snapshot_for_save()` result and persists via the Step 3 writer. Per-side (matches
  network semantics; doesn't clobber the absent player). `cargo check` ✅.
- **Step 5** — `config::read_custom_options_values()` re-reads `custom_options.{p1,p2}`
  **from disk** (not OnceCell). Persistence module: `JSON_LOAD_DELAY_SECS=12`,
  `spawn_json_load_timer()` (one-shot `thread::spawn` + sleep), `json_load_once()`
  → `resolve_from_load` per value. Spawned in `init()` iff `persist_json`. `cargo
  check` ✅.
  - **Open Q1 resolution (pending cabinet confirm):** timer fires at ~12s in
    attract mode, pre-login. WebUI's `on_change`→`try_apply_all` reads
    `player_work_table[side]`, which is **null** pre-login, so it early-returns
    before any `Customize` write — i.e. the off-thread callback is a safe no-op
    and the value is applied later at scene-20 entry. This is the design's
    "timer primes cache; scene-20 applies" leaning. To be verified in Step 9.
