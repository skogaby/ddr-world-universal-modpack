# Idea Honing — Native Customize Persistence

Requirements clarification Q&A. Items D1–D7 were decided in the working
session that preceded this PDD project (RE + initial write-through work);
they're recorded here as settled decisions with the maintainer's rationale.
Questions Q1+ are the genuinely-open items asked one at a time.

---

## D1 (decided): Wire mapping is fully known — no further RE needed

The `(category, key, pattern)` → Customize-field mapping was decoded from
disassembly this session and is documented (with evidence) in
`docs/player_customization_system_research.md` → "Category Dispatch" /
"Server-Side Persistence Mapping". Cross-version validation is unnecessary —
the deployed DLL already proves the field offsets across a broad range of
builds, and the wire protocol categories are stable.

## D2 (decided): Native fields become the single source of truth

The server persists in-game selections into the native `cust_*` profile
fields; the game's own `<customize>` load path applies them. The DLL's
network **load** of `mod_customize_*` is removed (it would be a second writer
contending with the game's own load). The DLL's network **save** of
`mod_customize_*` stays — the game has no native save path for these fields.

## D3 (decided): DLL menu state seeds from the game's Customize object at scene 25

On SONG_SELECT (scene 25) entry — the earliest point the options modal can be
summoned — the DLL reads the `ddr::player::Customize` object (which the game
populated from the server's `<customize>` block) and seeds its options
registry from it. No split across scenes: the previous scene-20 apply-all is
retired; all synchronization happens at scene 25.

## D4 (decided): No savekind gating needed

Maintainer confirmed via packet logging (100% definitive): the game never
sends a profile save before the player has finished their first song. To play
a song you must pass through song select, so the scene-25 seed always precedes
the first save — a save can never emit unseeded defaults. (An earlier
session's claim of a `savekind=1` card-in checkpoint save was a hallucination;
`savekind=1` never appears in server logs.)

## D5 (decided): JSON persistence is dropped for the WebUI options

Not optional, part of the plan. The WebUI options become network-save-only:
no JSON cache write, no JSON prime, no network load. Other custom options
(autoplay, premium-free toggles, power-user-statistics, timing offsets etc.)
keep their existing full round-trip — they have no native game fields.
Maintainer's server already supports the fields; other operators will be
helped to adopt the mapping.

## D6 (decided): No backward compatibility constraints

Both repos are closed-testing and co-maintained. DLL and server changes ship
together; no support matrix for old-DLL/new-server or vice versa.

## D7 (decided): bemani-buddy scope + division of labor

bemani-buddy changes (in scope for this design): rename `cust_<cat>_<pat>` →
semantic names, drop inert `cust_3_0`, drop `opt_mod_customize_*` columns +
protocol fields + load echo (keep `opt_mod_autoplay`), save path writes
incoming `mod_customize_*` directly into the renamed native columns. The
server work is documented as an implementation brief in bemani-buddy's `doc/`
(superseding `doc/ddr_world_customize_column_rename.md`) and delegated to a
Fable 5 xhigh subagent; local MySQL dev DB is available for migrations + sqlx
cache regen. DLL-side changes are implemented directly in this session.

---
## Q1: Seed cadence — every scene-25 entry, or once per card-in session?

Should the DLL re-seed its registry from the Customize object on EVERY
SONG_SELECT entry (including returns from results screens mid-session), or
only on the first entry after a card-in?

**Answer:** Every scene-25 entry. Self-healing, idempotent (a user change is
written into Customize on-change, so re-seeding reads back the same value),
no session tracking needed, and trivially cheap (10 u32 reads + index lookups
per side).

## Q2: How should the SaveOnly persistence mode be expressed in the framework?

The custom_options persistence layer currently treats every registered option
with persist transforms uniformly: emitted on network save, read on network
load, written to + primed from the JSON cache. WebUI options now need
"network-save-only" (emit on save; skip network load; skip JSON entirely).
How should this be expressed?

**Answer:** (a) — a `PersistMode` enum on `RegisterSpec`: `Full` (default,
today's behavior) vs `SaveOnly` (emitted by `save_sender`; skipped by
`load_receiver` and by the JSON cache write + prime). WebUI options register
with SaveOnly. One declarative knob at the registration site; the persistence
service stays generic.

## Q3: Stale JSON cache entries — purge or ignore?

Existing `mod-config.json` files (on the maintainer's cabinet and any testers)
already contain `customize_*` entries under `custom_options.{p1,p2}` from the
old JSON persistence. With WebUI options now SaveOnly, those entries become
dead data. Should the DLL actively purge them on boot, or just ignore them?

**Answer:** Ignore. Stale keys are inert (JSON prime skips SaveOnly ids) and
will likely age out on the next card-out rewrite of the per-side map. No
purge code.

## Q4: Seeding an asset id that isn't in the discovered list — what should the menu show?

At scene-25 seed time the DLL reverse-maps each Customize field's asset id to
a menu index via the discovered `asset_ids` list. If the id isn't present
(e.g. the server stored an id whose arc doesn't exist on this cabinet, or a
web UI set a newer asset), what should the registry be seeded with?

**Answer:** (a) — seed index 0 when the asset id isn't in the discovered
list, and the seed is strictly READ-ONLY (registry state only; never fires
on-change/apply, so it can never clobber Customize). The unknown-id case is a
wide edge case (reverting to an older game version after playing a newer one)
and is not worth solving beyond the index-0 display fallback.

## Q5: bemani-buddy — protocol model shapes for the removed fields

`crates/bemani-protocol/src/ddr_world/playdata_3.rs` is `@generated` from
`models/ddr_world/playdata_3.json`. The `mod_customize_*` fields appear in
BOTH the load `outputShapes/option` (the echo being removed) and the save
`inputShapes/data/option`. The save handler parses raw XML (register_raw), so
the input-shape fields appear unused. Remove the fields from both shapes and
re-run codegen, or only from the output shape?

**Answer:** Remove only from the load echo (`outputShapes/option`). The
`mod_customize_*` fields STAY in the save `inputShapes/data/option` — the
model JSONs dual-purpose as protocol documentation, and the DLL really does
send those fields on save. (`mod_autoplay` stays in both shapes.)

## F1 (implementation finding, not a preference): the seed needs a SILENT setter

Verified in `custom_options`: both `set_value` and `resolve_from_load`
dispatch the option's `on_change` callback. For WebUI options `on_change =
on_value_changed → try_apply_all` (writes the registry value into Customize).
So neither can be used for the read-only scene-25 seed: for an unknown asset id
(Q4) they'd write index-0's asset back into Customize, clobbering the server's
value. The design therefore adds a **silent** registry setter (sets state, no
callback dispatch); the seed uses it exclusively. Known ids would be harmless
either way (writes back the same value), but the silent path is what makes the
read-only guarantee unconditional.

## F2 (implementation finding): choke points for PersistMode

- `snapshot_for_save()` (network emit) → include `Full` + `SaveOnly`.
- `write_json_cache()` (card-out JSON) → `Full` only.
- `resolve_from_load()` (network load AND json-prime timer both funnel here)
  → early-return for non-`Full`, so `SaveOnly` ids are inert on every load
  path in one place.
The existing `persist: bool` field is replaced by `PersistMode { Full,
SaveOnly, None }` (None == today's `persist:false`).

---

## Clarification complete

Requirements clarification closed (maintainer confirmed). No `research/`
directory this session — the wire mapping is already durably documented in
`docs/player_customization_system_research.md`, and the change is
architecture/refactor work whose reality the code will document
self-evidently. Proceeding to detailed design + implementation plan.
