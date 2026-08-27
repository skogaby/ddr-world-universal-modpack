# Summary — Fast Bootup SSQ async-load race fix (20260721)

> **Status: DONE** — root-caused, fixed, and live-validated on CrossOver against
> gamemdx `20260721` (2026-07-21). Build gates green. Uncommitted (maintainer
> commits). Full RE record: `research/investigation.md`.

## Problem

After upgrading gamemdx 20260616 → 20260721, the game crashed on startup
(`EXCEPTION_ACCESS_VIOLATION`) under CrossOver. Bisected to the **Fast Bootup**
mod: stock game (no hook) boots; hook with fast-bootup disabled boots; hook with
fast-bootup enabled crashes. (Independent of NoteTypesExpansion — a red herring
from log ordering; NTX installs detours in `init()` so disabling it in config
doesn't remove them.)

## Root cause

Fast Bootup re-invokes `CheckStepDataActor::onUpdate` (the boot-time SSQ
preloader) up to 512×/frame to collapse the multi-minute boot. SSQ loading is
async (worker thread); the per-entry status flips to `6` ("loaded") right after
the data read is *issued*, and there's a window where status is `6` but the
buffer bytes aren't fully written/visible to the main thread. Stock processes
one entry/frame, so a full frame always settles the buffer before use; the
512/frame loop reaches freshly-`6` entries with no gap (worsened by x86→ARM's
weaker memory model under CrossOver), so the game's SSQ chunk-walker
(`FUN_1801cbdc0`) walks off the unsettled buffer. 20260616 didn't expose the
window; the guard offsets themselves were unchanged.

## Fix (`src/mods/fast_bootup.rs`)

Added a **bounded SSQ chunk-walk readiness gate**: before letting the original
process an entry, `should_process_more` now also requires the entry's buffer to
be a complete chunk list fully contained in `[buf, buf+len)` via
`ssq_chunk_list_walkable` — a strictly bounds-checked mirror of the game's own
walk. Unsettled buffers fail the walk and are deferred to a later frame; buffers
proven walkable in-bounds are safe for the game's identical walk (it cannot run
off the end). No-buffer entries (idle/failed) pass through unchanged.
`update_hook` now calls the original only through the gate (removed the
unconditional first call that could process the unsettled entry the loop stops
on). Fully version-independent (no dependence on the exact status-enum values);
loading is driven off-thread so gating `onUpdate` can't deadlock it.

## Validation

CrossOver, gamemdx 20260721, fast-bootup + note-types-expansion both enabled:
0 `EXCEPTION_ACCESS_VIOLATION`, NOW_LOADING → HARDWARE_CHECK < 1 s, boot-to-title
~14 s, attract loop cycled 3× stably (movie fix + mines exercised), no hang.

## Files touched

- `src/mods/fast_bootup.rs` — the readiness gate + reworked `update_hook`
  (rewritten module doc explaining the race and fix).
- `.agents/summary/components.md` — updated fast_bootup entry.
- `.agents/planning/20260721-fast-bootup-ssq-race-fix/research/investigation.md`
  — full RE record (crash triage, loader state machine, root cause, fix, and
  a checklist for future version bumps).

No signature or registration changes; `check_step_data_update` /
`step_data_global_table` still resolve correctly on 20260721 (the offsets
matched the game byte-for-byte — only the async timing regressed).

## Config note

Left `mod-config.json` with `fast-bootup: true` and `note-types-expansion: true`
(full working default on 20260721).
