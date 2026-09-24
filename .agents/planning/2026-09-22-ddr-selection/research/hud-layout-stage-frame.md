# Step 7 RE — layout builder, legacy markers, stage frame, danger 3–5, BPM / name (2026-09-24)

Addresses are 20260825 (`0x180000000` base) unless stated. A3 = `gamemdx_20240402`.
Companion to `hud-actors.md` §4 / §6 / §7 (this file supersedes their
coordinate tables: the ones below are pivot-composed screen positions).

## 1. World layout builder `FUN_18006bd40`

- Called from `LayoutActor::onUpdate` (RTTI slot 6, `FUN_18006bb30`) ONCE, after
  every name on the LayoutActor load list (`+0x170`, 0x28-stride
  `std::string`s) is registered AND loaded. Every HUD actor reads its marker
  after it (their inits run later).
- Root: record `"dance_common"` → export `dance_root`. Skin 0 ⇒ the loader slot
  `*DAT_1806f2d70 + 0x6b0` (loader-owned `dance_common_v3`).
- Root clip: `FUN_180257af0` (= `CMovieClip::Create`:
  `afpu_get_afp_info_at_package` + `afp_layer_create_with_property` + find `"/"`
  + attr `0x200`) — equivalent to `bm2d_api::create_layer_from_package(pkg,
  "dance_root")` + `layer_find_child(layer, "/")`. Values are readable right
  after the create (no frame advance needed). The builder destroys it before
  returning.
- Marker maps:
  - shared: setter parent `LayoutActor + 0x98` (map at parent `+0x28`): keys
    `dance_matching`, `stage`, `song_info`;
  - per side: parent `+0xE0 + side*0x48` (map at `+0x108 + side*0x48`).
    Written DIRECTLY via `FUN_18006f590(map, &std::string)` (map
    `operator[]`, bypassing the setter): `score`, `bpm`, `difficulty`, `name`,
    `gauge`, `danger_gauge`, `gameover`, `option`. Written through the setter
    `FUN_18006f020(parent, name, coord)`: `option_icon`, `fullcombo`, `judge`,
    `combo`, `fast_slow`, `filter`, `score_compare`, `arrow_raw`, `arrow`
    (`x - w/2`, `y ± h/2`: `+h/2` unless reverse), `freeze_judge`.
  - The setter OVERWRITES `map[name]` (so a post-pass `set` replaces World's
    value). The getter `FUN_18006f100(parent, name)` returns a static
    `{0,0,0,0,1.0f,1.0f}` on a miss — an element whose key is missing lands at
    (0,0), it does not crash.
