# DDR SELECTION — Arbitrary & User-Authored Gameplay Skins — Feasibility & Strategy (2026-09-25)

Status: **FEASIBILITY ONLY.** Nothing here is implemented or cabinet-tested.

**Question.** DDR SELECTION (`src/mods/ddr_selection/`) revives the five legacy gameplay skins
Konami rebuilt for A20–A3: 1st-5th, MAX-EXTREME, SuperNOVA, X, 2013-A. Can it support any number
of skins, including ones users build for DDR versions Konami never rebuilt? Examples: a single mix
(2ndMIX, 3rdMIX, EXTREME), SuperNOVA 2 on its own, X3, A20, a home version.

**Method.** The evidence comes from:

- the shipped mod and its RE notes (`.agents/planning/2026-09-22-ddr-selection/research/`,
  `docs/ddr_selection_research.md` §12);
- a new disassembly pass on World 20260825 over every game-side reader of the skin id (§2), with
  byte-pattern checks on all five supported builds (20250805, 20260224, 20260721, 20260825,
  20260915);
- an inventory of the stock World install's `data/arc/bm2d/` (`$DDR_WORLD_INSTALL`);
- a read of the repo's content tooling: LayeredFS, `core/arc`, `core/ap2`, S-Marvelous, and the
  Background Dancers custom-content feature.

Addresses are file-relative to `gamemdx.dll` @ `0x180000000`, build 20260825 unless noted. Tags:

- **[dis]** verified by disassembly in this pass.
- **[notes]** stated in the research notes.
- **[inf]** inference, not observed.

Builds on:

- `docs/ddr_selection_research.md` and the DDR SELECTION planning notes (mechanism, A3 spec,
  corrections);
- `docs/afp_system.md`, `docs/afp_texture_pipeline.md` (BM2D/AFP packages and textures);
- `.agents/planning/2026-09-22-background-dancers-custom-content/` (the precedent for content
  discovered at boot);
- `src/services/avs_layeredfs/` (file serving) and `src/mods/s_marvelous/` (the precedent for
  editing AFP templates).

---

## TL;DR

**Feasible.** The game engine is not the limit. The limit is that the mod uses one small integer for
three different jobs.

- **The three jobs.** Today the skin number N ∈ 1..=5 is:
  1. the package-name suffix (`dance_judge000N`) and the prefix of the texture names the DLL
     composes itself (`dance_combo000N_…`, `dance_score000N_…`, `stage_frame000N_stage_…`);
  2. the index into the DLL's per-skin behaviour tables (gauge fill, combo growth, announcer …);
  3. the value World's surviving A3 branches read.

  Split N into three things: an **identity** (a string key plus a catalog entry), **donor assets**,
  and an **engine class**. After that split, almost everything else becomes data.
- **World's code allows the split. It needs none of the identity** (§2):
  - **`GameWork+0xA8` must stay in 0..=5.** It indexes an unchecked `int[6]` on the stack.
    - While a skin is armed, the only observable effect of that field is its `== 1` meaning: no
      song info and no option icons.
    - So it becomes a boolean, the "classic gates".
  - **The package record's skin (`+0x28`) is the engine's behaviour switch.**
    - Legacy values fall into four classes: {1}, {2}, {3}, {4, 5}.
    - **Any value ≥ 6 is safe** [dis]: every reader is a compare; no reader indexes a table.
    - Values ≥ 6 behave as a fifth, "neutral" class: roughly World's / A3's skin-0 behaviour, on the
      record's own packages.
  - **Package identity is the name string.** The helper already registers arbitrary names
    (`policy::Entry::fixed_arc`), so user packages need no numeric suffix.
- **No new detours.** Every surface runs on a hook the mod already owns. The work is replacing
  hard-coded `1..=5` tables and fixed-size structures (§3) with a catalog.
