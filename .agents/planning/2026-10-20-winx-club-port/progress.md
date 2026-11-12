# Progress — DanceDanceRevolution Winx Club (Wii, EU) dancer + stage + transformation port

Updated: 2026-10-20 (session 3)
Status: Steps 1–5 of 5 DONE (dynamic bones, kira, transformation, wipe, wing sparkles) —
host-validated, NOT cabinet-tested. Uncommitted (maintainer commits manually).
NEXT ACTION: maintainer cabinet run #1 with the watch-list below (deploy the DLL + the whole
`data_mods/custom_models/dancers/WINX CLUB/` + the 5 new `data_mods/shader_fixes/blobs/*wipe*`).
Resume protocol: this file → `docs/wii_ddr_winx_club_research.md` §3 / §6 / §7 (the RE + what was
ported, exact vs approximated) → the `//!` docs of `background_dancers/{mod,dynbones_logic,
transform_logic,chara_fx,director}.rs` → `port_character_hottest2.py` / `port_winx_fx.py` docstrings.

## Source
- Disc `RW6PA4` "DanceDanceRevolution Winx Club" (EU 2009, Konami, zan engine — the HP2 / HP3
  generation). Image `~/Desktop/ddr_wii/Dance Dance Revolution - Winx Club (Europe) (En,Fr,De,Es,It).wbfs`.
- Dumped: `extract_wii_ddr_data.py disc` → `~/Desktop/ddr_wii/Winx Club (Europe)` (993 entries,
  1 s; the disc ships CVS/ dirs, not .svn). Partial `extract --png` (character, accessory, select,
  stage, text, game, common) → `~/Desktop/ddr_wii/winx_extracted`.
- `zan_dump.py survey .` → 0 problems (425 zmb, 7230 zab, 786 cam, 10 teb).
- NOT on this machine any more: the HP2–HP5 disc dumps (`~/Desktop/DDR Wii ISOs/` is gone), so
  `plan_stage_ports` cannot run against them; de-dup was done against the SHIPPED
  `data_mods/custom_models/stages/**` textures instead (below).

## Layout (HP2 / HP3 style, not HP4 / HP5)
- Costumes `character/CHR<nn:02><k>.bin` (k = 0 model {body, head}, 1 the one texture variant).
- Choreography `motion/MOT010_SSQ<nnn>.bin` (001–055; 028–033 / 049–052 short = demo / lesson),
  tempo `ssq/MU_WIN_<nnn>.ss9`. Same 37-bone rig as HP2–HP5 (+ AccSpine1, DBBOX_01,
  ZDRAW_kira_* on the Enchantix bodies; head has DB_hair* dynamic bones).
- Stages `stage/STG<nnn>.bin` = `/#0` models + `/#1` cameras (8 shots each).
- Per-song secondary motion: `game/HAIRMOT/HAIR_MOT<nn>.bin` (rot_DB* hair bones),
  `game/COLMOT/COL_MOT<nn>.bin` (DBBOX_* collision boxes), 56 ZABs each (≈ the MOT pieces).
- `game/APLMOT/APL_CHARA0<n>.bin` per girl: two 37-bone clips (120 f + 40 f) + DBBOX / hair
  tracks → the APPEAL / transformation pose (to confirm in RE).
- Effects: `game/EVENT_EFFECT.bin` = `chenge_effect.TEB` (the transformation effect, "change"),
  `game/CHARA_EFFECT.bin` = `chara_effects.TEB`, `GAME_APL_EFF.bin` = concent.TEB,
  `GAME_STG_EFF.bin` = ddr2_effect.TEB.
- Wings: `accessory/AccSpine1_wing0<n>.bin` — SKINNED to DB_Lwing/DB_Rwing_01/02 (flap bones;
  no ZAB found yet drives them — likely procedural, RE).

## The cast (SYS_TEXT.bin bank #1 order: Bloom, Stella, Musa, Flora, Tecna, Layla)
Person index n = 1..6 in that order (hair colours verified on the head textures; CHR03 navy =
Musa, CHR04 caramel = Flora; texture `wd_02_stella_body` confirms 02).
| CHR | prefix | outfit | notes |
|---|---|---|---|
| 0n | wb_0n | Enchantix (fairy form) | own head (kira sparkle nodes), + AccSpine1_wing0n |
| 1n | wa_0n | default casual | head wa_0n (shared by 2n / 3n) |
| 2n | wc_0n | "Dance" (unlockable) | wa head |
| 3n | wd_0n | "Denim" (unlockable) | wa head (CHR36 reuses wb_06_ref03) |
| 51–54 | mii / m_0x | Mii bodies | not ported (as HP4 / HP5) |
No colour-group-2 skin materials on the girls (skin baked into the texture, as HP4 / HP5).
Gameplay: after a set number of combos the girl transforms into her Enchantix form
(fandom wiki / MobyGames; maintainer has gameplay footage: a brief cutscene, wings appear).

