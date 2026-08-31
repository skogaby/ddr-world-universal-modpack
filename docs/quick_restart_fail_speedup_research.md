# Quick Restart / Quick Fail Speedup — Fast Scene-Jump Research

Research for making the Quick Restart (triple-1) and Quick Fail (triple-3)
gestures as fast as possible: skip the 0.25 s death fade, the "STAGE FAILED"
shutter banner, and (for Quick Fail) the results screen, by driving the
engine's own scene-advance primitive directly from gameplay.

Primary build: **20260721** (all addresses file-relative to `0x180000000`).
Cross-checked on 20260616 and 20250805 (`gamemdx_20250805_MODIFIED.dll`)
where noted. Builds on `docs/quick_logout_research.md` (the `finish`
primitive, loader masks, package residency) — read that first; this document
only re-states what it adds to.

> **2026-08-12 status (root cause CONFIRMED on-cabinet, fast path v3).**
> Both `finish`-based fast-path attempts limbo'd: v1 targeted *live scenes*,
> v2 targeted the *loaders*. The v2 log proved the loader IS installed yet
> never advances. The first diagnostic deploy then nailed the single root
> cause for BOTH: the loader exit gate read TRUE mid-song (`gate=1` —
> refuting the interim background/movie hypothesis) while **`shutter=6`**:
> the kind-3 stage jacket panel parks at state 6 mid-song, and the loader's
> **mask-apply gate** (state ∈ {0,4}) — like a fresh DPS's state-1 gate —
> rejects it forever (§4a). The fix is the ShutterActor's own bannerless
> dismiss (msg `0x100c`: 6→7→8→0 with no new banner art) sent before
> `finish` (§4b). Natural death + redirects (§4c, cabinet-validated) remains
> the fallback for every gate failure.

---

## 1. TL;DR

The blocker for a bannerless mid-song scene jump was never `finish` itself —
it is the **stage shutter** (the kind-3 jacket "READY?" panel) parked at
state 6 for the whole song. Every `finish`-installed successor waits on a
shutter gate that only accepts idle (0) or closed (4), and state 6 only
advances when a new shutter request arrives — which in the natural flow is
the FAILED-banner request itself. The ShutterActor's msg **`0x100c`** is a
purpose-built bannerless dismiss (kind-3-only, 6→7→8→0, pending kind
untouched ⇒ no banner art ever loads). The shipped fast path:

| Gesture | Fast path (primary) | What it skips |
|---|---|---|
| Quick Restart | dismiss shutter (`0x100c`) → `finish(DPS, 0x1C)` — the 0-idx 27 stage loader → `getNextID 0x1D` fresh gameplay | fade, FAILED banner, READY panel, results, everything — a sub-second cut into the same song from the top |
| Quick Fail (session continues) | dismiss shutter (`0x100c`) → `finish(DPS, 0x19)` — the 0-idx 24 select loader → `getNextID 0x1A` song select | fade, FAILED banner, result loader, results screen, stage bump |

Fallback (any gate failure — unresolved anchors, shutter in a transitional
state, course, session-might-end, layout drift): natural death + one-shot
redirects — restart 29→28 (proven since 2026-05), fail 29→24
(cabinet-validated 2026-08-12; banner shows, results still skipped when the
predicate + m_currentID repair hold, else the full natural tail).


---

## 2. What the hand-edited reference binary actually does

`gamemdx_20250805_MODIFIED.dll`'s "Quick Fail / force logout / skip results"
(R19) was decoded instruction-by-instruction. Trampoline at `0x1802b8b80`:

```
E8 63 7D FB 00        CALL 0x1812708E8      ; step-export CSV check (mod #12)
E8 BE 76 FB 00        CALL 0x181270248      ; "is either Start held?"
0F 85 F3 CC DF FF     JNZ  0x1800b5883      ; held -> jump to the ALTERNATE state-pair block
6A 20 59              PUSH 0x20; POP RCX    ; not held -> restore original mov ecx,0x20
E9 4E CD DF FF        JMP  0x1800b58e6      ; resume stock code
```

The alternate block at `0x1800b5883` (stock code, not mod code):

```
83 F9 01 74 09        cmp ecx,1 / je        ; al = (GameWork+0xD0 in {1,2})
83 F9 02 74 04        cmp ecx,2 / je
32 C0 EB 02 B0 01     xor al,al / jmp / mov al,1
BA 21 00 00 00        mov edx, 0x21         ; TotalResultSequence (normal)
B9 38 00 00 00        mov ecx, 0x38         ; TotalResultSequence (event chain, 805 numbering)
84 C0 0F 45 D1        test al,al / cmovne edx,ecx
49 8B CC E8 ...       mov rcx,r12 / call finish
```

