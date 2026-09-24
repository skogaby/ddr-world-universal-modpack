# Research: how AFP-embedded sounds resolve to XACT cues (2026-09-22)

Scope: open question 1 of `intro-and-skin-surface.md` §6 — when a legacy
(A3-era) BM2D/AFP clip fires an embedded sound in World, which bank(s) does
the game search, and will a cue that exists only in a mod-owned bank in
manager slot 4 (`game_audio::register_bank`) be found? Read-only RE: Ghidra
`gamemdx_20260825.dll`, `gamemdx_20250805_STOCK.dll`,
`gamemdx_20240402_A3_Final.dll`, `libafp-win64.dll`; byte sweeps over the five
builds in `~/Desktop/ddr_modules` + the A3 DLL (`$DDR_A3_INSTALL/modules/`).
Addresses file-relative to `0x180000000`.

**Answer in one line:** the AFP sound callback picks exactly ONE manager slot
from the cue *name* (`vo_*` or a hard-coded 25-name A3 voice list → slot 3
voice, everything else → slot 2 se_normal) and does a single
`GetCueIndex`+`Play` on that bank; a miss returns −1 silently. **A cue that
lives only in slot 4 is never found.** One small pre-original detour on the
callback's play method (below) routes our names to slot 4.

---

## 1. The callback chain (World 20260825)

