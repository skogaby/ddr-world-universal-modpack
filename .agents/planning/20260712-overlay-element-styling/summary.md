# Summary — Overlay Element Styling (PDD closing)

Feature: per-player **scale** (25–150 %) and **opacity** (0–100 %) settings
for the dynamic gameplay feedback elements — combo counter, judgement text
(incl. freeze O.K./N.G. and FAST/SLOW), and pacemaker score tracker.
Receptor hit flashes explicitly excluded.

## Artifacts

- `rough-idea.md` — feature statement + prior-RE recap
- `idea-honing.md` — 10 Q&A decisions (per-player, one shared knob pair,
  scalars 25–150/0–100 default 100, apply-at-song-start, PersistMode::Full,
  always-visible rows, versus in v1, both color hooks, two-tier degradation,
  naming)
- `research/pointer-to-re-docs.md` — pointer to the durable RE doc
- `design/detailed-design.md` — full design (components, per-kind
  one-shot/compose matrix, side binding, error handling, testing strategy)
- `implementation/plan.md` — 8 steps with checklist
- `docs/gameplay_overlay_elements_research.md` (repo-level) — the RE
  foundation, cross-version validated on gamemdx 20260616 + 20260324

## Design in one paragraph

One cold-path detour on `CMovieClip::Create` captures every scoped element
wrapper by template name each song; a non-fatal SetPosition detour binds each
clip to a player side (active-side for single/double, x-threshold for versus)
and applies one-shots — `afp_layer_set_matrix` for scale (sole-writer
invariant makes it stick) and `afp_layer_set_color` for opacity on the
never-colored clips — while compose detours on the wrapper's two SetColor
vfuncs multiply the alpha of the game's own color writes (combo visibility
gating, pacemaker dim) so opacity composes instead of fighting game state.
Values are two `custom_options` scalar rows with full profile persistence.

## Implementation plan shape

1. Signatures + color-twin IAT resolver
2. `bm2d_api` raw-id setters
3. Mod skeleton + option rows (settable + persisted)
4. Capture detour + registry
5. Side binding + scale (first visual)
6. Opacity one-shots + float compose
7. Int-variant compose + hardening + degradation drills
8. Docs + regression sweep + close-out

## Next steps

1. Start Step 1 (`core/signatures.rs`) — create `progress.md` in this
   directory at the same time (AGENTS.md PDD convention).
2. Cabinet time needed from Step 1 onward (every step validates via deploy +
   logs); versus-capable setup needed at Steps 5–6.
3. Open empirical items to resolve during implementation: `X_SPLIT` exact
   value (Step 5 bind logs), +0xB0 coverage verdict (Step 7 logs), step
   ergonomics 5/25 (Step 3), 150 % aesthetics (Step 5).
