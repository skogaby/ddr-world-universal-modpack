# Progress — Step 4 task-01: XACT IO-Callback Signatures and Runtime Address Derivations

Updated: 2026-08-10
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] Setup: working dir + context.md + plan.md (approval chain verified)
- [x] Ghidra cross-verification, all four builds (AOB single-match; LEA decodes;
      readFile prefix + handle_lookup; file_table incl. the two owed decodes
      0616 = 0x1806f1f50, 0421 = 0x1806ee068; decode arithmetic sanity-checked
      against the two known file_table values)
- [x] `src/core/signatures.rs`: `SONG_RATE_IO_*` consts + the
      `song_rate_io_callback_regsite` `SignatureDefinition` +
      `derive_song_rate_io_callbacks` (fail-closed publish-or-remove-all over the
      5-name set) + `resolve_derived` wiring (after
      `derive_song_rate_runtime_sites` — ordering dependency commented)
- [x] `docs/xact_streaming_research.md` §6: cross-version table (all six columns,
      all four builds), Option-A handle-lookup decision + double-lookup cost note,
      file-table struct layout, owed-verification note closed
- [x] Five gates green:
      1. `./scripts/validate_song_playback_speed.sh` — validation passed
         (streaming + replay legs all PASS; log in `logs/`)
      2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED
      3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings
      4. `cargo fmt` — whole crate, clean
      5. `./build.sh` — release DLL OK (log in `logs/`)
- [x] Records closed (this file + feature progress.md)

## Decisions (auto mode)

- Handle→file_id mechanism: **Option A — publish the stock lookup helper**
  (`song_rate_handle_lookup`, fastcall, RCX = handle, EAX = file_id or -1, takes the
  AVS mutex itself), derived from the first CALL in the readFile callback body
  (entry+0x21) behind a 34-byte literal-prologue validation. Rationale + cost note in
  context.md and the RE note §6.
- Host-test posture: no host tests for the derivation — the repo has none for any
  `derive_*` (the validator harness does not mount signatures.rs); boot-time
  fail-closed validation + 4-build Ghidra verification is the established posture
  (requirement 8 is a SHOULD; posture matched, per plan).
- Publish set removed atomically on any failure (via a local `fail!` macro so no
  early-return can leak a partial publication): `song_rate_io_callback_regsite`,
  `song_rate_readfile_callback`, `song_rate_overlapped_callback`,
  `song_rate_handle_lookup`, `song_rate_file_table`. None added to any required set.

## Execution log

- 2026-08-10: Ghidra re-verification pass (all four builds re-searched; both owed
  file_table decodes filled; regsite/readFile/handle_lookup evidence re-confirmed
  live; decode arithmetic validated against the two previously known file_table
  values). Tables in context.md.
- 2026-08-10: signatures.rs implementation (+161 lines, no other src changes);
  RE-note §6 rewrite; gates run (all green, windows check 0 warnings).

## Deviations

- None. (One type-level fix during implementation: the `in_module` bounds-check
  closure takes `*const u8` to match the scanner API — no behavioral significance.)

## Sibling status (plan Step 4)

- task-01: Complete (this record)
- task-02, task-03, task-04: outstanding — plan Step-4 checkbox NOT ticked
  (task-04 ticks it when all four records are Complete).