| Step | 20260825 | What it does |
|---|---|---|
| AFP bytecode | — | every legacy clip plays sounds as `asdlib.sound_play("<cue>")` (ActionScript method call on the `asdlib` object; 1 string arg). No SWF/AP2 sound tags (`0x0E/0x0F/0x65/0x81/0x85`) exist in any legacy package |
| libafp builtin | `FUN_180086360` (refs the `"sound_play"` string) → `FUN_180024de0` | `if (label) { if (dbg & 0x100) trace("render->sound_call(%s)"); (*cb_sound_call)(label); }` — **passes only the label string** (≤ 0x40 bytes). No bank, category, volume or pan. Gated per layer by the layer-attribute bit `0x2` (`afp_layer_check_attribute`); `afp_layer_create_with_property` initialises every layer's attribute word to `0x1000001F` (bit set), and no `afp_layer_set_attribute` (ord 56) call in gamemdx clears bit 1 |
| render-callback block | 0x138-byte table `0x180389980` copied to `0x1804617e0` by `FUN_18021cc20`, handed to libafp by `FUN_18025d150` (`afp_boot` / render setup) | entries 19..25 = `sound_call(label)`, `sound_stop(label)`, `sound_stop_all()`, `sound_volume(label,f)`, `sound_pan(label,f)`, `sound_fade(label,f,f,f)`, `sound_check_calling(label)` (arity + libafp's printf order) |
| dispatcher (entry 19) | `FUN_18021b9e0` | takes the AFP-state lock (`FUN_18021a610`, state object `*DAT_1806f2ff8`), reads `state+0x58` (the installed `agcs::Afp::Sound::Callback*`), calls **vtable slot 1** with `RDX = label` |
| installed object | `bm2d::SoundCallback` (RTTI `.?AVSoundCallback@bm2d@@`, base `.?AVCallback@Sound@Afp@agcs@@`), vtable `0x180380758` | heap **singleton** from getter `FUN_1801ad880` (static `DAT_180cf36e0`, 0x10 bytes, `+8` = side, init 2); installed into `state+0x58` by `Application::onBoot` `FUN_180002060`. Overrides only slot 0 (dtor) and **slot 1 = play `FUN_1801ad8e0`**; slots 2..6 (stop/stop_all/volume/pan/fade) are empty `ret` (`FUN_1801483e0`), slot 7 (check_calling) returns 0 — AFP `sound_stop` etc. are no-ops |
| side for pan | `BM2DGroupWithPan` (anon-ns, vtable `0x18035d158`) slot 5 `FUN_18002aa70` | writes `singleton+8 = group+0x30` then `FUN_180216020` → libafp **`afp_do_display`** (ord 18) for the group — i.e. sounds fire from inside the AFP *display* pass of that group, not from game logic |
| play | `FUN_1801ad8e0` | `slot = classify(label)`; `pan = 0; if (mgr+0x20C4 /*versus pan*/) pan = side==0 ? −1.0 : side==1 ? +1.0 : 0`; **tail-JMP `se_play(slot, label, pan)`** |
| classify | `FUN_1801ad7b0` | `strncmp(label,"vo_",3)==0 → 3`; else exact-match against the 25-entry table `0x180465350..0x180465418` → 3; else **2**. Returns the int in EAX (caller `MOV ECX,EAX`) |
| se_play façade | `FUN_1801aa180` (repo signature `se_play`) | mute filter `(*DAT_1806f2428)(5, label, slot−1)` unless slot ∈ {1,5}; AVS lock; `se_play_inner` |
| se_play_inner | `FUN_1801ab240` | bank = `mgr[(slot+1)*0x10]`; `GetCueIndex(label)`; `== 0xFFFF` ⇒ **return −1, no log**; else `Play` + register the handle in the 256-entry cue table (`FUN_1801ab050`) |

The 25-name table (identical on all five World builds and in A3):
`ACT2_1 ACT3_1 ACT4_2 ACT6 ACT9 sn2_dgm25..sn2_dgm34 sn2_dgm_high sn2_dgm_middle
sn2_etc73 sn2_etca2 sn2_etca3 sn2_etca4 sn2_etca5 sn2_etca7 sn2_gov sn2_mst09`
— the A3 `voice_n` cue names that don't start with `vo_`. World kept A3's
classifier verbatim, but World's slot-3 bank is `voice` (not `voice_n`), so
every one of them misses.

**No miss logging anywhere** (gamemdx drops `se_play`'s −1; libafp traces the
call only under its internal debug flag `0x100`).

### A3 (20240402) — same rule

| | A3 | notes |
|---|---|---|
| vtable / play / classify / se_play | `0x180279a98` / `FUN_1800ff640` / `FUN_1800ff5a0` (table `0x1802db4e0`, same 25 names) / `FUN_1800fc180` | byte-identical shape |
| side setter | `FUN_1800236a0` (same BM2DGroupWithPan pattern, singleton `DAT_1813b0248`) | |
| slot mapper | `FUN_1800fbda0`: prefix `strncmp` against `bgm_menu`, `se_system(_n)`, `se_normal(_n)`, `voice(_n)` → 0..3, else 5 | so A3's slots 2/3 held **`se_normal_n` / `voice_n`** — which is why the same classifier found every era cue in A3 |

Confirmed A3 relied on this path: `XAC_full_combo2`, `Plate_spin3_st`,
`STG_CLOSE01`, `se_shutter_in` appear in no A3 code string — they could only
ever play from the AFP clips.

### Per-build addresses (play / classify / se_play it tail-jumps to)

| Build | play (= RTTI vtable[1]) | classify | se_play | SoundCallback vtable |
|---|---|---|---|---|
| 20250805 | `0x180197fe0` | `0x180197eb0` (table `0x18042d590`) | `0x180194960` | `0x18035e0c8` |
| 20260224 | `0x18019a750` | `0x18019a620` | `0x180196ff0` | `0x180365758` |
| 20260721 | `0x1801ade50` | `0x1801add50` | `0x1801aa6e0` | `0x180380738` |
| 20260825 | `0x1801ad8e0` | `0x1801ad7b0` | `0x1801aa180` | `0x180380758` |
| 20260915 | `0x1801ad420` | `0x1801ad320` | `0x1801a9d40` | `0x180380778` |
| A3 20240402 | `0x1800ff640` | `0x1800ff5a0` | `0x1800fc180` | `0x180279a98` |

Pan constants read through the play body: side 0 → −1.0, side 1 → +1.0 on
every build.

---

## 2. Q3 — will a slot-4-only cue be found? **No.** Seam

The callback never searches: it computes one slot (2 or 3) and asks that one
bank. Today, with no mod bank, every legacy embedded cue is a silent miss
**except** `vo_ingame_ready` and `vo_stage_clear`, which resolve to World's own
(different) announcer takes in `voice.xsb`.

**Recommended seam — pre-original detour on `bm2d::SoundCallback::play`
(RTTI vtable[1]):**

```
play(this, label):
  if dsel_armed && dsel_has(label):          // lock-free: AtomicBool + immutable name set
      pan = stock_pan(*(u32*)(this+8))        // 0 / −1 / +1 exactly as stock; versus byte = game_audio::versus_pan()
      se_play(4, label, pan); return          // mute filter + AVS lock + cue table stay stock
  original(this, label)
```

- Prologue `48 89 5C 24 08 | 57 | 48 83 EC 20 | 8B 59 08 | 48 8B CA` — 19
  position-independent bytes before the first `CALL`, so a clean trampoline.
  Nothing in the repo hooks it today (one-detour rule satisfied).
- Runs inside `afp_do_display` (AFP display pass of a BM2DGroupWithPan node),
  **not** the game-logic thread — the body must be lock-free / allocation-free
  / log-free; calling `se_play` from there is exactly what stock does.
- `se_play(4, …)` with slot 4 empty ⇒ `se_play_inner` sees a null bank ⇒ −1
  (safe). Slot 4 through `se_play` is cabinet-proven audible in gameplay
  (assist-tick R-2); the mute filter is called with category 3 there.
- Gate on "armed" so stock behaviour is byte-identical when DDR SELECTION is
  off; route **ours-first** for any name in the dsel set (gives A3's own
  `vo_ingame_ready`/`vo_stage_clear` takes on legacy clips — see open Q2).

Alternatives (not recommended):
1. Detour `classify` to return 4 for our names — one line, pan stays in stock
   code, but its prologue carries a RIP-relative `LEA RDX,["vo_"]` at +6 that
   the trampoline must relocate.
2. Zero-detour: swap the singleton's vtable pointer for a mod clone
   (`rows.rs::build_mod_vtable` shape; object = `*DAT_180cf36e0` from the
   getter, or `*(afp_state)+0x58`). Needs the getter AOB and a clone of the
   pan logic; no gain over (main).
3. Putting the era bank into slot 2 or 3 — would evict World's own `se_normal`
   / `voice` for the window (World's gameplay SEs/voices go silent) and breaks
   `game_audio`'s "never write another slot" invariant. Rejected.

### AOBs (offline sweep: 1 match on every build listed)

`afp_sound_callback_play` — entry of `bm2d::SoundCallback::play` (55 bytes):

```
48 89 5C 24 08 57 48 83 EC 20 8B 59 08 48 8B CA 48 8B FA E8 ?? ?? ?? ??
48 8B 0D ?? ?? ?? ?? 0F 57 D2 80 B9 C4 20 00 00 00 74 ?? 85 DB 74 ?? FF CB
75 ?? F3 0F 10 15
```

`afp_sound_bank_classify` — entry of the name→slot classifier (61 bytes):

```
40 53 48 83 EC 20 48 8D 15 ?? ?? ?? ?? 41 B8 03 00 00 00 48 8B D9 E8 ?? ?? ?? ??
85 C0 41 0F 94 C2 45 84 D2 75 ?? 4C 8D 05 ?? ?? ?? ?? 4C 8D 1D ?? ?? ?? ??
49 8B 00 4C 8B CB 4C 2B C8
```

| Build | play match | classify match |
|---|---|---|
| 20250805 (= Ghidra `gamemdx_20250805_STOCK.dll`, decompile-confirmed) | 1 @ `0x180197fe0` | 1 @ `0x180197eb0` |
| 20260224 | 1 | 1 |
| 20260721 | 1 | 1 |
| 20260825 | 1 @ `0x1801ad8e0` | 1 @ `0x1801ad7b0` |
| 20260915 | 1 | 1 |
| A3 20240402 | 1 | 1 |

Cross-checks for the derivation (all hold on all six):
- the 0x59-byte full body (pattern above + `?? ?? ?? ?? EB ?? F3 0F 10 15 ?? ?? ?? ?? 48 8B D7 8B C8 48 8B 5C 24 30 48 83 C4 20 5F E9`) matches once — the whole function is shape-identical;
- `match+0x14` rel32 → the classifier match;
- `match+0x1B` disp32 → the audio-manager global (= the `se_play_inner_body` derivation);
- `match+0x55` rel32 (the tail `E9`) → the `se_play` signature's match;
- RTTI `.?AVSoundCallback@bm2d@@` vtable slot 1 == the play match;
- `match+0x37` / `+0x41` disp32 → floats +1.0 (side 1) / −1.0 (side 0);
- classifier `+0x09` → `"vo_"`, `+0x29`/`+0x30` → table begin/end (25 × 8).

Add both to the 4-build signature sweep + `shape_diff.py` before relying on
them (this sweep used a scratch harness, not `validate_signatures.sh`).

---

## 3. Cue inventory

Method: arcs unpacked with `scripts/unpack_arc.py` + `ifstools`; every AFP
parsed with bemaniutils' `SWF` (sibling checkout) and every DoAction's
bytecode scanned for `CALL_METHOD` on `sound*`; cross-checked by intersecting
every package's descrambled string table with the union of all XSB cue names
(no hits outside the `sound_play` literals). XSBs from `$DDR_WORLD_INSTALL/data/arc/soundbanks{,_n}.arc`,
waves from `voice{,_n}.xwb` / `se_normal{,_n}.arc`. "Route" = the stock
classifier's slot. All `_n` hits are in `voice_n.xsb` (cat 7, wavebank
`VOICE`) or `se_normal_n.xsb` (cat 6, wavebank `SE_NORMAL_n`). Complex cues
are all type-1 variation tables (flags `0x000B`) over bare simple sounds.
No name is in World's `se_system.xsb` / `bgm_menu.xsb`; World's `se_normal.xsb`
contains **none** of them (not even `se_shutter_in/out`).

### 3.1 AFP-embedded (`asdlib.sound_play`)

| Cue | Packages (clip) | Route | In World loaded bank? | `_n` bank | Shape | Waves | Bytes |
|---|---|---|---|---|---|---|---|
| `XAC_full_combo2` | dance_fullcombo0001..5 (`01_/02_fullcombo_*`) | 2 | no | se_normal_n | simple | 1 | 112 699 |
| `Plate_spin3_st` | dance_game_over0001..5 (`game_over`); common_choice_v2 | 2 | no | se_normal_n | simple | 1 | 48 159 |
| `Plate_spin4_st` | common_shutter0004/5 (`00_cleared 00_failed [00_prayforall]`); common_choice0004/5 (`choice_stage`) | 2 | no | se_normal_n | simple | 1 | 32 619 |
| `2nd_BIG2` | dance_message0001 (`00_ready`); common_shutter0001 (`00_cleared`) | 2 | no | se_normal_n | simple | 1 | 53 620 |
| `ACT2_1` | dance_message0001 (`00_ready`) | 3 (table) | no | voice_n | simple | 1 | 7 140 |
| `2nd_KANSEI_B` | dance_message0002 (`00_ready`); common_shutter0002 (`00_cleared`) | 2 | no | se_normal_n | simple | 1 | 43 260 |
| `sn2_mst09` | dance_message0002, 0003 (`00_ready`) | 3 (table) | no | voice_n | simple | 1 | 25 900 |
| `vo_ingame_ready` | dance_message0004, 0005 (`00_ready`); dance_message_v2 | 3 (`vo_`) | **yes** (voice, 29 var, World take) | voice_n | complex ×9 | 9 | 548 800 |
| `ACT9` | common_shutter0001 (`00_failed`) | 3 (table) | no | voice_n | complex ×2 | 2 | 61 950 |
| `STG_APP02` | common_shutter0001 (`00_enjoyddr 00_prayforall`), 0004/5 (`00_cleared [00_prayforall] choice_background`); common_choice_v2; common_shutter_v2 | 2 | no | se_normal_n | simple | 1 | 289 659 |
| `STG_APP03` | common_shutter0001 (`00_enjoyddr 00_prayforall`), 0003 (`00_cleared choice_background`); common_shutter_v2 | 2 | no | se_normal_n | simple | 1 | 273 559 |
| `STG_CLOSE01` | common_shutter0001/4/5 (`choice_background shutter_clear shutter_failed`), 0003 (`choice_background shutter_failed`) | 2 | no | se_normal_n | simple | 1 | 5 810 |
| `se_shutter_in` / `se_shutter_out` | common_shutter0001..5 (`shutter_entry`); common_choice_v2; common_shutter_v2 | 2 | no | se_normal_n | simple | 1 / 1 | 133 699 / 137 479 |
| `vo_stage_clear` | common_shutter0001 (`00_prayforall`); common_shutter_v2 | 3 (`vo_`) | **yes** (voice, 4 var) | voice_n | complex ×2 | 2 | 137 550 |
| `ext_failed` | common_shutter0002 (`00_failed`) | 2 | no | se_normal_n | simple | 1 | 79 799 |
| `sn2_gov` | common_shutter0002, 0003 (`00_failed`) | 3 (table) | no | voice_n | complex ×5 | 5 | 217 000 |
| `end_door` | common_shutter0003 (`shutter_clear`) | 2 | no | se_normal_n | simple | 1 | 28 700 |
| `banner_in` | common_shutter0004/5 (`choice_jacket`); common_choice_v2 | 2 | no | se_normal_n (also A3's) | simple | 1 | 17 290 |
| `ACE_shutter_choice_exc` | common_choice0001 (`choice_stage_exclusive`); common_choice_v2 | 2 | no | se_normal_n | simple | 1 | 203 419 |
| `ACE3_shutter_choice_savior`, `ACE_TEPPAN` | common_choice_v2 only (World ships it, World code never names `common_choice`) | 2 | no | se_normal_n | simple | 1 / 1 | 289 659 / 165 059 |

Packages with **no** embedded sound: dance_judge / fast_slow / danger /
gauge / combo / score / stage_frame 0001..0005, common_choice0002/0003,
common_choice_cutin0001..0005, common_choice_cutinbg_v0 (no `afp/`), and
World's own `dance_*_v3` + `common_shutter_v3` (World plays those sounds from
code). `dance_combo0005_v0` does not unpack (known zero-filled arc).

AFP set: 14 SE cues (14 waves, 1.46 MB) + 6 voice cues (20 waves, 1.00 MB).

### 3.2 Code-played (from `sounds-options-folder.md` §A) — all present

| Set (A3 site) | Cues (all `_n`, none in World's loaded banks unless noted) | Shape | Waves | Bytes |
|---|---|---|---|---|
| skins 1–3 CallVoice | `ACT6` (×7); `sn2_dgm25..34` (simple, 43–77 KB each); `sn2_dgm_high` (×7), `sn2_dgm_middle` (×6) | voice_n | 30 | 1 190 068 |
| crowd SE | `STG_APP02`, `STG_APP03`, `STG_BOO` (213 499), `2nd_BIG2`, `2nd_KANSEI_B` | se_normal_n simple | 5 | 873 597 |
| A3 announcer (skins 4–5 literal) | `vo_ingame_combo_100..1000` (×2 each; `_1000` also in World voice), `_combo_over` (×3), `_combo_gen` (×9), `_high` (×23), `_gen` (×19), `_regain` (×6), `_low_hard` (×11), `_low_easy` (×10), `_cheer` (×12), `_boo` (×8) | voice_n complex | 121 | 11 432 251 |
| HERE voice / cut-in / stage call | `ACT3_1` (17 640), `ACT4_2` (19 880); `sele_1st/ext/sn2/x2/2013` (stereo, 315–348 KB each); `sn2_etca2..5`, `sn2_etca7`, `sn2_etc73` (simple, 32–40 KB); `vo_stage_01..04/final/extra` (simple in `_n`; World's `voice` has its own multi-take versions) | mixed | 19 | 2 281 014 |

`sn2_etca6` and `vo_stage_encore/special` exist but no A3 rule plays them.

**Everything (AFP ∪ code): 205 unique waves ≈ 17.6 MB; without the A3
announcer ≈ 84 waves ≈ 6.1 MB.**

---

## 4. Corrections to earlier notes

- `sounds-options-folder.md` §A.2: "A3 has no READY voice at all" is true of
  the **binary** only — A3's READY sounds came from `dance_message000N`'s
  `00_ready` clip: skin 1 `2nd_BIG2`+`ACT2_1`, skin 2 `2nd_KANSEI_B`+`sn2_mst09`,
  skin 3 `sn2_mst09`, skins 4/5 `vo_ingame_ready` (A3's 9-take `voice_n`
  version). A re-hosted legacy READY clip brings its own sound; World's code
  `vo_ingame_ready` (`FUN_180033f60` @ `0x1800346f8` / `0x1800348ba`) must be
  suppressed on those songs or it doubles.
- `intro-and-skin-surface.md` §6 Q7: `banner_in` **is** in A3's (and World's)
  `se_normal_n.xsb` — not a dead cue.
- The results/cleared/failed cheers and shutter SEs of the legacy banners are
  AFP-driven, so the banner re-host gets them for free once routing exists.

## 5. Open questions

1. Register the dsel bank **before** the first legacy clip can fire:
   `shutter_entry`'s `se_shutter_in` plays at the very start of the
   song-select→stage shutter, so "first armed 25→26 edge" is too late —
   register at the first SONG_SELECT entry (or enable) instead.
2. Ours-first for `vo_ingame_ready` / `vo_stage_clear` (A3 takes, literal) or
   omit them from dsel so World's takes play (rule fidelity)?
3. Does World's code also play a shutter SE during the stage-choice / clear
   shutters it re-hosts (double SE with the clip's `se_shutter_in/out`)?
4. Mute filter `(*DAT_1806f2428)(5, name, 3)` for slot 4 outside gameplay
   (scenes 25→26, results 30) — proven non-vetoing only in gameplay; confirm
   on the first cabinet boot (one-shot INFO per routed cue).
5. `afp_do_display` thread identity was not pinned — design for any thread.
6. AFP `sound_stop`/`fade` are ignored by World (and A3); none of the legacy
   clips call them, so no action needed unless a looping era SE appears.

## 6. Addendum 2026-09-23 — code-played SEs that double a legacy clip

World's `FullcomboActor::onMessage` (`FUN_180069c00` on 20260825) plays
`se_game_fullcombo` itself (inlined slot-2 `GetCueIndex`/`Play` at +0xB7..+0x12D,
not through `se_play`), then the legacy clip's `XAC_full_combo2` routes to the
era bank — both sound (cabinet run #2). A3's binary contains no `se_game_*`
string, so its FullcomboActor played nothing from code. Seam: the null-bank
`JZ rel8` at +0xC5 (`TEST RDI,RDI; JZ release` after `MOV RDI,[audio_mgr+0x30]`)
flipped to `JMP` while the package is legacy — World's own "no se_normal bank"
path, AVS lock still released. AOB `ddr_sel_fullcombo_se_site` (107 bytes from
the `MOV RBP,[rip]` at +0xB7) is unique and shape-identical from +0x95 to
+0x13B on all five builds; the derivation gates the LEA's string, the JZ
opcode, its target (`85 C9 7E`) and the site's distance from the handler
entry. Other World code SEs: `vo_ingame_ready` ×2 (`FUN_180033f60`, Step 4),
`se_game_failed`/`se_game_clear` (shutter kind-table data, Step 6),
`se_game_miss` (battery gauge `FUN_180070de0`, no legacy embedded twin — Step 8
fidelity call).
