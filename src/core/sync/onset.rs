//! Onset-strength envelope of the audio.
//!
//! A short-window "rising edge" feature: each frame's spectrally
//! weighted log energy minus the energy a couple of milliseconds
//! earlier. It rises sharply at the attack of a hit, which is what a
//! chart's notes align to. The exact parameters live in the parent
//! module's constants and are part of the calibration.

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

use super::AudioView;
use super::{
    ONSET_BAND_HI_HZ, ONSET_BAND_LO_HZ, ONSET_EMPHASIS_HZ, ONSET_EMPHASIS_NORM,
    ONSET_HOP_SAMPLES_AT_44K, ONSET_LAG_S, ONSET_LOG_FLOOR, ONSET_WINDOW_S,
};

/// Onset strength, one value per analysis frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    values: Vec<f32>,
    /// Audio time (s) that frame 0 describes.
    t0_s: f64,
    /// Seconds between frames.
    dt_s: f64,
}

impl Envelope {
    /// Onset strength at `t_s`, linearly interpolated between frames; 0
    /// outside the analysed range.
    #[must_use]
    pub fn sample(&self, t_s: f64) -> f64 {
        let x = (t_s - self.t0_s) / self.dt_s;
        if !x.is_finite() || x < 0.0 {
            return 0.0;
        }
        let i = x.floor() as usize;
        match (self.values.get(i), self.values.get(i + 1)) {
            (Some(&a), Some(&b)) => {
                let f = x - i as f64;
                f64::from(a) * (1.0 - f) + f64::from(b) * f
            }
            _ => 0.0,
        }
    }

    /// Time (s) of the first frame.
    #[must_use]
    pub fn start_s(&self) -> f64 {
        self.t0_s
    }

    /// Time (s) just past the last frame.
    #[must_use]
    pub fn end_s(&self) -> f64 {
        self.t0_s + self.dt_s * self.values.len() as f64
    }

    /// Analysis frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Compute the onset envelope of `audio`.
///
/// Channels are averaged to mono. Returns `None` for empty audio, a zero
/// sample rate or channel count, or audio shorter than one window.
#[must_use]
pub fn envelope(audio: &AudioView<'_>) -> Option<Envelope> {
    if audio.sample_rate == 0 || audio.channels == 0 {
        return None;
    }
    let sr = f64::from(audio.sample_rate);
    let channels = usize::from(audio.channels);
    let scale = 1.0 / (channels as f32 * 32_768.0);
    let mono: Vec<f32> = audio
        .samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().map(|&s| f32::from(s)).sum::<f32>() * scale)
        .collect();

    let win = (ONSET_WINDOW_S * sr).round() as usize;
    let hop = ((sr * ONSET_HOP_SAMPLES_AT_44K / 44_100.0).round() as usize).max(1);
    if win == 0 || mono.len() <= win {
        return None;
    }
    let nfft = win.next_power_of_two();
    let lag = ((ONSET_LAG_S * sr / hop as f64).round() as usize).max(1);
    let n_frames = (mono.len() - win) / hop + 1;

    let window: Vec<f32> = (0..win)
        .map(|n| {
            let x = 2.0 * std::f64::consts::PI * n as f64 / win as f64;
            (0.5 - 0.5 * x.cos()) as f32
        })
        .collect();
    let bin_hz = sr / nfft as f64;
    let k_lo = ((ONSET_BAND_LO_HZ / bin_hz).ceil() as usize).max(1);
    let k_hi = ((ONSET_BAND_HI_HZ / bin_hz).floor() as usize).min(nfft / 2);
    let weights: Vec<f32> = (k_lo..=k_hi)
        .map(|k| {
            let f = k as f64 * bin_hz;
            (f * (-f / ONSET_EMPHASIS_HZ).exp() / ONSET_EMPHASIS_NORM) as f32
        })
        .collect();

    let fft = FftPlanner::<f32>::new().plan_fft_forward(nfft);
    let mut buf = vec![Complex32::new(0.0, 0.0); nfft];
    let mut scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut energies: Vec<f32> = Vec::with_capacity(n_frames);
    let mut values: Vec<f32> = Vec::with_capacity(n_frames);

    for frame in 0..n_frames {
        let start = frame * hop;
        let samples = mono.get(start..start + win)?;
        for (slot, (&s, &w)) in buf.iter_mut().zip(samples.iter().zip(&window)) {
            *slot = Complex32::new(s * w, 0.0);
        }
        for slot in buf.iter_mut().skip(win) {
            *slot = Complex32::new(0.0, 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);
        let energy: f32 = buf
            .get(k_lo..=k_hi)?
            .iter()
            .zip(&weights)
            .map(|(x, &w)| w * (x.norm_sqr() + ONSET_LOG_FLOOR).log2())
            .sum();
        energies.push(energy);
        values.push(if frame >= lag {
            energy - energies.get(frame - lag).copied().unwrap_or(energy)
        } else {
            0.0
        });
    }

    let dt_s = hop as f64 / sr;
    // Each frame describes its window's center; the difference spans
    // `lag` frames, so it is attributed half a lag earlier.
    let t0_s = (win as f64 / 2.0) / sr - lag as f64 * dt_s / 2.0;
    Some(Envelope { values, t0_s, dt_s })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stereo audio of `seconds` at `rate` with a 1 ms click at each time.
    fn clicks(rate: u32, seconds: f64, at: &[f64]) -> Vec<i16> {
        let frames = (seconds * f64::from(rate)) as usize;
        let mut samples = vec![0i16; frames * 2];
        let click_len = (0.001 * f64::from(rate)).round() as usize;
        for &t in at {
            let start = (t * f64::from(rate)).round() as usize;
            for f in start..(start + click_len).min(frames) {
                samples[2 * f] = 26_000;
                samples[2 * f + 1] = 26_000;
            }
        }
        samples
    }

    fn view(samples: &[i16], sample_rate: u32, channels: u16) -> AudioView<'_> {
        AudioView {
            samples,
            sample_rate,
            channels,
        }
    }

