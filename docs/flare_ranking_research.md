# Flare Ranking (Flare Skill) Research

Reverse engineering document for DDR World's Flare Skill system — how songs are
classified into the CLASSIC / WHITE / GOLD version categories, how per-chart
flare skill points are computed and accumulated into the player's flare skill
totals, and where to patch so that custom series values (raw `<series>` ≥ 22,
as introduced by the Series Expansion mod) are **excluded from flare ranking
entirely**.

**Game binaries**: `gamemdx.dll` — primary research on build **20260721**
(Ghidra program `gamemdx_20260721.dll`), cross-verified byte-for-byte on
**20260616** and **20260324**. All addresses file-relative to base
`0x180000000`; per-build addresses given as `721 / 616 / 324` where they
differ.

## Overview

Flare Skill is the per-profile ranking score shown on the profile/entry scene,
the song-select score window, and the TOTAL RESULTS screen. Its structure:

1. Every song is classified into a **version category** from its **raw**
   `<series>` byte (the same vtable+0xA0 accessor documented in
   `series_filter_internals.md` — NOT the mapped value from `series_mapper`):
   - series ≥ 18 → **GOLD** (category 3) — A20, A20 PLUS, A3, WORLD
   - series 14–17 → **WHITE** (category 2) — DDR 2013, 2014, A
   - series 1–13 → **CLASSIC** (category 1) — 1stMIX … X3 VS 2ndMIX
   - series 0 → **category 0** — no bucket (stock-dead: no stock song has raw series 0)
2. Per chart, flare skill points = `base_points[level] × (100 + 6 × flare_rank) / 100`
   (flare rank 0–9 = NONE…IX, 10 = EX ⇒ ×1.60).
3. Per style (single/double), the player's flare skill total = the sum of the
   **top 30 charts per category**, summed over categories **{1, 2, 3} only**.

**The GOLD test is an unbounded lower-bound check (`series >= 18`)** — this is
the confirmed root cause of custom series (≥ 22) counting toward GOLD. The
user-visible cutoff is series 18 (A20), not 17: series 17 (DDR A) is the last
WHITE entry.

Because category 0 is never iterated when summing totals, **classifying
series ≥ 22 as category 0 is a complete exclusion from flare ranking** using
only the game's own dead code path — no new mechanism needed.

## Key Data: Classification Tables

One 28-byte read-only block holds both tables plus the iterator sentinel.
Unique byte-content match in all three builds:

| Build | Block start | Category walk base | Threshold walk base |
|-------|------------|--------------------|---------------------|
| 20260721 | `0x18035A780` | `0x18035A788` (RVA `0x35A788`) | `0x18035A798` (RVA `0x35A798`) |
| 20260616 | `0x180359790` | `0x180359798` (RVA `0x359798`) | `0x1803597A8` (RVA `0x3597A8`) |
| 20260324 | `0x180357788` | `0x180357790` (RVA `0x357790`) | `0x1803577A0` (RVA `0x3577A0`) |

Layout (721 addresses; contents identical in all builds):

```
0x18035A780: 01 00 00 00    category[0] = 1  (CLASSIC)
0x18035A784: 02 00 00 00    category[1] = 2  (WHITE)
0x18035A788: 03 00 00 00    category[2] = 3  (GOLD)      ← cat walk base (walk offsets 0, -4, -8)
0x18035A78C: 00 00 80 3F    1.0f — unrelated neighbor; also serves as the
                            end-iterator address for the category-list vector
0x18035A790: 01 00 00 00    threshold[0] = 1   (CLASSIC if series >= 1)
0x18035A794: 0E 00 00 00    threshold[1] = 14  (WHITE   if series >= 14)
0x18035A798: 12 00 00 00    threshold[2] = 18  (GOLD    if series >= 18)   ← thr walk base
```

