# Scene Manager Research — DDR World

## Summary

DDR World's scene/screen management is handled by the `sequence::TransitionSequence` class. Scene transitions are dispatched through `TransitionSequence::createNextSequence` at `gamemdx.dll+0x2E140`, which receives the next scene ID in `RDX` and dispatches via a 58-case switch statement.

## Key Findings

### TransitionSequence::createNextSequence (gamemdx.dll+0x2E140)

**This is the hook target for scene change detection.**

- **RTTI class**: `.?AVTransitionSequence@sequence@@`
- **Function**: `TransitionSequence::createNextSequence`
- **Calling convention**: x64 fastcall — `RCX` = this pointer, `RDX` = next scene ID (1-indexed)
- **Internal behavior**: `lea edi,[rdx-01]` converts to 0-indexed, then dispatches via switch on `edi` (0–57, 58 total scenes)
- **String anchor**: References `"EntryFlow(%2d,%2d)"` format string at `+0x33E120` for debug logging

### How Scene IDs Work

- Scene IDs passed in `RDX` are **1-indexed** (range 1–58)
- The function internally subtracts 1 for the switch table (0-indexed range 0–57)
- Unknown/out-of-range IDs (>57 after subtraction) jump to a default handler

### Transition Chains

Scene transitions fire in **chains** — a single user action (e.g., pressing Start on attract screen) triggers multiple rapid calls to `createNextSequence` with intermediate scene IDs. The final call in the chain is the destination scene.

Example: Attract → Language Select fires this chain:
```
RDX: 0x26(38) → 0x12(18) → 0x07(7) → 0x08(8) → 0x09(9) → 0x0F(15) → 0x29(41) → 0x2A(42)
```
The user-visible screen is the last ID in the chain (42 = language select).

**Implication**: The hook fires multiple times per visible transition. A scene manager should track the "settled" scene (the last ID before the next quiet period), or simply always report the latest ID and let consumers decide what to do.

### arkEntryFlowGetCurrentScene (arkmdxbio2.dll export)

- **Exported from**: `arkmdxbio2.dll` as `arkEntryFlowGetCurrentScene`
- **Called via**: Indirect call through function pointer table at `gamemdx.dll+0x6B5878`
- **Signature**: `int arkEntryFlowGetCurrentScene(int playerIndex, int* outSceneId)`
- **Usage in createNextSequence**: Called twice with playerIndex=0 and playerIndex=1 to get current scene for each player, then formatted into `"EntryFlow(%2d,%2d)"` debug string
- **Note**: This returns the *current* scene, not the *next* scene. The next scene is in RDX.

### Related ark* Functions (arkmdxbio2.dll exports)

Found via string references in gamemdx.dll data section:
- `arkEntryFlowGetCurrentScene` — get current scene for a player
- `arkEntryFlowSetSceneResult` — set result/outcome of current scene
- `arkEntryFlowResetGameRequest` — request game reset
- `arkEntryFlowResetGameWait` — wait for game reset
- `arkEntryFlowGetGameOverFlag` — check game over state
- `arkEntryFlowGetGameOverState` — get game over details

These are part of a scripting/binding layer in `arkmdxbio2.dll` that exposes game state to external systems.

### TransitionSequence Object Layout

- **Offset +0x2C**: Class name string "TransitionSequence" (debug/RTTI)
- **RTTI vtable**: Discoverable via `.?AVTransitionSequence@sequence@@` RTTI string scan

## Scene ID Map (Partial — from breakpoint captures)

These are **0-indexed** scene IDs (RDX - 1):

| 0-indexed ID | RDX (1-indexed) | Observed Screen | Category |
|-------------|-----------------|-----------------|----------|
| 5 | 0x06 (6) | Transition/intermediate | transition |
| 6 | 0x07 (7) | Transition/intermediate | transition |
| 7 | 0x08 (8) | Transition/intermediate | transition |
| 8 | 0x09 (9) | Transition/intermediate | transition |
| 14 | 0x0F (15) | Transition/intermediate | transition |
| 17 | 0x12 (18) | Transition/intermediate | transition |
| 18 | 0x13 (19) | Transition (lang→mode) | transition |
| 20 | 0x15 (21) | Mode Select | in_game |
| 21 | 0x16 (22) | Transition (mode→entry) | transition |
| 24 | 0x19 (25) | Transition (entry flow) | transition |
| 25 | 0x1A (26) | Song Select / Caution / Difficulty | in_game |
| 37 | 0x26 (38) | Attract exit | transition |
| 40 | 0x29 (41) | Transition (→lang select) | transition |
| 41 | 0x2A (42) | Language Select | in_game |

**Note**: Many IDs are intermediate transitions that flash by in <1 frame. The "user-visible" scenes are the ones that persist (language select, mode select, song select, etc.). The full 58-scene map is documented separately.

## AOB Signature

```
Pattern: 48 8B C4 55 57 41 54 41 55 41 56 EB ? E8 ? ? ? ? 48 81 EC 40 03 00 00
Offset:  +0x2E140 (DDR World gamemdx.dll, 64-bit 20250805)
Unique:  Yes (1 match in gamemdx.dll)
```

Wildcards:
- `EB ?` at +0xB: Short JMP relative offset (changes between builds)
- `E8 ? ? ? ?` at +0xD: CALL rel32 displacement (changes between builds)

Stable bytes:
- `48 8B C4` — `mov rax, rsp` (prologue)
- `55 57 41 54 41 55 41 56` — push rbp/rdi/r12/r13/r14 (callee-saved registers)
- `48 81 EC 40 03 00 00` — `sub rsp, 0x340` (stack frame size, distinctive)

## Hook Strategy

1. **Scan** for the AOB signature to find `createNextSequence`
2. **Hook** on entry
3. **Apply redirects** — if a redirect is registered for this scene ID, overwrite `RDX` with the target ID before the function processes it
4. **Read** `RDX` as the next scene ID (1-indexed)
5. **Subtract 1** to get the 0-indexed scene ID
6. **Update** current/previous scene tracking
7. **Fire** registered callbacks with `(previousId, newId)`

The hook fires on the game thread (same thread as the render loop), so callback execution is thread-safe with respect to widget operations.

### Scene Redirects

Since `RDX` is writable in the hook's entry handler, the destination scene can be changed before `createNextSequence` processes it. This enables skipping interstitial screens (e.g., splash screens → main menu) by redirecting their scene IDs to the desired target.

## Build Version

All addresses from **gamemdx.dll build 20250805** unless otherwise noted.
