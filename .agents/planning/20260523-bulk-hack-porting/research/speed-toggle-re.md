# Speed Toggle — RE Research

## Overview

The "Updated Speed Toggle" mod (binary modpack §13) changes the SPEED option
row's per-press step from the vanilla `±0.25×` to `±0.05×` (fine, no Start) /
`±0.50×` (coarse, Start held). This research investigates how vanilla DDR
World's SPEED row stores those step values and recommends a port strategy.

**Top-line conclusion:** The user's mental model is **WRONG**. Vanilla DDR
does NOT have a built-in coarse/fine differentiation for the SPEED row. The
row uses a custom per-frame input handler (`FUN_180056c20`, the
`ControlSpeedActor` per-frame update) that reads two specific button bits in
the player input state and dispatches to one of two virtual methods on an
`OptionHispeed` object. Each of those virtual methods has a single hardcoded
`±0x19` step immediate baked into its disassembly. There is **no separate
coarse step**, **no Start-held check**, **no `OptionElement<int>::step_fine /
step_coarse` field pair to overwrite** — the speed row is its own animal,
hand-rolled and unlike the standard `OptionElement<int>` rows the
custom_options framework already RE'd.

This is why the original modder cave-stubbed input detection at the
`±0x19` sites: it wasn't a hex-edit modder's path of least resistance — it
was the only way to add coarse/fine semantics, because the game itself
doesn't have them. We need to do the same in the hook DLL, just idiomatically.

The recommended port is **option (i)** from the question prompt: install
short detours at the two `add edx, 0x19` sites (or, equivalently, at the
`vtable[0x1a8]` / `vtable[0x1b0]` virtual methods themselves, since each is
called from exactly one site) and write the new step value into `EDX` based
on a Start-held check we perform ourselves. No memory write to
"override stored step fields" is possible because no such fields exist.

---

## Speed-Toggle Handler Disassembly

Two consecutive virtual methods on `OptionHispeed`, called from
`ControlSpeedActor::onUpdate` (`FUN_180056c20` on 20260421 / `FUN_180053040`
on 20250805 stock; 20250805 MODIFIED has the same offset shape).

### Speed-up (vtable slot at offset `+0x1a8`)

20260421, file-relative `0x1801e01a0`:

```text
1801e01a0  SUB  RSP, 0x28
1801e01a4  MOV  RAX, [RCX]                  ; load OptionHispeed vtable
1801e01a7  CALL [RAX + 0x210]               ; vtable[0x210]() -> int* (hispeed value)
1801e01ad  MOV  EDX, [RAX]                  ; current value (0x19..0x320)
1801e01af  CMP  EDX, 0x320                  ; 800 = 8.00x ceiling
1801e01b5  JGE  0x1801e01df                 ; out of range -> clamp branch
1801e01b7  ADD  EDX, 0x19                   ; +0x19 = +25 = +0.25x
1801e01ba  LEA  R8,  [0x180358754]          ; ptr to const 800
1801e01c1  LEA  RCX, [RSP + 0x30]           ; ptr to local containing new value
1801e01c6  CMP  EDX, 0x320
1801e01cc  MOV  [RSP + 0x30], EDX
1801e01d0  CMOVG RCX, R8                    ; on overflow, substitute the const-800 ptr
1801e01d4  MOV  ECX, [RCX]                  ; load whichever (new value OR clamp)
1801e01d6  MOV  [RAX], ECX                  ; commit back to hispeed
1801e01d8  MOV  AL, 0x1                     ; return true (changed)
1801e01da  ADD  RSP, 0x28
1801e01de  RET
1801e01df  MOV  [RAX], 0x320                ; saturate at 800
1801e01e5  XOR  AL, AL                      ; return false (no change)
1801e01e7  ADD  RSP, 0x28
1801e01eb  RET
```

Decompile:

```c
undefined8 FUN_1801e01a0(longlong *param_1) {
  int *piVar1, *piVar2;
  int local_res8[8];
  piVar1 = (int *)(**(code **)(*param_1 + 0x210))();
  if (*piVar1 < 800) {
    local_res8[0] = *piVar1 + 0x19;
    piVar2 = local_res8;
    if (800 < local_res8[0]) piVar2 = &DAT_180358754;
    *piVar1 = *piVar2;
    return 1;
  }
  *piVar1 = 800;
  return 0;
}
```

### Speed-down (vtable slot at offset `+0x1b0`)

20260421, file-relative `0x1801e01f0` — symmetric, same `0x19` magnitude:

```text
1801e01f0  SUB  RSP, 0x28
1801e01f4  MOV  RAX, [RCX]
1801e01f7  CALL [RAX + 0x210]
1801e01fd  MOV  EDX, [RAX]
1801e01ff  CMP  EDX, 0x19                   ; floor = 25 = 0.25x
1801e0202  JLE  0x1801e0229
1801e0204  ADD  EDX, -0x19
1801e0207  LEA  R8,  [0x180358750]          ; ptr to const 25
1801e020e  LEA  RCX, [RSP + 0x30]
1801e0213  CMP  EDX, 0x19
1801e0216  MOV  [RSP + 0x30], EDX
1801e021a  CMOVL RCX, R8
1801e021e  MOV  ECX, [RCX]
1801e0220  MOV  [RAX], ECX
1801e0222  MOV  AL, 0x1
1801e0224  ADD  RSP, 0x28
1801e0228  RET
1801e0229  MOV  [RAX], 0x19
1801e022f  XOR  AL, AL
1801e0231  ADD  RSP, 0x28
1801e0235  RET
```

The clamp constants `0x180358750` (= `0x00000019` = 25) and
`0x180358754` (= `0x00000320` = 800) are read-only `.rdata` ints, used as
"loadable pointers" by the CMOV. They are NOT step fields — they're the
value-domain endpoints. They sit adjacent to other unrelated speed-related
ints (verified by reading 16 bytes from `0x180358750` =
`19 00 00 00 20 03 00 00 9C FF FF FF 64 00 00 00`).

### Caller — `ControlSpeedActor::onUpdate`

`FUN_180056c20` on 20260421 (`FUN_180053040` on 20250805):

```c
if (input_just_arrived) {                     // [actor + 0x58 + 8*page] == 1
    if (!(player_input[0x934] >> 0x16 & 1)    // bit 22 (DOWN) NOT set
        && (!actor[0x88] || !(other_player[0x934] >> 0x16 & 1))) {
        // UP path
        if ((player_input[0x934] >> 0x15 & 1)        // bit 21 (UP) set
            || (actor[0x88] && (other[0x934] >> 0x15 & 1))) {
            if (vtable[0x1a8](OptionHispeed)) {      // <-- speed-up step
                int* val = vtable[0x210](OptionHispeed);
                dispatch_message(0x1042, *val);      // notify listeners
            }
        }
    } else {
        // DOWN path
        if (vtable[0x1b0](OptionHispeed)) {          // <-- speed-down step
            int* val = vtable[0x210](OptionHispeed);
            dispatch_message(0x1042, *val);
        }
    }
}
```

Note **no Start-held check anywhere**. The DOWN bit and UP bit are both at
positions 21/22 in the player input bitmap at `[player + 0x934]`. There is
no parallel Start+UP / Start+DOWN bit position — those would have to be
introduced at the input-decode layer, which they aren't.

---

## Coarse/Fine Mechanism Investigation

### Vanilla SPEED row: hand-rolled, no coarse/fine

The above is the entire dispatch chain for the SPEED row. Three pieces of
evidence confirm that vanilla SPEED has no coarse/fine differentiation:

1. **`vtable[0x1a8]` / `vtable[0x1b0]` are each called from exactly ONE site
   in the binary.** Verified by AOB scanning `FF 90 A8 01 00 00` (CALL
   `[RAX+0x1a8]`) and `FF 90 B0 01 00 00` — single hit each on both 20260421
   and 20250805 MODIFIED, both inside `ControlSpeedActor::onUpdate`. If
   coarse/fine were stored as paired step values selected by another caller,
   we would see additional callsites.

2. **The two step functions hardcode `0x19` and `-0x19` as immediates.** No
   register-loaded step value, no per-instance step field. If the row had a
   coarse/fine field pair, the immediate would have been replaced by a
   `MOV EDX, [RCX + step_offset]` or similar memory load. The pattern is the
   simplest possible: `add edx, imm8`.

3. **Vanilla `ControlSpeedActor::onUpdate` does not register
   `event_register_no_consume` lambdas (the canonical scalar-row coarse-step
   path).** The `event_register_no_consume` function (`FUN_180050bc0` on
   20260421, anchor signature `4C 89 4C 24 20 57 48 83 EC 50 48 C7 44 24 28
   FE FF FF FF 48 89 5C 24 68 49 8B D9 44 8B CA 48 8B F9 48 83 79 10 02`) is
   called from 23 functions — all of them are the slot-4 advance-handlers of
   `OptionElement<int>`-derived rows (e.g. LaneTransparency, LaneCover,
   numeric Hispeed input, etc.). `FUN_180056xxx` (ControlSpeedActor) is
   conspicuously absent from the caller list. The custom_options framework
   in this codebase already encodes this asymmetry: `rows.rs` registers
   coarse-step lambdas for scalar rows specifically, which is where the
   "fine vs coarse" semantics live for those rows. The SPEED row participates
   in NEITHER side of this — it bypasses the option-row event system
   entirely.

