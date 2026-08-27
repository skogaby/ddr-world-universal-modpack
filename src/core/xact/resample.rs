//! Deterministic fixed-point linear resampling (preserve-pitch OFF).
//!
//! The rate engine's non-pitch-preserving mode: where the WSOLA stretcher
//! (`stretch.rs`) fits `output_frames` of source content at the source's own
//! pitch, this module reads the source at the plan's exact effective ratio
//! `source_frames / output_frames`, so pitch follows the playback rate like
//! a record player. Both forms below share one position map and one
//! interpolation helper, so the streaming state is byte-identical to the
//! whole-buffer reference by construction, not by test alone.
//!
//! The position map is piecewise, mirroring the stretcher's
//! `nominal_for_output` two-domain rule so loop seams align: output frames
//! inside the plan's loop segment map loop-relatively
//! (`output_start ↦ source_start`, approaching `source_end` at
//! `output_end`), everything else maps globally. All arithmetic is Q32
//! fixed-point with half-up/half-away rounding — fully deterministic.

use std::fmt;

use super::rate::{divide_half_away_i128, round_half_up_u128, RateError};
use super::stretch::{LoopContext, Produced, SourcePcm};

const Q32_ONE: u128 = 1u128 << 32;

#[derive(Debug, PartialEq, Eq)]
pub enum ResampleError {
    InvalidChannelCount,
    InvalidFrameCounts,
    IncompleteSourceFrame,
    OutputTooShort { actual: usize, required: usize },
    InvalidLoopContext { field: &'static str },
    ArithmeticOverflow,
    AllocationFailed,
}

impl fmt::Display for ResampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChannelCount => write!(f, "channel count must be nonzero"),
            Self::InvalidFrameCounts => {
                write!(f, "source and output frame counts must be nonzero")
            }
            Self::IncompleteSourceFrame => write!(f, "source PCM ends in a partial frame"),
            Self::OutputTooShort { actual, required } => {
                write!(
                    f,
                    "output has {actual} frames; at least {required} are required"
                )
            }
            Self::InvalidLoopContext { field } => write!(f, "invalid loop context {field}"),
            Self::ArithmeticOverflow => write!(f, "resample arithmetic overflow"),
            Self::AllocationFailed => write!(f, "resample allocation failed"),
        }
    }
}

impl std::error::Error for ResampleError {}

fn map_rate_error(_: RateError) -> ResampleError {
    ResampleError::ArithmeticOverflow
}

/// Range checks only — the resampler has no analysis window, so the
/// stretcher's minimum-length requirements do not apply.
fn validate_loop_context(
    context: LoopContext,
    source_frames: usize,
    output_frames: usize,
) -> Result<(), ResampleError> {
    if context.source_start >= context.source_end || context.source_end > source_frames {
        return Err(ResampleError::InvalidLoopContext {
            field: "source range",
        });
    }
    if context.output_start >= context.output_end || context.output_end > output_frames {
        return Err(ResampleError::InvalidLoopContext {
            field: "output range",
        });
    }
    Ok(())
}

/// Precomputed piecewise Q32 position map.
///
/// Positions come from DIRECT multiplication per output frame — never an
/// incremental accumulator — so any pull order, chunking, or seek reproduces
/// the identical positions.
#[derive(Clone, Copy, Debug)]
struct PositionMap {
    step_global: u128,
    /// `(output_start, output_end, source_start_q32, step_loop)`.
    loop_segment: Option<(usize, usize, u128, u128)>,
}

impl PositionMap {
    fn new(
        source_frames: usize,
        output_frames: usize,
        loop_context: Option<LoopContext>,
    ) -> Result<Self, ResampleError> {
        if source_frames == 0 || output_frames == 0 {
            return Err(ResampleError::InvalidFrameCounts);
        }
        let step_global = round_half_up_u128(
            (source_frames as u128)
                .checked_mul(Q32_ONE)
                .ok_or(ResampleError::ArithmeticOverflow)?,
            output_frames as u128,
        )
        .map_err(map_rate_error)?;
        let loop_segment = match loop_context {
            None => None,
            Some(context) => {
                validate_loop_context(context, source_frames, output_frames)?;
                let source_length = (context.source_end - context.source_start) as u128;
                let output_length = (context.output_end - context.output_start) as u128;
                let step_loop = round_half_up_u128(
                    source_length
                        .checked_mul(Q32_ONE)
                        .ok_or(ResampleError::ArithmeticOverflow)?,
                    output_length,
                )
                .map_err(map_rate_error)?;
                Some((
                    context.output_start,
                    context.output_end,
                    context.source_start as u128 * Q32_ONE,
                    step_loop,
                ))
            }
        };
        Ok(Self {
            step_global,
            loop_segment,
        })
    }

    /// Q32 source position of `output_frame`.
    fn position_q32(&self, output_frame: usize) -> u128 {
        if let Some((output_start, output_end, source_start_q32, step_loop)) = self.loop_segment {
            if (output_start..output_end).contains(&output_frame) {
                return source_start_q32 + (output_frame - output_start) as u128 * step_loop;
            }
        }
        output_frame as u128 * self.step_global
    }
}

