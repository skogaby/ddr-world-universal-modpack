# Research: era sounds, skin-1 option forcing, folder trigger (2026-09-22)

Scope: the three A3 behaviours `docs/ddr_selection_research.md` §2.5 / §7.2 /
§7.4 / §7.5 deferred. Specification = A3 20240402; targets = World 20260825
(current) and 20250805 (oldest). Addresses file-relative to `0x180000000`.
Read-only RE (no Ghidra edits). Scratch parsers used for bank listing:
`$TMPDIR/…/ddr_selection_audio/xact.py` (XSB/XWB v43 lister, same field rules
as `src/core/xact/xwb.rs` and `src/services/se_bank_synth/xsb.rs`).

**Headline corrections to the research doc**

1. **The era cues are NOT absent from World's data.** World ships — but never
   loads — the A3-generation `_n` bank set: `data/sound/win/voice_n.xwb`
   (byte-identical to A3's), `data/arc/soundbanks_n.arc` (`voice_n.xsb`,
   `se_normal_n.xsb`, …) and `data/arc/se_normal_n.arc` (`se_normal_n.xwb`).
   Every cue A3's skins use is in them; every needed SE wave is byte-identical
   to A3's. **No operator-supplied data is needed** on a stock install.
2. A3 had **three** skin-dependent sound sites, not one: `CallVoiceActor`,
   the `ReadyGoActor` HERE-WE-GO voice (skin 1), and the stage-choice shutter
   (cut-in SE `sele_*` + stage-call voice).
3. A3 DDR SELECTION membership is a **fixed list of 54 mcodes** (bucketed by
   series), not "all songs of a series". All 54 exist in World with the same
   series values.
4. World's `ddr::player::Option` layout is **completely different** from A3's
   `CourseOption` (no field offset carries over; enum orders differ too —
   guideline OFF is 0 in A3, 2 in World; arrow colour FLAT is 2 in A3, 3 in
   World). Full mapping in §B.
5. World publishes the **current folder's type id at `GameWork+0x1C`** on
   every folder change — the folder trigger's signal exists.

---

## A. Era sounds

### A.1 A3 `CallVoiceActor` — complete behaviour

| | A3 20240402 |
|---|---|
| ctor / onUpdate / onMessage | `FUN_180036890` / `FUN_1800369e0` (vt slot 6) / `FUN_180037220` (slot 8); vtable `0x1802689a8` |
| guarded voice play | `FUN_180037140(ctx, cue)`: skip if handle `+0x84` still playing (`FUN_1800fd310`), else mute-filter check `(*DAT_1802ee600)(5) != 6`, then `FUN_1800fd1f0(3, cue, 0.0)` (slot 3 = voice), store handle at `+0x84` |
| direct slot play | `FUN_1800fd1f0(slot, cue, t)` = `GetCueIndex` + `Play` on `mgr[(slot+1)*0x10]` + `FUN_1800fd000` (register in the 256-entry cue table). SEs use slot 2 (`se_normal`) |
| thresholds | low `0.2` (`DAT_180265034`), `0.4` (`DAT_1802645f8`), `0.7` (`DAT_1802888ac`), high `0.8` (`DAT_180288c60`) |
| combo tables | `0x1802db970` = `vo_ingame_combo_100…1000` (10); `0x1802db9c0` = `sn2_dgm25…34` (10, index = combo/100−1) |

Actor fields (identical on A3, World 20260825 and 20250805 — verified from the
A3/20260825 ctors + onMessages and all three onUpdates):

| Off | Meaning | Written by (A3 msg / World msg) |
|---|---|---|
| `+0x58+i*8` | step state (0 wait, 1/2 running, 3 disabled) | `0x104b`/`0x1048` start → 1; `0x1052`/`0x104f` → 3 |
| `+0x84` | last voice cue handle (−1) | the actor |
| `+0x88`,`+0x8c` | song time, second time word (regain gate `> 20000`) | `0x1048`/`0x1045` payload [1],[2] |
| `+0x90` / `+0x94` | next state-voice / next crowd-SE time; seeded `t+0xBE70` / `t+0xFC7C` at start, step `+0x8000` / `+0x10000` | the actor |
| `+0x98` | next combo milestone (`(c/50+1)*50`) | the actor |
| `+0x9c` / `+0x9d` | voices off / SE off (musicdb `<voice>` 1/2/3) | `0x1052`/`0x104f` (song info `+0xE7` A3 / `+0x17F` World) |
| `+0x9e` | "was low" (regain latch) | the actor |
| `+0xA0/+0xA4/+0xA8` side 0, `+0xAC/+0xB0/+0xB4` side 1 | gauge f32 0..1 / combo / difficulty (ctor args) | `0x1041,0x1042`/`0x103e,0x103f`; `0x1036`/`0x1033` |
| `+0xB8` | players left; 0 ⇒ actor dies | `0x104d`/`0x104a` |

