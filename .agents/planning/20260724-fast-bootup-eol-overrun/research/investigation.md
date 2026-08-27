# Research — Fast Bootup crash at ~97% loading (end-of-list overrun)

> RE record for the intermittent `fast-bootup` boot crash reported 2026-07-24
> by a tester porting ~900 custom songs. Ghidra addresses are file-relative to
> image base `0x180000000`, program `gamemdx_20260721.dll` (game
> `MDX:J:F:A:2026072100`). Hook DLL in the field: `sko1714.dll` (this codebase,
> pre-fix). Investigated 2026-07-24.

## Symptom

Intermittent `EXCEPTION_ACCESS_VIOLATION` at ~97% of the boot loading screen.
Key field observations:

- **Only with Fast Bootup enabled.** With it disabled the same data always boots.
- **Appears/disappears with `musicdb.xml` changes.** After adding a batch of
  custom songs the crash may start; *rearranging* entries in `musicdb.xml`
  (no content change) makes it go away.
- The crash log's tail is a burst of hundreds of `INVALID SSQ : mcode=888xxxx …
  noteNum=0` lines all in the same second (the custom songs with broken/absent
  charts, processed in one fast-bootup batch), then `W:DDR: EXCEPTION CATCH.`

Crash stack (spice2x stackwalker; gamemdx base `0x7ffb68230000`):

```
gamemdx+0x325EF   ← fault, inside FUN_180032360 (CheckStepDataActor::onUpdate)
sko1714+0xB1F3    ← fast_bootup update_hook (after hook.call)
gamemdx+0x21DD2A  ← FUN_18021dc70 actor dispatcher, case 0x102: CALL [RAX+0x30]
gamemdx+0x22EACB / +0x22EAF1  ← FUN_18022eaa0 message send / scene child walk
```

## The faulting instruction

`+0x325EF` is the **NULL-vtable deref after a failed music-DB lookup** inside
`onUpdate`'s per-side loop:

```
1800325ad: MOV R12D,[R14 + R13*4 + 0x8]   ; mcode from work item {idx, diff, mcode}
1800325d3: CALL FUN_1801b7e80             ; lower_bound binary search of music DB
1800325db: CMP RAX,RDI                    ; == end()?
1800325de: JZ  1800325ed
1800325e0:  … getMcode(hit) vcall; mismatch falls through …
1800325ed: XOR ESI,ESI                    ; MISS → RSI = NULL
1800325ef: MOV RAX,[RSI]                  ; ← EXCEPTION_ACCESS_VIOLATION
1800325fe: CALL [RAX+0x70]                ; would be MusicInfo vcall
```

`FUN_1801b7e80` is a `lower_bound` over the music-DB array (`DAT_1806f2d78`,
0x258-byte entries, keyed by `getMcode()` vcall). The game derefs the result
**unguarded** at this one site (elsewhere, e.g. `FUN_1801b4290`, it null-checks).

## Why stock can never hit this

The work list is built **from the music DB itself**: `onInit`
(`FUN_180032030`, vtable[4]) iterates the DB and pushes 5 work items (one per
difficulty) per song, each carrying that song's own mcode. Every legitimate
work item's mcode is therefore guaranteed to be found; a mid-list miss would
crash stock identically — and stock never crashes. The crash must live in a
code path stock never executes.

The only such path: **calling `onUpdate` after the work list is complete.**
When the cursor processes the final item, `onUpdate` sets the actor's done
flag (`actor+0x20 |= 4`, plus the parent-chain `|= 8` walk). The actor
dispatcher (`FUN_18021dc70`, message 0x102) gates every `onUpdate` call on
`(*(u32*)(actor+0x20) & 0x24) == 0` — once done, stock never calls it again.

## Root cause

`fast_bootup`'s `update_hook` calls the original **directly in a loop**,
bypassing the dispatcher's done-flag gate; and its `should_process_more` had
**no bounds check on the cursor** against the work-list length:

1. The batch processes the final work item; `onUpdate` sets done (`+0x20|=4`).
2. The loop evaluates `should_process_more` again. `counter == total` now, so
   `entry_index = work_array[total]` reads 12 bytes **past the end of the
   heap allocation** (the vector is `reserve`d to exactly `songs*5` items).
3. If the heap garbage passes the gates (`entry_index != 0`, garbage record's
   status ∈ {0,5,6,8}, buf null-or-walkable), the original is invoked once
   more. `onUpdate` reads the same garbage triple: garbage mcode →
   `lower_bound` miss → NULL deref at `+0x325EF`.

Every symptom follows:

- **Fast-bootup-only:** stock's dispatcher stops at the done flag.
- **Intermittent / layout-sensitive:** the OOB read happens on ~every
  fast-bootup boot; it *crashes* only when the bytes past the work array
  happen to pass all three gates. Growing or rearranging `musicdb.xml`
  perturbs allocation sizes/order → re-rolls that garbage. "Rearrange until
  it boots" was never a data fix — just a heap-layout reroll.
- **~97%:** the crashing frame is the final ≤512-item batch containing the
  list tail; the screen still shows the previous frame's percentage. The
  `INVALID SSQ` burst immediately before the fault is that same batch chewing
  through the chart-less custom songs sitting at the end of DB order.
- (~1/512 boots are immune even with bad garbage: if the final item lands
  exactly on the `MAX_PER_FRAME` cap, the loop exits before the OOB
  evaluation and the dispatcher never calls again.)

Distinct from the 20260721 SSQ async-load race
(`.agents/planning/20260721-fast-bootup-ssq-race-fix/`): that one faulted in
the chunk walker `FUN_1801cbdc0` on an unsettled buffer; its readiness gate
remains correct and in place. This is a second, independent defect.

## Fix (`src/mods/fast_bootup.rs`)

`should_process_more` now mirrors the game's own two stop conditions **before
touching the work array**:

- **Done-flag gate:** return false when `(*(u32*)(actor+0x20) & 0x24) != 0` —
  the dispatcher's exact check.
- **Cursor bounds gate:** return false when `counter[phase] >=
  ([actor+0x90] - [actor+0x88]) / 12` — `onUpdate`'s own completion
  condition. Logs one `log_info` line per boot when it first trips ("work
  list complete … stopping batch at list end") so field logs can confirm the
  gate is active.

Fidelity cleanups in the same pass:

- Cursor read is now per-phase like the game's
  (`[actor+0x58 + phase*8]`, phase u16 at `actor+0x82`) instead of assuming
  phase 0.
- Record status is read/compared as a full `i32` (the game compares `dword`;
  the old `u8` read could disagree on garbage).
- The record address uses an explicit signed offset (`entry_index as isize`)
  — the game stores `-1` for songs whose SSQ couldn't be registered and reads
  the bytes just before the records array; we gate on the same bytes instead
  of relying on usize wraparound.

The post-completion call is now structurally impossible; everything the batch
does is a strict subset of what stock's dispatcher would allow.

## Actor field map (`CheckStepDataActor`, 20260721)

| Offset | Type | Meaning |
|--------|------|---------|
| `+0x20` | u32 | lifecycle flags; bit 2 = done (set by onUpdate on completion), gate mask `0x24` |
| `+0x58` | {u32,u32}[] | per-phase {counter, aux} pairs (counter = work-list cursor) |
| `+0x80` | u16 | phase count |
| `+0x82` | u16 | current phase index |
| `+0x88` | ptr | work array begin — 12-byte items `{i32 entry_index, i32 difficulty, i32 mcode}` |
| `+0x90` | ptr | work array end |
| `+0xd8` | ptr→u32 | loading-percent display target (`counter*100/total`) |

## Validation

- Build gates green (`cargo check` / `cargo fmt` / `./build.sh`).
- Field test: deploy to the tester's cabinet **with the currently-crashing
  `musicdb.xml` arrangement** — it should boot without rearranging, and the
  log should show the new "work list complete" line at the end of loading.

## Notes for future version bumps

If fast-bootup crashes again after a gamemdx update, verify in this order:

1. The actor field map above (flags `+0x20`, counters `+0x58`, phase `+0x82`,
   work array `+0x88/+0x90`, 12-byte item stride).
2. The dispatcher gate mask (`0x24` at `FUN_18021dc70` case 0x102).
3. The step-data record offsets (`+0x8` buf, `+0x14` len, `+0x20` status u32;
   stride `0x40` at `[table+0x08]`) and the SSQ chunk-header layout in
   `FUN_1801cbdc0` (the 20260721 race-fix checklist).