## The stages (58 files)
- STG000 empty. **STG001–006 are NEW** (Winx locations; the stage-select text names five places
  + random: Red Fountain show stage, Alfea, a stained-glass lobby, a library, a mall; 1/38 shipped
  textures). 8 shots each; no `_Prm`, no group 91 / 92 materials.
- STG007–055, 101–103 = FuruFuru Party's (HP2) STG007–055 / 101–103 again: 100 % of their
  single-texture materials are byte-identical to the shipped `HOTTEST PARTY 2/Stage NN` except
  STG009's BG (a grey hexagon tile, 2 textures) and STG035 (one 512² graffiti texture — the EU
  "HOTTEST PARTY" logo). STG102 / 103 are HP2's ring-tunnel flight stages.

## RE questions for main.dol (Ghidra)
1. Trigger: what count transforms (combo N? score? gauge?), per player; can it revert (miss)?
2. Sequence: duration, cameras (GAME_DEF_CAM / a special shot?), the APL clip(s) used, the
   `chenge_effect` effect ids + attach joints, sound, and whether dancing / the chart pauses.
3. Costume swap: casual CHR1n/2n/3n → Enchantix CHR0n at which frame; the head swap too.
4. Wings: what drives DB_Lwing / DB_Rwing (procedural flap? a table?); also HAIRMOT / COLMOT
   (per-song dynamic-bone tracks — needed for the port, or bake rest?).
5. `CEventStage001..006` / `CtsStage*`: story-mode stage scripts — anything gameplay uses?

## Maintainer decisions (2026-10-20)
- Stages: only STG001–006 ship (`stages/WINX CLUB`).
- Dancers: casual / Dance / Denim transform into Enchantix mid-song; Enchantix also selectable alone.
  24 entries, labels `Bloom` / `Bloom Dance` / `Bloom Denim` / `Bloom Fairy`.
- Trigger: the player's own gameplay, like the Wii (RE: full Dance Gauge before 80 % of the song).

## main.dol RE (Ghidra `ddr_winx_club_main.dol`; r2 = 0x80297e20, r13 = 0x80296740)
- Character ids: `FUN_8001d760` groups 0–5 Enchantix (CHR0n), 10–15 casual (CHR1n), 20–25 Dance,
  30–35 Denim, 50–53 Mii; base table `DAT_801c845c` = {0, 10, 20, 30, 50}. Random picks exclude
  0–5 (`FUN_80033a88`): Enchantix is only reached by transforming. Loader `FUN_80089d74`.
- Each player has TWO dancer slots: i (her outfit) and i+2 (her Enchantix twin)
  (`FUN_800a76e4(seq, i, i, i+2, ..)` in `FUN_8002eb34`). Both carry the same choreography.
- Sequencer = game+0x2dc, per frame `FUN_800a6d3c`; timer in seconds; consts SDA2.
  - Trigger (`FUN_8003149c` → `FUN_800a7650`): player +0xa0970 (Dance Gauge, 0..1000, clamped in
    `FUN_800284a8`; gains scale with combo +0xa09e4 in `FUN_80028130`) == 1000 AND song pos
    (`FUN_8001caec`) ≤ 0.8 × song length; player not yet transformed. Up to 2 players, one at a time.
  - state 1 (0.5 s): scene colour 1 → 0 (stage + objects, `seq+0x2c` copied to the stage
    `+0x2b48` and `+0x6c`).
  - state 2 at 0.5 s: APL clip 0 (`APL_CHARA0n /#0/#0`, 120 f = 2 s) on both twins + APL hair /
    col tracks; effect `chenge_effect` #0 (EVENT_EFFECT bank = effect category 4,
    `FUN_800fafac(.., 0, 4, 0x23)`); camera cut to the event list (= GAME_DEF_CAM `/#3/#0`,
    92 f, `FUN_8003aedc` member 3 → cam+0x274; `FUN_8003d1d8`). Colour back 0 → 1 over 0.5 s.
    WIPE: a world point lerps (0, −10, 0) → (0, 30, 0) zan units over 2.0 s (seq+8 = 2.0,
    set in `FUN_8002eb34`); projected to screen y; the outfit twin is drawn ABOVE the line, the
    Enchantix twin BELOW (`FUN_800a73e0`, GX scissor viewports) — Enchantix is revealed from the
    feet up; the effect rides the line (+0.2 y). Normal dancers are hidden in states 2–3.
  - state 3 when APL 0 ends: APL clip 1 (`/#0/#1`, 40 f) on both; the effect fades out (0.5 s).
  - end (`FUN_800a7678`, from `FUN_800307f0`): at the next choreography piece start → state 4
    (0.5 s), player state 4 (transformed); slot visibility hands over to the twin
    (`FUN_800406d8` pair link); event camera released (`FUN_8003d224`).
  - Enchantix dancers (ids 0–5) are drawn 0.8 zan units higher (≈ 9 cm: they float).
