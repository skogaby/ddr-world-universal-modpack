# Pacemaker Display (dance_score_compare) — Visibility, Outro-Latch & White-Zone Research

RE notes for three PowerUserStatistics `pacemaker_to_mserror` bug fixes:
2026-08-18: (1) the pacemaker readout dying across in-place song resets
(instant restart / Training Mode SONG LOOP), and (2) the readout only
showing when ghost/rival-target data exists. 2026-08-31: (3) the white
zone blanking the displayed number (port deviation from the original
hex-edit mod) — see §7.

All addresses are file-relative to `gamemdx.dll`'s `0x180000000` base,
from **20260721** unless noted. Layout attestations cross-checked on
20260526 / 20260616 / 20260721 (see §5).

## 1. Cast of actors

The pacemaker readout (the ± score-delta digits the ms-error swap
repurposes) is the **`dance_score_compare`** CMovieClip, owned by
**`sequence::dance::NoteResultActor`** (0x110 bytes, ctor `0x18007a450`,
onSetup `0x18007a630`, onMessage `0x18007b300`) — the judge-display child
of each side's GamePlayActor (created in GamePlayActor onSetup
`0x18005be90`).

NoteResultActor layout (relevant fields):

| Offset | Field |
|---|---|
| +0x88 | side-config ptr (deref +0x00 = play side; the 0x1036 side gate) |
| +0xA0 | `dance_judge` clip |
| +0xA8 | `dance_fast_slow` clip |
| +0xB0 | **`dance_score_compare` clip wrapper** (created unconditionally in onSetup, except UI mode `*(int*)(*DAT_1806f14f8+0x1C) == 10`) |
| +0xB8 | AFP package ptr (sign-bitmap swap source) |
| +0xC0 | **visibility byte** — ctor writes 0 (`88 99 C0 00 00 00`); see §3 |
| +0xC8..+0xD0 | freeze-judge clip vector |
| +0xE8..+0xF0 | dance_effect clip vector |

CMovieClip wrapper (0x240-byte pool slots at `DAT_1806fa600`): layer id
at **+0x08**, MovieClip id at **+0x110** (the engine's own SetFrame in
case 0x1032 reads it there), name at +0x114.

## 2. Value pipeline — unconditional

`FUN_180060340(gamePlayActor, playhead)` (tail of every `judgeNotes`
call) is the ghost/pacemaker score-target updater:

1. Counts the judged-record prefix (records in `[+0xB0,+0xB8)`,
   `judgedAt >= 0 && judgedAt <= playhead`, stride 0x40).
2. If the count differs from the cache at **GamePlayActor+0x200**:
   computes the ghost target from the GhostActor's grade-history byte
   vector (`+0x1F8` child, vector at its +0x98..+0xA0; money or EX per
   the mode flag at +0x1D0), delta = own score − target, and broadcasts
   **`0x1036 {side, score, delta}`** to the subtree; cache ← count.
3. With NO GhostActor / empty history the broadcast still fires (target
   0). The value pipeline never gates on ghost presence — only the
   RENDER does (§3, §4).

Post-reset self-heal: the judge-record rebuild leaves count 0 ≠ stale
cache → one broadcast, cache 0, then normal per-step operation. The
+0x200 cache needs no reset-time handling.

## 3. Visibility gate (bug 2)

Case `0x1036` of the NoteResultActor handler (`0x18007b300`) re-applies
`set_visible(byte@+0xC0)` on EVERY dispatch (via `FUN_18026ee30` =
`afp_layer_play(id, 1.0)` + `afp_layer_set_attribute(id, 1, byte)`).

The byte is 0 from the ctor. The ONLY stock writer of 1 is
**`sequence::dance::GhostActor::onUpdate`** (`FUN_180056d90`; actor
created near the end of GamePlayActor onSetup, stored at
GamePlayActor+0x1F8, holds the NoteResultActor ptr at its +0x88): state
1's download-poll success path fills the grade-history vector
(`FUN_18001e140`) and writes `noteResultActor+0xC0 = 1`. Download empty
or failed ⇒ byte stays 0 ⇒ the 0x1036 case runs per judged step but
re-hides the clip every time.

**Fix (pacemaker_swap):** the swap stub (patched inside case 0x1036,
after all gates) now passes RDI (= the NoteResultActor) as a 3rd arg.
When `pacemaker_to_mserror` is ON for the dispatching side, the callback
writes the byte to 1 (guarded by `note_result_actor_vtable` RTTI match)
and re-asserts the clip layer's visibility attribute for the current
dispatch (the handler's own set-visible consumed the stale 0 just before
the patch site). Runs only while the byte is 0 ⇒ at most once per song
per side. Option OFF ⇒ fully stock.

## 4. Frame/outro gate (bug 1)

