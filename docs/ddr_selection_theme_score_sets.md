# DDR SELECTION themes — stage-panel score sets (RE, 2026-09-26)

Status: RE for plan Step 5 of `.agents/planning/2026-09-25-ddr-selection-a-a3-themes/` (design
§4.10, Appendix C rows 1–4). Implemented by `src/mods/ddr_selection/score_set_logic.rs` (pure)
and `score_set.rs` (engine); cabinet test pending.

Ghidra: A3 `gamemdx_20240402` (A3 final), World `gamemdx_20260825` (checks on `20250805`).
Addresses are file-relative to `0x180000000`. Tags: **[dis]** decompiled / disassembled in this
pass, **[data]** read from the stock install, **[inf]** inference.

## TL;DR

- **Best record — anchored on `ghost_id_lookup`, not on a new AOB.** The game's TARGET switch
  (`FUN_18001dc90`, the existing swept `ghost_id_lookup` signature) calls the own-best lookup
  `FUN_1801e2c40(PW + score_db, mcode, style, difficulty)` in its case 0, inside bytes the
  pattern already pins. `derive_ddr_sel_score_set` decodes the callee (prologue-gated) and the
  `score_db` displacement: **`0x178` on 20260324+, `0x188` on 20250805 / 20260224**. [dis]
- **Target** — World's resolver `FUN_1801efa00(kind, code)` walks the set container unchecked,
  so it is not called; the set is found by the same probed first-match search
  `multiplayer_bot::target_name` uses, then the pinned getters `rival_set_score_entry`
  (`FUN_1801ee220`) and `rival_set_dancer_name` (`FUN_1801ee8c0`) give the record and name.
  The class table is **`{3, 3, 3, 3, 0, 1, 2}`** (Ghidra shows `{3,3,3,3,3,1,2}` because an
  8-byte store writes indices 3 and 4 at once). [dis]
- **Area exists in World**: `PlayerWork + 0x20`, valid when the profile byte `PlayerWork + 5`
  is set (A3: `+0x1C` / `+1`; World inserted a 4-byte side index at `+0`). Header offsets are
  identical on 20250805. The region rule is A3's, fed by the ark export
  `arkMDXGetLicenceKeyVersion`; the package language by `arkMDXGetGameOptionsLanguage`. [dis]
- **A3's display**, reproduced: no record ⇒ score `0` (ones digit only), rank and full-combo
  mark hidden, name and area still written. Unmapped name characters and unused slots show
  `playername_blank`. [dis]

## 1. World's best record (Appendix C row 1)

`ghost_id_lookup` (20260825 `FUN_18001dc90`) case 0, bytes `+144 .. +165` of the signature:

```text
+144  44 8B 47 04           MOV  R8D,[RDI+4]          ; GameWork+4 style
+148  48 8D 8A d32          LEA  RCX,[RDX+score_db]   ; PlayerWork + 0x178 (0x188 old)
+155  44 8B CB              MOV  R9D,EBX              ; difficulty (doubles clamp >= 1)
+158  8B D6                 MOV  EDX,ESI              ; PlayerWork mcode
+160  E8 rel32              CALL best_record          ; FUN_1801e2c40 (20250805 FUN_1801ca8a0)
```

- The callee's first 19 bytes are identical on 20250805 and 20260825:
  `48 83 EC 18 4C 8B 51 08 49 8B 42 08 80 B8 01 02 00 00 00` (tree header at `+8`, `_Isnil`
  `+0x201`). [dis]
- `best_record(db, mcode, style, diff)` returns the song node's entry, or null when the song has
  no node: singles `node + 0x20 + diff·0x30`, doubles `node + 0x20 + (diff + 5)·0x30`. A pure
  tree lookup. [dis]
- Entry fields (0x30 bytes): `+0x0` score, `+0x4` rank (0..15 in A3's `scene_choice_rank_*`
  order; `0x11` = none), `+0x8` clear kind (0 none, 2 assisted, 3 clear, 4..6 LIFE4, 7 good FC,
  8 great FC, 9 PFC, 10 MFC), `+0x10` ghost id. [dis]