The **category-list vector builder** `FUN_1801dec00` (721) constructs
`vector<int>{1, 2, 3}` from iterators `&0x18035A780 .. &0x18035A78C`. This is
the list of categories that get **cleared and summed** in CalcFlareSkill —
category 0 is not in it.

## The Classification Walk (inlined 5×)

All five sites inline the identical 3-entry walk, highest threshold first.
Canonical form (CalcFlareSkill site, 721):

```
1801e26e0: FF 92 A0 00 00 00        CALL qword [RDX+0xA0]      ; raw <series> u8 → AL
1801e26e6: 44 0F B6 C0              MOVZX R8D, AL
1801e26ea: 33 C9                    XOR ECX, ECX               ; walk index = 0
1801e26ec: 0F 1F 40 00              NOP
1801e26f0: 42 8B 94 29 88 A7 35 00  MOV EDX, [RCX+R13+0x35A788]  ; R13 = module base (LEA-materialized)
1801e26f8: 46 39 84 29 98 A7 35 00  CMP [RCX+R13+0x35A798], R8D  ; threshold <= series ?
1801e2700: 7E 0C                    JLE classified
1801e2702: 48 83 E9 04              SUB RCX, 4
1801e2706: 48 83 F9 F8              CMP RCX, -8                  ; ← 3-entry loop bound (imm8 F8)
1801e270a: 7D E4                    JGE loop
1801e270c: 33 D2                    XOR EDX, EDX                 ; fallthrough → category 0
```

Key encoding facts (identical in all 3 builds, verified byte-for-byte):

- Addressing is `[index_reg + module_base_reg + disp32]` where the base
  register is materialized by a RIP-relative `LEA reg,[0x180000000]`. **The
  disp32 IS the table RVA** — module-base-relative, NOT rip-relative. A patch
  that redirects it must write `target_address − module_base` (contrast with
  the mod's usual `rip_disp` helper).
- The loop bound is the single imm8 byte `F8` (−8) in `CMP idx,-8`. Changing
  it to `F4` (−12) makes the walk consume a 4-entry table.
- Only the two disp32 values differ between builds; opcodes, ModR/M, SIB,
  register allocation, NOP padding, and branch displacements are identical
  across 20260324 / 20260616 / 20260721.

### All five walk sites

Addresses are of the walk's `MOV` instruction (its disp32 is at +4 for
4-byte-header forms, +3 for 3-byte-header forms — see encodings):

| # | Function (721) | Walk MOV 721 | 616 | 324 | MOV header | CMP header | Role |
|---|----------------|--------------|-----|-----|-----------|-----------|------|
| 1 | `FUN_1801e24f0` **CalcFlareSkill** | `0x1801E26F0` | `0x1801E2250` | `0x1801E0C40` | `42 8B 94 29` | `46 39 84 29` | **THE ranking calculation** |
| 2 | `FUN_1800fa7d0` score window | `0x1800FA920` | `0x1800F9FF0` | `0x1800FACB0` | `8B BC 08` | `44 39 9C 08` | song-select score window: picks `scre_tab_flare_{"",classic,white,gold}` tab bitmap; one interleaved `MOV [RSP+0x34],EDI` between MOV and CMP |
| 3a | `FUN_180192a10` sort comparator | `0x180192A70` | `0x180191990` | `0x180191050` | `42 8B AC 38` | `46 39 9C 38` | flare-category sort: classifies song A |
| 3b | `FUN_180192a10` sort comparator | `0x180192AE0` | `0x180191A00` | `0x1801910C0` | `42 8B B4 38` | `46 39 9C 38` | classifies song B; tiebreak = per-chart skill `FUN_1800fe410` |
| 4 | `FUN_180192b90` label builder | `0x180192C21` | `0x180191B41` | `0x180191201` | `8B AC 08` | `44 39 9C 08` | builds `"Category / %s"` header via `FUN_1801dec40` (name array at `DAT_18047B180 + cat*0x28`, runtime-init; entry 0 = empty string) |

