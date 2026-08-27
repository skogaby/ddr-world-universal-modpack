# Anytime Speedmod Adjustment Research

RE record for the "Anytime Speedmod Adjustment" mod: removing the stock ~10-second
window during which the player can change their speed mod (arrow scroll multiplier)
with the cabinet navigation buttons at the start of each song.

All addresses are file-relative to `gamemdx.dll`'s `0x180000000` base. Primary build:
**20260721**; cross-verified on 20260616, 20260526, 20260421.

## Overview

Stock DDR World lets the player tap the menu Left/Right buttons during roughly the
first 10 seconds of gameplay to step the speed multiplier down/up in 0.25× steps.
After that window the buttons go dead for the rest of the song.

The entire mechanism is owned by a small per-song actor,
`sequence::dance::ControlSpeedActor`, and the time limit is **one immediate
comparison** in its message handler: each frame the gameplay sequence broadcasts the
elapsed song time (msg `0x1045`), and once the payload's millisecond field reaches
`0x2710` (10 000) the actor self-destructs. Everything downstream of the actor —
the speed write, the smooth on-screen lerp, the renderer consumption — is
time-agnostic and works identically at any point in the song. Neutralizing that one
immediate removes the limit cleanly.

## Actor lifecycle (20260721 addresses)

| Item | Address | Notes |
|---|---|---|
| `ControlSpeedActor` ctor | `0x1800562f0` | size 0x128; embeds a `ddr::player::Option` at `+0x90` (vtable `0x180387978`); `+0x84` = player side, `+0x88` = doubles flag |
| ControlSpeedActor vtable | `0x180360778` | `+0x28` onFinalize `0x180056720`, `+0x30` onUpdate `0x180056500`, `+0x40` onMessage `0x180056780` |
| Creation site | `0x18005c6e2` in `FUN_18005be90` | GamePlayActor onInit (vtable `0x180360d60` `+0x20`); creation gated on `Option` vtable`+0x1B8` predicate (`0x1801e1630` — excludes tutorial/event modes). Not touched by the mod. |
| onUpdate (input poll) | `0x180056500` | Only acts while its StackStep state (`+0x58 + idx*8`, idx at `+0x82`) == 1. Reads nav-button bits (`DAT_1806f2cf0 + side*0x498 + 0x934`, bits 21/22), calls Option vtable `+0x1A8` (increment ×+0.25, `0x1801e1680`) / `+0x1B0` (decrement, `0x1801e16d0`), then broadcasts msg **`0x1042`** with the new ×100 multiplier. |
| onMessage | `0x180056780` | See below — the window open/kill logic. |

### The message handler (`0x180056780`)

```
180056780: 81 EA 43 10 00 00      SUB  EDX,0x1043
180056786: 4C 8B C9               MOV  R9,RCX
180056789: 0F 84 80 00 00 00      JZ   msg_1043_open_window   ; StackStep state := 1
18005678f: 83 EA 02               SUB  EDX,0x2
180056792: 74 3B                  JZ   msg_1045_time_check
180056794: 83 FA 05               CMP  EDX,0x5
180056797: 0F 85 28 01 00 00      JNZ  ret                    ; msg 0x104A song-end kill falls through
...
msg_1045_time_check:
1800567cf: 41 81 78 08 10 27 00 00  CMP  dword ptr [R8+0x8],0x2710   ; elapsed ms vs 10000
1800567d7: 0F 8C E8 00 00 00        JL   ret                          ; window still open
1800567dd: ...                      ; else: set own flags |= 4 (die), parents |= 8 — self-destruct
```

- **msg `0x1043`** (song start): opens the window — sets StackStep state 1, toggles
  the per-player "speed change available" HUD-hint bit in `DAT_1804c5a9c` and the
  footer id `DAT_1804c5a50 = 0xc`.
