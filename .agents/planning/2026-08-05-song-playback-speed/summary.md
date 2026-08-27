# PDD Summary: Song Playback Speed

Date: 2026-08-05

Status: Complete

## Artifacts

- `rough-idea.md`: original player-controlled song-rate concept.
- `idea-honing.md`: accepted 25-decision register and readiness confirmation.
- `research/orientation.md`: current-code architecture and integration survey.
- `research/pitch-preservation.md`: XWB/ADPCM reuse, WSOLA direction, exact-rate,
  performance, and cache findings.
- `research/runtime-integration.md`: cross-version clock window, XACT commit
  point, mode classification, and lifecycle evidence.
- `design/detailed-design.md`: approved self-contained detailed design, revised
  through a six-aspect and focused follow-up review.
- `implementation/plan.md`: approved eight-step proof-first implementation plan.

## Approved Feature

The feature adds a persisted per-player `SONG SPEED` option with 75%, 100%, and
125% values. Non-100% playback preserves pitch by transforming the selected
song's strict two-entry streaming XWB: decode stereo MS-ADPCM, apply a
deterministic joint-stereo WSOLA-like stretch, re-encode, rebuild, and serve an
immutable cached bank through LayeredFS.

The generated main entry's exact source/output frame ratio drives a permanent
identity-controlled `music_count` patch. XACT success is committed through a
call-nonced, fixed-slot `wavebank_create` transaction. Score protection, movie
suppression, Assist Tick, chart-ms statistics, and Real Speed consume the same
generation. Every non-100% stage is score-suppressed and logout-sanitized.

Ordinary e-amusement-connected solo and doubles are supported. Local versus,
courses, matching/BPL, demos, special-event chains, and unclassified state use
100%. Quick Restart retains or idempotently reloads the same generation.

Generated audio is stored under a configurable 10 GiB crash-safe LRU cache.
Unsupported sources and pre-exposure failures fall back to 100%; late XACT
rejection aborts loading and quarantines the cache identity rather than risking
an unsafe same-attempt retry.

## Implementation Shape

The approved plan proceeds in eight increments:

1. pure XWB/ADPCM/WSOLA pipeline and mandatory host validator;
2. persistent cache and bounded generation worker;
3. identity-only clock/XACT/LayeredFS transaction infrastructure;
4. hard-gated pre-generated 75% cabinet proof;
5. generalized on-demand 75%/125% generation and XACT integration;
6. per-player UI, mode policy, config, and bemani-buddy persistence;
7. Assist Tick, Real Speed, statistics, score audit, movie, and lifecycle work;
8. observability, fault injection, documentation, and full release evidence.

The 75% diagnostic is a hard gate. Failure stops later product work until the
audio/clock model is corrected.

## Assumptions and Live Gates

- Generated XWB termination and loop behavior must pass the diagnostic before
  generalized runtime generation is released.
- The Core-BPM cache source must be re-derived and live-validated because older
  project notes disagree on its semantic field label.
- Quick Restart must pass both retained-bank and idempotent-reload paths.
- Full score readiness requires the field-level competitive aggregate audit,
  checked league removal, and backend/database evidence.
- Cold generation must satisfy the approved latency/memory thresholds on native
  Windows and CrossOver.
- Game-derived XWBs and sensitive local evidence remain uncommitted; only hashes
  and validation status are retained in source documentation.

## Next Steps

1. Run the `code-task-generator` SOP against
   `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md` to
   generate Step 1 task files under `.agents/tasks/song-playback-speed/step01/`.
2. Run the `code-assist` SOP on each generated task in order.
3. At implementation start, create and maintain the canonical
   `.agents/planning/2026-08-05-song-playback-speed/progress.md`, updating it
   after every completed step and cabinet deployment.
4. Do not proceed past Step 4 unless the diagnostic acceptance evidence passes.
