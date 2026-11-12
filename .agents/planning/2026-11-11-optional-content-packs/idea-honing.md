# Decision register: optional content packs

Status of the register: **accepted by the maintainer 2026-11-11** (round 5). Rounds 1–3
settled D1–D20 (D7, D9, D19 overridden); round 4 moved the assets to a separate
repository (D3 and D13 revised, D21–D25 added); round 5 accepted those and named the
repository. `Overridden` rows carry the maintainer's answer, not the original
recommendation. `Assumed` rows were settled by the agent and are open to correction.
Findings behind these decisions: `research/orientation.md`,
`research/hosting-and-downloads.md`, `research/existing-code.md`.

`★` = a decision the rough idea did not raise.

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Pack unit | Sets what a user can choose and how much re-downloads when one model changes | One pack per ported source folder per kind (23 today); a pack is a zip rooted at the game folder | Accepted |
| D2 | What stays in the base zip | Decides whether v1.4 users lose content and whether dancers work out of the box | Base keeps everything except the ported sources; the `Custom` sources stay in the base | Accepted |
| D3 ★ | Where pack zips are hosted | Decides the per-release workflow and upload cost | Revised round 4: a file-shelf release in a SEPARATE content repository; assets named by content id; upload only when a pack changes | Accepted |
| D4 ★ | Catalog: where the updater learns what packs exist | Ties pack versions to DLL versions; decides API usage and offline behaviour | `ddr_world_hook_packs.json` generated at build time and shipped INSIDE the base zip, with a direct URL per pack | Accepted |
| D5 ★ | Where selection and pack ownership are recorded | An old updater prunes anything listed in the main manifest that the base zip lacks | Selection in an updater-owned file; ownership as one marker file per pack, shipped inside the pack zip; main manifest untouched (schema 1) | Accepted |
| D6 | How the user chooses | The updater normally runs unattended inside `gamestart.bat` | Console checklist, reached from the timed prompt (D7), a double-click, or `--packs`; flags for scripting | Accepted |
| D7 ★ | What a `gamestart.bat` run does with packs | The game does not start until the updater exits, and a cabinet may have no keyboard | Base update → update/repair selected packs → a 5-second prompt (D19) that times out into "start the game" | Overridden |
| D8 ★ | Packs that appear in a later release | Explicit selections go stale as new mixes are ported | Selection is "everything incl. future", "nothing", or an explicit list; with a list, new packs are named in the prompt's status line, not installed | Accepted |
| D9 | Protecting local models | Stated requirement | Paths the modpack never shipped are never touched; paths it ships are its own and are restored when missing or modified; no repair command | Overridden |
| D10 ★ | v1.4 → v1.5 transition | The old updater performs that update | Base asset keeps its name pattern and stays < 2 GiB uncompressed; pack assets use a different prefix; pack handling is evaluated even when the base is current | Accepted |
| D11 | Option-row placement | Stated requirement; already implemented for default-ordered configs | Keep the algorithm; add a tie-break so a new row follows the neighbour it is named after | Accepted |
| D12 ★ | Users who never run the updater | Their DLL shows unlisted rows at the end of the menu | Out of scope here | Accepted |
| D13 | Release tooling | The maintainer runs it for every release | Build script emits base zip + pack zips + catalog and says which packs are not on the shelf yet; a `gh`-based publish script uploads those; the build refuses a catalog that names an unpublished pack. Revised round 4: see D22 | Accepted |
| D18 ★ | How "missing or modified" is detected on every launch | Hashing ~3.5 GB of installed packs each boot would add tens of seconds on a cabinet disk | Every run checks presence + size against the marker; full hashes only with `--force` or when a pack's revision changes | Accepted |
| D19 ★ | When the timed prompt appears | Every launch pays the wait | Unconditionally on every run, with a 5-second wait; revisit if users complain | Overridden |
| D21 ★ | What lives in the content repository | Decides whether the main repository stops carrying 3.5 GB of models | The ported source folders (git-tracked authoring form) AND the pack zips (release assets there); the `Custom` sources stay in the main repository | Accepted |
| D22 ★ | How a modpack build learns the catalog | Decides whether cutting a release needs the 3.5 GB checkout | Publishing packs also uploads `catalog.json` to the shelf; the modpack build downloads it and embeds it in the base zip | Accepted |
| D23 | Where the content checkout sits for tools | Port scripts and the pack scripts need to find it | Sibling checkout by default, `DDR_WORLD_ASSETS` to override, no absolute fallback; pack/publish scripts stay in the main repository | Accepted |
| D24 ★ | Removing the models from the main repository's history | A plain delete-then-merge keeps every blob reachable forever | Squash-merge `v1_5` into `main` after deleting the ported sources, then delete `v1_5`; maintainer-run | Accepted |
| D25 | Progress display | Maintainer feedback on the mock | Countdown and download progress redraw one console line in place; the log file gets a start and an end line | Assumed |
| D26 ★ | Visibility of the assets repository | Release files of a private repository cannot be downloaded without credentials, and cabinets have none | Must be public before v1.5 is published. Made public by the maintainer 2026-11-11 (anonymous API request returns 200) | Accepted |
| D27 | Installed vs selected | A pack unzipped by hand, or a full image, has markers but no selection | With no selection file, the selection starts as "the packs whose markers are present"; afterwards unattended runs act only on selected packs and the chooser shows installed ones ticked | Assumed |
| D28 | `--check` and packs | `--check` exits 3 when "an update is available" | Also exits 3 when a selected pack needs installing or repairing | Assumed |
| D29 | Windows image script without the assets checkout | D17 promised a full image | Full image when the assets checkout is present, base-only image with a notice otherwise | Assumed |
| D14 | Download robustness | Packs are up to ~190 MB over arcade-venue connections | Resume partial downloads; one pack per transaction; free-space check | Assumed |
| D15 | Scope of the pack mechanism | Future optional content (themes, movies) | Catalog is generic (id, group, file set); only dancers and stages are produced now | Assumed |
| D16 | Tester builds | `install-update-*.bat` installs private builds with `--from-zip` | `--packs-dir <folder>` reads pack zips locally instead of from their URLs | Assumed |
| D17 | Windows image script | `build_release_distribution.bat` stages an unzipped full image | Unchanged: full image with every pack, markers included | Assumed |
| D20 | Prompt input | "Press a key" needs raw console input; a line read works everywhere | "Press Enter" (line-based, same mechanism as today's close prompt); revisit with the prototype if any-key is wanted | Assumed |

---

## D1 — Pack unit

**Question.** What is one selectable thing?

**Recommendation.** A pack is one ported source folder of one kind: `dancers/3rd MIX`,
`stages/SUPERNOVA 1 & 2`, … — 15 dancer packs and 8 stage packs today (the two `Custom`
sources are base content, D2), 4 MB to ~190 MB zipped. The zip is rooted at the game
folder (`data_mods/custom_models/dancers/3rd MIX/…`), so unzipping by hand still works.

**Rationale.** It is the unit the game already shows (DANCER SOURCE / STAGE SOURCE rows)
and the unit of the example in the idea. Rejected: per-model assets (~785, near GitHub's
1000-per-release cap, and a chooser nobody would read); two packs "all dancers / all
stages" (no per-mix choice); one pack per mix spanning both kinds (source names differ
between kinds — `HOTTSTPARTY 1-3` dancers vs `HOTTEST PARTY 1`…`5` stages — and the
idea's example wants dancers of one mix with stages of another).

## D2 — What stays in the base — Accepted

The base zip is today's zip minus the ported sources. It keeps the `Custom` dancer and
stage sources (the maintainer's own models, ~107 MB raw, roughly 30 MB zipped),
consistent with v1.4, which shipped five of those dancers and both stages.

## D3 ★ — Hosting

**What a "content release" is.** A GitHub release is a tag plus a list of downloadable
files. The proposal uses ONE extra release (tag `content`) purely as a shelf: every pack
zip ever published sits there under a name that includes its content id, e.g.
`ddr-world-pack-dancers-3rd-mix-a1b2c3d4.zip`. It is created once with "set as latest"
off, is never re-published, and is not a modpack version. Modpack releases (v1.5,
v1.6, …) keep carrying exactly one zip.

**How a cabinet finds a pack.** It never searches. The base zip of each modpack release
contains the catalog (D4), which lists every pack with its full download URL on the
shelf. The updater installs the base, reads the catalog from the game folder, and
downloads from those URLs.

**Worked example.**

| Release | What changed in the models | Build + publish | A cabinet with `3rd MIX` selected |
|---|---|---|---|
| v1.5 | first release with packs | 23 ids computed, none on the shelf → 23 zips uploaded once; v1.5 = one zip, as today | installs base, downloads `…3rd-mix-a1b2c3d4.zip` |
| v1.6 | nothing (DLL hotfix) | same 23 ids, all on the shelf → nothing uploaded; v1.6 = one zip | installs base; catalog names the same id its marker records → no pack download |
| v1.7 | one 3rd MIX dancer fixed; DDR EXTREME ported | 3rd MIX gets a new id, EXTREME is new → 2 zips uploaded; v1.7 = one zip | installs base; marker id ≠ catalog id → downloads `…3rd-mix-e5f6a7b8.zip`; EXTREME is offered as new |

**Alternatives.**
- **A. Attach every pack to every modpack release.** Everything for a release is on its
  own page and there is no shelf. Costs ~1 GB of upload and ~24 assets per release,
  hotfixes included (scripted with `gh`, so the cost is time and storage, not effort).
  Pack assets would still be named by content id so cabinets skip unchanged ones.
- **C. Attach a pack to the modpack release in which it last changed**, with later
  catalogs pointing back at older releases. No shelf and no re-upload, but pack files end
  up scattered over old release pages, and deleting an old release breaks newer ones.
- **D. A separate repository for content.** B with a second repository.

**Rationale for B.** Releases have been frequent (five in a month) and the ports change
rarely; B uploads a pack exactly once per revision and keeps a release page as it is
today. People installing by hand follow one link (README and release notes) to the
shelf.

**Settled.** Uploads are scripted with the `gh` CLI (accepted in round 1).

**Guards added by research** (`research/hosting-and-downloads.md` §2). GitHub documents
"latest" two ways, so the shelf is created with "set as latest" off AND tagged on the
repository's oldest commit AND published before v1.5; the publish script then checks
that `/releases/latest` is still a modpack release and fails loudly otherwise. If the
shelf ever did become "latest", v1.3/v1.4 cabinets would skip updates until it was
corrected.

### Round 4 revision — separate content repository

The maintainer is open to a separate repository, mainly to get the models out of the
main repository as well. The shelf then lives in the content repository's releases.
Everything above about content ids, the worked example and "the updater never searches"
is unchanged; only the URL host differs. The "latest release" risk and its four guards
disappear, because nothing ever asks the content repository for its latest release.

## D4 ★ — Catalog

**Recommendation.** The build writes `ddr_world_hook_packs.json` into the base zip
(game-folder root). Per pack: id, kind, display title, download URL, zip size and
sha256, installed size, and a content id. The updater reads the catalog from the game
folder after the base step.

**Rationale.** Each base release pins the exact pack revisions it was built with, so a
DLL never runs against models from a different revision. No extra API request (pack
downloads are plain HTTPS GETs, outside the 60-per-hour unauthenticated API limit). It
works for `--from-zip` and for the first launch after the old updater installed v1.5.
Consequence: new or changed packs reach users with the next modpack release, never on
their own. Rejected: a catalog fetched from the network independent of the base.

## D5 ★ — Selection and ownership records

**Recommendation.**
- Selection: `ddr_world_hook_updater.packs.json` in the game folder, written by the
  chooser, readable and editable by hand.
- Ownership: each pack zip carries `data_mods/custom_models/.packs/<pack-id>.json`
  listing its content id and its files with size and hash. The scanner already skips
  dot-directories. Installed by the updater or unzipped by hand, the same marker lands
  on disk, so a hand-installed pack is recognised and later updated or removed correctly.
- The main manifest keeps schema 1 and never lists pack files.

**Rationale.** See orientation §3.3: anything in the main manifest that the base zip
lacks gets pruned. Rejected: a `packs` section in `mod-config.json` (the DLL and the
additive config merge both own that file's shape; the updater would become a third
writer).

## D6 — Choosing

**Recommendation.** A plain line-based console checklist (no cursor control, so it works
on Windows 7 consoles and under Wine): packs grouped as Dancers / Stages with size and
state (installed, update available, new), toggled by number, `a` = all, `n` = none,
Enter = apply. Three ways in: answering the timed prompt of D7, double-clicking the
updater (the existing alone-on-console detection, which opens the checklist directly
after the update steps), or `--packs`. Downloads start in that
same session. Non-interactive equivalents: `--packs-install all|none|<id,id,…>`,
`--packs-list`. Once inside the chooser each question has a long idle cap, after which
nothing is applied and the updater exits.

**Alternatives.** An in-game overlay-menu page that records a selection for the next
launch (a second feature — proposed as a later phase). Hand-editing the selection file
(supported by D5 regardless).

**Prototype.** `prototypes/mock_updater.py` (fake data; the folder ignores itself in
git at the maintainer's request, so it is never committed). Scenarios: `boot`, `first`,
`update`, `chooser`; `--fast` removes the simulated delays. Feedback on wording and
layout is still open.

## D7 ★ — A `gamestart.bat` run — Overridden (rounds 1 and 3)

**Maintainer's direction.** The README's `gamestart.bat` setup makes the game wait for
the updater, so the updater must not wait on input every run; cabinets often have no
keyboard. When it does present pack options — after the core update — it waits 10
seconds, and with no input it closes and lets the game start.

**Resulting behaviour.**
1. Base update, as today.
2. Every selected pack is brought to the catalog's revision and repaired if damaged
   (D9, D18). These downloads do hold the game until they finish; a failed pack is
   reported, retried next launch, and never blocks the game or rolls back the base.
3. Every run: one status line (how many packs are installed, which are new in this
   release) and "Press Enter within 5 seconds to choose packs" (D19, D20). Enter opens
   the chooser (D6); silence means "not now" and the updater exits.

## D8 ★ — New packs later

**Recommendation.** Three selection modes: everything including future packs, nothing,
or an explicit list. With an explicit list, a pack new in a release is named in the
prompt's status line and marked NEW in the chooser, not installed.

## D9 — Protecting local models — Overridden (round 1)

**Maintainer's direction.** The concern is net-new models the repository does not
track: never delete or alter them. Tracked packs and models are the modpack's own; no
explicit repair option — if a selected pack's files were deleted or modified, the
updater restores them.

**Resulting rules.**
1. A path the modpack does not ship (not in the base zip, not in a selected pack) is
   never read, written or removed — including files the user adds inside a shipped
   source folder such as `dancers/3rd MIX/`. This is today's rule, kept.
2. A path the modpack ships is its own: missing or modified files of the base and of
   selected packs are restored. The replaced copy goes to the updater's backup folder
   until the next update, as today.
3. Deselecting a pack, or a release dropping a file, removes the recorded files and
   nothing else; the source folder stays if the user has files in it. A recorded file
   the user modified is kept and reported rather than deleted (today's rule for dropped
   files).
4. `data_mods/_cache/` is never touched.

**Not solvable by file rules.** A user model whose key (`pl_<key>`) equals a newly
shipped one: the DLL keeps the first and warns. The updater reports a same-named arc in
a different folder as a warning only.

## D10 ★ — The v1.4 → v1.5 transition

**Constraints to accept.**
- The base asset keeps the `ddr-world-universal-modpack-*.zip` name and stays under
  2 GiB uncompressed.
- Pack assets use the prefix `ddr-world-pack-`.
- The new updater evaluates packs on every run, including when the base is current —
  its first run happens after the old updater already installed v1.5.
- The old updater removes v1.4's loose `dancers/<Friendly>/…` folders (unmodified ones)
  and installs `Custom/<Friendly>/…` arcs from the base. A v1.4 model folder the user
  modified stays and then collides by key with the new arc; the DLL keeps one and warns.
  Accept and document in the release notes.

## D11 — Option-row placement

**Finding.** The requirement is already met for configs in the default order
(orientation §4): v1.3 → HEAD and v1.4 → HEAD both reproduce the canonical list exactly.

**Recommendation.** Keep the section-aware algorithm and change one thing. When the
nearest preceding canonical sibling and the nearest following canonical sibling are not
adjacent in the user's list, place the new row beside the one whose id shares the longer
prefix with it (ties keep today's behaviour: after the preceding one). Examples:
`background_dancer_source` goes before `background_dancer` wherever the user moved it;
`assist_tick_volume` still goes after `assist_tick`.

**Alternative.** An explicit hint field on canonical rows (`"near": "<id>"`): exact, but
adds a field the DLL must tolerate and the maintainer must remember to set.

**Checked while preparing the design.** A transcription of the rule (comparing whole
`_`-separated leading words, and looking for the following neighbour anywhere in the
user's list) reproduces the canonical list for every one- and two-row removal, for the
v1.3 and v1.4 defaults, and fixes both re-ordered cases of orientation §4. A second
candidate rule ("rows with no later neighbour go to the section end") broke 33 of 1826
round-trips and was dropped. Known limit kept: a user who moved a section's last row
elsewhere in the section gets new tail rows after that row, not at the section end.

**Limit.** The v1.4 → v1.5 merge runs in the old updater, so the tie-break first applies
to the release after v1.5. Default-ordered configs are correct in v1.5 regardless.

## D12 ★ — Users without the updater

**Recommendation.** Out of scope. A user who keeps an old `mod-config.json` and never
runs the updater gets unlisted rows at the end of the menu, except `ShowWhen` families
(which includes the dancer/stage source rows), which already stay beside their relative.
Teaching the DLL the canonical order is a separate change to `ordering.rs`.

## D13 — Release tooling

**How "changed" is determined.** Nothing is tracked by hand and git history is not
consulted. On every build the script packs each ported source exactly as it does today
(`scripts/pack_custom_models.py` on the staged copy), hashes every resulting file, and
derives the pack's **content id** from the sorted list of `path + file hash`. The same
models always give the same id; touching any file in the source gives a new one. The
asset name contains the id, so "changed" simply means "no asset with this name is on the
shelf yet" (one `gh release view content` call).

**Recommendation.**
- `scripts/build_release_archive.sh` stages the base without the ported sources, builds
  each pack zip, writes the catalog, and prints which packs are new to the shelf. For a
  pack already on the shelf the catalog records the published asset's size and sha256
  (a rebuilt zip can differ byte-for-byte while holding identical files).
- New `scripts/publish_packs.sh` uploads the new pack zips with `gh release upload`,
  creating the `content` release on first use.
- The build fails if the catalog names a pack that is not on the shelf, unless
  `ALLOW_UNPUBLISHED=1` (tester builds, see D16) — so a modpack release can never point
  at a missing file.
- Superseded pack assets are deleted by hand or left; nothing depends on them once every
  cabinet has moved to a newer base.

**Verified in research** (`research/hosting-and-downloads.md` §4). Packing is
byte-deterministic on one host. Across hosts it is not, yet: the model folders hold 845
LF text sidecars and nothing stops a Windows checkout from converting them to CRLF. The
design adds `data_mods/custom_models/** -text` to `.gitattributes`.

## D21 ★ — What lives in the content repository

**Recommendation.** One new repository (`skogaby/ddr-world-universal-modpack-assets`,
created by the maintainer in round 5, public since 2026-11-11 — see D26) holds two things:
- in git: the ported source folders in their authoring form, laid out as
  `dancers/<Source>/…` and `stages/<Source>/…` (the shape of a `custom_models` base);
- as release assets: the pack zips and `catalog.json` (the shelf).

The `Custom` sources stay in the main repository under `data_mods/custom_models/`
(base content, D2; 16 MiB of its history). The main repository ignores every other
folder under `data_mods/custom_models/{dancers,stages}/`.

**Facts** (measured 2026-11-11). The content tree is 3.5 GB and about 500 MiB as a git
pack — within GitHub's guidance for a repository (under 1 GB ideal, under 5 GB strongly
recommended) and under the 2 GB single-push limit. No file exceeds 40 MB (GitHub blocks
files over 100 MB). Rejected: Git LFS (GitHub meters LFS storage and bandwidth), a
submodule (ties every clone of the code to the content).

**Starting point.** A fresh repository with the current folders as its first commit; the
49 commits of model history on `v1_5` are not carried over.

## D22 ★ — How a modpack build learns the catalog

**Recommendation.** Publishing is a step run in the content checkout whenever models
change: pack every source, derive content ids, upload zips that are new to the shelf,
then upload `catalog.json` (ids, URLs, sizes, sha256) replacing the previous one. File
lists stay in each pack's marker, not in the catalog. The modpack release build downloads the current `catalog.json` and embeds it in the
base zip as `ddr_world_hook_packs.json` (D4). It fails if the download fails, unless a
local catalog is supplied (tester builds, D16).

**Consequence.** Cutting a modpack release needs neither the content checkout nor the
packing step — the release build gets faster than today. A release pins whatever was
published at build time, so new models are published BEFORE the release that should
carry them.

This replaces the "build refuses an unpublished pack" check of D13: a catalog downloaded
from the shelf cannot name a pack that is not there.

## D23 — Where the content checkout sits

**Recommendation.** Tools look for the content checkout beside the main one
(`../ddr-world-universal-modpack-assets`), overridable with `DDR_WORLD_ASSETS`, with
no absolute fallback — the pattern already used for `DDR_WORLD_INSTALL` and the sibling
`ddr-chart-tools` / `bemaniutils` checkouts. `scripts/pack_custom_models.py` and its
tests stay in the main repository (they mirror DLL rules and are harness-tested there);
the new build-packs and publish scripts sit beside them and operate on the content
checkout. The Blender port scripts (`tools/blender_ddr_addon/examples/`) write ported
sources into the content checkout through the same lookup.

## D24 ★ — Removing the models from the main repository's history

**Facts** (measured 2026-11-11).
- `main` equals the v1.4 tag and holds 16 MiB of custom models (the `Custom` ones).
- All of the bloat — 501 MiB packed, 3.5 GB raw — is in the 49 commits of `v1_5` that
  `main` does not have. `v1_5` is pushed; it is the only remote branch containing them.
  No release tag points into those commits.

**The maintainer's proposed sequence, and the one step that must change.** Copy the
assets to the new repository → delete them from `v1_5` → bring `v1_5` into `main` →
delete `v1_5`. Bringing `v1_5` in by rebase, fast-forward or ordinary merge would make
the 49 commits part of `main`: the delete commit only removes the files from the newest
snapshot, the earlier commits still hold every blob, and deleting the branch afterwards
changes nothing because `main` now reaches them.

**Recommendation.** Bring `v1_5` in as a SQUASH (`git merge --squash`): one new commit on
`main` holding the net difference, which after the delete contains no ported models.
Then delete `v1_5` locally and on the remote. No force-push, no rewritten tags.
- Cost: the 49 commits become one on `main`.
- Alternative that keeps the 49 commits: rewrite just that range with `git filter-repo`
  dropping the ported paths, push the result as a new branch, delete the old one.

**What it does and does not achieve.**
- A fresh clone fetches only reachable objects, so it shrinks from ~615 MiB to ~100 MiB
  as soon as `v1_5` is gone from the remote.
- The existing local checkout keeps the objects until the reflog entries expire and
  `git gc` prunes them. The six local stashes may also reference them; the maintainer
  confirmed (round 5) they are no longer needed, so the runbook clears them
  (`git stash clear`) before the final prune.
- GitHub keeps unreachable objects until its own garbage collection, and the two forks
  keep whatever they fetched. The size GitHub reports may lag.

**Who.** Maintainer-run (commits, pushes and branch deletion are the maintainer's by
repository rule). The plan carries it as a runbook step.

## D18 ★ — Detecting missing or modified pack files

**Question.** D9 rule 2 needs every launch to notice damage. How deep is the check?

**Recommendation.** Each run compares presence and size of every file a selected pack's
marker lists (a few thousand `stat` calls). A mismatch re-downloads that pack. Full
hashing happens when a pack's revision changes (the files are being replaced anyway) and
on `--force`. A same-size edit therefore goes unnoticed until `--force`.

**Rejected.** Hashing everything each launch: 3.5 GB of reads before the game starts.

**Consequence.** One damaged file re-downloads its whole pack (4–190 MB).

## D19 ★ — When the timed prompt appears — Overridden (round 3)

**Maintainer's direction.** The wait is unconditional: every time the updater runs, and
5 seconds rather than 10. Acceptable in the grand scheme; to be revisited if users
complain.

**Resulting behaviour.** Every normal run ends with the status line and the 5-second
prompt, whatever the selection mode and whether or not anything changed.

**Assumed edges** (none of them adds a way to switch the prompt off):
- `--check` (reports, changes nothing) and the scripting flags `--packs-install` /
  `--packs-list` do not prompt.
- With no catalog in the game folder, or a catalog listing no packs, there is nothing to
  choose and no prompt.
- When standard input is closed or redirected the read ends at once, which counts as
  "no" — such a launch does not wait.
- Offline runs still prompt; the chooser works from the installed catalog and downloads
  that fail are retried on the next launch.

**Original recommendation (not taken).** Prompt once per catalog change, then a one-line
reminder with no wait.

## D14–D17, D20, D25 — Assumed

- **D14.** Downloads resume from a kept `.part` file via HTTP `Range`; packs install one
  at a time, each its own journaled transaction, so peak extra disk is one pack's zip
  plus its contents; a free-space check precedes each download.
- **D15.** Nothing in the catalog or the updater is specific to dancers and stages
  beyond the group label.
- **D16.** `--packs-dir <folder>` resolves pack zips from a local folder; the tester
  `.bat` passes it when a `packs/` folder sits beside it.
- **D17.** The Windows image script keeps staging everything, markers included, so an
  updater later run on that folder recognises the packs.
- **D25.** The countdown and each download's progress redraw a single console line in
  place (carriage return); `ddr_world_hook_updater.log` records only the start and the
  result of each download. Shown in `prototypes/mock_updater.py`.
- **D20.** The timed prompt reads a line (Enter), the mechanism `console.rs` already
  uses for the close prompt, rather than raw key input.

---

## Readiness for design

Readiness Confirmed 2026-11-11 — the maintainer answered the round-4 request (confirm
D3 and D21–D24, name the repository, confirm readiness) with "These all sound good to
me" and created the assets repository. No decision is `Open` or `Proposed`.