Per frame (state ∈ {1,2}): `c` = max combo, `g` = max gauge, `d` = difficulty
of the higher-gauge side. "guarded" = `FUN_180037140` (skipped while the
previous voice plays); `G` = additionally only when `c % 100 < 91`.

| Trigger | skin 0 (A3 current) | skin 1 (1st–5th) | skins 2,3 (MAX–EXTREME, SuperNOVA) | skins 4,5 (X, 2013–A) |
|---|---|---|---|---|
| combo crosses a multiple of 100, ≤ 1000 | `vo_ingame_combo_<N>` (direct, unguarded) | — | `sn2_dgm25+N/100−1` (direct) | as skin 0 |
| combo multiple of 100, ≥ 1100 | `vo_ingame_combo_over` (G) | — | — | as skin 0 |
| combo crosses an odd multiple of 50 | `vo_ingame_combo_gen` (G) | — | — | as skin 0 |
| state voice due, `g > 0.8` | `vo_ingame_high` (G) | **`ACT6`** (G) | **`sn2_dgm_high`** (G) | as skin 0 |
| `0.2 ≤ g ≤ 0.8`, not (was-low ∧ `+0x8c>20000`) | `vo_ingame_gen` (G) | — | **`sn2_dgm_middle`** (G) | as skin 0 |
| same, was-low ∧ `+0x8c > 20000` | `vo_ingame_regain` (G) | — | — | as skin 0 |
| `g < 0.2` (sets was-low) | `vo_ingame_low_hard` (d∈{3,4}) / `_low_easy` (G) | — (latch not set) | — | as skin 0 |
| crowd SE due | `g>0.7`: `STG_APP02` + `vo_ingame_cheer`(G); `g≤0.4 ∧ c<13 ∧ g<0.2`: `STG_BOO` + `vo_ingame_boo`(G); `g≤0.4 ∧ c<13 ∧ g≥0.2`: nothing; else `STG_APP03` + `vo_ingame_cheer`(G) | unless `g≤0.4 ∧ c<13`: **`2nd_BIG2`** | unless …: **`2nd_KANSEI_B`** (2) / **`STG_APP03`** (3) | unless …: **`STG_APP02`** |

Every play is also gated by the mute filter `(*DAT_1802ee600)(5) != 6`.

### A.2 Other A3 skin-dependent sounds (complete list)

A scan of every `GameWork+0xB0` read within 30 instructions of a `GameWork`
load, plus every package-record-skin reader (`FUN_18004d830` callers), finds
only these three sound sites:

| Site (A3) | Skin source | Behaviour |
|---|---|---|
| `CallVoiceActor::onUpdate` `FUN_1800369e0` | `GameWork+0xB0` | §A.1 |
| `ReadyGoActor::onMessage` `FUN_180042570` (ctor `FUN_180042000`, vt `0x1802691a8`; onInit `FUN_1800420d0` sets `+0xA4` = `dance_message` package-record skin) | package record | on `0x104b` (HERE WE GO) with `+0xA4 == 1`: voice **`ACT3_1`**, or **`ACT4_2`** on the final stage (`FUN_180123a20`). Other skins: no voice (A3 has no READY voice at all — `vo_ingame_ready` is not referenced by the A3 binary) |
| stage-choice shutter `FUN_18002f5f0` (`+0x194` via msg `0x100B`) | msg `0x100B` | state 1 (cut-in `choice_cutin`): SE slot 2 **`sele_1st` / `sele_ext` / `sele_sn2` / `sele_x2` / `sele_2013`** for skins 1–5; state 4 → stage call `FUN_18002e210(skin)`: skin 1 **silent**; skins 2/3 **`FUN_18002e060`** = `sn2_etc` + `a{stage+2}` (stage ≤ 3) / `a7` (final) / `73` (one past max stage); others `vo_stage_01..`/`final`/`extra` |

