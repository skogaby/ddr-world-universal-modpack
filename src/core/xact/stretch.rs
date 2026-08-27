//! Deterministic joint-channel WSOLA-like time stretching.

use std::fmt;

use super::rate::{divide_half_away_i128, round_half_up_u128, RateError};

const Q32_ONE: u128 = 1u128 << 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StretchParameters {
    pub window: usize,
    pub synthesis_hop: usize,
    pub match_length: usize,
    pub search_radius: usize,
}

impl StretchParameters {
    pub fn for_sample_rate(sample_rate: u32) -> Result<Self, StretchError> {
        if sample_rate == 0 {
            return Err(StretchError::InvalidSampleRate);
        }
        let raw_window = round_parameter(sample_rate, 30, 1_000)?.max(32);
        let window = raw_window
            .checked_add(raw_window & 1)
            .ok_or(StretchError::ArithmeticOverflow)?;
        let match_length = round_parameter(sample_rate, 75, 10_000)?.max(8);
        Ok(Self {
            window,
            synthesis_hop: window / 2,
            match_length,
            search_radius: match_length,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopContext {
    pub source_start: usize,
    pub source_end: usize,
    pub output_start: usize,
    pub output_end: usize,
}

#[derive(Debug)]
pub struct StretchResult {
    pub samples: Vec<i16>,
    pub selected_source_starts: Vec<usize>,
    pub nominal_source_starts: Vec<usize>,
    pub cyclic_windows: usize,
    pub clipped_samples: usize,
    pub loop_seam_max_delta: Option<i32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StretchError {
    InvalidSampleRate,
    InvalidChannelCount,
    IncompleteSourceFrame,
    SourceTooShort { actual: usize, required: usize },
    OutputTooShort { actual: usize, required: usize },
    InvalidLoopContext { field: &'static str },
    InvalidCheckpoint { field: &'static str },
    NoCandidate,
    ArithmeticOverflow,
    ScoreOverflow,
    AllocationFailed,
    Cancelled,
}

impl fmt::Display for StretchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSampleRate => write!(f, "sample rate must be nonzero"),
            Self::InvalidChannelCount => write!(f, "channel count must be nonzero"),
            Self::IncompleteSourceFrame => write!(f, "source PCM ends in a partial frame"),
            Self::SourceTooShort { actual, required } => {
                write!(
                    f,
                    "source has {actual} frames; at least {required} are required"
                )
            }
            Self::OutputTooShort { actual, required } => {
                write!(
                    f,
                    "output has {actual} frames; at least {required} are required"
                )
            }
            Self::InvalidLoopContext { field } => write!(f, "invalid loop context {field}"),
            Self::InvalidCheckpoint { field } => write!(f, "invalid stretch checkpoint {field}"),
            Self::NoCandidate => {
                write!(f, "no valid source candidate lies within the search radius")
            }
            Self::ArithmeticOverflow => write!(f, "stretch arithmetic overflow"),
            Self::ScoreOverflow => write!(f, "joint-channel match score overflow"),
            Self::AllocationFailed => write!(f, "stretch allocation failed"),
            Self::Cancelled => write!(f, "stretch operation cancelled"),
        }
    }
}

impl std::error::Error for StretchError {}

#[derive(Clone, Copy, Debug)]
struct SourceWindow {
    start: usize,
    cyclic: bool,
}

/// Random-access view of decoded interleaved source PCM.
///
/// The streaming stretcher consumes its source through this trait instead of
/// a whole decoded buffer; implementations decode on demand (see
/// `adpcm::BlockCachePcm`). Implementations MUST panic on out-of-range access
/// — mirroring the reference stretcher's slice indexing — rather than return
/// silent garbage; fallible validation belongs to construction.
pub trait SourcePcm {
    /// Logical frame count of the source (duration-clamped).
    fn frames(&self) -> usize;

    /// Interleaved channel count (nonzero).
    fn channels(&self) -> usize;

    /// One sample. Panics if `frame >= frames()` or `channel >= channels()`.
    fn sample(&self, frame: usize, channel: usize) -> i16;
}

/// Trivial in-memory [`SourcePcm`] over an interleaved PCM slice.
pub struct SlicePcm<'a> {
    samples: &'a [i16],
    channels: usize,
}

impl<'a> SlicePcm<'a> {
    pub fn new(samples: &'a [i16], channels: usize) -> Result<Self, StretchError> {
        if channels == 0 {
            return Err(StretchError::InvalidChannelCount);
        }
        if samples.len() % channels != 0 {
            return Err(StretchError::IncompleteSourceFrame);
        }
        Ok(Self { samples, channels })
    }
}

impl SourcePcm for SlicePcm<'_> {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn sample(&self, frame: usize, channel: usize) -> i16 {
        assert!(frame < self.frames(), "frame {frame} is out of range");
        assert!(
            channel < self.channels,
            "channel {channel} is out of range for {} channels",
            self.channels
        );
        self.samples[frame * self.channels + channel]
    }
}

pub fn stretch_interleaved(
    source: &[i16],
    channels: usize,
    sample_rate: u32,
    output_frames: usize,
    loop_context: Option<LoopContext>,
) -> Result<StretchResult, StretchError> {
    stretch_interleaved_with(
        source,
        channels,
        sample_rate,
        output_frames,
        loop_context,
        &mut || true,
    )
}

pub fn stretch_interleaved_with(
    source: &[i16],
    channels: usize,
    sample_rate: u32,
    output_frames: usize,
    loop_context: Option<LoopContext>,
    should_continue: &mut impl FnMut() -> bool,
) -> Result<StretchResult, StretchError> {
    if channels == 0 {
        return Err(StretchError::InvalidChannelCount);
    }
    if source.len() % channels != 0 {
        return Err(StretchError::IncompleteSourceFrame);
    }
    let parameters = StretchParameters::for_sample_rate(sample_rate)?;
    let source_frames = source.len() / channels;
    let required_source = parameters
        .window
        .checked_add(parameters.search_radius)
        .ok_or(StretchError::ArithmeticOverflow)?;
    if source_frames < required_source {
        return Err(StretchError::SourceTooShort {
            actual: source_frames,
            required: required_source,
        });
    }
    let required_output = parameters
        .window
        .checked_add(parameters.synthesis_hop)
        .ok_or(StretchError::ArithmeticOverflow)?;
    if output_frames < required_output {
        return Err(StretchError::OutputTooShort {
            actual: output_frames,
            required: required_output,
        });
    }
    if let Some(context) = loop_context {
        validate_loop_context(context, source_frames, output_frames, parameters)?;
    }
    if !should_continue() {
        return Err(StretchError::Cancelled);
    }

    if output_frames == source_frames {
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(source.len())
            .map_err(|_| StretchError::AllocationFailed)?;
        samples.extend_from_slice(source);
        let terminal_source_start = source_frames - parameters.window;
        let loop_seam_max_delta = loop_context.map(|context| {
            seam_max_delta(&samples, channels, context.output_start, context.output_end)
        });
        return Ok(StretchResult {
            samples,
            selected_source_starts: vec![0, terminal_source_start],
            nominal_source_starts: vec![0, terminal_source_start],
            cyclic_windows: 0,
            clipped_samples: 0,
            loop_seam_max_delta,
        });
    }

    let output_samples = output_frames
        .checked_mul(channels)
        .ok_or(StretchError::ArithmeticOverflow)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_samples)
        .map_err(|_| StretchError::AllocationFailed)?;
    output.resize(output_samples, 0);

