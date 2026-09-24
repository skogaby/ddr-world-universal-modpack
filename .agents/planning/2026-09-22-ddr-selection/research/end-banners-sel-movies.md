# Research: legacy end banners, the in-lane game over, `_sel` movies (Step 6, 2026-09-24)

Builds: A3 `gamemdx_20240402` (spec), World 20260825 (addresses), checked on
20250805 / 20260224 / 20260721 / 20260825 / 20260915. File-relative to
`0x180000000`. Data: the stock World install (every legacy banner arc and all
18 `_sel` movies ship in World), unpacked with `scripts/unpack_arc.py` +
`ifstools`, AFP dumped with bemaniutils.

## 1. World's end banners (ShutterActor kinds 4 / 5)

### 1.1 Who requests them

`DancePlaySequence::onUpdate` (`FUN_180057e10`) step 8, once the song ends
(not in the attract demo, `DPS+0x12C`):

| condition | kind |
|---|---|
| course, a side alive, not the last course stage | 3 (the next stage's stage panel; 8 when `DPS+0x9C`) |
| mcode `0x9733` (the jitu2 event song) | 0 |
| any side alive (`GamePlayActor+0x1E8 == 0`) | **4 CLEARED** |
| every side dead | **5 FAILED** |

`FUN_180033a00(kind)` = msg `0x1007`. The banner then runs World's normal
state machine: 0 → art (row) → 1 → 2 (swap, `in` + row SE / voice) → 3 (wait
`loop`) → **4 covered** (the scene changes 29 → 30 underneath). The results
sequence opens it: `ResultSequence::onUpdate` (`FUN_1800bc120`, e.g.
`0x1800bc4dc`, `0x1800bd09b`) calls `FUN_180033a70` = msg `0x1008` → state 5
→ non-stage kind: `out` → state 8 (wait `max(out_end, end)`) → release. Other
`0x1008` senders: GameOverSequence, EAmExit, TransitionSequence, the
LanguageWindow, the DPS / MatchingDPS (stage kinds only).

### 1.2 What World does with them

Nothing beyond the kind row. The update fills only kinds 3 (`FUN_180035f00`)
and 7 (`check_usr`); nothing in the binary reads the kind-4/5 clip's children
(scan of all 45 loads of the shutter global). The rows (default table, right
after the stage row on every build):

| build | stage row | CLEARED row | FAILED row | stride |
|---|---|---|---|---|
| 20250805 | kind 1 `0x18033ecf0` | kind 2 `0x18033ed20` | kind 3 | 0x30 |
| 20260224 | kind 1 `0x180345f30` | kind 2 `0x180345f60` | kind 3 | 0x30 |
| 20260721 | kind 3 `0x18035e0e0` | kind 4 `0x18035e120` | kind 5 | 0x40 |
| 20260825 | kind 3 `0x18035e100` | kind 4 `0x18035e140` | kind 5 | 0x40 |
| 20260915 | kind 3 `0x18035e120` | kind 4 `0x18035e160` | kind 5 | 0x40 |

CLEARED = `{NULL, "shutter_cleared", "se_game_clear", "", "vo_stage_clear",
""}`, FAILED = `{NULL, "shutter_failed", "se_game_failed", "", "", ""}` — so
`ddr_sel_shutter_stage_row + stride` / `+ 2*stride` reach them on every build
(the Step 5 derivation already publishes both inputs). World's SE / voice are
row strings played by `FUN_180034d10(kind, "in"/"out")` from the per-kind
label maps the art loader fills — a row patch controls them exactly like the
Step 5 stage row (`se_start_game` → `""`).

World dropped A3's ENJOY DDR (`GameWork+0x64`) and PRAY FOR ALL banners: its
step 8 never picks another kind.

## 2. A3's end banners (20240402)

Song end (`FUN_180039650`, `0x180039f92..fe6`): `GameWork+0x64` ⇒ kind 5
(ENJOY DDR), `+0x65` ⇒ 0, any side alive ⇒ **2 CLEARED**, except **mcode
37789 (`0x939d`, `toho1`, Tohoku EVOLVED) ⇒ 3 PRAY FOR ALL**, all dead ⇒
**4 FAILED**. Kind art `FUN_1800306c0` (the same function as the stage panel):
per-kind `{root, overlay}` and the legacy `%s` suffix table:

| A3 kind | root (legacy package `common_shutter000N`) | overlay (2nd clip, `+0xB8 + kind*8`) |
|---|---|---|
| 2 CLEARED | `shutter_clear` | `00_cleared` |
| 3 PRAY FOR ALL | `shutter_clear` | `00_prayforall` |
| 4 FAILED | `shutter_failed` | `00_failed` |
| 5 ENJOY DDR | `shutter_clear` | `00_enjoyddr` |

Root: `SetView(5)`, `SetPriority(3)` (raw 97), SD scale, rate 0, invisible.
Overlay: `SetView(5)`, **`SetPriority(2)` (raw 98 — drawn after the root, i.e.
on top)**, SD scale, rate 0, invisible. The update's swap plays `in` on both
(`FUN_180030300`), its `out` step plays `out` on both, the release destroys
both (state 0xB). The overlay is driven from the root's states only — nothing
reads its labels.

### 2.1 The legacy clips (World ships them byte-identical)

Roots — labels `in 0 / loop N / loop_end 203 (→ deep loop) / out 575 / end
642`, no `out_end` (World's state 8 waits `max(out_end, end)` = `end`, so the
drain completes by itself):

| skin | `shutter_clear` loop / sound | `shutter_failed` loop / sound |
|---|---|---|
| 1 | 58, `STG_CLOSE01` f30 | 58, `STG_CLOSE01` f30 |
| 2 | 22, — | 100, — |
| 3 | 30, `end_door` f1 | 31, `STG_CLOSE01` f30 |
| 4 | 30, `STG_CLOSE01` f15 | 16, `STG_CLOSE01` f15 |
| 5 | 20, `STG_CLOSE01` f15 | 26, `STG_CLOSE01` f18 |

Overlays — labels `in 0 / loop N / loop_end 96 (→ loop) / out 97 / end 114`:

| skin | `00_cleared` | `00_failed` | `00_prayforall` |
|---|---|---|---|
| 1 | `2nd_BIG2` f1 | `ACT9` f1 | sounds only: `vo_stage_clear` f1, `STG_APP02` f16, `STG_APP03` f21 — **no art** (0 place tags in 360 frames; see §7.1) |
| 2 | `2nd_KANSEI_B` f1 | `ext_failed` f43, `sn2_gov` f81 | stub (300 frames, no labels, no content) |
| 3 | `STG_APP03` f1 | `sn2_gov` f1 | absent |
| 4 | `STG_APP02` f4, `Plate_spin4_st` f23 | `Plate_spin4_st` f20 | yes: `STG_APP02` f4, `Plate_spin4_st` f23 |
| 5 | `STG_APP02` f4, `Plate_spin4_st` f23 | `Plate_spin4_st` f20 | absent |

Every cue is already in the `dsel` era bank (`sound/cues.rs`). The overlay's
`out` (97) comes long before the root's `out` (575), so it has to be sent
`out` together with the root (A3 did) — left alone it loops.

## 3. The in-lane game over (Step 3 carry-over) — not a bug

World's DangerActor (`FUN_180068ce0` init, msg `FUN_1800694b0`) builds the
`game_over` clip from the `dance_game_over` record, invisible, rate 0; msg
`0x103c` sets rate 1 + visible (and, with a payload, `in`). The only sender is
`GamePlayActor::onMessage` (`FUN_18005e190`) case `0x103a` (gauge empty, sent
by the gauge actors) — and only when the Option predicate at vtable `+0x200`
(`FUN_1801e2740`) is true: **not a course, not an event chain, and the stage
is an EXTRA stage** (`GameWork+0xC == max_stage + 1` / beyond, not the
override's own stage). A3's equivalent (`Option` vtable `+0x198`) is
`return 0` for normal play (only `CourseOption` differs). So neither game
shows it on a normal-stage fail — exactly the maintainer's 2026-09-23
observation. Where World does show it (an EXTRA-stage fail), the legacy
`dance_game_over000N` already replaces World's clip (Step 1) and its
`Plate_spin3_st` routes (Step 3). **No work for Step 6.**

## 4. `_sel` background movies

### 4.1 World's dormant path (identical on all five builds)

`MovieActor::onInitialize` (RTTI slot 4, `FUN_18007cc20`) → `FUN_18007cd70`:
look up the song's music info, set `+0x144 = 1`, then **if `+0x149 != 0`**
try `FUN_18007c890` first (`<basename>_sel`, `litp` → `_w`), then `_w`, the
bare name, `_vj`, `_m` (`FUN_18007c7b0` builds
`data/mdb_apx/movie/<name>.wmv`, `+0xE8` = the found path). The flag test is
`CMP byte [RDI+0x149],0; JZ; LEA R8,[RDI+0xB0]; LEA RCX,[RDI+0xD8]; MOV
RDX,RBX; CALL` — unique on all five builds, flag `+0x149`, callee references
`"_sel"`. Nothing writes `+0x149` (the ctor `FUN_18007c960` writes `+0x148`,
the thumbnail flag; the actor comes from the zero-filling agcs pool, 0x150
bytes), so the flag is 0 today.

The MovieActor is created by **`SceneManageActor::onInitialize`** (RTTI
slot 4, `FUN_18007d700`): `CALL MovieActor ctor; NOP; MOV [R?+0xD8],RAX;
MOV RDX,RAX; MOV RCX,R?; CALL addChild` (RDI on 20250805 / 20260224 /
20260721, RBX on 20260825 / 20260915; `+0xD8` on all five). The MovieActor's
own `onInitialize` runs on a LATER tree tick, so **a post-original detour on
`SceneManageActor::onInitialize` that sets `*( *(sma+0xD8) + 0x149 ) = 1`
precedes the `_sel` lookup** (RTTI vtables of both classes are already
resolved by `background_dancers::movie_backdrop`; nothing detours this
function today).

World creates the MovieActor only when the music-info entry has a movie
(bytes `+0x140/+0x141`, 5 = none) and VIDEO SIZE (`+0xE4`) is FULLSCREEN (1)
or ON (2).

### 4.2 A3's rule

`FUN_180060090` (A3's SceneManageActor init): `_sel = (GameWork+0x14 == 13)
∧ FUN_18005f100(basename)` (the file exists). When true A3 creates the
MovieActor **even when the song has no ordinary movie or VIDEO SIZE is OFF**,
forces it non-thumbnail (`+0x120 = 0`), and passes the flag to the ctor
(`+0x119`). `GameWork+0x14 == 13` is the **DDR SELECTION folder** category:
in A3 a `_sel` movie played only when the song was picked from inside a DDR
SELECTION folder.

### 4.3 The 18 `_sel` songs in World

`afro afte bagg bfor bom2 bril burn cand drte ichi kaku maxx para para2 radu
roll stil trip` (all series 1–8: 1st MIX … EXTREME). **Only 7 of them have an
ordinary World movie** (bagg, bril, burn, ichi, maxx, para, radu — musicdb
`<movie>` set, a `_w.wmv` exists); the other 11 have none, so World creates
no MovieActor for them and the flag alone never reaches them. Covering those
means making World create the actor (A3's behaviour) — e.g. a scoped
"has a movie" override of the entry's `+0x140/+0x141` around
`SceneManageActor::onInitialize` — which also changes what `MovieActor`'s
init reads for its layout (`+0x144`), and plays a movie on songs World ships
without one.

## 5. Mapping onto World (proposed Step 6 design — as built: §7)

**End banners** (extends Step 5's mechanism, same detour, no new signature):

* Rows: CLEARED / FAILED rows = stage row + 1 / + 2 strides (derived; gated
  on their stock contents `shutter_cleared` / `shutter_failed`).
* When a legacy song ends, in the ShutterActor update whose state 0 has
  `pending == cleared/failed kind` (pre-original), patch that row for one
  update: `pkg "common_shutter000N"`, `root "shutter_clear" / "shutter_failed"`,
  `SE in ""` (the legacy root plays its own `STG_CLOSE01` / `end_door`),
  **voice in `""`** on CLEARED (World's `vo_stage_clear` — the legacy overlay
  plays the era cheer; skin 1's PRAY FOR ALL plays `vo_stage_clear` itself).
  World's named-package path loads the root; its state machine drives
  `in → loop (covered) → out → end` with no change (labels match).
* Overlay: our own AFP layer from World's copy of `common_shutter000N`
  (`lookup_unowned` — NOT a ticket of ours, see §7) —
  `00_cleared` / `00_failed`, Tohoku EVOLVED (`mcode 37789`) on a CLEARED
  banner `00_prayforall` for skins 1 and 4 (as built: skin 4 only, §7.1)
  (else `00_cleared`); group 5, raw
  priority 98 (A3 `SetPriority(2)`); started with the root's `in` at World's
  swap (state 2 → 3), sent `out` when World's state 5 plays the root's `out`,
  destroyed at its own `end` (before World's state-8 package release; as
  built: never at the disarm — §7).
* Timing: the banner outlives the gameplay scene — requested in 28, covered
  through 29, opened by ResultSequence in 30 — all inside the armed window
  {26..=30}. The disarm (first scene ∉ window) must not restore a row World
  is about to read, and must leave a live root to World (the release queue
  already waits for the root layer).
* Quick fail / quick restart never request kinds 4/5 on the fast paths
  (`0x100c` + `finish`); the natural-death fallback does, and gets the
  legacy banner like any fail.
* Old builds: CLEARED / FAILED are kinds 2 / 3; `services::shutter` already
  carries the layout; kind-4/5 fields have no per-kind code fill on either
  layout (checked 20250805 `FUN_180033d00`).

**`_sel` movies:** one post-original detour on `SceneManageActor::onInitialize`
(RTTI slot 4), writing `+0x149 = 1` on the new MovieActor at `+0xD8` when the
song plays a legacy skin; World then prefers `<basename>_sel.wmv` itself.
Coordinated with `movie_policy` / `movie_sync` (no change to their seams;
the path is resolved before BuildGraph). Trigger (maintainer, 2026-09-24):
any legacy-era song — A3's folder-only rule (§4.2) is not ported, there is no
folder (design R6). All 18 songs play (maintainer, 2026-09-24) — the 11
movie-less ones need World to create a MovieActor (§4.3); VIDEO SIZE OFF is
still respected (no MovieActor ⇒ no movie).

**In-lane game over:** nothing to do (§3).

## 6. Addresses (20260825 unless noted)

DPS update `FUN_180057e10` (step 8 request at `0x180058c89`); ShutterActor
update `FUN_180033f60`, msg `FUN_180035170`, kind art `FUN_180035420`, named
layer create `FUN_180035890`, request `FUN_180033a00`, open `FUN_180033a70`
(`0x1008`); ResultSequence update `FUN_1800bc120`; GamePlayActor msg
`FUN_18005e190` (`0x103a` → `0x103c`), Option `+0x200` = `FUN_1801e2740`;
DangerActor init / msg `FUN_180068ce0` / `FUN_1800694b0`; SceneManageActor
init `FUN_18007d700` (vtable `0x180363408`; 20250805 `0x1800799f0`, 20260224
`0x180078b30`, 20260721 `0x18007d320`, 20260915 `0x18007d870`); MovieActor
ctor `FUN_18007c960`, init `FUN_18007cc20` → `FUN_18007cd70`, `_sel` path
`FUN_18007c890`, flag test `0x18007ce00` (20250805 `0x1800790f0`, 20260224
`0x180078230`, 20260721 `0x18007ca20`, 20260915 `0x18007cf70`).
A3: song end `FUN_180039650` (`0x180039f92`), kind art `FUN_1800306c0`,
update `FUN_18002f5f0`, SceneManageActor init `FUN_180060090`, MovieActor
ctor `FUN_18005f370`, `_sel` exists `FUN_18005f100`, Option `+0x198` =
`FUN_1800ff560` (`return 0`).

## 7. As built (2026-09-24, uncommitted — awaiting the cabinet)

Verified against the five builds before writing code (capstone over
`~/Desktop/ddr_modules`, Ghidra 20260825):

* **Kinds**: DPS step 8 requests `test dil,dil; cmove edx,5|3` after `mov
  edx,4|2` — CLEARED / FAILED = stage kind + 1 / + 2 on all five builds.
  ResultSequence (`FUN_1800bc120` case `0x20`) can also request kind 4 (or 3
  when `+0xE8`) at the END of the results, right after World released the
  first banner's package — as built, only requests made in GAMEPLAY (the DPS
  step-8 request) are hosted: re-requesting the same name during its deferred
  destroy is the known crash class, so that one stays World's.
* **Rows**: stage row + 1 / + 2 strides hold `{NULL, shutter_cleared,
  se_game_clear, "", vo_stage_clear, ""}` / `{NULL, shutter_failed,
  se_game_failed, "", "", ""}` on all five (0x40 / 0x30 stride; voice in =
  field 4 on both layouts).
* **SceneManageActor::onInitialize gate** (identical on all five): `CALL
  lookup(basename); TEST RAX,RAX; JZ create; MOVZX ECX,[RAX+0x141]; CMP CL,5;
  JNZ; MOVZX ECX,[RAX+0x140]; CMP CL,5; JZ skip; …; TEST AL,AL; JZ skip` —
  create iff entry null, or `b141 ∉ {0,5}`, or `b141 == 5 ∧ b140 ∉ {0,5}`;
  then VIDEO SIZE `+0xE4` (1 ⇒ also `0x1011` to the parent, 2 ⇒ thumbnail,
  else return). `b141` behaves as musicdb `<movie>` (absent = 0; bril / burn
  / ichi / para 4, maxx 3, bagg / radu 1). The lookup (`FUN_1801b3fa0`) is a
  pure linear scan of the music DB (stride 0x258) comparing entry vslot 1 =
  basename.
* **MovieActor** (alloc 0x150 on all five): init `FUN_18007cd70` sets
  `+0x144 = 1`, the name = entry `+0x148` override string (musicdb
  `<movieoverride>`, e.g. xmax → maxx) else vslot 1, and with `+0x149` set
  tries `data/mdb_apx/movie/<name><+0xB0 suffix>_sel.wmv` first (AVS
  `lstat` ⇒ LayeredFS-aware); `_w` / `_sel` finds keep `+0x144 = 1`, bare /
  `_vj` / `_m` set 2. `FUN_18007cc20` copies the movie bytes into `+0x144`
  only when they say "has a movie", and copies entry `+0x144` (musicdb
  `<movieoffset>`; trip 3584) into `+0x140` (the 0x1045 start gate). No
  reader of `MovieActor+0x144` was found on 20260825 (writes only). The
  suffix `+0xB0` = the DPS's `+0xC8` (`_ac` / `_cs` for goru, the lesson
  song) — empty for every `_sel` song.

Implementation:

* Signatures: `derive_ddr_sel_panel` now also publishes
  `ddr_sel_shutter_cleared_row` / `_failed_row` (content-gated, optional) +
  `ddr_sel_shutter_cleared_kind` / `_failed_kind`; new AOBs
  `ddr_sel_sma_movie_gate`, `ddr_sel_movie_sel_test` → `derive_ddr_sel_movie`
  (all-or-nothing) → `ddr_sel_sma_init`, `ddr_sel_music_info_lookup`,
  `ddr_sel_music_movie_kind_off` (0x141) / `_kind2_off` (0x140) / `_name_off`
  (0x148), `ddr_sel_sma_basename_off` (0x88) / `_suffix_off` (0xB0) /
  `_video_size_off` (0xE4) / `_movie_off` (0xD8), `ddr_sel_movie_sel_flag_off`
  (0x149), `ddr_sel_movie_path_off` (0xD8). Sweep ALL GREEN; `shape_diff`:
  `ddr_sel_movie_sel_test` identical through 0x180; the gate diverges at +0x31
  on 20260825/20260915 (the VIDEO SIZE `CMP [RDI+0xE4],1` vs `MOV ECX,[RBX+
  0xE4]` — both accepted, disp at +51 on both), `ddr_sel_sma_init` at +0x0
  (prologue register choice; no RIP-relative bytes).
* `banner_logic.rs` (pure, 13 host tests) + `banner.rs` (engine, called from
  `panel.rs::update_hook` — no second ShutterActor detour): the one-update
  CLEARED / FAILED row patch (5 pointers: pkg, root, SE in, SE out, voice in;
  static CStrs + the row's own `""`), refused (World's banner, one WARN) when
  `common_shutter000N` is missing, still held by one of our tickets, or
  already resident under another owner; overlay created parked at state 1 → 2
  (only when World's registry holds the package under our name — the identity
  gate), `in` at 2 → 3, `out` at 5 → 8, destroyed at its own `end` (safety
  net: pre-original at state 8 when the root is within 4 frames of `max(
  out_end, end)`); the session ends when World releases the root and is never
  torn down by the disarm. The AFP sound route also routes while a legacy
  banner is live. The package is never shared both ways: the banner refuses
  while one of our tickets holds `common_shutter000N` (the stage panel's
  release queue), and `panel::arm` refuses while World's banner copy is still
  registered. PRAY FOR ALL: `mod.rs::armed_mcode()` (the committed mcode
  latched at the arm) == 37789 on skin 4 only (§7.1); the other skins log
  the fallback and show `00_cleared`.
* `sel_movie_logic.rs` (pure, 4 host tests) + `movie_sel.rs` (engine): ONE
  `GenericDetour` on `SceneManageActor::onInitialize`; the "has a movie" part
  is a scoped DATA write (entry `+0x141` → 4 for the one call, restored
  post-original) rather than a code patch of the gate: it touches only the one
  song's entry, needs no instruction shape beyond the gate's own disps, and
  the MovieActor's later init sees the stock byte (keeps `+0x144 = 1`, World's
  own value for `_w` / `_sel`). The existence check mirrors World's name
  (override, else vslot 1) + suffix and looks in LayeredFS mod folders, then
  `data/`. Post-original the MovieActor at `+0xD8` (RTTI vtable) gets
  `+0x149 = 1`; a frame callback logs the file it opened (≤ 10 s).

Not done (follow-ups): the SD-cabinet scale of the overlay (World scales only
the root on machine types 0/1 — the Step 5 SD follow-up); ENJOY DDR
(`GameWork+0x64`, not produced by World).

### 7.1 Cabinet run #1 (2026-09-24) — findings

* **Skin 1 PRAY FOR ALL has no art.** `common_shutter0001`'s exported
  `00_prayforall` sprite (360 frames, labels `in 0 / loop 34 / loop_end 96 /
  out 97 / end 114`) contains ZERO place-object tags — only the three sound
  DoActions (`vo_stage_clear` f1, `STG_APP02` f16, `STG_APP03` f21). Its four
  shapes are unused solid-colour guides (1280×720 yellow, 100×100 blue /
  green, the 16×16 mask dummy) and the package's texture list has no PRAY FOR
  ALL image (`1st_cleared`, `1st_failed`, `1st_shutter_l/r` only). By contrast
  skin 1's `00_cleared` places `00_cleared_shape13` (region `1st_cleared`),
  and skin 4's `00_prayforall` places 258 tags drawing
  `jx3hd_g_prayforall_*`. `common_shutter0001_v0.arc` is byte-identical in
  the A3 and World installs, so A3 showed the same wordless banner (shutter +
  voice). DECIDED (maintainer, 2026-09-24): PRAY FOR ALL only where the art
  exists — skin 4; skin 1 (and 2 / 3 / 5) show `00_cleared` for Tohoku
  EVOLVED (`banner_logic::has_pray_for_all` = skin 4).
* **`afp_mc_get_param afp_mc_id[…] is invalid` spam** (43 404 lines, two ids
  per song, from `legacy stage panel released by World` to the disarm — ~2
  per frame): a Step 5 bug, not `_sel`-related (every legacy song had it; the
  long ones made it obvious). After `Finished` the panel session kept its
  root / `choice_stage_usr2` MovieClip ids and `observe()` read their frames
  on every ShutterActor update. Fixed: `after_update` returns once the
  machine is `Gone`, `Finished` clears `root` / `stage_mc`, and `observe()`
  reads the root / stage frames only while the root layer id is valid
  (`layer_id_is_valid`, quiet) — World destroys it inside its state-8 update.
* Left alone (World's own reads, harmless, pre-existing since Step 5): ~61
  `afp_mc_get_label_frame no label[out_end] in stream[…shutter_choice_hd_root]`
  per song — World's state 8 polls `max(out_end, end)` and A3's root has no
  `out_end`.
