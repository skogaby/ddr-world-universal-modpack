# Orientation (blind-spot pass, 2026-11-11)

What the idea has to live in. Everything here was read from the repository or measured;
sources are named per finding.

## 1. The content

`data_mods/custom_models/` is 3.5 GB / ~31.7k files in the checkout; the rest of
`data_mods/` is under 7 MB. It is already organised one folder per SOURCE
(`<kind>/<Source>/<Friendly>/<model>`, `src/mods/background_dancers/custom_content.rs`
module doc; sources are the DANCER SOURCE / STAGE SOURCE option rows,
`src/mods/background_dancers/sources.rs`).

| Dancers | Raw | Models | | Stages | Raw | Models |
|---|---|---|---|---|---|---|
| HOTTSTPARTY 1-3 | 570 M | 137 | | HOTTEST PARTY 3 | 247 M | 17 |
| UNIVERSE 1-3 | 479 M | 63 | | HOTTEST PARTY 4 | 139 M | 26 |
| PARTYCOLLECTION | 292 M | 60 | | HOTTEST PARTY 1 | 134 M | 42 |
| STRIKE | 220 M | 45 | | HOTTEST PARTY 5 | 123 M | 14 |
| DDR WINDOWS 2K2 | 160 M | 49 | | HOTTEST PARTY 2 | 118 M | 59 |
| FESTIVAL | 127 M | 26 | | SUPERNOVA 1 & 2 | 95 M | 22 |
| HOTTSTPARTY 4 | 118 M | 32 | | Custom | 38 M | 2 |
| X & X2 | 104 M | 33 | | WINX CLUB | 30 M | 6 |
| WINX CLUB | 95 M | 24 | | X & X2 | 20 M | 6 |
| ULTRAMIX 1-4 | 89 M | 25 | | | | |
| 4th MIX | 88 M | 18 | | | | |
| 3rd MIX | 78 M | 16 | | | | |
| 5th MIX | 73 M | 15 | | | | |
| HOTTSTPARTY 5 | 69 M | 18 | | | | |
| Custom | 69 M | 6 | | | | |
| SUPERNOVA 1 & 2 | 56 M | 24 | | | | |

- 16 dancer sources + 9 stage sources = 25 natural pack units, ~785 friendly folders.
- The release ships each model PRE-PACKED as one uncompressed `.arc` plus its sidecar
  (`scripts/pack_custom_models.py`, called from `scripts/build_release_archive.sh`).
- **Measured compression** (pack five sources, `zip -9`): dancers shrink to ~30–35 %
  (3rd MIX 79 M → 23 M, SUPERNOVA 57 M → 20 M, WINX 97 M → 33 M), stages to ~6–20 %
  (SUPERNOVA 97 M → 5.7 M, X & X2 20 M → 3.9 M). Everything together is therefore
  roughly **1 GB of downloads**, not 3.5 GB; the largest single pack (HOTTSTPARTY 1-3
  dancers) is about 190 MB zipped.
- `Custom` holds the maintainer's own models (Big Smoke, Carl Johnson, Hatsune Miku,
  Ironmouse, Kasane Teto, Peter Griffin; Griffin House, Grove Street). v1.4 already
  shipped five of those dancers and both stages, as loose folders directly under
  `dancers/` / `stages/` (commit `80c8a70e`).
- Soft cross-pack relationship: flight stages exist in HOTTEST PARTY 2/3/4 stages and
  only flight-capable dancers (the HOTTSTPARTY dancer sources) fly on them.
- Shared runtime art the models need (`data_mods/background_dancers/`, shader blobs)
  is small and outside `custom_models/` — it stays in the base either way.

## 2. The updater as shipped (v1.3 / v1.4 cabinets run this binary)

Sources: `updater/src/*.rs`,
`.agents/planning/2026-09-13-auto-updater/design/detailed-design.md`.

- One API call (`/releases/latest`), then the FIRST asset named
  `ddr-world-universal-modpack-*.zip` (`github::select_asset`).
- `archive::DEFAULT_LIMITS.max_total_bytes` = **2 GiB uncompressed**, 100 000 entries.
- Up to date ⇔ manifest `tag` AND `asset_sha256` equal the release's
  (`manifest::needs_update`). One asset per manifest; `schema` must equal 1 or the
  manifest is ignored (treated as "first run").
- Plan rules (`plan.rs`): every staged file is release-owned and written
  unconditionally (the replaced file goes to `.ddr_world_hook_updater/backup/`, which
  the NEXT apply wipes); a path in the previous manifest that the new release dropped is
  pruned only if its on-disk hash still equals the recorded one; a path in neither set
  is never read or touched.
- Whole-zip pipeline: download → extract everything to `stage/` → hash every file →
  rename into place, one transaction with journal + rollback.
- Downloads are not resumable: a failed transfer deletes the partial file.
- Non-interactive by contract (R19/R20): no prompts, exit 0 whenever the game can start.
  It only waits for Enter when it is alone on its console (double-click detection,
  `console.rs`).
- Self-update is a rename-swap at the END of an install; the new binary first runs on
  the following launch.

