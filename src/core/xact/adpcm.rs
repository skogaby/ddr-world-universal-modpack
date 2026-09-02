//! Deterministic Microsoft ADPCM codec for interleaved PCM frames.

use std::cell::RefCell;
use std::fmt;
use std::io::Write;

use super::stretch::SourcePcm;
use super::WaveFormat;

const MAX_CHANNELS: usize = 6;

const COEFFICIENTS: [(i32, i32); 7] = [
    (256, 0),
    (512, -256),
    (0, 0),
    (192, 64),
    (240, 0),
    (460, -208),
    (392, -232),
];

const ADAPTATION: [i32; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];

#[derive(Debug)]
pub enum AdpcmError {
    UnsupportedCodec {
        codec: u8,
    },
    InvalidChannelCount {
        channels: u8,
    },
    InvalidBitsFlag {
        flag: u8,
    },
    InvalidBlockLayout,
    IncompletePcmFrame {
        samples: usize,
        channels: usize,
    },
    IncompletePcmBlock {
        frames: usize,
        samples_per_block: usize,
    },
    UnsupportedTail {
        remainder: usize,
        block_align: usize,
    },
    DurationBlockMismatch {
        logical_frames: u32,
        complete_blocks: usize,
        samples_per_block: usize,
    },
    BadPredictor {
        index: u8,
    },
    EncodedBlockSize {
        expected: usize,
        actual: usize,
    },
    ArithmeticOverflow,
    AllocationFailed,
    Write(std::io::Error),
    Cancelled,
}

impl fmt::Display for AdpcmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCodec { codec } => write!(f, "unsupported codec {codec}"),
            Self::InvalidChannelCount { channels } => {
                write!(f, "invalid channel count {channels}")
            }
            Self::InvalidBitsFlag { flag } => write!(f, "invalid ADPCM bits flag {flag}"),
            Self::InvalidBlockLayout => write!(f, "invalid ADPCM block layout"),
            Self::IncompletePcmFrame { samples, channels } => write!(
                f,
                "{samples} PCM samples do not form complete {channels}-channel frames"
            ),
            Self::IncompletePcmBlock {
                frames,
                samples_per_block,
            } => write!(
                f,
                "{frames} PCM frames do not form complete {samples_per_block}-frame blocks"
            ),
            Self::UnsupportedTail {
                remainder,
                block_align,
            } => write!(
                f,
                "unsupported ADPCM tail remainder {remainder} for {block_align}-byte blocks"
            ),
            Self::DurationBlockMismatch {
                logical_frames,
                complete_blocks,
                samples_per_block,
            } => write!(
                f,
                "logical duration {logical_frames} does not match {complete_blocks} complete {samples_per_block}-frame blocks"
            ),
            Self::BadPredictor { index } => write!(f, "predictor index {index} is out of range"),
            Self::EncodedBlockSize { expected, actual } => write!(
                f,
                "encoded block size {actual} does not match expected size {expected}"
            ),
            Self::ArithmeticOverflow => write!(f, "ADPCM size arithmetic overflow"),
            Self::AllocationFailed => write!(f, "ADPCM output allocation failed"),
            Self::Write(error) => write!(f, "writing ADPCM output failed: {error}"),
            Self::Cancelled => write!(f, "ADPCM operation cancelled"),
        }
    }
}

impl std::error::Error for AdpcmError {}

#[derive(Clone, Copy, Default)]
struct ChannelEncoding {
    predictor: u8,
    delta: i32,
    sample1: i16,
    sample2: i16,
}

#[derive(Clone, Copy, Default)]
struct ChannelState {
    coefficient1: i32,
    coefficient2: i32,
    delta: i32,
    sample1: i32,
    sample2: i32,
}

