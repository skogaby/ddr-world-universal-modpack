# Progress — Step 3 task-02: Synthetic Engine Replay Harness with Byte-Equality Proof

Updated: 2026-08-09
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] Audio fixture builder (`build_bank_bytes` generalization +
      `replay_fixture`/`replay_fixture_with_main_loop`: 8 kHz stereo tone
      audio, main 32,768 frames / preview 2,048, full-entry loops) + oracle
      helper (`transform_bank_oracle` — the validator's `transform_bank`
      composition rebuilt), pinned by T2/T3 reparses + planned lengths
- [x] `EncodedFeed` + T2 feed pin ({50, 175} × both entries: uneven in-order
      slices reassemble byte-equal to the oracle payloads; length ==
      plan `data_len` exactly, whole blocks)
- [x] Replay pump (`serve_read` over `resolve` + `replay_engine_reads`) +
      T3 matrix ({25, 50, 100, 175} × both entry orders: reassembly
      byte-identical to the oracle bank, reparse, per-entry decode equality,
      read-pattern fidelity from the read log — exact (0, 0x1000) header,
      sequential block-align-rounded packets, EOF read serves 0, nothing
      past `virtual_size`)
- [x] T4 loop-restart legs: 50% full-entry (zero checkpoint, zero discard —
      the production shape, asserted) and 75% interior loop (captured
      hop-aligned checkpoint strictly below the block-aligned target —
      nonzero discard bridge asserted); both re-served windows byte-match
      the first serving, produced ONLY via `restore_at_block`
      (`StretchState::restore` + produce-and-discard), never retained bytes
- [x] Debug runtime within budget: cargo-test phase 7.56 s (< 30 s)
- [x] Gate 1: validator green — 130/130 host tests, all report checks PASS
      (`logs/validator.log`)
- [x] Gate 2: se-bank ALL CHECKS PASSED (`logs/se-bank.log`)
- [x] Gate 3: windows check 0 warnings (`logs/check-windows.log`)
- [x] Gate 4: fmt clean (whole crate)
- [x] Gate 5: build.sh release DLL OK (`logs/build.log`; tests are
      `#[cfg(test)]`-only — release artifact unchanged, cargo up-to-date)
- [x] NO commit (maintainer commits personally)

## TDD cycles

1. Fixture-builder generalization first (`build_bank_with_data_lengths` →
   thin wrapper over the new `build_bank_bytes` with explicit
   formats/payloads/durations/loops): full suite re-run green (37/37)
   before any replay code — the shared-fixture guard.
2. Oracle + feed + pump + all three tests landed as one section (the
   assertions ARE the spec for a pure-test task; each equality compares two
   independent pipelines — reference/whole-buffer vs streaming/per-block —
   so a divergence anywhere fails loudly). Full suite green on the first
   complete run: 40/40 in 7.44 s.

## Acceptance criteria evidence

- AC1 replayed == oracle: `engine_replay_reassembles_the_oracle_bank` —
  8 cells, byte equality + `parse_song_bank` + decoded-PCM equality per
  entry (decode equality read as decode-vs-decoded-oracle: ADPCM is lossy,
  so "the reference's stretched PCM" is meaningful post-encode — recorded
  in context.md).
- AC2 read-pattern fidelity: the pump's read log asserted per cell (header
  (0, 0x1000, 0x1000); per-entry sequential offsets; requested ==
  `min(block-rounded 64 KiB, stream remaining)`; served == requested;
  `offset + served ≤ virtual_size`; past-the-end read → 0).
- AC3 loop restart: `loop_restart_reproduces_identical_bytes` (both legs).
- AC4: five gates green, Windows check 0 warnings.

## Deviations

- **Data packet requests are stream-bounded** (`min(rounded packet,
  data_len − cursor)`) rather than blindly full-sized: the engine bounds
  reads to the wave's data region (RE: "≤ 64 KiB", loop-aware stream
  context), and a full-size tail request would span into the alignment gap
  the engine never reads. The file-level EOF clamp is still exercised (the
  final entry-1 packet ends exactly at `virtual_size`; a defensive
  past-the-end read serves 0 via `Region::Eof`).
- **One interior-loop restart cell added at 75%** beyond the full-entry
  matrix: on full-entry loops (production shape, and the required 25/50
  envelope) the restart collapses to zero-checkpoint + zero-discard, which
  cannot prove the hop-aligned-resume → block-aligned-target discard bridge
  the task's R5 mechanics describe. Interior loops succeed at 75% (Step-2
  matrix), so the cell is within the recorded envelope; the nonzero-discard
  assertion is structural (the mapped loop start 5327 is not hop-aligned).
- **The feed retains produced bytes within a pass** so the header read's
  entry-0 overlap ([0, 2048) re-read by the first data packet) serves
  identical bytes — mirroring the production ring window, which always
  covers the engine's look-ahead. R5's prohibition binds on the restart
  proof, which never touches retained bytes (fresh feed via
  restore + discard only).
- Matrix runs {25, 50, 100, 175} (task minimum + both recommended rates) ×
  both entry orders; no trimming was needed (phase 7.56 s).

Status: Complete (uncommitted — maintainer commits personally)