    let terminal_output_start = output_frames - parameters.window;
    let estimated_windows = terminal_output_start
        .checked_div(parameters.synthesis_hop)
        .and_then(|windows| windows.checked_add(2))
        .ok_or(StretchError::ArithmeticOverflow)?;
    let mut selected_source_starts = Vec::new();
    selected_source_starts
        .try_reserve_exact(estimated_windows)
        .map_err(|_| StretchError::AllocationFailed)?;
    let mut nominal_source_starts = Vec::new();
    nominal_source_starts
        .try_reserve_exact(estimated_windows)
        .map_err(|_| StretchError::AllocationFailed)?;

    let first = SourceWindow {
        start: 0,
        cyclic: false,
    };
    if !should_continue() {
        return Err(StretchError::Cancelled);
    }
    copy_window(
        source,
        channels,
        first,
        loop_context,
        parameters.window,
        &mut output[..parameters.window * channels],
    );
    selected_source_starts.push(0);
    nominal_source_starts.push(0);

    let phase_step = q32_phase_step(parameters.synthesis_hop, source_frames, output_frames)?;
    let mut phase = phase_step;
    let mut output_start = parameters.synthesis_hop;
    let mut previous = first;
    let mut cyclic_windows = 0;
    let mut clipped_samples = 0;

    while output_start < terminal_output_start {
        if !should_continue() {
            return Err(StretchError::Cancelled);
        }
        let global_nominal = q32_to_frame(phase)?;
        let (nominal, cyclic) = nominal_for_output(output_start, global_nominal, loop_context)?;
        let selected = choose_candidate(
            source,
            channels,
            source_frames,
            previous,
            nominal,
            cyclic,
            loop_context,
            parameters,
        )?;
        if selected.cyclic && window_wraps(selected.start, parameters.window, loop_context) {
            cyclic_windows += 1;
        }
        overlap_window(
            source,
            channels,
            selected,
            loop_context,
            parameters,
            output_start,
            &mut output,
            &mut clipped_samples,
        )?;
        selected_source_starts.push(selected.start);
        nominal_source_starts.push(nominal);
        previous = selected;
        output_start = output_start
            .checked_add(parameters.synthesis_hop)
            .ok_or(StretchError::ArithmeticOverflow)?;
        phase = phase
            .checked_add(phase_step)
            .ok_or(StretchError::ArithmeticOverflow)?;
    }

    let terminal_source_start = source_frames - parameters.window;
    let terminal = SourceWindow {
        start: terminal_source_start,
        cyclic: false,
    };
    if !should_continue() {
        return Err(StretchError::Cancelled);
    }
    overlap_window(
        source,
        channels,
        terminal,
        loop_context,
        parameters,
        terminal_output_start,
        &mut output,
        &mut clipped_samples,
    )?;
    selected_source_starts.push(terminal_source_start);
    nominal_source_starts.push(terminal_source_start);

    let loop_seam_max_delta = loop_context
        .map(|context| seam_max_delta(&output, channels, context.output_start, context.output_end));
    Ok(StretchResult {
        samples: output,
        selected_source_starts,
        nominal_source_starts,
        cyclic_windows,
        clipped_samples,
        loop_seam_max_delta,
    })
}

fn round_parameter(
    sample_rate: u32,
    numerator: u32,
    denominator: u32,
) -> Result<usize, StretchError> {
    let value = round_half_up_u128(
        u128::from(sample_rate) * u128::from(numerator),
        u128::from(denominator),
    )
    .map_err(map_rate_error)?;
    usize::try_from(value).map_err(|_| StretchError::ArithmeticOverflow)
}

fn q32_phase_step(
    synthesis_hop: usize,
    source_frames: usize,
    output_frames: usize,
) -> Result<u128, StretchError> {
    let numerator = (synthesis_hop as u128)
        .checked_mul(source_frames as u128)
        .and_then(|value| value.checked_mul(Q32_ONE))
        .ok_or(StretchError::ArithmeticOverflow)?;
    round_half_up_u128(numerator, output_frames as u128).map_err(map_rate_error)
}

fn q32_to_frame(phase: u128) -> Result<usize, StretchError> {
    let frame = phase
        .checked_add(Q32_ONE / 2)
        .ok_or(StretchError::ArithmeticOverflow)?
        / Q32_ONE;
    usize::try_from(frame).map_err(|_| StretchError::ArithmeticOverflow)
}