- World's own stage panel (kind-3 fill `FUN_180035f00`) makes the same call with GameWork
  `+0x18` (mcode), GameWork `+4` (style) and the stage record's difficulty (`+4`); World's
  song-select side-info table (`FUN_18019b9f0`) through its three getters. The mod reads the
  stage record header (mcode `+0`, difficulty `+4`, style `+8`), which the song-select commit has
  already written for this stage (the same key `premium_free` uses). [dis]
- **"Has a record"** in the port: a readable entry whose clear kind is non-zero. World hides the
  song-select score on clear kind 0 and shows a target's score only when it is non-zero; A3 did
  not test it, so this only differs on an entry A3 would have shown as all zeros. [dis]/[inf]

Rejected anchor: deriving the lookup from `FUN_1800ff5a0` via `selectmusic_record_panel_refresh`
would read a call site behind a vtable getter; the `ghost_id_lookup` bytes are already pinned.

## 2. Target resolver, name, area (Appendix C row 2)

World kind-3 fill, target branch [dis]:

```text
type = (!edit && !FUN_1801de5b0() && GameWork+0xD0 not in {1,2}) ? PW+0x1328 : -1
-1          -> target_score_usr hidden
0           -> own record (score only if clear kind != 0)
1..3        -> FUN_1801efa00(3, PW+0x132C+(t-1)*4)  ; rival set by code, default set if absent
4..6        -> FUN_1801efa00(t-4, 0)                ; ranking set by kind, *end if absent (!)
then        -> FUN_1801ee220(set, mcode, style, diff) ; score if clear kind != 0
```

- `FUN_1801de5b0` is the same event-mode test (`GameWork+0xD0` ∈ {1, 2}); the edit byte is
  A3's, vestigial in World (no edit charts).
- Calling convention: MSVC x64. `FUN_1801efa00(i32 kind, i32 code) -> Set*`,
  `FUN_1801ee220(Set*, u32 mcode, i32 style, i32 diff) -> Entry*` (same 0x30 entry as §1),
  `FUN_1801ee8c0(Set*, u32, i32, i32) -> const char*` (rival: `set + 0x58`; ranking: the chart's
  holder `node + 0x24 + (style·5 + diff)·0x14`; absent: a static `""`).
- **No World area getter exists** (A3's `FUN_18012f680` has no counterpart; byte searches for
  its shape find nothing). The area sits just before the name in both layouts: rival set
  `+0x54` (code `+0x50`, name `+0x58`), holder entry `{code +0, area +4, name +8}`. The port
  reads `set + 0x54` for a rival and `name − 4` for a holder whose name is non-empty; an absent
  holder shows area 0 (`unknown`), as A3's getter returned 0.
- The mod never calls `FUN_1801efa00`; it mirrors its first-match search over the fully probed
  container, like `target_name::resolve`. A set it cannot find hides the whole target set.

A3 for comparison (`FUN_180030d10`): type `PW+0xDC0`, hidden for −1, edit data or
`FUN_180124000` (event mode); 0 = own name / area / record; 1..6 via `FUN_180130700` +
`FUN_18012eee0` (record) / `FUN_18012f680` (area) / `FUN_18012f5d0` (name). Any other value
leaves the set visible and unfilled; the port hides it.

## 3. Per-player area, region and language (Appendix C row 3)

- **Area.** The entry profile window (`FUN_180090f90`; 20250805 `FUN_18008a940`) reads
  `*(i32*)(PW + 0x20)` into World's area-name function after testing the e-pass profile byte
  `PW + 5`. [dis]
