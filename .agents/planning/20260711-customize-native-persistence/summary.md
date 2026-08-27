# Summary — Native Customize Persistence (single source of truth)

## What this is

A planned refactor that makes DDR World's **native `<customize>` profile
fields the single source of truth** for the WebUI Options cosmetics
(appeal board, characters, backgrounds, lanes, lane covers, video size),
retiring the duplicative `mod_customize_*` round-trip and its two-writer
contention.

The DLL keeps only the direction the game itself lacks — **sending** in-game
edits to the server on save. Everything else rides the game's own load path:
the server write-throughs saved values into the native columns, the game
applies the `<customize>` block, and the DLL seeds its in-game menu by
**reading** the game's `Customize` object at SONG_SELECT (scene 25) entry.
JSON offline persistence is dropped for these options; a new `PersistMode`
enum expresses "network-save-only" declaratively.

Spans two co-maintained repos (ddr-world-universal-modpack + bemani-buddy),
executed together by a single agent after planning.

## Artifacts

- `rough-idea.md` — the concept + motivation + division of labor.
- `idea-honing.md` — decisions D1–D7 (carried from the working session),
  clarifying Q1–Q5, and implementation findings F1–F2 (silent seed setter;
  `PersistMode` choke points).
- `design/detailed-design.md` — standalone design: requirements, the
  authoritative wire mapping, before/after data-flow diagrams, DLL component
  changes (`PersistMode`, silent setter, scene-25 seed), the full bemani-buddy
  change set, error handling, testing strategy, and alternatives.
- `implementation/plan.md` — 5 incremental steps with a checklist (2 DLL,
  1 server, 1 docs, 1 integration), each compile-complete and reviewable;
  end-to-end verified at the integration step.
- (No `research/` — the wire mapping is already durably documented in
  `docs/player_customization_system_research.md`; this is refactor work.)

## Design in one paragraph

Add `PersistMode { Full, SaveOnly, None }` to the custom-options framework and
a **silent** registry setter (sets state without firing `on_change`). Register
the WebUI options as `SaveOnly` (emitted on network save; skipped by network
load and JSON). Replace the WebUI mod's scene-20 "apply registry → Customize"
callback with a scene-25 "seed registry ← Customize" read-only pass, so the
game's own load is the only writer besides an explicit player edit. On the
server: write incoming `mod_customize_*` straight into the (renamed) native
`cust_*` columns, emit them in `<customize>` on load, remove the `<option>`
echo, drop the `opt_mod_customize_*` columns and the inert `cust_3_0`, and
rename the opaque `cust_<cat>_<pat>` columns to semantic names.

## Key decisions (see idea-honing.md)

- Seed on **every** scene-25 entry (self-healing, idempotent, cheap).
- **`PersistMode` enum** (not booleans / not a service-side id list).
- Stale JSON customize keys: **ignore** (inert, self-aging).
- Unknown asset id at seed → **index-0 display, read-only** (game keeps its
  value); wide downgrade edge case, not otherwise solved.
- Protocol JSON: remove the load echo only; **keep** the save-side fields as
  living protocol documentation.
- **No backward compatibility** — both repos ship together.

## Next steps

1. Review `design/detailed-design.md` and `implementation/plan.md`.
2. Execute the plan step-by-step (single agent, both repos):
   DLL framework → DLL WebUI flip → server consolidation → docs → integration.
3. Reconcile the two pre-existing uncommitted edits noted in the plan header
   (the interim server write-through; the research-doc mapping section written
   for the old dual-channel design).
4. Run the Step-5 cabinet acceptance gate (1P/2P round-trip, log checks).

## Areas that may need attention during execution

- The DLL `customize_offset` + `player_work_table` signatures are already
  `required_signatures` of WebUI Options, so the seed's pointer chain is
  covered; still confirm the seed's null-guards on an un-carded side.
- One migration + one `cargo sqlx prepare` covers the server schema change; the
  local dev MySQL is available for it.
- Deploy is a coordinated flip — don't ship one repo without the other.
