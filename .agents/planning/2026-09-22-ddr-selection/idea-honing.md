# Decision register — DDR SELECTION

Readiness Confirmed 2026-09-22 (user approved the round-2 register; `Assumed` rows accepted as stated).

Ordered by blast radius. ★ = opened by the full-fidelity scope (round 2).
Research backing: `research/orientation.md`, `research/hud-actors.md`,
`research/intro-and-skin-surface.md`, `research/sounds-options-folder.md`.

| ID | Decision | Recommendation / answer | Status |
|---|---|---|---|
| D1 | Trigger / UX | Per-player in-game OPTIONS row `ddr_selection` "DDR SELECTION": OFF / AUTO / 1st-5th / MAX-EXTREME / SuperNOVA / X / 2013-A. AUTO ⇒ D6 table; an explicit era applies to **every** song regardless of series. No DDR SELECTION folder; song select untouched | Accepted (user, round 2) |
| D2 | Governance | One skin per song for the cabinet. Solo: the entered side's row. Versus: P1 governs, row mirrored via `versus_mirror`. Bot side never governs | Accepted |
| D3 | Scope | **The entire A3 legacy-skin surface** (29-row enumeration, `intro-and-skin-surface.md` §4) minus the folder UI (D1). Delivered in phases inside this one project; the project is not complete until every phase ships (D27) | Overridden (user) |
| D4 | Mechanism | Restore A3's `%04d` append in the `LayoutActor` per-package helper (`FUN_18006b710`): per-package policy table {legacy as-is, legacy under a mapped base (`dance_stage` → `dance_stage_frame`), stock}; the record's skin (`+0x28`) = the skin only when a legacy variant was used; fallback = the **unsuffixed World base**, never `<base>0000`. Supersedes the resolver-level detour | Accepted |
| D5 | Skin-id plumbing | Write `GameWork+0xA8 = skin` for the gameplay window. Safe once D4 lands (suffixed names never collide with loader-owned entries); it revives World's live skin-1 gates (no SongInfo, no option icons) and routes every surviving skin branch through the record. D4 and D5 ship in the same step | Accepted |
| D6 | AUTO table | 1–5 → 1st-5th, 6–8 → MAX-EXTREME, 9–10 → SuperNOVA, 11–13 → X, **14–17 → 2013-A** (17 = DDR A), 0 / ≥18 (A20 onward) / custom → stock. Identical to A3's folder buckets | Accepted (user) |
| D7 | Persistence | `PersistMode::Local` | Accepted |
| D8 | S-Marvelous, first pass | Word flash + S-MFC splash stand down on songs whose judge / full-combo package is legacy; other S-Marv surfaces unchanged | Accepted |
| D9 | Modes | Normal play, training, versus armed. Course → stock (A3 never skinned a course), event chains 1/2 → stock (A3's shutter ignored the skin there), attract demo → stock | Accepted |
| D10 | `dance_danger` residency reconcile | Retired — D4 makes `dance_danger000N` a `LayoutActor`-owned entry, erased at finalize | Superseded |
| D11 | Mod default | `DEFAULT_OFF_MODS` until cabinet-proven | Assumed |
| D12 | Missing variant | Per-package fallback to stock + one INFO | Assumed |
| D13 | Spike first | Phase 0 = D4 + D5 + dev knob `DDR_SELECTION_FORCE=<1..5>` with the class-A packages only | Assumed |
| D14 | Series source | `find_music_by_mcode` + published `music_series_vslot` (`flare_skill_classifier` match+2); failure ⇒ AUTO = stock + one WARN | Assumed |
| D15 | Row label art | `seop_item_ddr_selection` via `scripts/option_strings.py`; values as Dynamic text | Assumed |
| D16 ★ | Song info panel | Faithful: skin 1 hidden (gate), skin 2 legacy band, skins 3–5 **A3's own panel** (`dance_song_info0000_v2` by explicit name + A3 SongInfoChild text layout) | Accepted |
| D17 ★ | Layout (element positions) | Faithful: World's root stays for World's builder; a post-pass overwrites the A3-defined markers from `dance_common000N` (skins 2–5) and from **A3's `dance_common0000_v2`** for skin 1 | Accepted |
| D18 ★ | World-only HUD elements | Hide on legacy skins: BPM display, player name (legacy frames have none). Keep: captions/subtitles, measure display, pacemaker, filter/cover/effect (A3 used its current art for those too). Option icons: skin 1 hidden (gate); skins 2–5 keep World's option display (A3's `dance_option_icon0000` has no afplist to load) | Accepted |
| D19 ★ | Gauge types without legacy art | FLARE / GRADE: exactly A3 (legacy bar, missing labels, continuous fill). FLOATING FLARE (no A3 spec): same as FLARE | Accepted |
| D20 ★ | Announcer for skins 4–5 | A3's announcer (`vo_ingame_*` from `voice_n`), as A3 players heard it (+≈11 MB bank) | Accepted |
| D21 ★ | World-only intro elements | Suppressed while a legacy skin is armed: the READY? panel, `vo_ingame_ready`, the 5.0 s ready dwell, World's stage voice. A3 had none of them | Accepted |
| D22 ★ | Skin-1 option forcing | Ported, mandatory for 1st-5th: speed ×1.0, boost/appearance/step zone/scroll normal, arrow colour FLAT, arrow CLASSIC, filter off, guideline off — World enum values per `sounds-options-folder.md` §B.2, snapshot/restore, never saved. In-song speed change stays available unless RE shows A3 blocked it | Accepted |
| D23 ★ | `_sel` background movies | Play `<movie>_sel` (18 songs) whenever a legacy skin is armed and the file exists (A3: category 13) | Accepted |
| D24 ★ | End-of-song banners | Legacy CLEARED / FAILED / PRAY FOR ALL from `common_shutter000N` | Accepted |
| D25 ★ | Stage-choice panel data | Fill every A3 panel field that has a World data source (stage, jacket/banner, difficulty, scores); fields with no World source stay empty and are listed in the design | Assumed |
| D26 ★ | S-Marvelous legacy art (final phase, per user) | Per-skin S-Marvelous word art + S-MFC splash + violet combo/digit treatment where the skin has them; art authored as PNGs under `data_mods/ddr_selection/s_marvelous/<skin>/` (drafted by a script from the installed legacy art, finished by hand; generated `_ifs` never committed); the runtime generalises S-Marv's clone recipes to the legacy templates | Accepted |
| D27 ★ | Phase order | P0 mechanism spike → P1 class-A swaps + gates + trigger UI → P2 intro (READY/HERE, kind-3 legacy panel, cut-in, stage voices, end banners, `_sel`) → P3 HUD re-hosts (gauge, combo, score, stage frame, song info, markers) → P4 era sounds → P5 skin-1 option forcing → P6 S-Marv legacy art. Risk-first: the shutter is the limbo-class risk | Accepted |
| D28 ★ | Legacy stage panel host | World's ShutterActor kind 3 (keeps the loader gate, DPS gates and quick-restart's `0x100c` dismiss valid) rather than a DLL-owned shutter | Assumed |
| D29 ★ | A3-only assets | Never committed. `scripts/ddr_selection/import_a3_assets.{sh,bat} <A3 install> <World install>` copies, from a hash-pinned manifest, every file the feature needs that World lacks or ships damaged into `<World>/data_mods/ddr_selection_a3/` (LayeredFS layout, path relative to `data/`). Today that is exactly one file: `arc/bm2d/dance_combo0005_v0.arc` (World's copy decompresses to all zeros). The DLL treats a missing import as "that element falls back to stock" + one WARN naming the script | Accepted |

## Notes

**D1 (round 2).** Folder trigger dropped by the user; folder research
(`sounds-options-folder.md` §C) stays on file, unused.

**D3.** Covered: class-A swaps (judge, fast/slow, full combo, game over,
danger), gauge (alias + P2 mirror + segmented fill), combo / score / stage
frame / song info re-hosts, legacy element positions, READY / HERE WE GO
(ReadyGoActor re-implemented), the legacy stage-choice panel + era cut-in +
SN2 banner + skin stage voices, legacy end banners, `_sel` movies, era
announcer + crowd SEs + skin-1 HERE voice, skin-1 option forcing, skin-1
SongInfo/OptionIcon hiding, S-Marv legacy art. Not covered: the DDR SELECTION
folder UI (D1).

**D4.** Consumers must be adapted before their package is allowlisted — a
World actor handed a package without its export NULL-derefs
(`hud-actors.md` C2) — so each package's policy entry lands in the same step
as its consumer's adaptation.

**D9.** A3 applied skins only via the folder, which courses never used. D1's
"all songs" is read as all songs a skin can reach; say so if courses should
be skinned too.

**D29.** Inventory (2026-09-22, stock installs side by side): 108 A3 files
the full scope reads — every legacy `dance_*000N_v0`, `dance_stage_frame000N`,
`dance_song_info0000_v2`, `dance_common0000_v2`, `common_choice_v2`,
`common_shutter_v2`, `common_choice*/common_shutter000N`, the cut-in arcs,
12 `banner_sn2_*`, 18 `_sel` movies, `voice_n.xwb`, `soundbanks_n.arc`,
`se_normal_n.arc`. World has all 108; 105 byte-identical. Differences:
`dance_combo0005_v0.arc` (World 61 312 B, dated with the 20260825 data update,
decompresses to 518 016 zero bytes — blanked; A3's is intact),
`soundbanks_n.arc` / `se_normal_n.arc` (World adds `se_edit_tick`; every
needed cue/wave identical — World's are used). The manifest makes the script
a no-op for anything World already ships intact, so other data versions that
lack files are covered by the same mechanism.

**D20.** Rule-fidelity alternative (World's announcer, A3 SEs only) saves the
11 MB but is not what A3 played.

**D22.** A3's value orders do not carry over (guideline OFF is 0 in A3, 2 in
World; FLAT is 2 vs 3). Speed is forced as type 1 / hispeed 100 so
`song_rate::real_speed` leaves it alone.