/// Encode complete interleaved PCM blocks without adding implicit silence.
pub fn encode_interleaved(samples: &[i16], format: WaveFormat) -> Result<Vec<u8>, AdpcmError> {
    let (channels, block_align, samples_per_block) = validate_format(format)?;
    if samples.len() % channels != 0 {
        return Err(AdpcmError::IncompletePcmFrame {
            samples: samples.len(),
            channels,
        });
    }

    let frames = samples.len() / channels;
    if frames % samples_per_block != 0 {
        return Err(AdpcmError::IncompletePcmBlock {
            frames,
            samples_per_block,
        });
    }

    let block_count = frames / samples_per_block;
    let output_len = block_count
        .checked_mul(block_align)
        .ok_or(AdpcmError::ArithmeticOverflow)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_len)
        .map_err(|_| AdpcmError::AllocationFailed)?;

    encode_interleaved_to(samples, format, &mut output)?;
    debug_assert_eq!(output.len(), output_len);
    Ok(output)
}

/// Encode complete interleaved PCM blocks directly to a byte sink.
///
/// Only one encoded block is buffered at a time. This is the production path
/// for writing large song entries without retaining a second full encoded copy.
pub fn encode_interleaved_to(
    samples: &[i16],
    format: WaveFormat,
    output: &mut impl Write,
) -> Result<(), AdpcmError> {
    encode_interleaved_to_with(samples, format, output, &mut || true)
}

pub fn encode_interleaved_to_with(
    samples: &[i16],
    format: WaveFormat,
    output: &mut impl Write,
    should_continue: &mut impl FnMut() -> bool,
) -> Result<(), AdpcmError> {
    let (channels, block_align, samples_per_block) = validate_format(format)?;
    if samples.len() % channels != 0 {
        return Err(AdpcmError::IncompletePcmFrame {
            samples: samples.len(),
            channels,
        });
    }
    let frames = samples.len() / channels;
    if frames % samples_per_block != 0 {
        return Err(AdpcmError::IncompletePcmBlock {
            frames,
            samples_per_block,
        });
    }
    if !should_continue() {
        return Err(AdpcmError::Cancelled);
    }

    let block_samples = samples_per_block
        .checked_mul(channels)
        .ok_or(AdpcmError::ArithmeticOverflow)?;
    let mut encoded_block = Vec::new();
    encoded_block
        .try_reserve_exact(block_align)
        .map_err(|_| AdpcmError::AllocationFailed)?;
    for block in samples.chunks_exact(block_samples) {
        if !should_continue() {
            return Err(AdpcmError::Cancelled);
        }
        encoded_block.clear();
        encode_block_raw(
            block,
            channels,
            samples_per_block,
            block_align,
            &mut encoded_block,
        )?;
        output
            .write_all(&encoded_block)
            .map_err(AdpcmError::Write)?;
    }
    Ok(())
}

/// Decode complete blocks and trim the result to the declared logical frames.
///
/// Stock DDR banks may append an unusable partial trailing block (usually
/// `block_align - 1` or `- 2` bytes, occasionally shorter). Those bytes are
/// ignored only when the declared logical duration requires exactly the
/// available complete blocks.
pub fn decode_interleaved(
    data: &[u8],
    format: WaveFormat,
    logical_frames: u32,
) -> Result<Vec<i16>, AdpcmError> {
    decode_interleaved_with(data, format, logical_frames, &mut || true)
}

