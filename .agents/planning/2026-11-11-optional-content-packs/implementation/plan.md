# Implementation Plan: Optional Content Packs

Status: Approved 2026-11-11

Design: `.agents/planning/2026-11-11-optional-content-packs/design/detailed-design.md`
(section numbers below refer to it). Decisions and research:
`.agents/planning/2026-11-11-optional-content-packs/idea-honing.md` and `research/`.

## Checklist

- [x] Step 1: Resumable downloads, proven against GitHub
- [x] Step 2: Move the ported models to the assets repository
- [x] Step 3: Pack build tool
- [x] Step 4: Publish packs to the shelf
- [x] Step 5: Catalog, markers and selection in the updater
- [x] Step 6: Pack transactions
- [x] Step 7: Packs in every run
- [x] Step 8: Network pack downloads
- [x] Step 9: The chooser
- [x] Step 10: The 5-second prompt and in-place lines
- [x] Step 11: Option-row placement refinement
- [x] Step 12: Release build
- [ ] Step 13: Documentation, first publication and history cleanup

## Conventions for every step

- The updater is its own crate. Validate with
  `cargo test --manifest-path updater/Cargo.toml`, format the whole crate with
  `cargo fmt --manifest-path updater/Cargo.toml`, and build the shipping binary with
  `ONLY=updater ./build_win7.sh`. The DLL crate is not touched by this plan.
- Commits, pushes and branch operations are the maintainer's. Steps name them as
  **Maintainer:** actions; the agent leaves work uncommitted and says what would be
  committed.
- No user name or machine-specific absolute path in any tracked file. Before handing a
  step back, `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no new hits.
- After each step, update
  `.agents/planning/2026-11-11-optional-content-packs/progress.md` (status, next action,
  deploy and test log).
- Existing updater tests keep passing unchanged unless a step says otherwise.

---

## Step 1: Resumable downloads, proven against GitHub

**Objective.** Settle the one unobserved fact the design rests on — that a ranged
request survives GitHub's redirect in `ureq` — and land the download primitive packs
will use.

**Implementation guidance.**
- Generalise `download::fetch_asset` as described in §4.7: a descriptor (name, URL,
  expected size, optional SHA-256) and a `resume` flag. `run_default` adapts the GitHub
  asset to the descriptor and passes `resume = false`, so the base download is unchanged.
- With `resume`: `.part` file, hash the existing bytes, `Range` request, the `206` /
  `200` / `416` handling, keep the part on a transport error, delete on a verification
  mismatch.
- If the live probe below shows the `Range` header is dropped on the redirect, resolve
  the redirect by hand (request with redirects disabled, read `Location`, send the
  ranged request there) and record the finding in `research/hosting-and-downloads.md`.

**Tests.**
- Unit, against an in-process `TcpListener`: fresh download, `206` append, `200`
  restart, `416` restart, redirect followed by a ranged response, size mismatch and
  digest mismatch delete the file, transport error keeps the part.
- One `#[ignore]` test that requests a byte range of the published v1.4 asset through
  the real URL and asserts the bytes equal the same range of a full download.

**Integration.** Nothing calls the resume path yet; the base path runs through the same
function.

**Demo.** `cargo test --manifest-path updater/Cargo.toml -- --ignored live_range` passes
on a networked machine, and a base update with the rebuilt updater behaves as before.

---

## Step 2: Move the ported models to the assets repository

**Objective.** Get the 3.5 GB out of the code checkout so every later tool targets one
layout. This is §8.2 steps 1–3; the squash and branch deletion wait for Step 13.

**Implementation guidance.**
- Clone `skogaby/ddr-world-universal-modpack-assets` beside the code checkout. Move every
  source folder under `data_mods/custom_models/dancers/` and `…/stages/` except `Custom`
  into it as `dancers/<Source>/` and `stages/<Source>/`. Add `.gitattributes` (`* -text`)
  and a short `README.md`. Do not copy `.DS_Store` files.