- Wings `AccSpine1_wing0n`: CtsCharaAtc (`FUN_80037ccc`) only looks up DB_Lwing_02 / DB_Rwing_02;
  no ZAB drives them (dynamic bones). HAIR_MOT / COL_MOT = per-song dynamic-bone drivers +
  DBBOX colliders. Heads are SKINNED to their own DB_hair / DB_side bones (unlike HP).
- Cameras in GAME_DEF_CAM `/#1` `/#2` `/#3` carry a third +0x34 signature
  `28f81300 7cf71300 5cf71300`; stage cameras use HP2's.
- Effects parse with teb_dump: chenge_effect (ribbon + sparkles + glow, 2 s loop), chara_effects.

## Done
- Disc dumped, survey clean, cast identified, stage de-dup vs shipped content.
- Transformation RE (above; write-up `docs/wii_ddr_winx_club_research.md` §3).
- Step 1 tooling: `zan_dump` third camera signature + `_fold_weights` (dynamic-bone skins baked
  rigid; HP weights pass through); `test_zan_formats` 33/33 (`TestWinxClub`);
  `validate_wii_ddr_tools.sh` surveys a Winx disc (`character/`): 0 problems.
- Step 2 ports:
  - `GAME=winx` in `port_character_hottest2.py`: 24 dancers → `dancers/WINX CLUB/` (91 MB):
    labels `X` / `X Denim` (CHR2n, the denim set) / `X Dance` (CHR3n) / `X Fairy` (CHR0n + wings,
    +0.8 units). Keys `winx<person>01..04`. DBBOX boxes and additive kira overlays skipped.
    363-clip library, 12 clips each. Every dancer has `motion/transform/{transform,pose}.anm`;
    the 18 outfits have `pl_<key>/transform.txt` naming their Fairy twin.
  - `GAME=winx` in `port_stage_hottest2.py`: STG001–006 → `stages/WINX CLUB/` (30 MB): Alfea,
    Entrance Hall, Redfountain, School Library, Forest Village (no plate: descriptive),
    City of Magix. Previews match the select thumbnails.
  - Format check (in Blender): 45 models round-trip, ≤ 64 bones, palettes ≤ 52, 339 .anm parse,
    keys unique across all 722 sources' models, labels ≤ 15 bytes, every twin exists: 0 problems.
  - README source list, research doc, script docstrings.
- NOT re-checked: a GAME=hp3 byte-identity regression (HP2–HP5 disc dumps are no longer on this
  machine). The hp code paths are unchanged by construction (same clip names, 2-tuple casts,
  `_fold_weights` is a no-op for rig-only weights, no DBBOX nodes on those discs).

## Step 3 design questions (transformation in the DLL)
- Trigger source: World's per-side combo (`song_reset::judge_diag(side).combo`, cheap per frame)
  at a threshold N, or the life gauge reaching full (needs a new gauge accessor; LIFE4 / FLARE /
  RISKY gauges never "fill"). Before 80 % of the song, once per side, human sides only.
- Switch visuals: World has no per-dancer scissor, so the Wii's bottom-up wipe needs a substitute
  (swap under the chenge_effect at the wipe midpoint 1.0–1.2 s; or a fade).
- Scene fade (0.5 s down / 0.5 s up) via stage instance tint (`intro_look`-style), the
  `/#3/#0` camera (port it as `<key>_event` into every stage? or ship it with the dancer),
  the chenge_effect TEB through `flight_fx` (bank shipped with the dancer).