So the original mod fires at **`ResultSequence`'s own exit decision** —
*after* the STAGE FAILED banner and *after* the results screen has run — and
forces the **session-over** branch (`finish(0x21)` → TOTAL RESULTS → logout
tail). It is a "force end session at results" (which is why the same patch
site is described as Premium Free's "force logout button"), **not** a "skip
STAGE FAILED to song select". Nothing in it is worth porting; the useful
insight is only that state-pair ids fed to the dispatch are ordinary
1-indexed scene ids for `finish`.

> Correction to `docs/binary_modpack_research.md` §16: the state pairs are
> not "normal vs failed/quit-out post-stage transitions". They select
> between the **normal and event-mode scene chains** — `0x20/0x37`(805) =
> stage-bump Wait (continue), `0x21/0x38`(805) = TOTAL RESULTS (session
> over). The cmov key `GameWork+0xD0 ∈ {1,2}` is the event-chain selector,
> not a "save flag". (`.agents/planning/20260523-bulk-hack-porting/research/quick-fail-re.md`
> §"State-Pair Behavior Confirmation" flagged exactly this uncertainty.)

---

## 3. Where today's latency comes from

`DancePlaySequence::update` = `FUN_180057ec0` (20260721), internal states:

| state | behavior (verified decompile) |
|---|---|
| 8 | (entered after every GamePlayActor reports done — for a forced fail, after the 0.25 s `STEP_GAME_OVER` fade) picks the shutter-close kind and requests it via `FUN_1800334f0(kind)`: **4 = CLEARED, 5 = FAILED banner**, 3/8 = course next-stage, 0 = event; stops the song when every side is dead (`FUN_1801aa7c0(this+0x128)`) |
| 9 | waits for the shutter to reach state 4 (closed), then `finish(this, 0)` → `getNextID(0x1D)=0x1E` → the 0-idx 29 loader |

Quick Restart redirects that natural `0x1E` to `0x1D` (scene-manager
one-shot 29→28), so its visible cost is fade + FAILED shutter close +
rebuild behind the closed shutter + shutter open. Quick Fail (since
2026-08-12) redirects `0x1E` to `0x19` (29→24, §4c), so it shares the same
front cost and then cuts straight to song select; before that it ran the
whole natural tail (loader → `ResultSequence` → Wait → loader → song
select). Removing the fade + banner entirely is what the parked `finish`
fast cut was for (§4a).

---

## 4a. Why both `finish` fast-path attempts limboed (corrected post-mortem)

Two implementations were deployed and both reproduced the same hard limbo
(playfield gone, gameplay background still visible and animating, no further
scene transitions, game otherwise alive):

- **v1 — live-scene targets:** restart `finish(DPS, 0x1D)` (fresh GAMEPLAY),
  fail `finish(DPS, 0x20)` (the Wait).
- **v2 — loader targets:** restart `finish(DPS, 0x1C)` (0-idx 27 stage
  loader), fail `finish(DPS, 0x19)` (0-idx 24 select loader).

The v2 cabinet log is the decisive evidence: the scene hook logged
`prev=28 next=27` / `prev=28 next=24` (the loader WAS constructed and
installed — for restart the case-0x1c body even ran, logging `STOP SCAN
EAMUSEMENTPASS`) and then **no further scene change ever appeared** — the
loader never reached its `getNextID` edge. That refutes v1's post-mortem
("the target must be a loader, not a live scene"): a loader also hangs.

### 4a.1 What the framework RE rules out (Ghidra, 20260721)

- **Update delivery is a flat tree broadcast, not gosub-driven.** The main
  loop broadcasts msg `0x102` from the root via `FUN_18022eaa0`, which
  recurses the plain child/sibling links (`+0x18`/`+0x10`).
  `agcs::Sequence::onMessage` (`FUN_18021e070`) handles `0x102` by ticking
  *itself* (wrapping `+0x60`) and returning 0 so the broadcast recurses into
  its tree children. The gosub slot `+0x58` is written by `installChild`
  (`FUN_18021def0`) and read by scene logic, but **plays no role in update
  delivery** — a `finish`-installed child ticks like any other tree child.
  The "gosub suspension" hypothesis is dead.
- **Reaping is symmetric with the proven Quick Logout case.** DPS and
  `SelectMusicSequence` share the same `vt+0x08` (death veto) and `vt+0x10`
  (pre-delete) slots (`0x18018e360` / `0x18021de20`), so `finish` on a DPS
  is reaped exactly like `finish` on the select scene. No zombie
  resurrection, no suppressed subtree.
- **`installChild`'s skip-one-update flag is not it** — msg `0x109`
  (lifecycle `|= 4`) is only sent when the *parent* is inside its own update
  (`TS+0x60`), and it self-clears on the first skipped `0x102`. One frame at
  most.

### 4a.2 The actual choke point: the parked stage shutter (state 6)

`LoadingSequence::onUpdate` = `FUN_18002d200`. Full progression logic:

```c
// gate A — mask apply, one-shot (+0x80): SHUTTER null, idle(0) or closed(4)
if (!this->0x80 && (shutter==0 || state==0 || state==4)) { apply masks; this->0x80 = 1; }
// gate B — async load complete
if (!this->0x80 || *(char*)(resourceMgr + 0x24)) goto tick_timer;
// one-shot kind-specific background/movie setup (+0x78):
//   kind 0 (stage loader)  -> NO setup at all (kind < 1 skips the switch)
//   kind 2 (select loader) -> bg+0x128=1; CLEAR the MoviePlayerFrame
//                             (FUN_1800319b0: bgActor+0x68 := empty); ...
// gate C — the exit gate:
if (this->minTime (+0x74) <= this->elapsed (+0x70)
    && (BgMovieActor == 0 || FUN_180031af0()))
    finish(this, 0);                      // -> getNextID edge
tick_timer: this->0x70 += frame_dt;
```

The first diagnostic deploy (2026-08-12, sampler in the mod) measured the
gate inputs AT GESTURE TIME mid-song:

```
loader exit gate=1 (shutter=6 mgr_loading=0 switch_ready=1 maps_ready=(1,1) frame=0x0)
```

- **Gate C (the interim hypothesis) was NOT the blocker** — the
  background/movie system reads ready mid-song (`FUN_180031af0` = 1, the
  `MoviePlayerFrame` slot empty, both async movie maps idle).
- **Gate A is the blocker: the shutter sits at state 6**, which is neither
  0 nor 4 — the loader never applies its masks, so it never even reaches
  gates B/C. Limbo.

### 4a.3 The ShutterActor state machine (the missing piece)

`sequence::common::shutter::ShutterActor` — created in `createNextSequence`
case 4 as a direct **TransitionSequence child** (so `finish(DPS, …)` never
kills it); singleton `DAT_1806f2d40`; vtable `0x18035eba8`; update
`0x180033a50`; custom message handler (vt`+0x40`) `0x180034c60`. State =
embedded `agcs::StackStep` (`+0x58 + (*(u16*)(+0x82))*8`); **active kind at
`+0x310`** (−1 = none, 3 = the stage jacket panel), **pending requested
kind at `+0x314`** (written by msg `0x1007`, −1 = none).

| state | behavior (verified decompile) |
|---|---|
| 0 | idle, layer released. Advances only when a request is pending (`+0x314 ≥ 0`): loads the requested kind's art → 1 |
| 1 | wait art load; kind 3 also queues `data/arc/jacket/<song>.arc` → 2 |
| 2 | wait jacket load; **swap `+0x310 ↔ +0x314`** (kind becomes active); play the close/"in" label → 3 |
| 3 | wait for the close anim to finish → **4 = closed/covering** (what DPS state 9 and the loaders' gate A accept) |
| 4 | **parked.** Advances only on a new request (`+0x314 ≥ 0`) → 5 |
| 5 | gated on `FUN_180031af0` (bg ready). Active kind 3: play `stage_out` (the READY panel reveal), voice `vo_ingame_ready` → **6**. Other kinds: play the out label → 8 |
| 6 | **parked — THE MID-SONG STATE.** The revealed stage panel's layer stays resident. Advances only on a new request → 7 |
| 7 | stage-panel out tail (replays `stage_out` if it never ran; plays the final out label) → 8 |
| 8 | wait `out_end`; **release the layer, `+0x310 = -1`, state = 0** |

The song lifecycle: stage loader requests kind 3 (0→…→4 covered), DPS
state 5 sends msg `0x1008` (force state 5) → `stage_out` reveal → **parks
at 6 for the whole song**. At song end, DPS state 8's banner request
(`FUN_1800334f0(kind)`, msg `0x1007`) un-parks 6→7→8→0, and the pending
banner kind then drives 0→…→4 with the FAILED/CLEARED art. **The banner IS
the game's shutter-drain mechanism** — which is why every attempt to skip
the banner while leaving the shutter parked hung.

This also retro-explains v1 precisely: `DancePlaySequence::update` state 1
gates on `state==4 || (pending<0 && state==0)` — a fresh DPS `finish`ed
mid-song stalls at state 1 against the same parked 6. And it explains why
Quick Logout never hit any of this: at song select the shutter is idle (0).

**The gift-wrapped fix — msg `0x100c`** (handler in `FUN_180034c60`):

```c
else if (msg == 0x100c) {
    if (*(int*)(this + 0x310) == 3)      // stage panel active
        FUN_180210ee0(this + 0x58, 7);   // force state 7 — the drain tail
}
```

It enters the 7→8→0 drain **without writing `+0x314`**, so state 0 parks
idle and no banner art ever loads. The write is synchronous (read-back
verifiable). No stock code on 20260721 sends `0x100c` (byte scan: no
`BA 0C 10 00 00` in text) — it appears to be a debug/vestigial message, but
the handler is live behind the vtable and lands in the same drain states
the natural flow uses.

### 4a.4 Why the counter-proof works (and the old theories don't)

The shipped 29 → 28 restart redirect constructs a fresh `DancePlaySequence`
with **no loader in between** — and works. Under the shutter explanation
that's exactly right: at redirect time the banner request has already
drained-and-reclosed the shutter to state 4, which DPS state 1 accepts. The
"target must be a loader" theory (v1 post-mortem) and the "background/movie
system never settles" theory (interim §4a) are both dead; the shutter state
is the single discriminator in every observed case.

## 4b. The fast path: dismiss the shutter, then `finish`

Shipped 2026-08-12 (v3 of the fast path):

1. Gates: `sequence_finish` + `shutter_actor_global` resolved; live TS;
   live, not-dying child at `TS+0x58` (`flags & 0x24`); ≥1 GamePlayActor
   (vtable match — proves the child is a live DPS).
2. Shutter gate (`ensure_shutter_dismissed`, all reads range-validated —
   any surprise ⇒ fallback):
   - no ShutterActor, or fully idle (state 0, active −1, pending −1):
     proceed;
   - **state 6 + active kind 3 + pending −1: send `0x100c`** via the
     actor's own `onMessage` (vt`+0x18`, guarded on tree-flags `0x20`) and
     verify state reads back 7 — proof the build's handler took it;
   - anything else (READY panel at 1–4, a banner request in flight, drain
     in progress): fallback. Transitional states last well under a second;
     a gesture inside one just takes the natural path.
3. `finish(DPS, 0x1C)` (restart) / `finish(DPS, 0x19)` (fail — behind the
   §7 session-continues predicate). The loader installs, ticks, waits the
   sub-second 7→8→0 drain at gate A, applies masks (restart: no-op — the
   gameplay packages are resident; fail: loads the `0x6000` select
   packages), passes gate C (cabinet-measured true), `finish(0)` →
   `getNextID` → the target the natural way. No redirect, no m_currentID
   repair, zero new detours.

Destination behavior with an idle shutter (both verified in the decompiles):
a fresh DPS's state 1 explicitly accepts `pending<0 && state==0` — it skips
the READY panel and cuts straight into the song (state 5's `0x1008` send is
conditional on `state==4`, so it cleanly no-ops); song select's steady-state
shutter IS idle, so the select loader → `SelectMusicSequence` entry matches
its normal resting state. The old DPS is reaped the next frame with its
clean `onTerminate` (stops the song ~1 frame after the gesture, frees the
bank handles, restores 3D visibility) — well before the fresh DPS's
`onSetup` re-registers the same song banks (restart) or select music
rebuilds (fail).

Visible result: the gameplay elements vanish, the background persists for
the loader's sub-second drain+load, then the destination appears. No fade,
no banner, no READY panel, no results.

## 4c. Fallback: natural death + scene redirects

Both gestures force every active `GamePlayActor` to `STEP_GAME_OVER` (the
long-shipped death simulation: `+0x2B7`/`+0x2B8`/`+0x1E8` flags + step write)
and let DPS run its natural states 8/9 (fade, stop song, FAILED shutter
banner, `finish(this, 0)` → the 0-idx 29 loader request). A one-shot
redirect then swaps the 29 loader's construction:

- **Restart: redirect 29 → 28** (shipped since 2026-05, cabinet-proven).
  Fresh DPS, same song, stage counter untouched.
- **Fail: redirect 29 → 24** (new 2026-08-12) — the song-select
  `LoadingSequence` (kind 2, load `0x6000`, unload `0x31800`);
  `getNextID(0x19) = 0x1A` → song select. Entered post-banner with the
  shutter closed (state 4 — its mask-apply gate accepts it) and with the
  song already stopped by state 8, i.e. exactly the state in which the
  natural 29 loader passes the same exit gate today. Gated on
  `redirect_repair_available()` (the m_currentID repair — without it the
  tail after the redirected scene runs the wrong successor) and on the
  session-continues predicate (§7); otherwise the full natural tail runs.

Residency note for the 29 → 24 skip: the 29 loader's `unload=0xF000` never
runs, so the gameplay packages stay resident across song select — harmless
(the next 0x1c loader's `load=0x8000` becomes a no-op) — and `scene_result`
(`0x10000`) is simply never loaded, which nothing at song select reads.
The 29 loader's `arkSetIOStartScanEAmusementPass(0x10)` re-enable is also
skipped (same open observation item as before: no known mid-session
consumer).

## 5. The v2 attempt in hindsight (targets right, missing the dismiss)

v2's `finish` targets were correct and are what the shipped fast path (§4b)
uses — v2 just fired them against a parked shutter:

- **Restart**: `finish(DPS, 0x1C)` — 0-idx 27 stage loader
  (`FUN_18002d130(alloc, load=0x8000, unload=0x32000, kind 0)`),
  `getNextID(0x1C) = 0x1D`.
- **Fail**: `finish(DPS, 0x19)` — 0-idx 24 select loader
  (`load=0x6000, unload=0x31800, kind 2`), `getNextID(0x19) = 0x1A`,
  gated on the §7 predicate.
- v1's live-scene targets (`0x1D`/`0x20`) remain dead even with the dismiss
  available: the direct-`0x1D` DPS would actually work post-dismiss (its
  state-1 gate accepts idle), but the loader hop costs almost nothing and
  keeps the case-body side effects (audio settle, card-scan stop) natural;
  the Wait (`0x20`) additionally spins on `arkGetCurrentMode ∈ {3,6}` and
  is not a useful target from gameplay.

---

## 6. New resolution targets (verified on 3 builds)

### 6.1 `final_stage_probe`

The 36-byte tail of `FUN_1801DD660` (the pure final-stage-override test),
designed in `quick_logout_research.md` §8.2 and now shipped. Matches at
function start + 7:

```
48 8B 08 48 83 79 70 00 8B 51 0C 75 17 8B 81 D0 00 00 00
83 F8 01 74 0C 83 F8 02 74 07 3B 51 10 0F 94 C0 C3
```

| Build | Match | Count |
|---|---|---|
| 20250805 (MODIFIED) | `0x1801c6e47` | 1 |
| 20260616 | `0x1801dd1b7` | 1 |
| 20260721 | `0x1801dd667` | 1 |

Byte map (match-relative):

| offset | bytes | yields |
|---|---|---|
| −7 | `48 8B 05 d32` | GameWork ptr-ptr global (verify opcode, RIP-decode at −4) |
| +6 | disp8 `0x70` | course field offset |
| +10 | disp8 `0x0C` | stage counter offset |
| +15 | disp32 `0xD0` | **event-mode field offset** |
| +31 | disp8 `0x10` | **final-stage override offset** |

The offsets are literal in the pattern (a layout change ⇒ no match ⇒ fail
closed). `stage_records` decodes them anyway and cross-checks: gw global ==
the `stage_record_accessor` decode, course offset == the accessor's disp8,
stage offset == the `premium_free_stage_inc` disp8. Any disagreement leaves
the session-state accessors unavailable (quick fail then always falls back).

### 6.2 `max_stage_global` (derived)

`DAT_18047E784` — the operator's `/gameOptions/max_stage/current`, read
once per session start inside `createNextSequence` case 7:

```
18002e903  48 8D 15 d32    LEA RDX,[DAT_18047e784]     ; out-pointer
18002e90a  48 8D 0D d32    LEA RCX,["/gameOptions/max_stage/current"]
18002e911  FF 15 d32       CALL [avs property read]
```

Derivation (house style, `derive_max_stage_global`): find the unique string
bytes (`0x18035ba98` on 20260721, `0x18035aaa8` on 20260616, `0x18033c798`
on 20250805 — unique on all three), collect RIP-relative `LEA RCX` xrefs to
it (exactly one on every build checked), require the 7 bytes before the LEA
to be `48 8D 15 d32` (LEA RDX), RIP-decode → the global. Semantics: normal
stage count = `max+1`; the last normal 0-based stage index = `max`; index
`max+1` is the EXTRA stage.

### 6.3 `shutter_close_request` → `shutter_actor_global` (derived)

The whole-function pattern of the shutter-close broadcast wrapper
(`FUN_1800334f0` on 20260721 — sends msg `0x1007` + the kind to the
ShutterActor and its children). The `BA 07 10 00 00` (`MOV EDX,0x1007`) imm
pins it: a shorter tail-only pattern collided with the sibling kind-close
wrapper (2 hits/build); the full-prologue form is unique:

```
89 4C 24 08 53 48 83 EC 20 48 8B 1D ?? ?? ?? ?? 48 85 DB 74 ??
F6 43 20 20 75 ?? 48 8B 03 4C 8D 44 24 30 BA 07 10 00 00 48 8B CB FF 50 18
```

| Build | Match | Count |
|---|---|---|
| 20250805 (MODIFIED) | `0x1800337e0` | 1 |
| 20260616 | `0x180034020` | 1 |
| 20260721 | `0x1800334f0` | 1 |

`derive_shutter_actor_global` RIP-decodes the `MOV RBX,[rip+d32]` at
match+9 (d32 at +12) → the ShutterActor singleton global. The fast path's
struct offsets on the actor (StackStep `+0x58`/`+0x82`, kinds
`+0x310`/`+0x314`) are not in the pattern; they are range-validated on
every read and the `0x100c` dismiss is verified by the synchronous state-7
read-back — a layout drift degrades to the fallback.

---

## 7. The session-over guard (Quick Fail only)

Skipping `ResultSequence` (the fast select-loader jump and the 29 → 24 redirect, §4b/§4c) means skipping the
game's only session-over decision (`+0xE8` computation,
`quick_logout_research.md` §5.2). The skip must therefore prove **the
session would have continued**; otherwise a final-stage quick fail would
gift a bonus song (the game self-corrects at the *next* song's
ResultSequence via the `max+1 < stage` early-out, but the extra pick is
wrong).

Conservative predicate — every condition must hold, any unknown ⇒ the full
natural tail:

```
course      == 0        (GameWork + course_off, qword)
event mode  == 0        (GameWork + 0xD0)   — ∈{1,2} = event chain (never at scene 28, belt-and-braces)
override    == -1       (GameWork + 0x10)   — nothing writes it in stock; a future
                                              Mechanism-C mod would, so respect it
stage       ∈ [0, 9]    (GameWork + 0xC)
max         ∈ [0, 9]    (DAT_18047E784)
stage < max             — strictly below the last normal stage index
```

`stage == max` (final normal stage — where the extra-stage grant decision
lives) and `stage == max+1` (extra stage) both take the full tail: the
natural flow runs `ResultSequence`, which ends the session properly
(→ TOTAL RESULTS → logout). With Premium Free active the counter is frozen
below `max`, so the skip applies to every play — consistent, since a
frozen session never ends via stage count anyway. The fallback redirect is
additionally gated on `redirect_repair_available()` (§4c).

Future option (not implemented): route final-stage quick fails through
quick-logout's Mechanism A from gameplay (natural death + one-shot
redirect 30→32) to skip the per-song results while keeping
TOTAL RESULTS → logout.

---

## 8. Threading / re-entrancy

The gesture fires from the render-thread input poll (the frame thread —
quick-logout-proven). The fast path's `0x100c` send is a synchronous state
write through the actor's own `onMessage` on the same thread the game
dispatches messages on (guarded on tree-flags `0x20` like the game's own
wrappers); nothing is freed and no hook re-enters. `finish` is synchronous,
frees nothing (reaper runs next frame), and re-enters our
`createNextSequence` hook during the call — **no lock may be held across
it** (the gesture-buffer lock is released before the triggers run;
`score_guard` calls complete first). After `finish`, `advanceToScene`
synchronously moves `current_scene()` off 28, so the scene gate rejects
repeat presses; the dying-child dead-mask gate covers the reaper window.
The fallback path only writes actor flags and arms a redirect. The
diagnostic sampler calls the game's read-only readiness predicates on the
same frame thread the loader evaluates them on.

---

## 9. What stays, what changed

- The death-simulation machinery (`force_game_over`, actor offsets
  `+0x58/+0x1E8/+0x2B7/+0x2B8`, `gameplay_actor_vtable`) **stays as the
  fallback** — and doubles as the fast path's "gameplay is live" probe
  (`find_gameplay_actors` non-empty ⇒ the child at `TS+0x58` really is a
  live `DancePlaySequence`).
- `score_guard` semantics unchanged: restart clears the song taint, fail
  sets the quick-fail taint (per-song suppression + session logout taint).
  The fast fail skips `ResultSequence` entirely, so there is no per-stage
  save to suppress; the logout sanitise still covers the session
  write-back.
- No new detours. Two AOBs (`final_stage_probe`, `shutter_close_request`)
  + two derivations (`max_stage_global`, `shutter_actor_global`), all
  fail-closed. ShutterActor struct offsets (`+0x58`/`+0x82`/`+0x310`/
  `+0x314`) are range-validated at every use; the `0x100c` dismiss is
  verified by the synchronous state-7 read-back — any surprise on a future
  build degrades to the fallback, never to a limbo.
- TEMPORARY: the `diag_sample_loader_exit_gate` sampler — its first deploy
  confirmed the root cause (`gate=1`, `shutter=6`); kept one more deploy to
  observe the fast path, then remove.

## 10. Cabinet verification checklist

1. **Quick Restart mid-song (fast): no fade, no FAILED banner, no READY
   panel** — log `quick-restart (fast) -- finish(28₁ᵢₙdₑₓ)` preceded by
   `stage shutter dismissed (6 -> 7 drain)`; scene chain 28→27→28 with no
   intermediate stops; song restarts from zero; no residual audio; stage
   counter unchanged; repeat restarts stay stable.
2. **Quick Fail mid-song (fast): no banner, lands directly at song select**
   — log `quick-fail (fast) -- finish(25₁ᵢₙdₑₓ)`; scene chain 28→24→25;
   score not submitted; next song saves normally. The failed stage
   **replays** (no stage consumed).
3. Gesture during the READY panel (first ~2 s of gameplay, shutter states
   1–4): ~~expect the `shutter not in a fast-path state` log + the
   natural-death fallback, not a limbo~~ **WRONG — the fallback IS a limbo
   pre-song (cabinet-observed 2026-08-31). Superseded by §14: fail takes
   the fast path (states 4/5 now dismissible), restart is a no-op, the
   fallback refuses pre-song.** A gesture during a natural song end (DPS
   8/9, banner pending) still expects the `shutter not in a fast-path
   state` log + the natural-death fallback (harmless there — the natural
   tail is already running).
4. Quick Fail on the final stage: predicate log line + the full natural
   tail (banner + results + session end).
5. Quick Fail with Premium Free ON: fast path lands at song select, counter
   still frozen, records virginised on the song-select return as usual.
6. Course/Dan session: restart blocked, fail takes the natural tail.
7. Quick Restart × Song Playback Speed (non-100 %) and × Assist Tick: the
   loader → fresh-gameplay path re-registers the song banks; verify the
   committed rate binding / tick track resync after a fast restart.
   **Top-risk item.**
8. e-amusement card-scan state after a fast fail (the 0-idx 29 loader's
   `StartScanEAmusementPass` re-enable is skipped; the fast RESTART path's
   0x1c case body even STOPS scanning — the natural per-stage behavior):
   observe whether anything mid-session cares.
9. The `[diag]` line on each gesture: shutter fields now read
   `state/active/pending` — expect `6/3/-1` mid-song before the dismiss.

## 11. Address reference (20260721, file-relative 0x180000000)

| Symbol | Address | Note |
|---|---|---|
| `createNextSequence` | `0x18002e240` | case labels in the decompile are the raw 1-indexed ids; case 4 creates the ShutterActor as a TS child |
| `getNextID` | `0x18002dd70` | edges: `0x1C→0x1D` (stage loader → gameplay), `0x19→0x1A` (select loader → song select), `0x1D→0x1E` (gameplay → result loader — the fallback redirect interception point) |
| `LoadingSequence` ctor / onUpdate | `0x18002d130` / `0x18002d200` | ctor args (this, loadMask, unloadMask, minTime, kind); onUpdate = the gate chain in §4a.2 (gate A = the shutter mask-apply gate that caused the limbos) |
| **`ShutterActor`** ctor / update / msg handler | `0x180033600` / `0x180033a50` / `0x180034c60` | vtable `0x18035eba8`; state machine in §4a.3; **msg `0x100c` = the bannerless stage-panel dismiss** |
| shutter close-request wrapper | `0x1800334f0` | msg `0x1007` broadcast; kind 5 = FAILED banner. The `shutter_close_request` AOB (unique on 3 builds: 20250805 `0x1800337e0`, 20260616 `0x180034020`, 20260721 `0x1800334f0`); its `MOV RBX,[rip+d32]` at +9 derives `shutter_actor_global` |
| shutter force-out wrapper (msg `0x1008`) | `0x180033560` | DPS state 5 uses it to run the READY panel's `stage_out` (4→5→6) |
| `ShutterActor` singleton | `DAT_1806f2d40` | state at `+0x58 + (*(u16*)(+0x82))*8`; active kind `+0x310`; pending kind `+0x314` |
| loader exit gate (`FUN_180031af0`) | `0x180031af0` | "background/movie system ready" — cabinet-measured TRUE mid-song (not the blocker) |
| bg switch-settled term (`FUN_18003f590`) | `0x18003f590` | called on `bgObj+0x150` |
| bg movie-map-idle term (`FUN_18003fa20`) | `0x18003fa20` | called on `bgObj+0x348` / `bgObj+0x3f0`; busy = mgr state ∉ {0,5,6,8} |
| movie-frame clear (`FUN_1800319b0`) | `0x1800319b0` | `BgMovieActor+0x68 := empty`; run by loader kinds ≥ 1 setup |
| movie-frame clear + 0x1006 broadcast | `0x1800318c0` | run from 4 `createNextSequence` case bodies + TS setup |
| `BgMovieActor` singleton | `DAT_1806f2d30` | `+0x58` bg object, `+0x68` `MoviePlayerFrame` (ready byte `frame+0xc0`) |
| movie manager | `DAT_1806f2f48` | per-slot load state ints (stride 0x40, state at `+0x20`) |
| scene resource manager | `DAT_1806f2d68` | `+0x24` async-load-in-progress byte |
| update broadcast helper | `0x18022eaa0` | msg recursion over `+0x18`/`+0x10` links |
| `agcs::Sequence::onMessage` | `0x18021e070` | `0x102` ticks self; `0x201` clears `+0x58` then vt`+0x48` (advanceToScene on a TS); `0x202` sets `+0x58` and adopts the passed child |
| `agcs::Actor::onMessage` | `0x18021dc70` | lifecycle flag semantics (incl. the `0x109` skip-one-update); dispatches vt`+0x40` custom handlers first |
| destruction reaper | `0x18022ebe0` | veto slot vt`+0x08`, pre-delete vt`+0x10` |
| `WaitSequence` ctor / update | `0x18002d060` / `0x18002d0f0` | update polls `arkGetCurrentMode ∈ {3,6}` — v1 fail-target stall |
| `DancePlaySequence` ctor / vtable | `0x180057150` / `0x180360ab8` | |
| `DancePlaySequence::onSetup` | `0x180057480` | registers `<song>.xsb/.xwb`, builds HUD |
| `DancePlaySequence::update` | `0x180057ec0` | state 1 = the shutter entry gate (`4`, or idle-`0` with no pending); state 5 sends `0x1008`; state 8 = the banner request |
| `DancePlaySequence::leave` | `0x1800590b0` | stops song (`+0x128` handle), releases bank handles |
| song play / stop by bank | `0x1801aa5c0` / `0x1801aa7c0` | slot 5 = per-song bank |
| `FUN_1801dd660` (probe host) | `0x1801dd667` (match) | see §6.1 for the byte map |
| max-stage read site | `0x18002e903` | see §6.2 |
| `arkGetCurrentMode` resolver slot | `DAT_1806f2420` | name bytes at `0x1802df278`; also polled by `TransitionSequence::update` (`0x18002d7d0`) as the test-menu/mode watchdog (mode change ∉ {3,6} ⇒ scene-1 reset broadcast) |
| R19 trampoline (805 MODIFIED) | `0x1802b8b80` | §2 decode |
| alternate state-pair block (805) | `0x1800b5883` | stock code the trampoline jumps into |

## 12. Post-fast-path latency (2026-08-12 cabinet measurements)

The v4.1 fast paths work but still show multi-second waits. Log-timestamp
breakdown:

| Phase | Measured | Verdict |
|---|---|---|
| shutter dismiss + drain + stage loader (restart) | **< 1 s** (`finish` → `28→27` → `27→28` inside one log second) | done — not worth further work |
| fresh `DancePlaySequence` init (restart) | **~6 s** (scene 28 entry → song start; the user watches the lane/guideline build raw — the natural flow shows the same cost behind the READY panel) | the remaining restart cost; see below |
| select loader (fail) | **5 s** (`28→24` 13:28:40 → `24→25` 13:28:45) | fixed by the select-residency patch |

Repeat-restart observation: after a fast restart the shutter reads
`0/-1/-1` (no jacket panel was ever re-created), so subsequent restarts
skip the dismiss entirely.

### 12.1 The select-residency patch (fail: 5 s → ~loader minimum)

The same 0-idx 24 loader took **< 1 s at boot** (CAUTION→24→25 both at
13:19:58) but 5 s post-gameplay. Difference: at boot the attract chain had
left the select-music packages resident; gameplay entry **evicts them** —
case 0x1c's `unload=0x32000` ⊇ the select-music mask `0x2000` — so every
gameplay → song-select hop re-loads them from disk. Fix: byte-patch the
unload imm32 `0x32000 → 0x30000` (`gameplay_loader_masks` AOB —
`BA 00 80 00 00 41 B8 00 20 03 00`, the load-imm pins it against the course
loader's identical unload; unique on all three builds: 20250805
`0x18002fabb`, 20260616 `0x1800301a0`, 20260721 `0x18002fc0b`). Applied
one-way at mod enable, checked + fail-open. Side benefit: the NATURAL
post-results return to song select gets the same ~5 s back. Cost: the
select packages stay in memory during gameplay (`scene_result` `0x10000` +
`0x20000` are still evicted) — a non-issue on the target host.

### 12.2 Where restart's ~6 s lives + the path to instant (Training Mode)

The per-frame init sampler (2026-08-12 cabinet run) settled it exactly:

```
step -1 -> 0 at 0.02s   loader hop → DPS installed (instant)
step  0 -> 1 at 1.03s   ~1.0s: wait for layout actor (state 0)
step  1 -> 2 at 1.04s   note-field actors built (instant)
step  2 -> 3 at 1.05s   dance_root movie layer
step  3 -> 4 at 1.16s   readiness poll (fast)
step  4 -> 5 at 1.16s   song bank REGISTERED + PREPARED (fast — not I/O-bound!)
step  5 -> 6 at 5.04s   ← 3.9s of pure dead waiting
step  6 -> 7 at 5.04s   song starts
```

So the bank prepare is NOT the bottleneck (done by 1.16 s). State 5 then
spins until the DPS's own elapsed-time counter (`DPS+0x130`, accumulated
every frame from DPS creation) reaches the hardcoded **5.0 s** ready-dwell
`DAT_18035a8b4` (`0x40A00000`; verified — the 5.04 s landing = 5.0 + the
0.02 s DPS-creation offset). That dwell is the "READY?" countdown, and on a
fast restart the jacket panel is already dismissed, so it is pure dead air.

