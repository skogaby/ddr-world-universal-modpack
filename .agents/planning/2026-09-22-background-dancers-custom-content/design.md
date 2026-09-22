# Background Dancers — custom dancers & stages from `data_mods` (design)

Date: 2026-09-22. Feature: a GLOBAL SETTINGS toggle under the Background Dancers
header that adds community-made dancers and stages (arcs produced by
`tools/blender_ddr_addon`, e.g. the 2026-09-15 Peter Griffin / Griffin living-room
proof of concept) to the mod's candidate tables, so they take part in random
picks, the BACKGROUND DANCER / BACKGROUND STAGE option rows AND the live 3D
previews exactly like the stock A3 content.

## 1. What exists

- Discovery today = the three rlists inside `startup.arc` (`chara_resources`,
  `map_resources`, `stage_camera_resources`), filtered to rows whose
  `pl_<key>.arc` / `mapset_<key>.arc` resolves (LayeredFS override, else stock).
  No directory scan. The rlist key IS the arc stem and the model names inside
  (`data/chara/pl_<key>/pl_<key>.model`, `data/map/gm_<key>_<part>/…`).
- The catalog (`catalog.rs`) labels keys `UPPER(prefix) [#variant]`, ≤ 15 bytes
  (MSVC SSO). Rows are `custom_options` scalars with a FIXED max at registration
  (append-only registry) — the row count cannot change live.
- The A3 proof of concept installed `pl_peter00.arc` (members
  `data/chara/pl_peter00/{pl_peter00.model,.b2it,.grp2it,pg_*.dds}`) and
  `mapset_griffin00.arc` (+ `_g`; members `gm_griffin00_room`, `gm_griffin00_footpanel`)
  straight into `data/arc/` with a repacked `startup.arc` carrying rows
  `peter00 → [pl, M, A, 1.0, 0.8, 0.0]` (chara row 1), `griffin00 → [000000,
  000000, room, footpanel]` (map row 34) and the `boom00` camera set on camera
  row 34. `docs/3d_model_format_research.md` §"Porting" + the add-on README.

## 2. Decisions

D1. **Toggle** `background_dancers.custom_content` (bool, default `true`) — GLOBAL
SETTINGS enum row "Custom Dancers & Stages" OFF/ON in `style.rs` (whole-section
persist like every other row there). **Applies at the next launch**: the catalog
and both option rows are built once at enable and the scalar-row range is fixed.
Default ON is harmless — with no custom content installed nothing is discovered.

D2. **Layout** — ONE fixed base, `data_mods/custom_models/` (maintainer
2026-09-22: no per-character LayeredFS mod folders — every custom dancer and
stage uses the same mechanism and the same base; the first version scanned
every mod folder for `dancers/`/`stages/` and was reverted the same day):

```
data_mods/custom_models/dancers/<Friendly Name>/pl_<key>/               body FOLDER (required): pl_<key>.model, .b2it, *.dds
data_mods/custom_models/dancers/<Friendly Name>/pl_<key>_<part>/        optional parts (head00/hips00/chest00/forearm00/face01)
data_mods/custom_models/dancers/<Friendly Name>/chara_resources.rlist   optional sidecar (binary MRL0 or `.rlist.txt`)
data_mods/custom_models/stages/<Friendly Name>/mapset_<key>/            stage FOLDER (required): gm_<key>_<part>/… per part
data_mods/custom_models/stages/<Friendly Name>/mapset_<key>/camera/*.camanm   the stage's own camera set (any depth)
data_mods/custom_models/stages/<Friendly Name>/map_resources.rlist      optional sidecar (parts + :priority)
data_mods/custom_models/stages/<Friendly Name>/stage_camera_resources.rlist  optional sidecar (camera set names)
```

D2a (maintainer, after the first cabinet test): **users never pack arcs** —
a model is the add-on's exported FOLDER (or a literally unpacked arc); the
scanner packs it into `data_mods/_cache/custom_models/` (`ArcArchive`,
`CacheHasher` on member paths + mtimes) and mounts the cache arc. A ready
`pl_<key>.arc` / `mapset_<key>.arc` is still accepted. A model may also sit
directly in `dancers/` / `stages/` (no friendly folder). The base is repo
content: the Peter Griffin / Griffin House PoC models are COMMITTED there as
folders (~24 MB) and ship with every release via `build_release_archive.sh`'s
wholesale `data_mods/` copy. The maintainer handles copies into the game
install — agents never stage content into `$DDR_WORLD_INSTALL`.