These are the **only** readers of either table in the whole binary (Ghidra
xref query on both labels: exactly 5 each; content-scan for the cat-RVA disp32
bytes on 616/324: exactly 5 hits each, all in the equivalent functions).

## `ddr::player::Record::CalcFlareSkill`

| Build | Function entry |
|-------|---------------|
| 20260721 | `0x1801E24F0` (`FUN_1801e24f0`) |
| 20260616 | `0x1801E2050` |
| 20260324 | `0x1801E0A40` |

Identity proven by its own log call:
`XCnbrep700017d("ddr::player::Record::CalcFlareSkill", "style=%d, category=%d, count=%d, skill=%d", ...)`
(string at `0x180387CC0` on 721).

Signature: `void CalcFlareSkill(Record* rec, int style)` where
`rec = player + 0x178` (player slots at `DAT_1806F2ED0[side]`, 721).

Algorithm (validated against full disassembly, 721):

1. `rec->total[style] = 0` — totals live at `rec + 0x60 + style*4`
   (= `player + 0x1D8 + style*4`).
2. For each category in `{1, 2, 3}` (from `FUN_1801dec00`): clear the
   per-(style, category) entry list (map at `rec + (style+1)*0x20`, keyed by
   category via `FUN_1801e2d80`), reserve 30.
3. For every song in the music DB (`DAT_1806F2D78` vector, element stride
   0x258, element = music-data object with vtable: `+0x00` mcode, `+0x70`
   level(style, diff), `+0xA0` raw series, `+0xD0` chart-presence):
   - mcode `0x9733` (38707) is special-cased: points forced to 0.
   - classify raw series → category (walk site 1, `0x1801E26F0`).
   - for each difficulty 0..4: locate the per-chart record entry in the
     rb-tree at `rec + 8` (keyed by mcode; entry block at node+0x20, chart
     stride 0x30, doubles offset by +0xF entries for style==1):
     - level from vtable`+0x70`; `level > 20` ⇒ 0 points.
     - `base = base_points[level]` — 21-entry dword table at `0x180387CF0`
       (721): `{0, 145, 155, 170, 185, 205, 230, 255, 290, 335, 400, 465,
       510, 545, 575, 600, 620, 635, 650, 665, 680}` (matches the published
       flare-skill base table, Lv1=145 … Lv19=665, Lv20=680).
     - `flare_rank` = entry`+0x1C` (0–10; >10 ⇒ multiplier 0).
     - `points = base × (100 + 6×flare_rank) / 100` (EX=10 ⇒ ×1.60).
     - **`entry+0x28 = points` — written unconditionally**, regardless of
       category (this is the per-chart value the song-select info panel shows
       in "flareskill" display mode via `FUN_1800fe410`).
     - if `points > 0`: push `{mcode, difficulty, points, score}` into the
       per-(style, **category**) list — including category 0.
4. For each category in `{1, 2, 3}`: sort descending
   (points, then score, then mcode), truncate to top 30, sum the points and
   add into `rec->total[style]`. **Category 0's list is never summed** (and
   never cleared — see Gotchas).

### Callers (all four then read `player + 0x1D8 + style*4`)

| Caller (721) | Context |
|--------------|---------|
| `FUN_180090dd0` | card-in profile scene (`sceawi_title_profile`) — renders per-style totals + flare rank |
| `FUN_1800b7270` | `ddr::player::Work` stage-end record update — snapshots total before/after, stores per-stage skill **gain** into the play-result struct (fields `[0x19]`/`[0x1A]`) |
| `FUN_1800c9a90` | TOTAL RESULTS scene (`total_result_root`) — session flare-skill-gained counter |
| `FUN_1800fa7d0` | song-select score window (also walk site 2) |

## Patch Recommendation: 4-entry Extended Table

Add a highest-priority exclusion rule `series >= 22 → category 0` by
redirecting each walk's two disp32s at a mod-owned 4-entry table pair and
widening the loop bound by one entry.