**Ready-dwell skip (shipped).** `DAT_18035a8b4` has 13 readers (a shared
5.0 float), so it can't be patched globally. Instead the restart driver
(the same per-frame `run_on_render_thread` closure that hosts the sampler)
seeds `DPS+0x130` to a large value (1000.0) every frame while the fresh DPS
is in its pre-song init states (0..=5). State 5's gate
(`5.0 <= DPS+0x130`) then clears the instant the OTHER state-5 condition —
the bank-prepared check — holds (~1.2 s), so nothing starts before the
song is actually ready. Writing a value far above any plausible threshold
(rather than a fixed just-over-5.0) avoids clamp-deadlock if a future
build raised the dwell. `+0x130` sits inside the 0x138-byte DPS allocation
and has no other known reader; the write is range/scene-gated
(GAMEPLAY + step ∈ 0..=5). Expected result: restart ~5 s → ~1.2 s.

Remaining ~1.0 s = state 0 (wait for the layout actor's arc). Left for a
later pass; likely a real load, and small next to the win above.

**In-place reset (the real endgame + Training Mode foundation):** never
kill the DPS — rewind the run. The engine hands us most primitives:
**audio** = stop the cue (`FUN_1801aa7c0(handle @ DPS+0x128)`) + replay by
bank name (`FUN_1801aa5c0(5, name)` — bank stays registered, zero disk
I/O); **clock** = re-broadcast msg `0x1044` with a fresh QPC tick to the
DPS subtree (the engine's own timing-anchor, sent by DPS state 6; msg
`0x1043` re-arms the start/input protocol); **ready-dwell** = the
`DPS+0x130` seed above. The open RE surface is **run state**: per-note
consumed/judged flags + per-side judge cursors (in the 0x11e0 chart holder
built by `FUN_1801c7a40`), score/gauge/combo accumulators on the
`GamePlayActor`, judge/combo UI actors, and the modpack's own per-song
subscribers (assist-tick track re-sync, song-rate binding, PUS
accumulators, score_guard taint). Sectioned start points / loops for
Training Mode fall out of the same machinery (anchor to `t ≠ 0`).

## 13. "Skip results" toggle — direct-to-results fast path rejected (2026-08-20)

The `skip_results_fast_exit` player option (Mods-tab bool, default ON)
lets a quick fail show the stage results screen instead of the instant
cut. The obvious "ideal" mechanism — dismiss the shutter and
`finish(DPS, 0x1E)` into the 0-idx 29 result loader, the natural
successor of gameplay (`getNextID(0x1D) = 0x1E`) — was RE-investigated
and **rejected**:

- **The record would be virgin.** The per-stage play record that
  `ResultSequence` displays is written by the **result commit** — a vfunc
  at GamePlayActor vtable **+0x28** (20260721: `FUN_18005d970`, vtable @
  `0x180360d68`; 20260526: `FUN_18005d180`, vtable @ `0x18035fd68`) that
  copies the actor's live judge counters (`+0x194/+0x19C/+0x1A0..+0x1BC`),
  score cluster (`+0x1D4..+0x1E0`), grade/clearkind decision, radar,
  ghost trail and end-time into `PlayerWork + 0x590 + stage*0x2B8`. It
  only runs from the natural song-end machinery. The song-select commit
  *zeroes* the record at selection
  (`docs/premium_free_stale_record_bug.md`), so jumping to the loader
  mid-song renders an all-zero results screen — defeating the feature.
- **Replicating the natural machinery is half of DPS's song-end state.**
  The "all actors done" block of `DancePlaySequence::update`
  (`FUN_180057ec0`) also performs the stage bump (`GameWork+0xC` INC @
  `0x180058c29`, guarded by `GameWork+0x59`/`+0x5A`), the msg `0x1053`
  broadcast, per-actor `+0x210` sub-object calls, and the song stop
  (`FUN_1801aa7c0`); the commit itself calls an `"MDX1529"` error handler
  when total judges == 0 (a quick fail before the first judged note).
  Every hand-replicated piece needs per-build cabinet validation, and the
  known failure mode of this machinery is a hard limbo (§4a).

The shipped OFF path therefore reuses the **natural fail flow** —
`fail_song(None)`: `force_game_over` on every GamePlayActor → 0.25 s fade
→ FAILED banner → the 0-idx 29 loader → `ResultSequence` with the true
partial score → the game's own natural tail (which makes the session-over
decision itself, so no predicate is needed). The pressing side's option
value governs (the fail is still cabinet-wide); the quick-fail score
taint applies in both modes (the results *display* reads live state and
is unaffected by save suppression). Design record:
`.agents/planning/2026-08-20-skip-results-toggle/design.md`.

