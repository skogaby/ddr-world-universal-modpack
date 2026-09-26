# Step 10 RE — legacy score, difficulty and song info (2026-09-25)

Addresses are 20260825 (`0x180000000` base) unless stated; A3 =
`gamemdx_20240402`. Starting point: `hud-actors.md` §3 / §5 (this file
corrects and completes them).

## 1. Actors and functions

| | World `ScoreActor` | A3 `ScoreActor` |
|---|---|---|
| vtable (RTTI `.?AVScoreActor@dance@sequence@@`) | `0x180362df8` | `0x18026ac78` |
| init (slot 4) | `FUN_1800775d0` | `FUN_180055390` |
| finalize (slot 5) | `FUN_180078240` | `FUN_180055fc0` |
| update (slot 6, rival / ghost target) | `FUN_180077d50` | `FUN_180055aa0` |
| digit refresh (slot 7) | `FUN_180077eb0` | `FUN_180055be0` |
| msg (slot 8) | `FUN_180078320` | `FUN_180056080` |

| | World `SongInfoActor` | A3 |
|---|---|---|
| init (slot 4) | `FUN_180078fd0` (center_arrows_single's dark-card detour target) | `FUN_180056cc0` |
| SongInfoChild ctor / update | `FUN_1800792e0` / `FUN_180079a80` (`music_usr`, `artist_usr`, `source_usr`) | `FUN_180057070` / `FUN_1800578a0` (`music_name_usr`, `artist_name_usr`, `course_name_usr`) |

ScoreActor fields (identical on all five builds — init 20250805 `0x1800738a0`,
20260224 `0x180072a10`, 20260721 `0x180077190`, 20260915 `0x180077590`):
side holder `+0x58` (`**` = side; the LayoutActor per-side record / marker
parent, reverse flag at `+4`), record skin `+0x60`, level `+0x64`, score target
`+0x68`, displayed `+0x6C` (ctor `-1`), difficulty `+0x70`, clips score / difficulty /
name `+0x78/+0x80/+0x88`, player text `+0x90`, EX flag `+0xA0`. A3: the same
through `+0x80`, EX `+0x90`, no name clip (A3's `+0x88` is the name text).

## 2. World vs A3

- **init.** Both: record lookup (`dance_score`) → skin, package; the score clip
  (priority 7, layer side + 2, play, marker `score` position + scale), the
  difficulty clip (layer 1, marker `difficulty` position + scale). World:
  exports `dance_score`, `dance_difficulty`, plus `dance_name` (priority 7,
  layer 1, marker `name`, text in `name_usr`), EX ⇒ `score_usr` bitmap
  `dasc_score_exscore`. A3: exports `frame_score`,
  `frame_difficulty_<side+1>p[_reverse]` (reverse = `*(holder + 4)`,
  priority 3 on skin 2 else 7), no name clip (its name text went into the
  difficulty frame's `name_usr`, which no legacy frame has), EX ⇒ `ex_tex`
  visibility. Each create is `MOV R9D,7; LEA R8,[rip+name]; … CALL create`
  (World `FUN_180257af0`), and World NULL-derefs a failed create.
- **digits.** Identical logic (smoothing `min((t + d + 1) / 2, t)`, 7 places
  `0000001_usr … 1000000_usr` — the 10⁴ place is authored `0010001_usr` —,
  a place rewritten only when its digit changed or `displayed < 0`, leading
  zeros after the value is exhausted, hidden in EX mode, commas at places 3 /
  6 = `comma2_usr` / `comma1_usr`). Names: World `dasc_score_num_%d`,
  `dasc_score_score_num_0_off`, `dasc_score_comma[_off]`; A3
  `dance_score%04d_score_num_%d`, `…_score_num_0_gray`,
  `…_score_comma[_gray]`.
- **msg.** World `0x1036` (target = payload+4) / `0x104F` (difficulty) = A3
  `0x1039` / `0x1052`. World's difficulty: goto-play label `<difficulty>` on the
  clip root, `level_<abbr>_usr` ← `dasc_dif_<abbr>_level_%02d`. A3: child
  `difficulty_level_usr` label `<diff><side+1>` (skin 2) or `<diff><1|2>`
  (level ≥ 10 ⇒ 2) + `difficulty_level_usr/level_tex` ← `dance_score%04d_lv%02d`
  (not skin 2); skin 2 also `difficulty_level_base_usr` label `<diff>_in`.
  Difficulty names by index: beginner, basic, difficult, expert, challenge,
  basic.
- **finalize.** World releases `+0x90`, `+0x80`, `+0x78` — never `+0x88` (its
  own name clip is never released; the stand-in below inherits that).

## 3. Legacy data (`dance_score0001..5_v0`, all in World)

Exports `frame_score` (labels `in`/`loop`/`out`, the 7 places; `ex_tex` in 2,
3, 5; `comma1/2_usr` in 3, 5), `frame_difficulty_{1p,2p}[_reverse]`
(`difficulty_level_usr` = `difficulty_level` with labels
`beginner1/2 … challenge1/2`; skin 2 also `difficulty_level_base_usr` =
`difficulty_level_base` with `<diff>_in`; skin 5 a `level_base_tex`),
`difficulty_level`, `difficulty_level_base`. No `dance_name` / `name_usr`.
Textures: `dance_score000N_score_num_{0..9}` (all), `_0_gray` and commas
(2–5), no `lv%02d` art except skin 2's embedded A3 `dance_score0000_lv*` (so
the level-texture write misses on every skin, as in A3 — C6).

`dance_song_info0002_v0` exports `dance_song_info` (labels `in` / `loop` /
`out`, no text or jacket children) and `dance_song_info_sd`.

## 4. Interactions

- `song_reset` writes the score target `+0x68 = 0` and the displayed value
  `+0x6C = -1` — the port's digit refresh treats a negative displayed value as
  "repaint all", like World and A3.
- Step 7 markers: `score`, `difficulty` (`difficuty_normal_%dp%s_usr`) move
  with `dance_score`; `name` is parked off screen; `song_info` moves with
  `dance_song_info`.
- center_arrows_single detours the SongInfoActor init (dark card: style
  `+0xC4` forced to 1 ⇒ the `_double` name) — the song-info patch rewrites
  both names, so either choice yields the band.
- No other mod detours a ScoreActor function.

## 5. Mechanism as built — score (Step 10)

- **Derivation** `derive_ddr_sel_score` (RTTI slots 4 / 7 / 8, no AOB): the
  three creates (exactly three `41 B9 07 00 00 00 4C 8D 05` in the init, names
  checked in order), each clip store after its create, the record head `MOV
  RCX,[RCX+side]; CALL record; MOV R8,RAX; MOV EAX,[RAX+0x28]; MOV
  [RSI+skin],EAX`, the EX store `MOV [RSI+ex],AL`, the digit head `CMP
  [RCX+clip],0 … MOV R8D,[RCX+displayed]; MOV ECX,[RCX+target]`, the msg
  `SUB EDX,0x1036; JZ; CMP EDX,0x19` (0x104F), the difficulty `MOVSXD
  RAX,[RBX+diff]; … MOV R8,[RBX+clip]` and level `MOVSXD R8,[RBX+diff]; MOV
  R9D,[RBX+level]` loads — cross-checked against each other. Identical on
  all five builds; sweep ALL GREEN.
- **`score.rs`**: three detours on World's ScoreActor (init, digits, msg; no
  other owner). Init PRE for a legacy record: patches for this one call —
  the three name LEAs → a near buffer (`frame_score`, the per-call
  `frame_difficulty_<n>p[_reverse]`, the stand-in `difficulty_level_base`
  for the name clip) and skin 2's difficulty priority `7 → 3`; POST restores,
  hides the stand-in clip and sets `ex_tex` visibility from the EX flag.
  Digits: full replacement for legacy actors (A3 textures). Msg `0x104F`:
  A3's difficulty writes for legacy actors, World's otherwise. A patch
  failure skips World's init for that actor (no score that song — World's
  init would NULL-deref) and its digits / difficulty stay no-ops. The
  reverse flag = `holder + (hud_layout_reverse_off − records_side_off)`.