- **The real gap is authoring content.** Nothing in the repo, or in public tooling, compiles *new*
  AP2 (AFP) animation.
  - User skins therefore start as **donor-based re-skins**, "Tier 0":
    - take any on-disk skin's packages;
    - replace their textures (at the donor's image sizes);
    - move elements;
    - choose behaviour parameters and sounds.
  - That already covers most of what separates one mix from its neighbours: fonts, colours,
    frames, gauge art, layout, announcer.
  - New animation timing belongs to later tiers (§5.3).
- **Two hard safety requirements:**
  - An **export preflight** for any user package that is not a byte copy of a donor. A World HUD
    actor dereferences NULL on a missing export, which crashes the cabinet (§4).
  - The option row must persist a **stable key**, not a list index (§6.5).
- **Bonus.** The install already holds more Konami skins than the five (§5.4):
  - A3's own UI in two colour schemes (`*0000_v1` gold cabinet, `*0000_v2` every other cabinet);
  - the DDR A generation (`*0000_v0`);
  - early-World prototype art (`*0000_v3`).

  These are zero-authoring test cases for a generalized pipeline. A3's own UI is also the missing
  "A20–A3" era: today AUTO leaves series ≥ 18 on World's UI. Their HUD and READY packages were
  checked and export exactly the names the adapters ask for, FLARE art included. The plan for them
  is `docs/ddr_selection_a3_themes_research.md`.

| Tier | The author supplies | The DLL does | New tooling needed | What can differ from the donor |
|---|---|---|---|---|
| **0 — Re-skin** | `skin.json`, PNGs named like the donor's textures, optional WAVs | Copies donor arcs under the skin's own names, stages the PNGs as `_ifs` overlays, applies layout overrides and behaviour parameters, adds the WAVs to the era bank | skin-kit extractor (script), WAV → XWB ingest | all art, colours, element positions, gauge / combo / score styles from the A3 set, announcer, sounds |
| **1 — Remix** | Tier 0 plus declarative AP2 edit ops (move / scale / clone / relabel a segment, add a sprite) | Runs `core/ap2::edit` primitives on the copied template at build time | recipe runner + op vocabulary | Tier 0 plus element geometry, extra sprites, extra placements |
| **2 — Bring your own** | complete `.arc` packages built with external tools | Checks them against the skin contract (§4), then registers them | preflight | anything the contract allows, including new animation |

| Phase | Content | Effort |
|---|---|---|
| P0 spike | Alias copy of a donor + `_ifs` texture overlay under a dev knob; record skin ≥ 6 on a cabinet; texture names across consecutive songs (§8) | 1–2 days |
| P1 catalog | The five A3 skins re-expressed as built-in catalog entries, behaviour-identical; host tests unchanged | ≈ 1 week |
| P2 user packs (Tier 0) | Discovery, manifest, alias build step, overlays, layout overrides, stable-key row, skin kit | 1.5–2 weeks |
| P3 user sounds | WAV ingest into the era bank, per-skin cue remap | ≈ 1 week |
| P4 Konami bonus skins | A3 GOLD / A3 WHITE / DDR A — planned separately in `docs/ddr_selection_a3_themes_research.md` (doable before P1, as appended skins) | ≈ 1–1.5 weeks for all three |
| P5 Tier 2 | Export / child / label preflight for user-built arcs | 3–5 days |
| Tier 1 | Recipe runner | open-ended; after P2 |

---

## 1. How the five skins work today

Pointers (read the module docs for the details):

1. **Trigger** (`trigger.rs`, pure). The per-player option row holds a value: 0 OFF, 1 AUTO, or
   2..=6 for the eras (skin = value − 1, `trigger.rs:145-150`).
   - AUTO buckets the song's raw musicdb `<series>` as A3's folders did (`auto_skin`,
     `trigger.rs:45-54`).
   - One skin applies per song for the whole cabinet. The entered side governs; P1 in versus; a
     bot side never governs.
2. **Arm** (`mod.rs::arm`, scene 25 → 26..=28). Writes the skin to `GameWork+0xA8`
   (`mod.rs:280-290`) and publishes `ARMED_SKIN: AtomicU8` (`mod.rs:137`).
3. **Package helper** (`package_helper.rs`). Fully replaces World's `LayoutActor` per-package
   helper. It **ignores the skin argument World passes in** and uses `armed_skin()`
   (`package_helper.rs:128`).
   - `policy::decide(base, skin, adapters)` → `Stock` or `Legacy{arc_base, skin, fixed_arc}`
     (`policy.rs:265-279`).
   - For a legacy package, the helper probes `<arc_base>000N` (or the fixed arc) through the game's
     own LayeredFS-aware probe.
   - It then inserts `records[side][base] = {name, skin N}` and pushes the name on the load list
     (`package_helper.rs:200-298`).
4. **Adapters** (gauge, combo, score, stage frame, song info, markers, option icons, intro, stage
   panel, banners, `_sel` movies, sounds). They re-host A3 behaviour inside World's actors. They
   are keyed on the record skin or on `armed_skin()`.
5. **Cross-mod seams.** `legacy_package(base)` and `armed_skin()` are read by S-Marvelous
   (per-skin S-Marvelous art) and by the planned PS1-style dancers (`armed_skin() == 1`).

| Job of N | Where | Consequence for new skins |
|---|---|---|
| Package / texture name | `policy::legacy_name` (`{:04}`), `marker_keys::root_name`, `intro.rs` (`dance_message`), `combo_math::sheet_prefix`, `score_math`, `stage_frame`, `panel_logic::packages` / `stage_texture`, `banner.rs` `PACKAGES`, S-Marvelous `targets::legacy_base` | A new skin needs files named `…000N`, or a name map |
| DLL behaviour tables | `gauge_math::fill_mode`, `combo_math::{growth, cell_width, single_sheet}`, `score_math::{difficulty_priority, difficulty_writes}`, `song_info_logic::mode_for_skin`, `panel_logic::{stage_voice, cutin_se, jacket}`, `intro_logic::here_voice`, `banner_logic::has_pray_for_all`, `sound/rules.rs::step`, `options_force_logic::FORCED_SKIN` | Every `match` has arms 1..=5 only |
| World's branches | `GameWork+0xA8`, record `+0x28` (§2) | Must stay in World's accepted range |

---

## 2. Engine constraints (World's own code)

### 2.1 `GameWork+0xA8` and the play sequence's identity table

`DancePlaySequence::onInitialize` (`FUN_1800573d0`) [dis]:

```
180057af4 mov dword [rbp-0x60], esi        ; t[0] = 0
…                                          ; t[1..4] = 1..4
180057b13 mov qword [rbp-0x4c], 5          ; QWORD: t[5] = 5 AND [rbp-0x48] = 0
180057b1b mov rax,[rip+…]                  ; GameWork global
180057b25 movsxd rax, dword [rcx+0xa8]
180057b2c mov r9d, dword [rbp+rax*4-0x60]  ; no bounds check
180057b6b call 0x18006b3f0                 ; LayoutActor ctor, arg 4 -> +0x190
```

- `MatchingDancePlaySequence` (`FUN_180061520` @ `0x180061a13`) has the same shape. The AOB
  `dps_skin_table_read` pins the load and the read as adjacent instructions on all five builds.
- **What an index ≥ 6 reads:**
  - 6: the zero high half of the qword store.
  - 7: uninitialised stack.
  - 8 and above: live locals, then the caller's frames.
- **The only reader of `+0x190`** is `LayoutActor::onInitialize`, which forwards it unchanged to
  all 18 helper calls. The mod's helper discards it while a skin is armed.

**Conclusion.** Keep `GameWork+0xA8` in 0..=5. The only other readers are the three live
`CMP [GameWork+0xA8],1` gates [dis, notes]:

- DPS / Matching `onUpdate` skip `SongInfoActor`;
- `GamePlayActor` init skips `OptionIconActor`.

The credit reset writes 0. Nothing else in World reads the field. This pass checked the 40
instructions after each of the GameWork global's 519 loads [dis]. That scan is a heuristic, but it
agrees with the notes' earlier scan for `+0xA8` stores (`docs/ddr_selection_research.md` §3.2).

So for a generalized skin the field is a boolean: **1 = classic gates, any other value in 2..=5 =
off.** Write 1 or 2, never the skin's identity.

### 2.2 Readers of the package record's skin (`record+0x28`)

- **Getter.** The record getter `FUN_18006ece0` has 19 call sites [dis]. Only the 8 below read
  `+0x28`. The other 11 look the package up by name only.
- **Getter miss.** When no record exists, the getter returns a static default `{"", 0}`. That is
  how unregistered shared packages fall through to the stage loader's slots.
- **Cross-build check.** The compare shapes are byte-identical on all five builds [dis].

| # | Actor / function | Branch | Value ≥ 6 |
|---|---|---|---|
| 1 | DanceDangerActor init `FUN_180068ce0` (`+0xB4`) | Package: 0 → loader slot, else the record's. Export: ≠ 0 → always `danger_single`. Position: 1–2 centred (640,360), 3–5 at the `danger_gauge` marker, else the `filter` marker. Layer: 3–5 → side+2 / prio 3, else 0 / prio 6. Second clip only for 0 | record package, `danger_single` (**also in doubles**), `filter` marker, layer 0 / prio 6, no second clip |
| 2 | Percent-gauge init `FUN_180073cf0` (`+0xD4`) | `== 3` → root label `1p_in` / `2p_in` intro | no intro |
| 3 | Percent-gauge update `FUN_1800743d0` | `== 1` → no easing | eased |
| 4 | LifeGauge init / update `FUN_1800706e0` / `FUN_180070de0` (`+0xB4`) | full-lives state = `skin != 2` | as skin 0 (rainbow) |
| 5 | StageFrameActor init `FUN_18007a190` (`+0x68`) | 0 → loader slot `+0x6f0`, else the record's package | record package |
| 6 | SongInfoActor init `FUN_180078fd0` (`+0xC0`) | 0 → loader slot `+0x730`, else the record's package | record package |
| 7 | Layout builder `FUN_18006bd40` (`dance_common`) | 0 → loader slot `+0x6b0` | the mod never registers `dance_common` |
| 8 | MatchingBattleFrame `FUN_180071ce0` (`dance_matching`) | 0 → loader slot `+0x7f0` | the mod never registers it |
| — | ScoreActor init `FUN_1800775d0` (`+0x60`) | stored only, never read | — |

Two findings are new in this pass; the notes had never traced them:

- the `> 5 → filter marker` fall-through;
- the fixed `danger_single` export for skin ≠ 0.

World has no other table indexed by skin. A3's skin tables (the ComboActor `{1,2,3}` list, the
shutter SE / voice rows, the `CallVoiceActor` era tables) are deleted or orphaned [notes].

### 2.3 Engine classes

The record skin value selects one of these behaviour bundles. The value that is written is the
**engine class**. It is not the skin's identity.

