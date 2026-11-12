# Summary: Optional Content Packs

Planning completed 2026-11-11. Design and plan are approved; nothing is implemented.

## Artifacts

| File | Holds |
|---|---|
| `rough-idea.md` | The request as given. |
| `idea-honing.md` | Decision register D1–D29 with rationale; readiness confirmed. |
| `research/orientation.md` | What the idea has to fit: content sizes, the installed updater's limits, the existing option-row merge. |
| `research/hosting-and-downloads.md` | GitHub limits, ranged downloads, pack identity, measured compression. |
| `research/existing-code.md` | Updater modules and release scripts, and what each needs to change. |
| `design/detailed-design.md` | The approved design. |
| `implementation/plan.md` | The approved 13-step plan. |
| `progress.md` | Live resume point during implementation. |
| `prototypes/` | Throwaway console mock; ignored by git, never committed. |

## The design in brief

- The ported dancers and stages (3.5 GB) leave the code repository for
  `skogaby/ddr-world-universal-modpack-assets`. Its `packs` release stores one zip per
  source folder (23 packs) and a `catalog.json`.
- A modpack release stays one zip. It embeds the catalog, pinning pack revisions.
- The updater updates the base, then keeps the user's selected packs current and
  repaired, then shows a 5-second prompt on every run that opens a console checklist.
- Files the modpack never shipped are never touched. Shipped files are restored when
  missing or modified.
- Missing option rows are inserted by section as before, with one refinement so a row
  lands beside the row it is named after when the user has re-ordered.
- The models leave the code repository's history when `v1_5` is squash-merged into
  `main` and deleted.

## Next steps

1. Run the code-task-generator sop against `implementation/plan.md`, one step at a
   time, starting with Step 1.
2. Run the code-assist sop on each generated task in order.
3. Keep `progress.md` current after every step.

## Before implementation starts

- `gh` is installed and logged in on the build machine (2026-11-11).
- The assets repository is public and cloned beside the code checkout; it holds only a
  licence file so far (Step 2 fills it).
- Disk space is not a constraint: macOS reports about 81 GB free once purgeable space
  is counted (`df` shows 16 GB).

## Assumptions and areas that may need refinement

- **Sizes are extrapolated** from five measured sources (about 1 GB of downloads, largest
  pack about 190 MB). Step 3 measures all 23; a large departure goes back to the
  maintainer.
- **`ureq` forwarding `Range` across GitHub's redirect is unobserved.** Step 1 proves it
  or switches to resolving the redirect by hand.
- **Selected packs download before the prompt**, so a large pending download holds a
  cabinet at the console.
- **The refined row rule first applies to the release after v1.5**, because the v1.4
  updater performs the v1.5 update. One re-ordering case stays imperfect (design §4.13).
- **Console wording** follows the mock and is reviewed again on the first real build.
- **The release build fetches the catalog with `gh`** per the design. Now that the
  assets repository is public, an anonymous HTTPS download would work too and would not
  require `gh` on the Windows build host; decide in Step 12.
- **Bloat remains on GitHub until Step 13.** Nothing should be branched from `v1_5`
  before the squash.