- **`score_math.rs`** (pure, host-tested): export names, priority,
  difficulty labels / level texture, smoothing, the digit / comma write plan.

## 6. Mechanism as built — song info (Step 10)

- Skin 1: World's own gate (no SongInfoActor). **Skin 2:** A3's band —
  `derive_ddr_sel_song_info` finds, in the SongInfoActor init (RTTI slot 4),
  the one `LEA RAX,["dance_song_info_single"]; LEA R8,["dance_song_info_double"];
  TEST; CMOVNE; MOV [RSP+0x20],1; MOV R9D,5` site (all five builds);
  `song_info.rs` patches both LEAs → near `"dance_song_info"` and the
  priority `5 → 9`, applied by the package helper before it registers
  `dance_song_info0002` (Step 7/8 pattern), restored on a stock
  `dance_song_info` request / disarm / disable. World's SongInfoChild finds
  no text children in the band ⇒ no text, as in A3. The `SongInfo` adapter
  also needs the marker post-pass (the band lands at `song_info_usr`).
- **Skins 3–5:** A3's own `dance_song_info0000_v2` panel with title /
  artist text — see §7 (RE + mechanism as built).

## 7. Skins 3–5 song info — A3's `0000` panel (RE 2026-09-25)

Answers the §6 open questions. Addresses 20260825 / A3 `gamemdx_20240402`.

