# Summary — Song Playback Speed: Streaming Rate Engine

Date: 2026-08-08. PDD cycle complete (idea → research → register → design → plan, all
approved same day).

## Artifacts

- `rough-idea.md` — the 2026-08-08 pivot brief (streaming-only, hard constraints,
  keepers/removals).
- `research/orientation.md` — module-level keep/remove/rework verdicts; WSOLA
  streamability analysis; unknowns U1–U7.
- `research/streaming-mechanism.md` — design implications of the RE findings
  (binding, deferral, ring analysis, failure surface).
- `docs/xact_streaming_research.md` (repo-durable) — the RE evidence chain: file-IO
  callback pair + registration AOB anchor, engine read pattern (0x1000 header read,
  64 KiB packets, polled `bWait=0` completion, native `ERROR_IO_PENDING` deferral),
  no file-size checks, create/unregister ordering. Doubles as the durable song-rate
  RE note deferred from the retired feature's plan.
- `idea-honing.md` — decision register D1–D15 (all Accepted/Overridden; readiness
  confirmed 2026-08-08). Notable: D6 overridden — Assist Tick at rate is a delivery
  requirement; D7 — no schema versioning; D9 — drop `cache_limit_gib` outright;
  D15 — tick capacity 1200 s wall.
- `design/detailed-design.md` — **Approved 2026-08-08.** 42 requirements (keepers
  marked), virtual-bank architecture, binding transaction, deferral/silence-fill
  policy, streaming WSOLA equality contract, threading/error models, testing
  strategy.
- `implementation/plan.md` — **Approved 2026-08-08.** 7 steps: removal → streaming
  core → virtual bank/replay → runtime wiring → live bring-up + benchmark →
  dependent features → docs + matrix. Two maintainer deployments (Steps 5, 7).

## Design in one paragraph

Detour gamemdx's XACT file-IO callback pair; bind one file id per rate-armed song
inside the existing `wavebank_create` transaction (pre-original), serving a
synthesized stretched-metadata header and ring-buffered incrementally-generated ADPCM
from a producer thread, with the engine's own polled-async contract as back-pressure.
The FileManager RAM copy stays stock and becomes the generator's source; the on-disk
cache, worker deadline, admission ceiling, and open-redirect seams are deleted. Q31
commits post-original, LAST, exactly as shipped; score containment unchanged. Hard
mid-song failure degrades to silence-fill with clock and taint retained.

## Next steps

1. Run the code-task-generator sop against `implementation/plan.md` one step at a
   time (Step 1 first), then the code-assist sop on each generated task in order.
2. Per repo convention (`AGENTS.md`): implementation progress is tracked in THIS
   directory's `progress.md` + the plan checklist — never `.agents/scratchpad/`.
3. Watch-items for implementation: cross-build AOB verification (0324/0421/0616) in
   Step 4; the Step 5 throughput benchmark is a STOP gate if cabinet margin is
   inadequate; the assist-tick scaffold gate (Step 4) must precede any live rate run
   and is removed by Step 6.

## Assumptions to keep in view

- Cabinet DSP margin ≥ 1× realtime at 25 % (measured ≈11× under Wine on stronger
  hardware) — retired only by Step 5.
- The engine's header parse accepts the synthesized pre-data block (same canonical
  layout the proven on-disk generated banks served) — retired by Step 5's first
  bound create.
- Tick-conversion domain algebra (req 30) is oracle-verified live at Step 7.