**Mod-owned tables** (allocate ≥ 32 bytes with `memory::alloc_near` so the
base-relative offset fits in i32):

```
cat4: [ 1, 2, 3, 0 ]      cat_walk_base = cat4 + 12   (offsets 0, -4, -8, -12)
thr4: [ 1, 14, 18, 22 ]   thr_walk_base = thr4 + 12
```

Walk order after patch: `>=22 → 0` (excluded), `>=18 → GOLD`, `>=14 → WHITE`,
`>=1 → CLASSIC`, else 0. Stock behavior for series 0–21 is bit-identical.

**Per site, three byte patches** (fits the existing `SavedPatch` style):

1. MOV disp32 → `(cat_walk_base − module_base)` as i32
2. CMP disp32 → `(thr_walk_base − module_base)` as i32
3. loop-bound imm8 `F8` → `F4`

⚠ These disp32s are **module-base-relative** (the code adds a
LEA-materialized base register), so compute `target − ctx.game_module.base` —
do **not** use `rip_disp`.

**Scope guidance:**

- **Site 1 (CalcFlareSkill) is the only site required** to satisfy "excluded
  from flare ranking": totals, per-stage gain, and the profile/total-results
  numbers all flow from it.
- Sites 3a/3b/4 (sort comparator + "Category / %s" header) are cosmetic: patched,
  custom songs group under an empty-named category 0 bucket in the
  flare-category sort; unpatched, they group under GOLD. Either is safe —
  `FUN_1801dec40`'s name array entry 0 is a valid empty `std::string`.
- Site 2 (score window tab) picks from `{"", "classic", "white", "gold"}` —
  category 0 formats bitmap name `scre_tab_flare_` (empty suffix), a
  stock-dead path (no stock song has series 0). `afp_mc_load_bitmap` with a
  missing bitmap name no-ops with a log, but this is **untested territory**:
  validate on-cabinet before shipping site 2's patch, or leave site 2
  unpatched (custom songs keep showing the GOLD tab, cosmetically misleading
  but harmless).
- The exclusion category **must be 0** — site 2 indexes a 4-element array with
  it; any value > 3 would OOB-read if site 2 is patched.

## Signatures

### Primary derivation (recommended — codegen-immune, auto-discovers all sites)

Two-stage, in the style of `derive_playfield_styling`'s content-verified
disp32 redirects:

1. **Data scan** (`.rdata`) for the 28-byte table block — unique in all three
   builds:

   ```
   01 00 00 00 02 00 00 00 03 00 00 00 00 00 80 3F 01 00 00 00 0E 00 00 00 12 00 00 00
   ```

   Match `M` ⇒ `cat_rva = M + 8 − base`, `thr_rva = M + 0x18 − base`.

2. **Code scan** (`.text`) for the 4-byte LE encoding of `cat_rva`. For each
   hit `H`, validate structurally (fail-closed, skip non-conforming hits):
   - the 4-byte LE encoding of `thr_rva` occurs within `(H+4, H+20]` — call
     its location `T` (site 2 has one interleaved 4-byte MOV between the two);
   - within `(T+4, T+12]` the tail matches `7E ?? 48 83 ?? 04 48 83 ?? F8 7D`
     — the `F8` in that match is the loop-bound byte to patch.
   - patch points: disp32 at `H`, disp32 at `T`, bound byte in the tail.

   Expect 5 conforming sites (verified count on all three builds); treat any
   other count as a structure change and disable the patch (or require ≥ the
   CalcFlareSkill site, identified per below).

### Single-site AOB (CalcFlareSkill classification only)

```
FF 92 A0 00 00 00 44 0F B6 C0 33 C9 0F 1F 40 00 42 8B 94 29 ?? ?? ?? ??
46 39 84 29 ?? ?? ?? ?? 7E 0C 48 83 E9 04 48 83 F9 F8 7D E4 33 D2
```

