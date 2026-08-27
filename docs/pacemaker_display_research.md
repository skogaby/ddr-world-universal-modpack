# Pacemaker Display (dance_score_compare) — Visibility & Outro-Latch Research

RE notes for two PowerUserStatistics `pacemaker_to_mserror` bug fixes
(2026-08-18): (1) the pacemaker readout dying across in-place song resets
(instant restart / Training Mode SONG LOOP), and (2) the readout only
showing when ghost/rival-target data exists.

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
