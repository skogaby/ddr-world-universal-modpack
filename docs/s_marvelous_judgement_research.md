# S-Marvelous — Net-New Judgement Type Feasibility Research

Deep-dive RE for adding a wholly new, discrete judgement grade — **"S-Marvelous"**,
window **±12 ms** — ABOVE the game's strictest stock grade (Marvelous, confirmed
**±17 ms**). Requirement constraints from the maintainer:

- NOT a replacement of an existing judgement; NOT a shift-everything-down insertion.
- A discrete, net-new grade with its own graphics, gameplay flash, and results
  presentation.
- New art / AFP layout work is acceptable and in scope for the effort estimate.

All addresses are **file-relative to base `0x180000000`**, from **gamemdx 20260721**
(Ghidra program `gamemdx_20260721.dll`) unless marked otherwise. Cross-version spot
checks against 20260616 and 20250805 are in §9.

---

## 0. Executive summary / verdict

**Feasible — with one architectural fork that decides everything.**

The judgement subsystem is *beautifully centralized on the input side* (one static
window table with exactly one consumer, one classification function, one submit
choke point) and *brutally decentralized on the output side* (at least ten
independent consumers of the grade index, every one of them built around a
hardcoded 8-grade space with **zero spare room**: 8-slot counter arrays whose
neighboring fields are live, an 8-case message-code range whose successor codes
`0x1030/0x1031` are already occupied by shock judgements, 8-entry display label
arrays, a fixed 8-field network save schema, and a per-note ghost byte stream).

Two viable architectures:

| | **Option A — true 9th internal grade** | **Option C — presentation-layer discrete grade** (recommended) |
|---|---|---|
| Engine sees | new grade index 8 / new judge code | Marvelous (unchanged) |
| Scoring/EX/gauge/combo | must be reimplemented per consumer | byte-identical to stock |
| Network/server | schema break; stock-server incompatible | untouched |
| Display | native, but every handler patched | mod-driven (proven modpack patterns) |
| Cross-version risk | extreme (jump tables, mid-function patches) | low (2 existing anchors + display hooks that already ship) |
| Effort | XL, effectively a fork of the judgement subsystem | M overall |

**Option C** keeps the engine's internal grade space untouched — an S-Marvelous IS
a Marvelous to every internal consumer (score, EX, gauge, combo, MFC logic, ghost,
save payload) — and implements the *discreteness* (separate count, separate flash
art, separate results row) at the modpack layer, classified from the exact same
per-note millisecond delta the stock classifier used. Because ±12 ⊂ ±17, the
subset relationship is structurally guaranteed. This is the same "engine-internal
identity, presentation-layer discrete" shape the modpack already ships for other
features, and every hook it needs **already exists in the codebase** (§6.1).

Option B (insert at index 0 and renumber downward) is excluded by requirement, and
§5.2 shows it is also the *only* way a table-level insertion can work — which is
the structural proof that a "native" discrete S-Marvelous cannot be done by data
patching alone.

---

## 1. The classification pipeline (input side)

### 1.1 The timing window table — `0x18035B9C0`

One static table of signed `(min, max)` int32 pairs, in music-count ticks
(1 tick = 1 ms of content time):

| pair index = grade | bytes | window | grade name |
|---|---|---|---|
| 0 | `EF FF FF FF 11 00 00 00` | −17 … +17 | MARVELOUS |
| 1 | `DE FF FF FF 22 00 00 00` | −34 … +34 | PERFECT |
| 2 | `AC FF FF FF 54 00 00 00` | −84 … +84 | GREAT |
| 3 | `84 FF FF FF 7C 00 00 00` | −124 … +124 | GOOD |
| 4 | `60 FF FF FF A0 00 00 00` | −160 … +160 | BOO |
| 5 | `00 00 00 00 00 00 00 00` | (dead) | — |
| 6–7 | (read from `0x18035B900`, a shared 16-byte zero block) | (dead) | — |

Confirms the maintainer's belief: **Marvelous is ±17 ms**, bounds inclusive.

Properties that matter:

- **Single copy in the whole binary.** Byte-pattern search for pair 0
  (`EF FF FF FF 11 00 00 00`) and pair 2 across all of `gamemdx_20260721.dll`
  each return exactly one hit — this table.
- **Single consumer.** The only xref to `0x18035B9C0` is the classifier at
  `0x18005F149` inside `judgeNotes`.
- The classifier stack-copies the table as **four XMM loads — three from
  `0x18035B9C0` and the fourth from `0x18035B900`** (the compiler pooled the
  trailing zero pairs into a shared zero block). Any table-patch plan must
  account for pairs 6–7 living at a *different* address than pairs 0–5:

```
18005f149: MOVDQA XMM0, xmmword ptr [0x18035b9c0]   ; pairs 0-1
18005f156: MOVDQA XMM1, xmmword ptr [0x18035b9d0]   ; pairs 2-3
18005f163: MOVDQA XMM0, xmmword ptr [0x18035b9e0]   ; pairs 4-5
18005f170: MOVDQA XMM1, xmmword ptr [0x18035b900]   ; pairs 6-7 (shared zeros!)
```

### 1.2 `judgeNotes` — `FUN_18005EC70`

`sequence::dance::GamePlayActor::judgeNotes(GamePlayActor* this, int musicCount)`.
Resolved version-agnostically today by the modpack's existing `judge_notes`
anchor (xref to the debug string `"sequence::dance::GamePlayActor::judgeNotes"`
@ `0x180360CF8`), and already detoured by the shared `judge_hook` dispatcher.

The classification walk (per candidate note, after press-to-note pairing):