pub fn decode_interleaved_with(
    data: &[u8],
    format: WaveFormat,
    logical_frames: u32,
    should_continue: &mut impl FnMut() -> bool,
) -> Result<Vec<i16>, AdpcmError> {
    let (channels, block_align, samples_per_block) = validate_format(format)?;
    let complete_blocks = validate_encoded_layout(data.len(), format, logical_frames)?;
    let output_len = (logical_frames as usize)
        .checked_mul(channels)
        .ok_or(AdpcmError::ArithmeticOverflow)?;
    let decoded_frames = complete_blocks
        .checked_mul(samples_per_block)
        .ok_or(AdpcmError::ArithmeticOverflow)?;
    let decoded_len = decoded_frames
        .checked_mul(channels)
        .ok_or(AdpcmError::ArithmeticOverflow)?;
    if !should_continue() {
        return Err(AdpcmError::Cancelled);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(decoded_len)
        .map_err(|_| AdpcmError::AllocationFailed)?;

    for block in data[..complete_blocks * block_align].chunks_exact(block_align) {
        if !should_continue() {
            return Err(AdpcmError::Cancelled);
        }
        decode_block_raw(block, channels, samples_per_block, &mut output)?;
    }
    output.truncate(output_len);
    Ok(output)
}

/// Decode one complete encoded ADPCM block, appending its
/// `samples_per_block * channels` interleaved samples to `output`.
///
/// Thin validating wrapper over the per-block internal shared with
/// `decode_interleaved`; the block must be exactly `block_align` bytes.
pub fn decode_block(
    block: &[u8],
    format: WaveFormat,
    output: &mut Vec<i16>,
) -> Result<(), AdpcmError> {
    let (channels, block_align, samples_per_block) = validate_format(format)?;
    if block.len() != block_align {
        return Err(AdpcmError::EncodedBlockSize {
            expected: block_align,
            actual: block.len(),
        });
    }
    decode_block_raw(block, channels, samples_per_block, output)
}

/// Encode exactly one block's worth of interleaved PCM frames, appending
/// `block_align` bytes to `output`.
///
/// Thin validating wrapper over the per-block internal shared with
/// `encode_interleaved`; `pcm` must hold exactly `samples_per_block` frames.
pub fn encode_block(
    pcm: &[i16],
    format: WaveFormat,
    output: &mut Vec<u8>,
) -> Result<(), AdpcmError> {
    let (channels, block_align, samples_per_block) = validate_format(format)?;
    if pcm.len() % channels != 0 {
        return Err(AdpcmError::IncompletePcmFrame {
            samples: pcm.len(),
            channels,
        });
    }
    let frames = pcm.len() / channels;
    if frames != samples_per_block {
        return Err(AdpcmError::IncompletePcmBlock {
            frames,
            samples_per_block,
        });
    }
    encode_block_raw(pcm, channels, samples_per_block, block_align, output)
}

/// Direct-mapped block slots for [`BlockCachePcm`]. At the stock geometry of
/// 128 frames per block this covers 8192 source frames — generously above the
/// streaming stretcher's bounded access window (≈ 2160 frames at 48 kHz).
/// Correctness never depends on the size; evicted blocks are re-decoded.
const BLOCK_CACHE_SLOTS: usize = 64;

/// Random-access decoded view of a borrowed ADPCM source-bank entry.
///
/// Blocks are self-contained and decoded on demand into a small direct-mapped
/// cache. Construction applies the same layout and logical-duration rules as
/// `decode_interleaved` (including the stock partial-tail remainders and the
/// final-block clamp) and pre-scans every block's predictor bytes, so the
/// on-demand decode cannot fail after `new` returns.
pub struct BlockCachePcm<'a> {
    data: &'a [u8],
    channels: usize,
    block_align: usize,
    samples_per_block: usize,
    logical_frames: usize,
    cache: RefCell<BlockCache>,
}

struct BlockCache {
    tags: [Option<usize>; BLOCK_CACHE_SLOTS],
    slots: Vec<Vec<i16>>,
}

impl<'a> BlockCachePcm<'a> {
    pub fn new(
        data: &'a [u8],
        format: WaveFormat,
        logical_frames: u32,
    ) -> Result<Self, AdpcmError> {
        let (channels, block_align, samples_per_block) = validate_format(format)?;
        let complete_blocks = validate_encoded_layout(data.len(), format, logical_frames)?;
        for block in 0..complete_blocks {
            let predictors = &data[block * block_align..block * block_align + channels];
            for &predictor in predictors {
                if usize::from(predictor) >= COEFFICIENTS.len() {
                    return Err(AdpcmError::BadPredictor { index: predictor });
                }
            }
        }
        Ok(Self {
            data,
            channels,
            block_align,
            samples_per_block,
            logical_frames: logical_frames as usize,
            cache: RefCell::new(BlockCache {
                tags: [None; BLOCK_CACHE_SLOTS],
                slots: (0..BLOCK_CACHE_SLOTS).map(|_| Vec::new()).collect(),
            }),
        })
    }
}

