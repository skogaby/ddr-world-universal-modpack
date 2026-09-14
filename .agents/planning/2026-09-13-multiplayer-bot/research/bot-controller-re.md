# Bot Controller — RE Findings (Step 4)

How the game's autoplay object is consumed by the judge, verified on `gamemdx_20260825`
and `gamemdx_20250805` (the two ends of the supported range). Addresses file-relative to
`0x180000000`.

## 1. `AutoFootPanel` vtable — identical shape on every build

| Slot | 20260825 | 20250805 | Body |
|---|---|---|---|
| 0 | `0x180022b60` | `0x180022680` | dtor |
| 1 `update(this, &results, cur_beat, mc)` | `0x180022d00` | `0x180022820` | see §2 |
| 2 `wasJustPressed(this, panel) → u8` | `0x180022ec0` | `0x1800229f0` | `return *(u8*)(this + 0x10 + panel)` |
| 3 `isHeld(this, panel) → u8` | `0x180022ed0` | `0x180022a00` | `return *(u8*)(this + 0x08 + panel)` |
| 4 (thunk) | `0x180022ee0` | `0x180022a10` | `tail-call vtable[5]` |
| 5 `getPressAge(this, panel) → i32` | `0x180022ef0` | `0x180022a20` | 0825: `(i32)(ord45_now − *(i64*)(this+0x18+panel*8))`; 0805: `timeGetSystemTime().ms − *(i32*)(this+0x18+panel*4)` |
| 6 `consumePress(this, panel)` | `0x180022f20` | `0x180022a70` | zero the pressTime slot (qword / dword) |

Vtable at `0x18035c938` (0825) / `0x18033d618` (0805); the DLL already resolves it by
RTTI (`auto_foot_panel_vtable`). Slots 2 and 3 are byte-array reads at fixed offsets on
every build; only 5/6 differ (clock + stride). Object size 0x58 (0825) / 0x40 (0805).

## 2. `update` (both builds, same algorithm; only the stamp differs)

```
zero isHeld[8] (+0x08), wasJustPressed[8] (+0x10)
for result in results:                     ; 0x40-stride, note = *result
    if mc < note.mc − 8: return             ; lookahead 8 ms
    if cur_beat < note.beat + max(note.length[0..8]):
        for i: if note.state[i] >= 2: wasJustPressed[i] = 1          ; freeze body hold
    if result.ts < 0 && result.grade == 0xFF:                          ; unjudged
        if isShock(note):  for i: wasJustPressed[i] = (state[i] != 1) ; avoid the shock
        else for i where state[i] ∈ {1, 4}:
            isHeld[i] = 1; wasJustPressed[i] = 1
            pressTime[i] = STAMP
```
STAMP: 0825 = `ord45_now − (mc − note.mc)` (back-dated so the judged event lands on
`note.mc`); 0805 = `timeGetSystemTime().ms` (event ≈ frame `mc`, within Marvelous).

## 3. The judge's algebra (`sequence::dance::GamePlayActor::judgeNotes`, 0825 `0x18005efed`)

Per frame, with `fp = *(actor + foot_panel_offset)` (0x278 new / 0x270 old):

1. Any `fp->wasJustPressed(i)` ⇒ `actor+0x1E4 = 0` and msg `0x1046` (input-activity).
2. `held_mask = OR_i fp->isHeld(i) << i`.
3. For each unjudged note in order (results vector `actor+0xB0..+0xB8`):
   - miss mark: if `actor+0x1E8 == 0 && note.mc + 160 <= mc` → `result+0x11 = 1`; grade 5,
     submit `0x102D` (shock: 6 / `0x1030`).
   - too early: `mc < note.mc − 260` → stop.
   - for each panel `i` with `state[i] ∈ {1,4}` and `held_mask & (1<<i)` (non-shock):
     `event = mc − fp->getPressAge(i)`; `diff = |note.mc − event|`; the panel matches if
     `diff < best[i]` (init `0x7fffffff` — **no window test at match time**); `event_used
     = max(event)`.
   - shock notes use `wasJustPressed` inside `[note.mc − 34, note.mc + 84]`.
   - jumps: a second panel's event must be within 66 ms (`0x42`) of the first.
   - when all arrows matched: grade = first `g` with `note.mc + lo[g] <= event_used <=
     note.mc + hi[g]` from the window table at `0x18035b9e0`; **no window ⇒ the note stays
     unjudged this frame and the press is not consumed** (so it may match the NEXT note
     on that panel later in the same loop).