## 14. The pre-song READY-window soft lock (2026-08-31)

**Symptom (cabinet-observed):** pressing 3 (quick fail) while the "READY?"
jacket panel is still on-screen — after the stage loader finishes but
before the arrows start scrolling — soft-locks the game (only the Test
Menu button breaks out). Code analysis confirmed quick restart (press 1)
takes the *identical* path in that window and locks the same way.

**Root cause:** §10's checklist item 3 assumed the natural-death fallback
was safe "during the READY panel" — it is not. In that window DPS is in
its **pre-song init states 0..=6** (layout → actors → bank register →
bank-prepare wait + ready-dwell → timing anchor); the shutter is at
state 4 (covered) or 5 (`stage_out` reveal in flight), so
`ensure_shutter_dismissed` (which then accepted only 0/6) refused and
both gestures fell back to `fail_song` → `force_game_over`. But the
death flags (`+0x1E8` etc.) are only consulted by DPS's **in-song**
machinery (state 7 → 8): with the GamePlayActors force-killed mid-init,
DPS never reaches its state-8 banner request, nothing ever drains the
shutter, and the scene parks forever — the same limbo class as §4a, now
from the fallback side. (Restart additionally never reached the in-place
reset: `song_reset` refuses pre-song by its own "DPS in-song" gate.)