Also noticed in the shutter (non-sound, for the shutter design): skin 3 loads
the song **banner** (`data/arc/banner/%s.arc`) instead of the jacket (state 3);
skins 1/2 pause the jacket clip (state 4). Results voices (`FUN_1800aead0`)
and the danger/game-over actors are skin-independent.

### A.3 Where the cues live

A3 loader `FUN_180024c60` loads exactly the `_n` set (`se_system_n.arc`,
`se_normal_n.arc`, `voice_n.xwb`, `bgm_menu_n.xwb`; XSBs from
`soundbanks_n.arc`). World (both builds) references only the non-`_n` names
(`data/arc/soundbanks.arc`, `se_normal.arc`, `voice.xwb`, `bgm_menu.xwb`) —
its `_n` files are shipped, unloaded leftovers.

| File | A3 | World install | Internal names | Type | Cues |
|---|---|---|---|---|---|
| `data/sound/win/voice_n.xwb` | 45 707 264 B | **identical md5** | wb `VOICE` | streaming (`0x90001`, align 2048), 566 waves, all MS-ADPCM mono 22/44 kHz | — |
| `soundbanks_n.arc → voice_n.xsb` | 14 550 B | same cue set | sb/wb `VOICE` | 39 simple + 82 complex cues | all `vo_ingame_*` (A3 announcer), `ACT3_1/ACT4_2/ACT6`, `sn2_dgm25..34`, `sn2_dgm_middle/high`, `sn2_etca2..7`, `sn2_etc73`, `vo_stage_*` |
| `se_normal_n.arc → se_normal_n.xwb` | 19.48 MB | differs only by `+se_edit_tick`; the 10 needed waves byte-identical | wb `SE_NORMAL_n` | in-memory (`0x90000`, align 4, seek seg empty), 258 waves, ADPCM mono/stereo | — |
| `soundbanks_n.arc → se_normal_n.xsb` | 10 218 B | 10 256 B | sb/wb `SE_NORMAL_n` | 249 simple + 1 complex | `STG_APP02/03`, `STG_BOO`, `2nd_BIG2`, `2nd_KANSEI_B`, `sele_*` |

World's **loaded** banks (`voice.xsb` sb `voice`, 990 waves; `se_normal.xsb`
139 cues) contain none of these names — confirmed by cue-name set difference.

Cue shapes: SEs are simple cues (one bare sound, category 6); voice cues are
simple (`sn2_dgm25..34`, category 7) or complex cues with a **type-1 variation
table** (flags `0x000B`: sound-offset entries, random-no-repeat) whose every
sound is a bare simple sound (flags `0x00`, no RPC/DSP). Wave payload needed:

| Set | Waves | Bytes |
|---|---|---|
| skins 1–3 voices (`ACT6`, `sn2_dgm_*`, `sn2_dgm25..34`) | 23 | ≈ 1.2 MB |
| A3 announcer for skins 4–5 (`vo_ingame_*`, 20 cues) | 128 | ≈ 11.4 MB |
| SEs (`STG_APP02/03`, `STG_BOO`, `2nd_BIG2`, `2nd_KANSEI_B`) | 5 | 0.87 MB |
| stage calls / HERE voice / cut-in SEs (`sn2_etc*`, `ACT3_1`, `ACT4_2`, `vo_stage_*`, `sele_*`) | ~25 | small (not summed) |

### A.4 World's `CallVoiceActor` — the seam

