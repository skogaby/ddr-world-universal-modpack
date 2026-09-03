# Summary — Split SSQ Auto-Discovery

## Artifacts
- `rough-idea.md`, `idea-honing.md` (15 decisions, Readiness Confirmed 2026-09-03), `research/orientation.md`
- `design/detailed-design.md` (Approved 2026-09-03), `implementation/plan.md` (Approved 2026-09-03, 3 steps, all ticked)
- `progress.md` — live resume point + cabinet test checklist
- Code: `src/mods/split_ssq_auto_discovery/{mod,resolver,discovery}.rs`, `build_ssq_path` in `src/core/signatures.rs`, `scripts/validate_split_ssq.sh`, AGENTS.md row. RE: `docs/split_ssq_research.md`.

## Design in one paragraph
One `GenericDetour` fully replaces the game's `build_ssq_path`. At enable the mod scans stock + LayeredFS `mdb_apx/ssq/` for `*_[1-5].ssq`, reads each file's chart levels, and builds a Rule-A index (highest `N ≤ d+1` whose file contains the level-`d` chart, else the unsplit file). Basename-opaque ⇒ `toho1..4` unchanged. Fail-open everywhere; the original serves only as a divergence oracle whose INFO lines are the cabinet validation signal.

## Next steps
1. Cabinet deploy + the four checks in `progress.md`.
2. Maintainer commits (`feat(split-ssq): ...`).
3. Follow-up (out of scope): route `src/services/chart_length.rs` through `resolver::Index::resolve`.
