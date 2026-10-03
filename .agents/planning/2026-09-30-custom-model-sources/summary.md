# Summary — Custom dancer / stage SOURCES (2026-09-30)

## Artifacts

| File | Purpose |
|---|---|
| `rough-idea.md` | The maintainer's request as received |
| `idea-honing.md` | Decision register D1–D19 (accepted; D3 → CUSTOM, D14 → out of plan, D5 refined to merge-by-slug); `Readiness Confirmed 2026-09-30` |
| `research/orientation.md` | Where the feature lives, on-disk reality, the framework facts that shaped the design, the two candidate shapes |
| `research/framework-plumbing.md` | Exact touch points in `custom_options` (aliases, ordering), the mod's pick / preview / catalog paths, harness legs, label art |
| `design/detailed-design.md` | Approved 2026-09-30 — requirements R1–R21, architecture, components, data models, error handling, testing strategy |
| `implementation/plan.md` | Approved 2026-09-30 — five steps with checklist |
| `progress.md` | To be created at the start of implementation (live resume point, per AGENTS.md) |

## Design in one paragraph

`data_mods/custom_models/dancers/<Source>/<Friendly>/pl_<key>/` makes `<Source>` a dancer source
(same for stages); anything at today's two levels lands in the implicit source **CUSTOM**. The
Background Dancers mod registers a **DANCER SOURCE** / **STAGE SOURCE** row (RANDOM · STOCK · sources)
whenever a custom source of that kind exists, plus one model row per source shown through
`ShowWhen::Equals` — `background_dancer` keeps meaning the stock pool, `background_dancer_<slug>` per
source, so every source remembers its last pick per player. Source RANDOM draws exactly as today;
source S + RANDOM draws within S, honouring the movie screen rule and falling back to all of S with a
WARN. The stage rows stay cabinet-wide (mirrored through a value-changed observer). The framework gains
two small general capabilities: texture aliases on a row (one label PNG / chrome for N rows) and
family-aware ordering (an unlisted source row sits above its listed child in every existing config).

## Plan in one paragraph

Step 1 lands the framework pieces (host-tested, no visible change). Step 2 lands discovery (`sources.rs`,
three-level walk, sourced plan entries) behind an adapter that keeps today's flat rows. Step 3 lands the
grouped catalog and the per-element choice / pool logic in `selection.rs`, still behind a flat shim.
Step 4 replaces the shim with the real rows, requests, preview wiring, label art and the shipped config
order — the one cabinet step, validated against the design's §7.3 checklist. Step 5 updates the docs.

## Status (2026-09-30)

DONE — cabinet-validated by the maintainer; content moved into source folders (8 dancer sources).
Steps 1–5 implemented and documented in one session (task files under
`.agents/tasks/2026-09-30-custom-model-sources/step0{1..4}/`, per-task records under
`.agents/scratchpad/2026-09-30-custom-model-sources/`). Gate: `cargo check` clean, `cargo fmt`,
`validate_custom_options.sh` 72/72, `validate_background_dancers.sh` 215/215, `./build.sh` clean.
Nothing committed (maintainer commits).

## Next steps

1. Maintainer commits (`git add -A data_mods/custom_models` pairs the moves as renames).
2. Optional: re-run the codebase-summary workflow (`.agents/summary/interfaces.md` predates the
   `RegisterSpec` texture aliases and the family ordering rule); clear `data_mods/_cache/custom_models/`
   once to drop the orphaned pre-move cache arcs.

## Assumptions and areas to watch

- N (custom sources per kind) stays small; every registered row allocates one hidden native row per
  form open per side. If sources grow past a few dozen, revisit.
- The observer-based mirror relies on `set_value`'s unchanged-value check for termination, exactly as
  the `on_change` tail does today; watch the versus log on the first cabinet pass.
- A source folder literally named `Source` is refused (id collision); `Custom/` merges into CUSTOM by
  design. Document both in README (Step 5).
- Legacy folders with a nested non-model subdirectory that itself holds models were ignored before and
  now promote their parent to a source — a behaviour change for an odd layout, documented in the
  layout contract.
- The maintainer's content move is outside the plan; until it happens every existing dancer shows
  under CUSTOM and the rows behave as before with one extra source row.
