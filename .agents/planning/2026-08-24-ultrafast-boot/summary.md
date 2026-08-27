# Ultrafast Boot — Planning Summary

Date: 2026-08-24
Feature: refactor of the `fast-bootup` mod — cache the boot-time SSQ analysis
outputs and replay them on subsequent boots; remove the loader's per-frame
pacing so cache misses load at disk speed.

## Artifacts

| Artifact | Path | State |
|---|---|---|
| Rough idea | `.agents/planning/2026-08-24-ultrafast-boot/rough-idea.md` | Final |
| Orientation research | `.agents/planning/2026-08-24-ultrafast-boot/research/orientation.md` | Final |
| Decision register | `.agents/planning/2026-08-24-ultrafast-boot/idea-honing.md` | Readiness Confirmed 2026-08-24 (17 decisions; D6/D9 overridden by maintainer) |
| Detailed design | `.agents/planning/2026-08-24-ultrafast-boot/design/detailed-design.md` | **Approved 2026-08-24** |
| Implementation plan | `.agents/planning/2026-08-24-ultrafast-boot/implementation/plan.md` | **Approved 2026-08-24** (8 steps) |
| Primary RE record | `docs/ultrafast_boot_research.md` | Published (durable repo doc; predates this planning dir) |

## Design in one paragraph

First boot captures the game's own analyzer outputs
(`result[14]/radar[5]/ret` per file × difficulty × mode) at the Analyze
boundary — via a new shared `services/analyze_hook.rs` dispatcher that also
carries NTX's mine injection — and persists them to
`data_mods/_cache/step_data/v1.bin` keyed by resolved-file identity
(path + size + mtime, LayeredFS-aware). Subsequent boots build a per-item
boot plan: verified items are replayed (records flipped to the stock
"complete, empty" shape, music-DB writes + actor accumulators transcribed
from the decompiled arithmetic, releases through the game's own machinery),
misses and always the final work item run the existing gated stock path
(which also runs the game's completion block natively). The loader's
4-opens-per-pump cap is raised to 64 during the pass. Everything fails open
to today's fast-bootup behavior; the replay never touches the game's ME1529
error reporter (hard boot blocker).

## Plan shape

Steps 1–2 land the pure layers + address derivations (highest
design-invalidation risk first); Step 3 the pacing raise (dev-velocity +
the measurement that decides whether the bounded drain in design Appendix B
is ever built); Step 4 the dispatcher refactor proven by NTX itself;
Steps 5–7 the trust ladder (capture-only → temporary parity diff → replay);
Step 8 mutation drills, removal of the temporary diff code, and docs
(AGENTS.md row, README operator notes).

## Next steps

1. Run the **code-task-generator** sop against
   `.agents/planning/2026-08-24-ultrafast-boot/implementation/plan.md`
   (one step at a time), then **code-assist** on each generated task in
   order (task files land under `.agents/tasks/2026-08-24-ultrafast-boot/`).
2. Maintain `.agents/planning/2026-08-24-ultrafast-boot/progress.md`
   throughout implementation (per AGENTS.md conventions) — it is the
   cross-session resume point, including the cabinet deploy & test log.
3. Cabinet validation is the real harness: Steps 3–8 each carry a deploy
   demo; Step 6's zero-mismatch parity boot is the hard gate before any load
   is skipped.

## Assumptions / watch items

- **Analyze-arg stability (design assumption):** the `result`/`radar` stack
  pointers must remain valid at post-original dispatch time on both builds —
  verify during Step 4/5 (design flags this; NTX's identical-shaped detour is
  strong evidence).
- **Pre-batch in-flight window:** ≤ open-cap files load stock before the
  first hooked call — self-healing by design, but confirm the count in the
  Step 5 logs.
- **Pacing measurement (Step 3)** decides Appendix B; expected outcome is
  that the cap raise alone suffices.
- **Percent bar** jumps 0→100 on full-hit boots (accepted, D16).
- The register (D1–D17) is consolidated in the design's Detailed
  Requirements; any implementation-time deviation should be logged in
  `progress.md` and, if user-visible, re-confirmed.