- Twin loading: the Fairy arc loaded beside the outfit for transform-capable picks (frame-board
  slot budget), hidden until the switch; schedule continuity (the twin takes the outfit's
  playlist position).

## Deploy & test log
- none yet. Cabinet watch-list (dancers / stages only): the 24 rows under WINX CLUB with their
  labels; faces + wings; Fairy floats slightly; hair / wings static (by design for now); the six
  stages' UV animation (Redfountain), flip-books (Forest Village, City of Magix fountain).

## Maintainer decisions (2026-10-20, round 2)
- Trigger: combo ≥ 250 before 80 % of the song. Twin loaded alongside every transform-capable pick.
- In scope too: hair / wing / ribbon dynamic bones, the kira sparkle overlays, a real feet-up
  wipe (emulated in our own code / shaders).

## Round-2 RE so far (main.dol)
- zan node-name prefix table `801e3da8` (22 entries; node +0x2a = index + 1): … `DB_` = 0x15,
  `DBBOX_` = 0x16 (collected per model by `FUN_800d5020` → model +0x1d4 / +0x1d8).
- Dynamic bones: `FUN_800d4ea8` allocates per node a 0x24 param record {node world, parent
  world, model mtx +0x1e0, node +0x90, f32 (written per step), collider model, mtx +0x210,
  DBBOX count, DBBOX index list} + a 0xb4 solver state (`FUN_8010ec9c` init). Per frame
  `FUN_800d52c0` → `FUN_800d5114(param, model, node)` for nodes with +0x98 set → solver step
  `FUN_8010d430(out_mtx, state, params, node+0x9c config)`; helpers `FUN_8010d864` (orient from
  a target), `FUN_8010da98` / `FUN_8010dcd4` / `FUN_8010e8fc` (DBBOX collision), `FUN_8010e138`
  (damping), `FUN_8010e224` (angle limits from config +0x30.. / +0x50..), `FUN_8010e30c` /
  `FUN_8010e494` (angles ↔ positions), `FUN_8010e5c8` (spring blend). Reset `FUN_8010ed1c`
  (`FUN_800d5248`). The config is per node in the ZMB (+0x9c); not yet decoded.
- DLL capability survey (subagent): additive dancer meshes are possible at the format level
  (`ddr_flags` 0x06C1 / `ddr_flags2` 4 on the object), stay stock-shaded; dancer parts have no
  `.sanm` path yet (small change in session.rs / director.rs); no engine clip plane — a world-Y
  wipe needs a synth shader variant (`mdl_cel.hlsl` + world-y interpolant + a per-item threshold,
  e.g. c2.w written in `node::visit(2)`); spare bones can be driven between `evaluate_into` and
  the body publish in `director.rs` (needs `dt` + names + per-dancer state + reset on Latched /
  Rewound); a twin = new `InstanceKind::Twin`, its arc via `Pick::arcs_for`.

## Cameras (maintainer playtest, 2026-10-20)
- NO stage port ships the games' per-song camera choreography: HP1's `c_000_NN` takes, HP4 / HP5
  `dance/DANCE_*_FRE.bin` / `_CHO.bin`, A3's own `camera_music_*.arc` and Winx Club's
  `ssq/<lang>/SSQ<nnn>.bin` (whole-song camera takes: SSQ002 /#0 is a 115 s clip; cued by
  `FUN_800307f0` → `FUN_8003bec8`) are all unported. Every port gives the director the stage's own
  shots (`_st`) + GAME_DEF_CAM `/#0` close-ups (`_non`), cut on World / A3's stage-mode cadence.
- Open: retime the Wii takes onto World songs by bars, or derive a cut grammar from them.

## Implementation plan (round 2; RE write-up: research doc §7)
Order chosen so each step is cabinet-testable on its own:
1. **Dynamic bones** (data + DLL): port keeps `bone_head` / `rot_DB*` / `DB_*` (head, under
   `mii_head`) and the wing chain (under `AccSpine1`) instead of folding them; writes
   `pl_<key>/dynbones.txt` (per bone: parent, length, rest local, gravity, z / y limits,
   stiffness, damping, excluded DBBOX ids; DBBOX boxes as joint-local AABBs). DLL: pure
   `dynbones_logic.rs` (the FUN_8010d430 step, harness-tested against a Python reference in
   `zan_dump`), state per dancer in `Session`, solved in `director.rs` after `evaluate_into`
   (before Big Head / publish), real `dt` passed into `produce`, reset on Latched + Rewound.
   Roots stay at rest (HAIR_MOT cues are per Winx song).
