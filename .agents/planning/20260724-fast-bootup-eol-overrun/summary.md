# Summary — Fast Bootup end-of-list overrun fix (20260724)

> **Status: IMPLEMENTED** — root-caused in Ghidra against gamemdx `20260721`
> and fixed; awaiting field validation on the tester's cabinet. Build gates
> green. Full RE record: `research/investigation.md`.

## Problem

A tester porting ~900 custom songs hit an intermittent
`EXCEPTION_ACCESS_VIOLATION` at ~97% of the boot loading screen — but only
with **Fast Bootup** enabled, and only for some `musicdb.xml` arrangements
(rearranging entries, with no content change, made it go away).

## Root cause

`CheckStepDataActor::onUpdate` sets the actor's done flag when it processes
the final work-list item, and the game's actor dispatcher never calls it
again (`(actor+0x20 & 0x24) == 0` gate). Fast Bootup's batch loop calls the
original directly, bypassing that gate, and `should_process_more` had no
bounds check on the cursor — so after the final item it read
`work_array[total]`, 12 bytes past the heap allocation. When that garbage
happened to pass the status/buffer gates, the re-invoked `onUpdate` looked up
a garbage mcode in the music DB — an **unguarded** `lower_bound` whose miss
NULL-derefs (`MOV RAX,[RSI]` at gamemdx+0x325EF). Growing/rearranging
`musicdb.xml` merely rerolled the heap bytes past the array.

Distinct from (and additional to) the 20260721 SSQ async-load race fix, which
remains correct and in place.

## Fix (`src/mods/fast_bootup.rs`)

`should_process_more` now mirrors the game's own stop conditions before
touching the work array: a **done-flag gate** (`actor+0x20 & 0x24`, the
dispatcher's exact check) and a **cursor bounds gate**
(`counter[phase] < (end-begin)/12`, onUpdate's completion condition), with a
one-shot `log_info` ("work list complete … stopping batch at list end") for
field confirmation. Fidelity cleanups: per-phase cursor read
(`[actor+0x58 + phase*8]`, phase at `+0x82`), record status read as u32 (the
game compares dword), explicit signed record offset for the game's `-1`
sentinel entries.

## Validation

- `cargo check` / `cargo fmt` / `./build.sh` green.
- Field test pending: boot the tester's currently-crashing `musicdb.xml`
  arrangement with the fixed DLL — should boot without rearranging and log
  the new completion line.

## Files touched

- `src/mods/fast_bootup.rs` — the two gates + fidelity cleanups (module doc
  extended with the overrun mechanism).
- `.agents/summary/components.md` — updated fast_bootup entry.
- `.agents/planning/20260724-fast-bootup-eol-overrun/research/investigation.md`
  — full RE record (crash triage, dispatcher/actor field map, root cause,
  fix, version-bump checklist).
