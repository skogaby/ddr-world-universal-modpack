# Rough Idea — Genericize custom-options JSON persistence

## Motivation

Today the **WebUI Options** mod owns its own `webui_options` key in
`mod-config.json`, where it stores an offline cache of P1 and P2's last-chosen
options. It loads these values back from the JSON in case the network being
played on doesn't support custom-options storage on the backend server.

This offline-cache concept is useful for *all* custom options, not just the
WebUI-related ones — but right now it lives specifically inside the WebUI Options
mod.

## Goal

Genericize JSON persistence of custom options into the **custom_options
framework** itself, so it covers every registered option from every registered
mod — not just WebUI options.

### Config changes

1. **Rename `webui_options` → `custom_options`** as the mod-config.json key that
   holds the offline cache. Keep the same shape as before:
   ```
   custom_options: {
     p1: { ... },
     p2: { ... }
   }
   ```
   …but include *all* registered custom options, not only the WebUI ones.

2. **Move the read/write code** for this JSON block out of the WebUI Options mod
   and into the custom-options framework, so it aggregates all registered options
   across all registered mods.

3. **Expand the `custom_options` config entry** from a single `persist` key into
   two boolean keys:
   - `persist_network` — what the current `persist` key does today: gates whether
     custom options are saved to the backend server.
   - `persist_json` — new: gates whether custom options are saved to / read from
     `mod-config.json`. A user may want to disable offline JSON saving when
     playing on a server that supports custom options natively.

## Notes / open questions for honing

- Exact current shape of the `webui_options` JSON block and how WebUI Options
  reads/writes it today.
- How the custom-options framework currently keys options per-player (P1/P2) and
  whether all registered options are addressable generically for save/load.
- Migration: what to do with an existing `webui_options` block in a user's
  `mod-config.json` after the rename.
- Interaction/precedence between network load and JSON load (which wins when both
  are enabled?).
- Default values for `persist_network` / `persist_json`.
