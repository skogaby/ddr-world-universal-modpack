# Progress — Background Dancers: custom dancers & stages from `data_mods/custom_models`

Updated: 2026-09-22 (rev 3)
Status: DONE — cabinet-validated (tests #2/#3, 2026-09-22); uncommitted — maintainer commits manually.
NEXT ACTION (maintainer): commit. Follow-ups, if wanted: a `_non` cut-away shot for the house; per-song
camera sets (`camera_music_*.arc`) as a separate feature.

Resume protocol: read `design.md` (the decisions D1–D10 + module map), then this file. No task files —
single-session implementation.

## Done

- `config.rs`: `BackgroundDancersConfig.custom_content: bool` (`default_true`).
- `style.rs`: `LIVE_CUSTOM_CONTENT` mirror, GLOBAL SETTINGS enum row "Custom Dancers & Stages" OFF/ON
  (`background-dancers-custom-content`, 8th row of the group), whole-section persist re-emits the key;
  the on_change INFO says "applies at the next launch".
- `services/scene3d/arc_set.rs`: mount registry (`mount` / `unmount_all` / `mount_count` /
  `mounted_path`); `resolve()` consults mounts FIRST and reports them as `Resolved::ModOverride`.
- NEW pure `mods/background_dancers/custom_content.rs` (planner: `classify_arc_name`,
  `classify_sidecar_name`, `parse_text_rlist`, `label_from_folder` / `label_from_key` / `fit_label`,
  `body_model_present` / `stage_parts_from_members` / `camera_names_from_members`, `plan(...) → Plan`)
  — 10 host tests incl. the PoC shape (`PETER GRIFFIN` / `GRIFFIN HOUSE`, rows 26 / 34, `_g` never
  mounted, defaults reproduce the A3 rows).
- NEW impure `mods/background_dancers/custom_scan.rs` (`CUSTOM_MODELS_DIR` = `./data_mods/custom_models`;
  `discover_and_mount(&StockContext) → Plan`: walks the `dancers/` + `stages/` roots and their
  subfolders, 64 KiB header walk via `header_fits`, sidecar rlists via `core::anm::rlist` or the text
  grammar, mounts + logs). REVISED same day from "every LayeredFS mod folder" to the one base
  (maintainer: no per-character mod folders).
- `lifecycle.rs::init_tables`: when `custom_content` ⇒ builds `StockContext`, appends the plan's
  candidates + camera rows (at the custom stage rows), stores `Tables.custom_labels`;
  `custom_labels_snapshot()`; the "tables ready" INFO now ends `… N custom`.
- `catalog.rs::build_catalog_with_custom` (stock block byte-identical first, custom block sorted by
  label, labels for non-candidate keys dropped) + test; `build_catalog` delegates with `&[]`.
- `mod.rs`: registers the rows over `build_catalog_with_custom(…, &custom_labels_snapshot())`; module
  docs.
- `session.rs::parse_pick`: the stage reader stays open; each camera name is looked up in the stage
  arc's `*.camanm` members first, then `camera/stage_camera.arc` (opened only when needed).
- Repo content: `data_mods/custom_models/dancers/Peter Griffin/pl_peter00/` (6 files, 10.8 MB
  unpacked) + `data_mods/custom_models/stages/Griffin House/mapset_griffin00/` (`gm_griffin00_room/`,
  `camera/griffin_st01..04.camanm`; the A3 `gm_griffin00_footpanel/` dance pad DROPPED after test #2) — the 2026-09-15 A3 PoC arcs UNPACKED
  (test #1 shipped the `.arc` files; maintainer: no arc packing for users); `.gitignore` keeps the
  `!/data_mods/custom_models/**/*.arc` exception for ready arcs. Ships via `build_release_archive.sh`'s
  wholesale `data_mods/` copy (no script change needed).
- **Revision after cabinet test #1 (2026-09-22):** (a) model FOLDERS — `custom_content::folder_member_path`
  + `classify_folder_name`, `custom_scan::pack_model_folder` (walk → `ArcArchive` → `_cache/custom_models/
  <name>-<fnv8>.arc`, `CacheHasher` fingerprint), `ArcFile.source` for logs, ready arcs still accepted;
  (b) the camera: test #1 filmed the house from OUTSIDE — no stage set ⇒ the borrowed `boom00` orbit;
  the A3 PoC's camera was the lesson SONG set (`camera_music_lesa.arc`, one 6238-frame clip), which the
  World mod (stage mode only) never reads. NEW `scripts/split_camanm.py` slices a song-camera clip into
  per-shot stage clips (re-timed to 450 frames, game-equivalent sampling, self-verifying: max error 1.5e-5);
  its 4 outputs ship in the stage folder's `camera/` and become the stage's own camera set.
- `scripts/validate_background_dancers.sh` mounts `custom_content.rs` — 164 tests green.
- Docs: README (feature paragraph + config table), AGENTS.md (Key Entry Points row + config section),
  `docs/background_dancers_research.md` §6.
- Gates: `cargo check` clean, `cargo fmt`, `./build.sh` release clean. No signature change ⇒ no sweep.

## Deploy & test log

- **Cabinet test #1 (2026-09-22, `.arc` layout):** PASS on discovery, labels (`PETER GRIFFIN` /
  `GRIFFIN HOUSE`), option rows, live previews, gameplay. FAIL on the stage camera: outside the house in
  preview AND gameplay (the borrowed stock set — see the revision above).
- What to look for in `log.txt` after launch (with the repo's `data_mods/custom_models/` installed):
  0. First boot only: `custom content -- packed ./data_mods/custom_models/dancers/Peter Griffin/pl_peter00 (6 file(s), 10556 KiB) into ./data_mods/_cache/custom_models/pl_peter00-<hash>.arc` (+ the stage, 22 files).
  1. `BackgroundDancers: custom dancer PETER GRIFFIN (peter00) from ./data_mods/custom_models/dancers/Peter Griffin/pl_peter00 -- sex M class A scale 1/0.8 (no sidecar row -- stock male defaults)`
  2. `BackgroundDancers: custom stage GRIFFIN HOUSE (griffin00) from …/mapset_griffin00 -- 1 part(s) [room] (arc members), camera set: 4 name(s) (the arc's own camanm clips)`
  3. `BackgroundDancers: custom content -- 1 dancer(s) + 1 stage(s) from ./data_mods/custom_models, 2 arc(s) mounted`
     then `tables ready -- 35 stage rows (26 distinct stages), 27 dancers, 35 camera rows, 2 custom`.
  Any `custom content -- …` WARN names the folder and the reason. The per-song pick INFO for the house
  should read `cameras=main:4 non:0`.
- **Cabinet test #2 (2026-09-22, folder layout + own camera shots):** PASS — camera inside the living room in
  the preview and in gameplay, previews "perfect". One remark: the A3 dance pad (`gm_griffin00_footpanel`, the
  stock lesson-demo `boom00_footpanel` copy) was visible in the room; World's normal play never shows a pad.
- **Cabinet test #3 (2026-09-22):** deleting `mapset_griffin00/gm_griffin00_footpanel/` from the repo folder
  (parts derive from the folder ⇒ `1 part(s) [room]`; the cache arc repacked on the fingerprint change) — PASS,
  maintainer-confirmed. No code change.

## Deviations & open questions

- Sidecar rows are OPTIONAL by design; the `.rlist.txt` grammar is new (no A3 precedent).
- Custom part arcs (`pl_<key>_<part>.arc`) are accepted only beside an accepted body in the SAME folder.
- Song-camera sets (`camera_music_*.arc`) stay unread — a stage's camera is a STAGE property here; a
  per-song camera would be a separate feature.
- The 4 Griffin shots have no `_non` cut-away (the A3 authoring had none); the scheduler simply cycles
  the main list through dance changes.
- The toggle is next-launch (the option rows' scalar range is fixed at registration); a live edit only
  persists.

## Key facts for a cold resume

- ONE base `data_mods/custom_models/{dancers,stages}/`; models are FOLDERS packed into
  `_cache/custom_models/` (ready arcs accepted too); key = model folder / arc name; label = enclosing
  folder name; never shadow stock; mounts live in `scene3d::arc_set`.
- A stage's `*.camanm` files (any depth in its folder) ARE its camera set; `_non` in the name = cut-away.
  `scripts/split_camanm.py` makes stage clips from a long song-camera clip.
- Row indices: custom stage `row = len(map_rows) + i` with the camera row at that index; custom dancer
  `row = len(chara_rows) + i`.
- Catalog: stock block first (unchanged), custom block after (sorted by label) ⇒ stable stock values.
- The maintainer owns copies into the game install; the repo's `data_mods/` IS the end-user layout.