D3. **Friendly name = the arc's parent folder name** when the arc sits in a
subfolder of `dancers/`/`stages/` (`Peter Griffin/` → `PETER GRIFFIN`, `Griffin
House/` → `GRIFFIN HOUSE`); a flat arc uses the stock key rule with `_` → space
(`pl_peter_griffin00.arc` → `PETER GRIFFIN`). ASCII-uppercased, non-ASCII
dropped, ≤ 15 bytes (truncated with one WARN). The KEY always comes from the arc
filename (`pl_<key>.arc` / `mapset_<key>.arc`) because the engine keys models by
the member stems inside.

D4. **Metadata = sidecar rlist rows keyed by the key**, in the SAME grammar as the
stock `startup.arc` lists, so the rows the A3 install used can be copied verbatim
(unpack the A3 `startup.arc`, drop the three `.rlist` files next to the arcs; rows
for other keys are ignored). A `.rlist.txt` twin (`key, field, field, …` per line,
`#` comments) exists for hand authoring. Defaults without a row: dancer `M, A,
1.0, 0.8` (the modal stock male row, one INFO); stage parts = every
`data/map/gm_<key>_<part>/gm_<key>_<part>.model` member (no priorities); camera
set = the stage arc's own `*.camanm` member stems if any, else stock camera row 0
(`boom00`'s `st001_*` set — what the A3 test assigned to `griffin00`).

D5. **Validation by content at discovery**: arc header parsed (64 KiB prefix,
whole file on demand); a body must contain `data/chara/pl_<key>/pl_<key>.model`,
a stage ≥ 1 `gm_<key>_<part>.model`; keys must not collide with stock keys or an
earlier folder's key (WARN + skip). A rejected arc is never handed to the engine.

D6. **Mounting**: accepted arcs are registered with the `scene3d::arc_set` mount
registry under their logical `data/arc/<name>` path; `arc_set::resolve` checks
mounts FIRST (a mount is a `Resolved::ModOverride` — the FileManager gets the
filesystem path, the parse thread reads the same file). Everything downstream
(`Pick::arcs_for`, `parts_present`, `read_bytes`, `load`) is unchanged.

D7. **Tables**: custom stages become `StageCandidate { row = stock_map_rows +
i }` with a camera row appended at that index (row-parallel invariant kept);
custom dancers `DancerCandidate { row = stock_chara_rows + i }`. Random picks,
`resolve_choice`, `stage_only`/`dancer_only`, the PIN and the previews need no
change.

D8. **Catalog**: stock entries first (sorted by key, unchanged), then the custom
entries sorted by label — stock row values stay stable under the toggle; OFF ⇒
values beyond the stock count clamp to RANDOM at load (`clamp_to_catalog`).

D9. **Camera clips**: `parse_pick` looks a camera name up in the STAGE arc's
`*.camanm` members first (any directory), then the stock
`camera/stage_camera.arc` — a custom stage can ship its own camera set. The
first cabinet test (borrowed `boom00` set) filmed the Griffin house from
outside its walls: World runs A3 STAGE mode (a list of short clips), while the
A3 PoC used the lesson song's SONG camera (one long clip, shots baked in).
`scripts/split_camanm.py` slices such a clip into per-shot stage clips
re-timed to the stock 450-frame cadence; the shipped `griffin_st01..04.camanm`
are that output.

D10. Fail-open everywhere: discovery errors are per-item WARNs; a folder with
nothing valid adds nothing; the stock tables are never touched.

## 3. Modules

- `mods/background_dancers/custom_content.rs` — PURE (std + `super::selection`):
  filename classification, labels, text rlist parser, defaults, the planner
  (`plan(dancer_dirs, stage_dirs, stock) → Plan { stages, camera_rows, dancers, labels,
  mounts, notes, warnings }`). Host-tested (harness mount).
- `mods/background_dancers/custom_scan.rs` — impure: walks the two base
  directories, packs model folders into cache arcs, reads ready-arc headers /
  rlists, calls the planner, mounts, logs.
- `scripts/split_camanm.py` — song-camera clip → per-shot stage clips.
- `services/scene3d/arc_set.rs` — mount registry.
- `lifecycle.rs::init_tables` — extends the tables when the toggle is on;
  `Tables.custom_labels`; `mod.rs` feeds them to `catalog::build_catalog_with_custom`.
- `session.rs::parse_pick` — stage-arc-first camera lookup.
- `style.rs` — the row + persistence; `config.rs` — the key.