| Class (record skin) | Gauge easing | Percent-gauge intro | LIFE full lives | Danger placement | Danger layer | Song info / stage frame package |
|---|---|---|---|---|---|---|
| 0 | World (stock) | — | rainbow | `filter` marker + second clip | 0 / 6 | loader (World) |
| 1 | **none** | — | rainbow | **centred** | 0 / 6 | record |
| 2 | eased | — | **normal** | **centred** | 0 / 6 | record |
| 3 | eased | **`1p_in` / `2p_in`** | rainbow | `danger_gauge` marker | side+2 / 3 | record |
| 4, 5 | eased | — | rainbow | `danger_gauge` marker | side+2 / 3 | record |
| ≥ 6 "neutral" | eased | — | rainbow | `filter` marker, `danger_single` only | 0 / 6 | record |

What this means:

- A user skin picks one class for all the engine behaviours together.
- **Mixing behaviours across classes needs emulation.** Example: "centred danger plus the skin-3
  gauge intro".
  - Centred danger can be emulated in any class 3–5 by writing a `danger_gauge` marker at (640,360)
    in the marker post-pass. The machinery exists (§6.4).
  - The easing, full-lives and percent-intro behaviours cannot be reached without new detours.
- The neutral class is what A3's and early World's own UIs want (§5.4).
- Its fidelity gap against A3's skin 0: doubles gets `danger_single`, and there is no second danger
  clip. Classes 1–5 also always get `danger_single`.

### 2.4 Package names: rules the engine imposes