Case `0x1036` refuses whenever the clip's current frame
(`afp_mc_get_param 0x1010`) has reached the frame of label **"out"**
(`afp_mc_get_param 0x1012, "out"`). Case **`0x103A`** (the pacemaker
outro) jumps the clip to "out" via SetFrameLabel — a **one-way latch**
for the actor's lifetime.

`0x103A` senders (all "this run is over/dead" events):

| Sender | Trigger |
|---|---|
| `FUN_180074d90` (percent/flare/grade gauge update) | gauge value hits 0, non-instant-death (+0xD8 clear) → `0x103A` + own died-latch +0xB8; instant-death variant sends `0x103B` instead |
| `FUN_180070f70` (LIFE4/RISKY lives gauge) | lives hit 0 → `0x103A` + latch +0xB0 |
| `FUN_18005cce0` state 1 (GamePlayActor update, course only) | course carry-over target `playerObj+0x254 <= 0` at stage start |

Natural song flow destroys the NoteResultActor with the scene, so stock
never observes the latch. The **in-place reset** (`song_reset`) reuses
the actor: one gauge-empty moment in ANY earlier pass of the song leaves
the clip at/past "out", so the pacemaker (stock delta OR ms-error swap)
never renders again for that stage — every subsequent loop iteration /
instant restart inherits it. (Grinding a hard section with SONG LOOP's
death-bypass hits this constantly.)

**Fix (song_reset::reset_side_state):** on every reset/seek, locate the
NoteResultActor child by RTTI vtable and restore the clip to the exact
song-start state its onSetup produces: `afp_mc_op(mcId, 0xF08 /*SetFrame*/, 0)`
+ `afp_layer_play(layerId, 0.0)` (paused at frame 0). The next judged
step's 0x1036 then replays it exactly like the first judge of a fresh
song. Fail-open: unresolved vtable / null clip / invalid ids skip the
rewind only.

## 5. Cross-build attestation

- NoteResultActor ctor's `+0xC0 = 0` byte write (`88 99 C0 00 00 00`,
  also pinning the +0xA0/+0xA8/+0xB0/+0xB8 zeroing run immediately
  before it): unique on 20260526 (`0x1800794ff`), 20260616
  (`0x18007a0ef`), 20260721 (`0x18007a4ef`).
- `.?AVNoteResultActor@dance@sequence@@` RTTI present (vtable resolved
  by the same `find_vtable_by_rtti` path as the gauge/Score/CMA set).
- The 0x1036 case layout (+0xB0 clip / +0x88 side / digit format path)
  matches the 20260421 notes in
  `.agents/planning/20260523-bulk-hack-porting/research/per-step-data-feed.md`
  and `docs/gameplay_overlay_elements_research.md` §NoteResultActor.

## 6. Non-findings / rejected approaches

- GhostActor handles NO messages (its onMessage is the Actor default) —
  the reset's 0x1043/0x1044 broadcasts cannot disturb its state.
- The GamePlayActor+0x200 judged-count cache self-heals (§2); resetting
  it is unnecessary.
- Forcing visibility from a scene callback was rejected: the
  NoteResultActor is created asynchronously (DPS state 1) and the ghost
  download completes mid-run — the per-dispatch stub write is the only
  spot that is both after creation and authoritative.

## 7. White-zone color decoupling (bug 3, 2026-08-31)

### The port deviation

Inside case 0x1036, ONE register carries the delta to two consumers:

```text
0x18007b432  MOVSXD RSI,[R14+8]           ; ← the 11-byte swap patch site
0x18007b436  MOV    RDX,[RDI+0xB0]
0x18007b43d  MOVD   XMM0,ESI              ; → |value| → digit formatter FUN_1801ae0a0
...
0x18007b48b  TEST   ESI,ESI               ; → color branch
0x18007b48d  JNZ    colored
```

The original hex-edit mod forced ZF at the TEST site while ESI held the
real ms error — real digits, white color. The first port instead
returned 0 from the stub callback for `|error| < threshold`, which also
fed 0 to the digit formatter: **the whole white zone displayed the
value 0, not the real error** (tester-reported as "no number shows").

### The color branch (byte-identical shape on 20250805/20260616/20260721/20260825)