- Verify before anything is removed from git: file count and a sorted SHA-256 listing of
  the moved tree equal those taken from `git ls-files` content of the original paths.
- In the code repository: add the `.gitignore` rules of §4.4, and point the comments and
  defaults that name `data_mods/custom_models/dancers` as a port destination
  (`tools/blender_ddr_addon/examples/`, its `README.md`) at the assets checkout, resolved
  as in requirement P26.
- **Maintainer:** commit and push the assets repository; commit the removal on `v1_5`.
- Check free disk space first: the move itself needs none, but the assets repository's
  git objects add about 0.5 GB.

**Tests.**
- The verification listing above (kept as a command in `progress.md`, not as a file).
- `scripts/validate_background_dancers.sh` still passes (it reads no ported model).
- `scripts/build_release_archive.sh` still produces a zip; it now holds only the `Custom`
  models.

**Integration.** Later steps read models only from the assets checkout.

**Demo.** `git status` in the code checkout shows the ported folders gone and ignored;
the assets checkout holds 15 dancer and 8 stage sources; a release build produces a base
zip of a few tens of megabytes.

---

## Step 3: Pack build tool

**Objective.** Produce pack zips, markers and a local catalog, and replace the
extrapolated sizes with measured ones.

**Implementation guidance.**
- `scripts/build_packs.py` with the `build`, `stage` and `check` subcommands of §4.2
  (the `catalog` subcommand arrives with Step 4). It imports
  `scripts/pack_custom_models.py`; it locates the assets checkout per P26.
- Work one source at a time — pack, hash, write marker, zip, delete that source's
  mirror — so peak scratch space is one pack, not 3.3 GB. Output goes to `release/packs/`.
- Record the measured zip sizes and build time in
  `research/hosting-and-downloads.md` §5. If the total departs materially from 1 GB, or
  any pack approaches 2 GiB, raise it before continuing.
- Generate a tiny synthetic pack (two fake models, a sidecar) with the tool and commit
  it, its catalog entry and its expected content id under `updater/tests/fixtures/`.

**Tests.** `scripts/test_build_packs.py` (unittest, same style as
`scripts/test_pack_custom_models.py`): id derivation and id collisions; content id
stable across two builds and across changed file timestamps, different after a one-byte
change; marker contents; zip entry order with the marker last; `check` rejects each
malformed catalog; the committed fixture is reproduced exactly. Add the test to
`scripts/validate_background_dancers.sh` beside the packer's.

**Integration.** Uses Step 2's layout. The fixture feeds Steps 5–7.

**Demo.** `python3 scripts/build_packs.py build` prints 23 packs with ids, content ids
and sizes and leaves 23 zips plus a catalog in `release/packs/`; running it again yields
the same content ids; unzipping one pack into an empty folder shows its source folder and
its marker.

---

## Step 4: Publish packs to the shelf

**Objective.** Populate the `packs` release and produce the catalog a modpack build
embeds.

**Implementation guidance.**
- Add the `catalog` subcommand (§4.2) and `scripts/publish_packs.sh` (§4.3).
- `gh` is a new prerequisite on the build machine; the script says how to install and
  authenticate it when it is missing.
- The anonymous-reachability warning is expected while the repository is private.

**Tests.**
- `scripts/test_build_packs.py`: catalog composition from a fake shelf listing — all
  packs present; a pack missing from the shelf fails; an asset without a digest fails;
  published size and digest win over the local zip's.
- The script's `--dry-run` prints the uploads it would make without calling `gh` to
  upload.

**Integration.** Consumes Step 3's output. Its catalog is what Steps 5–10 are exercised
with by hand, and what Step 12 embeds.

**Demo.** **Maintainer:** run `scripts/publish_packs.sh`. The `packs` release shows 23
zips and `catalog.json`. A second run uploads no zip. Changing one file in one source
and running again uploads exactly one new zip and a new catalog.

---

## Step 5: Catalog, markers and selection in the updater

