# Idea Honing: Auto-Updater

Decision register for the standalone auto-updater. Built from `rough-idea.md`,
`research/orientation.md` and `research/technologies.md`. Ordered by blast
radius (data model / interfaces / user-visible behaviour first; reversible
details last). `Status` ∈ Proposed · Accepted · Overridden · Assumed · Open.

**Register accepted by the maintainer 2026-09-13** (D1 overridden then accepted; D2–D13 accepted as recommended; D14–D18 assumed, with D14 and D3 clarified on request). No decision is Open.

**Readiness Confirmed 2026-09-13** — research complete (`research/orientation.md`, `research/technologies.md` incl. the Win7/TLS build probe); proceeding to the detailed design.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Product shape & delivery | Defines what users install and how it evolves | One standalone exe **`ddr_world_hook_updater.exe`** in the game folder (maintainer override 2026-09-13: the `_hook_` infix makes clear it updates the hook, not the game install); a separate Rust crate `updater/` in this repo (own `Cargo.toml`, NOT a workspace member); built with the DLL's Win7 recipe and **shipped inside the release zip** (so it self-updates) | Overridden → Accepted |
| D2 | Version identity / "needs update" test | Decides when the cabinet downloads 6.5 MB and rewrites files | Updater-owned marker `ddr_world_hook_updater.manifest.json` (tag + asset sha256 + per-file hashes). Update iff no manifest, OR `tag_name` differs, OR the asset digest differs (catches same-tag re-uploads like `v1.2`'s `_hotfix`). **Equality, not ordering** — deleting a bad release rolls cabinets back | Accepted |
| D3 | `data_mods/` + DLL sync semantics | Wrong choice either deletes user mods/caches or leaves stale shipped files (a known atlas-poisoning hazard) | Release-owned files overwritten unconditionally; files in the PREVIOUS manifest but absent from the new release deleted **only if unchanged since install** (hash match); anything in neither manifest (user mod folders, `_cache/`, generated `*_ifs/`, `texturelist.merged.xml`, `*.arc`) never touched; first run (no manifest) = overlay only | Accepted |
| D4 | `mod-config.json` merge rule | The user's stated core requirement, made precise | Recursive additive merge on the JSON tree: object∧object → recurse; key only in release → copy subtree; key in both → user value wins (arrays and scalars are atomic; type mismatch → user wins). Output keeps the user's key order, appends new keys at the end of their parent object, 2-space indent. Missing user file → copy release file. Sole exception: D5 | Accepted |
| D5 | `option_menu_settings` insertion algorithm | Must place new rows under the release's header without disturbing user ordering; a missing HEADER never renders at all | Header-scoped anchor insertion: walk release entries in order; for each id absent from the user list, the anchor is the nearest preceding release entry that exists in the user list **inside the same header's section**; insert right after it; no anchor → insert at the END of that header's section (before the next `header_*`); header itself missing → it is inserted by the same rule (lands after the previous section) and its rows follow. Entries before any header anchor within the pre-header region, fallback index 0. New entries copied verbatim (flags included); existing entries and ids the release dropped are left alone | Accepted |
| D6 | `judgement_offsets.csv` merge granularity | The DLL's boot crawl appends a BLANK row for every song after one boot — a row-level rule would then never deliver new community offsets | **Cell-level**: for every release row, blank user cells are filled from the release; non-blank user cells are never overwritten; rows missing from the user file are appended (release order); user row order preserved; header + LF written; grammar mirrors the DLL (`csv.rs`: trim, ±100 clamp, first duplicate wins, bad lines dropped) | Accepted |
| D7 | Failure policy & exit codes | Cabinets boot unattended into `gamestart.bat`; the updater must never strand them | Non-interactive. Exit 0 for up-to-date / updated / skipped (offline, timeout, API error, refusal) / failed-and-rolled-back. Exit 1 ONLY when rollback itself failed (install may be inconsistent), with a loud console message. No `pause`, no prompts. Connect timeout 10 s, per-read timeout 30 s, no overall cap, no in-run retries | Accepted |
| D8 | Transactional apply | A half-applied update (new DLL, old data_mods) is worse than no update | Stage under `<game>/.ddr_world_hook_updater/` (`download/`, `stage/`, `backup/`); every file the run replaces or deletes is moved to `backup/` first; any I/O error → restore all backups; order: release-owned files → merged config → merged CSV → updater self-replace → manifest write (last, so a crash before it re-runs the update next boot). Last run's `backup/` kept as a manual one-generation rollback; download + stage deleted on success | Accepted |
| D9 | Game-folder resolution & safety gate | Running from Downloads or a wrong CWD must not scatter files | Game dir = directory containing the updater exe (`current_exe().parent()`), `--game-dir <path>` override; refuse (exit 0 + message) unless `spice64.exe` or `ddr_world_hook.dll` is present there | Accepted |
| D10 | Updater self-update | The exe is in the zip and is running while it installs | Rename running exe → `ddr_world_hook_updater.exe.old`, write the new one, delete `*.old` on the next start (Windows and Wine both allow renaming a running image). Rename failure → skip self-update, WARN, continue | Accepted |
| D11 | `gamestart.bat` integration | The stated invocation contract | User adds ONE line, `ddr_world_hook_updater.exe`, above the `spice64.exe` line (after `cd /d %~dp0`); a bare exe call blocks the batch until exit. README gains an "Automatic updates" section with the snippet. Updater never launches spice itself and never edits the `.bat` | Accepted |
| D12 | Build / release integration | The archive must contain the exe and the maintainer's flow must stay one script | `scripts/build_release_archive.sh` also builds `updater/` (`cargo xwin build --release --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort`) and copies `ddr_world_hook_updater.exe` to the zip root; its stale rsync comment is corrected. Host tests: `cargo test --manifest-path updater/Cargo.toml` (pure merge/manifest logic, runs natively on macOS) | Accepted |
| D13 | Handling an unparseable local `mod-config.json` / CSV | Replacing a broken user file would destroy their data; skipping leaves the DLL to its own defaults | Back it up, leave it untouched, WARN, skip that one merge, continue the rest of the update | Accepted |
| D14 | Pre-release channel | Lets the maintainer stage builds for testers without pushing to every cabinet | `/releases/latest` (stable) by default; `--include-prerelease` flag picks the newest non-draft release by `published_at` | Assumed |
| D15 | Diagnostics / UX | Field reports need a log; operators need to see something happened | Console progress lines + `ddr_world_hook_updater.log` in the game folder, overwritten per run; on update, print release name + first lines of the changelog; `--check` (dry run) and `--force` (reinstall latest) flags | Assumed |
| D16 | Naming | Cosmetic, but fixed once shipped | Everything the updater owns carries the exe's stem so it is obviously the hook updater's: exe `ddr_world_hook_updater.exe`; marker `ddr_world_hook_updater.manifest.json`; work dir `.ddr_world_hook_updater/`; log `ddr_world_hook_updater.log` (revised after the D1 override; the earlier `modpack-*` names are withdrawn) | Assumed |
| D17 | Dependencies | Win7 + Wine + GitHub TLS 1.2 constraints | `ureq` (rustls + ring + bundled webpki-roots — no OS TLS/root store), `zip` (deflate only), `serde_json` (`preserve_order`), `sha2`; probe-built and live-run under CrossOver (see `research/technologies.md`) | Assumed |
| D18 | Out of scope (this feature) | Keeps the first cut shippable | No `.bat` editing, no spice2x management, no code signing, no DLL-side reading of the manifest (follow-up: the hardcoded splash `v1.2` in `src/lib.rs` could read `ddr_world_hook_updater.manifest.json`), no version pinning (users remove the bat line to opt out) | Assumed |

---

## Detail per decision

### D1 — Product shape & delivery
**Question.** Where does the updater live, how is it built, how does it reach users?
**Recommendation.** A separate binary crate `updater/` (own `Cargo.toml`/`Cargo.lock`), not a cargo workspace member — the root `Cargo.toml` is a plain package with unstable `cargo-features`, and converting it to a workspace risks the DLL build for no benefit. It is compiled with the DLL's Win7 recipe (the tier-3 target avoids the `ProcessPrng` import that makes default-msvc binaries unloadable on Win7 — verified by the probe) and shipped at the zip root so existing installs receive updater fixes automatically (⇒ D10).
**Rejected.** PowerShell/batch (Win7 ships PowerShell 2.0, no TLS 1.2, no `Invoke-WebRequest`); a Python script (no interpreter on cabinets); folding the updater into the DLL (the DLL can't replace itself while the game runs, and the requirement is to update BEFORE spice starts).

### D2 — Version identity
**Question.** How does the updater know what is installed?
**Recommendation.** Nothing on disk records the installed release today (`Cargo.toml` says `0.1.0`, the splash text is hardcoded). The updater writes `ddr_world_hook_updater.manifest.json` after every successful install: `{ "tag", "release_name", "asset_name", "asset_sha256", "installed_at", "files": { "<relative path>": "<sha256>" } }`. "Needs update" = no manifest, or `tag_name` ≠ manifest tag, or asset sha256 ≠ manifest asset sha256. The asset digest comes from the API's `digest` field (present on every release so far) and is also used to verify the download.
**Why equality, not ordering.** Tags are `vMAJOR.MINOR` today but nothing guarantees that; equality needs no parsing, handles same-tag hotfix re-uploads, and turns "delete the broken release on GitHub" into an automatic fleet rollback.
**First run for existing users.** No manifest ⇒ the latest release is installed once (idempotent: merges preserve everything user-owned) and the manifest is written.

### D3 — `data_mods/` and other release-owned files
**Question.** Overlay, mirror, or something in between?
**Recommendation.** Three classes of path under the game folder:
1. **Release-owned** — every path in the new zip except `mod-config.json`, `judgement_offsets.csv`, `ddr_world_hook_updater.exe` (handled by D4/D6/D10): overwrite unconditionally (a user who edited a shipped texture loses the edit — same as today's manual reinstall).
2. **Previously shipped, now dropped** — in the previous manifest's `files` but not in the new zip: delete iff the current on-disk sha256 equals the previous manifest's hash; otherwise keep and log ("modified locally, left in place").
3. **Everything else** — never touched. This covers user mod folders under `data_mods/` (the LayeredFS root), `data_mods/_cache/**`, enable-time generated `*_ifs/` dirs (s_marvelous, custom_folders geo/afp), `texturelist.merged.xml` files, `*.arc`, `bg_preview/`, `step_data_exports/`, `log.txt`, etc.
**Why pruning matters.** AGENTS.md: a stray `seop_op_item_*.png` left under `data_mods/custom_options/` gets packed back into the served atlas — removed files must actually disappear from cabinets.
**First run — "overlay only".** The prune rule in class 2 needs a PREVIOUS
manifest to know which files on disk were shipped by an earlier release (as
opposed to being the user's own mods or generated artifacts). The very first
time the updater runs in a game folder there is no manifest yet (existing users
installed v1.0–v1.2 by hand), so it cannot tell a stale shipped file from a user
file and therefore **deletes nothing** — it only extracts the new zip over the
folder (class 1: write/overwrite every file the zip contains) and then writes
the manifest. From the second run on, class 2 pruning is active. Consequence: a
file that an OLD release shipped and a later release dropped, on a cabinet
that upgraded across that gap by hand before adopting the updater, is never
removed automatically. Today this is moot — `git log --diff-filter=D --
data_mods` shows **no file has ever been removed from `data_mods/` since the
initial commit**, so no published release has dropped a shipped file; the
`seop_op_item_*.png` removal predates v1.0. If a future release ever needs to
retire a file that pre-updater installs might still hold, a maintainer-authored
`retired_paths.txt` in the zip (patterns deleted unconditionally) is an easy
follow-up; not included now.

### D4 — `mod-config.json` merge
**Question.** Exactly what does "copy new keys, keep existing values" mean on a nested document?
**Recommendation.** `merge(user, release)`: for each key in `release`: absent in `user` → insert the release subtree; present and both are objects → recurse; otherwise keep the user value. Arrays (`fps_unlock.presets`, `resolution.presets`, `series_expansion.custom_series`, `layeredfs.allowlist`, …) are atomic user values. Only `custom_options.option_menu_settings` gets element-level treatment (D5). New `mods.<id>` keys are copied with the release's value — the committed `mod-config.json` is therefore the release's defaults (today it is the maintainer's own config, incl. `false` for several mods and a cabinet-specific `timing_offsets.sound_offset`; that is pre-existing behaviour for fresh installs, not changed here).
**Formatting.** The DLL rewrites the file alphabetically-sorted with 2-space indent on any menu edit, so formatting fidelity has no runtime value; the updater still preserves the user's key order (`preserve_order`) and appends new keys at the end of their parent object so a diff of the merge is readable.

### D5 — `option_menu_settings`
**Question.** How to insert new rows "under whatever header they were under in the release" without reordering the user's list.
**Facts.** A header is just an id starting with `header_` (registered by `decorative-option-headers`); grouping is array position only. An unlisted non-header row falls to the END automatically; an unlisted HEADER is never rendered — so missing headers MUST be inserted or a new section is invisible.
**Algorithm.** Let `R` = release list, `U` = user list (ids compared case-insensitively). For each `r` in `R` (in order) whose id is not in `U`:
1. `H(r)` = the nearest preceding `header_*` entry in `R` (or none).
2. Candidates = release entries preceding `r` back to (and including) `H(r)`, most recent first, that exist in `U` **and** sit inside `H(r)`'s section of `U` (from `U`'s occurrence of `H(r)` up to the next `header_*`).
3. Insert `r` (copied verbatim, flags included) immediately after the first candidate; if none, insert at the end of `H(r)`'s section in `U`; if `H(r)` is not in `U`, it was already inserted by this same loop (headers are ordinary entries to it) — the fallback for a `r` with no header is the pre-header region of `U`, then index 0.
Entries only in `U` (renamed/removed options) stay — the DLL logs one WARN per unknown id and ignores it. Existing entries' flags are never changed.

### D6 — `judgement_offsets.csv`
**Question.** Row-level ("insert missing songs") or cell-level?
**Recommendation.** Cell-level. The DLL's boot crawl appends `code,,` for every song in the musicdb the first time it boots (`bootstrap.rs`), so after one boot the user's file already has a row per song and a row-level rule would never propagate community-list additions/corrections for existing songs. Cell-level fills only BLANK user cells and never touches a non-blank one — every value the user set (through the in-game row, which can only produce −100..+100, never blank) is preserved. Limitation (accepted): a value the user inherited from an older release and never touched is indistinguishable from one they set, so release CORRECTIONS to previously-shipped values do not propagate; only additions/fills do.
**Mechanics.** Parse both with the DLL's grammar (`csv.rs`: optional header, LF/CRLF, trimmed cells, blank = unset, ±100 clamp, >3 cells or non-integer → line dropped, first duplicate wins). Output: header, user rows in their order (cells filled), then new rows in release order, LF endings, trailing newline, via tmp+rename. Missing user file → copy release. Unparseable → D13.

### D7 — Failure policy
Cabinets run `gamestart.bat` unattended. Any network or update failure prints one line, logs, and exits 0 so spice starts. Exit 1 is reserved for "rollback failed" so an operator who does check `errorlevel` can stop the launch; the console message says exactly which files to inspect. No retries within a run — the next boot is the retry.

### D8 — Transactional apply
Work dir `<game>/.ddr_world_hook_updater/` (same volume ⇒ atomic renames). Per run: `download/<asset>` (verified against the digest), `stage/` (extracted with `enclosed_name()` traversal checks), `backup/` (cleared at run start). Apply moves each replaced/deleted target into `backup/` (mirroring its relative path) before writing; the merged config/CSV are written to temp files and renamed into place; the manifest is written LAST. Failure anywhere ⇒ walk `backup/` back into place; if that fails ⇒ exit 1. Success ⇒ delete `download/` and `stage/`, keep `backup/` until the next run.

### D9 — Game folder
Exe-relative, not CWD-relative: `gamestart.bat` files usually `cd /d %~dp0` but a double-clicked exe or a mis-written bat must still target the right folder. Safety anchor: `spice64.exe` or `ddr_world_hook.dll` must exist in the resolved folder.

### D10 — Self-update
The updater is release-owned but is the running image. Rename-swap is the standard Windows dance and works under Wine (rename of an open file is permitted; delete/overwrite is not). Leftover `*.old` is removed at the next start (best-effort).

### D11 — `gamestart.bat`
```bat
@echo off
cd /d %~dp0
ddr_world_hook_updater.exe
start spice64.exe -ddr -modules modules -K ddr_world_hook.dll ...
```
The bare call blocks; `start`/`call` are not needed for an `.exe`. Documented in README.

### D12 — Build / release integration
One script stays the release path. `build_release_archive.sh` gains a second `cargo xwin build` for `updater/` and a `cp` of the exe into the stage dir; the comment claiming `_cache`/`*_ifs`/`*.arc` exclusions is fixed to describe reality (they never exist in a checkout / are committed shipping content). Pure logic (`merge_json`, `merge_option_menu_settings`, `merge_csv`, manifest diff/prune planning, asset selection) lives in library modules with `cargo test` coverage runnable on the macOS host.

### D13 — Unparseable local files
The DLL treats an unparseable `mod-config.json` as `{}` on its next write (destroying it), so the updater must not make things worse: leave the file, WARN loudly (console + log), skip only that merge.

### D14 — Pre-release channel (how it works)
GitHub lets a release be published with the **"Set as a pre-release"** box
ticked. `GET /releases/latest` never returns pre-releases (or drafts), so the
default updater behaviour — call `/releases/latest`, install if different —
gives every cabinet the newest STABLE release and nothing else.

Opt-in: a tester edits their bat line to
`ddr_world_hook_updater.exe --include-prerelease`. With the flag the updater
calls `GET /releases?per_page=10` instead, drops drafts (unauthenticated calls
never see them anyway), and picks the entry with the newest `published_at`
regardless of the `prerelease` flag. Everything downstream is identical: same
asset selection, digest check, merge, manifest. Because the "needs update" test
is EQUALITY (D2):
- promoting the build to stable under a new tag (GitHub requires a distinct tag,
  e.g. `v1.3-rc1` → `v1.3`) re-downloads once on tester cabinets — identical
  content, harmless;
- deleting a bad pre-release moves testers back to the newest remaining release;
- removing the flag from the bat moves a tester cabinet back to the latest
  stable on the next boot even if it currently runs a pre-release.

Maintainer flow: `scripts/build_release_archive.sh` → create the GitHub release
with the pre-release box ticked → attach the zip. No repo/config change; no
second channel to maintain. Stable users are unaffected.

### D15–D18 — Assumed
Settled by the agent as reversible; recorded so the design can be audited. D15's per-run log overwrite keeps the folder clean (the last run is the interesting one). D16 follows the D1 override so every updater-owned artifact shares the `ddr_world_hook_updater` stem. D17's stack is the probe-verified one. D18 lists what this feature deliberately does not do; the splash-version follow-up is the most likely next request.
