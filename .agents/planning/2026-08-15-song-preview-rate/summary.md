# Summary: Real-Time Rate Preview at Song Select

Date: 2026-08-15. PDD planning complete — design and plan both approved.

## What this feature is

The `song_speed` / `preserve_pitch` option rows (mod `song-playback-speed`)
now govern the song-select preview: while the controlling player desires a
non-100 % rate, every music-wheel preview plays at that rate in the selected
DSP mode (WSOLA pitch-preserved or plain resample), and editing either row
while a preview plays restarts it at the new settings ~150 ms (debounced)
after the last tick — including edits back to 100 %, which restore the
literal stock preview.

## Artifacts

| Artifact | Path |
|---|---|
| Rough idea | `rough-idea.md` |
| Decision register (D1–D15, all settled; Readiness Confirmed 2026-08-15) | `idea-honing.md` |
| Orientation survey | `research/orientation.md` |
| Preview-pipeline RE (SelectMusicSequence → View → AudioPlayer → AudioLoader; restart mechanism; R-A evidence; 20260721 inventory + 20260616 cross-check) | `research/preview-retrigger-re.md` |
| Engine integration survey (planner/binding/registry/io/detour touch points) | `research/engine-integration.md` |
| Detailed design (Approved 2026-08-15) | `design/detailed-design.md` |
| Implementation plan (Approved 2026-08-15; 6 steps, 2 cabinet deploys) | `implementation/plan.md` |

## Design in one paragraph

Two independent halves. (1) **Wheel-settle binding**: the already-detoured
`wavebank_create` gains a preview-qualification branch — at scene 25, a
slot-5 dance-bank create with exactly one entered side desiring ≠ 100 % gets
a `StretchTarget::Side` virtual bank (stretched `_s` entry, verbatim main)
published into a new independent preview registry slot consulted on the io
hot path's miss branch. (2) **Live-edit restart**: `on_change` stamps a
debounce cell; a game-thread input-poll executor fires 150 ms after the
last tick (unless a wheel-settle create superseded it), validates the
loader chain behind vftable identity gates, then stop-cue → unregister both
banks → re-create via the load-completion router (the create re-qualifies
through the detour) → re-arm the game's own loader tick, which replays the
cue itself. Zero new hooks; preview bindings never touch Q31/score/movie/
lifecycle; every failure fails open to a stock preview.

## Next steps

1. Run the **code-task-generator** sop against
   `.agents/planning/2026-08-15-song-preview-rate/implementation/plan.md`,
   one step at a time (Step 1 first).
2. Implement each step with the **code-assist** sop in order; tick the
   plan checklist and keep `progress.md` current per the repo's PDD
   progress-tracking convention.
3. Cabinet deploys land at Step 3 (wheel-settle stretched previews) and
   Step 5 (full C1–C9 matrix).

## Assumptions / refinement candidates before implementation

- The `_s` entry shares the main entry's ADPCM profile (strictly validated
  at parse; a violation surfaces as a fail-open bind refusal, not a bug).
- Restart audio cleanliness (compressed stop→unregister gap) and the
  per-settle private-source memcpy (~8–30 MiB) are cabinet-measured items;
  both have fallback room in the design (immediate-stop variant; copy-size
  cap) without re-planning.
- The four new AOB signatures must be validated unique on all four
  supported builds during Step 4 — patterns are sketched in the RE note but
  not yet committed.