impl SourcePcm for BlockCachePcm<'_> {
    fn frames(&self) -> usize {
        self.logical_frames
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn sample(&self, frame: usize, channel: usize) -> i16 {
        assert!(
            frame < self.logical_frames,
            "frame {frame} is past the logical duration {}",
            self.logical_frames
        );
        assert!(
            channel < self.channels,
            "channel {channel} is out of range for {} channels",
            self.channels
        );
        let block = frame / self.samples_per_block;
        let offset = frame % self.samples_per_block;
        let slot = block % BLOCK_CACHE_SLOTS;
        let mut cache = self.cache.borrow_mut();
        if cache.tags[slot] != Some(block) {
            let encoded = &self.data[block * self.block_align..(block + 1) * self.block_align];
            let pcm = &mut cache.slots[slot];
            pcm.clear();
            decode_block_raw(encoded, self.channels, self.samples_per_block, pcm)
                .expect("predictor bytes were validated at construction");
            cache.tags[slot] = Some(block);
        }
        cache.slots[slot][offset * self.channels + channel]
    }
}

pub(crate) fn validate_encoded_layout(
    data_len: usize,
    format: WaveFormat,
    logical_frames: u32,
) -> Result<usize, AdpcmError> {
    let (_, block_align, samples_per_block) = validate_format(format)?;
    // A partial trailing block is legal and IGNORED. Stock DDR banks end in a
    // `block_align - 1` / `- 2` byte tail almost everywhere, but 10 DDR-era
    // previews (`abys agai baby dgra ecst eran feal feax inse orio`, 2026-09-02
    // install sweep) carry 103–137-byte tails — refusing them silently pinned
    // those songs to stock speed. The safety property is the equation below:
    // the declared duration must fit inside the COMPLETE blocks, so the tail
    // bytes are never decoded. (Corpus fact, not enforced:
    // `duration == blocks·spb − (remainder − 12)` for every tail-bearing
    // stock entry.)

    let complete_blocks = data_len / block_align;
    let expected_blocks = (logical_frames as usize)
        .checked_add(samples_per_block - 1)
        .ok_or(AdpcmError::ArithmeticOverflow)?
        / samples_per_block;
    if logical_frames == 0 || complete_blocks == 0 || expected_blocks != complete_blocks {
        return Err(AdpcmError::DurationBlockMismatch {
            logical_frames,
            complete_blocks,
            samples_per_block,
        });
    }
    Ok(complete_blocks)
}

pub(crate) fn validate_generated_layout(
    data_len: usize,
    format: WaveFormat,
    logical_frames: u32,
) -> Result<(), AdpcmError> {
    let (_, block_align, samples_per_block) = validate_format(format)?;
    if data_len % block_align != 0 {
        return Err(AdpcmError::UnsupportedTail {
            remainder: data_len % block_align,
            block_align,
        });
    }
    let complete_blocks = data_len / block_align;
    let generated_frames = complete_blocks
        .checked_mul(samples_per_block)
        .ok_or(AdpcmError::ArithmeticOverflow)?;
    if complete_blocks == 0 || generated_frames != logical_frames as usize {
        return Err(AdpcmError::DurationBlockMismatch {
            logical_frames,
            complete_blocks,
            samples_per_block,
        });
    }
    Ok(())
}

