# Summary: Per-Song Judgement Offsets — PDD Session

Completed 2026-08-17. All gates passed: register accepted, readiness confirmed,
design approved, plan approved.

## Artifacts

| Artifact | Path |
|----------|------|
| Rough idea | `.agents/planning/2026-08-17-per-song-judgement-offsets/rough-idea.md` |
| Decision register (D1–D20, Readiness Confirmed 2026-08-17) | `.agents/planning/2026-08-17-per-song-judgement-offsets/idea-honing.md` |
| Orientation research | `.agents/planning/2026-08-17-per-song-judgement-offsets/research/orientation.md` |
| Persistence / save-flow / str-wire research (incl. Ghidra verification) | `.agents/planning/2026-08-17-per-song-judgement-offsets/research/persistence-and-save-flow.md` |
| musicdb crawl + options-UI research | `.agents/planning/2026-08-17-per-song-judgement-offsets/research/musicdb-and-ui.md` |
| Detailed design (Approved 2026-08-17) | `.agents/planning/2026-08-17-per-song-judgement-offsets/design/detailed-design.md` |
| Implementation plan, 8 steps (Approved 2026-08-17) | `.agents/planning/2026-08-17-per-song-judgement-offsets/implementation/plan.md` |

## Design overview

A new top-level mod (`per-song-judgement-offsets`) adds a parent toggle
"ADJUST OFFSET FOR CURRENT SONG" + child scalar "CURRENT SONG OFFSET"
(−100..+100) to the options menu, tracking the song highlighted on the wheel
per player side. During gameplay the stored per-song value replaces the stock
JUDGEMENT OFFSET (`Option+0x24`), written at first judge dispatch and restored
at the `prev == GAMEPLAY` scene change — with a redundant song-select sweep
and a save-trampoline tree-fix of `<timing_music>` as independent safety
layers, because research proved the save marshal snapshots PlayerWork before
the trampoline runs. Persistence: local `judgement_offsets.csv` (baseline) +
per-profile `mod_judge_offsets` kbin str field on bemani-buddy (session-map
overlay; server load never touches the CSV). The CSV self-seeds at boot from
the merged musicdb (custom songs included) via `xml_merger` reuse — zero new
detours, zero new signatures. A one-time script pre-seeds the CSV from a
community mcode-keyed offsets list.

## Plan overview

1. Pure state layers (`store.rs`, `csv.rs`) — host-tested
2. Pre-seed script + repo-committed CSV
3. Mod skeleton + musicdb bootstrap crawl
4. Option rows + wheel-poll seeding + edit capture + label textures
5. Gameplay override + restore layers + tree-fix safety net
6. String-field persistence extension + client wire
7. bemani-buddy backend field (migration 016 etc.)
8. Integration hardening + full cabinet validation

## Next steps

1. Run the **code-task-generator** sop against
   `.agents/planning/2026-08-17-per-song-judgement-offsets/implementation/plan.md`
   to produce task files (one plan step at a time).
2. Run the **code-assist** sop on each task in order.
3. Maintain `progress.md` in this planning directory throughout implementation
   (per AGENTS.md's PDD feature-progress convention).

## Assumptions / refinement candidates before implementation

- Minus-glyph rendering in the options-menu digit compositor is strongly
  evidenced (stock JUDGE TIMING row is ±100) but confirmed only at the Step 4
  cabinet deploy; the fallback (display transform) is noted in the design.
- Ordinal-176 str-read overflow behavior (truncate vs fail) is unverified;
  mitigated by a 64 KiB read buffer vs a ~26 KB capped payload.
- The `stage_records` course/event accessors are assumed sufficient to detect
  all no-override modes; Step 5's cabinet pass validates.
- bemani-buddy's `playdata_3.json` ↔ generated-code desync (missing 015 field)
  is scheduled for backfill in Step 7 — regen tooling should not be run before
  that lands.


---

## Implementation closeout (2026-08-18)

All 8 plan steps implemented and cabinet/server-validated across four deploy
gates. Everything is staged, uncommitted (maintainer commits manually per
AGENTS.md Git rules) — in BOTH repos (this one and bemani-buddy).

Post-design evolution (all maintainer-approved, recorded in the register and
the amended design):
- D21: Training Mode AND Course/Dan apply overrides (requirement 8
  superseded); per-stage identity = wheel latch + SSQ-open observer +
  dance-bank-create observer; lazy value resolution at first judge.
- ScalarFormat::SignedUnit { unit } framework extension (stock-parity
  "-41ms"/"+10ms"/"±0ms" value text; formatter emits raw SJIS-capable bytes).
- Judge pre-hook at Priority::Early (assist_tick reads the field on the same
  dispatch at Normal — claps follow the per-song offset).
- Disk-based musicdb crawl (AVS trampolines are game-thread-only).
- String-field persistence extension in custom_options_persistence
  (register_string_field / register_card_in_callback / replace_option_s32).

Deliverables beyond the plan: docs/per_song_judgement_offsets.md (RE note),
README + AGENTS.md rows, scripts/validate_judgement_offsets.sh (host-test
harness), repo-committed pre-seeded judgement_offsets.csv.

Deferred (maintainer, separate effort): generalize SignedUnit to arbitrary
prefix/suffix scalar qualifiers for other rows.