/// Linear interpolation of one channel at a Q32 position, half-away rounded.
///
/// The result of interpolating between two `i16` samples always lies between
/// them, so no saturation is needed. The upper index clamps to the last
/// source frame (the final positions of a downshifted map land inside the
/// last frame).
fn interpolate(
    source: &impl SourcePcm,
    source_frames: usize,
    position_q32: u128,
    channel: usize,
) -> i16 {
    let base = ((position_q32 / Q32_ONE) as usize).min(source_frames - 1);
    let fraction = position_q32 % Q32_ONE;
    let first = source.sample(base, channel);
    if fraction == 0 {
        return first;
    }
    let second = source.sample((base + 1).min(source_frames - 1), channel);
    let delta = i128::from(second) - i128::from(first);
    let blended = divide_half_away_i128(delta * fraction as i128, Q32_ONE as i128)
        .expect("interpolation blend cannot overflow i128");
    (i128::from(first) + blended) as i16
}

/// Whole-buffer reference resampler — the frozen validation oracle.
///
/// Like `stretch_interleaved`, this form exists as the independent
/// ground truth for the streaming state and the validation harness; it is
/// never modified once landed.
pub fn resample_interleaved(
    source: &[i16],
    channels: usize,
    output_frames: usize,
    loop_context: Option<LoopContext>,
) -> Result<Vec<i16>, ResampleError> {
    if channels == 0 {
        return Err(ResampleError::InvalidChannelCount);
    }
    if source.len() % channels != 0 {
        return Err(ResampleError::IncompleteSourceFrame);
    }
    let source_frames = source.len() / channels;
    let map = PositionMap::new(source_frames, output_frames, loop_context)?;
    let view = super::stretch::SlicePcm::new(source, channels)
        .map_err(|_| ResampleError::IncompleteSourceFrame)?;
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(
            output_frames
                .checked_mul(channels)
                .ok_or(ResampleError::ArithmeticOverflow)?,
        )
        .map_err(|_| ResampleError::AllocationFailed)?;
    for frame in 0..output_frames {
        let position = map.position_q32(frame);
        for channel in 0..channels {
            samples.push(interpolate(&view, source_frames, position, channel));
        }
    }
    Ok(samples)
}

/// Resumable, pull-driven form of the reference resampler.
///
/// The state is purely positional: the next output frame is the only mutable
/// field, every emitted sample is a direct function of `(map, frame)`, and
/// `positioned_at` is therefore an O(1) seek — the resample counterpart of
/// the stretcher's checkpoint-restore machinery, with no checkpoint needed.
#[derive(Debug)]
pub struct ResampleState {
    source_frames: usize,
    output_frames: usize,
    channels: usize,
    map: PositionMap,
    next_output_frame: usize,
}

impl ResampleState {
    pub fn new(
        source_frames: usize,
        output_frames: usize,
        channels: usize,
        loop_context: Option<LoopContext>,
    ) -> Result<Self, ResampleError> {
        if channels == 0 {
            return Err(ResampleError::InvalidChannelCount);
        }
        let map = PositionMap::new(source_frames, output_frames, loop_context)?;
        Ok(Self {
            source_frames,
            output_frames,
            channels,
            map,
            next_output_frame: 0,
        })
    }

    /// Produce the next run of output frames into `out`.
    ///
    /// Mirrors `StretchState::produce`: minimum granularity is ONE FRAME
    /// (`OutputTooShort` on a zero-frame buffer — never a silent stall),
    /// only the whole-frame prefix of `out` is used, and any sequence of
    /// calls with any capacities yields the identical byte stream.
    pub fn produce(
        &mut self,
        source: &impl SourcePcm,
        out: &mut [i16],
    ) -> Result<Produced, ResampleError> {
        assert_eq!(
            source.frames(),
            self.source_frames,
            "source view frame count diverges from construction"
        );
        assert_eq!(
            source.channels(),
            self.channels,
            "source view channel count diverges from construction"
        );
        let capacity = out.len() / self.channels;
        if capacity == 0 {
            return Err(ResampleError::OutputTooShort {
                actual: 0,
                required: 1,
            });
        }
        let remaining = self.output_frames - self.next_output_frame;
        let take = remaining.min(capacity);
        for index in 0..take {
            let position = self.map.position_q32(self.next_output_frame + index);
            let offset = index * self.channels;
            for channel in 0..self.channels {
                out[offset + channel] = interpolate(source, self.source_frames, position, channel);
            }
        }
        self.next_output_frame += take;
        Ok(Produced {
            frames: take,
            done: self.next_output_frame == self.output_frames,
        })
    }

    /// Seek to an absolute output frame (clamped to the end). O(1).
    pub fn positioned_at(&mut self, output_frame: usize) {
        self.next_output_frame = output_frame.min(self.output_frames);
    }

    /// The next output frame `produce` will emit.
    #[must_use]
    pub fn position(&self) -> usize {
        self.next_output_frame
    }
}