| | 20260825 | 20250805 |
|---|---|---|
| ctor / onUpdate / onMessage | `FUN_180055260` / `FUN_1800553b0` / `FUN_180055ad0`; vtable `0x180360698`; RTTI `.?AVCallVoiceActor@dance@sequence@@` `0x180482dc0` | onUpdate `FUN_180051f70` (same field layout, same logic) |
| created by | DPS onInit `FUN_1800573d0` @ `0x180057cde` (args = side infos `+4`), Matching `FUN_180061520` | — |
| guarded voice | `FUN_1800559f0`: `FUN_1801ab360(mgr, +0x84)` is-playing → mute filter `(*DAT_1806f2428)(5)!=6` → `FUN_1801ab240(3, cue, 0)` under the AVS lock | `FUN_1800525b0` |
| SE | slot-2 bank `*(mgr+0x30)` `GetCueIndex`/`Play` + `FUN_1801ab050(mgr, cue, 2, 0)` | same |
| logic | one table: `vo_ingame_combo_%04d` for {100..1000,2000,3000,4000} else `_combo_other`; states `vo_ingame_state_01_highest` (g ≥ 1.0) / `_02_high` (> 0.7) / `_04_danger` (< 0.2) / `_03_regain` / `_99_other`; SEs `se_kansei_big` (g > 0.7) / `se_kansei_\x82\x8Diddle` (`0x180360660`; the Shift-JIS-mangled "middle" is also the cue's name in World's `se_normal.xsb`, so it resolves) / `se_kansei_small` | same |

Everything the A3 logic needs is on World's actor at A3's offsets, and World's
own `onMessage` keeps filling them. **The seam is a detour on `onUpdate`
(vtable slot 6, resolve via the RTTI vtable — build-proof on both builds):
armed skin ∈ 1..5 ∧ era bank registered ⇒ run the A3 rules; else call the
original.** No message handling changes.

### A.5 Recommendation