### 7.1 Which arc A3 opened

- A3's stage loader lists `dance_song_info0000` in its gameplay package
  table (`0x180262f28`, mask `0x5000`); its probe `FUN_1800fe370` /
  `FUN_1800fe260` tries `%s_v%d` from `DAT_1802eee98` DOWN to 0, and
  `FUN_1800fe420` sets that to **2** unless the PC type (`FUN_180011b40`) is
  6 / 7 (then 1). So an HD A3 cabinet opened **`dance_song_info0000_v2`**
  (SD: `_v1`). A3's SongInfoActor init `FUN_180056cc0` takes the loader
  slot `+0x830` for record skin 0 — the skins 3–5 fallback — so skins 3–5 all
  showed that panel.
  **CORRECTION 2026-09-25** (`docs/ddr_selection_a3_themes_research.md` §2, §8):
  `FUN_180011b40` returns 6 / 7 only for machine type 4 = the **gold cabinet**
  (the same test picks `ddra3_bg_gold`). So `_v1` is the gold-cabinet set and `_v2`
  every other cabinet's, not HD vs SD: both are HD with identical image rects.
- World ships `dance_song_info0000_v0..v3`; `_v0` / `_v1` / `_v2` are
  byte-identical to A3's (md5), `_v3` is early-World art (292-px base).
  World's probe reaches `_v2` only through its bare rung, so the package is
  registered by its full name `dance_song_info0000_v2` (C3; the proven
  precedent is the skin-1 layout root `dance_common0000_v2`).
- `dance_song_info0000_v2` (`dance_song_info` export, labels `in` 0 / `loop`
  30 / `out` 90): base `dance_song_info0000_base` (376×56 dark rounded bar,
  pivot = centre, at (188, 28)), `music_name_usr` placeholder 340×28 at
  (188, 18), `artist_name_usr` 340×18 at (188, 44), both centred on their
  position; **no** `course_name_usr`, no `jacket_usr`, no colour transforms
  anywhere in the clip.
- Marker: `song_info_usr` = (640.5, 663) in `dance_common0003..5` (skin 2:
  668) — the Step 7 post-pass already moves the `song_info` key whenever
  `dance_song_info` is legacy.

### 7.2 A3 SongInfoChild vs World's

| | A3 (`FUN_180057070` ctor / `FUN_1800578a0` update) | World (`FUN_1800792e0` / `FUN_180079a80`, vtable slot 6) |
|---|---|---|
| children | `music_name_usr`, `artist_name_usr`, `course_name_usr` | `music_usr`, `artist_usr`, `source_usr` |
| font (BmpString id; `2d_font_*` table order identical in both games) | 3 = `2d_font_songtitle_m` (constant in `FUN_1800574f0`) | 4 = `2d_font_songtitle_s` (init: `MOV R9D,4` → ctor param) |
| text layout `+0xA8` (h-align: 0 left, 1 centre, 2 right — `FUN_18020cce0`) | 1 | 0 (`MOV [RDX+0xA8],R12D`, r12 = 0) |
| `+0xAC` (v-align) / `+0xB4` (fit mode) | 1 / 1 | 1 / 1 |
| fit box `+0x68/+0x6C` (absolute left / right limit) | `x ± w/2`, x = `(int)(pos.x + 0.5)`, w = `(int)` param `0x1015` | the same around `x − w` (`SUB EBX,EAX`) — with left align this starts the text at the placeholder's left edge |
| per-frame position | `(int)(pos + 0.5)` | same |
| colour | MC param `0x100a` RGBA | child `+0xA0..+0xAC` RGB × MC `0x100a` alpha (`FUN_180258af0`); the init writes white only for the double card, the ctor black |
| scale (× MC `0x100d`) | music 1.1, artist 0.8, course 1.1 | music 1.1, artist 0.8, source 0.8 |
| msg `0x104F` | rewrites the strings | same (null-checks every child) |