```
18005f190: MOV  EAX, dword ptr [RDX]          ; pair.min
18005f192: LEA  ECX, [R8 + RAX*1]             ; note_mc + min
18005f196: CMP  ECX, R9D                      ; vs press_mc
18005f199: JG   18005f1a7                     ; too early -> next pair
18005f19b: MOV  EAX, dword ptr [RDX + 0x4]    ; pair.max
18005f19e: LEA  ECX, [R8 + RAX*1]             ; note_mc + max
18005f1a2: CMP  R9D, ECX
18005f1a5: JLE  18005f1b6                     ; match -> grade = pair index
18005f1a7: ADD  RDX, 0x8                      ; next pair
18005f1ab: LEA  RAX, [RBP + 0x20]             ; table end (8 pairs)
18005f1af: CMP  RDX, RAX
18005f1b2: JZ   18005f200                     ; walked off the end -> no grade
...
18005f1c3: LEA  RAX, [RBP + -0x20]
18005f1c7: SUB  RDX, RAX
18005f1ca: SAR  RDX, 0x3                      ; grade = (pair_ptr - base) >> 3
```

**Grade = index of the first matching pair.** First-match means windows must be
ordered tightest-first — this single fact kills every "just add a 12 ms pair"
idea that doesn't renumber the existing grades (§5.2).

Dispatch at the end of the walk (tap grades):

```
18005f700: LEA  R8D, [RBX + 0x1028]           ; judge code = 0x1028 + grade
18005f707: LEA  R9,  [RBP + -0x60]            ; info struct (side, delta, lanes, note*, ...)
18005f70b: MOV  RDX, RDI                      ; per-note result record
18005f70e: MOV  RCX, R13                      ; GamePlayActor
18005f711: CALL 0x18005fd30                   ; judge_submit
```

Other dispatch sites inside `judgeNotes`:

- Expired unjudged note → grade **5** (MISS, code `0x102D`) or grade **6** with
  code **`0x1030`** when the note is a shock arrow (avoided = success):
  `0x18005F45B` (`ADD EAX,0x5`), codes selected at `0x18005F477–0x18005F485`.
- Shock stepped → grade **7** (`MOV dword [RBX+0xC], 0x7` @ `0x18005F300`),
  code **`0x1031`** @ `0x18005F389`.

Tail calls: `FUN_18005F790` (freeze judge — dispatches
`(state != 2) + 0x102E`, i.e. **`0x102E` = freeze O.K. (grade 6), `0x102F` =
freeze N.G. (grade 7)**, through the same `judge_submit`, at `0x18005FC46`
region) and `FUN_180060340` (ghost/pacemaker comparator, §3.8).

### 1.3 Per-note result record (2nd arg to `judge_submit`)

Entries in the active-note ring, stride 0x40 (`ADD RBX,0x40` @ `0x18005F548`):

| offset | field |
|---|---|
| +0x00 | note* (note: +0x00 kind byte — 2 = freeze-class; +0x08 note musicCount; +0x1C..+0x38 per-lane occupancy ints (1/4 = steppable); +0x3C..+0x5C freeze lengths) |
| +0x08 | judged musicCount (the press time the classifier used) |
| +0x0C | grade (0xFF = unjudged sentinel) |
| +0x10 | display/count flag (`suppression byte at actor+0x1E8 == 0`) |

**The millisecond delta for classification is `(result+0x08) − (*(result+0x00)+0x08)`**
— exactly what the modpack's `power_user_statistics::data_feed` already computes
in its shipping `judge_submit` detour. Positive = late, negative = early. This
delta already includes every engine-side offset (SOUND OFFSET, per-player
JUDGEMENT OFFSET / per-song offsets mod) because it is the same quantity the
stock window walk classified — so a mod-side `|Δ| ≤ 12` test is *guaranteed*
to select a strict subset of stock Marvelous.

---

## 2. `judge_submit` — the choke point (`FUN_18005FD30`)

`judge_submit(GamePlayActor* this, result_record* rec, int judge_code, info*)` —
the modpack's existing `judge_submit` AOB resolves here (matches at
`0x18005FD31`; the function begins with a redundant-REX `40 55` prologue pad, the
shipped signature starts at the `55` — this quirk is already live on the cabinet
and harmless: byte 0 becomes a prefix to the detour JMP).

Every judgement of every note type — tap, jump, freeze O.K./N.G., shock
avoided/hit, mine (via shock-NG path) — funnels through this ONE function. What
it does, in order (grade = `rec+0x0C`):

1. **Per-grade counters** (unless suppressed via `actor+0x1E8`):
   - `actor + 0x1A0 + grade*4` ++ — GamePlayActor live counter array,
     **exactly 8 slots** (`0x1A0..0x1BC`); `+0x1C0` (freeze-O.K.-only counter)
     is a live neighbor, so the array cannot grow in place.
   - Mirrored into the per-side `ddr::player::Record`
     (`*(&DAT_1806F2ED0)[side]`): `Record + 0x218 + grade*4` ++ — again exactly
     8 slots (`0x218..0x234`); `+0x238` (current combo) is a live neighbor.
   - `+0x1C0`/`Record+0x248` ++ when note kind == 2 (freeze) and grade == 6.
   - `+0x1CC` judged-note count (double-counted for grades 4/5 on notes with a
     freeze tail — the "breaking a freeze head also forfeits the OK" rule).
2. **Fast/slow**: only for grades **1..4** (`if (grade-1U < 4)`) — Marvelous
   never counts fast/slow (so S-Marvelous inherits "no FAST/SLOW display" for
   free). delta<0 → `+0x1C4`/`Record+0x24C` (fast), delta>0 → `+0x1C8`/`+0x250`.
