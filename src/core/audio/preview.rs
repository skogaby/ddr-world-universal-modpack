//! Wheel-preview window: `#SAMPLESTART` / `#SAMPLELENGTH` → a clamped,
//! block-aligned frame range in the normalised stream, decoded with the
//! stock-style fades.
//!
//! Rules (design §4.4): default start = ⅓ of the song, default length
//! 15 s, length clamped to 5–20 s, the window clamped inside the audio
//! (it slides back rather than shrinking), never empty; 0.25 s linear
//! fade-in and 1.5 s linear fade-out. The length is rounded up to whole
//! blocks so the preview entry's duration is `blocks · 128` like the
//! tool's and the stock banks'. The sibling CLI cuts a hard window with no
//! fades and no clamping (an off-the-end start yields an empty preview);
//! the runtime must always produce something audible.

use super::decode::{Decoder, Fade};
use super::{round_up_to_blocks, AudioError, AudioInfo, StereoBlock, BLOCK_FRAMES};

pub const DEFAULT_LENGTH_S: f64 = 15.0;
pub const MIN_LENGTH_S: f64 = 5.0;
pub const MAX_LENGTH_S: f64 = 20.0;
pub const FADE_IN_S: f64 = 0.25;
pub const FADE_OUT_S: f64 = 1.5;

/// A preview range in normalised (target-rate) frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewWindow {
    pub start_frame: u64,
    /// Whole blocks; ≥ 1 block.
    pub len_frames: u64,
    pub rate: u32,
}

impl PreviewWindow {
    /// Build from the simfile's tags (either may be absent or nonsense)
    /// and the song's normalised length.
    #[must_use]
    pub fn from_tags(
        sample_start_s: Option<f64>,
        sample_length_s: Option<f64>,
        song_len_s: f64,
        rate: u32,
    ) -> Self {
        let rate_f = f64::from(rate.max(1));
        let song_len_s = if song_len_s.is_finite() && song_len_s > 0.0 {
            song_len_s
        } else {
            0.0
        };
        let song_frames = (song_len_s * rate_f).floor().max(0.0) as u64;

        let mut len_s = sample_length_s
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(DEFAULT_LENGTH_S)
            .clamp(MIN_LENGTH_S, MAX_LENGTH_S);
        // Never longer than the song itself.
        if len_s > song_len_s {
            len_s = song_len_s;
        }
        let mut len_frames = round_up_to_blocks((len_s * rate_f).round() as u64);
        if len_frames > song_frames {
            // Round down instead when rounding up overshoots the audio.
            len_frames = song_frames / BLOCK_FRAMES as u64 * BLOCK_FRAMES as u64;
        }
        if len_frames == 0 {
            len_frames = BLOCK_FRAMES as u64;
        }

        let start_s = sample_start_s
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(song_len_s / 3.0);
        let mut start_frame = (start_s * rate_f).round() as u64;
        // Slide back so the whole window sits inside the audio.
        if start_frame.saturating_add(len_frames) > song_frames {
            start_frame = song_frames.saturating_sub(len_frames);
        }
        Self {
            start_frame,
            len_frames,
            rate,
        }
    }

    /// Convenience over [`PreviewWindow::from_tags`] for a probed file.
    #[must_use]
    pub fn for_audio(
        sample_start_s: Option<f64>,
        sample_length_s: Option<f64>,
        info: &AudioInfo,
    ) -> Self {
        let rate = info.target_rate();
        let len_s = info.target_frames() as f64 / f64::from(rate.max(1));
        Self::from_tags(sample_start_s, sample_length_s, len_s, rate)
    }

    #[must_use]
    pub fn blocks(&self) -> u64 {
        self.len_frames / BLOCK_FRAMES as u64
    }

    #[must_use]
    pub fn start_s(&self) -> f64 {
        self.start_frame as f64 / f64::from(self.rate.max(1))
    }

    #[must_use]
    pub fn len_s(&self) -> f64 {
        self.len_frames as f64 / f64::from(self.rate.max(1))
    }
}

/// Block iterator over a [`PreviewWindow`]: seeks, decodes `len_frames`
/// with the fades applied, pads the final block.
pub struct PreviewDecoder<'a>(Decoder<'a>);

impl<'a> PreviewDecoder<'a> {
    pub fn open(
        bytes: &'a [u8],
        info: &AudioInfo,
        window: &PreviewWindow,
    ) -> Result<Self, AudioError> {
        if window.rate != info.target_rate() {
            return Err(AudioError::BadTarget {
                what: "preview window rate differs from the normalised source rate",
            });
        }
        let rate_f = f64::from(window.rate.max(1));
        let fade = Fade {
            len: window.len_frames,
            fade_in: ((FADE_IN_S * rate_f) as u64).min(window.len_frames / 2),
            fade_out: ((FADE_OUT_S * rate_f) as u64).min(window.len_frames / 2),
        };
        let inner = Decoder::open_window(
            bytes,
            info,
            window.start_frame,
            window.len_frames,
            round_up_to_blocks(window.len_frames),
            Some(fade),
        )?;
        Ok(Self(inner))
    }

