# Research — Fast Bootup crash on 20260721 (SSQ async-load race)

> RE record for the `fast-bootup` startup crash after upgrading gamemdx
> 20260616 → 20260721. All Ghidra addresses are file-relative to image base
> `0x180000000`, program `gamemdx_20260721.dll` (game `MDX:J:F:A:2026072100`).
> Investigated 2026-07-21.

## Symptom

After upgrading to 20260721, the game crashes on startup (`EXCEPTION_ACCESS_VIOLATION`)
a few seconds into boot. Reproduces via CrossOver/Wine on Apple Silicon. Stock
game (no `-K ddr_world_hook.dll`) boots fine → the crash is caused by the hook DLL.
The crash is **independent of NoteTypesExpansion** (identical stack with NTX
disabled; NTX installs its detours in `init()` so config-disabling it doesn't
remove them, and its mine-injection log line just happened to precede the fault).

## Triage

Crash stack (spice2x stackwalker; gamemdx base `0x6ffffb3c0000`,
ddr_world_hook base `0x6ffffa850000` from `minidump.dmp`):

```
0x6ffffb58bdfd gamemdx  = FUN_1801cbdc0  +0x3d   ← fault: CMP dword ptr [R8],EDI
0x6ffffb3f246d gamemdx  = FUN_180032360  (calls FUN_1801cbdc0)
0x6ffffabc03af ddr_world_hook +0x3703af  ← fast_bootup update_hook, after hook.call()
0x6ffffb5ddd2a gamemdx  = FUN_18021dc70 +0x...  actor dispatcher, CALL [RAX+0x30] (onUpdate)
...
```

The ddr_world_hook frame disassembled to fast_bootup's loop (constants
`0x200`=512 `MAX_PER_FRAME`, bitmask `0x161`={0,5,6,8} `READY_STATUSES`,
`IN_HOOK` flag, reads `[actor+0x58]`/`[actor+0x88]`). Confirmed culprit:
**fast_bootup**, not NTX.

## The functions

- **`FUN_180032360` = `CheckStepDataActor::onUpdate`** (`check_step_data_update`,
  vtable[6]) — the boot-time step-data (SSQ) preloader. Per call, if the current
  cursor entry's status ∈ `{0,5,6,8}`, it builds a `step::SsqReader`, calls
  `FUN_1801cbdc0(&reader)` to summarize the SSQ, then loops both play-sides
  calling `FUN_1801c8680` (Analyze), and advances the cursor. Entire body is
  gated by the status guard — nothing runs when the entry isn't ready.
- **`FUN_1801cbdc0`** — SSQ chunk-list walker. Reads the buffer at `reader+0x10`
  (= step-data record `+0x8`) and walks `ptr += *ptr` over chunk headers
  `{+0:u32 len, +4:u16 type, +6:u16 mark}`, stopping on `len==0`, `type==2`, or
  `mark==0xffff`. **Fault is `1801cbdfd: CMP dword ptr [R8],EDI` — reading the
  next chunk header after `R8 += chunk_len` ran off the buffer** (a garbage
  chunk length from an unsettled buffer).
- **`FUN_1801fdbf0`** — async worker's SSQ load state machine, writing the
  step-data record (stride `0x40`, base `[mgr+8]`, `mgr = DAT_1806f2f48`):
  - `+0x08` buffer, `+0x14` length, `+0x18` alloc size, `+0x1c` result,
    **`+0x20` status**, `+0x28` reader, `+0x30` stream.
  - status: `1` open → `2` read-header (allocs buffer `+0x8`, sets len `+0x14`)
    → `3` **reading data into buffer** → (`4` alt-load) → `5` failed | `6`
    **load complete** → (`7` cleanup) → `8` finalized.
- `FUN_1801fe380` — blocking "drain all pending loads" pump; `FUN_1801fe150` —
  done-queue finalize. Loader pumps are **not** called from `FUN_180032360`, so
  loading progresses independently of `onUpdate` (no deadlock from gating it).

## Root cause

`FUN_180032360`'s status gate `{0,5,6,8}` (idle / failed / loaded / finalized)
correctly excludes the in-flight states `{1,2,3,4}`, so in principle it only
processes settled entries. But the worker sets status `6` **immediately after
issuing the data read** (status 3 allocates buf + kicks read; completion flips
to 6), and there is a window where status == 6 but the buffer bytes are not yet
fully written / visible to the main thread.

- **Stock (1 entry/frame):** a full frame always elapses between "worker sets 6"
  and the main thread's cursor reaching that entry, so the buffer is settled.
  No crash.
- **fast_bootup (up to 512/frame):** the loop reaches freshly-`6` entries with
  ~0 gap. Under CrossOver on Apple Silicon (x86 → ARM, weaker memory model) the
  buffer write can lag the status write, so `FUN_1801cbdc0` walks an unsettled /
  partially-written buffer and runs off the end.

20260616 happened not to expose the window (loading timing differed); the guard
offsets themselves did **not** change (fast_bootup's `should_process_more`
matched `FUN_180032360` byte-for-byte on 20260721).

## Fix

`should_process_more` now additionally requires the entry's SSQ buffer to be a
**complete chunk list fully contained in `[buf, buf+len)`**, via
`ssq_chunk_list_walkable` — a strictly bounds-checked mirror of
`FUN_1801cbdc0`'s traversal (same header layout, same terminators, same
`ptr += len` advance; every access bounds-checked, capped at `MAX_CHUNKS`).
Entries with no buffer (`buf==0`/`len==0`, e.g. idle/failed) pass through
unchanged (the game skips the walk for them via its own `buf!=0 && len!=0`
guard). Entries whose buffer isn't settled fail the walk → deferred to a later
frame (loading is concurrent, so they settle within a frame or two).

Key property: **if the gate passes, the game's identical walk provably stays
in-bounds → it cannot run off the buffer.** The batch stays fast (all genuinely
settled entries still process in one frame) and the fix is version-independent
(no dependence on the exact status-enum numbering). `update_hook` was also
simplified to call the original only through the gate (no unconditional first
call), so the freshly-`6`-but-unsettled entry the loop stops on is never
processed until it validates.

## Validation (2026-07-21, CrossOver, gamemdx 20260721)

Stock (no hook) boots ✓. Hook with fast-bootup **disabled** boots ✓ (confirmed
culprit). Hook with fast-bootup **re-enabled + fix**: 0 `EXCEPTION_ACCESS`,
NOW_LOADING → HARDWARE_CHECK < 1 s, boot-to-title ~14 s, attract loop cycled 3×
stably, no hang. Build gates green (`cargo check`/`fmt`/`./build.sh`).

## Notes for future version bumps

If fast-bootup crashes again after a gamemdx update, this same async-load race
is the prime suspect. The fix is structural (bounded buffer validation) and
should survive status-enum renumbering. If the crash moves, re-verify:
`check_step_data_update` still resolves to `CheckStepDataActor::onUpdate`
(vtable[6]); the step-data record offsets (`+0x8` buf, `+0x14` len, `+0x20`
status; stride `0x40` at `[table+0x08]`); and the SSQ chunk-header layout in
`FUN_1801cbdc0`.