### Why the prior research doc framed this differently

`binary_modpack_research.md §13` describes the original modder's cave stub
that "calls `arkMDXGetStart` for the OTHER player's slot, and uses 0x32 if
held else 0x05". One could read that as the modder *replacing* existing
coarse/fine logic. The disassembly shows otherwise: the modder was
*introducing* coarse/fine where none existed. The vanilla pre-mod
instructions at the `±0x19` sites had no Start-held conditional anywhere in
the call chain — they were a flat `add edx, ±0x19` and that was it.

### The user's mental model — corrected

User stated: "vanilla DDR's option-row infrastructure already implements
fine/coarse step semantics based on Start-held, and the proper hack is to
mutate those baked-in step values rather than installing a parallel input
detector."

Corrections:

- "vanilla DDR's option-row infrastructure already implements fine/coarse"
  — **partially true.** The standard `OptionElement<int>` infrastructure
  (LaneCover, LaneTransparency, numeric scroll-speed input, etc.) DOES have
  fine vs coarse, dispatched via `event_register_no_consume`. But the
  SPEED row (Hispeed adjuster on the song-select screen via the
  ControlSpeedActor) is not built on that infrastructure — it has a custom
  handler with no Start-held branch.
- "we just need to override the SPEED option row's coarse and fine step
  values" — **infeasible as stated.** There are no fields to override. The
  `0x19` is an immediate in two specific instruction bytes. The only way to
  change it is to patch the bytes (a 1-byte instruction tweak, not a field
  write) OR install a code hook.

---

## Storage Model Analysis (a vs b)

The question prompt offered two models:

- **(a) Hardcoded immediates.** The `0x19` is baked into the disassembly at
  `0x1801e01b7` (and at `0x1801e0204` for the negative direction). To change
  the step, patch the immediate byte or detour the instruction.
- **(b) OptionElement field.** The `0x19` is loaded from a per-instance
  field on some `OptionElement` subclass; the disassembly is reading that
  field into EDX before the ADD.

**The answer is unambiguously (a).** The disassembly literally reads
`83 C2 19` (add edx, imm8) and `83 C2 E7` (add edx, -25 in two's-complement
imm8). There is no `MOV EDX, [RCX + N]` preceding either ADD that could be
the source of the magnitude. The clamps are constants, not fields.

If the user's preferred port has to be "memory write to a stored step
field, restore on disable" — that strategy is **not possible** for SPEED
because there is no such field.

---

## Recommended Implementation Approach

### Option (i): two short detours at the speed-step functions — RECOMMENDED

Install one `retour::GenericDetour` per speed-step function
(`FUN_1801e01a0` and `FUN_1801e01f0` on 20260421; equivalent file-relative
addresses on other versions, located by AOB). The detour function is
trivial:

```rust
// pseudocode — illustrative, not the final API surface
extern "C" fn speed_up_detour(this: *mut u8) -> bool {
    let value_ptr = call_vtable(this, 0x210) as *mut i32;
    let current = unsafe { *value_ptr };
    if current >= 800 { return false; }
    let step = if start_held_for_other_side() { 50 } else { 5 };
    let next = (current + step).min(800);
    unsafe { *value_ptr = next; }
    true
}
```

Why this is the cleanest port:

- **One detour per target function** (not per instruction), which is the
  rule in this project's CLAUDE.md ("One detour per target function. Never
  install two independent `retour::GenericDetour` handles on the same
  function").
- **No code-cave or mid-instruction hook** — `GenericDetour` patches the
  function prologue, which is the well-tested path in this codebase.
- **Zero state to maintain.** The detour reads the value via the existing
  `vtable[0x210]` getter, computes the new value with our chosen step,
  writes it back. No row pointer registry, no enable/disable mutation
  cycle.
- **Survives all the structural identity we have on this code path.** Both
  vanilla and modified 20250805 plus stock 20260421 show identical structure
  (same vtable offsets, same value-getter index, same clamp constants). A
  future game update would have to actively rewrite this whole speed-button
  pipeline to break the hook — the AOB anchors below would tolerate the
  expected drift (RIP-relative shifts).