fn nominal_for_output(
    output_start: usize,
    global_nominal: usize,
    loop_context: Option<LoopContext>,
) -> Result<(usize, bool), StretchError> {
    let Some(context) = loop_context else {
        return Ok((global_nominal, false));
    };
    if !(context.output_start..context.output_end).contains(&output_start) {
        return Ok((global_nominal, false));
    }
    let relative_output = output_start - context.output_start;
    let source_length = context.source_end - context.source_start;
    let output_length = context.output_end - context.output_start;
    let mapped = round_half_up_u128(
        (relative_output as u128)
            .checked_mul(source_length as u128)
            .ok_or(StretchError::ArithmeticOverflow)?,
        output_length as u128,
    )
    .map_err(map_rate_error)?;
    let mapped = usize::try_from(mapped).map_err(|_| StretchError::ArithmeticOverflow)?;
    Ok((context.source_start + mapped.min(source_length - 1), true))
}

#[allow(clippy::too_many_arguments)]
fn choose_candidate(
    source: &[i16],
    channels: usize,
    source_frames: usize,
    previous: SourceWindow,
    nominal: usize,
    cyclic: bool,
    loop_context: Option<LoopContext>,
    parameters: StretchParameters,
) -> Result<SourceWindow, StretchError> {
    let (minimum, maximum) = if cyclic {
        let context = loop_context.ok_or(StretchError::InvalidLoopContext {
            field: "missing cyclic range",
        })?;
        (
            nominal
                .saturating_sub(parameters.search_radius)
                .max(context.source_start),
            nominal
                .checked_add(parameters.search_radius)
                .ok_or(StretchError::ArithmeticOverflow)?
                .min(context.source_end - 1),
        )
    } else {
        let latest = source_frames - parameters.window;
        (
            nominal.saturating_sub(parameters.search_radius),
            nominal
                .checked_add(parameters.search_radius)
                .ok_or(StretchError::ArithmeticOverflow)?
                .min(latest),
        )
    };
    if minimum > maximum {
        return Err(StretchError::NoCandidate);
    }

    let reference = SourceWindow {
        start: previous
            .start
            .checked_add(parameters.synthesis_hop)
            .ok_or(StretchError::ArithmeticOverflow)?,
        cyclic: previous.cyclic,
    };
    let mut best: Option<(i64, usize, usize)> = None;
    for candidate in minimum..=maximum {
        let window = SourceWindow {
            start: candidate,
            cyclic,
        };
        let score = joint_sad(
            source,
            channels,
            reference,
            window,
            loop_context,
            parameters.match_length,
        )?;
        let rank = (score, candidate.abs_diff(nominal), candidate);
        if best.is_none_or(|current| {
            candidate_precedes(score, candidate, current.0, current.2, nominal)
        }) {
            best = Some(rank);
        }
    }
    let (_, _, start) = best.ok_or(StretchError::InvalidLoopContext {
        field: "empty candidate range",
    })?;
    Ok(SourceWindow { start, cyclic })
}

#[must_use]
pub(crate) fn candidate_precedes(
    score: i64,
    source_start: usize,
    best_score: i64,
    best_source_start: usize,
    nominal: usize,
) -> bool {
    (score, source_start.abs_diff(nominal), source_start)
        < (
            best_score,
            best_source_start.abs_diff(nominal),
            best_source_start,
        )
}

fn joint_sad(
    source: &[i16],
    channels: usize,
    reference: SourceWindow,
    candidate: SourceWindow,
    loop_context: Option<LoopContext>,
    match_length: usize,
) -> Result<i64, StretchError> {
    let mut score = 0i64;
    for frame in 0..match_length {
        for channel in 0..channels {
            let left = source_sample(source, channels, reference, frame, channel, loop_context);
            let right = source_sample(source, channels, candidate, frame, channel, loop_context);
            score = score
                .checked_add((i64::from(left) - i64::from(right)).abs())
                .ok_or(StretchError::ScoreOverflow)?;
        }
    }
    Ok(score)
}

#[allow(clippy::too_many_arguments)]
fn overlap_window(
    source: &[i16],
    channels: usize,
    window: SourceWindow,
    loop_context: Option<LoopContext>,
    parameters: StretchParameters,
    output_start: usize,
    output: &mut [i16],
    clipped_samples: &mut usize,
) -> Result<(), StretchError> {
    for frame in 0..parameters.synthesis_hop {
        for channel in 0..channels {
            let output_index = (output_start + frame) * channels + channel;
            let old = i128::from(output[output_index]);
            let new = i128::from(source_sample(
                source,
                channels,
                window,
                frame,
                channel,
                loop_context,
            ));
            let old_weight = (parameters.synthesis_hop - frame) as i128;
            let new_weight = frame as i128;
            let numerator = old
                .checked_mul(old_weight)
                .and_then(|value| value.checked_add(new * new_weight))
                .ok_or(StretchError::ArithmeticOverflow)?;
            let mixed = divide_half_away_i128(numerator, parameters.synthesis_hop as i128)
                .map_err(map_rate_error)?;
            if mixed < i128::from(i16::MIN) || mixed > i128::from(i16::MAX) {
                *clipped_samples += 1;
            }
            output[output_index] = mixed.clamp(i128::from(i16::MIN), i128::from(i16::MAX)) as i16;
        }
    }
    for frame in parameters.synthesis_hop..parameters.window {
        for channel in 0..channels {
            let output_index = (output_start + frame) * channels + channel;
            output[output_index] =
                source_sample(source, channels, window, frame, channel, loop_context);
        }
    }
    Ok(())
}

fn copy_window(
    source: &[i16],
    channels: usize,
    window: SourceWindow,
    loop_context: Option<LoopContext>,
    frames: usize,
    output: &mut [i16],
) {
    for frame in 0..frames {
        for channel in 0..channels {
            output[frame * channels + channel] =
                source_sample(source, channels, window, frame, channel, loop_context);
        }
    }
}

fn source_sample(
    source: &[i16],
    channels: usize,
    window: SourceWindow,
    frame_offset: usize,
    channel: usize,
    loop_context: Option<LoopContext>,
) -> i16 {
    let mut frame = window.start + frame_offset;
    if window.cyclic {
        if let Some(context) = loop_context {
            let loop_length = context.source_end - context.source_start;
            frame = context.source_start + (frame - context.source_start) % loop_length;
        }
    }
    source[frame * channels + channel]
}

