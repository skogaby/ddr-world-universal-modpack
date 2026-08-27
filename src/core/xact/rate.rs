//! Exact frame-rate and fixed-point clock arithmetic.

use std::fmt;

pub const MAX_XWB_DURATION: u64 = (1 << 28) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateRatio {
    pub source_frames: u64,
    pub output_frames: u64,
}

impl RateRatio {
    pub const IDENTITY: Self = Self {
        source_frames: 1,
        output_frames: 1,
    };

    pub fn new(source_frames: u64, output_frames: u64) -> Result<Self, RateError> {
        if source_frames == 0 || output_frames == 0 {
            return Err(RateError::ZeroFrameCount);
        }
        let divisor = gcd(source_frames, output_frames);
        Ok(Self {
            source_frames: source_frames / divisor,
            output_frames: output_frames / divisor,
        })
    }

    pub fn q31(self) -> Result<i64, RateError> {
        let numerator = u128::from(self.source_frames)
            .checked_mul(1u128 << 31)
            .ok_or(RateError::ArithmeticOverflow)?;
        let rounded = round_half_up_u128(numerator, u128::from(self.output_frames))?;
        i64::try_from(rounded).map_err(|_| RateError::ArithmeticOverflow)
    }

    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.source_frames as f64 / self.output_frames as f64
    }

    pub fn content_to_wall_ms(self, content_ms: i64) -> Result<i64, RateError> {
        let rounded =
            multiply_divide_half_away(content_ms, self.output_frames, self.source_frames)?;
        i64::try_from(rounded).map_err(|_| RateError::ArithmeticOverflow)
    }

    #[must_use]
    pub fn scale_i32(self, value: i32) -> i32 {
        let rounded = self
            .q31()
            .and_then(|factor| {
                let product = i128::from(value)
                    .checked_mul(i128::from(factor))
                    .ok_or(RateError::ArithmeticOverflow)?;
                divide_half_away_i128(product, 1i128 << 31)
            })
            .unwrap_or_else(|_| {
                if value.is_negative() {
                    i128::MIN
                } else {
                    i128::MAX
                }
            });
        rounded.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateTarget {
    pub output_blocks: u64,
    pub output_frames: u64,
    pub rate: RateRatio,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RateError {
    ZeroFrameCount,
    ZeroSamplesPerBlock,
    UnsupportedPercent { percent: u32 },
    DurationOutOfRange { output_frames: u128 },
    ArithmeticOverflow,
}

impl fmt::Display for RateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroFrameCount => write!(f, "rate frame counts must be nonzero"),
            Self::ZeroSamplesPerBlock => write!(f, "samples per block must be nonzero"),
            Self::UnsupportedPercent { percent } => {
                write!(f, "unsupported song-rate percentage {percent}")
            }
            Self::DurationOutOfRange { output_frames } => write!(
                f,
                "target duration {output_frames} does not fit the XWB 28-bit field"
            ),
            Self::ArithmeticOverflow => write!(f, "rate arithmetic overflow"),
        }
    }
}

impl std::error::Error for RateError {}

pub fn target_for_percent(
    source_frames: u64,
    samples_per_block: u32,
    percent: u32,
) -> Result<RateTarget, RateError> {
    if source_frames == 0 {
        return Err(RateError::ZeroFrameCount);
    }
    if samples_per_block == 0 {
        return Err(RateError::ZeroSamplesPerBlock);
    }
    if !(25..=175).contains(&percent) || percent % 5 != 0 {
        return Err(RateError::UnsupportedPercent { percent });
    }

    let numerator = u128::from(source_frames)
        .checked_mul(100)
        .ok_or(RateError::ArithmeticOverflow)?;
    let denominator = u128::from(samples_per_block)
        .checked_mul(u128::from(percent))
        .ok_or(RateError::ArithmeticOverflow)?;
    let output_blocks = round_half_up_u128(numerator, denominator)?.max(1);
    let output_frames = output_blocks
        .checked_mul(u128::from(samples_per_block))
        .ok_or(RateError::ArithmeticOverflow)?;
    if output_frames > u128::from(MAX_XWB_DURATION) {
        return Err(RateError::DurationOutOfRange { output_frames });
    }

    let output_blocks = u64::try_from(output_blocks).map_err(|_| RateError::ArithmeticOverflow)?;
    let output_frames = u64::try_from(output_frames).map_err(|_| RateError::ArithmeticOverflow)?;
    Ok(RateTarget {
        output_blocks,
        output_frames,
        rate: RateRatio::new(source_frames, output_frames)?,
    })
}

pub(crate) fn round_half_up_u128(numerator: u128, denominator: u128) -> Result<u128, RateError> {
    if denominator == 0 {
        return Err(RateError::ZeroFrameCount);
    }
    numerator
        .checked_add(denominator / 2)
        .ok_or(RateError::ArithmeticOverflow)
        .map(|adjusted| adjusted / denominator)
}

pub(crate) fn divide_half_away_i128(numerator: i128, denominator: i128) -> Result<i128, RateError> {
    if denominator <= 0 {
        return Err(RateError::ZeroFrameCount);
    }
    let negative = numerator < 0;
    let magnitude = if negative {
        numerator
            .checked_neg()
            .ok_or(RateError::ArithmeticOverflow)?
    } else {
        numerator
    };
    let rounded = magnitude
        .checked_add(denominator / 2)
        .ok_or(RateError::ArithmeticOverflow)?
        / denominator;
    Ok(if negative { -rounded } else { rounded })
}

fn multiply_divide_half_away(
    value: i64,
    numerator: u64,
    denominator: u64,
) -> Result<i128, RateError> {
    if denominator == 0 {
        return Err(RateError::ZeroFrameCount);
    }
    let product = i128::from(value)
        .checked_mul(i128::from(numerator))
        .ok_or(RateError::ArithmeticOverflow)?;
    divide_half_away_i128(product, i128::from(denominator))
}

const fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}
