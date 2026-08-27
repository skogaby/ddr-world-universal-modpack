# R3 — Hook point & coordinate payload (CONFIRMED)

## The hook target: `FUN_18006f5d0` — the named-layout setter

Signature (3-arg `__fastcall`):

```c
void FUN_18006f5d0(longlong parent, char *name, undefined4 *coord);
//                 RCX             RDX          R8
```

Verified by decompile (20260324). Body `0x18006f5d0`–`0x18006f6a6`. It:
1. Measures `strlen(name)` and builds a temp string from `name`.
2. Copies **6 dwords** from `coord` into the layout entry it allocates/looks up
   (`FUN_18006fb40(parent + 0x28, name)`):
   ```c
   puVar8[0] = coord[0];  // X        <-- the field the hack rewrites
   puVar8[1] = coord[1];  // Y
   puVar8[2] = coord[2];
   puVar8[3] = coord[3];
   puVar8[4] = coord[4];  // (scale X, per 32-bit analysis)
   puVar8[5] = coord[5];  // (scale Y)
   ```

So **`coord[0]` = X, `coord[1]` = Y**, 6 dwords total (0x18 bytes). This matches the
32-bit `FUN_1005bcd0` analysis in `docs/hex_edit_porting.md` (Hack 2).

## Hook strategy

- Install a **detour** (retour `GenericDetour`) on `FUN_18006f5d0`. In the callback:
  - Read `name` (RDX) as a NUL-terminated C-string — it's a readable `char*` at call
    time (the function itself does `strlen` on it, so it's always valid).
  - If centering is active for this call (see R2 for the side/single-player gate and the
    name-set membership), **rewrite `coord[0]` (X)** before calling the original.
  - Call the original with the (possibly) modified `coord`.
- Per project rules: **one detour per target function**. `FUN_18006f5d0` is not currently
  hooked by any other service, so a dedicated detour is fine. If another mod ever needs it,
  promote to a shared dispatcher (cf. `judge_hook`).
- Callback must be panic-safe (`catch_unwind` or strictly panic-free) — it's `extern "C"`
  called from the game's layout build (not the 60 Hz hot path, but still game-thread).

## AOB signature

Need a stable byte pattern for `FUN_18006f5d0`'s prologue. Candidate (derive from the
function bytes at `0x18006f5d0` and wildcard the stack-cookie RIP-relative disp and any
RIP-relative `LEA`s). **Impl task:** capture the prologue bytes on BOTH 64-bit builds
(20260324, 20260526) and confirm one masked pattern matches exactly one site in each.
Follow the existing `core/signatures.rs` style (e.g. mask `? ? ? ?` over RIP displacements).

## Cross-version

Decompile structure identical on 20260526 (verified during RE: `FUN_18006f5d0` analog
present, `double_lane_usr`/`arrow_raw` anchors present). Re-confirm the exact prologue AOB
on both builds when authoring the signature.

## Reference
- `docs/hex_edit_porting.md` → Hack 2 (full mechanism + 32-bit↔64-bit mapping).
- Pattern style: `src/core/signatures.rs`.
- Detour lifetime mgmt: `src/core/hooks.rs`.