- **Region rule.** World's area-name function `FUN_1801ae0d0` is A3's `FUN_180100280` with
  `sceawi_area_region_*` names: region ∈ {1, 4, 6} ∧ area 1..=47 → `japan`; region ∉ {1, 6} ∧
  area 54..=104 → `america`; else the 119-entry table (area < 119), else `unknown`. The region
  is `(*DAT_1806f2388)(&out)`; the boot table `FUN_1800042c0` binds that slot to the ark export
  **`arkMDXGetLicenceKeyVersion`** (pairs are {slot, name}; `arkMDXGetMachineType`'s slot
  confirms the pairing). A missing export reads 0, as A3's zero-initialised out-parameter did.
- **A3's table** (`0x180262960`, `dancer_region_<name>`): 0 unknown, 1..47 the prefectures,
  48 hongkong, 49 korea, 50 taiwan, 51 america, 52 europe, 53 overseas, 54..104 the US states,
  105 japan, 106..118 the other countries (`score_set_logic::AREA_NAMES`). Every name is a
  texture in all eight `common_area_lang_{eng,jpn}_v{0,1,2}` / `_kor_v{1,2}` packages. [data]
- **Language.** `DAT_180cf349c` = `{0, 1, 10, 9, 8, −1}[arkMDXGetGameOptionsLanguage()]`
  (`FUN_180001060`), indexing the suffix table `0x1804652f0`: 0 `_lang_jpn`, 1 `_lang_eng`,
  8 `_lang_kan`, 9 `_lang_han`, 10 `_lang_kor`, null → `_lang_jpn` (`FUN_1801ace90`). World
  never loads `common_area` itself (only `common_texture`, which resolves `_v3`); the theme
  panel requests `common_area<suffix>_vN` when that arc exists (Chinese has none, Korean none at
  `_v0`: the area stays hidden). [dis]/[data]

## 4. A3's no-record display and glyphs (Appendix C row 4)

`FUN_180032240(root, set, name, area, record)` [dis]:

| Child | A3 |
|---|---|
| `choice_dancer_name_usr/highscore_name1..8_usr` | 8 slots always written: `playername_` + the glyph of `name[i]`, or of `' '` past the end |
| `choice_score_usr/highscore_%07d_usr` (10^i, i = 0..6) | digit `(score / 10^i) % 10`; visible iff `i == 0` or `score ≥ 10^i` (`FUN_1800ff9d0`, no padding); no record ⇒ score 0 |
| `highscore_rank_usr` | hidden when no record or rank > 15; else `scene_choice_rank_<r>` and visible |
| `fullcombo_mark_rotate_usr/fullcombo_mark_usr` | `scene_choice_fullcombomark_{good, great, perfect, marvelous}` for clear kind 7..10, visible; else hidden (the rotate container is untouched) |
| `highscore_area_usr` | always written (`FUN_180100280(area)`) |
| `highscore_difficulty_usr` (high-score set only) | `scene_choice_{beginner, basic, difficult, expert, challenge}` by difficulty (A3 also had `_edit_*`) |

- Glyphs (`FUN_1800ffe00`): `a`–`z` (upper case lower-cased), `0`–`9`, `' '` → `blank`,
  `!` → `exclamation`, `$` → `doll`, `&` → `and`, `-` → `hifun`, `.` → `dot`, `?` → `question`;
  **every other byte → `blank`**. World's names may also carry `, % + / ~`: they show blank.
- `pN_score_set_mc` visible iff the side is entered and (not a course or the first course
  stage). `pN_target_usr` is set visible explicitly unless hidden by the target rule.
- Name: World's rule (`FUN_180035f00`): `PW + 0xC`, or `PLAYER1` / `PLAYER2` (by the side index
  at `PW + 0`) when the entered side's name is empty. A3 used the raw name.
- Packages: `common_texture_v0` has every `playername_*` glyph and `scene_choice_num_0..9`
  (`_v3`, World's, has none of them — no texture name is shared); each theme root has the 16
  ranks, 4 marks and 5 difficulty textures; all three theme roots carry the score-set tree
  above (the target set without `highscore_difficulty_usr`). [data]
