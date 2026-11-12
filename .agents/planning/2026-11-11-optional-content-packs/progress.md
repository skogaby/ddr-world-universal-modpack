# Progress: Optional Content Packs

Updated: 2026-11-12
Status: Step 13 of 13 — in progress. Everything is implemented, the packs are published
and `v1_5` has been squashed into `main`. Remaining: the hardware checks and the v1.5
release itself.
NEXT ACTION (maintainer): run the checks under "Still to validate", then build with
`./scripts/build_release_archive.sh` (it embeds the published catalog) and publish v1.5
from `main`.

Resume protocol: read `summary.md` → `implementation/plan.md` (checklist) →
`design/detailed-design.md` for the section a step cites. Decisions are in
`idea-honing.md`.

## Done

- Planning: register accepted, design approved, plan approved (2026-11-11).
- Step 1: resumable downloads (`download::fetch`); Range across GitHub's redirect
  confirmed live.
- Step 2: 31 536 model files moved to the assets repository, verified by git blob id.
  Committed and pushed by the maintainer.
- Step 3: `scripts/build_packs.py`; 23 packs measured (1 036 MB of zips, 3 450 MB
  installed, largest 184 MB); fixture under `updater/tests/fixtures/packs/`.
- Step 5: `updater/src/packs/{catalog,state}.rs`, `--packs-list`.
- Step 6: pack archives, pack plans, transaction descriptor, per-transaction backups,
  `--packs-install`, `--packs-dir`. Updater version 0.2.0.
- Step 7: `packs/{resolve,step}.rs`; the pack step runs after the base step.
- Step 8: pack downloads with resume, free-space check, dead-network rule.
- Step 9: `console::LineReader`, `packs/chooser.rs`, `--packs`.
- Step 10: the unconditional 5-second prompt; in-place countdown and progress lines.
- Step 11: option-row refinement in `merge/option_menu.rs`.
- Step 12: `build_packs.py base-image`; both release scripts; base zip 30.5 MB.
- Step 13 (agent part): `README.md`, `AGENTS.md` Custom Instructions,
  `.agents/learnings/learnings.md`, `release-notes-draft.md`.
- Step 4: packs published by the maintainer (2026-11-12). The catalog downloads
  anonymously (23 packs, 1 036.4 MB) and all 23 pack URLs answer with the catalog's
  sizes.
- Step 13, history: `v1_5` (52 commits, tip `d837389e`) squashed into `main` as
  `1390301a`, pushed; `v1_5` deleted locally and on GitHub; local stashes cleared. A
  fresh clone is 103 MiB (was about 615 MiB) and holds no ported-model history.

Per-step working records: `.agents/scratchpad/2026-11-11-optional-content-packs/<task>/`.

## In flight

Uncommitted on `main`: this file and the Step 4 tick in `implementation/plan.md`.

## Deploy & test log

- 2026-11-12, host: `cargo test --manifest-path updater/Cargo.toml` — 200 unit (1
  ignored: the live GitHub test, run and passing in Step 1), 17 + 9 + 33 end-to-end, all
  green; `cargo fmt --check` clean; Win7 updater build clean, no `ProcessPrng`.
- 2026-11-12, host: `scripts/test_build_packs.py` 26 green;
  `scripts/validate_background_dancers.sh` OK.
- 2026-11-12, host: `scripts/publish_packs.sh --dry-run` against the real, empty shelf:
  23 uploads listed, nothing uploaded.
- 2026-11-12, host: `PACKS_CATALOG=release/packs/catalog.json
  ./scripts/build_release_archive.sh` OK; the zip was installed into a scratch game
  folder with the host-built updater, two real packs installed from `release/packs`,
  then one swapped for another through the chooser with scripted input.
- 2026-11-12, host: a real pack download from GitHub by the host-built updater
  (`--packs-install stages-x-x2`, no `--packs-dir`): downloaded, verified against the
  catalog's digest and installed.
- No cabinet or Windows run yet.

## Still to validate (maintainer; nothing below has been run)

1. A full release build without `PACKS_CATALOG` (only its catalog step was run against
   the published catalog).
2. A pack download by the Win7 updater build on Windows, including pulling the network
   mid-download and relaunching (the host build has downloaded one pack from GitHub).
3. On a real console — Windows 10/11, Windows 7, CrossOver: the countdown and the
   progress bar redraw in place; Enter opens the chooser; a double-click opens it
   directly; the wording reads well.
4. The Windows free-space check (compiled, never executed).
5. `scripts\build_release_distribution.bat` on Windows (edited, never executed).
6. The game with no packs, some, and all: source rows list the installed sources.
7. v1.4 → this build using the v1.4 updater binary and `--repo` on a test repository.
8. The eleven Blender port examples changed in Step 2 (byte-compiled only).

## Deviations & open questions

- **Release builds fetch the catalog by anonymous HTTPS**, not `gh release download` as
  the design says. The repository is public, and this needs no `gh` on the Windows build
  host. `gh` is needed only to publish.
- **The status line says "Optional packs: …"**, not "Optional dancer and stage packs".
- **Applying a selection removes only packs the installed release offers.** An installed
  pack the release dropped, and that is not selected, is left alone (it was not on the
  list to untick). A SELECTED dropped pack is removed, as designed.
- **Closing the chooser unchanged records "seen" packs only when a selection file
  already exists**; it does not create one.
- **No log line when the prompt times out.**
- **The 10 % progress log lines of the base download are gone** (console progress is the
  live line; the log keeps the start and result lines), as the design's P18 asks.
- `.agents/summary/` has not been regenerated; it does not know about packs yet.
- The nightly toolchain updated itself to 1.102.0-nightly on 2026-11-11; cargo warns
  that the `trim-paths` cargo feature no longer needs listing in the manifests.
- Two hangs during development were both in the test harness of
  `updater/tests/packs_e2e.rs` (a mutex re-locked inside a `match`, and a stdin pipe not
  closed), not in the updater. Every updater run in that file is now killed after 60 s.
  That kill path has not itself been exercised.

## Key facts for a cold resume

- Assets repository: `skogaby/ddr-world-universal-modpack-assets`, public, a sibling
  checkout (`DDR_WORLD_ASSETS` overrides). Its `packs` release holds the 23 pack zips
  and `catalog.json`.
- `main` is the working branch and holds v1.5 development as one commit on top of the
  v1.4 tag. `v1_5` no longer exists. The local checkout still stores the old objects
  until the reflog expires or `git gc --prune=now` is run after
  `git reflog expire --expire=now --all`; until then `d837389e` is recoverable locally.
- The updater on v1.3 / v1.4 cabinets performs the next update: the base asset keeps its
  name pattern and stays under 2 GiB uncompressed (enforced by `base-image`).
- Updater tests: `cargo test --manifest-path updater/Cargo.toml`. The agent never
  commits, pushes or publishes.
- `prototypes/` is git-ignored on purpose.