4. On a grade: `fp->consumePress(i)` for the note's panels, `result+0x08 = mc`,
   `result+0x0C = grade`, submit `0x1028 + grade` with `delta = mc − note.mc`… (the
   `judge_submit` payload the DLL already taps).

Window table (inclusive, ms relative to `note.mc`): Marvelous ±17, Perfect ±34, Great
±84, Good ±124, Boo ±160; Miss when `mc > note.mc + 160`.

**Nothing in the judge checks causality (`event <= mc`).** A press whose `event` lies in
the future is graded on the frame it becomes visible.

## 4. Design consequence: a mod-owned `IFootPanel` with its own getter

Because the judge computes `event = mc − getPressAge(panel)` and reads the two flag
arrays through the vtable, a controller object that supplies its OWN slots 5/6 is
clock- and stride-independent:

```
struct BotFootPanel { vtable: *const [fn; 7], is_held: [u8; 8], was_just_pressed: [u8; 8],
                      event_mc: [i32; 8], _pad… }              // layout matches +0x08/+0x10
slot 2/3 = the STOCK readers (byte arrays at +0x10/+0x08 — same on all builds)
slot 4   = stock thunk (tail-calls slot 5 through OUR vtable)
slot 5   = fn(this, panel) -> i32 { CURRENT_MC[side] − this.event_mc[panel] }
slot 6   = fn(this, panel)        { this.event_mc[panel] = 0 }
slot 1   = never called by the game (the DLL fills the object itself)
slot 0   = never called (DLL-owned allocation)
```
Vtable clone precedent: `two_player_bpl_mode/logic.rs::clone_vtable_image` (COL copied to
`[-1]`). `CURRENT_MC[side]` is stashed by the pre-judge callback (the dispatcher passes
`music_count`); sides are judged sequentially on one thread.

The filler (pre-judge, per side, replacing the game's `update`):

```
zero both flag arrays
for result in results (in order):
    note = *result
    if mc < note.mc − LOOKAHEAD_MAX: break                    ; LOOKAHEAD_MAX ≥ |d_min| + 8
    freeze body: identical to §2 (always hold — D6)
    if unjudged:
        if isShock(note): wasJustPressed[i] = (state[i] != 1)    ; identical to §2
        else:
            plan = decide(note)            ; cached per (note ptr): Hit{d} | Miss
            match plan:
              Miss   → press nothing; blocked_until[i] = note.mc + 161 for its panels
              Hit{d} → E = max(note.mc + d, blocked_until[i] over its panels)
                       if E > note.mc + 160: treat as Miss
                       if mc >= E − 8: for i in arrows: isHeld[i]=1; wasJustPressed[i]=1; event_mc[i]=E
```
- `E − 8` reproduces the stock 8 ms lookahead so the judgement lands on the frame the
  virtual press happens; early presses (`d < 0`) are judged before the receptor crossing,
  exactly like a human's.
- `blocked_until` keeps a decided Miss honest in dense streams: without it, the next note's
  press on the same panel (≤ 124 ms later) would be attributed to the missed note as a
  Good/Boo (§3, no-window ⇒ not consumed). Pushing the next press past the miss window
  may downgrade that note — realistic and deterministic.
- Jumps: one `E` per note (all its panels share it) so the 66 ms jump rule never bites.
- Decisions are cached per note pointer for the note's life (`Results` entries are stable
  for the song; a `song_reset` / restart rebuilds them ⇒ cache cleared on GAMEPLAY entry
  and on `song_reset::on_song_reset`).