fn validate_format(format: WaveFormat) -> Result<(usize, usize, usize), AdpcmError> {
    if format.codec() != WaveFormat::CODEC_ADPCM {
        return Err(AdpcmError::UnsupportedCodec {
            codec: format.codec(),
        });
    }
    let channels = format.channels() as usize;
    if !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(AdpcmError::InvalidChannelCount {
            channels: format.channels(),
        });
    }
    if format.bits_per_sample_flag() != 0 {
        return Err(AdpcmError::InvalidBitsFlag {
            flag: format.bits_per_sample_flag(),
        });
    }
    let block_align = format.block_align() as usize;
    let samples_per_block = format.samples_per_block() as usize;
    if block_align < 7 * channels || samples_per_block < 2 {
        return Err(AdpcmError::InvalidBlockLayout);
    }
    Ok((channels, block_align, samples_per_block))
}

fn encode_block_raw(
    block: &[i16],
    channels: usize,
    samples_per_block: usize,
    block_align: usize,
    output: &mut Vec<u8>,
) -> Result<(), AdpcmError> {
    let block_start = output.len();
    let mut encodings = [ChannelEncoding::default(); MAX_CHANNELS];
    for channel in 0..channels {
        let (predictor, delta) = select_predictor(block, channels, channel, samples_per_block);
        encodings[channel] = ChannelEncoding {
            predictor,
            delta,
            sample1: block[channels + channel],
            sample2: block[channel],
        };
    }

    for encoding in &encodings[..channels] {
        output.push(encoding.predictor);
    }
    for encoding in &encodings[..channels] {
        output.extend_from_slice(&(encoding.delta as i16).to_le_bytes());
    }
    for encoding in &encodings[..channels] {
        output.extend_from_slice(&encoding.sample1.to_le_bytes());
    }
    for encoding in &encodings[..channels] {
        output.extend_from_slice(&encoding.sample2.to_le_bytes());
    }

    let mut states = [ChannelState::default(); MAX_CHANNELS];
    for channel in 0..channels {
        let encoding = encodings[channel];
        let (coefficient1, coefficient2) = COEFFICIENTS[encoding.predictor as usize];
        states[channel] = ChannelState {
            coefficient1,
            coefficient2,
            delta: encoding.delta,
            sample1: i32::from(encoding.sample1),
            sample2: i32::from(encoding.sample2),
        };
    }

    let mut pending_high = None;
    for frame in 2..samples_per_block {
        for channel in 0..channels {
            let state = &mut states[channel];
            let actual = i32::from(block[frame * channels + channel]);
            let predicted =
                (state.sample1 * state.coefficient1 + state.sample2 * state.coefficient2) >> 8;
            let signed_nibble = if state.delta == 0 {
                0
            } else {
                ((actual - predicted) / state.delta).clamp(-8, 7)
            };
            let nibble = (signed_nibble & 0xf) as u8;
            let reconstructed = (predicted + signed_nibble * state.delta).clamp(-32768, 32767);
            state.sample2 = state.sample1;
            state.sample1 = reconstructed;
            state.delta = ((state.delta * ADAPTATION[nibble as usize]) >> 8).max(16);

            if let Some(high) = pending_high.take() {
                output.push((high << 4) | nibble);
            } else {
                pending_high = Some(nibble);
            }
        }
    }
    if let Some(high) = pending_high {
        output.push(high << 4);
    }

    let actual = output.len() - block_start;
    if actual != block_align {
        return Err(AdpcmError::EncodedBlockSize {
            expected: block_align,
            actual,
        });
    }
    Ok(())
}

fn select_predictor(
    block: &[i16],
    channels: usize,
    channel: usize,
    samples_per_block: usize,
) -> (u8, i32) {
    let mut best_predictor = 0;
    let mut best_error = i64::MAX;
    let mut best_delta = 16;
    for (index, &(coefficient1, coefficient2)) in COEFFICIENTS.iter().enumerate() {
        let (error, delta) = simulate(
            block,
            channels,
            channel,
            samples_per_block,
            coefficient1,
            coefficient2,
        );
        if error < best_error {
            best_error = error;
            best_predictor = index as u8;
            best_delta = delta;
        }
    }
    (best_predictor, best_delta)
}

