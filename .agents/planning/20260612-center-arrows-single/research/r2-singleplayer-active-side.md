# R2 — Single-player detection & active-side identification (MOSTLY CONFIRMED)

## What the builder knows about sides

From the `FUN_18006c230` decompile (20260324), the builder iterates `side = 0..1`:

```c
for (local_1b0 = 0; local_1b0 < 2; local_1b0++) {
    iVar4 = *(int *)(param_1 + 0x84 + local_1b0 * 4);   // per-side PLAY-STATE
    if (iVar4 != 2) {                                    // 2 => this side is NOT playing
        local_28b = (iVar4 == 0);                        //  sub-state flag (single vs. ...)
        ... lays out this side's HUD, calling FUN_18006f5d0(perSideParent, name, &coord) ...
    }
    // side skipped entirely when iVar4 == 2
}
```

- **Play-state field:** `builder_root + 0x84 + side*4` (int per side).
  - `== 2` → side not playing (skipped).
  - `!= 2` → side active. `== 0` vs `== 1` distinguishes a sub-state used to pick
    `normal`/`reverse` and feeds `local_28b` (read by the lane single/double + reverse logic).
- **Per-side layout parent** passed to `FUN_18006f5d0`:
  `perSideParent = builder_root + 0xE0 + side*0x48`
  (decompile: `lVar8 = local_1d8 + 0xe0 + local_1c0*8`, `local_1c0 = side*9` ⇒ `side*0x48`).

## Recovering builder_root + side from inside our hook

Our detour is on `FUN_18006f5d0`, whose `param_1` (RCX) is the **per-side parent**, i.e.
`builder_root + 0xE0 + side*0x48`. Every per-side element (score/bpm/lane/arrow/…) uses the
same per-side parent, so each call carries a `param_1` of this form. `param_1` alone doesn't
reveal the side index without the builder root.

**Recommendation — lightweight entry hook on `FUN_18006c230`:** per layout pass, record
`builder_root` (RCX) and compute `{single_player, active_side}` from the two play-states at
`+0x84+side*4`:
- `single_player` = exactly one side has state `!= 2`.
- `active_side`  = the index of that lone active side.

The `FUN_18006f5d0` detour then maps `side = (param_1 - (builder_root + 0xE0)) / 0x48` and
gates centering on `single_player && side == active_side && option_enabled[active_side]`.
This keeps detection authoritative (read from the object being laid out) and avoids guessing
a global player-count.

> Two detours on two *different* functions (`FUN_18006c230` entry + `FUN_18006f5d0`) is fine —
> "one detour per target" is per-function. Because `FUN_18006c230` calls `FUN_18006f5d0`
> synchronously on the same thread, the entry hook can stash
> `{builder_root, single_player, active_side}` in a `static` that the setter hook reads within
> the same nested call stack — no cross-thread concern.

## "Single-player" definition (verify at impl time)

Working definition: **exactly one** side has play-state `!= 2` at builder entry. Robust to
P1-side vs P2-side single play (Q3: center the lone active side regardless of which it is).

**Impl verification (diagnostic build first, per project learnings):** read both sides'
`+0x84` values in known single-player and known 2P sessions. Expected:
- 1P (P1 side): `[+0x84]=0|1`, `[+0x88]=2`.
- 1P (P2 side): `[+0x84]=2`, `[+0x88]=0|1`.
- 2P/versus: both `!= 2` ⇒ two active sides ⇒ NOT single-player ⇒ mod no-ops (satisfies the
  Q2 hard gate that centering never triggers in 2P).

## Cross-version
Field offsets (`+0x84`, `+0xE0`, stride `0x48`) are from the 20260324 decompile; re-confirm
on 20260526 when authoring. Builder structure was verified identical during RE; spot-check
these specific offsets.

## CORRECTION (live diagnostic, 2026-06-13) — `+0x84` is NOT the presence signal

Deploying the detection diagnostic disproved the `+0x84`-based theory. Captured builder passes
in a **2P attract demo** and a **real 1P session** were byte-identical:
`count@0x80=5 idx@0x82=0 s0=0 s1=0 s2=0 s3=0` in BOTH. Root cause: the builder object is a
`sequence::dance::LayoutActor` (ctor `FUN_18006abe0`), and `+0x80`(=5, element-array count),
`+0x82`(index), `+0x84..`(per-side play STYLE: 0=single/1=double/2=absent, used only for the
`"single"`/`"double"` lane-name selector) are **construction params**, not player count. They
read identically in 1P and 2P, so they cannot discriminate.

**Authoritative signal (verified):** the engine's own per-side lamp/credit code
(`FUN_1800102a0` family) gates on `*(*player_ptr + 4) != 0`, where the player pointers are a
2-element array (P1=`[0]`, P2=`[1]` at `+8`):
- 20260526: array @ `0x1806F1ED0`
- 20260324: array @ `0x1806EBE50`

So: `single_player := (p0_present != p1_present)`, `active_side :=` the present one, where
`pN_present := *(array[N] + 0x4) != 0`.

**Resolution mechanism:** new signature `player_array_anchor` matches the accessor prologue
`48 8B 05 <disp32> 66 C7 05 <disp32> 00 FF …` (the `MOV RAX,[RIP+disp32]` that loads the
array). Several near-identical accessors match; all alias the same global, so the first match's
disp32 (decoded at +3 via `decode_rip_relative`) is authoritative. Verified: first match
decodes to `0x6EBE50` (20260324) / `0x6F1ED0` (20260526).

**Still confirmed-correct from the original research:** the `parent → side` mapping
(`side = (parent - (root+0xE0)) / 0x48`) resolved correctly live (`side=Some(0)`), and the
setter/coord layout + option plumbing all work — `single_player` was the sole defect.