- `judge_submit`'s delta for the bot side = `E − note.mc = d` — PUS/S-Marv see honest ms
  errors for the bot.

Nothing about `Option+0x24` (JUDGMENT TIMING) or `SOUND_OFFSET` reaches the bot's
grading: `event = mc − (mc − E) = E` regardless of how `mc` was offset.

## 5. `ddr::player::Option` layout (0825; inline at `PW + player_option_offset`)

From the setters' debug strings (`FUN_1801e1b90..FUN_1801e23b0`, each `"Updated. %d->%d"`):

| Offset | Field (setter) | Notes |
|---|---|---|
| +0x08 | SpeedType | 0 = Real-Speed mode (`song_rate::real_speed`) |
| +0x0C | Hispeed (×100, 25..800, step 5) | |
| +0x10 | derived multiplier | recomputed by SetScrollSpeed from `+0x14` and BPM `+0x90` |
| +0x14 | ScrollSpeed (BPM target) | |
| **+0x18** | **Gauge** | default 0 = NORMAL (`PlayerWork::reset` → `Option::reset` yields 0; the extra-stage static forces 0xF) |
| +0x1C | ScrollDirection | |
| +0x20 | TimingDisp (±100) | |
| +0x24 | TimingMusic (±100) = JUDGMENT TIMING | irrelevant to the bot controller (§4) |
| +0x28 | Visibility | |
| +0x2C | ConstantValue (100..3000) | |
| +0x30 | LaneTransparency | +0x34 LaneCover, +0x38 FastSlow, +0x3C Guideline, +0x40 Stepzone |
| +0x44 | ComboDrawOrder | +0x48 JudgementDrawOrder, +0x4C JudgementLayout, +0x50 LaneNotice, +0x54 ScrollMoving |
| +0x58 | ArrowPlacement | +0x5C ArrowColor, +0x60 ArrowDesign |
| +0x64 | CutTiming | +0x68 CutFreeze, +0x6C CutJump (chart-altering — copying keeps chart parity) |
| +0x90 | f64 BPM | |

Block size for the copy: 0x98 (`+0x00` vtable ptr must NOT be copied — copy `+0x08..+0x98`).
The extra-stage override (`FUN_1801ea0e0` returning a static Option when `GameWork+0x59`
and mcode `0x9733`) is outside the bot's eligibility (never on the extra stage — the flip
gate requires `GameWork+0 == 0` and the grant is a later-stage concern).

## 6. Difficulty model inputs

Grade windows (§3) fix the model's geometry: with Gaussian σ per level,
`P(Marvelous) = erf(17/(σ√2))`. Anchors kept from D6 (σ₁₀ ≈ 5 ms ⇒ P ≈ 0.9993/note ⇒
≈ 70 % MFC on 500 notes; σ₁ ≈ 75 ms spreads across Great/Good/Boo; `p_miss` 0 → ~12 %).
Boo/Miss gauge damage and the NORMAL gauge (D10) make L=1 fail most charts without
guaranteeing it. All constants live in one pure module and are tuned on cabinet.

## 7. Interactions the controller must arbitrate

- **`autoplay` on the bot side**: per-side option values outlive the player, so the bot
  side's cached `autoplay` may be ON. The shared service must give the bot controller
  precedence over `Perfect` for that side (one INFO), and the autoplay watermark should
  ask the service (`controller(side) == Perfect && side_entered`) instead of its own flag.
- **`per_song_judgement_offsets`** writes the bot side's `Option+0x24` at first dispatch
  (`Priority::Early`) — harmless (§4).
- **`assist_tick`, `power_user_statistics`, `s_marvelous`** treat the bot as an entered
  side: the tick chooses its side by the sibling walk (same chart — harmless), PUS may
  show a stats widget for the bot, S-Marv paints the bot's pane. All acceptable; none
  alter the human's play.
- **`judge_hook` priorities**: the swap stays at pre `Late` / post `Early` (autoplay's
  current slots) inside ONE owner so `per_song_judgement_offsets` (Early) and PUS
  (Normal) keep their relative order.
