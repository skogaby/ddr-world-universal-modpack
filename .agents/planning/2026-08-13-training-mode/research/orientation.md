# Orientation — Training Mode

Step-2 orientation record. Unusually for a PDD run, the deep research was
**front-loaded before this process started** — two dedicated RE passes exist
as durable repo docs, and this file is a pointer map plus the deltas found
while orienting, not a re-derivation.

## Research base (read these first)

| Doc | Covers |
|---|---|
| `docs/training_mode_research.md` | The entire feasibility surface: seek-to-T record rebuild semantics (§3), natural song-end chain + loop clamps (§4), shifted audio serving design (§5), wall(T) anchor math (§6), 0x1044 subscriber audit (§7), prior-doc errata (§8), open questions (§9) |
| `docs/option_header_rows_research.md` | Non-selectable header rows: the `+0x28` selectability interface, cursor-path predicates, implementation strategy (mod-owned 2-slot vtable swap, zero new signatures), per-row height control (`+0xA8`) |
| `.agents/planning/20260812-inplace-restart/` | The shipped in-place reset (`src/services/song_reset.rs`) — the foundational primitive; `request_reset(t_ms, delay_ms, on_recovery)` with `t_ms != 0` = the designed Training Mode extension point |

## Key orientation facts

1. **Every feature maps to existing or designed-for machinery.** Seek =
   in-place reset with back-dated anchor + record rebuild at T + shifted
   audio serving through the song-rate binding. Loop = per-frame content-time
   check → seek. Score integrity = `score_guard` taint. HUD = widget system
   (`timing_stats_widget` precedent). Grouped options = header-row vtable
   swap on the existing donor-clone row factory.
2. **The one lifecycle extension**: training seeks at 100 % speed need a
   song-rate binding armed at identity — `passthrough_plan` for the main
   entry (NOT `plan_entry(100)`, which block-quantizes), shifted-passthrough
   serving. Fail-open: no binding ⇒ loop-from-0 only.
3. **Hard clamps**: loop end bound and forward seeks must stay below the
   ControlMessageActor end thresholds (`+0x94` display-domain / `+0x98`
   raw-ms) — the end cascade is one-way and a run at StackStep 6 is
   unresettable.
4. **Gesture space during gameplay**: pinpad 1 (restart) and 3 (fail) taken;
   0 = menu (all scenes). 2/4/5/6/7/8/9 free during gameplay. 9 is taken
   only at song select (quick logout, scene-25 gated).
5. **Repo conventions that bind the design**: one detour per target,
   AOB/RTTI resolution only, fail-open with one WARN, per-player options via
   `custom_options` rows, validation = cabinet deploys.

## Proposed sequence

Requirements register first (research is done), then design. No further
research runs anticipated except opportunistic verification during
implementation (the §9 open-questions list in the training research doc is
deploy-verification, not design-blocking).
