# Orientation (2026-09-22)

Inputs: `docs/ddr_selection_research.md` (the RE record), a repo sweep, and a
short Ghidra pass on World 20260825 to check the load chain before designing
against it. Addresses are file-relative to `gamemdx.dll` @ `0x180000000`.

## Confirmed from the research doc

- **Resolver `FUN_1801ac260(int* out, const char* dir, const char* name)`**
  tries `<name>_v3`, `<name>_v0`, `<name>_lite` (pcType-gated), `<name>`
  through `FUN_1801ac160` (lstat + FileManager register), then writes
  `out[0] = file index`, `out[1] = FNV-1("<cand>.ifs")` (multiply-then-xor,
  basis `0x811c9dc5`). Called twice per package by `FUN_1801ace90`: first with
  the language-suffixed name, then with the plain name. The package-vector
  entry is filled with the **requested** name (`param_3` of `FUN_1801ace90`),
  so a detour that swaps only the resolver's `name` argument leaves every actor
  lookup unchanged. Nothing in the repo detours or signs this function today.
- `FUN_1801aca30(dir, name, flag)` (= the repo's existing
  `bm2d_data_request_load` signature, `src/services/bm2d_package.rs`)
  **dedupes by name**: a request for a name already in the package vector
  returns without touching the resolver.
- All 25 needed legacy arcs are on disk in the stock install
  (`dance_{judge,fast_slow,fullcombo,game_over,danger}000{1..5}_v0.arc`); no
  `_lang_*` variants exist for these packages, so the language-suffixed
  resolver call is a miss for both stock and skinned names.

## New findings that change the research doc's plan

1. **`dance_message` has no World consumer.** Its only string reference is in
   `LayoutActor`; World has no `00_ready`/`00_here` strings at all (World's
   READY/HERE WE GO are the `ready_loop`/`ready_out`/`ready_%dp_usr` clips of
   another package). Swapping `dance_message` would load a package nothing
   plays. The research doc's §5 "class A" row for it is wrong; legacy READY /
   HERE WE GO need a re-hosted message actor (Phase 2+).
2. **World's `LayoutActor::onInitialize` (`FUN_18006b8b0`) marks
   `dance_danger` SHARED** (`param_5 = 1`) together with `dance_stage`,
   `dance_message`, `dance_song_info`, `dance_common`. Shared packages are only
   registered by the `LayoutActor` when the skin id (`+0x190`, from
   `GameWork+0xA8`) is non-zero; with skin 0 they are loaded by the **scene
   loader**: the stage loader's group table at `0x18035af00` lists
   `dance_common`, `dance_stage`, `dance_song_info`, `dance_danger`,
   `dance_shock_arrow`, `dance_matching` under mask `0x9000`.
3. **LayoutActor slot 5 (`FUN_18006bc30`) releases every package it
   registered** via `FUN_1801acd00(name)` (erase by name; entries ≥ 72 only).
   Per-side LayoutActor packages therefore never outlive their DPS — no
   cross-song residency for judge/fast_slow/fullcombo/game_over.
4. Consequence of 2+3 for the research doc's optional `GameWork+0xA8 = skin`
   write: a non-zero skin makes the `LayoutActor` register the shared set too;
   the requests dedupe onto the **loader-owned** entries (`dance_common` = the
   layout root, `dance_stage`, …), and the `LayoutActor`'s finalize then
   erases them. A package released before its layers are destroyed asserts
   (`docs/bm2d_background_preview_research.md`). Not safe as a side effect.
5. **`dance_danger` residency hazard** (moot once the `%04d` append is
   restored in the per-package helper — round-2 D4 — because the legacy copy
   is then a separate, `LayoutActor`-owned entry). Loader-owned packages survive the
   quick-fail skip-results path (29 → 24 skips the 29 loader's unload —
   `docs/quick_restart_fail_speedup_research.md` §4c residency note), and the
   next stage loader's load is then a dedupe no-op. A skin change across that
   path would show the previous song's danger art. Needs an explicit reconcile
   at song select.

## Repo building blocks (paths)

- Package API: `src/services/bm2d_package.rs` (`request_load` / `is_ready` /
  `release` wrappers; no detours).
- LayeredFS: an untouched legacy arc passes through `arc_handler::handle_arc`
  unchanged; the lstat hook honours mod-folder copies of skin arcs. Texture
  injection keys on the member basename (`dance_judge0001_v0_ifs`), so
  S-Marvelous's `dance_judge_v3_ifs` staging never reaches a legacy package.
- `afplist_ext` can only append geo ids; nothing can rename an AFP export
  (the `dance_gauge` class-B swap needs new machinery — afplist name, AP2
  exported name, `afp/<md5>` / `bsi` / `<name>_shapeN` paths).
- S-Marvelous (`src/mods/s_marvelous/`): its AFP patch refuses non-stock bytes
  (fails open), but `afp_patches.rs` `PATCH_APPLIED` is a session latch and
  `flash.rs` gates only on it — a legacy judge after a stock one would get an
  `in_smarvelous` play on a clip that lacks the label. Same shape for the
  S-MFC splash on `dance_fullcombo_v3`. Needs a per-song stand-down query.
- Options: `ScalarFormat::Dynamic` scalar rows render values as text (no
  per-value chips) — `src/mods/background_dancers/options.rs` is the closest
  precedent (`PersistMode::Local`, `.in_game_only()`, load clamp,
  `Duplicate` re-enable path). Row label PNG `seop_item_<id>` comes from
  `scripts/option_strings.py` + `scripts/gen_option_labels.py`.
- Governance: `services/versus_mirror` (`register` / `mirror_edit`),
  `stage_records::{side_entered, player_work, course_field_offset,
  event_mode}`, `multiplayer_bot::is_bot_side`.
- Series: no mcode→series helper exists. `find_music_by_mcode` is already
  derived (`signatures.rs` `derive_ultrafast_boot`); the raw-series vslot is
  the wildcarded disp32 at `flare_skill_classifier` match+2 (0xA0 on
  20260324+, 0x88 on 20250805/20260224) — publishable via `publish_value`.
  Raw series (corrected by `intro-and-skin-surface.md` §5 from A3's musicdb
  anchors + flare classifier ≥14 WHITE / ≥18 GOLD): 1–5 1st–5th, 6–8
  MAX–EXTREME, 9–10 SuperNOVA 1–2, 11–13 X–X3, 14 2013, 15–16 DDR (2014),
  **17 A, 18 A20, 19 A20 PLUS, 20 A3**. The research doc §7.1 labels (16 = A,
  17 = A20) are off by one from 16 up; `docs/series_filter_internals.md`'s
  filter table is right.
- Scene lifecycle: `scene_manager::on_scene_change(prev, next)` fires before
  `createNextSequence`; window {26,27,28} (the Background Dancers shape);
  quick restart stays inside it (28 → 27 → 28); the stage loader (27) is where
  `dance_danger` loads.