- Coord (0x18 bytes) = `{i32 x = (int)(pos.x + K), i32 y = (int)(pos.y + K),
  i32 w = (int)param 0x1015, i32 h = (int)param 0x1016, f32 sx, f32 sy}`
  (`sx/sy` = param `0x100d` float pair, stored as raw float bits). `pos` =
  `afp_mc_get_param(mc, 0x1008)` → two floats = the clip's screen / global
  position (a placement's `rotation_origin` pivot is honoured). `K` =
  `DAT_18035b7b4` = **0.5** (round-half-up, then `cvttss2si`).
- Named markers: `Ordinal_103` = `afp_layer_mc_refer(root_layer, "name")`.
  Nested (`<lane>/judge_usr`): CMovieClip find `FUN_180257f20`, then
  `FUN_1802588b0` (pos), `FUN_180258bc0` (0x1015 / 0x1016), `FUN_180258b50`
  (0x100d).

## 2. A3 builder `FUN_18004ace0` — the spec

Same coord semantics. Name → key:

| Map | A3 marker | Key |
|---|---|---|
| shared | `matching_*` | `dance_matching` |
| shared | `stage_frame_usr` | `stage_frame` (World's key is **`stage`**) |
| shared | `song_info_usr` | `song_info` |
| side | `score_%dp_usr` | `score` |
| side | `difficuty_normal_%dp%s_usr` | `difficulty` (§5) |
| side | `gauge_%dp_usr` | `gauge` |
| side | `danger_gauge_%dp_usr` | `danger_gauge` |
| side | `%dp_gameover_usr` | `gameover` |
| side | `option_icon_%dp%s_usr` | `option` (A3) |
| side | `%dp_lane_usr` / `double_lane_usr` (the lane MC itself) | `fullcombo` |
| lane | `%s/judge_usr` | `judge` |
| lane | `%s/combo_set_usr/combo_usr` | `combo` |
| lane | `%s/combo_set_usr/fast_slow_usr` | `fast_slow` |
| lane | `%s/filter_usr`, `%s/score_compare_usr`, `%s/arrow_usr` (→ `arrow_raw` + `arrow`), `%s/freeze_judge_usr` | same names |

A3 has no `bpm`, `name` or `option_icon` keys. The lane is `%dp_lane_usr` when
the style `+0x84 + side*4` == 0, else `double_lane_usr`; reverse comes from the
player-option vfunc. World's NoteResultActor (`FUN_18007aa40`) positions
FAST/SLOW exactly like A3 (`FUN_180058590`): x = marker.x, y = marker.y − h/2,
align (2,3); the judge word is set at the marker too.

## 3. Marker coordinates (frame 0, pivot-composed, 1P)

World root `dance_common_v3` vs the legacy roots (`dance_common0000_v2` = A3
skin 0 / 1, `dance_common0002..0005_v0`):

| Element | World | Legacy |
|---|---|---|
| stage | 641,23 | 640,36 (0000_v2: 640,42) |
| score | 281,672 | 192,663 |
| difficulty | 235,630 | 193,622–630 |
| gauge | 280,37 | 278,36 |
| danger_gauge | absent | 440,38 |
| judge | 281,251 | 281,242 |
| combo | 281,354 | 281,377 |
| fast_slow | 346,317 | 497,371 |
| score_compare | 182,307 | 281,285 |

Identical in all roots: `arrow` 136,117; `freeze_judge` 136,186; `filter` /
`fullcombo` / `gameover` 281,360. P2 legacy `danger_gauge` 1110,38 (840,38 on
0004 / 0005). Every legacy arc is byte-identical to A3's. World ships
`dance_common0000_v2.arc` but no `dance_common0001` ⇒ skin 1 = the explicit
name `dance_common0000_v2` (World's probe tries `_v3` / `_v0` / `_lite` /
bare, so `"dance_common0000_v2"` resolves through the bare rung).

## 4. Stage frame (`StageFrameActor`)

- RTTI `.?AVStageFrameActor@dance@sequence@@`, created only by DPS /
  MatchingDPS (`FUN_18007a100`). Slot 4 init `FUN_18007a190`, slot 8 msg
  `FUN_18007a360` (`0x104f` → texture fn `FUN_18007a390` via the `CALL` at
  msg+0x24; `0x1052` → hide). The msg fn is byte-identical on all five builds.
- Init: record under World's base `"dance_stage"` (the helper inserts
  `dance_stage_frame000N` there with skin N — policy row exists, adapter
  `StageFrame`). `actor+0x68` = skin; skin 0 ⇒ loader slot `+0x6f0`. It
  creates the export via `LEA R8,["dance_stage"]; MOV R9D,5; … CALL
  FUN_180257af0` (AOB `4C 8D 05 ?? ?? ?? ?? 41 B9 05 00 00 00`, unique on all
  five builds; gate on the target string). The legacy export is
  **`stage_frame`** (`stage_frame_sd` on SD — A3 picked it on machine types
  0/1). A missing export ⇒ NULL-deref crash. Marker key `"stage"`, align (3,3).
- Texture fn: `MOV R8D,0xb; LEA RDX,["dast_stage_"]; LEA RCX,[R11-0x40]; CALL
  assign` at fn+0x46 (AOB `41 B8 0B 00 00 00 48 8D 15 ?? ?? ?? ?? 49 8D 4B ??
  E8`; 2 hits per build — gate by the string and by lying inside the texture
  fn). World then appends its stage suffix (`01..05` / `final` / `extra` /
  `encoreextra` / `galaxy` / `howto` / `checking` by World's own stage rules)
  and `load_bitmap`s `stage_number_usr`. A3 used `"stage_frame%04d_stage_%s"`.
  The legacy textures are `01`, `02`, `extra`, `final` (+ `03` on skin 4);
  others miss, as in A3 (hud-actors C6).

## 5. Danger 3–5

World DanceDangerActor init `FUN_180068ce0`, per-side parent `actor+0x60`:
skin 0 ⇒ `filter` marker + a second clip; skins 1–2 ⇒ centred (640,360);
skins 3–5 ⇒ the `danger_gauge` marker — absent from World's root, so (0,0)
today. The legacy `dance_danger0003..5` packages export `danger_single` /
`danger_double` (checked). A post-pass writing `danger_gauge` + the `Markers`
adapter therefore unlocks the existing policy row. The game-over clip also
applies the marker scale (vt+0xc0 with sx/sy).


## 6. Builder details finished in Step 7

- **Coord reads** (all through `afp_mc_get_param`, Ordinal 115): `0x1008` →
  `{f32 x, f32 y}` (return 0 = ok; the builder does not guard a failure — it
  reads a stale stack slot); `0x1015` / `0x1016` → one f32 each, truncated
  (`FUN_180258bc0`, return ignored); `0x100d` → `{sx, sy}` (`FUN_180258b50`; on
  failure both = the first float). A missing marker (`find` null) ⇒ coord
  `{0,0,0,0,1.0,1.0}`. `bm2d_api::{mc_get_vec2, mc_get_param}` reproduce the
  reads exactly (`mc_get_param` truncates `out[0]` like `cvttss2si`).
- **Named lookups** (`FUN_180257f20` → `FUN_1802583b0(new, root+8, name)`) are
  `afp_layer_mc_refer(root_layer, path)` — paths with `/` included — i.e.
  `bm2d_api::layer_find_child(layer, "1p_lane_usr/judge_usr")`.
- **Per-side loop** (`41 8B 84 9D 84 00 00 00 83 F8 02 0F 84` — style
  `+0x84 + side*4`; style 2 ⇒ the side is skipped entirely, no key written):
  reverse = the player Option's vslot `+0x2F8` (`Option+0x1C == 1`), stored by
  the builder at **`LayoutActor + 0xE4 + side*0x48`** (the per-side parent
  `+4`; the record map sits at parent `+8`, the marker map at `+0x28`). Same
  offsets on all five builds (the store `41 88 84 CD E4 00 00 00` after
  `FF 92 F8 02 00 00`, unique).
- **Lane content is reloaded before the lane children are read.** World
  `load_movie`s the root export `lane_<single|double>_<normal|reverse>` into
  the lane MC (`FUN_1802585c0` = `afpu_get_afp_info_at_package` +
  `afp_mc_load_movie` + `afp_mc_set_param(0x101e, 1)` over the traversal(6)
  chain — `bm2d_api::mc_load_movie`), twice:
  1. after `fullcombo` (read from the lane MC itself, before any load): variant
     `(Option vslot 0x298 == 1) XOR reverse` — vslot 0x298 returns
     `Option+0x4C`, World's **`judge_position`** option — then `judge`,
     `combo`, `fast_slow`, `filter`, `score_compare`;
  2. variant = `reverse` alone, then `arrow_raw` / `arrow`, `freeze_judge`.
  The Option object is `FUN_1801ea0e0(player, 0)` (`dl=1` ⇒ `*player +
  0xE0` = `PlayerWork + player_option_offset`; `dl=0` substitutes a forced
  Option only for the special mcode 0x9733 on an extra stage). The vslot is
  read from the builder's `MOV RCX,[RBP+x]; MOV R11,[RCX]; CALL [R11+0x298];
  CMP EAX,1; SETZ AL` (unique on all five builds, `0x298` everywhere).
- **A3 loads the lane once**, `lane_<single|double>_<reverse ? reverse :
  normal>` (A3 has no `judge_position`). With `judge_position` 0 both World
  loads equal A3's. The legacy roots carry both variants (legacy
  `lane_single_reverse`: judge 439, combo 316, arrow 591 — lane-local), so
  the post-pass mirrors World's two loads: a legacy song honours the player's
  `judge_position` (documented deviation from A3, which had no such option).
- `arrow` = `{x − w/2, y − h/2}` (reverse: `y + h/2`), C division toward zero,
  the rest of the coord unchanged; `arrow_raw` = the raw coord.
- The builder destroys its root clip (`CMovieClip` vt+0x18) before
  returning; the transient root is never drawn.

## 7. A3 builder argument check

`difficuty_normal_%dp%s_usr` and `option_icon_%dp%s_usr`: `%s` = `""`
(`0x1801e6ccc`, an empty string) or `"_reverse"` by the scroll-reverse byte
(`CMOVNE`). A3 marker names in the legacy roots (dump of
`dance_common0002_v0/dance_root`): `1p_lane_usr/{filter,score_compare,judge,
arrow,freeze_judge}_usr`, `1p_lane_usr/combo_set_usr/{combo,fast_slow}_usr`,
`stage_frame_usr`, `1p_gameover_usr`, `difficuty_normal_1p[_reverse]_usr`,
`score_1p_usr`, `gauge_1p_usr`, `danger_gauge_1p_usr`,
`option_icon_1p[_reverse]_usr`, `song_info_usr` (+ the `2p_` / `double_lane_usr`
twins).

## 8. Stage frame — five-build shape

RTTI `StageFrameActor` vtable slot 4 (init) / slot 8 (msg). On every build:
export LEA (`4C 8D 05 … 41 B9 05 00 00 00 48 8D 3C C0 48 8B D6`, unique) at
**init + 0xC8** → `"dance_stage"` (the init's first `"dance_stage"` LEA at
+0x21 is the RECORD key — never patch it); the msg fn's `CALL` at +0x24 is the
texture fn (msg bytes identical on all five); the texture site (`41 B8 0B 00 00
00 48 8D 15 … 49 8D 4B ?? E8 ?? ?? ?? ?? 90 48 8B 1D ?? ?? ?? ?? 48 8B 13 80
7A`, unique — the short form also hits a `"music_title"` assign far away) at
**texture fn + 0x46** → `"dast_stage_"`. 20250805 init 0x180076460 / texture
0x180076690; 20260224 0x1800755d0 / 0x1800757d0; 20260721 0x180079d80 /
0x180079f80; 20260825 0x18007a190 / 0x18007a390; 20260915 0x18007a180 /
0x18007a3b0. The legacy `dance_stage_frame0001..5_v0` export `stage_frame` +
`stage_frame_sd` with `stage_frame_usr` / `stage_number_usr`; textures
`stage_frame000N_stage_{01,02,extra,final}` (+ `03` on 0004).

Mechanism (as built): checked code patches, applied from the package helper
when `dance_stage` turns legacy and restored whenever `dance_stage` goes
through the helper stock (every `LayoutActor` requests it before its
StageFrameActor initialises), at disarm and at disable:
(a) the export LEA disp32 → a near-allocated `"stage_frame\0"`; (b) the
texture site's 11 bytes `imm32 | 48 8D 15 | disp32` → `22 | 48 8D 15 |
→ "stage_frame000N_stage_"`. World's stage-suffix logic (01..05 / final /
extra / encoreextra / galaxy / howto / checking) is kept. If either patch
cannot be applied the package stays stock (the record would otherwise hand
World's init a package without a `dance_stage` export — NULL-deref). SD
(`stage_frame_sd` on machine types 0/1) is not ported (follow-up, like the
panel's SD root). Chosen over two detours: the window is exactly the
`LayoutActor` lifetime, the game calls both functions on the game thread,
and a disp32/imm rewrite has no call-time cost.

## 9. BPM display and player name (hide)

- **BpmActor** (`.?AVBpmActor@dance@sequence@@`, init slot 4
  `FUN_180065960`): creates `dance_bpm` from the `dance_bpm` record (always
  the stock package — no legacy variant), aligns (3,3), positions it ONCE at
  the `bpm` marker (`vt+0x38`) and scales it by the marker's `sx/sy`
  (`vt+0xc0`). Its update (slot 6 → a stored functor → `FUN_180065dc0` →
  `FUN_180065bd0`) only re-textures the digits (`dabp_bpm_num_%d` via
  `%s/%04d_usr` load_bitmap); nothing repositions or re-shows it.
- **ScoreActor name** (`FUN_1800775d0`): creates `dance_name` into `+0x88`,
  aligns (3,3), positions it ONCE at the `name` marker, scales by its `sx/sy`,
  then fills `name_usr`. No other ScoreActor function reads `name` or `+0x88`.
- ⇒ **Hide = write the `bpm` and `name` markers off-screen** in the builder
  post-pass (`{−4096, −4096, 0, 0, 1.0, 1.0}`, through the setter — its
  `map[name] = coord` overwrite also replaces World's direct `operator[]`
  writes). Zero detours, survives the per-frame digit updates, restored for
  free (the marker map dies with the LayoutActor). Both keys are World-only:
  A3's legacy screens have neither element. Option icons: World's own skin-1
  gate hides them on skin 1; skins 2–5 keep World's (design P8).
- Other `"bpm"` LEAs (`0x1800f15d6`, `0x1801213f6`, `0x1801a5ceb`) are
  song-select / option tables, not marker reads.

## 10. Danger 3–5 dependency

`dance_danger` skins 3–5 need the legacy root's `danger_gauge` marker. The
policy row carries `Adapter::Markers`; the helper additionally refuses the
package when the skin's root arc is missing (`markers::root_available`), so
danger never lands at (0,0).

## 11. Mechanism as built (Step 7)

- **`services/hud_layout_hooks.rs`** — the ONE owner of the builder and
  setter detours (promoted from center_arrows_single, behaviour-identical):
  builder pre subscribers → original → builder post subscribers; setter pre
  subscribers (copy of the coord, rewritten, copied back — World's builder
  reads nothing back from the buffer except the arrow math, which uses
  registers). `set_marker(parent, key, coord)` runs the setter subscribers
  and then the original, so center-arrows' +/−360 shift applies to every key
  the post-pass overwrites. Detours install on the first `acquire()` and stay
  for the session; center-arrows' disable now only turns its subscribers off
  (its song-info dark-card detour stays in its own file — only Step 10 needs
  it shared). Builder entry derivation moved to
  `SignatureStore::derive_hud_layout` (same prologue-AOB / cluster − 0x1DC
  logic) together with the side extras (§6).
- **Root loading.** The package helper, for `dance_common` on an armed song:
  World's original (skin 0 — stock root, no record), then pushes the legacy
  root name (`marker_keys::root_name`, probe-checked) onto the load list with
  no record. The LayoutActor waits for it like any of its packages and
  releases it at finalize. `markers::on_common_request` records
  `{actor, skin, root}`.
- **Post-pass** (builder post, same call): `lookup_unowned(root)` →
  `create_layer_from_package(pkg, "dance_root")` + attribute 0x200 (World's
  `CMovieClip::Create`: `afp_layer_create_with_property`, `afp_id_is_valid`,
  find `/`, attr 0x200 — no play) → shared keys → per laid-out side: root keys
  and `fullcombo` (lane MC), then the judge-group lane load + keys, the
  arrow-group lane load + keys (`arrow` derived from `arrow_raw`) → the
  World-only hides → layer invisible + destroyed. One INFO per side + one
  summary (`moved / World's (package stock) / missing / hidden`).
- **Per-key gating** (`marker_keys.rs`, pure): a key moves only when its
  element's World package is the legacy one this song (`legacy_package`) —
  `score` / `difficulty` (dance_score, Step 10), `gauge` (Step 8), `combo`
  (Step 9), `song_info` (Step 10) therefore stay World's until their adapters
  make those packages legacy; `stage` (dance_stage), `danger_gauge`,
  `gameover`, `fullcombo`, `judge`, `fast_slow` move with their packages;
  `filter`, `score_compare` (World's pacemaker at A3's spot), `arrow_raw` /
  `arrow`, `freeze_judge` always move (identical in every root except
  `score_compare`).
- **Adapters.** `Markers` = the post-pass installed; `StageFrame` = the
  patch sites + buffer ready AND `Markers` (the legacy frame needs A3's
  `stage_frame_usr` position). `dance_danger` skins 3–5 are additionally
  refused when the skin's root arc is missing.
