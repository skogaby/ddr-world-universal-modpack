//! Pack audio → 128-frame stereo PCM blocks, pure and host-testable.
//!
//! Mechanism: [`probe::probe`] reads container headers only (Ogg page walk,
//! MP3 frame headers, RIFF chunks) and returns the exact frame count the
//! decoder will yield, so the song bank's layout (`core::xact::song_bank`)
//! is known before a single sample is decoded. [`decode::Decoder`] then streams
//! the file through `symphonia` and a normalisation chain — channels to
//! stereo, rate to 44.1/48 kHz, `lead_in_frames` of silence in front,
//! padded or truncated to exactly `total_frames` — as [`StereoBlock`]s
//! sized for the MS-ADPCM encoder (`core::xact::adpcm::encode_block`).
//! [`preview::PreviewWindow`] + [`preview::PreviewDecoder`] cut the wheel
//! preview with the stock fades; [`lead_in::choose_lead_in`] picks the silence the SSQ writer and
//! the audio must agree on.
//!
//! StepMania parity rules (community simfiles are synced against SM's
//! decoders, design §4.4):
//! - MP3: no gapless trimming (`libmad` never trims encoder delay). A CBR
//!   `Info` tag frame (and a `VBRI` frame) is decoded by SM as one frame of
//!   silence (`RageSoundReader_MP3::handle_first_frame` deliberately keeps
//!   it "to match DWI sync"); a VBR `Xing` frame is skipped. `symphonia`
//!   skips both, so [`probe::probe`] reports `leading_silence_frames` and the
//!   decoder prepends them.
//! - Vorbis: `libvorbisfile` semantics — the first packet yields nothing
//!   and the final packet is trimmed to the last page's granule — which is
//!   exactly `symphonia`'s gapless mode, so Vorbis decodes with gapless ON.
//!   The frame count is the last page's granule position; `libvorbisfile`
//!   additionally subtracts a first-page pre-roll that is zero for every
//!   mainstream encoder (documented in `probe.rs`).
//!
//! Purity contract: no `windows`, no logging macros, no panics on
//! untrusted bytes (every parser returns `Err`), bounded loops, imports
//! only `crate::core::{audio,xact}` and `symphonia`. The caller enforces
//! the file-size cap (`max_audio_mb`); this module caps the frame count
//! at [`MAX_FRAMES`]. `scripts/validate_stepmania.sh` mounts this
//! directory via `#[path]` under a `core` module mirror.

pub mod decode;
pub mod lead_in;
pub mod preview;
pub mod probe;

/// Frames per MS-ADPCM block at the DDR geometry (140 bytes, stereo).
pub const BLOCK_FRAMES: usize = 128;

/// One block: 128 stereo frames, interleaved `[L0, R0, L1, R1, …]`.
pub type StereoBlock = [i16; BLOCK_FRAMES * 2];

/// Longest audio accepted, in source frames: the XWB duration field is 28
/// bits (`core::xact::xwb::MAX_DURATION`), ≈ 93 min at 48 kHz.
pub const MAX_FRAMES: u64 = (1 << 28) - 1;

/// Rates the song bank carries as-is; anything else is resampled to 44.1 kHz.
pub const NATIVE_RATES: [u32; 2] = [44_100, 48_000];
pub const FALLBACK_RATE: u32 = 44_100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Vorbis,
    Mp3,
    Wav,
}

/// What [`probe`] learned without decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioInfo {
    pub codec: Codec,
    /// Source sample rate.
    pub rate: u32,
    /// Source channel count (1..=8).
    pub channels: u16,
    /// Exact source-rate frames the decoder yields, `leading_silence_frames`
    /// included.
    pub frames_exact: u64,
    /// Silence the decoder prepends to match StepMania (MP3 `Info`/`VBRI`
    /// frame rule); 0 for everything else.
    pub leading_silence_frames: u64,
    /// Byte range handed to the container parser (ID3v2/ID3v1 stripped for
    /// MP3; the whole input otherwise).
    pub(crate) audio_start: usize,
    pub(crate) audio_end: usize,
}