**Fix (shipped 2026-08-31):**

1. **`0x100c` works from states 4/5 too.** The dismiss handler
   (§4a.3, `FUN_180034c60`) checks only `active kind == 3` — it never
   reads the current state — and state 7's drain "replays `stage_out` if
   it never ran" before the out label, so a dismiss from 4 (covered) or
   5 (revealing) lands in the same verified 7→8→0 idle park as the
   mid-song 6. `ensure_shutter_dismissed` now accepts
   `state ∈ {4,5,6} + active 3 + pending −1` (art-load states 1–3 and
   any pending banner still refuse). Mid-song this is a no-op (the
   shutter is always 6 there); the new states only occur pre-song.
2. **Quick fail pre-song = fast path only.** `trigger_fail` detects the
   window (`dps_pre_song()`: the DPS StackStep read the init sampler
   already used, `+0x68`/`+0x92`, range-validated; unreadable ⇒ treated
   as in-song so mid-song behavior is unchanged on layout drift) and
   takes ONLY the `finish(DPS, 0x19)` fast exit (still behind the
   session-continues predicate). Any refusal ⇒ the gesture is **ignored**
   — never the fallback. The quick-fail taint is set only on success
   (setting it up front would suppress the score of the song about to
   play on a refused gesture, and `reset_song_taint` can't be used to
   undo it — it collaterally clears training taints). `skip_results` is
   moot pre-song (no score exists to show), so the fast exit runs
   regardless of the pressing side's preference.
