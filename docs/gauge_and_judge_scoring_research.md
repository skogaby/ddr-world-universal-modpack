# NORMAL gauge, judge acceptance and scoring — RE facts for the bot simulator

Addresses file-relative to `gamemdx.dll` base `0x180000000`, build **20260825** unless
noted. Purpose: reproduce, off-cabinet, the exact gauge / score / EX / combo / FC arithmetic
the game applies to a stream of judgements, so `tools/bot_sim` can render pass/fail and
scorecards for the Multiplayer Bot's skill model over the real chart corpus. Every formula
below is transcribed from the decompile; integer arithmetic is C (truncating) unless stated.

## 1. Judge acceptance — DDR World has NO Boo

`sequence::dance::GamePlayActor::judgeNotes` (`0x18005EC00`; window table `0x18035B9E0`).
The table still holds five `(lo, hi)` pairs — `±17, ±34, ±84, ±124, ±160` — but the accept
test after a window match is

```
threshold = (best_grade_this_frame < 5) ? best_grade_this_frame : 4;
if (grade_index < threshold) accept      // grade 4 (±160) can NEVER win
```

so an event 125..160 ms from the note is **matched (no window test at match time — the
held panel goes to the EARLIEST unjudged note carrying that arrow) and then rejected**:
the note stays unjudged, the press is NOT consumed (`consumePress` runs only for the
accepted note), and the note becomes a Miss (`grade 5`, code `0x102D`) once
`mc > note.mc + 160` (`0xA0`). Other facts the emulator needs:

- Walk cutoff: `mc < note.mc − 260` (`0x104`) ends the frame's walk.
- **One accepted note per actor per frame** — the best grade among candidates (strict `<`).
- No causality check: `event = mc − getPressAge(panel)` may lie in the future.
- Jump rule: a second panel's event must be within 66 ms (`0x42`) of the first.
- Shock arrows (all four panels of a pad TRG): checked with `wasJustPressed` inside
  `[note.mc − 34, note.mc + 84]`; a press there ⇒ `0x1031` (N.G., grade 7); reaching
  `note.mc + 84` untouched ⇒ `0x1030` (O.K., grade 6).