2. **Kira overlays**: additive meshes kept in the body (ddr_flags 0x06C1 / flags2 4) + a dancer
   body `.sanm` (u scroll 1 tile/s); DLL: `ParsedDancer.material_clip`, publish with materials.
3. **Twin + trigger + sequence** (DLL): `InstanceKind::Twin`, twin arc via `Pick::arcs_for`,
   combo ≥ 250 before 80 % (`song_reset::judge_diag(side).combo`, human sides only), the
   0.5 s fade / 2 s transform / pose-until-next-clip schedule, `/#3/#0` camera (port it as a
   dancer-side `.camanm`), chenge_effect via flight_fx (bank shipped with the Fairy dancer).
4. **Wipe**: `_wipe` PS variants (stock `mdl_ch_constant_vc` + synth lit / cel) with
   `clip(side · (vPos.y − c2.w))`, threshold written per frame; crossfade fallback.
5. **Wing sparkles** (`chara_effects.TEB` at DB_*wing_02): after the emitter binding RE.

## Session 3 (2026-10-20) — the five features, implemented in one batch
- Step 1 dynamic bones: `zan_dump.{is_dynamic_bone, dynbone_config, dbbox_id, dynbone_step,
  dynbone_world, format_dynbones, parse_dynbones}` + 5 tests (38/38). Port (`GAME=winx` only):
  `winx_extras` keeps the head / wing skeletons (≤ 59 bones; `FX_*` wing markers skipped — duplicate
  names), `clip_worlds(.., extras)` evaluates them at rest, `dynbones_sidecar` writes
  `pl_<key>/dynbones.txt` (21 dancers; Tecna's outfits have no DB nodes). DLL: `dynbones_logic.rs`
  (6 tests, cross-checked vs the Python reference ≤ 2e-3 m), per-dancer `WinxRun::dyn_body/dyn_twin`,
  solved in `director::produce` after `evaluate_into`, before Big Head, 60 Hz steps on the real
  clock; reset on Latched / Rewound / a real-clock jump; segment cut / clip-family change = teleport.
- Step 2 kira: additive slots kept (material-level `ddr_flags` 0x06C1 / flags2 4 — new override in
  `export_model._mesh_flags`, a no-op for every other port), `pl_<key>/pl_<key>_play_loop.sanm`
  (12 bodies: every Fairy + CHR24 / 33 / 34 / 35 / 36 outfits); DLL `WinxBody::material_clip`,
  `director::body_material_params` (real clock).
- Step 3 transformation: marker moved to `pl_<key>/transform/<twin>.txt` (the planner sees member
  names only; the text adds `lift`); `camera/transform.camanm` (GAME_DEF_CAM /#3/#0, 18 outfits);
  `port_winx_fx.py` writes `winx_fx/chenge.*` + 4 pools per Fairy. DLL: `transform_logic.rs`
  (pure; 5 tests), `selection::own_twin_from_members` + member paths, `DancerCandidate.twin`,
  `pick::transform_pick` (+ test), `Pick::{twins, sides}`, `InstanceKind::{Twin, CharaFx}`
  (+ test), `session::{parse_winx_body, parse_twin, WinxRun, TransformRun}`,
  `director::{winx_prepass, produce_twin, transform_camera}`, `lifecycle::transform_trigger_tick`.
- **Trigger refined by the maintainer mid-session:** a held FULL COMBO of any kind (no Boo / Miss
  / N.G.; Good counts) when the song reaches 80 % — difficulty-independent (was: combo ≥ 250
  before 80 %). `transform_logic::{is_full_combo, past_trigger_point, should_trigger}`.
- Step 4 wipe: c2.w verified free on non-hull items; 5 new blobs (`build_shaders.sh`, every old
  blob byte-identical), `shader_layout::WIPE_VARIANTS` (+ test), synthesis "v8" + `wipes_available`;
  frame board AUX word (+ test), `SceneNode.aux_role` (+0x82; hull = hide while set),
  `render_item::{repoint_all_materials, set_model_param_w_raw}`; NDC-based (resolution
  independent). Crossfade fallback with one WARN.
- Step 5 wing sparkles: binding RE'd (subagent, Ghidra): main.dol NEVER plays CHARA_EFFECT
  (research §7.4) → the maintainer's fallback: at both `DB_*wing_02` joints (where the wings'
  `FX_wing` markers sit), continuously, `chara_fx::JointFx`, 1 pool of 70 quads per Fairy.
- Re-port: all 24 dancers + `port_winx_fx.py` (6 Fairies); previews checked (rest montage, transform
  frames); format check (plain python): 54 models round-trip, ≤ 64 bones, palettes ≤ 52, 336 .anm,
  12 .sanm (targets in their models), 18 .camanm, 21 dynbones (every name in its .b2it), 18 twins
  installed, 12 fx layouts (pool bone counts match): 0 problems.
- Gate: `cargo fmt`, `cargo check`, `./build.sh`, `validate_background_dancers.sh` (288),
  `validate_overlay_draw.sh` (27), `test_zan_formats` (38), `validate_wii_ddr_tools.sh` (0 problems).
- HP regression (no HP disc on this machine — by construction): every new port path is behind
  `GAME == 'winx'` (`winx_extras` returns [] otherwise; `clip_worlds` without extras = the old
  code; the kira branch keeps the old `SKIP additive` for HP; dynbones / sanm / camera / marker
  writers are winx-only; the stale-dir removals are no-ops); `export_model`'s material override
  needs a material `ddr_flags` prop no HP port sets.

## Cabinet watch-list (run #1)
1. Boot log: `shader_synthesis: ... wipes=true`, containers `mdl_ch_constant_vc_wipe` / `_lit_wipe` /
   `_cel_wipe` written once (fingerprint v8 re-synthesizes everything once).
2. A Winx outfit pick: INFO `winx -- dancer 1 winxbloom01: 5 dynamic bone(s) / 1 collider(s), twin
   winxbloom04 (9 dynamic bone(s), lift 0.090 m, 12 clip(s) retargeted, event camera yes)`; the pick
   summary ends `TRANSFORM[1->winxbloom04]`; `[dancer] transformation wipe -- N material(s) on the
   wipe containers` and `[twin]` the same. No `instances exceed the frame board` WARN (Redfountain
   is the biggest stage).
3. Physics: hair / side locks / Layla Denim's ribbons / wings sway and settle, never explode or
   jitter at 120 Hz; no fling at clip cuts; hair does not pass through the back (DBBOX); Big Head on
   still works. Previews show it too.
4. Kira: sparkle overlays on every Fairy + CHR24/33/34/35/36 outfits scroll (≈ 1 tile / s), additive.
5. Transformation (play to 80 % with a full combo, any difficulty): INFO `Enchantix transformation --
   P1 full combo ...`; 0.5 s fade to black → the event camera on the dancer (check the framing in 2P
   — the shot is moved by the dancer's x, an assumption) → fade in, both forms play the
   transformation, the line rises from the feet: outfit above, Fairy below, the ribbon/sparkle
   effect riding the line; the other dancer hidden → pose → INFO `... switch at ...` on the next bar
   → the Fairy dances the outfit's choreography (lifted ~9 cm), stage cameras back. Watch: line
   direction / position (NDC sign), outline hulls absent only during the wipe, the effect's scale,
   kira clipped by the wipe too.
6. No transformation: a combo break before 80 %, the bot side, previews, flight stages, a standalone
   Fairy. A rewind / quick restart before the trigger re-arms it (INFO `... undone`).
7. With shader-fixes OFF: one WARN `... crossfades instead`; the forms crossfade over 2 s.
8. Wing sparkles on Fairy dancers (standalone and after the switch) at the wing tips' joints;
   no hitch.

## Deviations & open questions
- Labels decided: X / X Denim / X Dance / X Fairy (CHR2n is the denim set — the first guess had Dance / Denim swapped).
- Stages decided: STG001–006.
- Decided (maintainer, session 3): full combo held to 80 % (any grade), any stage but flight, any
  difficulty.
- Assumptions to confirm on the cabinet: the event camera moved by the dancer's placement; the
  switch on the next BAR (a Wii piece) rather than the next World clip; the wing sparkle binding
  (§7.4); the collision approximation (§7.1).
- Not ported: the dancer mode 5 → 6 at 1.2 s, the transformation sound, the APL hair / col tracks,
  HAIR_MOT / COL_MOT per-song drivers (roots at rest).
- A second side that reaches 80 % with a full combo while the first transforms waits for the first
  switch and must still hold its full combo then.