3. **Combo**: grades `<4 || ==6` continue (`+0x1DC`, max at `+0x1E0`,
   `Record+0x238/+0x240`; freeze-end notes kind==2 don't increment), else reset.
4. **Full-combo detection** (fires the moment the last step lands): when
   `combo == taps(+0x194) + shocks(+0x19C)` and `freezeOK(+0x1C0) == freezes(+0x198)`,
   broadcasts msg **`0x1034`** with type `0=MFC / 1=PFC / 2=GFC / 3=FC(good)`
   (good>0→3, else great>0→2, else perfect>0→1, else 0).
5. **Combo-changed** msg **`0x1033`** `{side, combo, maxcombo, grade, isFC}`.
6. **EX score** (`+0x1D8`, `Record+0x214`):
   `EX = (count[MARV] + count[OK]) * 3 + count[PERF] * 2 + count[GREAT]`.
7. **Money score** (`+0x1D4`, `Record+0x210`):
   `score = ((((MARV+OK+PERF)*5 + GREAT*3 + GOOD) * 200000) / ((taps+freezes+shocks)*10) − GOOD − GREAT − PERF) * 10`.
8. **Broadcast** of the judge code itself (`0x1028+grade`, `0x102E/F`,
   `0x1030/1`) to the GamePlayActor's child actor tree — this is what all the
   display consumers in §3 receive.

**Implication for Option C:** a post-original detour on this ONE function sees
the grade, the ms delta, the side, and runs after all stock bookkeeping AND all
stock display broadcasts — the perfect classification + display-override point.

### 2.1 GamePlayActor field map (judgement-relevant, 20260721)

| offset | field |
|---|---|
| +0x84 | side index |
| +0x88 | is_double |
| +0xB0/+0xB8 | active-note ring begin/end |
| +0x150 | foot-panel lamp target (`FUN_180027F00` per-panel flash) |
| +0x194 / +0x198 / +0x19C | total taps / freezes / shocks (MDX1529 error if taps+shocks==0 at commit) |
| +0x1A0..+0x1BC | per-grade counts [8]: M,P,G,Gd,Boo,Miss,OK,NG |
| +0x1C0 | freeze-O.K. count |
| +0x1C4 / +0x1C8 | fast / slow counts |
| +0x1CC | judged-note count |
| +0x1D4 / +0x1D8 | money score / EX score |
| +0x1DC / +0x1E0 | combo / max combo |
| +0x1E4 | consecutive-miss counter |
| +0x1E8 / +0x1E9 | judgement-count suppression / judgement-dispatch suppression bytes |
| +0x270/+0x278 | IFootPanel* slot (build-dependent; `judge_hook` auto-detects) |

`ddr::player::Record` (per-side global `*(&DAT_1806F2ED0)[side]`): `+0x204/208/20C`
note-population totals, `+0x210` money, `+0x214` EX, `+0x218[8]` grade counts,
`+0x238/+0x240` combo/max, `+0x248` freeze-OK, `+0x24C/+0x250` fast/slow.

---

## 3. Consumer inventory (output side — the fan-out surface)

Every place a grade index or judge code is consumed. This is the surface a
*native* 9th grade would have to conquer, and the menu a presentation-layer
implementation picks from.

### 3.1 Judge-code message map

| code | meaning | grade index |
|---|---|---|
| 0x1028–0x102D | tap grade 0–5 (M/P/G/Gd/Boo/Miss) | 0–5 |
| 0x102E / 0x102F | freeze O.K. / N.G. | 6 / 7 |
| **0x1030** | shock avoided (success) | 6 (display "O.K.") |
| **0x1031** | shock stepped / mine hit | 7 (display "N.G.") |
| 0x1032 | freeze-hold tick | — |
| 0x1033 | combo changed | carries grade |
| 0x1034 | full-combo splash, type 0..3 = MFC/PFC/GFC/FC | — |
| 0x1035 | fast/slow X reposition | — |
| 0x1036 | pacemaker / score-diff update | — |
| 0x103A / 0x103B | gauge empty (normal / risky-class) | — |
| 0x103C / 0x103F / 0x1046 | hide / gauge value / judge reset | — |

⚠ `0x1028 + 8 == 0x1030` — a hypothetical grade index 8 dispatched through the
stock `0x1028+grade` scheme **collides with the shock-avoided code**. A native
9th grade therefore needs a non-contiguous code AND a patch to every switch
below.

### 3.2 NoteResultActor — judgement flash (`FUN_18007B300`, msg handler)

The gameplay judgement text. Builds an **8-entry stack array of frame labels**
`{"in_marvelous","in_perfect","in_great","in_good","in_boo","in_miss","in_ok","in_ng"}`
(strings @ `0x1803630F8` etc.) and on codes `0x1028..0x102F`:
`this+0x94 = code−0x1028`, then on the `dance_judge` clip (`this+0xA0`):
`afp_layer_play` + `afp_layer_set_attribute(1,1)` + label→frame lookup
(`FUN_18026F3E0`) + goto-frame (`FUN_18026EE80`). Codes `0x1030/0x1031` map to
display indices 6/7. Fast/slow clip at `this+0xA8` (`in_fast`/`in_slow`,
grade ≠ 0 only). Receptor flash vector at `+0xE8` keys frame offsets off the
same label array; freeze tick (0x1032) uses `"in_marvelous_freeze"` as its
label base. This actor and its clip registry are **already captured by the
modpack** (`overlay_element_styling::capture` binds `dance_judge` per side;
`note_result_actor_vtable` signature ships today) —
`docs/gameplay_overlay_elements_research.md` has the full actor layout.

### 3.3 Groove gauge family

- `FUN_180070F70` (life-gauge message handler): recover-set {0,1,2,3,6}
  vs break-set {4,5,7}; **remaps 0x1030→0x102E and 0x1031→0x102F** then
  re-enters itself; per-grade recovery/penalty applied via the Record's gauge
  object vfunc `+0x1F8(grade)`; broadcasts gauge value msg `0x103F`.
- `FUN_180074BD0` (dance-gauge actor handler): explicit 10-case switch mapping
  every judge code to grade class 0..7, then `FUN_180074D90(this, grade, Δms)` —
  gauge 0..10000, per-grade delta from vfunc `+0x60`, death msgs `0x103A/0x103B`,
  consecutive-bad counter reset on {0,1,2,3,6}.
- `FUN_1800757F0` = `sequence::dance::FlareGaugeActor::calcJudgePoint` (debug
  string @ `0x1803628F8`): **its own 8-slot per-grade counter block at
  `this+0xEC..+0x108`, populated by a hardcoded 8-case switch**, flare-level
  damage tiers per grade class computed inline (huge constant-folded switch).

### 3.4 ComboActor — combo digit tinting (`FUN_180066950` digit refresh)

Combo digits are texture-swapped per "worst judgement this combo": suffix
pointer table @ **`0x180483350`** (10 pointers; `[0]="_marvelous"`,
`[1]="_perfect"`, `[2]="_great"`, `[3]="_good"` …), indexed by `this+0x6C`,
with matching RGB tint constants inlined in code (`0xA9FEEC`, `0xDFA6EF`, …).
Digit bitmaps loaded as `daco_combo{suffix}_{digit}` via `afp_mc_load_bitmap`.

### 3.5 Full-combo splash (`FUN_180069C50`)

msg `0x1034`: plays `se_game_fullcombo` and sets frame label on the splash clip —
`"marbelous_in"` (sic, Konami typo) / `"perfect_in"` / `"great_in"` / `"good_in"`
for types 0..3.

### 3.6 Result commit (`FUN_18005D970` — GamePlayActor vtable `0x180360D68`, slot +0x28)

Copies the live counters into the **stage record** at
`PlayerWork + 0x590 + stage*0x2B8` (course record at `+0x2D8`; PlayerWork =
`*(&DAT_1806F2ED0)[side]`, stage = `*(int*)(*DAT_1806F14F8 + 0xC)`):

| stage-record offset | source | field |
|---|---|---|
| +0x10 / +0x14 | +0x1D4 / +0x1D8 | money score / EX |
| +0x18 / +0x20 | +0x1DC / +0x1E0 | combo / max combo |
| +0x24 | +0x194 + +0x19C | judged-note denominator |
| **+0x28..+0x44** | +0x1A0..+0x1BC | **per-grade counts [8]** (next field +0x48 is live) |
| +0x50 | `FUN_1801E5320(money_score, cleared)` | dance-grade letter (**score-derived only** — a new judgement that keeps scoring intact cannot move grades) |
| +0x54 | computed | clear kind: 1=fail, 2=life4/risky clear, 3/6=gauge clears, **7=FC, 8=GFC, 9=PFC, 10=MFC** |
| +0x6C / +0x70 | +0x1C4 / +0x1C8 | fast / slow |
| +0x1B0/4/8/BC | +0x194/8/C, +0x1CC | note-population totals |

Also emits `MDX1529` when taps+shocks == 0, and the per-grade counts flow from
this record into the **network score save** (fixed field set — the marshal has
no slot for a 9th judgement; server schema is closed).

### 3.7 Results screens

- **Score tab** `FUN_1800F6BC0`: populates AFP text widgets by name —
  `marvelous_num_usr` (record+0x28), `perfect_num_usr` (+0x2C), `great_num_usr`
  (+0x30), `good_num_usr` (+0x34), `ok_num_usr` (+0x40), and **`miss_num_usr`
  = boo(+0x38) + miss(+0x3C) + NG(+0x44) summed**; plus score/EX/maxcombo/
  fast/slow widgets. The row set is baked into the results AFP layout
  (`scre_tab_detail_*` textures).
- **Graph tab** `FUN_1800ED610`: per-section judge markers
  (`scre_tab_graph_judge_%s` @ `0x18036D018`) and the Shift-JIS legend strings
  `MARVELOUS/PERFECT/GREAT/GOOD/MISS/COMBO` @ `0x18036CEC0` region.

### 3.8 Ghost / pacemaker comparator (`FUN_180060340`)

The ghost stream is **one byte per judged note, value = grade class**; the
comparator histogram-bins the bytes and computes an EX-equivalent to drive the
pacemaker delta (msg `0x1036`). Ghosts are downloaded from the server for other
players — a native new grade would change stream semantics for every consumer
of your ghosts.

### 3.9 Misc consumers

- `FUN_180063940` (TalentMeasurementSequence): per-section accumulation via a
  runtime `std::map<judge_code, weight>` @ `DAT_1806F3880`.
- `FUN_1800787B0`: shock-hit lane flash (0x1031 only).
- Song-select score popup: renders per-grade counts from *server* data (closed
  schema again).

---

## 4. Asset-side facts

- The judgement text art lives in the `dance_judge` AFP package —
  `data/arc/bm2d/dance_judge0000_v0.arc` (AFP + BSI + geo + texture atlas,
  loaded via `afplist.xml` by MD5 name — see `docs/afp_texture_pipeline.md`).
  The words MARVELOUS/PERFECT/… are timeline art addressed by the frame labels
  in §3.2.
- Results score tab layout + textures: `scre_tab_detail_*` set inside the
  result-scene package.
- The modpack's proven asset paths: whole-file arc replacement via LayeredFS,
  net-new texture injection into served atlases via `atlas_cloner` FRESH mode,
  and fully mod-owned widgets (`ImageWidget`/`TextWidget`/`SpriteLayer`)
  rendered through the game's own UI pipeline.

---

## 5. Design options analysis

### 5.1 Option A — true native 9th grade (NOT recommended)

What it would take, per §1–§3:

1. **Classification**: the window walk is first-match over an ordered table;
   a ±12 window must sit at index 0, which renumbers every stock grade. So the
   native path cannot be a table patch — it needs a code patch inside
   `judgeNotes` (or a full detour-reimplementation of the walk) that classifies
   ±12 separately and dispatches a *new* judge code.
2. **Judge code**: `0x1030` is taken (shock). A new code (e.g. `0x1050`) must be
   taught to **every** consumer in §3 — ≥8 message handlers, several of which
   lower their switches through compiler jump tables in `.rdata` (per-version
   table layouts; patching them is the most fragile patch class in this
   codebase's experience).
3. **Counters**: all four 8-slot arrays (actor `+0x1A0`, Record `+0x218`, flare
   `+0xEC`, stage record `+0x28`) have live neighbors — no in-place growth.
   Every counter write, every read (results screens, save marshal, FC logic)
   would need redirection to mod-owned storage.
4. **Scoring**: EX/money formulas read specific slots — unchanged if S-Marv
   also increments the Marvelous slot, but then the "native" grade is already
   half-presentation-layer anyway.
5. **Network**: the save marshal's per-grade field set is fixed; a 9th count
   has nowhere to go. Bemani-buddy could add a column, but stock-server
   compatibility (a project requirement historically) dies, and ghost streams
   (§3.8) change semantics for other players.
6. Estimated shape: a permanent detour-reimplementation of `judge_submit` plus
   display-handler patches on every consumer, per version. **Effort XL, risk
   extreme, and the result is still not server-representable.**

### 5.2 Option B — insert-and-shift (excluded, and structurally confirmed)

Writing `(−12,+12)` into the table at index 0 works mechanically (single table,
single consumer, and the modpack can already write it — the hex-edit community
mod ports in `docs/binary_modpack_research.md` do exactly this block-write) but
grade indices are *positional*: Marvelous becomes grade 1, Perfect grade 2, …
Boo grade 5 lands on the MISS slot. Every consumer in §3 misinterprets every
grade. Excluded by requirement; documented here only to close the door on
"cheap" table tricks.

### 5.3 Option C — presentation-layer discrete judgement (RECOMMENDED)

**Principle:** the engine's grade space stays untouched. S-Marvelous is defined
as `judge_code == 0x1028 && |Δms| ≤ 12`, evaluated in a post-original
`judge_submit` detour callback (the delta source in §1.3 — identical inputs to
the stock classifier, so S-Marv ⊆ Marvelous holds by construction). Everything
the player *sees* is then overridden or added by the mod:

1. **Classification + stats service** — extend the existing
   `power_user_statistics::data_feed` tap (it already parses the same opcodes
   and computes the same delta for calibration/timing-stats) or register a
   sibling subscriber; per-side, per-song S-Marv counters; reset via the
   existing `song_reset` subscription; per-song latch of enables at GAMEPLAY
   entry (house pattern).
2. **Gameplay flash** — two sub-options, in ascending fidelity:
   - **C-widget (lowest risk)**: keep the stock flash suppressed for that step
     (or let it start) and drive a mod-owned `ImageWidget` with new
     "S-MARVELOUS" art at the judge position, animating pop/fade from
     `input_manager::on_frame`. Position comes from
     `overlay_element_styling`'s existing `dance_judge` clip capture (per-side
     binding ships today). Zero AFP-format risk.
   - **C-afp (native look)**: LayeredFS-replace `dance_judge0000_v0.arc` with
     an edited AFP whose timeline gains an `in_smarvelous` labeled segment +
     art; the mod then re-drives the stock clip after the stock handler:
     `afp_layer_play` + label lookup + goto-frame — the *same three calls* the
     stock 0x1028 case makes (§3.2), on the already-captured clip, one event
     later in the same frame (post-original detour ordering guarantees the
     override wins). Needs AP2 timeline editing in the asset pipeline
     (bemaniutils-class tooling); the runtime side is trivial.
3. **Results screen** — mod-owned `TextWidget`/`SpriteLayer` row(s) on the
   result scenes (scene ids known; widget overlay on arbitrary scenes is a
   shipped capability): show `S-MARV n` and optionally re-render
   `MARV (stock − n)` so the two are visually exclusive. A full AFP-layout
   edit of the results score tab is the high-fidelity alternative (arc
   replacement + relayout of `scre_tab_detail_*` — significantly more asset
   work, same information).
4. **PUS integration** — S-Marv count in the timing-stats widget and the PUS
   CSV: trivial column additions to existing code.
5. **Stretch goals** (optional, independently shippable):
   - "All S-Marvelous combo" tint: a mod-tracked combo-quality bit driving
     `layer_set_color_raw` on the ComboActor roots (color-hook infrastructure
     ships in `overlay_element_styling`) — avoids touching the suffix table @
     `0x180483350` / inlined color constants entirely.
   - FC splash variant ("S-MFC"): same widget-overlay pattern on msg 0x1034.
   - Per-song S-Marv PB persistence via the persistence string-field registry
     (per_song_judgement_offsets pattern) — server-optional.

**What Option C does NOT change, by design:** score, EX, gauge, combo, MFC
classification, dance grade, clear kind, save payload, ghost bytes — all remain
bit-identical to stock. No score-guard/taint interaction. Autoplay steps land
at Δ≈0 and would all classify S-Marv; autoplay is already score-tainted so no
policy work is needed (display will simply show S-MARVELOUS, which is accurate).

---

## 6. Effort breakdown (Option C)

### 6.1 Existing infrastructure it stands on (no new RE needed)

| need | shipped provider |
|---|---|
| judge event + ms delta per step | `judge_submit` detour (`power_user_statistics::data_feed`, idempotent install) |
| `dance_judge` clip per side | `overlay_element_styling::capture` registry |
| per-song reset / in-place restart correctness | `services/song_reset` subscription |
| scene gating (gameplay/results) | `scene_manager` (+ scene ids doc) |
| widget rendering on any scene | `widget_renderer`, `ImageWidget`, `TextWidget`, `SpriteLayer` |
| net-new textures | `atlas_cloner` FRESH mode + LayeredFS |
| AFP clip ops (play/label/frame) | `bm2d_api` wrappers |
| side/entered-state | `stage_records` |

### 6.2 Workstreams

| # | workstream | size | notes |
|---|---|---|---|
| 1 | Classification + counters service (mod `s_marvelous`, config + option row) | **S** | ~1 deploy cycle; pure extension of data_feed pattern |
| 2 | Gameplay flash, C-widget variant | **M** | art + animation curve + position binding; cabinet iteration for look/feel |
| 3 | Gameplay flash, C-afp variant (optional upgrade) | **M–L** | AP2 timeline editing is the only genuinely new *tooling* in the whole feature; runtime side is ~30 lines |
| 4 | Results-screen S-MARV row (widget overlay) | **S–M** | includes the "MARV minus S-MARV" re-render decision |
| 5 | PUS / timing-stats / CSV integration | **S** | |
| 6 | Art: S-MARVELOUS word (stock style), results row label | **S–M** | style-matching the stock atlas art |
| 7 | Stretch: combo tint, FC splash variant, persistence | **S–M each** | independent |

Realistic core scope (1+2+4+5+6): **M overall** — comparable to the mid-size
shipped mods (music_wheel_song_length class), smaller than any of the L-class
features. Zero new signatures required for the core (see §7).

---

## 7. Signatures & anchors

Everything the core needs already ships:

| anchor | kind | status |
|---|---|---|
| `judge_notes` | debug-string xref `"sequence::dance::GamePlayActor::judgeNotes"` | shipped, resolves on all builds |
| `judge_submit` | AOB (prologue + `MOVZX [RCX+0x1E8]`) | shipped; verified matching `0x18005FD31` on 20260721 (fn `0x18005FD30`, REX-pad quirk) |
| `note_result_actor_vtable` / CMovieClip capture | RTTI + AOBs | shipped (`overlay_element_styling`) |

New anchors only if wanted for diagnostics / the C-afp variant:

- **Window-table derivation** (for logging the live Marvelous window, or a
  future window-tuning feature): from the resolved `judge_notes` body, scan for
  the first `MOVDQA XMM, [RIP+disp32]` (`66 0F 6F 05`) and decode — lands on
  `0x18035B9C0`. Structurally grounded: the table copy is the only XMM load of
  `.data` in the function.
- The label strings (`in_marvelous` @ `0x1803630F8`) need no anchor — the mod
  supplies its own `in_smarvelous` string for the C-afp variant.

## 8. Table of key addresses (20260721)

| symbol | address | role |
|---|---|---|
| `judgeNotes` | `0x18005EC70` | classifier |
| timing window table | `0x18035B9C0` (+ zero tail `0x18035B900`) | grades 0–4 windows |
| classification walk | `0x18005F149`–`0x18005F1CA` | table copy + first-match walk |
| tap dispatch | `0x18005F700` | `code = 0x1028 + grade` |
| miss / shock-avoid dispatch | `0x18005F45B`–`0x18005F512` | codes 0x102D / 0x1030 / 0x1031 |
| freeze judge | `FUN_18005F790` | codes 0x102E/0x102F @ `0x18005FC46` |
| `judge_submit` | `0x18005FD30` | counters/combo/EX/score/broadcast |
| ghost comparator | `0x180060340` | grade-byte stream → pacemaker |
| result commit | `0x18005D970` | GamePlayActor vtbl `0x180360D68` +0x28 |
| NoteResultActor msg handler | `0x18007B300` | judgement flash (8 labels) |
| LifeGauge handler / grade map / delta | `0x180070F70` / `0x180074BD0` / `0x180074D90` | gauge |
| FlareGaugeActor::calcJudgePoint | `0x1800757F0` | flare damage + own counters |
| ComboActor digit refresh | `0x180066950` | suffix table `0x180483350` |
| FC splash | `0x180069C50` | msg 0x1034, `*_in` labels |
| TalentMeasurement judge accumulator | `0x180063940` | map `DAT_1806F3880` |
| results score tab | `0x1800F6BC0` | `*_num_usr` widgets |
| results graph tab | `0x1800ED610` | `scre_tab_graph_judge_%s` |
| shock lane flash | `0x1800787B0` | 0x1031 only |
| per-side Record globals | `DAT_1806F2ED0` | `ddr::player::Record`/PlayerWork table |
| GameWork ptr | `DAT_1806F14F8` | stage counter @ +0xC |

## 9. Cross-version notes

Spot checks performed this session:

| build | window table | judgeNotes string anchor |
|---|---|---|
| 20260721 | `0x18035B9C0` (unique byte match) | `0x180360CF8` ✓ |
| 20260616 | `0x18035A9D0` — **byte-identical, unique** | `0x18035FD08` ✓ |
| 20250805 | `0x18033C710` — byte-identical (matches the address independently recorded in `docs/binary_modpack_research.md` for the community timing-window hack) | (not re-checked; `judge_notes`/`judge_submit` ship on this build) |

The window values (±17/±34/±84/±124/±160) have been stable across a year of
builds. `judge_submit`'s AOB and the `judge_notes` string anchor are already
cabinet-proven across the modpack's supported builds. Full four-build
verification of the *display-side* addresses (NoteResultActor handler, ComboActor
tables) is only needed if the C-afp variant or the combo stretch goal is picked
up — the C-widget core path touches none of them beyond the already-verified
capture signatures.

## 10. Gotchas

- **First-match walk ⇒ tightest-first ordering** — no table-level insertion can
  be discrete (§5.2). Do not revisit.
- **`0x1028+8 == 0x1030` (shock avoided)** — never dispatch a synthetic grade
  through the stock code scheme.
- **All four grade arrays are exactly 8 slots with live neighbors**
  (`actor+0x1C0`, `Record+0x238`, stage-record `+0x48`, flare `+0x10C`) — any
  out-of-range grade index write corrupts adjacent live fields.
- Marvelous (hence S-Marvelous) **never counts FAST/SLOW** (`grade-1U < 4`
  gate) — don't "fix" this; it is stock semantics.
- `miss_num_usr` on results = boo+miss+NG summed — any mod results row that
  re-renders stock numbers must reproduce this aggregation.
- Music-count ticks are content-time ms: under Song Playback Speed rate play,
  all windows (stock and the mod's ±12) scale identically in wall-clock terms —
  no special handling, but worth remembering when reasoning about "12 ms".
- Judged deltas already include SOUND OFFSET / JUDGEMENT OFFSET / per-song
  offsets — classify from the `judge_submit` delta and the subset property is
  free; classify from any *other* clock and it is not.
- The freeze-tick display path keys off `"in_marvelous_freeze"` on the receptor
  clips — unrelated to the judgement word; leave it alone.
- Autoplay ⇒ Δ≈0 ⇒ everything S-Marvelous; already score-tainted, no policy
  work needed.
- In-place restarts (quick restart / training loops) must clear the per-song
  S-Marv counters — subscribe to `song_reset` like calibration does.
- The full-combo splash label for MFC is `"marbelous_in"` (Konami typo) — match
  exactly if the stretch goal touches it.

## 11. Verdict

**Option C is feasible and well-matched to this codebase.** The engine keeps
Marvelous as its strictest internal grade; the modpack renders S-Marvelous as a
real, discrete, player-visible judgement with its own window (±12 ms), its own
art, its own gameplay flash, its own counters and results presentation — with
core effort **M**, zero new signatures, zero score/network risk, and every
required hook already shipping. The native-grade route (Option A) is documented
above as structurally hostile (message-code collision, four closed 8-slot
arrays, closed server schema, ghost-stream semantics) and should not be pursued.

Suggested next step if green-lit: PDD planning dir + a thin
`mods/s_marvelous/` skeleton with workstream 1 (classification + counters +
log-only diagnostics) as the first cabinet deploy, before any art is drawn —
it validates the ±12 subset classification against live play data for free.

## 12. Shipped implementation record (2026-08-30, plan Steps 1–9)

Option C shipped as `src/mods/s_marvelous/` (id `s-marvelous`, config
`s_marvelous.window_ms`, clamp 1..=17, default 12). Classification rides
the shared `judge_submit` tap in `power_user_statistics::data_feed`
(pre-original state ordering; display re-drives POST-original — the
dispatch is synchronous, §10). Per-surface mechanisms, each fail-open:

- **Runtime AFP synthesis** (`core/ap2/` — parser/serializer with 76-template
  byte-identity, editing primitives, `afp_patcher` patches on
  `dance_judge` / four `dance_fullcombo` templates /
  `body_tab_detail_result` / `result_root`): the display-side engine
  invariants this uncovered (label tables binary-searched by name ⇒
  serializer sorts on write; object id == death frame ⇒ cloned placements
  shift ids by the frame distance; afplist-listed geos only; per-image
  texture serving; dual-timeline label sets; split label/dictionary
  topology in result_root; looping segments' `gotoAndPlay` DoActions
  retargeted via the string-offset table) are recorded in
  `.agents/learnings/learnings.md` and enforced by
  `scripts/validate_s_marvelous.sh` Legs A–G.
- **Gameplay flash** (`flash.rs`): `in_smarvelous` label re-drive on the
  NoteResultActor's own wrapper (0xF09; actor resolved in the dispatch
  subtree via the RTTI vtable).
- **Combo digits** (`combo.rs`): post-original repaint of places
  {10,100,1000} + violet tint pair via the wrapper SetColor vfunc
  (`daco_combo_smarvelous_%d` FRESH textures).
- **S-MFC splash** (`splash.rs`): `s_marbelous_in` re-drive on MFC type 0
  when the combo was all-S.
- **Results score tab** (`results_score.rs`): 7-row sheet swap (stock-name
  replacements, purged at init/disable) + translate-only row moves + the
  game's OWN row-write helper for the S-MARV row + glyph rewrite for the
  exclusive MARVELOUS. Counts recomputed fail-closed from the stage
  record's grade/ms streams, judged-slots-only (partial plays).
- **Judgement graph** (`results_graph.rs`): three detours riding the
  game's rebuild/append/legend fns; violet series leads the judge stack;
  violet ■MARVELOUS legend entry first (maintainer art language: stock
  wording, violet hue, no "S-" prefix).
- **FC emblems** (`results_emblem.rs`): `loop_smfc` segment cloned in
  `result_root` (HSL rainbow-rotation records dropped ⇒ static violet;
  loop DoAction retargeted) re-driven at the results build; total-results
  badge `scre_total_player_fc_smfc` re-loaded into the `fullcombo_usr`
  leaves per S-MFC (side, stage).

S-MFC predicate (record-only): `clear_kind(+0x54)==10 && smarv==marv &&
marv>0` with the side's last-armed window. Full display-side RE (exact
addresses both builds, template dumps, suffix tables):
`.agents/planning/2026-08-29-s-marvelous-judgement/research/display-side-re.md`.