fn window_wraps(start: usize, window: usize, loop_context: Option<LoopContext>) -> bool {
    loop_context.is_some_and(|context| start + window > context.source_end)
}

fn validate_loop_context(
    context: LoopContext,
    source_frames: usize,
    output_frames: usize,
    parameters: StretchParameters,
) -> Result<(), StretchError> {
    if context.source_start >= context.source_end || context.source_end > source_frames {
        return Err(StretchError::InvalidLoopContext {
            field: "source range",
        });
    }
    if context.output_start >= context.output_end || context.output_end > output_frames {
        return Err(StretchError::InvalidLoopContext {
            field: "output range",
        });
    }
    if context.source_end - context.source_start < parameters.window {
        return Err(StretchError::InvalidLoopContext {
            field: "source range too short",
        });
    }
    if context.output_end - context.output_start < parameters.window {
        return Err(StretchError::InvalidLoopContext {
            field: "output range too short",
        });
    }
    Ok(())
}

fn seam_max_delta(samples: &[i16], channels: usize, loop_start: usize, loop_end: usize) -> i32 {
    (0..channels)
        .map(|channel| {
            let first = i32::from(samples[loop_start * channels + channel]);
            let last = i32::from(samples[(loop_end - 1) * channels + channel]);
            (first - last).abs()
        })
        .max()
        .unwrap_or(0)
}

fn map_rate_error(error: RateError) -> StretchError {
    match error {
        RateError::ArithmeticOverflow => StretchError::ArithmeticOverflow,
        _ => StretchError::ArithmeticOverflow,
    }
}

// ---------------------------------------------------------------------------
// Streaming WSOLA state machine (design reqs 19–20).
//
// `stretch_interleaved_with` above is the REFERENCE implementation and test
// oracle — it is never modified. The resumable machine below reproduces its
// output byte-for-byte, generating whole synthesis hops incrementally and
// emitting only FINALIZED frames: an event that writes [p, p+window) blends
// only its first synthesis_hop frames, so frames below the NEXT event's
// position are final and the provisional tail is retained internally.
// ---------------------------------------------------------------------------

/// Result of one [`StretchState::produce`] call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Produced {
    /// Whole output frames written to the front of `out`.
    pub frames: usize,
    /// True once every output frame has been handed to the caller.
    pub done: bool,
}

/// Snapshot of the tiny per-step state at a main-event boundary.
///
/// `resume_frame` is the output frame the restored state regenerates from;
/// everything the machine needs beyond these five words is either a
/// construction parameter or recomputable (`phase` is exactly
/// `phase_step · (resume_frame / synthesis_hop)`, and the provisional tail is
/// the previous window's direct-copy region, rebuilt from the source).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StretchCheckpoint {
    resume_frame: usize,
    previous_start: usize,
    previous_cyclic: bool,
    cyclic_windows: usize,
    clipped_samples: usize,
}

