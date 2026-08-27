# Quick Restart — Scene-Transition Trigger Research

## Overview

Quick Restart fires a triple-press of NUM_1 on either pinpad during scene 28
(GAMEPLAY) and re-enters scene 28 with a fresh `DancePlaySequence` actor — the
same song restarts from the beginning, the stage counter does **not** advance,
and the results screen is not visited.

Goal of this research: identify the cleanest mechanism for the hook DLL to
trigger that fresh re-entry, verified on both 20250805 stock and 20260421.
Cross-version verification was done against `gamemdx.dll` (20260421) and
`gamemdx_20250805_MODIFIED.dll`. The MODIFIED build's only relevant
divergence from stock 20250805 is in the loader-installed prologue of
`createNextSequence` and the R19 patch site — neither affects the
findings below; the dispatch body, `FUN_18002de40` (the explicit-scene
helper), `FUN_18021c390` (the message-send-up wrapper), and the
`DancePlaySequence` constructor (`FUN_180053c60`) are byte-identical to
the stock 20250805 layout.

All file-relative addresses below are at base `0x180000000`. Each section
gives addresses for both 20250805 and 20260421; the AOB anchors are
verified to resolve uniquely on each.

## TL;DR (Recommended Approach)

**Direct 28 → 28 re-entry via `FUN_18002de40` (the "advance to explicit
scene" vtable slot of `TransitionSequence`).** Pseudocode the mod will run
on triple-NUM_1 detection inside scene 28:

```rust
// transition_sequence_ptr is captured in our existing
// scene_manager hook on createNextSequence (it's the `this` arg).
// scene_id_1indexed = 0x1d for GAMEPLAY (0-indexed scene 28).
unsafe { fun_18002de40(transition_sequence_ptr, 0x1d) };
```

`FUN_18002de40` (= `FUN_18002db20` on 20250805) does the entire teardown +
reconstruction sequence the canonical state machine uses: it calls
`createNextSequence(this, 0x1d)`, which triggers case `0x1d` in the
58-case switch — that allocates a fresh 0x138-byte `DancePlaySequence`
via `FUN_180057840`, then `FUN_18021c310` installs the new sequence at
`this + 0x58`, replacing the old one (the previous `DancePlaySequence`
is destroyed via shared_ptr decref). The 58-case switch contains no
"is current scene == target scene? short-circuit" check, so 28 → 28 is
handled identically to any cross-scene transition.

Bouncing through scene 26 (`SONG_TO_STAGE_INTERSTITIAL`) is **not**
suitable — scene 26's case body constructs a `SelectMusicTerminateSequence`,
not an interstitial-to-stage actor; entering it would tear down song
selection state we want to keep.

The R19 state-pair hijack (Quick Fail's mechanism) is unsuitable — it
exits gameplay through the failed/quit-out path that skips the results
screen and goes back to song select, not back into a fresh gameplay
attempt.

## Key Findings

### Q1: How does the game initiate a fresh `DancePlaySequence`?

**Mechanism: `FUN_18002de40(this, 1indexed_scene_id)`.**

Two callers wrap `createNextSequence` (= `FUN_18002e470` on 20260421,
`FUN_18002e140` on 20250805 stock — body region beginning at the
prologue that the MODIFIED 20250805 loader patched at `0x18002e137`):

| Function (20260421) | Function (20250805) | Role |
|---|---|---|
| `FUN_18002d9c0` | `FUN_18002d6a0` | Advance using `this + 0x68` (the actor's pending-scene slot, written by `createNextSequence`) |
| `FUN_18002de40` | `FUN_18002db20` | Advance to an **explicit** 1-indexed scene ID passed as `param_2` |

Both call `createNextSequence(this, scene_id)` then `FUN_18021c310`
(`FUN_180205650` on 20250805) to install the result at `this + 0x58`.

These two functions are **vtable slots 4 and 9** (offsets 0x20 and 0x48)
of the `TransitionSequence` vtable. The vtable on 20260421 starts at
`0x18035b588`:

```
+0x00  destructor
+0x08  agcs::Actor::?
+0x10  agcs::Actor::?
+0x18  message handler — FUN_18021c490
+0x20  FUN_18002d9c0   advance-using-pending
+0x28  FUN_18002db20   ?
+0x30  FUN_18002da00   ?
+0x38  FUN_180269700   ?
+0x40  FUN_18002dd00   ?
+0x48  FUN_18002de40   advance-to-explicit-scene  ← target
+0x50  0 (terminator)
```

The natural transition flow for an in-game actor that wants to leave
gameplay is to send message `0x201` upward via `FUN_18021c390`
(`FUN_1802056d0` on 20250805); that message is intercepted by
`FUN_18021c490` (the parent's `slot 3` message handler), which calls
`(*vtable)[9]` (offset 0x48) — i.e. `FUN_18002de40` — to do the actual
transition. So calling `FUN_18002de40` directly with the target scene ID
short-circuits one indirection but produces an **identical** transition.

#### Acquiring the `TransitionSequence` `this` pointer

The Rust hook does not need to AOB-scan a global to obtain the
`TransitionSequence` pointer. Our existing `services::scene_manager`
already hooks `createNextSequence` and receives the `TransitionSequence`
as `RCX` (param_1) on every call. Capturing that pointer once during
the first hook fire and stashing it gives the `this` argument needed
to call `FUN_18002de40` later. (Alternative path: the global
`RootSequence` — `DAT_1806ede40` on 20260421, `DAT_1806b5ab0` on
20250805 — holds a pointer at `+0x58` that walks down to the active
`TransitionSequence`, but that's unnecessary indirection given we have
direct access through the scene_manager hook.)

#### Disassembly — `FUN_18002de40` body (identical structure on 20250805 `FUN_18002db20`)

```
18002de40: MOV qword ptr [RSP + 0x8], RBX
18002de45: PUSH RDI
18002de46: SUB RSP, 0x20
18002de4a: MOV EBX, EDX                    ; target scene
18002de4c: MOV RDI, RCX                    ; this = TransitionSequence*
18002de4f: TEST EDX, EDX
18002de51: JNZ +7                          ; non-zero param_2 → use it
18002de53: CALL 0x18002dfa0                ; param_2 == 0 → query state machine for next scene
18002de58: MOV EBX, EAX
18002de5a: MOV EDX, EBX
18002de5c: MOV RCX, RDI
18002de5f: CALL 0x18002e470                ; createNextSequence(this, scene)
18002de64: TEST RAX, RAX
18002de67: JZ done                         ; case returned NULL → no-op
18002de69: MOV RDX, RAX
18002de6c: MOV RCX, RDI
18002de6f: CALL 0x18021c310                ; install new sequence at this+0x58
18002de74: MOV ECX, EBX                    ; (post-store)
18002de76: MOV dword ptr [RDI + 0x68], EBX ; this+0x68 = active scene
18002de79: ...
18002de83: JMP 0x1801b9a20                 ; tail-call (post-transition hook)
```

Note: passing `param_2 = 0` means "let the state machine decide" via
`FUN_18002dfa0` (the per-scene next-state lookup). For Quick Restart we
pass `0x1d` (1-indexed gameplay) explicitly — bypassing the state
machine — so the next-state lookup's normal `0x1d → 0x1e` (gameplay →
results) advancement is not consulted.

### Q2: What happens on scene 28 → scene 28?

**The dispatch is a flat switch with no "current == target" short-circuit.**
`createNextSequence` (`FUN_18002e470` on 20260421) decompiles to:

```c
switch(param_2) {  // 1-indexed scene id
  case 0x1d:       // 0-indexed 28 = GAMEPLAY
    ...
    if (*(longlong *)(lVar15 + 0x70) == 0) {
        // Path A: BGM not yet running
        plVar10 = (longlong *)FUN_1801b2c00(*(undefined4 *)(lVar15 + 0x18));
        puVar11 = (undefined8 *)FUN_180244b50(DAT_180462020, 0x138, 0);  // alloc 0x138 bytes
        if (puVar11 != NULL) {
            puVar13 = FUN_180057840(puVar11, song_path, &local_340, &local_330);
        }
    } else {
        // Path B: BGM already running
        local_58 = FUN_180244b50(DAT_180462020, 0x138, 0);  // alloc 0x138 bytes
        if (local_58 != NULL) {
            ...
            puVar13 = FUN_180057840(local_58, song_data, &local_340, &local_330);
        }
    }
    break;
  ...
}
return puVar13;
```

`FUN_180057840` (= `FUN_180053c60` on 20250805) is the
`sequence::dance::DancePlaySequence` constructor. Both paths
unconditionally allocate a fresh 0x138-byte object and run the
constructor. The previous `DancePlaySequence` at `(this+0x58)` is
released by `FUN_18021c310` (the install function) via shared_ptr
decref — its destructor runs, child actors get their destroy bits set
(via flags 0x4/0x8 in `agcs::Actor::flags`), and per-onUpdate-tick state
on the actor goes away with the object.

Cross-version verification:

| Symbol | 20250805 | 20260421 |
|---|---|---|
| `createNextSequence` body entry | `0x18002e140` (loader-redirected to `0x18002e137` in MODIFIED, body identical) | `0x18002e470` |
| `DancePlaySequence` constructor | `0x180053c60` | `0x180057840` |
| Fresh-alloc size in case 0x1d | 0x138 | 0x138 |
| Install function | `FUN_180205650` | `FUN_18021c310` |
| Slot for active child on TransitionSequence | `+0x58` | `+0x58` |

There is no "if current_scene == target_scene return early" branch in
either version of the dispatch. The two paths inside case 0x1d (BGM
not running vs running) both unconditionally allocate and construct.

### Q3: Per-step timing accumulator field locations

**The per-stage timing accumulator block lives on the per-player game
state struct, NOT on the `DancePlaySequence` actor.** It is therefore
not reset by the 28 → 28 actor swap.

The per-player game state pointers live in the global array
`DAT_1806edff0` on 20260421 (`DAT_1806b5ad0` on 20250805) — two slots,
indexed by player. Each slot points to a struct with multiple per-stage
data blocks at stride `0x2b8` starting at offset `+0x594`, indexed by
`*DAT_1806ec618 + 0xc` (the stage counter). Sample structural references
visible in `createNextSequence` and `FUN_18002dfa0`:

```c
// FUN_18002e470 case 0x35 (20260421)
*(undefined4 *)((longlong)&local_98 + lVar15 * 0x10 + 4) =
     *(undefined4 *)(lVar9 + 0x594 + (longlong)iVar7 * 0x2b8);
//                          ^^^^^                      ^^^^^^^
//                  base of per-stage block        stride per stage
//                  iVar7 = stage index (*DAT_1806ec618 + 0xc)
```

```c
// FUN_18002e470 case 0x1f (20260421) — STAGE_RESULT, reads per-stage clear-status
if (*(int *)(*(longlong *)(&DAT_1806edff0)[*(int *)(*DAT_1806ec618 + 8)] + 0x328) < 0xf) {
//                                                                       ^^^^^^
//                                                  some other clear-status field on per-player state
```

**Implication for Quick Restart:** Re-entering scene 28 with the same
stage counter means the new gameplay round writes into the SAME 0x2b8
per-stage block at `[per_player + 0x594 + stage_idx*0x2b8]`. The
initialization at the start of a new song's `DancePlaySequence` setup
(see `FUN_180057b70`, vtable slot 4 of DancePlaySequence — the
post-construction onUpdate that does song-name lookup, audio path
setup, child-actor creation) does NOT explicitly zero this block; it's
zeroed at a different point in the standard flow (likely on case
`0x1c` / `0x34` — the `STAGE_INDICATOR` entry that precedes gameplay).
Mod-side the cleanest defense is: if the user-visible end-of-song
"timing stats" / stat counters are seen accumulating across a
Quick Restart, write 0 over the active stage's
`[per_player + 0x594 + stage_idx*0x2b8 .. + 0x2b8]` block before
firing the transition.

**This is hypothesis-grade — needs deploy verification.** The 0x594 /
0x2b8 layout is read directly from the binary and matches in both
versions, but the precise field semantics (which fields hold ms-error
vs judgment counts vs clear-flags) was not exhaustively mapped because
the recommended path (Q5) includes an empirical test that will surface
this directly. If the live test shows accumulator pollution, return
here and add explicit field offsets.

### Q4: Is bouncing through scene 26 cleaner?

**No.** Scene 26 (0-indexed; `SONG_TO_STAGE_INTERSTITIAL` in our
`scenes.rs` table) maps to switch `case 0x1b` (1-indexed = 27). Its
body constructs a `SelectMusicTerminateSequence`, not an interstitial-
to-stage actor:

```c
case 0x1b:
    local_58 = FUN_180244b50(DAT_180462020, 0xa0, 0);
    if (local_58 == NULL) {
        puVar13 = NULL;
    } else {
        puVar13 = (undefined8 *)FUN_180112680(local_58, 0);
        // ^ FUN_180112680 = SelectMusicTerminateSequence::SelectMusicTerminateSequence
    }
    break;
```

`SelectMusicTerminateSequence` is the closing animation that runs after
the user's song selection completes — it tears down song-select state.
Re-entering it from inside gameplay would unnecessarily teardown
selection state we want to preserve, and the next-state lookup
(`FUN_18002dfa0`'s case `0x1b`) returns `0x1c` (= `STAGE_INDICATOR`
inter-stage), not back into gameplay — so the scene 26 → 27 → 28
chain would advance the stage counter through `case 0x20`/`0x37`'s
`*(int *)(*DAT_1806ec618 + 0xc) = *(int *)(*DAT_1806ec618 + 0xc) + 1`
in the canonical post-stage transition. That's the opposite of what
Quick Restart should do.

**Use case 0x1d (gameplay) directly.**

### Q5: Recommendation

**(a) Direct 28 → 28 transition** via `FUN_18002de40(transition_seq, 0x1d)`.

Rationale:
- The dispatch is a flat switch with no short-circuit, so 28 → 28
  re-enters case 0x1d and constructs a fresh `DancePlaySequence`
  identical to any other entry into gameplay.
- The previous `DancePlaySequence` at `(transition_seq + 0x58)` is
  cleanly destroyed by the install function (`FUN_18021c310` on
  20260421, `FUN_180205650` on 20250805) via shared_ptr decref —
  matching how every other scene transition tears down the outgoing
  actor.
- Per-stage accumulators on the per-player game state struct
  (`DAT_1806edff0[player] + 0x594 + stage_idx*0x2b8`) need a
  hypothesis-test on first deploy; if they pollute the second attempt's
  end-of-song stats, mod can zero them as a pre-step before firing the
  transition.
- The stage counter at `*DAT_1806ec618 + 0xc` is **not** advanced by
  case 0x1d — only by `case 0x20`/`0x37` (the post-stage transition
  cases). Quick Restart's direct jump to case 0x1d skips that bump,
  which is exactly the desired behavior.
- We get the `transition_seq` pointer from our existing
  `scene_manager.rs` hook (it's the `this` arg of every
  `createNextSequence` call). No additional AOB scan needed for the
  pointer itself.
- Two functions need AOB anchors: `FUN_18002de40` (the entry point we
  call) and one internal call site for sanity-check / deploy diagnostics.
  Both have unique anchors verified on both versions — see "AOB Anchors"
  below.

Quick Fail's R19 hijack (case-0x1c state-pair rewrite to 0x21/0x39) is
not appropriate for Quick Restart — that path takes the player out of
gameplay through the failed/quit-out scene chain. Reuse R19 for Quick
Fail (separate gesture, separate semantics).

## AOB Anchors

All anchors verified to resolve uniquely on both 20250805 (file:
`gamemdx_20250805_MODIFIED.dll` — body region byte-identical to stock
modulo R19 / R7 / loader patches that don't touch these sites) and
20260421.

### Anchor 1: `FUN_18002de40` — advance-to-explicit-scene entry

```
Pattern: 48 89 5C 24 08 57 48 83 EC 20 8B DA 48 8B F9 85 D2 75 07 E8
Disasm:  MOV [RSP+8], RBX                        ; canonical Win64 prologue
         PUSH RDI
         SUB RSP, 0x20                           ; small frame
         MOV EBX, EDX                            ; save target scene
         MOV RDI, RCX                            ; save this
         TEST EDX, EDX                           ; param_2 == 0 ?
         JNZ short +7
         CALL FUN_18002dfa0                      ; ← rel32 wildcarded
```

| Version | Address | Match count |
|---|---|---|
| 20250805 stock / MODIFIED (matches at same VA in both) | `0x18002db20` | 1 |
| 20260421 | `0x18002de40` | 1 |

The `E8` byte is the start of the rel32 call to `FUN_18002dfa0`; the
following 4 displacement bytes (not included in the anchor — pattern
ends at `E8`) change between versions.

The structural invariants the pattern pins:
- The 5-byte `48 89 5C 24 08` Win64 prologue saving RBX
- `PUSH RDI; SUB RSP, 0x20` — tight frame
- `MOV EBX, EDX; MOV RDI, RCX` — moving incoming args to non-volatiles
  in this order (param_2-then-param_1 swap)
- `TEST EDX, EDX; JNZ short +7` — the param_2-zero guard
- The leading `E8` of the call into the next-state-decision helper

These invariants follow the function's logic, not its surrounding
basic-block layout — they are stable across compiler tunings as long
as the Win64 ABI and the function's "default param_2 → consult state
machine" branch remain.

### Anchor 2: `FUN_18021c390` — message-send-up wrapper (sanity check / alternative path)

We are NOT planning to call this directly (we go through `FUN_18002de40`),
but documenting the anchor is useful for two reasons: (1) future
mods that want to fire transitions from arbitrary actor depths can use
this lower-level entry, and (2) Quick Fail (R19) hooks the same family
of transition mechanics — sharing this anchor keeps the transition-
related signature set coherent.

```
Pattern: 48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 59 08 48 8B F1 8B FA F6 43 20 20
Disasm:  MOV [RSP+8], RBX
         MOV [RSP+0x10], RSI
         PUSH RDI
         SUB RSP, 0x20
         MOV RBX, [RCX+8]                        ; RBX = parent
         MOV RSI, RCX                            ; RSI = this
         MOV EDI, EDX                            ; EDI = target_scene
         TEST byte ptr [RBX + 0x20], 0x20        ; check parent's destroy-flag
```

| Version | Address | Match count |
|---|---|---|
| 20250805 (MODIFIED) | `0x1802056d0` | 1 |
| 20260421 | `0x18021c390` | 1 |

Structural invariants pinned:
- 3 callee-saved `MOV [RSP+...], reg` saves (RBX, RSI, RDI)
- Tight 0x20-byte frame
- `MOV RBX, [RCX+8]` — reading parent pointer at `+8` of the actor
  struct (this is a stable layout invariant for the agcs::Actor base)
- The `TEST byte ptr [RBX + 0x20], 0x20` — checks the destroy flag
  byte at offset `+0x20` of the parent. Both `+8` (parent) and `+0x20`
  (flags) are agcs::Actor base-class invariants, very stable.

### Anchor 3: `createNextSequence` body entry (already in use)

This is `services::scene_manager`'s existing `scene_transition` signature.
We reuse it without modification:

```
Pattern: 48 8B C4 55 57 41 54 41 55 41 56 EB ?? E8 ?? ?? ?? ?? 48 81 EC 40 03 00 00
```

| Version | Address | Match count |
|---|---|---|
| 20250805 stock | `0x18002e140` | 1 |
| 20260421 | `0x18002e470` | 1 |

(Anchor verified on 20260421 in this session via the equivalent shorter
pattern; 20250805 stock match is documented in `docs/scene_manager_research.md`.)

## Recommended transition flow

```mermaid
flowchart TD
    A[scene 28: gameplay running] -->|triple NUM_1 detected| B[QuickRestartOrFailMod gesture handler]
    B --> C{Pre-step: clear per-stage block at\nDAT_1806edff0[player] + 0x594 + stage_idx*0x2b8\nfor each active player?\nDeploy-verify needed.}
    C -->|optional, on first deploy:\nzero block| D[Call FUN_18002de40 transition_seq, 0x1d]
    C -->|happy path:\nleave block alone| D
    D --> E[createNextSequence runs case 0x1d]
    E --> F[Allocate fresh 0x138-byte\nDancePlaySequence via FUN_180057840]
    F --> G[Install at transition_seq+0x58 via FUN_18021c310\nold DancePlaySequence destroyed via shared_ptr]
    G --> H[scene 28 fresh; stage counter unchanged]
```

## Open Questions / Follow-ups

1. **Per-stage accumulator pollution.** The per-stage block at
   `[per_player + 0x594 + stage_idx*0x2b8]` may or may not get
   re-zeroed by the new `DancePlaySequence`'s setup (vtable slot 4 =
   `FUN_180057b70` on 20260421 / `FUN_180053?? ` on 20250805). The
   reset is *probably* done by `case 0x1c` / `case 0x34`
   (`STAGE_INDICATOR`) which fires on the canonical pre-gameplay
   transition path that Quick Restart deliberately skips. **Action:**
   on first deploy, run a song that produces visible per-step stats
   (e.g. lots of greats), Quick-Restart it, finish the second attempt,
   inspect the end-of-song stats. If counters carried over, return here
   to identify the exact reset hook.

2. **`FUN_18002dfa0`'s next-state lookup.** Calling `FUN_18002de40` with
   an explicit `0x1d` bypasses `FUN_18002dfa0`. We rely on case 0x1d
   being self-contained (i.e. doesn't read state-flow flags that would
   normally be set by case 0x1c). Inspection of case 0x1d's decompile
   shows it reads `*DAT_1806ec618 + 0x18` (game mode), `*DAT_1806ec618
   + 0x70` (BGM running flag), and the song-data pointer at
   `[*DAT_1806ec618 + 0x70]` — none of these are written by case 0x1c.
   But two paths (BGM-on / BGM-off) exist and the runtime Quick Restart
   will hit Path B (BGM already running from the first attempt). This
   should be correct (Path B is what inter-stage transitions normally
   use) but is worth confirming on the deploy.

3. **Audio-state / BGM continuity.** Path B in case 0x1d reuses the
   currently-playing BGM. Quick Restart users likely want the song to
   restart from the beginning, not continue from where the failure
   happened. The fresh `DancePlaySequence` may or may not auto-rewind
   the audio cursor — needs deploy verification. If not, the mod can
   pause/seek-to-zero the BGM before firing the transition. (Cheap to
   add later if needed.)

4. **Cheat Engine validation of pointer captures.** The plan to capture
   `transition_seq` from the first `createNextSequence` hook fire is
   sound (the same `this` is passed every time), but if the
   `RootSequence` global ever swapped its child pointer, our cached
   `this` would dangle. A defensive check: re-fetch `(*DAT_1806ede40)
   + 0x58` (= `(*DAT_1806b5ab0) + 0x58` on 20250805) at gesture time
   to refresh. **Action:** wire this fallback in implementation;
   confirm on a multi-stage session via DebugView log.

5. **Two-player ms-error / step ring buffers (mod-allocated state in
   `binary_modpack_research.md §16`).** The pre-modded build's
   `0x1811d0000` ring buffer is mod-allocated, not game-allocated. Our
   port doesn't share that allocation, so it's unaffected by the actor
   swap. The PowerUserStatistics mod's per-step accumulators (when
   we port that) will be in our own state and we'll handle reset
   there. Not a Quick Restart concern.

## Cross-version anchor verification summary

| Symbol | Pattern | 20250805 | 20260421 |
|---|---|---|---|
| `FUN_18002de40` (advance-to-explicit-scene) | `48 89 5C 24 08 57 48 83 EC 20 8B DA 48 8B F9 85 D2 75 07 E8` | unique @ `0x18002db20` | unique @ `0x18002de40` |
| `FUN_18021c390` (msg-send-up) | `48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 59 08 48 8B F1 8B FA F6 43 20 20` | unique @ `0x1802056d0` | unique @ `0x18021c390` |
| `createNextSequence` (existing scene_transition signature) | `48 8B C4 55 57 41 54 41 55 41 56 EB ?? E8 ?? ?? ?? ?? 48 81 EC 40 03 00 00` | unique @ `0x18002e140` (stock) | unique @ `0x18002e470` |

Pattern hygiene rules used: opcodes / ModR/M / SIB never wildcarded;
RIP-relative displacements wildcarded (`?? ?? ?? ??`); short-jump
displacements that depend on basic-block layout wildcarded (`??`);
register encodings (R8D / R10 / etc.) preserved as fixed bytes since
they are the structural invariants that distinguish these functions
from generic prologues.
