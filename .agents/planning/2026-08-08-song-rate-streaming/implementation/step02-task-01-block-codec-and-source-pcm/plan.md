# Plan — Step 2 task-01: Block-Level Codec Wrappers and the SourcePcm Decode View

Status: Approved 2026-08-09 (via the maintainer-approved Step 2 task breakdown;
Source Plan `Status: Approved 2026-08-08`, design `Status: Approved 2026-08-08` —
verified in context.md. Auto mode per handoff instruction.)

## Test scenarios (written first, against the absent API — expected initial
failure mode is compilation failure of the mounted harness/tests)

All in `src/core/xact/tests.rs`, following the file's existing helper
conventions (`format()` builder, deterministic synthetic PCM).

T1. `per_block_codec_wrappers_match_whole_buffer` (AC1)
    For channels in [1, 2, 6]: build 5 blocks of deterministic sine PCM
    (existing generator pattern), `encode_interleaved` → whole. Then per block:
    - `adpcm::decode_block(&whole[b*align..(b+1)*align], fmt, &mut out)`
      accumulated over all blocks == `decode_interleaved(whole, fmt, frames)`
      (full-block duration, so equality is exact with no trim).
    - `adpcm::encode_block(&pcm[b*spb*ch..(b+1)*spb*ch], fmt, &mut enc)`
      accumulated == `whole` byte-for-byte.

T2. `block_cache_view_matches_whole_buffer_decode` (AC2)
    Stereo entry: 80 blocks (forces 64-slot eviction + revisits), logical
    duration = 80·128 − 17 (partial final block, stock trim semantics).
    `expected = decode_interleaved(data, fmt, logical)`;
    `view = adpcm::BlockCachePcm::new(&data, fmt, logical)`.
    - frames()/channels() match.
    - In-order, reverse, and LCG-shuffled random full sweeps over
      (frame, channel): `view.sample(f, c) == expected[f*ch+c]`.
    - Same-slot alternation (block 0 ↔ block 64) stays correct.
    - `catch_unwind`: `sample(logical, 0)` and `sample(0, channels)` panic.
    Repeat the equality sweep (in-order only) for 1- and 6-channel entries.

T3. `block_codec_and_cache_reject_malformed_inputs` (R5)
    - `decode_block` with block.len() = align−1 and align+1 → Err.
    - `encode_block` with a non-multiple-of-channels slice and with a
      2-block-sized window → Err.
    - Both wrappers with a zero-channel format → Err (InvalidChannelCount).
    - `BlockCachePcm::new`: predictor byte 7 in a non-first block → Err
      (BadPredictor at construction — pre-scan); tail remainder 1 → Err;
      duration/block-count mismatch → Err; zero-channel format → Err.

T4. `slice_pcm_view_is_a_faithful_trivial_source` (R4)
    `stretch::SlicePcm::new(&pcm, 2)` → frames/channels/sample() agree with
    direct indexing; `new` rejects channels = 0 and a partial-frame slice;
    `catch_unwind`: out-of-range frame panics.

AC3 is covered by the existing suites passing unmodified; AC4 by the standing
gates.

## Implementation approach

1. `src/core/xact/adpcm.rs`
   - Rename private `encode_block` → `encode_block_raw`, `decode_block` →
     `decode_block_raw` (2 internal call sites; not observable).
   - Add `pub fn encode_block(pcm: &[i16], format: WaveFormat, output: &mut
     Vec<u8>) -> Result<(), AdpcmError>`: `validate_format`, require exactly one
     block of frames (`IncompletePcmFrame`/`IncompletePcmBlock`), delegate.
   - Add `pub fn decode_block(block: &[u8], format: WaveFormat, output: &mut
     Vec<i16>) -> Result<(), AdpcmError>`: `validate_format`, require
     `block.len() == block_align` (`EncodedBlockSize`), delegate.
   - Add `pub struct BlockCachePcm<'a>` + `impl stretch::SourcePcm`:
     - `new(data, format, logical_frames)`: `validate_format` +
       `validate_encoded_layout` (stock-tail + duration semantics identical to
       `decode_interleaved`), pre-scan every block's predictor bytes (≤ 6),
       allocate 64 direct-mapped slots (`RefCell`, reused `Vec<i16>` per slot).
     - `sample(frame, channel)`: assert in-range; slot = block % 64; on tag
       miss, `decode_block_raw` into the slot (infallible post-validation).
2. `src/core/xact/stretch.rs`
   - `pub trait SourcePcm { fn frames(&self) -> usize; fn channels(&self) ->
     usize; fn sample(&self, frame: usize, channel: usize) -> i16; }`
     (documented panic contract on out-of-range).
   - `pub struct SlicePcm<'a>` with validating `new` (reuses
     `StretchError::InvalidChannelCount` / `IncompleteSourceFrame`) and
     asserting `sample`.
   - `stretch_interleaved_with` and everything below it: UNTOUCHED.
3. Tests per T1–T4 in `src/core/xact/tests.rs`.

Rationale: append-to-buffer wrapper shape matches the internals and avoids
per-block allocations for the streaming producer; typed errors at construction
+ panic on misuse mirrors the reference stretcher's own indexing contract;
predictor pre-scan converts the only data-dependent decode failure into a
constructor error so the hot read path cannot return garbage or fail silently.

Risks: none to shipped behavior (pure host-side addition; whole-buffer paths
byte-identical after the private rename). Maintainability: the 64-slot
direct-map is the simplest structure whose correctness is size-independent.

## Gate checklist (run per task, in order)

1. `./scripts/validate_song_playback_speed.sh`
2. `./scripts/validate_se_bank_synth.sh`
3. `cargo check --target x86_64-pc-windows-msvc` (0 warnings)
4. `cargo fmt` (whole crate)
5. `./build.sh`
