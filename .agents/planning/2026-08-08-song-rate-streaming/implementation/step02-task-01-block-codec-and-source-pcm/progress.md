# Progress — Step 2 task-01: Block-Level Codec Wrappers and the SourcePcm Decode View

Updated: 2026-08-09
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] Baseline: validator green before any change (114/114, `logs/validator-baseline.log`)
- [x] T1–T4 tests written in `src/core/xact/tests.rs` (failed against absent API:
      E0603 private fns, E0433 missing types, E0432 missing trait —
      `logs/check-tests-failing.log`)
- [x] adpcm.rs: private rename (`encode_block_raw`/`decode_block_raw`) + public
      `encode_block`/`decode_block` wrappers
- [x] stretch.rs: `SourcePcm` trait + `SlicePcm` (reference stretcher untouched)
- [x] adpcm.rs: `BlockCachePcm` (64-slot direct-mapped, predictor pre-scan at
      construction, `RefCell` interior mutability for `&self` sampling)
- [x] All tests pass: 118/118 in the validator cargo-test phase (114 + 4 new)
- [x] Gate 1: `./scripts/validate_song_playback_speed.sh` — validation passed
      (`logs/validator.log`)
- [x] Gate 2: `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED
- [x] Gate 3: `cargo check --target x86_64-pc-windows-msvc` — 0 warnings
- [x] Gate 4: `cargo fmt` (whole crate; `cargo fmt --check` clean)
- [x] Gate 5: `./build.sh` — release DLL OK
- [x] NO commit (maintainer commits personally per handoff instruction)

## TDD cycles

1. Wrote T1–T4 (4 tests + 2 helpers `deterministic_pcm`/`expect_panic`) against
   the absent API; confirmed the expected failure mode (compilation failure of
   the test cfg: private `encode_block`/`decode_block`, missing `BlockCachePcm`,
   `SourcePcm`, `SlicePcm`).
2. Implemented adpcm.rs wrappers + rename, stretch.rs trait + `SlicePcm`,
   `BlockCachePcm` in one increment (the plan's shapes were fully settled);
   `cargo check --tests` clean, then validator: 118 passed, 0 failed.

## Acceptance criteria evidence

- AC1: `per_block_codec_wrappers_match_whole_buffer` — mono/stereo/6-channel
  per-block outputs byte-match the whole-buffer codec slices.
- AC2: `block_cache_view_matches_whole_buffer_decode` — 80-block stereo entry
  with a 17-frame-trimmed final block; in-order/reverse/stride-4871 sweeps +
  slot-collision alternation all equal `decode_interleaved`; out-of-range frame
  and channel panic (via `expect_panic`); 1- and 6-channel sweeps included.
- AC3: existing codec/stretch suites pass unmodified (118 = 114 prior + 4 new;
  zero edits to existing tests; `stretch_interleaved_with` untouched).
- AC4: all five gates green; Windows check 0 warnings (harness-side
  `QUICK_FAIL_TAINT` warning is the documented pre-existing one).

## Deviations

- None from the plan. Notes: the private per-block internals were renamed with
  a `_raw` suffix (matches the module's `block_align_raw` naming precedent);
  wrong-size single-block PCM windows reuse `IncompletePcmBlock` rather than a
  new error variant (single-block contract documented on the wrapper).

Status: Complete (uncommitted — maintainer commits personally)