impl StretchCheckpoint {
    /// The output frame index the restored run reproduces from.
    #[must_use]
    pub fn resume_frame(&self) -> usize {
        self.resume_frame
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StretchStage {
    /// `output_frames == source_frames`: pure incremental copy.
    Identity,
    /// The first-window copy at output position 0 has not run yet.
    FirstCopy,
    /// Main overlap events at `output_start` (< terminal position).
    Main,
    /// Only the terminal end-anchor event remains.
    Terminal,
    /// All output frames generated (emission may still be draining).
    Done,
}

/// Resumable, pull-driven reformulation of the whole-buffer stretcher.
#[derive(Debug)]
pub struct StretchState {
    parameters: StretchParameters,
    source_frames: usize,
    output_frames: usize,
    channels: usize,
    loop_context: Option<LoopContext>,
    phase_step: u128,
    phase: u128,
    output_start: usize,
    previous: SourceWindow,
    cyclic_windows: usize,
    clipped_samples: usize,
    stage: StretchStage,
    /// Absolute frame index of `buffer[0]`; frames below have been emitted.
    emit_frame: usize,
    /// Absolute frame index up to which generated output is final.
    finalized_frame: usize,
    /// Interleaved samples for frames `[emit_frame, emit_frame + len/channels)`.
    buffer: Vec<i16>,
}

impl StretchState {
    /// Validates exactly like the reference (same errors, same order) and
    /// starts at output frame 0.
    pub fn new(
        source_frames: usize,
        output_frames: usize,
        channels: usize,
        sample_rate: u32,
        loop_context: Option<LoopContext>,
    ) -> Result<Self, StretchError> {
        if channels == 0 {
            return Err(StretchError::InvalidChannelCount);
        }
        let parameters = StretchParameters::for_sample_rate(sample_rate)?;
        let required_source = parameters
            .window
            .checked_add(parameters.search_radius)
            .ok_or(StretchError::ArithmeticOverflow)?;
        if source_frames < required_source {
            return Err(StretchError::SourceTooShort {
                actual: source_frames,
                required: required_source,
            });
        }
        let required_output = parameters
            .window
            .checked_add(parameters.synthesis_hop)
            .ok_or(StretchError::ArithmeticOverflow)?;
        if output_frames < required_output {
            return Err(StretchError::OutputTooShort {
                actual: output_frames,
                required: required_output,
            });
        }
        if let Some(context) = loop_context {
            validate_loop_context(context, source_frames, output_frames, parameters)?;
        }
        let identity = output_frames == source_frames;
        let phase_step = if identity {
            0
        } else {
            q32_phase_step(parameters.synthesis_hop, source_frames, output_frames)?
        };
        Ok(Self {
            parameters,
            source_frames,
            output_frames,
            channels,
            loop_context,
            phase_step,
            phase: phase_step,
            output_start: parameters.synthesis_hop,
            previous: SourceWindow {
                start: 0,
                cyclic: false,
            },
            cyclic_windows: 0,
            clipped_samples: 0,
            stage: if identity {
                StretchStage::Identity
            } else {
                StretchStage::FirstCopy
            },
            emit_frame: 0,
            finalized_frame: 0,
            buffer: Vec::new(),
        })
    }

    /// Produce the next run of finalized output frames into `out`.
    ///
    /// Minimum granularity is ONE FRAME: `out` must hold at least `channels`
    /// samples (otherwise `OutputTooShort` — never a silent stall); only the
    /// whole-frame prefix of `out` is used. Any sequence of calls with any
    /// capacities yields the identical byte stream.
    pub fn produce(
        &mut self,
        source: &impl SourcePcm,
        out: &mut [i16],
    ) -> Result<Produced, StretchError> {
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
            return Err(StretchError::OutputTooShort {
                actual: 0,
                required: 1,
            });
        }
        let mut written = 0usize;
        loop {
            let available = self.finalized_frame - self.emit_frame;
            if available > 0 && written < capacity {
                let take = available.min(capacity - written);
                let samples = take * self.channels;
                let offset = written * self.channels;
                out[offset..offset + samples].copy_from_slice(&self.buffer[..samples]);
                self.buffer.drain(..samples);
                self.emit_frame += take;
                written += take;
            }
            let done = self.emit_frame == self.output_frames;
            if written == capacity || done {
                return Ok(Produced {
                    frames: written,
                    done,
                });
            }
            self.advance(source)?;
        }
    }

    /// Snapshot the state at the current event boundary, when reconstructible.
    ///
    /// Returns `None` once the terminal end-anchor region has begun: its blend
    /// input has mixed provenance and cannot be rebuilt from
    /// `(previous window, source)` alone. Every earlier boundary — including
    /// the initial (zero) state — is reconstructible.
    pub fn checkpoint(&self) -> Option<StretchCheckpoint> {
        match self.stage {
            StretchStage::Identity => Some(StretchCheckpoint {
                resume_frame: self.finalized_frame,
                previous_start: 0,
                previous_cyclic: false,
                cyclic_windows: 0,
                clipped_samples: 0,
            }),
            StretchStage::FirstCopy => Some(StretchCheckpoint {
                resume_frame: 0,
                previous_start: 0,
                previous_cyclic: false,
                cyclic_windows: 0,
                clipped_samples: 0,
            }),
            StretchStage::Main => {
                debug_assert_eq!(self.finalized_frame, self.output_start);
                Some(StretchCheckpoint {
                    resume_frame: self.output_start,
                    previous_start: self.previous.start,
                    previous_cyclic: self.previous.cyclic,
                    cyclic_windows: self.cyclic_windows,
                    clipped_samples: self.clipped_samples,
                })
            }
            StretchStage::Terminal | StretchStage::Done => None,
        }
    }

    /// Rebuild a state from `checkpoint`; the run regenerates output from
    /// `checkpoint.resume_frame()` onward, byte-identical to an uninterrupted
    /// run's suffix.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        checkpoint: &StretchCheckpoint,
        source_frames: usize,
        output_frames: usize,
        channels: usize,
        sample_rate: u32,
        loop_context: Option<LoopContext>,
        source: &impl SourcePcm,
    ) -> Result<Self, StretchError> {
        let mut state = Self::new(
            source_frames,
            output_frames,
            channels,
            sample_rate,
            loop_context,
        )?;
        assert_eq!(
            source.frames(),
            source_frames,
            "source view frame count diverges from construction"
        );
        assert_eq!(
            source.channels(),
            channels,
            "source view channel count diverges from construction"
        );
        let resume = checkpoint.resume_frame;
        if resume == 0 {
            if checkpoint.cyclic_windows != 0 || checkpoint.clipped_samples != 0 {
                return Err(StretchError::InvalidCheckpoint {
                    field: "zero-resume counters",
                });
            }
            return Ok(state);
        }
        if matches!(state.stage, StretchStage::Identity) {
            if resume > output_frames {
                return Err(StretchError::InvalidCheckpoint {
                    field: "identity resume frame",
                });
            }
            state.emit_frame = resume;
            state.finalized_frame = resume;
            if resume == output_frames {
                state.stage = StretchStage::Done;
            }
            return Ok(state);
        }
        let hop = state.parameters.synthesis_hop;
        let window = state.parameters.window;
        let terminal = output_frames - window;
        if resume % hop != 0 || resume < hop || resume >= terminal {
            return Err(StretchError::InvalidCheckpoint {
                field: "resume frame",
            });
        }
        if checkpoint.previous_cyclic {
            let context = loop_context.ok_or(StretchError::InvalidCheckpoint {
                field: "cyclic window without loop context",
            })?;
            if checkpoint.previous_start < context.source_start
                || checkpoint.previous_start >= context.source_end
            {
                return Err(StretchError::InvalidCheckpoint {
                    field: "previous window",
                });
            }
        } else if checkpoint.previous_start > source_frames - window {
            return Err(StretchError::InvalidCheckpoint {
                field: "previous window",
            });
        }
        // phase advances by exactly phase_step per main event, so the value at
        // a main boundary is fully determined by the resume position.
        let steps = (resume / hop) as u128;
        state.phase = state
            .phase_step
            .checked_mul(steps)
            .ok_or(StretchError::ArithmeticOverflow)?;
        state.output_start = resume;
        state.previous = SourceWindow {
            start: checkpoint.previous_start,
            cyclic: checkpoint.previous_cyclic,
        };
        state.cyclic_windows = checkpoint.cyclic_windows;
        state.clipped_samples = checkpoint.clipped_samples;
        state.stage = StretchStage::Main;
        state.emit_frame = resume;
        state.finalized_frame = resume;
        // Rebuild the provisional tail [resume, resume + hop): the previous
        // event's direct-copy region, i.e. its window frames [hop, window).
        state.ensure_buffer_through(resume + hop);
        for frame in 0..hop {
            for channel in 0..channels {
                let index = state.buffer_index(resume + frame, channel);
                state.buffer[index] =
                    view_sample(source, state.previous, hop + frame, channel, loop_context);
            }
        }
        Ok(state)
    }

    #[must_use]
    pub fn clipped_samples(&self) -> usize {
        self.clipped_samples
    }