3. **Quick restart pre-song = no-op.** The song hasn't started; a
   restart is semantically nothing, and no restart shape is safe there
   (in-place reset refuses, a mid-init fresh-DPS `finish` reload is
   unvalidated, the fallback locks).
4. **Structural backstop:** `fail_song` itself refuses when
   `dps_pre_song()` — no future call path can reintroduce the lock.

**Cabinet verification items:**

- Press 3 during READY (session mid-stream): expect
  `stage shutter dismissed (4 -> 7 drain)` (or `5 -> 7`) +
  `quick-fail (pre-song fast) -- finish(25₁ᵢₙdₑₓ)`, landing at song
  select with no limbo; stage not consumed; next song plays normally.
  Watch specifically for the old DPS's teardown racing the in-flight
  song-bank register/prepare (DPS states 4/5) — the select loader's
  gates B/C should absorb the async settles, but this is the untested
  half of the pre-song `finish`.
- Press 3 during READY on the final/extra stage: predicate refusal ⇒
  `quick-fail ignored during the pre-song READY window` and the song
  proceeds normally (no taint applied).
- Press 1 during READY: `restart ignored during the pre-song READY
  window`, song proceeds normally.
- Mid-song behavior unchanged (dismiss still logs `6 -> 7`).

### 14.1 The REAL observed limbo: the drain's state-8 wait (2026-08-31, local)