**Objective.** The updater can read everything it needs to know about packs, and show
it.

**Implementation guidance.**
- `updater/src/packs/{mod,catalog,state}.rs` with the data models of §5.1–5.3 and their
  validation. Paths for the selection file and the `packs/` work folder in `gamedir.rs`.
- `--packs-list` in `cli.rs`: prints each catalog pack with its state (not installed,
  installed, installed with another revision, selected) from local files only.

**Tests.** The `packs::catalog` and `packs::state` unit tests of §7, reading the Step 3
fixture; `cli` tests for the new flag; an end-to-end test that `--packs-list` on a
folder with no catalog prints a one-line explanation and exits 0.

**Integration.** Read-only; no existing path changes.

**Demo.** Copy the Step 4 catalog into a game folder as `ddr_world_hook_packs.json`, run
`ddr_world_hook_updater.exe --packs-list`: 23 packs, none installed. Unzip one pack by
hand, run it again: that pack shows as installed.

---

## Step 6: Pack transactions

**Objective.** Install, replace and remove one pack transactionally from a local zip —
the core end-to-end behaviour.

**Implementation guidance.**
- `archive.rs` pack mode (§4.8).
- Pack install and removal plans and the `apply::execute` transaction descriptor, with
  per-transaction backup folders, the journal's `backup_subdir`, the once-per-run backup
  wipe and the post-commit trim (§4.9). The base install goes through the same
  descriptor with `backup/base/`.
- `packs/install.rs`: obtain the zip from `--packs-dir`, verify it against the catalog,
  extract, plan, apply, delete the zip from the work folder.
- `--packs-install <all|none|ids>` and `--packs-dir <DIR>`: write the selection, remove
  deselected packs, install selected ones. No network yet: a pack absent from
  `--packs-dir` is reported as not available.
- The name-collision warning (§4.9, last paragraph).
- Bump the updater version to 0.2.0.

**Tests.**
- Unit: pack plans; archive confinement cases (§7).
- End to end with the fixture: install; install over an older revision with a dropped
  file; removal keeps a user-added file and a modified recorded file; a user file
  elsewhere under `custom_models/` is byte-identical afterwards; fault injection —
  rollback, failed rollback (exit 1), simulated crash then recovery; a base install
  followed by a pack install in one run leaves `backup/base/` intact; a journal without
  `backup_subdir` recovers from `backup/`.
- The existing `apply_e2e.rs` suite passes with only the backup path adjusted.

**Integration.** Uses Step 5's models and Step 3's fixture and real zips.

**Demo.** In a scratch game folder holding the catalog:
`--packs-install dancers-3rd-mix,stages-x-x2 --packs-dir release/packs` installs both;
`--packs-list` shows them; a file dropped into `dancers/3rd MIX/` by hand survives
`--packs-install none`, which removes everything else of both packs.

---

## Step 7: Packs in every run

**Objective.** A normal run keeps selected packs correct without being asked.

**Implementation guidance.**
- `packs/resolve.rs` (§4.6) and `packs::run_step`, called from `main.rs` after the base
  step for every outcome except a failed rollback (§4.5). The base step reports whether
  GitHub was reachable.
- Initial selection from markers when no selection file exists; adoption of new packs
  under `install_new`; removal of selected packs the catalog dropped; presence-and-size
  check each run, hashes under `--force`; `--check` raising the outcome when pack work
  is pending.
- The status line is computed here and logged (the prompt arrives in Step 10).

**Tests.**
- Unit: the `packs::resolve` table of §7.
- End to end, still with `--packs-dir`: `--from-zip` of a second base whose catalog
  names a new revision updates the pack in the same run; deleting or truncating a pack
  file is repaired on the next run; a same-size edit is repaired only with `--force`; a
  hand-unzipped pack with no selection file becomes selected; a failed pack leaves the
  base installed and exit code 0; `--check` exits 3 with pending pack work and changes
  nothing.