    #[must_use]
    pub fn cyclic_windows(&self) -> usize {
        self.cyclic_windows
    }

    fn terminal_output_start(&self) -> usize {
        self.output_frames - self.parameters.window
    }

    fn buffer_index(&self, absolute_frame: usize, channel: usize) -> usize {
        (absolute_frame - self.emit_frame) * self.channels + channel
    }

    fn ensure_buffer_through(&mut self, end_frame: usize) {
        let needed = (end_frame - self.emit_frame) * self.channels;
        if self.buffer.len() < needed {
            self.buffer.resize(needed, 0);
        }
    }

    /// Run exactly one generation event; every event finalizes at least one
    /// new output frame, so `produce` can never stall.
    fn advance(&mut self, source: &impl SourcePcm) -> Result<(), StretchError> {
        match self.stage {
            StretchStage::Identity => {
                let start = self.finalized_frame;
                let end = (start + self.parameters.window).min(self.output_frames);
                self.ensure_buffer_through(end);
                for frame in start..end {
                    for channel in 0..self.channels {
                        let index = self.buffer_index(frame, channel);
                        self.buffer[index] = source.sample(frame, channel);
                    }
                }
                self.finalized_frame = end;
                if end == self.output_frames {
                    self.stage = StretchStage::Done;
                }
                Ok(())
            }
            StretchStage::FirstCopy => {
                let window = self.parameters.window;
                let hop = self.parameters.synthesis_hop;
                let first = SourceWindow {
                    start: 0,
                    cyclic: false,
                };
                self.ensure_buffer_through(window);
                for frame in 0..window {
                    for channel in 0..self.channels {
                        let index = self.buffer_index(frame, channel);
                        self.buffer[index] =
                            view_sample(source, first, frame, channel, self.loop_context);
                    }
                }
                self.finalized_frame = hop;
                self.stage = if hop < self.terminal_output_start() {
                    StretchStage::Main
                } else {
                    StretchStage::Terminal
                };
                Ok(())
            }
            StretchStage::Main => {
                let parameters = self.parameters;
                let position = self.output_start;
                let global_nominal = q32_to_frame(self.phase)?;
                let (nominal, cyclic) =
                    nominal_for_output(position, global_nominal, self.loop_context)?;
                let selected = view_choose_candidate(
                    source,
                    self.channels,
                    self.source_frames,
                    self.previous,
                    nominal,
                    cyclic,
                    self.loop_context,
                    parameters,
                )?;
                if selected.cyclic
                    && window_wraps(selected.start, parameters.window, self.loop_context)
                {
                    self.cyclic_windows += 1;
                }
                self.overlap_into_buffer(source, selected, position)?;
                self.previous = selected;
                self.output_start = position
                    .checked_add(parameters.synthesis_hop)
                    .ok_or(StretchError::ArithmeticOverflow)?;
                self.phase = self
                    .phase
                    .checked_add(self.phase_step)
                    .ok_or(StretchError::ArithmeticOverflow)?;
                let terminal = self.terminal_output_start();
                if self.output_start < terminal {
                    self.finalized_frame = self.output_start;
                } else {
                    self.finalized_frame = terminal;
                    self.stage = StretchStage::Terminal;
                }
                Ok(())
            }
            StretchStage::Terminal => {
                let terminal_start = self.terminal_output_start();
                let terminal = SourceWindow {
                    start: self.source_frames - self.parameters.window,
                    cyclic: false,
                };
                self.overlap_into_buffer(source, terminal, terminal_start)?;
                self.finalized_frame = self.output_frames;
                self.stage = StretchStage::Done;
                Ok(())
            }
            StretchStage::Done => {
                debug_assert!(false, "advance called on a completed stretch");
                Ok(())
            }
        }
    }

    /// Exact mirror of the reference `overlap_window`, writing into the
    /// internal buffer: blend the first `synthesis_hop` frames against the
    /// retained content, direct-copy the rest.
    fn overlap_into_buffer(
        &mut self,
        source: &impl SourcePcm,
        window: SourceWindow,
        output_start: usize,
    ) -> Result<(), StretchError> {
        let parameters = self.parameters;
        self.ensure_buffer_through(output_start + parameters.window);
        for frame in 0..parameters.synthesis_hop {
            for channel in 0..self.channels {
                let index = self.buffer_index(output_start + frame, channel);
                let old = i128::from(self.buffer[index]);
                let new = i128::from(view_sample(
                    source,
                    window,
                    frame,
                    channel,
                    self.loop_context,
                ));
                let old_weight = (parameters.synthesis_hop - frame) as i128;
                let new_weight = frame as i128;
                let numerator = old
                    .checked_mul(old_weight)
                    .and_then(|value| value.checked_add(new * new_weight))
                    .ok_or(StretchError::ArithmeticOverflow)?;
                let mixed = divide_half_away_i128(numerator, parameters.synthesis_hop as i128)
                    .map_err(map_rate_error)?;
                if mixed < i128::from(i16::MIN) || mixed > i128::from(i16::MAX) {
                    self.clipped_samples += 1;
                }
                self.buffer[index] = mixed.clamp(i128::from(i16::MIN), i128::from(i16::MAX)) as i16;
            }
        }
        for frame in parameters.synthesis_hop..parameters.window {
            for channel in 0..self.channels {
                let index = self.buffer_index(output_start + frame, channel);
                self.buffer[index] = view_sample(source, window, frame, channel, self.loop_context);
            }
        }
        Ok(())
    }
}

/// Mirror of the reference `source_sample` reading through a [`SourcePcm`].
fn view_sample(
    source: &impl SourcePcm,
    window: SourceWindow,
    frame_offset: usize,
    channel: usize,
    loop_context: Option<LoopContext>,
) -> i16 {
    let mut frame = window.start + frame_offset;
    if window.cyclic {
        if let Some(context) = loop_context {
            let loop_length = context.source_end - context.source_start;
            frame = context.source_start + (frame - context.source_start) % loop_length;
        }
    }
    source.sample(frame, channel)
}