    /// Irregular click times: gaps cycle through 0.21–0.37 s.
    fn click_times() -> Vec<f64> {
        let gaps = [0.21, 0.37, 0.29, 0.33, 0.25];
        let mut t = 0.3;
        (0..20)
            .map(|i| {
                let now = t;
                t += gaps[i % gaps.len()];
                now
            })
            .collect()
    }

    /// Parabolically refined time of the envelope maximum near `t`.
    fn peak_near(env: &Envelope, t: f64) -> f64 {
        let lo = ((t - 0.015 - env.t0_s) / env.dt_s).floor().max(1.0) as usize;
        let hi = (((t + 0.015 - env.t0_s) / env.dt_s).ceil() as usize).min(env.values.len() - 2);
        let i = (lo..=hi)
            .max_by(|&a, &b| env.values[a].total_cmp(&env.values[b]))
            .unwrap_or(lo);
        let (a, b, c) = (
            f64::from(env.values[i - 1]),
            f64::from(env.values[i]),
            f64::from(env.values[i + 1]),
        );
        let den = a - 2.0 * b + c;
        let frac = if den.abs() > 1e-12 {
            0.5 * (a - c) / den
        } else {
            0.0
        };
        env.t0_s + (i as f64 + frac) * env.dt_s
    }

    fn median(mut v: Vec<f64>) -> f64 {
        v.sort_by(f64::total_cmp);
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            0.5 * (v[n / 2 - 1] + v[n / 2])
        }
    }

    /// Per-click lag (s) from click time to envelope peak.
    fn lags(rate: u32) -> Result<(Vec<f64>, f64), Box<dyn std::error::Error>> {
        let times = click_times();
        let samples = clicks(rate, 6.5, &times);
        let env = envelope(&view(&samples, rate, 2)).ok_or("envelope")?;
        let lags = times.iter().map(|&t| peak_near(&env, t) - t).collect();
        Ok((lags, env.dt_s))
    }

    #[test]
    fn click_train_peaks_follow_clicks_with_constant_lag() -> Result<(), Box<dyn std::error::Error>>
    {
        let (lags, hop) = lags(44_100)?;
        let med = median(lags.clone());
        assert!(med.abs() < 0.010, "lag {med} s should be a few ms");
        for (i, lag) in lags.iter().enumerate() {
            assert!(
                (lag - med).abs() <= hop,
                "click {i}: lag {lag} vs median {med} (hop {hop})"
            );
        }
        Ok(())
    }

    #[test]
    fn sample_rate_does_not_move_onsets() -> Result<(), Box<dyn std::error::Error>> {
        let m44 = median(lags(44_100)?.0);
        let m48 = median(lags(48_000)?.0);
        assert!(
            (m44 - m48).abs() <= 0.0005,
            "44.1 kHz lag {m44} vs 48 kHz lag {m48}"
        );
        Ok(())
    }

    #[test]
    fn silence_is_flat() -> Result<(), Box<dyn std::error::Error>> {
        let samples = clicks(44_100, 2.0, &[]);
        let env = envelope(&view(&samples, 44_100, 2)).ok_or("envelope")?;
        assert!(!env.values.is_empty());
        assert!(env.values.iter().all(|&v| v == 0.0));
        Ok(())
    }

    #[test]
    fn degenerate_audio_has_no_envelope() {
        let silent = vec![0i16; 88_200];
        assert!(envelope(&view(&[], 44_100, 2)).is_none(), "empty");
        assert!(envelope(&view(&silent, 0, 2)).is_none(), "zero rate");
        assert!(
            envelope(&view(&silent, 44_100, 0)).is_none(),
            "zero channels"
        );
        assert!(
            envelope(&view(&silent[..200], 44_100, 2)).is_none(),
            "shorter than a window"
        );
    }

    #[test]
    fn sample_interpolates_and_is_zero_outside() {
        let env = Envelope {
            values: vec![0.0, 1.0, 2.0],
            t0_s: 1.0,
            dt_s: 0.5,
        };
        assert!((env.sample(1.25) - 0.5).abs() < 1e-12);
        assert_eq!(env.sample(0.9), 0.0);
        assert_eq!(env.sample(2.0), 0.0);
        assert_eq!(env.start_s(), 1.0);
        assert_eq!(env.end_s(), 2.5);
    }
}
