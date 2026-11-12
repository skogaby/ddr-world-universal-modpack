# Progress: Optional Content Packs

Updated: 2026-11-12
Status: Step 13 of 13 — in progress. Steps 1–3 and 5–12 are code complete and tested on
the host; Step 4 (publishing) and the hardware checks are waiting on the maintainer.
NEXT ACTION (maintainer): run `scripts/publish_packs.sh` (uploads 23 zips, about 1 GB,
and `catalog.json` to the assets repository's `packs` release). Then build a release
with `./scripts/build_release_archive.sh` — without `PACKS_CATALOG` it embeds the
published catalog — and run the checks under "Still to validate" below.

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

Per-step working records: `.agents/scratchpad/2026-11-11-optional-content-packs/<task>/`.

## In flight

Uncommitted in the code repository, for the maintainer to commit:
`updater/` (sources, tests, fixtures, `Cargo.toml`, `Cargo.lock`), `scripts/`
(`build_packs.py`, `test_build_packs.py`, `publish_packs.sh`, both release scripts,
`validate_background_dancers.sh`), `README.md`, `AGENTS.md`, `.gitattributes`,
`.agents/` (learnings, this planning folder, tasks, scratchpad).

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
- No cabinet or Windows run yet.

## Still to validate (maintainer; nothing below has been run)

1. `scripts/publish_packs.sh` for real, then a release build without `PACKS_CATALOG`.
2. A real pack download from GitHub by the Win7 updater build, including pulling the
   network mid-download and relaunching.
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
  checkout (`DDR_WORLD_ASSETS` overrides). Its `packs` release does not exist until the
  first publish.
- Working branch `v1_5`; `main` equals the v1.4 tag. `v1_5` reaches `main` only by
  `git merge --squash` (plan Step 13) — never rebase, fast-forward or an ordinary merge.
- The updater on v1.3 / v1.4 cabinets performs the next update: the base asset keeps its
  name pattern and stays under 2 GiB uncompressed (enforced by `base-image`).
- Updater tests: `cargo test --manifest-path updater/Cargo.toml`. The agent never
  commits, pushes or publishes.
- `prototypes/` is git-ignored on purpose.