1. **Era bank (pure, host-tested):** at enable, on a background thread, read
   the four `_n` files from the install (LayeredFS-aware lookup; `std::fs`,
   ~2–13 MB of targeted reads out of `voice_n.xwb`), and build ONE in-memory
   bank pair holding exactly the needed cues: XWB = N entries copied verbatim
   (ADPCM, per-entry format/duration), align 4, empty seek segment, mod-owned
   internal name (e.g. `dsel` — never `VOICE`/`voice`/`SE_NORMAL_n`: the engine
   pairs by internal name globally, and the case-sensitivity of that match is
   only inferred — `VOICE` vs World's `voice` must not depend on it); XSB = the needed simple cues + complex
   cues with their type-1 variation tables, each sound byte-copied (category
   6/7, volume, pitch, priority kept) with wave index remapped and
   `wavebank_index = 0`. Needs an N-entry XWB writer and a variation-capable XSB
   writer (`se_bank_synth` today writes one entry / one simple cue; reuse its
   CRC-16 and cue-hash functions, `src/services/se_bank_synth/xsb.rs:97`).
   Offline validation: round-trip against the parsed source banks.
2. **Registration:** `game_audio::register_bank` (`src/services/game_audio.rs:491`,
   slot 4 — currently unused by every mod; assist-tick uses the slot-less
   `register_tick_bank`, `:765`) on the game thread at the first armed
   SONG_SELECT→stage edge. ~14 MB leaked for the full set (tick bank precedent
   is 28.9 MB); a skins-1–3-only build is ~2 MB.
3. **Playback API additions** in `game_audio`: `play_cue` returning the handle
   (today `:704` returns bool) and `is_cue_playing(handle)` — derive the
   game's `FUN_1801ab360` from the guard helper's first `CALL` (guard helper =
   the callee invoked with RDX = `vo_ingame_state_*`). Calling the public
   `se_play(4, cue, 0.0)` reproduces the game's own mute-filter + AVS-lock
   behaviour (slot 4 ∉ {1,5}, so the filter applies, like slot 3 today), and its
   handle lives in the same cue table the `+0x84` guard reads.
4. **Detours** (all game-thread, per-frame-cheap):
   - `CallVoiceActor::onUpdate` — A3 rules §A.1 for skins 1–5 (pure decision
     function over the field snapshot → list of effects, host-tested);
     skin-1 HERE voice: `ACT3_1`/`ACT4_2` on the first frame the actor's state
     becomes 1 (World msg `0x1048` = A3 `0x104b`, the same broadcast the
     ReadyGoActor used), final = World `FUN_1801de170(GameWork+0xC)` or
     `stage_records`.
   - stage-call voice World `FUN_180033760` (20250805 `FUN_180033540`; sole
     caller = shutter update `FUN_180033f60`): skin 1 → return; 2/3 →
     `sn2_etc*` per `FUN_18002e060` (gate fields via `stage_records`:
     course, event mode, stage, final override, max stage); 4/5 → A3
     `vo_stage_*` from the bank (or passthrough — design choice).
   - optional: suppress World-only `vo_ingame_ready` (two plays inside
     `FUN_180033f60` @ `0x1800346f8` / `0x1800348ba`) while armed — A3 never
     played a READY voice.
   - `sele_*` cut-in SE belongs to the cut-in re-host (World has no
     `choice_cutin`); cues go in the same bank.
5. **Data contract:** nothing to copy on a stock World install. Optional
   override folder (e.g. `data_mods/ddr_selection/sound/`) accepting either
   A3's or World's `_n` files (compatible). Missing/invalid ⇒ one WARN, the
   onUpdate detour passes through (World's announcer keeps playing), the rest
   of DDR SELECTION unaffected.

Design choice to record: skins 4/5 in A3 used A3's *current* announcer
(`vo_ingame_*`). Literal A3 fidelity = those A3 voices (+11.4 MB);
rule-fidelity = World's announcer (passthrough + only the SE differs).

**Effort:** 4–6 days (bank builder + tests 2 d, decision function + detours
2 d, stage/HERE voices 1 d, cabinet validation). **Risks:** `_n` files absent on
some cabinets (only one World install inspected — verify on others); XSB
complex-cue/variation encoding (engine validator is strict and silent — CRC,
offsets); name-case semantics of bank pairing (sidestepped by a unique name).

---

## B. Skin-1 option forcing

### B.1 A3 `CourseOption` (vtable `0x1802806E8`) — confirmed names

Names from the course option-name table `0x1802db370` (entries
`{type, 1, block_index, …, name}`: 0 `speed`, 1 `boost`, 2 `turn`, 3 `dark`,
4 `scroll`, 5 `color`, 6 `cut`, 7 `freeze`, 8 `jump`) — the course-fixed block
`this+0x90` is indexed by exactly those — plus the setter/getter vtable pairing
(`+0x18/+0x20` speed … `+0xe8/+0xf0` gauge) and the consumers:

| getter vslot | field | name | skin-1 value | meaning (evidence) | course guard `+0x90` |
|---|---|---|---|---|---|
| `+0x20` `FUN_180126ab0` | `+0x0C` | speed | 3 | ×1.00 (GPA: `(v+1)·0.25`, `0x180288848`) | yes (block[0]) |
| `+0x30` `FUN_180126b20` | `+0x10` | boost | 0 | normal (`%s_boost_%s`) | yes (block[1]) |
| `+0x40` `FUN_1801267d0` | `+0x14` | appearance | 0 | visible (`visible, hidden, sudden, hidden+, sudden+, hidden+_sudden+, stealth`; GPA creates lane covers for 3..5) | **no** |
| `+0x60` `FUN_180126bc0` | `+0x1C` | dark (step zone) | 0 | step zone shown (GPA: `spot.visible = (v != 1)`) | yes (block[3]) |
| `+0x70` `FUN_180126c40` | `+0x20` | scroll | 0 | normal (`==1` reverse) | block[4]==1 flips |
| `+0x80` `FUN_180126c00` | `+0x24` | color | 2 | **FLAT** (icon table `{vivid, note, flat, rainbow}` @ `0x18002c6de`) | yes (block[5]) |
| `+0xC0` `FUN_1801267f0` | `+0x34` | arrow (shape) | 2 | `2d_arrow02` = classic | **no** |
| `+0xD0` `FUN_180126810` | `+0x38` | filter | 0 | off (filter actor `FUN_1800497b0`) | **no** |
| `+0xE0` `FUN_180126830` | `+0x3C` | guideline | 0 | **OFF** (`GuidelineRenderer` draw `FUN_18001f240`: `mode==0 ⇒ return`; 2 = centre offset) | **no** |

Not forced: turn, cut, freeze, jump, gauge (`+0x40`) and the rest. In practice
the course guard is moot — a course never commits through a DDR SELECTION
folder, so the skin is always 0 in courses.

### B.2 World `ddr::player::Option` (identical layout on 20260825 and 20250805)

Setter layout from the `ddr::player::Option::Set*` assert strings (both
builds); vtable 20260825 `0x180387998` (COL `0x180387990`); getters are
4-byte `MOV EAX,[RCX+off]; RET` stubs. PlayerWork offset `+0xE0` / `+0xF0`
(old) — `stage_records::player_option_offset()`. Value orders from the name
table `0x1804b4288` (20260825), confirmed against PlayerWork-ctor defaults
(`FUN_1801e70a0`) and consumers.

| A3 option | World field | World name (wire node) | getter vslot | World enum | forced value |
|---|---|---|---|---|---|
| speed ×1.0 | `+0x08` SpeedType + `+0x0C` Hispeed | `speed_type` + `hispeed` | `0x208` / `0x220`; effective `0x218` (`FUN_1801e27d0`: type 1 → `+0x0C`, type 0 → `+0x10`) | `speed_type` {0 real_speed, 1 speed_rate}; hispeed ×100, 25..800 | **type 1, hispeed 100** (type 0 would be recomputed by `song_rate::real_speed`, which only touches type-0 sides — `real_speed.rs:249`) |
| boost | `+0x54` ScrollMoving | `scroll_moving` | `0x2A8` | {normal, boost, brake, wave} | 0 |
| appearance | `+0x28` Visibility **and** `+0x34` LaneCover | `visibility`, `lane` | `0x250`, `0x268` | {normal, constant, stealth}; {off, hidden, sudden, hidden_sudden} | 0, 0 |
| dark / step zone | `+0x40` Stepzone | `stepzone` | `0x280` | {on, off} (GPA `FUN_18005cc70`: `spot.visible = (v==0)`) | 0 |
| scroll | `+0x1C` ScrollDirection | `scroll_direction` | `0x238` (`0x2F8` = is-reverse) | {normal, reverse} | 0 |
| color FLAT | `+0x5C` ArrowColor | `arrow_color` | `0x2B8` | {note, rainbow, vivid, flat} | **3** |
| arrow shape 2 | `+0x60` ArrowDesign | `arrow_design` | `0x2C0` (GPA init `FUN_18005be20` → `2d_arrow%02d`) | {normal, x, classic, cyber, medium, small, dot} | 2 |
| filter off | `+0x30` LaneTransparency | `lane_filter` | `0x260` (filter actor `FUN_18006a710`: alpha = (100−v)/100) | 0..100, default 70 | **100** |
| guideline off | `+0x3C` Guideline | `guideline` | `0x278` | {center, border, off} (renderer `FUN_180025db0`: `mode==2 ⇒ skip`) | **2** |

World-only fields A3 did not force (leave alone for fidelity): TimingDisp
`+0x20`, TimingMusic `+0x24`, ConstantValue `+0x2C` (inert once Visibility =
normal), FastSlow `+0x38`, Combo/Judgement draw order `+0x44/+0x48`,
JudgementLayout `+0x4C`, LaneNotice `+0x50`, ArrowPlacement (turn) `+0x58`,
Cut* `+0x64..+0x6C`, lane positions `+0x70..+0x78`, gauge `+0x18`.
World's option resolver `FUN_1801ea0e0` has no course-fixed block (the only
override is a static default Option for mcode `0x9733` in a special mode) —
the A3 course guard needs no port.

Readers (20260825): GPA init `FUN_18005be20` (`0x218`, `0x268`, `0x2C0`),
`FUN_18005cc70` (`0x250`, `0x278`, `0x280`, `0x2A8`, `0x2B8`), filter
`FUN_18006a710` (`0x260`), `FUN_180076c80` (`0x250`, `0x2A8`) — **all latch at
gameplay init**; the save marshal `ark::network::ReflectSavePlayerData`
`FUN_180018ee0` reads the same getters at save-build time; the options menu
`FUN_180164f90`. Repo readers: `mine_render.rs:287` / `training_mode` strip
read `+0x60` directly (consistent with forced values).

### B.3 Recommendation — per-side field writes with snapshot/restore

- Getter-level interception is not viable: the getters are 4-byte stubs (no
  room for a detour), a vtable swap would be shared by the options-menu copies
  and the save marshal, and several consumers (repo + possibly game) read the
  fields directly.
- **Write** the 11 fields on every side that will get a GamePlayActor, at the
  SONG_SELECT→stage edge (A3 applied the forcing from the commit on, so the
  stage-choice shutter also sees forced values), **after** the multiplayer
  bot's impersonation copy (`impersonation.rs:71` copies `+0x08..=0x6C`) —
  i.e. force both sides whenever the bot is armed; re-assert idempotently at
  the 27→28 edge (quick restart builds a fresh DPS that re-reads them).
  Snapshot once per window.
- **Restore** on the first scene outside {26,27,28} (the per-song-offsets
  shape, `override_hook.rs:185/295`); the savekind-2 marshal runs after
  results and the logout save later, so the restore precedes both.
  Belt-and-braces: if a save is built while armed, rewrite the 11 native
  nodes (`speed_type hispeed scroll_moving visibility lane stepzone
  scroll_direction arrow_color arrow_design lane_filter guideline`) with the
  snapshot via `custom_options_persistence::replace_option_s32`
  (`custom_options_persistence.rs:1157`, the `timing_music` precedent `:1069`).
- Interactions: `per_song_judgement_offsets` (`+0x24`) disjoint;
  `song_rate::real_speed` opts out via SpeedType 1; multiplayer bot as above;
  `playfield_styling` / `player_perspective` / `overlay_element_styling` use
  their own rows (not Option fields). Open: World's in-song speed adjust
  (`ControlSpeedActor`, anytime_speedmod) still works under skin 1 — A3's
  getter would have masked a stored change but its in-song path was not
  traced.

**Effort:** ~2 days incl. host tests of the value table. **Risk:** low; the
one mistake to avoid is porting A3 values literally (enum orders differ).

---

## C. Folder trigger

### C.1 A3 wiring (complete)

- Membership: `FUN_1800f2460(music::Info*)` assigns the DDR SELECTION folder id:
  if `mcode` ∈ table `0x1802630d0` (**54 mcodes**) then by series
  (`vslot +0x78`): 1–5 → `0xB1`, 6–8 → `0xB2`, 9–10 → `0xB3`, 11–13 → `0xB4`,
  14–17 → `0xB5`; if `mcode` ∈ `0x1802631a8` (4: `ddrm suns2 syur anni`) →
  `0xB6` (event folder `sl06`). Callers `FUN_1800432f0`, `FUN_1800ee320`.
- Commit (`FUN_1800c53e0`, `FUN_1800ec8a0`): `GameWork+0x10` mcode, `+0x14`
  category (`FUN_1800f45f0`), `+0x18` folder id (`FUN_1800f4630`, walks the
  cursor list back to the folder header) → skin setter `FUN_180123360`.

| Skin | Songs (all present in World 20260825 with identical series) |
|---|---|
| 1 (9) | trip para bril para2 afte afro bfor burn stil |
| 2 (9) | maxx cand drte roll kaku bom2 radu bagg ichi |
| 3 (12) | gate fasc chao cach hana flow2 fway arra vemb alth plur sunk |
| 4 (12) | ontb sabe geis rint smoo delt poss tasf toho1 litp fwer alst |
| 5 (12) | mobu bedr synf anot ostt dind syak endr huia obor hope coli |

Each song is in exactly one bucket, so **"skin = bucket(mcode) when the song
was committed from DDR SELECTION"** is equivalent to A3's per-sub-folder rule.

### C.2 World equivalents

- World folders have no category/sub-id; each `FolderProperty` has a
  `type_id` at `+0x00` (1–7 genre/ALL MUSIC, 8/9 brave, 10 Dan, 0x63; custom
  folders `0x10+i`, `folder_expansion.rs:88`).
- **Signal:** `FUN_1800fd900(select_seq, shared_ptr<Folder>)` (20260825;
  20250805 same body near `0x1800f2060`, seq field `+0x110` vs `+0x130`) stores
  the current folder and writes **`GameWork+0x1C = folder->type_id`** on
  every folder change. Select init `FUN_1800fccf0` seeds `GameWork+0x18 ←
  PlayerWork+0x54` (mcode) and `GameWork+0x1C ← PlayerWork+0x4C` (last
  folder); the credit reset `FUN_1801dd6d0` zeroes it. The value is still
  read in the stage-choice shutter (`FUN_180033760` tests `+0x1C == 9`), so it
  is valid at the SONG_SELECT→stage edge. Derivation: the AOB
  `4C 8B 1B 45 8B 03 48 8B 05 ?? ?? ?? ?? 48 8B 08 44 89 41 ??` matches once
  on each of 20260825 (`0x1800fd92a`) and 20250805 (`0x1800f208a`); the
  trailing imm8 is the offset (publish via `publish_value`; run the 4-build
  sweep before relying on it). Committed
  song = `PlayerWork+0x54` of the governing side (AGENTS.md).
- Membership: `folder_expansion` membership is the per-song filter predicate
  `FUN_180144c50(int* bit, shared_ptr<InfoCommon>*)` (20260825; tests
  `music::Info+0x178` (fallback `+0x174`) bit, with the bit-8/`0x40` quirk;
  `FUN_180124420` is a second, multi-folder variant). It cannot express a
  mcode list. Options:
  1. **(recommended) mod-owned filter functor vtable** for the DDR SELECTION
     folder(s): clone the filter functor's vtable with the call slot pointing
     at a predicate that answers from the pure A3 table (mcode = `Info`
     vslot 0) — zero shared detours, only our folders affected
     (`custom_options/rows.rs::build_mod_vtable` shape). Has-songs is already
     forced true for configured bits (`folder_expansion.rs:412`).
  2. detour `FUN_180144c50` and special-case reserved bit indices;
  3. OR property bits into `Info+0x178` after DB load (fragile: `+0x178==0`
     fallback semantics, boot ordering) — avoid.
- Registration: `folder_expansion` only takes operator config entries
  (`bit_index`, `key`, `voice_key`); DDR SELECTION needs a code-side
  registration hook (built-in entries + predicate kind) so the operator does
  not hand-edit `mod-config.json`. Folder voice: World's `voice.xsb` has
  `vo_select_folder_selection` — use it as `voice_key`.
- UX: one "DDR SELECTION" folder (54 songs, skin by bucket — exact A3 skin
  semantics, fewer carousel slots) vs five folders (A3 navigation; carousel
  capacity is an open question in `docs/folder_system_research.md` §Open 3).

### C.3 Folder art

`folder_expansion` art contract (`folder_expansion.rs:110-127`, `:623`): per
key, PNGs under `data_mods/custom_folders/select_music_folder_v3_ifs/tex/`
(`mufo_folder_back_{key}_{on,off}` 240×164, `mufo_txt_folder_title_{key}_{on,off}`
240×126) and `…/select_music_folder_lang_eng_v3_ifs/tex/`
(`mufo_txt_folder_subtitle_{key}_{on,off}` 240×26, `mufo_txt_folder_info_{key}`
488×104), atlas-cloned at firststep's UVs; AFP/geo cloned from firststep.

Legacy art in the stock World install (unused by World):

| Legacy texture (arc) | Size | Best World target |
|---|---|---|
| `semuca_selection01..05_bnr` (`select_music_card_v2`) — era logos + year range | 468×92 | `mufo_txt_folder_info_{key}` 488×104 (pad; near-exact aspect) |
| `folder_ddrselection01..05` (`select_music_card_lang_{eng,jpn,kor}_v2`) — "[1998-2001] DanceDanceRevolution 1st-5th" | 348×24 | subtitle 240×26 (scale ×0.69) |
| `category_name_ddrselection[_b]`, `folder_version_ddrselection` | 208×56 / 208×104 / 348×24 | title 240×126 (centre, no scale) |
| — | — | back art 240×164: no legacy equivalent (hue-shifted firststep back or a banner crop) |

Generate at enable from the install (no Konami bytes in the repo), before
`folder_expansion` builds its atlases (ordering/API dependency).

**Effort:** 3–5 days (registration hook + functor vtable + trigger + art
generation). **Risks:** carousel capacity with 5 folders; custom type id saved
as last-play folder (existing folder_expansion behaviour); 20250805
`FolderProperty` layout differs (folder_expansion already detects it).

---

## Open questions

1. Are the `_n` banks present on every supported World install (only one
   install inspected; 20250805-era install not available)?
2. Skins 4/5: A3 announcer (`vo_ingame_*` from `voice_n`) or World's?
3. Suppress World-only `vo_ingame_ready` for skins 1–5?
4. Block World's in-song speed change under skin 1 (A3 in-song path not
   traced)?
5. One DDR SELECTION folder or five?
6. XSB variation-table writer: confirm the engine accepts a rebuilt complex
   cue table (validate offline, then one cabinet boot).
