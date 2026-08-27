# Project Summary: 20260531-custom-options-json-persistence

## Artifacts produced

```
.agents/planning/20260531-custom-options-json-persistence/
├── rough-idea.md                  ← genericize the WebUI offline JSON cache
├── idea-honing.md                 ← D1–D4 (research-phase) + Q1–Q7, all settled
├── research/
│   └── current-state.md           ← file:line map of the 2 persistence paths,
│                                     registry value model, config plumbing,
│                                     init ordering
├── design/
│   └── detailed-design.md         ← R1–R14, components, data model, error
│                                     handling, testing, appendices
├── implementation/
│   └── plan.md                    ← 9 incremental steps + checklist
└── summary.md                     ← this file
```

## What this feature does

Moves JSON (offline) persistence of custom options **out of the WebUI Options
mod and into the custom-options framework**, so it covers every registered
persistable option from every mod — mirroring what the **network** persistence
path already does generically. Renames the `mod-config.json` key
`webui_options` → `custom_options` (co-habiting with the gate keys) and splits
the single `persist` gate into `persist_network` + `persist_json`.

Rust-layer only — no reverse engineering, no new game signatures.

## Key design decisions

| Ref | Decision |
|-----|----------|
| D1 | JSON load runs on a **one-shot lazy timer (~12s)** — after all mods register options (which happens at mod `enable()`, *after* persistence init), before any login. |
| D2 / Q7 | **Network wins** over JSON: timer fires before any card swipe, and the network `load_receiver` always re-applies on swipe. No extra tracking state. |
| D3 | JSON path **reuses the registry's `save_transform`/`load_transform`** via `snapshot_for_save()` / `resolve_from_load()`; stores the same wire value as network (asset_id for WebUI). |
| D4 | JSON write is **dirty-checked** — skip the disk write if the `custom_options` block is unchanged. |
| Q1 | JSON **saves on the ess.dll `save_sender`** (card-out), same moment as network. |
| Q2 | Detours install if **either** gate is on; network emit/read gated on `persist_network`, JSON write gated on `persist_json`; both off → no detours. |
| Q3 | **One-time migration**: copy `webui_options` → `custom_options.{p1,p2}`, delete old key. |
| Q4 | **Flat** section shape: `custom_options: { persist_network, persist_json, p1:{}, p2:{} }`. |
| Q5 | Both gates **default true**; legacy `persist` key **dropped** (not read). |
| Q6 | WebUI keeps asset discovery + scene-20 `Customize` write; **sheds all JSON I/O**; registers plain defaults; transforms stay as registered fns. |

## Implementation plan (high-level)

9 steps, each compiles and (from Step 4 on) is demoable on a cabinet:

1. Config schema split (`persist` → `persist_network`/`persist_json`).
2. Gate detours + network emission on `persist_network` (decouple from network).
3. Dirty-checked `save_custom_options_values` writer in `config.rs`.
4. JSON save on `save_sender` — **first end-to-end JSON save**.
5. One-shot lazy JSON load timer — **first end-to-end JSON load** (round-trip complete).
6. One-time `webui_options` → `custom_options` migration.
7. Slim the WebUI mod (remove its JSON I/O).
8. Remove `ConfigFile.webui_options` field + docs/`mod-config.json` updates.
9. Cabinet acceptance test (full gating matrix + precedence + lazy load).

Core round-trip is exercisable by Step 5; Steps 6–8 are cleanup/migration.

## Why genericizing is small

Research confirmed the **network** path is already fully generic (it round-trips
every registered option via `snapshot_for_save()` / `resolve_from_load()`). JSON
persistence was the only mod-specific holdout. So the work is largely "do what the
network path already does, but write `mod-config.json` instead of kbin children"
— reusing the same two registry APIs.

## Areas that may need refinement during implementation

- **Open Q1 — off-thread `on_change` (highest uncertainty).** The lazy timer
  calls `resolve_from_load` from a background thread, which fires WebUI's
  `on_change` → writes the `Customize` game struct. Confirm this is safe off the
  render thread; the leaning (and safest shape) is **timer primes the registry
  cache only**, and the next scene-20 entry applies it via WebUI's existing path.
  Surfaced in Step 5, resolved in Step 9.
- **Dirty-check placement** — kept local to the custom_options writer rather than
  generalized into `save_json_key` (to avoid changing `save_mod_states`
  semantics). Revisit only if a second caller wants it.
- **`ConfigFile.webui_options` removal timing** — safe to remove because migration
  reads raw JSON, not the typed struct; sequenced in Step 8 after WebUI stops
  reading it (Step 7).
- **Lazy-load delay value** (12s) — a const; tune on deploy if boot timing on the
  target cabinet differs.

## Next steps for the user

1. Review `design/detailed-design.md` and `implementation/plan.md`.
2. Begin implementation at Step 1 (each step is independently revertable; 1–2 and
   7–8 especially so).
3. Pay attention to Open Q1 at Step 5 — decide cache-only-prime vs off-thread
   apply before the Step 9 cabinet test.

## Status (2026-06-05)

**Steps 1–8 implemented; Step 9 (cabinet acceptance) pending maintainer deploy.**
Requirements clarification was fully settled earlier (D1–D4, Q1–Q7). Implementation
ran 2026-06-05 in auto mode via `/code-assist`; every step compiles clean under
`cargo check --target x86_64-pc-windows-msvc`.

Code changes:
- `src/mods/config.rs` — `CustomOptionsConfig { persist_network, persist_json }`;
  new `save_custom_options_values(side, values)` (per-side, dirty-checked),
  `read_custom_options_values()`, `migrate_webui_options_to_custom_options()`;
  removed the `ConfigFile.webui_options` field.
- `src/services/custom_options_persistence.rs` — `PERSIST_NETWORK`/`PERSIST_JSON`
  gate statics; detours install if either gate on; network emit/read extracted to
  `emit_network_children()` and gated on `persist_network`; JSON write
  (`write_json_cache`) on `save_sender` gated on `persist_json`; one-shot lazy
  load timer (`JSON_LOAD_DELAY_SECS=12`, `spawn_json_load_timer`/`json_load_once`);
  migration called in `init()`.
- `src/mods/webui_options/mod.rs` — all JSON I/O removed (`load_from_json`,
  `save_to_json`, `side_key`, `CONFIG_KEY`); registers plain defaults; keeps
  discovery, scene-20 `Customize` write, and the registered transforms.
- `mod-config.json`, `README.md`, `AGENTS.md`, and `.agents/summary/*` updated.

**Implementation deviation from design (minor):** the dirty-checked writer is
**per-side** (`save_custom_options_values(side, values)`) rather than taking both
`{p1,p2}` at once. `save_sender` fires once per carded-out side, so writing both
sides would clobber the absent player's block; the per-side writer preserves the
other side straight from the on-disk read. Behaviorally equivalent to the design.

**Open Q1 (off-thread `on_change`) — resolved by code reasoning, to confirm on
cabinet:** the lazy timer fires ~12s post-boot in attract mode (pre-login). WebUI's
`on_change`→`try_apply_all` early-returns on the null per-player work pointer
before any `Customize` write, so the off-thread callback is a safe no-op; the
cached value is applied at the next scene-20 entry. This is the design's "timer
primes cache; scene-20 applies" leaning. Verify on cabinet (Step 9, check 8).

**Not committed** — left for the maintainer to review and commit (solo-repo
convention).
