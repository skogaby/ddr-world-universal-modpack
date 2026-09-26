# Orientation (2026-09-25)

Inputs: `docs/ddr_selection_a3_themes_research.md` (the feasibility pass), the shipped mod
(`src/mods/ddr_selection/`), its planning record (`.agents/planning/2026-09-22-ddr-selection/`),
the stock World and A3 installs, and a Ghidra pass (A3 `gamemdx_20240402`, World 20260825).
Addresses are file-relative to `0x180000000`.

## Starting state

- The parent project (`2026-09-22-ddr-selection`) is complete: Steps 1–14 done, `ddr-selection`
  default ON. This project extends it; it does not reopen any of its decisions.
- Working tree: `src/mods/ddr_selection/trigger.rs` has uncommitted maintainer label edits. The row
  now reads `OFF / AUTO / 1stMIX-5thMIX / MAX-EXTREME / SuperNOVA 1-2 / X-X3 vs 2ndMIX /
  2013-2014`. `policy::skin_name` (logs) still carries the old names.

## Data (verified; see the research doc §1–§4 for the full inventory)

- Three generations, all byte-identical to A3's install:
  - `_v0` = the DDR A generation;
  - `_v1` = A3 on the gold cabinet;
  - `_v2` = A3 on every other cabinet.
- **New evidence for `_v1` = gold:** `dance_common0000_v1`'s `dance_root` is the only layout root
  with `matching_usr` / `matching_left_usr` / `matching_right_usr`, the BPL matching markers. That
  matching feature exists only on the gold cabinet.
- `_v1` and `_v2` judge / combo / full-combo textures are **pixel-identical**. Only the frames
  (gauge, score, stage frame, song info), the layout root and the shutter / panel art differ.
- Every theme layout root (`dance_common0000_v{0,1,2}`) carries the full marker set the post-pass
  reads (`dance_root` and `lane_single_normal` children compared against `dance_common0003_v0`:
  nothing missing).

## A3's skin-0 stage panel (Ghidra, A3 `FUN_180030d10`)

The fill takes its legacy branch only when both era names (`+0x1B0`, `+0x1D8`) are set. Otherwise
it takes the skin-0 branch:

1. Hide `choice_stage_usr2`.
2. Choose the band texture:
   - `scene_choice_stage_extra` when the stage is the extra stage;
   - else `_final`;
   - else `_4th` (stage 3), `_3rd` (2), `_2nd` (1), `_1st`.
3. Normal stages (not course; not the override stage; not past `max + 1`): write that texture into
   the **root's own** `choice_stage_usr/scene_choice_stage_usr`. The root's default
   `choice_stage` content stays. Nothing is loaded into `choice_background_usr` or
   `choice_jacket_usr` (except a `choice_background_%s` variant when `+0x198` ∈ 1..=3 — an event /
   exclusive mode, not normal play).
4. Special stages (the override stage, or beyond `max + 1`): load `choice_stage` /
   `choice_background` from a loader-owned package (`*DAT_1802eee90 + 0x170`) and set
   `scene_choice_stage_{fl%s, galaxy, encore}` into `choice_stage_usr/scene_choice_stage{1,2}_usr`.
   These are A3 event presentations with no World counterpart.
5. Score sets, `caution_usr`, `fullcombo_challenge_usr` and rinon behave as on every skin.

The jacket goes into the root's default `choice_jacket_usr/jacket_root_usr/jacket_usr`; the
existing panel's `Jacket::Song` path writes exactly that child. There is no cut-in on skin 0: the
cut-in state is gated on the cut-in package having loaded.

## Danger actor, skin 0 (Ghidra)

- World `FUN_180068ce0` and A3 `FUN_180048630` share the same branch shape.
- **The second clip at `+0xA0` is dead in both games.** World creates a second copy of the danger
  export; A3 created `danger_{single,double}_failed`. Both are created paused and are never touched
  again: World's update `FUN_180069100` and msg `FUN_1800694b0`, and A3's `FUN_180048b10` /
  `FUN_180048ec0`, read only `+0x98` and `+0xA8`. Only finalize `FUN_180069430` destroys it.
- **The only visible gap for a record skin ≥ 6** is the doubles export: skin ≠ 0 always creates
  `danger_single`. That is one `JNZ` at the head of the export choice (research doc §5, site A,
  unique on all five builds). Site B (the second clip) needs no patch.

## BM2D texture names

- `libafputils` `read_texture_list` (`FUN_18002dbb0`, `afpu-ngp.c`) allocates each IFS's texture
  and image records **under the package id** (`FUN_18000cb50(pkg, n)`, `FUN_18000e2f0(pkg, n)`).
  That suggests per-package storage with a lookup across mounted packages [inf].
- Whether a name present in two packages resolves to the right one was **not traced to the
  lookup**. Cabinet cases that would expose a problem:
  - `stage_frame0000_*` is in skin 1's stage-frame package and in the themes';
  - `dance_song_info0000_*` is in `_v0` / `_v1` / `_v2`;
  - `scene_choice_stage_*` is in `common_choice_v0` / `_v1` / `_v2`.

  Front-load a consecutive-song test.

## Code that the themes touch (mod side)

- Skin-range literals and caps: `policy.rs` (`SKIN_MAX`, `Entry.skins: u8`, `skin_name`),
  `trigger.rs` (`ROW_MAX`, labels, `auto_skin`, dev knob `1..=5`), `gauge.rs:246`,
  `combo.rs:110/561`, `score.rs:325`, `stage_frame.rs:38-40/193`, `option_icons.rs:231`,
  `banner.rs:60`, `panel_logic.rs:59-69/124-165`, `marker_keys.rs:197`, `song_info_logic.rs:42`,
  `sound/rules.rs:138`, `intro.rs:321` (builds `dance_message000N` itself).
- `GameWork+0xA8` is read only by World's three `== 1` gates and the DPS `int[6]` table; the helper
  ignores the value it produces (`package_helper.rs:128`). So the themes can write 0.
- S-Marvelous stands down cleanly on skins outside `LEGACY_SKINS`
  (`s_marvelous/targets.rs:141-149` → `None` ⇒ no patch, no re-drive). Its per-skin masks are `u8`
  (`skin_bit` → 0 above 7), so an art-supported skin 8 needs wider masks.
- Every cue the theme clips embed or the A3 skin-0 announcer / crowd plays is already in the
  `dsel` bank (`sound/cues.rs`: `vo_ingame_cheer`, `vo_ingame_boo`, `STG_BOO`, `vo_stage_*`,
  `se_shutter_in/out`, `banner_in`, `Plate_spin3_st`, `XAC_full_combo2`, `vo_ingame_ready`,
  `vo_stage_clear`).
- The A3 import manifest already lists `common_choice_v2`, `common_shutter_v2`,
  `dance_common0000_v2` and `dance_song_info0000_v2` as `missing` entries. The themes' other files
  (`_v0` / `_v1`, `dance_message_vN`) are not listed; a stock World install ships them all.
- The cabinet machine type is readable through `arkMDXGetMachineType` (already resolved by
  `custom_resolution/debug_ui.rs:145`; detoured by `smx_hardware/cabinet_force.rs` when SMX forces
  GOLD).
