# Task record: restart derivations (plan Step 4)

Status: Complete (2026-08-16) — host gates green; cabinet demo rides
deploy #2 per the plan.
Task file: `.agents/tasks/2026-08-15-song-preview-rate/step04/task-01-restart-derivations.code-task.md`

## What landed

- `src/core/signatures.rs`: four new `SignatureDefinition`s
  (`audio_loader_ctor`, `selectmusic_view_ctor`, `cue_handle_stop`,
  `sound_bank_create_router`) + `derive_preview_restart` (uniqueness
  re-check, RIP-decode of the two vftables at match+3 / match+30,
  in-module + slot-0-in-module validation, publishes
  `audio_loader_vftable` / `selectmusic_view_vftable`).
- `src/services/song_rate/preview.rs`: restart section — cfg-free
  `LoaderSnapshot` + pure `loader_sane` (slot 5, mode 1, both ids ≥ 0;
  handle/failed deliberately NOT sanity inputs — they are the
  executor's/watchdog's); windows `init_restart` (all-or-nothing
  4-pointer stash) / `restart_available` / `resolve_loader` (TS →
  child+0x58 live → View+0xB8 identity-gated → loader @ +0xC8+0x08
  identity-gated → field snapshot; game-thread-only contract) /
  `probe_loader_chain` (post-publish, outcome atomic — the detour never
  logs) / `take_chain_probe`.
- `src/services/song_rate/runtime.rs`: drain reports each distinct
  probe outcome once (OK INFO / unresolved WARN / insane WARN).
- `src/mods/song_playback_speed.rs`: `Mod::init` calls
  `init_restart(ctx.signatures)`; `enable()` reports availability
  (INFO available / WARN degraded — edits apply at next settle).
- `src/services/song_rate/preview_tests.rs`: `loader_sane` accept +
  reject matrix (2 new tests).

## Ghidra validation (this session)

All four patterns matched EXACTLY ONCE on 20260324 / 20260421 /
20260616 / 20260721 (`ghidra_search_byte_patterns`); per-build match
table + annotated byte breakdowns written to
`research/preview-retrigger-re.md` §9. Cross-check on 20260616: both
LEA decodes land on in-module vftables (loader vft slot-0 = the tick;
View vft xrefed only from ctor/dtor).

Key RE correction vs. the handoff notes: the View ctor's FIRST LEA
(`48 8D 05`, match+23) is an inner interface vftable stored at +0x28 —
the View's own vftable is the SECOND LEA (`4C 8D 1D`, match+30, stored
bare to `[RBX]` by `4C 89 1B`). The derivation decodes match+30.

## Gates

- validator: 234 passed (232 → 234; `logs/tests-validator.log`)
- `cargo check --target x86_64-pc-windows-msvc`: clean
- `cargo fmt` (whole crate): applied
- `./build.sh`: clean