The first fix deploy proved the user's repro was NOT the pre-song window:
the press landed ~1 s *after* `on audio play` (DPS already in-song, shutter
at the normal `6/3/-1`), the dismiss verified `6 → 7`, `finish` ran,
gameplay tore down cleanly — and the post-`finish` watchdog then showed the
shutter parked at **state 8 for 20 s** with every other loader gate green
(`gate=1, mgr_loading=0`). A press 20 s into the song on the same install
drained in 0.03 s. (The "READY banner still visible" was the jacket panel's
`stage_out` reveal play failing — see below — leaving the panel art
on-screen after the song had already started.)

Decompile of the update's case 8 (`FUN_180033a50`):

```c
get_param(mc, 0x1012, "out_end", &a);   // label → frame (0 on miss)
get_param(mc, 0x1012, "end",     &b);   // DAT_18035dff0 = "end"
target = max(a, b);
get_param(mc, 0x1010, &current);
if (current < target) break;            // wait
// else: release layer, active = -1, state = 0
```

and case 7 unconditionally advances to 8 after playing label `"out"`
(`DAT_18035db90`) — the ONLY thing that moves the clip during the drain.

On this install the `shutter_play` clip has **no labels at all** (afp-access
warns: `in`, `stage_out`, `ready_out`, `out`, `out_end` all missing; only
`end` resolves). So: `"out"` play fails → clip never advances; target =
`frame("end")` > 0. Mid-song the clip already sits at its final frame
(masked), but early in the song it is still near frame 0 → state 8 waits
forever → loader gate A never opens → limbo. Timing-dependent, not
state-machine-dependent — which is why every mid-song test passed and every
early press hung.