- **Identity is the pushed name** [notes]:
  - dedupe, the actor lookup (record → package vector) and the release at `LayoutActor` finalize
    are all keyed by name;
  - the resolver hashes `FNV-1("<resolved candidate>.ifs")`.

  So an arc's inner member must be named after the file it was resolved from. For example,
  `foo_v0.arc` must contain `data/bm2d/foo_v0.ifs`. A stale inner name has crashed
  `Manager::Update` (`core/arc.rs`; `bg_preview_overlay::ensure_alias_arc`, "v1 plain copies crash
  the manager").
- **Probe order.** The probe tries `_v3`, `_v0`, `_lite` (PC-type gated), then the bare name.
  - An explicit `_vN` name (`dance_song_info0000_v2`) is reached through the bare rung. The mod
    uses this for A3's own art.
  - **Never** fall back to `<base>0000`: it resolves to early-World or A3-oldest arcs with the wrong
    export names (`policy.rs:16-22`).
- **No collisions with stock ids.** The stock install ships no `dance_*` or `common_*` arcs numbered
  0006 or above. Counts from the listing: 47 `0000_v*` (17 `_v0`, 10 each `_v1` / `_v2` / `_v3`),
  13 `0001`, 15 `0002`, 14 `0003`, 14 `0004`, 14 `0005`.
- **Name length.**
  - The engine has no limit: package names are MSVC `std::string` views, inline up to 15 bytes and
    heap-pointed above that (`package_helper.rs:312-324`).
  - World's dead `"%04d"` write truncates into an 8-byte buffer and is never read [dis].
- **Mod folder paths.** LayeredFS strips everything up to `data/` from game paths
  (`mod_paths.rs:72-84`). A new arc therefore lives at `data_mods/<mod>/arc/bm2d/<name>.arc`,
  without `data/`, and is visible to the game's `avs_fs_lstat` probe. Proven additive cases:
  - bg-preview's generated alias arcs;
  - the A3 import's `missing` entries.

---

## 3. Mod-side constraints

### 3.1 Structural caps

| Where | Cap | Fix |
|---|---|---|
| `policy::SKIN_MAX = 5`, checked in `decide` / `adapter_for` (`policy.rs:26, 266, 284`) | 5 | catalog length |
| `Entry.skins: u8` bitmask with `1 << skin` (`policy.rs:91-110`) | 7 (a shift of ≥ 8 on a `u8` overflows) | per-entry package map in the catalog |
| `ARMED_SKIN: AtomicU8`, `Resolution.skin: u8` (`mod.rs:137`, `trigger.rs:85`) | 255 | `u16` index (or keep `u8` with a documented cap) |
| `ROW_MAX = 6`, `row_label`, `clamp_row`, era = value − 1 (`trigger.rs:14-39, 145-150`) | 5 eras | catalog-driven Dynamic labels (§6.5) |
| Dev knob `DDR_SELECTION_FORCE` 1..=5 (`mod.rs:615-641`) | 5 | accept a skin key |
| `combo.rs:110` `PACKAGE_STATE: [AtomicU8; 6]` | 5 | per-catalog-entry state |
| `stage_frame.rs:38-40, 193`: one 0x20-byte near-buffer slot per skin (1..=5), reach checked only up to slot 5, `PREFIX_LEN = 22` patched as an imm32 | 5 slots; prefix ≤ 31 chars | per-entry slot (the 0x1000 buffer holds ~127) or rebuild the prefix at apply time; the imm32 takes any length |
| `banner.rs:60` `static PACKAGES: [&CStr; 5]` (the ShutterActor loader keeps the row's *pointers*) | 5 | leaked per-entry `CString`s |
| `option_icons.rs:231` `2..=5`, texture `daopic0000_…` hard-coded | — | per-entry prefix |
| `gauge.rs:246`, `combo.rs:561`, `score.rs:325` record-skin `1..=5` checks | — | "the record is legacy" (`LEGACY_MASK`) plus the armed entry's parameters |
| `sound/call_voice.rs:74` `cues_ready()` — needs **every** cue of **every** skin, latched for the process | one missing cue gives World's announcer on all skins | per-entry readiness |
| `sound/bank.rs:56` `MAX_CUES = 128` (74 used). Going over fails **the whole bank** | 54 free cues | raise; per-skin budget |
| S-Marvelous: `LEGACY_SKINS: [u8; 5]` (`s_marvelous/targets.rs:19`), `skin_bit()` → 0 above 7 (`:152-158`), `u8` masks in `legacy.rs`, `afp_patches.rs`, `splash.rs`, `combo.rs`, `flash.rs` | 7 | S-Marvelous stands down on non-A3 skins until it takes catalog entries (§6.7) |

### 3.2 Surface inventory

A **profile** is one of the distinct behaviours the five A3 skins show. Every surface's per-skin
logic reduces to a profile choice plus names.

| Surface | Keyed on N today | Profiles present in A3 | Generalizes as | Difficulty |
|---|---|---|---|---|
| Policy / helper | skin masks, fixed arcs | — | per-entry `base → package name` map | low |
| Markers (layout) | `root_name` only | — | root name, plus **per-key coordinate overrides and extra hidden keys in the manifest** (new: users can reposition elements without touching AFP) | low |
| Stage frame | prefix `stage_frame{:04}_stage_` | — | prefix from the package's donor | low |
| Gauge (DLL) | `fill_mode` (`gauge_math.rs:43-60`) | segmented 63 × 6.984 px (1); continuous (2–4 and every FLARE state); segmented 26 × 17 px + partial (5) | `fill: continuous \| segmented{cells, cell_w, partial}`; LIFE intro label on/off | medium: cell geometry must match the art |
| Gauge (engine) | class | §2.3 | engine class only | — |
| Combo | `growth`, `cell_width`, `single_sheet`, `sheet_prefix` | 1st-style growth + half cells + single sheet (1); standard + single (2–3); standard + per-grade sheets (4–5) | `{growth: first\|standard, cells: half\|full, sheets: single\|per_grade}` + prefix | medium |
| Score / difficulty / EX | `difficulty_priority`, `difficulty_writes` | side-label scheme, prio 3 (2); level texture, prio 7 (others) | `difficulty: side_label\|level_texture` + prefix | low–medium |
| Song info | `mode_for_skin` (`song_info_logic.rs:42-48`) | none (1, via the gate); band (2); A3 panel (3–5) | `none\|band\|panel` + package. "none" without the gate = park the `song_info` marker off-screen (`marker_keys` `HIDDEN_COORD`) | medium |
| Option icons | `2..=5` | none (1); A3 row (2–5) | on/off + package + texture prefix | low |
| Option forcing | `FORCED_SKIN = 1` | classic set (1) | boolean | low |
| READY / HERE | `dance_message000N`, `here_voice` (skin 1) | voice (1) | package + `here_voice{normal, final}` + `ready_has_voice` (§4) | low |
| Stage panel + cut-in | `panel_logic::{packages, stage_texture, stage_voice, cutin_se, jacket}` | jacket hidden (1–2) / SN2 banner (3) / song (4–5); stage call none / `sn2_etc*` / `vo_stage_*` | package names, band prefix, cut-in SE, three stage-call names, jacket mode | low–medium |
| End banners | `PACKAGES[5]`, `has_pray_for_all` (skin 4, mcode 37789) | — | package + PRAY FOR ALL flag / mcodes | low |
| `_sel` movies | any armed skin | — | per-skin on/off. **Per-skin movies need BuildGraph path rewriting in `movie_policy`** (DirectShow opens `data/` directly; `data_mods` is never seen) | low (flag) / high (per skin) |
| Announcer / crowd | `rules::step` (`sound/rules.rs:138`) | none-plus-`ACT6` (1); `sn2_dgm*` (2–3); A3 `vo_ingame_*` (4–5); crowd SE per skin | profile + per-slot cue-name overrides | low |
| AFP-embedded sounds | none; the route is name-based | — | per-skin remap table (§6.6) | low–medium |
| `code_se` flips | package / state gates; cue `XAC_full_combo2` | — | full-combo cue and "READY carries its own voice" as manifest fields | low |

**What does not change.** The arm / disarm lifecycle, the stage-panel / intro / banner state
machines, the marker post-pass, the AFP sound route, the scoped-patch discipline and the one-detour
ownership are skin-agnostic already.

---

## 4. The skin contract

This is what a skin's packages must contain for each World consumer or DLL adapter:

- **"Crash"**: a World init creates this export and dereferences NULL when it is missing, so it
  must be preflighted.
- **"Graceful"**: the DLL creates the clip or looks the child up with `layer_find_child`, so a
  missing item only degrades that surface.

A Tier 0 re-skin inherits a donor's package byte-for-byte except for its textures. It satisfies the
contract by construction, as long as its package-to-base mapping is one the policy already proves.

| World base | Exports (crash if missing) | Labels / children the code uses (graceful unless noted) | Code-composed textures | Layout markers |
|---|---|---|---|---|
| `dance_judge` | `dance_judge`, `dance_judge_for_freeze` | `in_marvelous in_perfect in_great in_good in_miss in_ok in_ng` (+ `in_boo`) | — | `<lane>/judge_usr` |
| `dance_fast_slow` | `dance_fast_slow` | — | — | `<lane>/combo_set_usr/fast_slow_usr` |
| `dance_fullcombo` | the 16 World export names (`01_fullcombo_single_normal` … `which_fullcombo_perfect`) | `marbelous_in` (sic) / `perfect_in` / `great_in` / `good_in` | — | lane clip position |
| `dance_game_over` | `game_over` | `in` / `loop` / `out` | — | `{n}p_gameover_usr` |
| `dance_danger` | `danger_single`. A record skin ≠ 0 never asks for `danger_double` [dis]; the A3 packages ship both | — | — | `danger_gauge_{n}p_usr` (classes 3–5) |
| `dance_gauge` | `00_dance_gauge` | `gauge_frame_usr` (`loop_normal`, `loop_%dlife`), `gauge_usr` (`loop_normal`, `loop_rainbow`, `loop_danger`; FLARE / grade labels optional), **`fill _usr`, `fill _2_usr`** (with a space), `damage_1..8_usr`, root `1p_in` / `2p_in` (class 3) | — | `gauge_{n}p_usr` |
| `dance_combo` | `dance_combo` (≤ 0x12 bytes; the `overlay_element_styling` classifier matches it exactly) | root `in` / `loop`; `combo_usr`, `number_usr/{0001,0010,0100,1000}_usr` (graceful: no combo, one WARN) | `<prefix>[_<grade>]_{0..9,combo}` | `<lane>/combo_set_usr/combo_usr` |
| `dance_score` | `frame_score`, `frame_difficulty_{1,2}p[_reverse]`, **`difficulty_level_base`** | `0000001_usr` … `1000000_usr` (the 10⁴ place is authored `0010001_usr`), `comma1/2_usr`, `ex_tex`, `difficulty_level_usr/level_tex`, `difficulty_level_base_usr` | `<prefix>_score_num_{0..9}`, `_score_num_0_gray`, `_score_comma[_gray]`, `_lv%02d` | `score_{n}p_usr`, `difficuty_normal_{n}p[_reverse]_usr` (A3 spelling) |
| `dance_stage` | `stage_frame` | `stage_frame_usr`, `stage_number_usr` | `<prefix>{01..05,final,extra,…}` (missing names miss, as in A3) | `stage_frame_usr` |
| `dance_song_info` | `dance_song_info` | panel: `music_name_usr`, `artist_name_usr` | — | `song_info_usr` |
| `dance_option` | — (texture-only IFS) | — | `<prefix>_{1,2}p_<kind>_<value>` (every value `option_icons_logic::icon` can produce) | `option_icon_{n}p[_reverse]_usr` (width > 0) |
| `dance_message` | — (DLL-created: graceful) | `00_ready` (required, else World's intro), `00_here` (optional); labels `out`, `end` | — | — |
| layout root (`dance_common…`) | `dance_root` (graceful) | root markers above + `lane_{single,double}_{normal,reverse}` exports with `judge_usr`, `combo_set_usr/{combo,fast_slow}_usr`, `filter_usr`, `score_compare_usr`, `arrow_usr`, `freeze_judge_usr` | — | — |
| stage panel | shared root `common_choice_v2` / `shutter_choice_hd_root` (**fixed**; identity child `choice_stage_usr2`) | per skin: `choice_stage` (optional `voice` label), `choice_background`, `choice_jacket` (`choice_jacket_usr/jacket_root_usr/jacket_usr`), `choice_cutin` (`in` / `out` / `close`); shared `common_choice_cutinbg` | `<prefix>_{1st,2nd,final,extra}` | — |
| end banners | roots `shutter_clear` / `shutter_failed` | overlays `00_cleared` / `00_failed` / `00_prayforall` (`in` / `out` / `end`); **all banner sound must be embedded** (World's code SEs are blanked) | — | — |

**Sound contract.**

- Cues the clips name through `sound_play("<cue>")` play from the mod's `dsel` bank when present,
  and fall through to World's slot-2/3 banks otherwise. `se_system` and `bgm_menu` are
  unreachable.
- Code-played cues (cut-in SE, stage call, HERE voice, announcer, crowd) are looked up by name in
  `dsel`.
- **`vo_ingame_ready` is silenced whenever legacy READY clips exist** (`code_se`). A user `00_ready`
  without an embedded voice therefore makes READY silent unless `ready_has_voice: false` restores
  World's voice.

**Missing textures and labels.** A3 itself relied on libafp tolerating unknown frame labels and
unknown texture names. Examples: FLARE labels in legacy gauges, stage `_03` / `_howto` art. The
visible result has never been observed on a cabinet (`research/hud-actors.md` open Q1). For user
content this should be settled once; it decides whether the preflight must also check labels and
textures, or only exports.

---

## 5. Content: what can be authored

### 5.1 File serving (LayeredFS)

- **Additive arcs.** `data_mods/<mod>/arc/bm2d/<name>.arc` is visible to the game's probe.
  - Mod folders are scanned once at boot. Files written later are invisible until
    `mod_paths::init_mod_paths()` rescans (the bg-preview / S-Marvelous pattern).
  - Folder priority is the folder name sorted case-insensitively; `_cache` is skipped; the
    operator's allowlist / blocklist apply.
- **Texture overlays.**
  - A PNG at `<ifs>_ifs/tex/<image>.png` replaces the IFS's texture of that name. It is mapped by
    `md5(image name)`, which works because `dance_*` packages store one texture per image.
  - The PNG must match the texturelist image rect. A smaller PNG is padded at the **top-left**,
    which offsets the art under the shape's UVs. A larger one is skipped with a WARN
    (`ifs_textures.rs:601-618`). In practice: exact size.
  - Converted textures are cached and **not** mtime-checked on the hot path. An edited PNG needs
    `ifs_textures::purge_texture_replacement` or a cleared cache (the S-Marvelous restage
    precedent).
  - Additions go through `tex/texturelist.merged.xml` / `afp/afplist.merged.xml`, merged across all
    mods. A full `texturelist.xml` replacement bypasses the MD5 mapping [inf].
- **Alias arcs.** `core::arc::rewrite_paths` renames members without recompressing
  (`core/arc.rs:243`). `bg_preview_overlay::ensure_alias_arc` (`:741`) is the working precedent:
  copy a stock arc under a new stem, rename the inner IFS to match, hash-guard, write into a
  generated mod folder, rescan.
- **What is missing.**
  - An IFS writer: `core/ifs.rs` is read-only, and `scripts/build_ddr_package` shells out to the
    external `ifstools`. So a loose folder cannot become a *new* IFS inside the DLL.
  - That does not block Tier 0: the alias arc supplies the IFS, and loose PNGs override its
    members.

### 5.2 AFP tooling

- **`core/ap2`** parses and serializes AP2 with byte-identical round-trips. Its edit primitives
  (`edit.rs`) cover:
  - adding labels, shapes and named placements;
  - cloning labelled segments, sprite definitions and word segments with new shapes;
  - translating and rescaling placements.
- **`services/afp_patcher`** patches a template by export name at `afp_stream_do_create`. It holds
  **one patch function per name**: `register_patch` replaces any earlier one, so two consumers of
  one export must cooperate.
- **Missing for new animation:**
  - any timeline authoring format or SWF → AP2 path;
  - full PlaceObject encoding (colour / blend / per-frame matrices);
  - a DoAction assembler;
  - GE2D shape synthesis;
  - a retiming primitive.

  bemaniutils has a reader and a renderer but no writer. Its renderer is a usable offline preview
  oracle.
- **No 2D authoring tool in `tools/`.** The Blender add-on is 3D only.

### 5.3 The tiers in practice

**Tier 0 (re-skin)** is where almost all user value is, and it is fully buildable on existing
machinery. What one mix can express against another as a donor:

| Distinguishing feature | Tier 0? |
|---|---|
| Judgement / combo / score / stage-frame fonts and colours | yes (textures) |
| Gauge frame and fill art, segmented vs continuous fill, cell count | yes (textures + `gauge.fill`; the cell geometry must match the donor's fill clip width, ~440 px) |
| Element positions (score, gauge, judge, combo, difficulty, stage frame, danger, song info, option icons) | yes (layout root choice + manifest coordinate overrides) |
| Hidden elements (BPM, name, song info, option icons) | yes (hide keys / gate) |
| Stage panel art, cut-in art, CLEARED / FAILED art | yes (textures) |
| Announcer, crowd, cut-in SE, stage call, READY voice, embedded clip sounds | yes, with P3 (WAV ingest) |
| Word bounce / zoom / fade timing, the combo pop, the full-combo splash choreography | **no**: the donor's animation (Tier 1 for geometry, Tier 2 for timing) |
| An element no donor has (e.g. a separate 2ndMIX "combo word" sprite) | Tier 1 (`add_place_object_named` + texture addition) |

**Tier 1 (remix)** exposes a vetted subset of `core/ap2::edit` as JSON ops, applied to the alias's
AFP at the build step. The ops are already used in production by S-Marvelous and folder expansion.
The risks:

- op semantics are low-level (character ids, segment frames);
- every op must be validated against the contract (§4) after it runs.

A shape-level "move / scale / swap texture of placement X in segment Y" vocabulary is realistic.
General animation editing is not.

**Tier 2 (bring your own).** Authors build arcs with external tooling. The DLL accepts them only
after the preflight:

- IFS magic;
- inner member name = resolved stem, repaired with `rewrite_paths` if not;
- `afplist.xml` exports for every base the skin claims;
- optionally, AP2-parsed child and label names.

This is the tier a future authoring tool (§8) would feed.

### 5.4 Konami skins already on disk

**Superseded in detail by `docs/ddr_selection_a3_themes_research.md`**, which pins each generation
and plans the A3 / DDR A themes. Summary:

| Generation | What it is | Judgement art |
|---|---|---|
| `*0000_v1` (+ `dance_message_v1`, `common_choice_v1`, `common_shutter_v1`) | **A3, gold cabinet**. A3's probe starts at `_v1` on cabinet class 6/7 (machine type 4). Gold frames, FLARE art | condensed caps `Marvelous!!!` |
| `*0000_v2` (+ `_v2` message / choice / shutter) | **A3, every other cabinet**. Same design and image rects as `_v1`, silver frames. The skins 3–5 song-info panel and the skin-1 layout root already use it | same as `_v1` |
| `*0000_v0` (+ `_v0` message / choice / shutter) | **The DDR A generation** (IFS dates 2016-03 … 2018-09, no FLARE art). The only copy of danger, effect, filter, game over, measure, pacemaker and option icons, so A3 used those on every cabinet | heavy rounded `Marvelous!!!`, plus a Boo word |
| `*0000_v3` | **early-World prototype art** with A3-structured exports (`research/hud-actors.md` C3) | slanted World-style words, spelled **"Mervelous!!!"** |

(An earlier revision of this section called `_v1` "A3's SD variant" and left `_v0`'s era open.
Both are corrected above; the evidence is in the themes document §1–§2.)

- **A20 and A20 PLUS have no gameplay packages of their own** in World's or A3's install. Only
  their menu-background movies survive.
- **A3's own UI and the DDR A generation meet the §4 contract.** Exports, labels, children and
  code-composed texture names were all checked. A3's gauge carries the full FLARE art and label
  set.
- **Engine class: neutral** (≥ 6). This reproduces A3's skin-0 branches, except danger doubles and
  its second clip. A 2-site scoped patch closes that gap (themes doc §5).
- **The stage panel is A3's own fill** of the `common_choice_vN` root (no cut-in), and the banners
  are `common_shutter_vN`. The panel fill is the one new component.
- **AUTO could route series 18–20** (A20, A20 PLUS, A3) to the A3 theme. That changes today's AUTO
  results, so it must be opt-in (a design decision).
- **Early World (`0000_v3`)** is a novelty skin. It is useful as a second neutral-class test case.

---

## 6. Proposed architecture

### 6.1 Identity: the skin catalog

```text
SkinEntry {
    key:          "a3_supernova" | "user_x3vs2nd" | …   // [a-z0-9_]{1,24}, unique
    label:        "SuperNOVA"                            // ≤ 15 bytes ASCII (row SSO budget)
    persist_id:   i32                                    // §6.5
    engine_class: u8                                     // record skin: 1..=5 or 6 (neutral)
    classic_gates: bool                                  // GameWork+0xA8 = 1 (else 2)
    packages:     map<World base, PackageSource>         // Donor(id) | Alias(name) | Arc(name)
    tex_prefix:   map<World base, String>                // from each package's donor
    layout:       { root, moves, hidden }
    params:       { gauge, combo, score, song_info, option_icons, force_options,
                    intro, panel, banner, announcer, sel_movies }
    sounds:       { cue remaps, bank cues }
    auto:         { series[], mcodes[] }
    smarv:        Option<art dir>
}
```

- **Order.** Built-ins come first, in A3 order: `a3_1st_5th`, `a3_max_extreme`, `a3_supernova`,
  `a3_x`, `a3_2013_a`. Then Konami bonus skins, then user skins sorted by label (the Background
  Dancers catalog convention).
- **Arming.** Arming stores a catalog index. `armed_skin()` stays for compatibility (§6.7).
- **Adapters.** Every adapter reads its parameters from `catalog[armed].params`. Parameters are no
  longer derived from N.
- **The P1 invariant.** The five A3 entries, expressed as data, must reproduce today's tables
  exactly. The existing host tests (`scripts/validate_ddr_selection.sh`) become tests of those
  entries. That is a zero-behaviour-change refactor, and it proves the schema before any user
  content exists.

### 6.2 Skin pack layout and manifest

Discovery follows the Background Dancers custom-content model:

- one fixed base folder;
- friendly-name subfolders;
- validation skips a bad pack with a WARN and never blocks boot;
- the scan runs once at enable.

```text
data_mods/ddr_skins/
  X3 VS 2ndMIX/            ← friendly name (label fallback)
    skin.json
    judge/  dance_judge0004_marvelous.png …      ← donor texture names, exact image rects
    combo/  dance_combo0004_perfect_0.png …
    panel/  scene_choice_stage0004_1st.png …
    snd/    cutin.wav  crowd.wav
    arc/    (Tier 2 only) my_gauge_v0.arc
```

`skin.json` (JSON, since `serde_json` is already a dependency):

```json
{
  "format": 1,
  "key": "x3vs2nd",
  "label": "X3 VS 2ND",
  "donor": "a3:4",
  "engine_class": 4,
  "classic_gates": false,
  "textures": {
    "dance_judge": "judge",
    "dance_combo": "combo",
    "common_choice": "panel"
  },
  "packages": {
    "dance_gauge": { "donor": "a3:5" }
  },
  "layout": {
    "root": "a3:4",
    "move": { "1p:score": [200, 660], "stage": [640, 30] },
    "hide": ["bpm", "name"]
  },
  "gauge": { "fill": "segmented", "cells": 26, "partial": true },
  "combo": { "growth": "standard", "cells": "full", "sheets": "per_grade" },
  "score": { "difficulty": "level_texture" },
  "song_info": "panel",
  "option_icons": true,
  "force_classic_options": false,
  "panel": { "jacket": "song", "cutin_se": "sele_x2", "stage_voice": "vo" },
  "announcer": "a3",
  "sounds": { "sele_x2": "snd/cutin.wav", "STG_APP02": "snd/crowd.wav" },
  "auto": { "series": [13] }
}
```

**Resolution rules.**

- Every omitted field inherits from `donor`. A donor is:
  - `a3:1..5` for the DDR SELECTION eras;
  - `a3:0` for A3's UI, `*0000_v2`;
  - `world:proto` for `*0000_v3`;
  - later, possibly another user key.
- Mixing donors per package is allowed: the gauge above comes from 2013-A. Each package's
  code-composed texture prefix follows **its own** donor.
- Behaviour parameters take only the values the adapters implement (§3.2). The manifest selects
  and combines A3's behaviours; it never scripts new ones.
- Keys: `[a-z0-9_]`, globally unique. On a collision, the pack that sorts later is refused.
- A label over 15 bytes is cut with a WARN (the Background Dancers `MAX_LABEL_BYTES` rule).

**The skin kit.**

- Authors must know the donor's texture names and exact image rects. A script
  (`scripts/ddr_skin_kit.py extract --donor a3:4 --out <dir>`) would unpack the donor's arcs from
  the operator's own install:
  - all PNGs at their names and sizes;
  - a filled-in `skin.json`;
  - a marker coordinate dump for the layout.
- Konami art never enters the repo; the kit is generated locally (the `import_a3_assets`
  convention).
- A `--render` leg through the bemaniutils renderer could preview a pack offline.

### 6.3 Build step at enable

Runs once per boot, hash-guarded (the `CacheHasher` / `ensure_alias_arc` pattern):

1. **Scan** `data_mods/ddr_skins/*/skin.json`. Parse, resolve inheritance, validate. A bad pack is
   skipped with one WARN.
2. **Aliases.** For every package a skin re-textures:
   - copy the donor arc (from the install, LayeredFS-first) to
     `data_mods/ddr_selection_skins/arc/bm2d/<arc_base>_sk_<key>_v0.arc`;
   - rename its inner member to `data/bm2d/<arc_base>_sk_<key>_v0.ifs` (§2.4).

   The folder is machine-owned and never committed. It is **not** `_cache`, which the scan skips.
   Packages the skin uses unchanged reference the donor's own name (no copy).
3. **Overlays.** Copy the pack's PNGs into
   `data_mods/ddr_selection_skins/<arc_base>_sk_<key>_v0_ifs/tex/`. Call `purge_texture_replacement`
   for each changed file.
   - Check each PNG against the donor's texturelist rect. A mismatch gets one WARN naming the file
     and the expected size (LayeredFS would otherwise skip it silently to the log).
4. **Tier 2 preflight** for `arc/*.arc` (§5.3), then copy the arc into the generated folder.
5. **Rescan** with `mod_paths::init_mod_paths()`. Build the catalog, then register the option row
   (§6.5).
6. **Sounds.** Hand the sound work to the era-bank background thread (§6.6).

Cost: a few MB of arc copies per skin on the first boot, nothing afterwards. The PNG conversion
happens lazily at the first load, as for every LayeredFS texture.

### 6.4 Runtime changes by component

| Component | Change |
|---|---|
| `trigger.rs` | Row value → catalog index; AUTO → `auto` tables (§6.5); dev knob takes a key |
| `mod.rs::arm` | Store the index; write `GameWork+0xA8 = if classic_gates {1} else {2}`; the record skin comes from `engine_class` |
| `policy.rs` / `package_helper.rs` | `decide(base, entry, adapters)` → the entry's package name (generalized `fixed_arc`) + record skin = `engine_class`. Adapter availability stays code. "Danger needs markers" derives from `engine_class ∈ 3..=5`, not from the donor |
| Markers | Root from the entry; after reading the root, apply `layout.move` (coordinates via the existing setter) and `layout.hide` (`HIDDEN_COORD`). Emulate centred danger in classes 3–5 by writing `danger_gauge` = (640,360) |
| Gauge / combo / score / song info / option icons | "Legacy side" = the record is legacy (`LEGACY_MASK`), not `1..=5`; parameters and texture prefixes from the entry; per-entry state instead of `[_; 6]` arrays |
| Stage frame | Build the prefix from the entry at `apply` (per-entry near slot or on demand); `PREFIX_LEN` per entry |
| Panel / banners / intro | Names from the entry (leaked `CString`s where World keeps the pointer); **probe every era package before hosting** (the panel requests them unprobed today, and World's named loader `int3`s on a miss); PRAY FOR ALL flag / mcodes from the entry |
| Announcer | `rules::step(fields, profile, overrides)`; `cues_ready` per entry |
| `code_se` | Full-combo cue from the entry; READY gate honours `ready_has_voice` |
| S-Marvelous | §6.7 |

No new detours, signatures or derivations are required. The signature sweep is unaffected.

### 6.5 Option row, persistence, AUTO

- **Row.**
  - `RegisterSpec::scalar(0, 1 + N)` with `ScalarFormat::Dynamic` labels from the catalog.
  - `custom_options::set_scalar_bounds` (`custom_options/mod.rs:441`) exists for a count that
    changes at runtime.
  - Background Dancers is the working precedent for a scalar over a catalog discovered at boot.
  - The coarse step (START held) keeps a long list usable.
  - `versus_mirror` works unchanged: both sides share the catalog.
- **Stable key.**
  - Today the JSON cache stores the raw index (`PersistMode::Local`, identity save, `clamp_row`
    load). Adding or removing a pack would silently re-point every saved choice after it. This is
    also a live gap in Background Dancers.
  - Use `save_transform` / `load_transform` (`custom_options/api.rs:669`; the WebUI index → asset-id
    precedent, `webui_options/mod.rs:129-142`):
    - index → `persist_id` when saving;
    - `persist_id` → current index when loading;
    - an unknown id → OFF.
  - **Backward compatibility:** reserve `persist_id` 0..=6 for OFF / AUTO / the five A3 eras, so
    existing caches (values 0..=6) load unchanged. If the A3 / DDR A themes ship first as row
    values 7..=9 (`docs/ddr_selection_a3_themes_research.md` §6.1), reserve 0..=9. User skins get
    `persist_id = 0x100 + (fnv1a32(key) & 0x3FFF_FFFF)`, collision-checked at discovery.
- **Registration timing.** Register before the ~12 s JSON prime
  (`custom_options_persistence::JSON_LOAD_DELAY_SECS`). An option not registered by then loses its
  saved value.
- **AUTO.**
  - Build one `series → entry` table at discovery:
    - A3's buckets for 1–17 (today's behaviour);
    - then Konami bonus claims (opt-in: 18–20 → A3's UI);
    - then user `auto.series` claims;
    - then `auto.mcodes` overrides per song (A3's own membership was a curated mcode list).
  - Conflicts: the first claimant in catalog order wins, with one WARN.
  - Whether user claims may override A3's defaults is a maintainer decision. One option is a second
    row value, "AUTO+", so plain AUTO stays A3-exact.

### 6.6 Sounds

**Constraints** (`sound/bank.rs`, `sound/bank_build.rs`, `services/game_audio.rs`):

- one free manager slot (4);
- the bank is built once per boot on a background thread, then registered and immortal (leaked
  buffers);
- 128 cues;
- copy-only merging of XACT2 pairs inside a narrow envelope: XSB v43, XWB v42, PCM / MS-ADPCM,
  simple or type-1 variation cues.

**What is needed:**

1. **WAV ingest.**
   - A RIFF reader (none exists in the crate).
   - A synthetic bank `Source`: the 12-byte bare-sound template of `se_bank_synth/xsb.rs`, with PCM
     verbatim or MS-ADPCM through `core::xact::adpcm::encode_interleaved`, plus
     `core::xact::resample` for rate conversion.
   - Cues are named `sk<n>_<name>`, where `n` is the entry's catalog index; every generated name is
     ASCII, which the engine's cue hash needs.
   - Merged into the same `dsel` build.
2. **Per-skin remap in the AFP route.** A precomputed, immutable `(entry, label) → dsel cue` table.
   While entry E is armed, a donor clip's `sound_play("2nd_BIG2")` plays E's replacement. The table
   is lock-free and allocation-free, as the route requires (it runs inside libafp's display pass).
   Code-played cues read the entry's own names.
3. **Budgets.**
   - Raise `MAX_CUES`. The `PLAYED` counters and the bank validation are sized by it, and the XSB
     writer's own limits are 0xFFFF.
   - Cap each skin (for example 32 cues / 8 MB) with a WARN.
   - **All discovered skins' sounds are resident for the whole process** (≈ 17.6 MB today for the A3
     set; ADPCM ≈ 5.3 MB per stereo minute). A second bank would need a manager slot that does not
     exist.
4. **Operator XACT pairs.** XSB/XWB pairs inside the envelope need no new code: they are one more
   `Source`.

### 6.7 Cross-mod seams

- **`armed_skin() -> u8`.** Keep the current meaning: the A3 skin 1..=5 when a built-in A3 entry is
  armed, else 0. Every existing consumer then fails safe on new skins: S-Marvelous stands down; the
  PS1-dancers pairing only fires on 1st-5th.
- **New `armed() -> Option<ArmedSkin{ key, index, engine_class, era_tags }>`.** `era_tags` (e.g.
  `"system573"`) lets the PS1-style dancers pair with any skin that declares the tag.
- **`legacy_package(base)`** is unchanged. It is keyed by World base, not by skin.
- **S-Marvelous on new skins.**
  - Its legacy targets resolve templates by package name and pick the target from the song, because
    legacy templates can be byte-identical to each other (`s_marvelous/afp_patches.rs`).
  - Supporting catalog entries means: replace `LEGACY_SKINS` and the `u8` masks with the catalog;
    point `targets` at the entry's judge / fullcombo / combo package names; take the art from the
    manifest's `smarv` directory.
  - Alias templates are byte-identical to their donor's, so the donor's recipes apply unchanged.
  - Until then, S-Marvelous stands down (the current fail-safe).
- **`afp_patcher`** holds one patch function per export name. A Tier 1 recipe on `dance_judge` would
  collide with S-Marvelous's. Tier 1 recipes run at the **build step** on the alias's bytes (static
  data), so they never register a patch function.

### 6.8 Failure behaviour

| Failure | Behaviour |
|---|---|
| Manifest invalid / key collision / unknown donor | Pack skipped, one WARN; the rest of the catalog loads |
| Donor arc missing (e.g. `dance_combo0005` without the A3 import) | That package inherits nothing: stock for that base, one INFO (today's rule) |
| PNG size mismatch | That texture stays the donor's, one WARN with the expected size |
| Tier 2 preflight failure | That package stock, one WARN naming the missing export — **never registered** |
| Sound over budget / unreadable WAV | That cue stays the donor's, one WARN |
| Persisted id unknown | OFF |
| Anything at arm time | Today's per-surface fail-open rules; nothing new |

### 6.9 Score integrity and data policy

- **Skins are presentation only.**
  - The manifest exposes no timing, judgement or gauge-rule knob.
  - `force_classic_options` is A3's fixed restrictive set, restored after the song and scrubbed
    from saves.
  - So "no surface taints through `score_guard`" (`ddr_selection/mod.rs`) stays true.
  - Arbitrary option-forcing values must **not** be offered: they would turn a skin pack into a
    gameplay modifier.
- **Konami data never enters the repo.**
  - Tier 0 packs are derived from the operator's own install. The skin kit runs locally; generated
    arcs live in a machine-owned folder.
  - Packs that contain donor art are personal-install content, the same position as
    `data_mods/custom_models/`.
  - A pack of wholly original art (PNGs only) plus a manifest carries no Konami bytes. The DLL
    builds the alias from the recipient's install.

---

## 7. Phasing

1. **P0 — spike (1–2 days, cabinet).** Behind a developer knob:
   - alias-copy `dance_judge0001_v0` to `dance_judge_sk_test_v0`;
   - drop re-coloured PNGs into the generated `_ifs/tex/` overlay;
   - register the alias under record skin 1.

   Exit criteria:
   - the re-coloured words render;
   - no `movieclip is invalid`, no WARN;
   - consecutive songs stock → alias → A3 skin 1 → alias show the right pixels each time (§8,
     texture names);
   - one song under record skin 6 with a legacy danger / gauge shows the §2.3 neutral behaviour.
2. **P1 — catalog refactor (≈ 1 week).**
   - `SkinEntry` with the five A3 entries as data;
   - adapters, policy, trigger and sounds read the entry;
   - §3.1 caps removed;
   - `armed()` added;
   - row persistence moved to `persist_id` (0..=6 unchanged).

   Host tests green with the same expectations. A cabinet regression pass over the existing matrix.
3. **P2 — Tier 0 user packs (1.5–2 weeks).** Discovery, manifest and inheritance, the build step
   (§6.3), layout overrides, AUTO tables, the skin kit script, README section. Host tests for
   manifest resolution, the persist-id mapping and AUTO precedence (pure modules, harness-mountable).
4. **P3 — user sounds (≈ 1 week).** RIFF reader, synthetic `Source`, the remap table in
   `afp_route`, per-entry announcer overrides, budgets. The offline round-trip leg in
   `validate_ddr_selection.sh`.
5. **P4 — Konami bonus skins.** A3 GOLD, A3 WHITE and DDR A, planned in
   `docs/ddr_selection_a3_themes_research.md`. They can ship before P1 as appended skins 6–8, and
   become catalog entries at P1. Their stage panel and banners were resolved there: A3's own
   `common_choice_vN` fill and `common_shutter_vN`. Early World follows the same pattern.
6. **P5 — Tier 2 preflight (3–5 days).** Exports (crash class) mandatory; children and labels once
   §4's missing-label question is settled.
7. **Tier 1 — recipes (open-ended).** Start with `move` / `scale` / `swap_texture` on named
   placements, validated after each op.

---

## 8. Risks and open items

| # | Item | Why it matters | How to close |
|---|---|---|---|
| 1 | **BM2D texture names across packages** (`research/hud-actors.md` open Q3): are texture names resolved per package or globally? Legacy packages embed other skins' names (`dance_score0000_*` …) | Tier 0 aliases keep the donor's texture names. If resolution is global and a stale registration survives a song boundary, an alias could show its donor's (or another alias's) pixels | P0 consecutive-song test; if global, restrict to one alias per donor per window (already true: one skin per song) and verify release |
| 2 | `_ifs/tex` overlay on an IFS inside a *generated* arc in another mod folder [inf] | The whole Tier 0 mechanism | P0 |
| 3 | libafp with unknown labels / textures (open Q1) | Decides the Tier 2 preflight depth and what a partial user package looks like | One cabinet observation |
| 4 | Neutral class (≥ 6) on a cabinet | Only traced statically; doubles shows `danger_single` | P0 |
| 5 | A3's skin-0 stage-panel fill: control flow known from string references only | Completes the A3 / DDR A themes | The RE step in `docs/ddr_selection_a3_themes_research.md` §6.2 |
| 6 | Stage-panel packages other than `common_choice`: one texture per image? | The md5 overlay mapping needs one texture file per image | Inspect `common_choice000N`, `common_shutter000N`, `common_choice_cutin000N` IFS layouts |
| 7 | Resident sound memory grows with the number of skins | Immortal single bank | Per-skin budget; document |
| 8 | Row label texture appears only after the next launch when the mod is enabled live | Existing framework-wide limitation | Leave (maintainer decision on record) |
| 9 | `_sel` movies are per song, not per skin | A user skin cannot bring its own background movies | Out of scope; would need path rewriting in `movie_policy`'s BuildGraph detour |
| 10 | The stage-panel root `common_choice_v2` is shared and fixed | Users re-skin the panel's sub-clips, not its root | Contract (§4) |
| 11 | Pack authors' texture PNGs must match image rects exactly | Top-left padding offsets smaller art | Skin kit exports exact-size templates; build-step WARN |

---

## 9. Key references

| What | Where |
|---|---|
| DPS identity table read (AOB `dps_skin_table_read`, `gamework_skin_off`) | `src/core/signatures.rs` (the `dps_skin_table_read` description), `derive_ddr_selection` |
| `GameWork+0xA8` write / armed state | `src/mods/ddr_selection/mod.rs:137, 280-290` |
| Package helper (ignores World's skin arg; registers any name) | `src/mods/ddr_selection/package_helper.rs:118-150, 200-298, 300-307` |
| Policy table, `fixed_arc`, `SKIN_MAX`, `u8` masks | `src/mods/ddr_selection/policy.rs:26, 85-110, 121-248, 265-306` |
| Trigger / row values / AUTO buckets | `src/mods/ddr_selection/trigger.rs:14-54, 112-152` |
| Per-skin tables | `gauge_math.rs:43` `fill_mode`; `combo_math.rs:68/115/154/169`; `score_math.rs:58/83`; `song_info_logic.rs:42`; `panel_logic.rs:59/106/124/156/180`; `intro_logic.rs:123`; `banner_logic.rs:109`; `sound/rules.rs:138`; `options_force_logic.rs:136`; `marker_keys.rs:197` |
| Fixed-size structures | `combo.rs:110`; `stage_frame.rs:38-40, 193`; `banner.rs:60`; `sound/bank.rs:56`; `sound/call_voice.rs:74`; `s_marvelous/targets.rs:19, 152-158` |
| Alias arc precedent | `src/mods/webui_options/bg_preview_overlay.rs:741` `ensure_alias_arc`; `src/core/arc.rs:243` `rewrite_paths` |
| Texture overlay rules | `src/services/avs_layeredfs/ifs_textures.rs:601-618` (size), `purge_texture_replacement`; `mod_paths.rs:72-84` (path normalisation) |
| Runtime-sized option rows | `src/services/custom_options/mod.rs:441` `set_scalar_bounds`; `api.rs:669` `save_transform`; `src/mods/background_dancers/options.rs`, `catalog.rs`, `custom_content.rs:359-366` (label cut) |
| Stable-id persistence precedent | `src/mods/webui_options/mod.rs:129-142` |
| Content-discovery precedent | `src/mods/background_dancers/custom_scan.rs`, `custom_content.rs`; `.agents/planning/2026-09-22-background-dancers-custom-content/design.md` |
| AP2 editing | `src/core/ap2/edit.rs`; `src/services/afp_patcher.rs`; `src/mods/s_marvelous/assets.rs` |
| Sound bank | `src/mods/ddr_selection/sound/{bank,bank_build,cues,afp_route,call_voice,rules}.rs`; `src/services/game_audio.rs`; `src/services/se_bank_synth/`; `src/core/xact/adpcm.rs` |
| World addresses (20260825) | DPS `FUN_1800573d0` @ `0x180057af4..b6b`; Matching `FUN_180061520` @ `0x180061a13`; LayoutActor ctor `FUN_18006b3f0`, onInitialize `FUN_18006b8b0`, helper `FUN_18006b710`, record getter `FUN_18006ece0`; gates `0x1800582b3`, `0x180062155`, `0x18005c2fb`; readers in §2.2 |