fn simulate(
    block: &[i16],
    channels: usize,
    channel: usize,
    samples_per_block: usize,
    coefficient1: i32,
    coefficient2: i32,
) -> (i64, i32) {
    let sample2 = i32::from(block[channel]);
    let sample1 = i32::from(block[channels + channel]);
    let predicted = (sample1 * coefficient1 + sample2 * coefficient2) >> 8;
    let initial_error =
        (i32::from(block[2 * channels + channel]) - predicted).unsigned_abs() as i32;
    let initial_delta = (initial_error / 4).max(16);
    let mut delta = initial_delta;
    let mut previous1 = sample1;
    let mut previous2 = sample2;
    let mut total_error = 0i64;

    for frame in 2..samples_per_block {
        let actual = i32::from(block[frame * channels + channel]);
        let predicted = (previous1 * coefficient1 + previous2 * coefficient2) >> 8;
        let signed_nibble = if delta == 0 {
            0
        } else {
            ((actual - predicted) / delta).clamp(-8, 7)
        };
        let nibble = (signed_nibble & 0xf) as u8;
        let reconstructed = (predicted + signed_nibble * delta).clamp(-32768, 32767);
        let error = i64::from(actual - reconstructed);
        total_error = total_error.saturating_add(error * error);
        previous2 = previous1;
        previous1 = reconstructed;
        delta = ((delta * ADAPTATION[nibble as usize]) >> 8).max(16);
    }
    (total_error, initial_delta)
}

fn decode_block_raw(
    block: &[u8],
    channels: usize,
    samples_per_block: usize,
    output: &mut Vec<i16>,
) -> Result<(), AdpcmError> {
    let mut states = [ChannelState::default(); MAX_CHANNELS];
    let delta_offset = channels;
    let sample1_offset = delta_offset + 2 * channels;
    let sample2_offset = sample1_offset + 2 * channels;
    let nibble_offset = sample2_offset + 2 * channels;

    for channel in 0..channels {
        let predictor = block[channel];
        let Some(&(coefficient1, coefficient2)) = COEFFICIENTS.get(predictor as usize) else {
            return Err(AdpcmError::BadPredictor { index: predictor });
        };
        let delta_pos = delta_offset + 2 * channel;
        let sample1_pos = sample1_offset + 2 * channel;
        let sample2_pos = sample2_offset + 2 * channel;
        states[channel] = ChannelState {
            coefficient1,
            coefficient2,
            delta: i32::from(i16::from_le_bytes([block[delta_pos], block[delta_pos + 1]])),
            sample1: i32::from(i16::from_le_bytes([
                block[sample1_pos],
                block[sample1_pos + 1],
            ])),
            sample2: i32::from(i16::from_le_bytes([
                block[sample2_pos],
                block[sample2_pos + 1],
            ])),
        };
    }

    for state in &states[..channels] {
        output.push(state.sample2 as i16);
    }
    for state in &states[..channels] {
        output.push(state.sample1 as i16);
    }

    let mut nibble_index = 0;
    for _ in 2..samples_per_block {
        for state in &mut states[..channels] {
            let byte = block[nibble_offset + nibble_index / 2];
            let nibble = if nibble_index % 2 == 0 {
                byte >> 4
            } else {
                byte & 0xf
            };
            nibble_index += 1;
            let signed_nibble = if nibble >= 8 {
                i32::from(nibble) - 16
            } else {
                i32::from(nibble)
            };
            let predicted =
                ((state.sample1 * state.coefficient1 + state.sample2 * state.coefficient2) >> 8)
                    + signed_nibble * state.delta;
            let sample = predicted.clamp(-32768, 32767);
            output.push(sample as i16);
            state.sample2 = state.sample1;
            state.sample1 = sample;
            state.delta = ((state.delta * ADAPTATION[nibble as usize]) >> 8).max(16);
        }
    }
    Ok(())
}