/// Mirror of the reference `joint_sad` reading through a [`SourcePcm`].
fn view_joint_sad(
    source: &impl SourcePcm,
    channels: usize,
    reference: SourceWindow,
    candidate: SourceWindow,
    loop_context: Option<LoopContext>,
    match_length: usize,
) -> Result<i64, StretchError> {
    let mut score = 0i64;
    for frame in 0..match_length {
        for channel in 0..channels {
            let left = view_sample(source, reference, frame, channel, loop_context);
            let right = view_sample(source, candidate, frame, channel, loop_context);
            score = score
                .checked_add((i64::from(left) - i64::from(right)).abs())
                .ok_or(StretchError::ScoreOverflow)?;
        }
    }
    Ok(score)
}

/// Mirror of the reference `choose_candidate` reading through a [`SourcePcm`].
#[allow(clippy::too_many_arguments)]
fn view_choose_candidate(
    source: &impl SourcePcm,
    channels: usize,
    source_frames: usize,
    previous: SourceWindow,
    nominal: usize,
    cyclic: bool,
    loop_context: Option<LoopContext>,
    parameters: StretchParameters,
) -> Result<SourceWindow, StretchError> {
    let (minimum, maximum) = if cyclic {
        let context = loop_context.ok_or(StretchError::InvalidLoopContext {
            field: "missing cyclic range",
        })?;
        (
            nominal
                .saturating_sub(parameters.search_radius)
                .max(context.source_start),
            nominal
                .checked_add(parameters.search_radius)
                .ok_or(StretchError::ArithmeticOverflow)?
                .min(context.source_end - 1),
        )
    } else {
        let latest = source_frames - parameters.window;
        (
            nominal.saturating_sub(parameters.search_radius),
            nominal
                .checked_add(parameters.search_radius)
                .ok_or(StretchError::ArithmeticOverflow)?
                .min(latest),
        )
    };
    if minimum > maximum {
        return Err(StretchError::NoCandidate);
    }

    let reference = SourceWindow {
        start: previous
            .start
            .checked_add(parameters.synthesis_hop)
            .ok_or(StretchError::ArithmeticOverflow)?,
        cyclic: previous.cyclic,
    };
    let mut best: Option<(i64, usize, usize)> = None;
    for candidate in minimum..=maximum {
        let window = SourceWindow {
            start: candidate,
            cyclic,
        };
        let score = view_joint_sad(
            source,
            channels,
            reference,
            window,
            loop_context,
            parameters.match_length,
        )?;
        let rank = (score, candidate.abs_diff(nominal), candidate);
        if best.is_none_or(|current| {
            candidate_precedes(score, candidate, current.0, current.2, nominal)
        }) {
            best = Some(rank);
        }
    }
    let (_, _, start) = best.ok_or(StretchError::InvalidLoopContext {
        field: "empty candidate range",
    })?;
    Ok(SourceWindow { start, cyclic })
}

// ---------------------------------------------------------------------------
// Seeded stretch (training design §4.5 amendment, 2026-08-13).
//
// A `shift > 0` mapping epoch in pitch-preserved mode is served by a FRESH
// stretch seeded at the shift-mapped source position — O(1), never a slice
// of the canonical whole-song stream (whose bytes at output P require the
// full WSOLA alignment chain up to P). Frame count and duration are exact
// by construction: the seeded run emits exactly `output_frames − seek_frame`
// frames. Byte-level alignment is deliberately unpinned ACROSS epochs
// (imperceptible over a seek's cue stop/replay discontinuity); WITHIN one
// epoch the seeded run is the deterministic byte authority. Seeded runs
// play linearly and expose no loop-restart anchor (that belongs to the
// canonical `{0, 0}` stream); their candidate search runs over a full-tail
// cyclic ALIGNMENT context — see `seeded_alignment_context`.
// ---------------------------------------------------------------------------

/// Offset view over another [`SourcePcm`]: frame `f` reads inner frame
/// `offset + f` — the seeded stretch's window onto the source suffix.
pub struct TailPcm<'a, P: SourcePcm> {
    inner: &'a P,
    offset: usize,
}

impl<'a, P: SourcePcm> TailPcm<'a, P> {
    /// Panics if `offset` exceeds the inner view's frame count — mirroring
    /// the [`SourcePcm`] contract (fallible validation belongs to the
    /// seeded constructor, which derives `offset` from validated bounds).
    pub fn new(inner: &'a P, offset: usize) -> Self {
        assert!(
            offset <= inner.frames(),
            "tail offset {offset} exceeds {} source frames",
            inner.frames()
        );
        Self { inner, offset }
    }
}

impl<P: SourcePcm> SourcePcm for TailPcm<'_, P> {
    fn frames(&self) -> usize {
        self.inner.frames() - self.offset
    }

    fn channels(&self) -> usize {
        self.inner.channels()
    }

    fn sample(&self, frame: usize, channel: usize) -> i16 {
        assert!(frame < self.frames(), "frame {frame} is out of range");
        self.inner.sample(self.offset + frame, channel)
    }
}