`SetColor` = clip-wrapper vtable **+0x90** (the same slot
overlay_element_styling's color_hook detours). Three paths:

| path | condition | components |
|---|---|---|
| white | `ESI == 0` | all four from `[rip] → 1.0f` (`0x180359eb8` on 20260721) |
| positive | `ESI > 0` | 1.0 / **0.5** / **0.5** / 1.0 |
| negative | `ESI < 0` | 1.0 / 1.0 / **0.5** / **0.5** |

Both colored paths source their 0.5 component from ONE shared constant
via `MOVSS xmm,[rip+disp32]` (`F3 0F 10 15` @ patch+0x8A and
`F3 0F 10 1D` @ patch+0x9D on 20260721; same offsets ±0 on all four
builds). The 1.0 loads and the pow-base 10.0 load are the only other
rip-form MOVSS loads within 0x100 bytes of the patch site.

### Fix (pacemaker_swap, cull_window-style disp32 redirect)

At enable, `install_color_patch` scans `[patch+11, patch+0x10B)` for
rip-form `MOVSS` loads whose target reads **exactly 0.5f**
(content-identified — register allocation is unverified on future
builds, the loaded value cannot change), requires EXACTLY two, and
redirects both disp32s at a mod-owned f32 co-located in the stub's
`alloc_near` block. The stub callback then always returns the REAL ms
error and writes the slot per dispatch: `1.0` (⇒ both colored paths
degenerate to (1,1,1,1), the white branch's exact SetColor) inside the
white zone, `0.5` (stock) otherwise and on every option-off/early-return
path. Sign choice + sign-slot placement read the real ESI, so a
white-zone readout is correct digits + correct sign in white. Disable
restores both displacements. Fail-open: derivation failure ⇒ one WARN +
the legacy value-zeroing behavior.

The `pacemaker_threshold` option's clamp was extended to 0..50
(0 = white zone disabled, always colored).

### The "exact 0 renders no digit" mystery (RESOLVED 2026-09-01)

Symptom: autoplay (true 0 ms errors) showed the `dascco_plusminus` sign
but no digit. Static analysis said a lone "0" SHOULD render — and it was
right about the image on disk. The running process differed.

**Root cause: the real-speed-fix mod's ported "logf guard" (R15/R16)
was patching THIS function.** The original hex-edit modpack's research
notes attributed R15/R16 to "the scroll-speed display function"; that
attribution was wrong. The anchor AOB
(`0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6` — `movaps xmm0,xmm7; call
log10f; addss xmm0,xmm6`) matches EXACTLY ONE site on every attested
build (20250805 `0x180077be6`, 20260616 `0x18007b0e6`, 20260721
`0x18007b4e6`, 20260825 `0x18007b8f6`) — and that site is the log10f
call inside `NoteResultActor::onMessage` case 0x1036, i.e. the
pacemaker readout itself (on 20250805 the anchor sits at the
documented R16 VA `0x180077bea`, inside `FUN_180077a00` = this
handler; there is no separate scroll-speed match).

The R15 patch (single byte `0x48 → 0x37` at anchor−0x38) rewrote the
zero branch's `LEA R13D,[RSI+1]; JMP +0x48` (skip the log path — R13D
already holds the digit count 1) into `JMP +0x37`, which lands ON the
log10f call sequence. Exact-0 dispatches then recomputed
`R13D = trunc(guarded_log(0) + XMM6)` — and XMM6 is only loaded (with
1.0f) in the NONZERO branch, so the zero branch consumed the caller's
stale XMM6. Observed at runtime: `R13D = 0` → sign slot =
`powf(10, 0) = 1` → `"00000001_usr"` = the ONES slot — the same slot
the digit formatter had just written `dascco_0` into. The sign loop
runs after the formatter, so the `±` overwrote the `0`.

Evidence chain (2026-09-01 live session, 20260721):

1. Visual localiser (v3 diagnostic): forcing `dascco_8` onto the ones
   slot replaced the `±`; forcing it onto the tens slot showed `8 ±` —
   the sign provably rendered on the ONES slot for value 0.
2. CE non-breaking register captures: at the zero-branch `LEA`
   (`+0x7B4A9`) RSI=0 as expected; at the sprintf (`+0x7B525`)
   **R13=0, R9D=1** — impossible from the static code (only a JMP sits
   between the LEA setting R13D=1 and the powf), proving the control
   flow had been altered.
3. Runtime byte read at `+0x7B4AD`: `EB 37` where the static image has
   `EB 48` — `0x37` is `logf_stub.rs`'s `R15_PATCHED` constant.

Fix: the logf guard (`logf_stub.rs` + the `real_speed_logf_anchor`
signature) was retired outright. It was never part of the Real Speed
math (that is the independent R24/R25/R26 Core-BPM divisor swap at
`real_speed_bpm_anchor`, byte-identical shape verified on all four
builds), and it protects nothing in stock flow: the nonzero branch only
reaches log10f with |v| ≥ 1, and the stock zero branch never calls it.
With the mispatch gone, stock behavior is correct by construction:
`R13D = 1 → powf(10,1) = 10 → "00000010_usr"` — sign at tens, digit at
ones, matching the stock `±0` reference capture.

Corollary of the mechanism: while the mispatch was live, exact-0 was
the ONLY affected value (nonzero single digits take the nonzero branch,
which loads XMM6 = 1.0 before its log10f → `R13D = trunc(log10(v)+1)` =
correct), which is why `±30`/`-10`/single digits all rendered fine.
