# Summary — Multiplayer Bot "Target Score" tier (2026-09-14)

## Artifacts

- `rough-idea.md` — the maintainer's brief + directives (no S-Marv, local-only persistence, text values).
- `research/orientation.md` — codebase + Ghidra + backend findings (GhostActor wait, per-build field, ghost alphabet/alignment).
- `idea-honing.md` — 14-decision register, all Accepted/Assumed; Readiness Confirmed 2026-09-14.
- `design/detailed-design.md` — Approved 2026-09-14.
- `implementation/plan.md` — Approved 2026-09-14; 5 steps, all checked.
- `progress.md` — resume point + cabinet log anchors.

## What shipped (uncommitted — maintainer commits)

Framework: `PersistMode::Local` + `LoadSource` gate split; `ScalarFormat::Labeled`.
Signatures: `gpa_ghost_actor_probe` → published `gpa_ghost_actor_off` (0x1F0 old / 0x1F8 new) with an `isReady`-prologue identity gate; RTTI `ghost_actor_vtable`.
Bot: `BotMode::{Level, Target}` (row 1..=11, text values, `TARGET` plate, both rows local-only); `ghost.rs` sampler (grade bands, uniform magnitude, sticky side chain, S-Marv floor); planner ghost source + freeze/shock N.G. reproduction + `repro_miss`; `ghost_source.rs` (probed, vtable-gated, state-2 read of the human's GhostActor); filler bind on the Results rebuild with the LV10 fallback (WARN + toast); restore-line diagnostics.
Docs: AGENTS.md rows, `docs/multiplayer_bot_research.md` §11, README, option strings + regenerated preview PNGs.

Host validation: `validate_multiplayer_bot.sh` 94/94, `validate_custom_options.sh` 56/56, `validate_signatures.sh` ALL GREEN, `./build.sh` clean.

## Next steps

1. Deploy (`./scripts/deploy.sh`) and run the design's cabinet checks 1–5 — the key invariant is the `TARGET` bot's money score == the target's points with `repro_miss ≈ 0`.
2. If a `len mismatch` fallback ever appears in field logs, revisit D9 (prefix mapping) with the two lengths in hand.
3. Commit when satisfied (conventional message, no trailers).