---

## Addendum 2026-09-01 — attract-demo arming + Marvelous FAST/SLOW

Two follow-ups from cabinet testing of the shipped mod.

### A. Attract demo "hodgepodge" (violet combo under a white MARVELOUS)

The mod armed the classification tap only at GAMEPLAY (0-idx 28) entry. The
attract demo (0-idx 16) runs its autoplay through the same GamePlayActor /
`judge_submit` / NoteResultActor / ComboActor chain, so during the demo:

- the tap was disarmed ⇒ no `in_smarvelous` re-drive ⇒ stock white word;
- but `state::combo_is_all_smarv` was a bare `!combo_has_loose_marv`, and a
  never-armed side's bit is trivially false ⇒ the combo override painted the
  S-Marvelous digits/violet tint for a combo the mod never classified.

Fix: (1) `combo_is_all_smarv` now requires `is_armed(side)` — the combo
override AND the S-MFC splash decline for any side whose judgements were not
classified; (2) the scene callback arms/disarms for a *play scene* =
GAMEPLAY ∪ ATTRACT_DEMO (`is_play_scene`), so with the mod enabled the demo
shows the full S-Marvelous presentation (flash word, combo, splash) exactly
like a credit. Autoplay steps land at delta 0 ⇒ every demo Marvelous is an
S-Marvelous.