    #[must_use]
    pub fn failed(&self) -> Option<&AudioError> {
        self.0.failed()
    }

    #[must_use]
    pub fn frames_emitted(&self) -> u64 {
        self.0.frames_emitted()
    }
}

impl Iterator for PreviewDecoder<'_> {
    type Item = StereoBlock;

    fn next(&mut self) -> Option<StereoBlock> {
        self.0.next()
    }
}

#[cfg(test)]
mod tests {
    use super::super::probe::{self, tests::build_wav};
    use super::*;

    #[test]
    fn defaults_are_third_of_song_and_15_s() {
        let w = PreviewWindow::from_tags(None, None, 180.0, 44_100);
        assert_eq!(w.start_frame, (60.0 * 44_100.0) as u64);
        assert_eq!(w.len_frames, round_up_to_blocks(15 * 44_100));
        assert_eq!(w.len_frames % 128, 0);
        assert_eq!(w.rate, 44_100);
    }

    #[test]
    fn length_is_clamped_to_5_20_s() {
        let short = PreviewWindow::from_tags(Some(10.0), Some(1.0), 180.0, 48_000);
        assert_eq!(short.len_frames, round_up_to_blocks(5 * 48_000));
        let long = PreviewWindow::from_tags(Some(10.0), Some(90.0), 180.0, 48_000);
        assert_eq!(long.len_frames, round_up_to_blocks(20 * 48_000));
        let nan = PreviewWindow::from_tags(Some(f64::NAN), Some(f64::INFINITY), 180.0, 48_000);
        assert_eq!(nan.len_frames, round_up_to_blocks(15 * 48_000));
        assert_eq!(nan.start_frame, 60 * 48_000);
    }

    #[test]
    fn start_past_the_end_slides_back_inside_the_audio() {
        let song = 40.0;
        let w = PreviewWindow::from_tags(Some(500.0), Some(10.0), song, 44_100);
        let song_frames = (song * 44_100.0) as u64;
        assert!(w.start_frame + w.len_frames <= song_frames);
        assert_eq!(w.len_frames, round_up_to_blocks(10 * 44_100));
        assert_eq!(w.start_frame, song_frames - w.len_frames);
    }

    #[test]
    fn short_songs_never_yield_an_empty_window() {
        let w = PreviewWindow::from_tags(Some(1.0), Some(15.0), 3.0, 44_100);
        assert!(w.len_frames >= 128);
        assert!(w.len_frames <= 3 * 44_100);
        assert_eq!(w.start_frame + w.len_frames, 3 * 44_100);
        let tiny = PreviewWindow::from_tags(None, None, 0.001, 44_100);
        assert_eq!(tiny.len_frames, 128);
        assert_eq!(tiny.start_frame, 0);
    }

    #[test]
    fn preview_decoder_applies_fades_and_pads_to_blocks() {
        let frames = 44_100 * 12;
        let pcm: Vec<i16> = (0..frames).flat_map(|_| [8000i16, -8000]).collect();
        let wav = build_wav(44_100, 2, &pcm);
        let info = probe::probe(&wav).expect("probe");
        let w = PreviewWindow::for_audio(Some(2.0), Some(5.0), &info);
        assert_eq!(w.start_frame, 2 * 44_100);
        let mut dec = PreviewDecoder::open(&wav, &info, &w).expect("open");
        let flat: Vec<i16> = dec.by_ref().flat_map(|b| b.into_iter()).collect();
        assert!(dec.failed().is_none(), "{:?}", dec.failed());
        assert_eq!(flat.len() as u64, round_up_to_blocks(w.len_frames) * 2);
        let fade_in = (0.25 * 44_100.0) as usize;
        assert_eq!(flat[0], 0);
        assert!((i32::from(flat[(fade_in / 2) * 2]) - 4000).abs() <= 2);
        assert_eq!(flat[(fade_in + 100) * 2], 8000);
        assert_eq!(flat[(fade_in + 100) * 2 + 1], -8000);
        let len = w.len_frames as usize;
        let fade_out = (1.5 * 44_100.0) as usize;
        assert!((i32::from(flat[(len - fade_out / 2) * 2]) - 4000).abs() <= 2);
        assert!(flat[(len - 1) * 2].abs() < 10);
    }
}