**Integration.** Wraps Step 6's transactions in the run sequence.

**Demo.** With two packs installed: delete one model file and launch the updater with
`--packs-dir` — the log shows the pack repaired. Install a base zip whose catalog was
rebuilt after changing a model — the log shows that one pack updated and the status line
"2 of 23 installed".

---

## Step 8: Network pack downloads

**Objective.** Packs come from the shelf, survive bad connections, and never hold a
cabinet on a dead network.

**Implementation guidance.**
- `packs/install.rs` uses Step 1's resume path when `--packs-dir` has no matching zip.
- Free-space check (§4.7); add the `Win32_Storage_FileSystem` feature of `windows-sys`.
- Network rules of §3.1: no pack downloads when the base step could not reach GitHub;
  the first transport failure ends pack downloads for the run. Stale part files are
  cleared at the start of the step.

**Tests.**
- End to end against the in-process listener from Step 1 (a test-only switch lets the
  catalog name a loopback `http` URL): install by download; a connection cut mid-way
  leaves a part file and the next run completes from it; a `404` fails one pack and the
  next still installs; a digest mismatch discards the download.
- Unit: the free-space decision with an injected probe; the "network unusable" rule.

**Integration.** Completes Step 7's run step; `--packs-dir` remains for testers.

**Demo.** Requires the assets repository to be public (assumption A1). If it is by now:
`--packs-install dancers-3rd-mix` in a scratch folder downloads from GitHub; pulling the
network cable mid-download and relaunching resumes instead of restarting. If it is still
private, the listener tests stand in and this demo is run in Step 13.

---

## Step 9: The chooser

**Objective.** Users can pick packs interactively.

**Implementation guidance.**
- `console::LineReader` (§4.10) replacing the one-shot reader; the existing
  close-on-Enter wait uses it.
- `packs/chooser.rs` (§4.11): pure parsing and rendering, and the loop with the 5-minute
  idle cap. Wording follows the mock under `prototypes/`, reviewed with the maintainer
  when the first build runs.
- Entry points: `--packs`, and a run that is alone on its console (§3.1). Applying saves
  the selection, then runs removals and installs through Step 7's machinery.

**Tests.** Unit: input parsing (numbers, ranges, `d` `s` `a` `n` `f` `q`, junk), the
rendered screen for a fixed state, the apply summary, idle timeout applies nothing.
End to end with scripted standard input: select two packs and confirm; decline at the
confirmation; end of input leaves everything unchanged. `LineReader`: several timed
reads in sequence from one stream.

**Integration.** Sits after Step 7's run step; uses Step 8 for downloads.

**Demo.** `ddr_world_hook_updater.exe --packs` in a scratch game folder: tick `1-3 21`,
Enter, confirm; the packs install and `--packs-list` agrees. Open it again and untick
one; it is removed.

---

## Step 10: The 5-second prompt and in-place lines

**Objective.** Every normal run offers the chooser for 5 seconds and shows progress on
one line.

**Implementation guidance.**
- The timed prompt of §4.10 and the conditions of P13–P15, wired after the pack step.
- The live line in `log.rs` / `console.rs`: countdown and download progress redrawn in
  place, at most 79 columns, absent when standard output is not a terminal; the log file
  gets start and result lines only. Base and pack downloads both use it.

**Tests.** Unit: the prompt decision for a line, end of input, a read error and a
timeout; which invocations prompt (table over the flags and catalog states of P14); live
line padding and the width cap for the longest pack title in the catalog; log file
content for a download contains exactly two lines. End to end: a run with standard input
closed finishes without a 5-second delay.

**Integration.** Enter hands over to Step 9's chooser.

**Demo.** Launch from a `gamestart.bat` copy: the status line appears, the countdown
ticks down in place and the batch file continues after 5 seconds; launching again and
pressing Enter opens the chooser. A pack download shows one advancing line. Repeat on
Windows 7 and under CrossOver when the build is deployed there.

---