/// The seeded sub-problem boundary for a seek to output frame `seek_frame`:
/// the run's start frame (backed off from `seek_frame` just far enough that
/// the source/output TAILS satisfy the stretcher's minimum requirements —
/// near-end seeks only; mid-song `start == seek_frame`) and the half-up
/// mapped source start. Shared by the reference and streaming forms — this
/// IS the boundary spec.
fn seeded_bounds(
    source_frames: usize,
    output_frames: usize,
    seek_frame: usize,
    parameters: StretchParameters,
) -> Result<(usize, usize), StretchError> {
    let required_source = parameters
        .window
        .checked_add(parameters.search_radius)
        .ok_or(StretchError::ArithmeticOverflow)?;
    if source_frames < required_source {
        return Err(StretchError::SourceTooShort {
            actual: source_frames,
            required: required_source,
        });
    }
    let required_output = parameters
        .window
        .checked_add(parameters.synthesis_hop)
        .ok_or(StretchError::ArithmeticOverflow)?;
    if output_frames < required_output {
        return Err(StretchError::OutputTooShort {
            actual: output_frames,
            required: required_output,
        });
    }
    if seek_frame >= output_frames {
        // Nothing to emit past the end; the mapped serve tiles silence
        // there and never constructs a seeded run.
        return Err(StretchError::OutputTooShort {
            actual: 0,
            required: 1,
        });
    }
    let out_max = output_frames - required_output;
    // Largest x with round_half_up(x·sf, of) ≤ src_limit, closed form:
    // x·sf + of/2 ≤ (src_limit + 1)·of − 1  ⇔  x ≤ ((src_limit+1)·of − 1 − of/2) / sf.
    let src_limit = (source_frames - required_source) as u128;
    let of = output_frames as u128;
    let sf = source_frames as u128;
    let numerator = (src_limit + 1)
        .checked_mul(of)
        .and_then(|value| value.checked_sub(1 + of / 2))
        .ok_or(StretchError::ArithmeticOverflow)?;
    let src_max = usize::try_from(numerator / sf).unwrap_or(usize::MAX);
    let start = seek_frame.min(out_max).min(src_max);
    let source_start = round_half_up_u128(start as u128 * sf, of).map_err(map_rate_error)?;
    let source_start =
        usize::try_from(source_start).map_err(|_| StretchError::ArithmeticOverflow)?;
    debug_assert!(source_frames - source_start >= required_source);
    debug_assert!(output_frames - start >= required_output);
    Ok((start, source_start))
}

/// The seeded sub-problem's ALIGNMENT context: the full tail mapped
/// cyclically, in sub-problem (tail-relative) coordinates.
///
/// This mirrors the shape the production full-entry loop gives the
/// canonical stream — without a cyclic domain the WSOLA candidate search
/// structurally starves (`NoCandidate`) near the run's end at ratios below
/// ~`1 − search_radius/window` (playback ≲ 75 %), because the linear
/// nominal outruns the last non-wrapping window start by more than the
/// search radius. It is an alignment domain ONLY: seeded runs still play
/// linearly, capture no checkpoints, and carry no loop-restart anchor.
fn seeded_alignment_context(sub_source: usize, sub_output: usize) -> LoopContext {
    LoopContext {
        source_start: 0,
        source_end: sub_source,
        output_start: 0,
        output_end: sub_output,
    }
}

/// Whole-buffer SEEDED reference: output frames `[seek_frame, output_frames)`
/// of a fresh stretch of the source tail onto the output tail — the byte
/// authority a shift>0 mapping epoch's streaming run is validated against.
/// Built on the frozen [`stretch_interleaved`] reference.
pub fn stretch_seeded_interleaved(
    source: &[i16],
    channels: usize,
    sample_rate: u32,
    output_frames: usize,
    seek_frame: usize,
) -> Result<Vec<i16>, StretchError> {
    if channels == 0 {
        return Err(StretchError::InvalidChannelCount);
    }
    if source.len() % channels != 0 {
        return Err(StretchError::IncompleteSourceFrame);
    }
    let parameters = StretchParameters::for_sample_rate(sample_rate)?;
    let source_frames = source.len() / channels;
    let (start, source_start) =
        seeded_bounds(source_frames, output_frames, seek_frame, parameters)?;
    let sub_source = source_frames - source_start;
    let sub_output = output_frames - start;
    let result = stretch_interleaved(
        &source[source_start * channels..],
        channels,
        sample_rate,
        sub_output,
        Some(seeded_alignment_context(sub_source, sub_output)),
    )?;
    let skip = (seek_frame - start) * channels;
    Ok(result.samples[skip..].to_vec())
}

/// Streaming twin of [`stretch_seeded_interleaved`]: a fresh
/// [`StretchState`] over the source/output tails, the bounded near-end
/// back-off skip drained at construction. `produce` then emits exactly
/// `output_frames − seek_frame` frames. Seeded runs expose no checkpoints —
/// within-epoch regeneration reconstructs fresh (deterministic either way).
#[derive(Debug)]
pub struct SeededStretchState {
    state: StretchState,
    source_start: usize,
    channels: usize,
}

impl SeededStretchState {
    pub fn new(
        source_frames: usize,
        output_frames: usize,
        channels: usize,
        sample_rate: u32,
        seek_frame: usize,
        source: &impl SourcePcm,
    ) -> Result<Self, StretchError> {
        if channels == 0 {
            return Err(StretchError::InvalidChannelCount);
        }
        assert_eq!(
            source.frames(),
            source_frames,
            "source view frame count diverges from construction"
        );
        let parameters = StretchParameters::for_sample_rate(sample_rate)?;
        let (start, source_start) =
            seeded_bounds(source_frames, output_frames, seek_frame, parameters)?;
        let sub_source = source_frames - source_start;
        let sub_output = output_frames - start;
        let mut state = StretchState::new(
            sub_source,
            sub_output,
            channels,
            sample_rate,
            Some(seeded_alignment_context(sub_source, sub_output)),
        )?;
        // Drain the back-off skip now (bounded by the stretcher's minimum
        // output requirement — O(1) by construction).
        let view = TailPcm::new(source, source_start);
        let mut skip = seek_frame - start;
        while skip > 0 {
            let mut scratch = vec![0i16; skip.min(1_024) * channels];
            let produced = state.produce(&view, &mut scratch)?;
            debug_assert!(produced.frames > 0, "seeded skip stalled");
            skip -= produced.frames;
        }
        Ok(Self {
            state,
            source_start,
            channels,
        })
    }

    /// Produce the next run of finalized frames; the contract mirrors
    /// [`StretchState::produce`] (one-frame minimum granularity, identical
    /// bytes under any chunking). `source` is the FULL source view — the
    /// tail offset is applied internally.
    pub fn produce(
        &mut self,
        source: &impl SourcePcm,
        out: &mut [i16],
    ) -> Result<Produced, StretchError> {
        let view = TailPcm::new(source, self.source_start);
        self.state.produce(&view, out)
    }

    #[must_use]
    pub fn clipped_samples(&self) -> usize {
        self.state.clipped_samples()
    }

    #[must_use]
    pub fn channels(&self) -> usize {
        self.channels
    }
}