- **msg `0x1045`** (every frame): payload `+0x0` = side, `+0x8` = elapsed song ms
  (sender `FUN_18005eb00`, the gameplay time broadcaster). At `>= 10000` ms the
  actor marks itself dead. **This is the entire time limit.**
- **msg `0x104a`** (song end): unconditional kill — the normal cleanup path, left
  untouched by the mod.
- onFinalize (`0x180056720`) toggles the HUD-hint bit back off when the actor dies.

### Downstream apply path (why any-time adjustment is safe)

`sequence::dance::GamePlayActor::onReceiveMessage` (`0x18005e200`) case `0x1042`:

```c
old   = interpolate_current_speed(now);        // FUN_180060650
this->+0x290 = old / 100.0f;                   // lerp FROM: current on-screen speed
this->+0x294 = newMult / 100.0f;               // lerp TO
this->+0x298 = now;                            // animation anchor
this->+0x29C = newMult;                        // int ×100
```

The lerp is re-anchored at the **current** song time and the per-frame state-4 tick
re-writes `SpotRenderer+0x28` / `ArrowRenderer+0xA0` from these floats (see
`docs/song_playback_speed.md` § the consumer chain). Nothing in this path checks
elapsed time, so a speed change at 3:00 into a song behaves exactly like one at
0:05 — including the smooth animation.

## Signature

```
anytime_speedmod_gate: 41 81 78 08 10 27 00 00 0F 8C
                       └─ CMP dword [R8+0x8],0x2710 ──┘└ JL
```

- Exactly **1 hit on all four builds**. Patch target: the imm32 at **match+4**
  (`0x2710` → `0x7FFFFFFF`). Elapsed song ms can never approach `INT_MAX`
  (~24.8 days), so the actor lives until the normal `0x104a` song-end kill.
- No wildcards needed: opcode + ModRM/SIB + both immediates are all structurally
  fixed (the payload layout and the 10 000 ms constant are game logic, not
  compiler artifacts). The `JL` opcode pair is included to pin the branch context;
  its rel32 displacement is deliberately excluded.
- Context anchor (used for validation only, also unique ×4): the handler prologue
  `81 EA 43 10 00 00 4C 8B C9 0F 84` (`SUB EDX,0x1043; MOV R9,RCX; JZ`), with the
  gate at **entry+0x4F on every build**.

## Cross-Version Notes

| Build | Handler entry | Gate (`CMP`) | Gate − entry |
|---|---|---|---|
| 20260721 | `0x180056780` | `0x1800567cf` | +0x4F |
| 20260616 | `0x1800567c0` | `0x18005680f` | +0x4F |
| 20260526 | `0x180055fd0` | `0x18005601f` | +0x4F |
| 20260421 | `0x180056ea0` | `0x180056eef` | +0x4F |

## Gotchas

- **The HUD footer hint stays up all song.** The "speed change available" indicator
  bit (`DAT_1804c5a9c`, consumed via `FUN_18000d8b0` under footer id 0xc) is set at
  window open and cleared by the actor's onFinalize. With the gate neutralized the
  actor only finalizes at song end, so the hint remains visible for the whole song.
  Accepted as-is (it is accurate — adjustment IS available). Cleanup still happens
  at song end via the untouched `0x104a` path.
- **Mid-song enable cannot resurrect a dead actor.** If the mod is toggled on after
  the stock window already expired for the current song, adjustment returns only at
  the next song (the actor is per-song and already destroyed). Toggling off mid-song
  restores stock bytes but an already-alive actor stays alive until song end. Both
  edges are harmless.
- The imm32 rewrite is a plain data-byte patch inside an instruction that is
  executed every frame during gameplay — use the project's standard
  `memory::patch_bytes`-style protected write; the 4-byte imm is not atomic with
  the opcode but single-byte-tearing is a non-issue since both values keep the
  instruction well-formed and either constant is acceptable on any given frame.
- Do NOT patch the `JL` to an unconditional jump instead: msg `0x1045` shares its
  epilogue with other cases; the imm rewrite is the minimal, reversible edit.