- **Trivial Start-held wiring.** The same `arkMDXGetStart` pair the
  custom_options framework already uses can be queried inside the detour.
  Question Q9 wanted "OTHER player's Start held" semantics — the original
  modder used this because the player triggering speed-toggle naturally has
  Start held themselves; we should match that to keep one player's Start
  from making the other player's speed coarse-step. Implementation:
  `arkMDXGetStart(1 - own_side)`.

### Why NOT install at the `±0x19` ADD sites with mid-function hooks

Patching a 3-byte `add edx, imm8` requires either:

- A byte-level write to flip `0x19` → `0x05` permanently (not a hook;
  defeats enable/disable). Doesn't give us coarse-vs-fine — it picks ONE
  value.
- A 5-byte `JMP rel32` overwriting bytes 0..5 of the function and a
  trampoline. Functionally equivalent to detouring the function, but harder
  because the ADD instruction we want to control is at offset +0x17 from
  the function entry — we'd have to hook the function entry anyway. Just do
  that.

### Why NOT replicate the binary modpack's cave-stub approach

The modpack's choice to inject at the `±0x19` byte sites (replacing 15
contiguous bytes in the body of each function with a trampoline) was
constrained by working as a binary patch with no debugger and no relocation
infrastructure. The hook DLL has both. Detouring at the function prologue
is the canonical approach, and `services/judge_hook` and friends already
demonstrate the pattern.

### Why NOT acknowledge the user's mental model is wrong and "just" change `0x19` to `0x05`

That would lose the coarse-step (Start-held) UX feature, which is the
distinguishing feature of this mod over a plain step-size change. Two-tier
stepping is the user-facing value; eliminating it for cleanliness is a bad
trade.

### Sub-toggle wiring

Per Q8, the SPEED toggle hooks are gated by JSON sub-toggle
`speed_toggle_smaller_steps` under `song_selection_improvements`. When
`false` (or the JSON section is absent), neither detour installs.

---

## AOB Anchors

### Primary anchor — `±0x19` ADD instruction sites

These are the same anchors the modpack research doc already published.
Re-verified during this RE pass. They identify the ADD-immediate
instructions inside the speed-step functions, NOT the function entry. If
implementing the hook as a `GenericDetour`, hook the **function start
address** derived from these anchors (`anchor - 0x17` for both).

| Site | Pattern | 20250805 stock | 20260421 |
|---|---|---|---|
| Speed-up ADD (R27) | `83 C2 19 4C 8D 05 ?? ?? ?? ?? 48 8D 4C 24 30` | unique @ `0x1801ca147` | unique @ `0x1801e01b7` |
| Speed-down ADD (R28) | `83 C2 E7 4C 8D 05 ?? ?? ?? ?? 48 8D 4C 24 30` | unique @ `0x1801ca194` | unique @ `0x1801e0204` |

**Wildcards:** the `LEA R8, [rip+...]` 4-byte disp32 (clamp-constant ptr).
This is the only address that drifts between versions — fully wildcarded.

**Function-start derivation:** subtract `0x17` from the anchor (`SUB
RSP,0x28; MOV RAX,[RCX]; CALL [RAX+0x210]; MOV EDX,[RAX]; CMP EDX, ...`
prologue is 0x17 bytes before the ADD).

### Stronger alternative — function-entry anchor

If a more robust anchor is desired (one whose pattern includes the
prologue and so identifies the function unambiguously even if the body
were slightly reorganized), a longer pattern starting at function entry:

```text
Pattern (speed-up):
  48 83 EC 28 48 8B 01 FF 90 10 02 00 00 8B 10 81 FA 20 03 00 00 7D ?? 83 C2 19
   ^-- SUB RSP, 0x28
                ^-- MOV RAX, [RCX]
                       ^-- CALL [RAX+0x210]
                                  ^-- MOV EDX, [RAX]
                                        ^-- CMP EDX, 0x320
                                                          ^-- JGE rel8 (offset wildcarded)
                                                                ^-- ADD EDX, 0x19
```

Verified `48 8B 01 FF 90 10 02 00 00 8B 10` is unique to the two callsites
in `FUN_1801e01a0` and `FUN_1801e01f0` (and their 20250805 equivalents).
This is more robust against changes to the clamp-pointer indirection but
matches less surrounding context. Either form works.

### Caller-side anchor — `ControlSpeedActor::onUpdate` vtable[0x1a8] / [0x1b0]