- `result+0x08` = the EVENT (not the frame's `mc`) for the accepted note, so the FAST/SLOW
  and the `judge_submit` delta are `event − note.mc`.

## 2. Freeze judge (`0x18005F720`, called after the tap walk every frame)

For each head note (kind 0) with a panel `length > 0` whose head was judged (`result.ts ≥ 0`)
and not Missed (`grade != 5`): while `cur_beat − note.beat < length` the panel must be held
(`isHeld`), a release increments a per-panel "release" counter (`result+0x34+p`, capped
`0xC`); the tail is resolved when the kind-2 note whose `beat == head.beat + max(length)`
is reached: **O.K. (grade 6, `0x102E`) if every held panel stayed held, N.G. (grade 7,
`0x102F`) otherwise**. A Missed head leaves the body unheld ⇒ N.G. The bot always holds
after a hit head ⇒ every hit freeze is O.K.

## 3. `judge_submit` (`0x18005FCC0`) — counters, combo, FC, EX, money score

Broadcast ORDER matters for the gauge: `0x1034` (FC type) → `0x1033` (combo changed,
`{side, combo, maxcombo, grade, isFC}`, only when the combo value changed) → **then** the
judge code (`0x1028 + grade` / `0x102E/F` / `0x1030/1`). The gauge therefore sees the
post-judgement combo.

- Per-grade counters `actor+0x1A0 + grade*4` (0 Marv, 1 Perf, 2 Great, 3 Good, 4 Boo,
  5 Miss, 6 O.K., 7 N.G.); freeze O.K. also `+0x1C0`.
- Combo `+0x1DC`: grades `< 4 || == 6` continue (kind-2 tail notes do NOT increment),
  else reset to 0; max `+0x1E0`.
- FC (`0x1034`) when `combo == taps(+0x194) + shocks(+0x19C)` and
  `freezeOK(+0x1C0) == freezes(+0x198)`: type `3` if Good > 0, else `2` if Great > 0,
  else `1` if Perfect > 0, else `0` (MFC).
- FAST/SLOW: grades 1..4 only; `delta < 0` fast, `> 0` slow.
- EX `+0x1D8` = `(Marv + OK)·3 + Perf·2 + Great`.
- Money score `+0x1D4` =
  `((((Marv + OK + Perf)·5 + Great·3 + Good) · 200000) / ((taps + freezes + shocks)·10)
   − Good − Great − Perf) · 10`   (i64 numerator, i32 truncating division).

## 4. Gauge actors (`GamePlayActor` ctor `0x18005BE20`, `Option::gauge` = `Option+0x18`)

| gauge value | class (vtable) | notes |
|---|---|---|
| 0 | `NormalGaugeActor` (`0x1803629C8`) | initial **0.5** (`0x18035B7B4`) × 10000 |
| 1..0xB | `GradeGaugeActor` | not modelled |
| 0xC / 0xD | `LifeGaugeActor` (`0x180361E18`, ctor `0x180070590`) | LIFE4 / RISKY: life count, Miss/N.G./Boo decrement, Good asks `Option` vslot `+0x1F8` |
| 0xE | `FlareGaugeActor` | not modelled |
| 0xF | `ImmortalGaugeActor` | never dies |

`GaugeActor` base ctor `0x180073B80(this, layout, initial_frac, danger_frac, rec_pct,
dmg_pct, gate)`: gauge `+0x90` (int, 0..10000, `= initial_frac × 10000`), `+0x94/+0x9C`
display, `+0xA0` = **level index 3** (or the `GAME_LEVEL` debug config), `+0xA4` = danger
threshold `= 0.28 × 10000 = 2800` (`0x180399C98`), `+0xA8` = recovery % (100), `+0xAC` =
damage % (100), `+0xB8` = dead flag (`initial ≤ 0`), `+0xBC/+0xC0` = last `0x1045`
`{cur_beat, music_count}`, `+0xC4/+0xC8` = combo / max combo (`0x1033`), `+0xCC` =
consecutive-bad streak, `+0xD0` = `cur_beat` at streak start, `+0xD8` = instant-death gate
(the `GamePlayActor+0x2B7` byte; 0 in normal play).

Message handler `0x180075010` (vslot 8): `0x1028..0x102D` ⇒ `apply(grade 0..5, delta)`;
`0x102E/0x1030` ⇒ grade 6; `0x102F/0x1031` ⇒ grade 7; `0x1033` ⇒ combo/maxcombo;
`0x1045` (sent every frame by the actor's tick `0x18005EA90` with
`{side, cur_beat, music_count, +0x18C}`) ⇒ `+0xBC = cur_beat`, `+0xC0 = music_count`.

### 4.1 `apply` (`0x1800751D0`)

```
if grade ∈ {4, 5, 7}:  if streak == 0 { streak_start_beat = cur_beat }; streak += 1
else:                  streak = 0
pts = judge_point(grade, delta, music_count)          // vslot 12, class-specific
if !dead: scale = pts > 0 ? rec_pct : dmg_pct  else: pts = 0, scale = dmg_pct
gauge += (scale * pts) / 100
if gauge < 1:
    if gate == 0: gauge = 0; broadcast 0x103A (DEATH) once; dead = true
    else:         gauge = 1; broadcast 0x103B once
else: gauge = min(gauge, 10000)
```

### 4.2 `NormalGaugeActor::judge_point` (`0x180075370`) — exact transcription

```
LEVEL_TABLE = [0, 3, 6, 10, 15, 22, 32, 50]; lvl = (LEVEL_TABLE[+0xA0] << 6) / 100   // = 6
if grade == 3 (Good): return 0                                   // Good never moves the gauge
if grade == 6 (O.K.): sev = 0 → RECOVERY
elif grade == 7 (N.G.): sev = 9 → DAMAGE
elif grade == 5 (Miss): sev = 13 → DAMAGE
else (0,1,2,4):  u = (delta·150)/1000 − 1;  sev = |u| / 2;  sev < 9 ? RECOVERY : DAMAGE
                 // ⇒ |delta| ≤ 84 (Great) always recovers; the dead ±160 row would damage

RECOVERY:
  t = ((combo + 1)·combo) / (maxcombo + 1)
  if combo < 1025 { if t > 20 { t = (t − 20)/10 + 20 } } else { t = 21 }
  t = min(t, 30)
  rec = ((((((t·t + 400) · ((20 − sev)/2)) / 500) · 20000) / (init + 10000)) · (96 − lvl) · 2) / 192
  if dead { rec /= 2 }; rec = max(rec, 2); if dead { rec = 0 }
  if init == 0 && combo < 3 { rec = 0 }
  value = rec

DAMAGE:
  ct = min(maxcombo, 30)
  v  = ((((ct·ct + 700) · sev · 4) / 700) · (init + 5000) · 2) / 30000) · (lvl·3 + 64)
  s  = streak                                  // already incremented for this event
  if (12 − maxcombo) < s && s > 4 { s = 0 }
  if cur_beat > streak_start_beat + 3072 { s = 0 } else { s = min(s, 8) }
  dmg = (trunc(v / 64) · 3) / (s + 2)
  if init < dmg && init > 1250 { dmg = init − 625 }
  value = −dmg

FINAL (both):
  x = ((music_count − 20000)·64) / 20000;  bell = max(4096 − x·x, 0)
  return (trunc(((bell + 4096) · value) / 8192)) · 10
```

`init` = the gauge's INITIAL value (`+0x90`, 5000 for a fresh NORMAL gauge; a course
carries the previous stage's gauge in). `trunc(a/b)` = C division. The `bell` term halves
every change at song start/end and is 1.0 exactly at `music_count = 20 s`.

Sanity anchors (fresh song, init 5000, `mc = 20 s` so the bell is 1.0; values in
gauge units of 10000 = 100 %): Marvelous at combo/maxcombo 1 ⇒ **+90**; Marvelous at
combo 100 ⇒ **+280**; Great (+80 ms) at combo 50 ⇒ **+150**; an isolated Miss with
maxcombo 30 (streak 1 ⇒ `/3`) ⇒ **−990**; the 2nd..4th consecutive Miss divides by
`s + 2` (`/4`, `/5`, `/6` — the only mercy); the **5th consecutive Miss** (with
maxcombo > 7) resets `s` to 0 ⇒ `/2` ⇒ **−1480**; a Miss in the first second (`bell`
0) ⇒ −490; N.G. at maxcombo 30 ⇒ −690. So a fresh NORMAL gauge survives roughly five
isolated Misses mid-song, and combos above ~50 recover ≈ 2–3 % per Marvelous.

## 5. Ranks (community table — NOT RE'd yet)

AAA ≥ 990000, AA+ ≥ 950000, AA ≥ 900000, AA− ≥ 890000, A+ ≥ 850000, A ≥ 800000,
A− ≥ 790000, B+ ≥ 750000, B ≥ 700000, B− ≥ 690000, C+ ≥ 650000, C ≥ 600000, C− ≥ 590000,
D+ ≥ 550000, D otherwise; a failed song shows E. The extra-stage grant reads
`record+0x50 >= 0xF` (rank index 15 = AAA), consistent with a 16-entry table (E + the 15
above). Verify against `scre_rank_*` if a rank ever matters beyond the simulator.