## 3. What that means for this idea (the unknown unknowns)

1. **The status quo cannot ship.** A single v1.5 zip carrying 3.5 GB would exceed the
   2 GiB uncompressed cap in every installed updater: each cabinet would log "archive is
   too large", skip, and never update again. Splitting is required, not optional.
2. **The v1.4 → v1.5 update is performed by the OLD updater.** It picks the first
   `ddr-world-universal-modpack-*.zip` asset, runs its own merge and plan code, and only
   then swaps itself. Consequences:
   - pack assets must never match that name pattern, or an old updater may install a
     pack as if it were the modpack;
   - whatever the new updater does differently (pack selection, changed merge rules,
     changed overwrite rules) first applies on the launch AFTER the v1.5 base landed —
     at which point the manifest already says "v1.5, up to date". Pack handling must
     therefore be evaluated independently of "is the base current".
   - the old updater prunes v1.4's loose `dancers/<Friendly>/…` folders when the new base
     no longer lists them (unmodified ones only).
3. **Pack ownership must not live in the main manifest.** Any updater that installs a
   base release prunes manifest paths the base zip lacks. Pack files recorded there would
   be deleted on the next base update (and by any older updater binary).
4. **The updater blocks the game launch.** A first-time 1 GB download inside
   `gamestart.bat` holds the cabinet at a console window; and a cabinet may have no
   keyboard attached, so the unattended path can never ask a question.
5. **Rebuilding a zip changes its hash.** If pack identity were the zip's sha256, every
   release would look like "all packs changed" (zip timestamps, zlib differences between
   the macOS and Windows build hosts). Identity has to come from the pack's contents.
6. **Where packs are hosted decides the release workflow.** GitHub: each asset < 2 GiB,
   ≤ 1000 assets per release, no total-size or bandwidth limit, ranged GETs supported
   (https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).
   Attaching ~25 zips (~1 GB) to every release works but re-uploads unchanged content
   each time; a long-lived content release avoids that but must not become
   `/releases/latest`. `gh` is not installed on the build machine today, and releases
   appear to be published by hand.
7. **User-model clobber paths that exist today:** (a) a user edits or replaces a shipped
   model file in place → the next update overwrites it; (b) a user's own file sits at a
   path a later release starts shipping → overwritten; (c) deselecting a pack must not
   remove files the user added to that source folder. Files at paths the modpack never
   shipped are already safe. Separately, the DLL requires model keys to be unique across
   stock and all sources (first wins, `custom_content.rs` tests
   `duplicate_key_across_sources_keeps_the_first_source`) — a user key colliding with a
   newly shipped one is a DLL-side conflict the updater cannot resolve by file rules.
8. **Manual installers exist** (unzip by hand, no updater). Pack zips rooted at the game
   folder keep that working; a pack unzipped by hand should later be recognised by the
   updater rather than treated as foreign files.

## 4. Option-row placement: mostly already built

`updater/src/merge/option_menu.rs` (design R13 of the auto-updater, shipped since v1.3,
unchanged since) already merges `custom_options.option_menu_settings` element-wise:
a missing release row is inserted inside the section of the header it sits under in the
release, after the nearest preceding release sibling the user still has in that section,
else at the section end; existing rows, their order and flags are never touched.

Simulated with the real files (a Python transcription of the algorithm):

| User config | Canonical (HEAD) | Result |
|---|---|---|
| v1.3 default (`a30040d2`) | HEAD | inserts `ddr_selection`, both source rows, both model rows → **identical to canonical** |
| v1.4 default (`80c8a70e`) | HEAD | inserts `background_dancer_source`, `background_stage_source` → **identical to canonical** |
| v1.4, user moved `ddr_selection` to the top of Playfield Styling | HEAD | `background_dancer_source` lands right after `ddr_selection` (top), away from `background_dancer` (end) |
| v1.4, user moved the dancer/stage rows under Power User | HEAD | both source rows land at the end of Playfield Styling, away from their rows |

So the stated requirement holds for anyone who kept the default order. The gap is the
anchor rule: it only looks at the PRECEDING canonical sibling, so a row that canonically
sits just before its partner (`…_source` before the model row) follows the wrong
neighbour once the user has re-ordered.

Two adjacent facts:
- The DLL (`src/services/custom_options/ordering.rs`) renders rows absent from the
  config at the END of the menu, except `ShowWhen` families, which stay beside their
  listed relative. That is what a user who never runs the updater and keeps an old
  `mod-config.json` sees.
- The v1.4 → v1.5 merge runs in the old updater, so a refined rule first takes effect
  for the release after v1.5.

## 5. Prior art in this repository

- `.agents/planning/2026-09-13-auto-updater/` — the updater's design and plan (complete).
  Not the same project; this one extends it.
- `.agents/planning/2026-09-30-custom-model-sources/` — the source folder level.
- `.agents/planning/20260723-custom-options-row-order/` — `option_menu_settings`.
- Release tooling: `scripts/build_release_archive.sh`,
  `scripts/build_release_distribution.bat` (Windows twin, unzipped image),
  `scripts/pack_custom_models.py`.