The A3 panel's placeholders carry no colour transform ⇒ A3's text is white
× the MC alpha — exactly World's double-card colour. Identical shapes on all
five builds (ctor / update / text helper / init sites; 20250805 init
`0x180075270`, ctor `0x1800755b0`, helper `0x1800759d0`). The ctor's other
caller is `DemoPlaySequence` (attract, font 3) — never during an armed song.

### 7.3 center_arrows_single

Its SongInfoActor-init detour flips style `+0xC4` to 1 around the original:
that only picks the `_double` card name (both names are patched to
`dance_song_info`) and the white child colour (forced for the panel anyway).
No second detour needed.

### 7.4 Mechanism as built

Checked code patches, helper-scoped like skin 2 (applied by the package
helper right before it registers `dance_song_info0000_v2`, restored on a stock
`dance_song_info` request / disarm / disable), pure plan
`song_info_logic::plan(Mode::Panel)`:

1. both card-name LEAs → near `"dance_song_info"` (priority stays 5);
2. init `MOV R9D,4` imm → 3 (font);
3. init `JNZ rel8` after `TEST R13B,R13B` → `90 90` (white text on the single
   card too);
4. child ctor + update: the two `music_usr` / two `artist_usr` LEAs each →
   near `"music_name_usr"` / `"artist_name_usr"` (`source_usr` left alone —
   A3's panel has no third child, it simply misses);
5. text helper: `44 89 A2 A8 00 00 00` (`MOV [RDX+0xA8],R12D`) → `C6 82 A8 00
   00 00 01` (`MOV BYTE [RDX+0xA8],1`; the dword was zeroed by the layout
   ctor `FUN_180210770`, same length), and `SUB EBX,EAX` → `90 90` (A3's
   centred fit box).

Derivation `derive_ddr_sel_song_info_panel` (optional group inside
`derive_ddr_sel_song_info`, never required; RTTI `SongInfoChild` vtable —
the ctor = the one init CALL whose target LEAs that vtable): 15 names, all
five builds, sweep ALL GREEN. Policy: `dance_song_info` skins 3–5 →
`fixed_arc = "dance_song_info0000_v2"`, adapter `SongInfoPanel` (panel sites
∧ marker post-pass); any patch failure or a missing arc keeps World's card.
Residual differences from A3: none known on HD (SD not ported — World always
uses the HD sizes); an unknown song shows empty strings (World) instead of
A3's raw basename.

## 8. A3 pacemaker on every legacy skin (maintainer request 2026-09-25)

Not in the original phase plan (A3 had no per-skin pacemaker). Findings:

- A3's NoteResultActor looks up the `dance_score_compare` record
  (`0x180058bf6`) exactly like World's (`FUN_18007aa40` `0x18007b07e`, record
  lookup `FUN_18006ece0` → registry by the record's NAME, no skin-0 loader
  branch). A3's helper appended the skin; no `dance_score_compare000N`
  exists for N ≥ 1, so A3's probe fallback loaded `dance_score_compare0000_v0`
  on every skin (the only version on both installs).
- World ships that arc byte-identical to A3's (md5). Against World's
  `dance_score_compare_v3`: same export `dance_score_compare`, same labels
  (`in` 1 / `loop` 25 / `out` 100), same eight `%08d_usr` children, same 13
  `dascco_*` textures. Only the art and layout differ: A3 = flat row, 20 px
  pitch, 22×20 blocky glyphs; World = rising diagonal, 14 px pitch, 22×22
  italic glyphs.
- World's msg `0x1036` digit / sign / tint code (`FUN_18007b710`) is A3's
  `0x1039` (`dascco_%d`, `%08d_usr`, `dascco_plus/minus/plusminus`, SetColor
  1.0 / 0.5 by sign) — identical logic.
- Everything else keys on the export / clip name (unchanged): PUS
  `pacemaker_swap` (ms-error readout, white zone), `song_reset`'s clip rewind,
  overlay_element_styling's Pacemaker class. The `score_compare` marker already
  moves to the legacy root on every armed song (Step 7).

**As built:** one policy row `dance_score_compare`, all skins, `Adapter::None`,
`fixed_arc = "dance_score_compare0000_v0"`. No code, no signature.