**Fix:** `unblock_shutter_drain` — after the verified `0x100c` dismiss,
compute state 8's own target (`max(frame("out_end"), frame("end"))` via the
same label queries) and `SetFrame` (`afp_mc_op` 0xF08) the clip there
(layer obj at `shutter+0x88+kind*0x10`, mc id at `layer+0x110`). On
label-less art this satisfies the wait directly; on healthy art state 7's
`"out"` play re-seeks the playhead anyway, so the write is invisible and
the stock sub-second out animation still runs. Fail-open (missing layer /
mc id / both labels ⇒ stock behavior; both-absent means the wait threshold
is 0 and passes anyway).

Open question: WHY this install's `shutter_play` lacks labels (stock data,
no shutter art in data_mods) — possibly a libafp/label-table parsing
difference under this data version. The unblock is correct regardless.

**VALIDATED 2026-08-31 (local CrossOver install):** with
`unblock_shutter_drain` in place, quick fail AND quick restart succeed at
every timing — including presses while the READY/jacket panel art is still
on-screen — with no limbo. The post-`finish` watchdog is kept permanently
in silent mode: no per-second sampling; on a 20 s timeout it emits ONE gate
sample (`diag_sample_loader_exit_gate`, still 20260721-RVA-guarded) + a
LIMBO WARN, so any future limbo self-diagnoses from a single log. The
gesture-time diag calls were removed (superseded by the watchdog).
