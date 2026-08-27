# Orientation — Ultrafast Boot (fast-bootup refactor)

> Step-2 blind-spot pass. The heavy RE was already completed and published as
> `docs/ultrafast_boot_research.md` (2026-08-24) — that document is the primary
> research artifact for this feature and is not duplicated here. This file
> records the codebase-fit findings and the unknowns that shape the decision
> register.

## What already exists

- **`docs/ultrafast_boot_research.md`** — full decode of the boot SSQ pipeline:
  `CheckStepDataActor` onInit/onUpdate, the step-data manager (record layout,
  status machine, refcounts, pending/release queues, the 4-opens-per-pump cap
  at `mgr+0x70`), the Analyze output surface (14-int result block + 5-int
  radar block + ret bool = the complete cache payload), every music-DB write,
  the actor accumulators + global copy, the blocking drain
  (`0x1801fe380`), and log-measured pacing evidence (~15.5 s window,
  cap × fps bound, not disk bound).
- **`src/mods/fast_bootup.rs`** — the mod being refactored. Already detours
  `check_step_data_update` (resolved via RTTI vtable[6]); carries two
  load-bearing safety gates (buffer-settled chunk walk; done-flag +
  cursor-bounds) that must survive on the cache-miss path.
- **Signatures** — `check_step_data_vtable` (RTTI) gives onInit (vtable[4]) for
  free; `step_data_global_table` already resolves the manager singleton. All
  other needed addresses (release fn, music-DB global, by-mcode lookup, error
  reporter, global config, BPM threshold, pumps) are RIP-relative/call-rel32
  decodes from the resolved onUpdate/onBoot bodies (scanner primitives:
  `decode_rip_relative`, `scan_first_call_rel32`).
- **Host-side file identity machinery** —
  `services/avs_layeredfs/mod_paths::find_first_modfile(rel)` +
  `std::fs` fallback to `data/{rel}` is the proven LayeredFS-aware host
  resolution (used by `chart_length::parse_for_code` and the
  per-song-judgement-offsets bootstrap, which established that AVS trampolines
  must NOT be called off game threads — host `std::fs` from the game's
  `contents/` working directory is the pattern, e.g. `./data/arc/startup.arc`).
- **Cache-file precedent** — `data_mods/_cache/shader_synthesis/` (fingerprint
  header, fail-open regeneration). Coalesced background CSV writer precedent
  in `per_song_judgement_offsets`.

## Constraints that shape the design

1. **One detour per target.** Analyze (`0x1801c8680`) is already detoured by
   NoteTypesExpansion (`src/mods/note_types_expansion/hooks.rs`). First-boot
   capture needs the Analyze boundary (the result/radar blocks are onUpdate
   stack locals — unreachable from the onUpdate detour). Capturing there means
   converting NTX's detour into a shared dispatcher service (judge_hook /
   render_notes_hook model) — or choosing a capture mechanism that doesn't
   need the boundary (music-DB read-back, with known information loss: exact
   per-chart radar values and per-slot analyzable bools).
2. **Refactor, not a new mod** (maintainer decision): same id `fast-bootup`,
   same config toggle, cache + pacing removal folded in.
3. **Fail-open everywhere**: any cache read/format/identity error ⇒ stock boot
   path (which is exactly today's fast-bootup behavior).
4. **The replay must be bit-exact** with what stock would compute, or
   score/display metadata (BPM readouts, EX scores, flags, radar normalization)
   silently drifts. This argues for a cabinet verify mode before trusting the
   skip path.
5. **NTX interplay is proven safe** (research §8.1): boot-time mine injection
   affects nothing persistent, so skipping boot Analyze changes no observable
   state and NTX config need not invalidate the cache.
6. **Percent display + watchdog**: onBoot itself blocks in the drain (no
   rendering), so blocking is platform-tolerated — but the loading bar is the
   only user feedback; a bounded per-frame drain preserves it.

## Unknowns / open items feeding the register

- Capture mechanism choice (Analyze dispatcher vs DB read-back) — the one
  decision that changes module structure.
- Completion mechanics (full replication vs leave-last-item-stock).
- Whether raising `mgr+0x70` alone reaches disk speed on real 60 Hz hardware
  (research §9.3) — affects whether the bounded drain is needed at all.
- Cache identity granularity and invalidation set (research §7.2).
- Corruption-report (`ME1529`) replay parity.

## Proposed sequence

Clarification first (the RE is done; the register can be built entirely from
`docs/ultrafast_boot_research.md` + codebase facts), then design. No further
research runs are expected unless a register decision demands one (e.g. a
cabinet pacing measurement, which can also land as an early implementation
step).