If the project later wants to detour `ControlSpeedActor::onUpdate` itself
(rather than the speed-step functions), anchor the unique `CALL
[RAX+0x1a8]` / `[RAX+0x1b0]` callsites:

```text
Pattern: 48 8B 87 90 00 00 00 48 8D 8F 90 00 00 00 FF 90 A8 01 00 00
                                                    ^-- CALL [RAX+0x1a8]  (speed-up)
```

`FF 90 A8 01 00 00` and `FF 90 B0 01 00 00` are each unique in the binary
(verified on both 20260421 and 20250805 MODIFIED). Either anchors the
speed-button dispatch site uniquely.

The simpler hook surface (option (i) above) is the speed-step functions
themselves; this caller-side anchor is documented for completeness in case
a future feature needs to intercept earlier in the dispatch chain.

---

## Cross-Version Notes

| Element | 20250805 stock | 20260421 |
|---|---|---|
| Speed-up function entry | `0x1801ca130` | `0x1801e01a0` |
| Speed-up ADD-imm site | `0x1801ca147` | `0x1801e01b7` |
| Speed-down function entry | `0x1801ca180` | `0x1801e01f0` |
| Speed-down ADD-imm site | `0x1801ca194` | `0x1801e0204` |
| Clamp constants (lo, hi) | `0x18033b4d0`, `0x18033b4d4` | `0x180358750`, `0x180358754` |
| `OptionHispeed` vtable[0x210] (value getter) | unchanged | unchanged |
| Vtable[0x1a8] / [0x1b0] callsite count | 1 each | 1 each |
| Caller (`ControlSpeedActor::onUpdate`) | `FUN_180053040` | `FUN_180056c20` |
| Player input bit positions (UP=21, DOWN=22) | unchanged | unchanged |
| Hispeed value range `[0x19..0x320]` (=0.25x..8.00x) | unchanged | unchanged |

The full call chain — input bit detection, vtable dispatch, value getter,
message dispatch (`0x1042`) — is structurally identical across these two
versions despite ~7 months of game updates. This is consistent with the
SPEED row being a stable, hand-written subsystem (and not, say, a generated
template specialization that gets re-emitted with each build).

---

## Gotchas

1. **`OptionHispeed` is NOT `OptionElement<KIND::SpeedType>`.** The binary
   contains both — `OptionHispeed@selectmusic@sequence` (the actual SPEED
   numeric adjuster) and `OptionElement<KIND::SpeedType>` (the X-MOD/M-MOD
   /CMOD speed-type enum row). The latter uses the standard
   `OptionElement<T>` machinery with fine/coarse via
   `event_register_no_consume`; the former does not. Mistaking one for the
   other and looking at the standard scalar machinery is what initially led
   me toward the user's hypothesis about coarse/fine fields existing — they
   do exist for `OptionElement<int>`, just not for `OptionHispeed`.

2. **The clamp constants at `0x180358750`/`0x180358754` look like they could
   be "step values" because the lower one (= 25 = 0x19) coincidentally
   equals the step magnitude.** It does not — it's the floor of the value
   range. `Option::SetHispeed` (`FUN_1801df9a0`) uses both as
   CMOV-substitutable ptrs for clamping `[25..800]`. Don't try to overwrite
   them as "step values"; you'd break the floor clamp instead.

3. **`Option::SetHispeed` has a `% 5 != 0 && < 100` snap-up.** When the
   speed-step function commits a new value via `*value = next`, no further
   snapping occurs. `Option::SetHispeed` is called when a value is set
   from elsewhere (e.g. profile load) and snaps non-multiples of 5 below
   1.00x. Below 1.00x our `±5` step keeps everything as multiples of 5
   automatically; the `+50` coarse step likewise keeps multiples of 5 (and
   can land on `0.50x` exactly from `0x` after one press, which is the
   intended UX).

4. **`vtable[0x210]` is a plain pointer-getter — it does NOT clone.** It
   returns a writable `int*` into the `OptionHispeed`'s internal storage.
   Both vanilla code and the detour we're proposing rely on this. Don't be
   tempted to call it twice for the same press — once is enough.

5. **The "OTHER player's Start" rule from the original mod.** The player
   pressing the SPEED button on their own panel is naturally pressing
   Start-on-their-own-side as part of the "Speed up/down" button gesture
   (or it's a different button mapped to that bit and the rule doesn't
   apply — depends on cab config). The original modder used the OTHER
   player's Start as the coarse-step modifier specifically so that one
   player's normal speed-adjust input isn't promoted to coarse by their own
   Start press. This codebase should match: query Start for `1 - own_side`.