impl AudioInfo {
    /// Rate the normalised stream has: the source rate when native, else
    /// [`FALLBACK_RATE`].
    #[must_use]
    pub fn target_rate(&self) -> u32 {
        normalised_rate(self.rate)
    }

    /// Frames the normalised stream has (before lead-in / padding).
    #[must_use]
    pub fn target_frames(&self) -> u64 {
        normalised_frames(self.frames_exact, self.rate)
    }

    /// Source duration in seconds.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        if self.rate == 0 {
            return 0.0;
        }
        self.frames_exact as f64 / f64::from(self.rate)
    }

    pub(crate) fn audio_bytes<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        let end = self.audio_end.min(bytes.len());
        bytes.get(self.audio_start..end).unwrap_or(&[])
    }
}

/// Rate the normalisation chain outputs for a source rate.
#[must_use]
pub fn normalised_rate(rate: u32) -> u32 {
    if NATIVE_RATES.contains(&rate) {
        rate
    } else {
        FALLBACK_RATE
    }
}

/// Frames after rate normalisation: unchanged for native rates, else
/// `round(frames · 44100 / rate)` — the output length the resampler is
/// asked for, so decoder and planner agree by construction.
#[must_use]
pub fn normalised_frames(frames: u64, rate: u32) -> u64 {
    if rate == 0 {
        return 0;
    }
    if NATIVE_RATES.contains(&rate) {
        return frames;
    }
    let scaled = u128::from(frames) * u128::from(FALLBACK_RATE);
    let rate = u128::from(rate);
    u64::try_from((scaled + rate / 2) / rate).unwrap_or(u64::MAX)
}

/// Round a frame count up to whole blocks.
#[must_use]
pub fn round_up_to_blocks(frames: u64) -> u64 {
    let block = BLOCK_FRAMES as u64;
    frames.div_ceil(block).saturating_mul(block)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// No audio frames at all.
    Empty,
    /// None of OGG/MP3/WAV.
    UnknownFormat,
    Truncated {
        what: &'static str,
    },
    Unsupported {
        what: &'static str,
    },
    Malformed {
        what: &'static str,
    },
    TooLong {
        frames: u64,
    },
    /// The requested target disagrees with the source (rate, lengths).
    BadTarget {
        what: &'static str,
    },
    /// `symphonia` refused to open / find a track.
    Open(String),
    Decode(String),
    Seek(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("audio has no frames"),
            Self::UnknownFormat => f.write_str("not an OGG, MP3 or WAV file"),
            Self::Truncated { what } => write!(f, "truncated {what}"),
            Self::Unsupported { what } => write!(f, "unsupported {what}"),
            Self::Malformed { what } => write!(f, "malformed {what}"),
            Self::TooLong { frames } => write!(f, "audio too long ({frames} frames)"),
            Self::BadTarget { what } => write!(f, "bad target format: {what}"),
            Self::Open(why) => write!(f, "open failed: {why}"),
            Self::Decode(why) => write!(f, "decode failed: {why}"),
            Self::Seek(why) => write!(f, "seek failed: {why}"),
        }
    }
}

impl std::error::Error for AudioError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalised_frames_round_half_up_and_pass_native() {
        assert_eq!(normalised_frames(1000, 44_100), 1000);
        assert_eq!(normalised_frames(1000, 48_000), 1000);
        assert_eq!(normalised_frames(1000, 22_050), 2000);
        assert_eq!(normalised_frames(1, 32_000), 1);
        assert_eq!(normalised_frames(3, 32_000), 4);
        assert_eq!(normalised_frames(5, 0), 0);
        assert_eq!(normalised_rate(32_000), 44_100);
        assert_eq!(normalised_rate(48_000), 48_000);
    }

    #[test]
    fn round_up_to_blocks_is_exact_on_multiples() {
        assert_eq!(round_up_to_blocks(0), 0);
        assert_eq!(round_up_to_blocks(1), 128);
        assert_eq!(round_up_to_blocks(128), 128);
        assert_eq!(round_up_to_blocks(129), 256);
    }
}