## Step 11: Option-row placement refinement

**Objective.** A missing row lands beside the row it is named after when the user has
re-ordered.

**Implementation guidance.** The four-point rule of §4.13 in
`updater/src/merge/option_menu.rs`; update the module comment.

**Tests.** The existing tests unchanged, plus: the two re-ordered cases of §4.13's table;
the v1.3 and v1.4 default lists (committed as fixtures) against the repository's
`mod-config.json`; every two-row removal of the committed list round-trips (the suite
has single and adjacent-pair removals today).

**Integration.** Self-contained; reached through the existing config merge.

**Demo.** Take the v1.4 `mod-config.json`, move `ddr_selection` to the top of its
section, install the current base with `--from-zip`: `background_dancer_source` sits
directly above `background_dancer`, and no existing row moved.

---

## Step 12: Release build

**Objective.** A release build produces the base zip with the catalog inside, and
testers can install a private build with packs.

**Implementation guidance.** The changes of §4.4 to
`scripts/build_release_archive.sh` (staging limited to `Custom`, catalog download or
`PACKS_CATALOG`, `build_packs.py check`, the 2 GiB guard, the reachability warning, the
tester `.bat`'s `--packs-dir`) and to `scripts/build_release_distribution.bat`
(`build_packs.py stage` when the assets checkout is present). Update the scripts' header
comments.

**Tests.**
- A build with `PACKS_CATALOG` set to a deliberately broken catalog fails; with no
  catalog obtainable it fails.
- A stray ported source folder placed under `data_mods/custom_models/dancers/` does not
  appear in the zip.
- The built zip contains `ddr_world_hook_packs.json` byte-identical to the shelf's
  `catalog.json`.
- The updater's end-to-end suite runs once against a zip produced by the script (manual
  invocation recorded in `progress.md`).

**Integration.** Embeds Step 4's catalog; the tester path exercises Steps 6–10.

**Demo.** `./scripts/build_release_archive.sh` yields a base zip, the bare executables
and the tester `.bat`. Copying those plus a `packs/` folder with two pack zips into a
v1.4 game folder and running the `.bat` installs the base; launching the updater again
shows the prompt, and choosing those two packs installs them from the folder.

---

## Step 13: Documentation, first publication and history cleanup

**Objective.** Ship it, and finish removing the models from the code repository's
history.

**Implementation guidance.**
- `README.md` per §4.14. Regenerate `.agents/summary/` with the codebase-summary
  workflow; add to `AGENTS.md` only what an agent needs on every task (the assets
  checkout lookup, the two new scripts, "ported models are not in this repository").
  Add any trap met during implementation to `.agents/learnings/learnings.md`.
- Draft release notes covering §8.1: packs and how to choose them, the v1.4 loose
  folders becoming `Custom/` arcs, and the modified-folder key collision.
- **Maintainer**, in this order:
  1. Run the manual matrix of §7 on Windows 10/11, Windows 7 and CrossOver, including
     the v1.4 → v1.5 path with the v1.4 updater and `--repo` pointed at a test
     repository, and any demo deferred from Step 8.
  2. Make the assets repository public; confirm the catalog URL opens in a private
     browser window.
  3. `scripts/publish_packs.sh`.
  4. Squash `v1_5` into `main`, push, delete `v1_5` locally and on GitHub (§8.2 steps
     4–6).
  5. Build and publish v1.5 from `main`.

**Tests.** The manual matrix above, logged in `progress.md`. After the branch deletion:
a fresh clone of the code repository is about 100 MiB and
`git log --all --oneline -- 'data_mods/custom_models/dancers/STRIKE'` is empty.

**Integration.** Everything before it.

**Demo.** A v1.4 cabinet with the stock `gamestart.bat` line updates to v1.5 on one
launch; the next launch shows the 5-second prompt; choosing 3rd–5th MIX dancers and the
SuperNOVA stages downloads them, and the game lists exactly those sources plus `CUSTOM`.