- Unique match in all three builds: `0x1801E26E0` (721), `0x1801E2240` (616),
  `0x1801E0C30` (324) — pattern starts at the `CALL [RDX+0xA0]` (raw series
  getter), 0x10 before the walk MOV.
- Offsets from match: cat disp32 at **+20**, thr disp32 at **+28**, loop-bound
  `F8` at **+41**.
- Wildcards: only the two disp32s (data-layout dependent). Everything else is
  structurally fixed: the virtual-call ModRM (`92`/`A0` = the +0xA0 series
  accessor slot), MOVZX/XOR setup, both walk instructions' ModRM+SIB, and the
  intra-block branch displacements. Register allocation verified identical
  across three builds spanning four months; if a future build reallocates
  registers this AOB breaks loudly (no match) — fall back to the primary
  derivation, which survives that case.

## Cross-Version Notes

- Byte-identical walk encodings (modulo disp32) on 20260324, 20260616,
  20260721. Site count stable at 5.
- The table block moved `+0x2008` (324→616) and `+0xFF0` (616→721) — always
  re-derive RVAs from the data scan, never hardcode.
- `CMP idx, -8` / `SUB idx, 4` and the 3-entry table have been stable since
  the flare system's introduction; if Konami adds a 4th stock category
  (e.g., a new cabinet generation), the data-scan pattern changes and the
  derivation fails closed — re-research then.

## Gotchas

- **Raw vs mapped series**: everything here reads the **raw** series byte via
  music-data vtable+0xA0. The Series Expansion mod's `series_mapper` default
  patch (`xor eax,eax → mov eax,esi`) is irrelevant to flare skill —
  do not expect it to influence these sites, and do not "fix" this by
  changing the raw accessor (it feeds the version filter and label builders).
- **Category-0 list accumulation**: category 0's per-(style,category) list is
  pushed to but never cleared and never summed. With the patch live, each
  CalcFlareSkill call (profile scene, each stage end, total results — a
  handful per session) re-pushes one 0x18-byte entry per played custom chart
  with points > 0. Growth is bounded per session and freed with the Record at
  logout. Harmless, but expect the list to hold duplicates if inspecting at
  runtime.
- **Per-chart points still computed**: `entry+0x28` is written for every chart
  regardless of category, so the song-select info panel's "flareskill" display
  mode (`FUN_18019b9e0`, string `flareskill` @ `0x18037F5D8`) still shows
  nonzero per-chart values for custom songs. Only the *totals* exclude them.
  If per-chart display should also read 0, that's a separate patch (the
  `entry+0x28` store at `0x1801E288E` on 721) — not recommended; the value is
  informative and feeds nothing else.
- **The song-select FLARE sort tab is a separate mechanism**: the sort
  grouping registered in `FUN_18011f900` (`sequence::selectmusic` tab
  registration, tab id 6 "flare") uses a parallel name table
  `{0:"classic", 4:"white", 6:"gold", 9:"-"}` keyed by **version-filter entry
  index**, not raw series. With Series Expansion's custom filter entries
  (indices ≥ 9) custom songs appear to already group under "-" there.
  Hypothesis from table contents — the tab's predicate lambda was not
  disassembled; validate on-cabinet if this UI grouping matters.
- **Server-side ranking**: the client computes flare skill locally for
  display, and the per-stage gain lands in the play-result struct
  (`FUN_1800b7270`) which is part of the per-stage network save. If the
  backend (bemani-buddy) independently derives flare skill / flare ranking
  from raw score data, it needs a matching series ≥ 22 exclusion server-side —
  the client patch only governs what the game computes and shows.
- **Flare CLEAR banner is unrelated**: the results-screen flare banner logic
  (`FUN_1800f2700` lineage in `binary_modpack_research.md` §15) is the flare
  *gauge clear level*, not flare skill — no interaction with this patch.