### B. Marvelous FAST/SLOW (gameplay indicator + results totals)

Stock excludes Marvelous from FAST/SLOW in two independent places:

1. **Gameplay indicator** — NoteResultActor handler (`FUN_18007B300`,
   20260721), grade case, right after the word drive:

   ```
   18007b6f1  83 BF 98 00 00 00 00   CMP dword [RDI+0x98], 0   ; ms delta == 0
   18007b6f8  74 7B                  JZ  hide
   18007b6fa  83 BF 94 00 00 00 00   CMP dword [RDI+0x94], 0   ; grade == 0 (Marvelous)
   18007b701  74 72                  JZ  hide
   18007b703  ... show: play, set_attribute(1,1), in_fast/in_slow (CMOVL on the
              delta sign), goto-frame, SetPosition(this+0x108, this+0x10C) via vt+0x38
   ```

   `hide` = play + `set_attribute(1,0)`. Signature `note_result_fast_slow_gate`
   (`83 BF 98 00 00 00 00 74 ?? 83 BF 94 00 00 00 00 74 ??`) — unique and
   byte-identical on 20250805/20260616/20260721/20260825 (`0x180077DF1` /
   `0x18007B2F1` / `0x18007B6F1` / `0x18007BB01`). `fast_slow.rs` rewrites the
   grade CMP's imm8 (match+15) `00 → FF` on enable: grade ∈ 0..=5 in this
   branch so `grade == -1` never holds and the JZ is never taken; restored on
   disable. The delta==0 hide is kept (exactly on time is neither). The clip
   at `this+0xA8` is only created when the player's FAST/SLOW option is on —
   the patch respects that (null clip ⇒ nothing shown). Cabinet-wide, one
   aligned byte, no thread suspension needed.

   **Top-tier exemption (cabinet test #1 → fix):** the gate cannot tell an
   S-Marvelous from a Marvelous (a display-layer notion), so with the patch
   alone S-Marvelous steps showed FAST/SLOW too. Rule: the HIGHEST tier is
   exempt — stock Marvelous, now S-Marvelous. `flash::on_smarvelous` (already
   post-original on every S-Marv event with the NoteResultActor resolved)
   calls `fast_slow::hide_for_smarvelous`, which clears the `+0xA8` clip's
   visibility bit (`afp_layer_set_attribute(layer, 1, 0)` — the stock hide
   branch minus the redundant play) one event later in the same frame. Runs
   even when the word re-drive declines (unpatched template).

2. **Results totals** — `judge_submit` step 2 (`grade-1U < 4`) only
   accumulates `actor+0x1C4/+0x1C8` → record `+0x6C/+0x70` for grades 1..=4,
   and the score-tab populate (`FUN_1800F6BC0`) writes those into the
   SpriteLayer widgets anchored `fast_usr/num_usr` / `slow_usr/num_usr`
   (`%d` glyphs, alignment 1/1). The mod does NOT touch the counters (the
   score save marshals from the same record) — `results_score.rs` step 5
   rewrites the two widgets' glyphs to `stock + LOOSE-marvelous share`, the
   share recomputed from the record's grade/ms streams
   (`records::count_marv_fast_slow(grades, ms, window)`: grade-0 slots with
   `|ms| > window` — S-Marvelous is the exempt top tier — `ms<0` fast /
   `ms>0` slow, the stock counters' own sign rule), judged-slots-only like
   every other recompute, fail-open per widget. Invariant (host-tested):
   `smarv + marv_fast + marv_slow == marvelous total`.

Not changed: the graph tab's per-beat-division FAST / MARVELOUS / SLOW
statistics box (`GraphTab+0x4B8`, ingest `FUN_1800EB9C0`) — the game files
every grade-0/6 note into its own MARVELOUS column there (`iVar26 == 2`), so
Marvelous is already accounted for in that box by design.
