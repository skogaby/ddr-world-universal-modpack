//! Measuring one materialised SSQ file against its song's decoded audio —
//! the single code path the prep tool's measurement pass and its
//! `--dry-run --auto-sync` report (`prep/`) share.
//!
//! What is measured is what the game plays:
//! - **timeline**: the `(tick, ms)` anchors and TPS of the file's own tempo
//!   chunk — so the lead-in, a split file's own timing and any delay nudge
//!   are exactly the game's;
//! - **events**: the parsed simfile's charts that THIS file carries. The
//!   file's step chunks name its slots; each slot maps back to the chart
//!   `core::ssq::writer::write_song` selected for it — the first chart of
//!   that style/difficulty whose effective timing is representable
//!   (`Edit` has no slot; a later duplicate is only ever picked when an
//!   earlier one was refused as unrepresentable);
//! - **audio**: the source decoded by `core::audio::decode::decode_whole`
//!   (normalised stereo, NO lead-in). The bank the game plays is
//!   `lead_in_frames` of silence + that PCM, so event times on the file's
//!   anchors are moved into the PCM's time base by subtracting
//!   `lead_in_s = lead_in_frames / rate` — equivalent to prepending the
//!   silence, without the copy.
//!
//! The decode and the onset envelope are per song ([`SongAnalysis`]); a
//! song with several `<b>_N.ssq` files measures each against the same
//! envelope.

use crate::core::sm::model::{Chart, Song};
use crate::core::ssq::ssq_chunk::find_chunk;
use crate::core::ssq::timing::TempoConverter;
use crate::core::ssq::writer::{slot_code, slot_index};

use super::onset::{self, Envelope};
use super::{chart_events, estimate_on, AudioView, Params, SyncEstimate, TimeMap};

/// The per-song half of a measurement: the onset envelope of the decoded
/// audio (`None` when the audio has none — every file is then refused as
/// `no_audio`).
#[derive(Debug, Clone, PartialEq)]
pub struct SongAnalysis {
    envelope: Option<Envelope>,
}

impl SongAnalysis {
    /// Analyse decoded PCM (interleaved, `channels` per frame). This is the
    /// expensive step: one 512-point FFT per ~0.73 ms of audio.
    #[must_use]
    pub fn new(audio: &AudioView<'_>) -> Self {
        Self {
            envelope: onset::envelope(audio),
        }
    }

    #[must_use]
    pub fn envelope(&self) -> Option<&Envelope> {
        self.envelope.as_ref()
    }

    /// Measure one SSQ file of `song`; see the module docs. `lead_in_s` is
    /// the silence the bank prepends to the decoded PCM.
    #[must_use]
    pub fn measure(
        &self,
        song: &Song,
        ssq: &[u8],
        lead_in_s: f64,
        params: &Params,
    ) -> FileMeasurement {
        measure_file(song, ssq, self.envelope.as_ref(), lead_in_s, params)
    }
}

/// One file's measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct FileMeasurement {
    pub estimate: SyncEstimate,
    /// Slots (0..9) whose step chunks the file carries, ascending.
    pub slots: Vec<u8>,
    /// `tempo_data[0]` of the file as measured (file ticks), when readable.
    pub td0: Option<i32>,
}

/// [`SongAnalysis::measure`] over an explicit envelope.
#[must_use]
pub fn measure_file(
    song: &Song,
    ssq: &[u8],
    env: Option<&Envelope>,
    lead_in_s: f64,
    params: &Params,
) -> FileMeasurement {
    let slots = file_slots(ssq);
    let conv = TempoConverter::from_ssq(ssq);
    let td0 = conv
        .as_ref()
        .and_then(|c| c.entries().next().map(|(_, ms)| ms));
    let map = conv.as_ref().and_then(|c| {
        let pairs: Vec<(i32, i32)> = c.entries().collect();
        TimeMap::from_tempo_pairs(&pairs, u32::try_from(c.tps()).ok()?)
    });
    let charts: Vec<&Chart> = slots
        .iter()
        .filter_map(|&s| declared_chart(song, s))
        .collect();
    // A missing map (unusable timing) leaves no events: refused as having
    // too few, like the tool.
    let events = map
        .map(|m| {
            let mut ev = chart_events(&charts, &song.timing, &m);
            for e in &mut ev {
                e.time_s -= lead_in_s;
            }
            ev
        })
        .unwrap_or_default();
    FileMeasurement {
        estimate: estimate_on(env, &events, params),
        slots,
        td0,
    }
}

/// Slots whose step chunk (type 3, `param2 = slot_code(slot)`) is present.
#[must_use]
pub fn file_slots(ssq: &[u8]) -> Vec<u8> {
    (0u8..10)
        .filter(|&s| slot_code(s).is_some_and(|code| find_chunk(ssq, 3, code).is_some()))
        .collect()
}

/// The chart `writer::write_song` declares in `slot`: the first chart of
/// that style/difficulty whose effective timing the writer can represent
/// (non-empty `#BPMS`, no warp / non-positive BPM / negative stop or
/// delay — `Timing::has_gimmicks`, the writer's `HideChart` rule).
#[must_use]
pub fn declared_chart(song: &Song, slot: u8) -> Option<&Chart> {
    song.charts.iter().find(|c| {
        slot_index(c.style, c.difficulty) == Some(slot) && {
            let t = c.effective_timing(&song.timing);
            !t.bpms.is_empty() && !t.has_gimmicks()
        }
    })
}

/// `tempo_data[0]` of an SSQ (file ticks), or `None` without a readable
/// tempo chunk — the reconcile probe.
#[must_use]
pub fn first_anchor(ssq: &[u8]) -> Option<i32> {
    TempoConverter::from_ssq(ssq)?
        .entries()
        .next()
        .map(|(_, ms)| ms)
}

/// TPS of an SSQ's tempo chunk.
#[must_use]
pub fn tempo_tps(ssq: &[u8]) -> Option<i32> {
    TempoConverter::from_ssq(ssq).map(|c| c.tps())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sm::simfile;
    use crate::core::ssq::writer::{self, WriteOptions};
    use crate::core::sync::{Outcome, Refusal, TARGET_OFFSET_MS};

    const RATE: u32 = 44_100;

    /// Deterministic xorshift64* in [-1, 1).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
            (x >> 11) as f64 / (1u64 << 52) as f64 - 1.0
        }
    }

    /// An SSC at 150 BPM, `#OFFSET:-0.250`, one Hard chart (plus an Edit
    /// the writer drops) with an irregular tap pattern over 24 measures.
    fn ssc() -> String {
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        let mut measures = Vec::new();
        for m in 0..24 {
            let mut rows = Vec::new();
            for r in 0..16 {
                let on = (m == 0 && r == 0) || rng.next() > 0.45;
                rows.push(if on { "1000" } else { "0000" });
            }
            measures.push(rows.join("\n"));
        }
        let notes = measures.join("\n,\n");
        format!(
            "#TITLE:t;\n#OFFSET:-0.250;\n#BPMS:0=150;\n\
             #NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#METER:9;\n#NOTES:\n{notes}\n;\n\
             #NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Edit;\n#METER:9;\n#NOTES:\n1111\n;\n"
        )
    }

    /// Note times in the audio: beat `b` sounds at `0.25 + 0.4·b` s for
    /// `#OFFSET:-0.250` at 150 BPM.
    fn note_times(song: &Song) -> Vec<f64> {
        song.charts
            .iter()
            .find(|c| c.difficulty == crate::core::sm::model::Difficulty::Expert)
            .map(|c| {
                c.notes
                    .iter()
                    .map(|n| 0.25 + n.beat.as_rational().as_f64() * 0.4)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Stereo PCM with a decaying noise burst at every time + `shift_ms`.
    fn bursts(times: &[f64], shift_ms: f64, seconds: f64) -> Vec<i16> {
        let frames = (seconds * f64::from(RATE)) as usize;
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let mut mono: Vec<f64> = (0..frames).map(|_| 0.01 * rng.next()).collect();
        let burst = (0.020 * f64::from(RATE)) as usize;
        for &t in times {
            let start = ((t + shift_ms / 1000.0) * f64::from(RATE)).round() as usize;
            for k in 0..burst {
                if let Some(s) = mono.get_mut(start + k) {
                    let decay = (-(k as f64) / (0.005 * f64::from(RATE))).exp();
                    *s += 0.6 * decay * rng.next();
                }
            }
        }
        mono.iter()
            .flat_map(|&s| {
                let v = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
                [v, v]
            })
            .collect()
    }

    fn measure_with(shift_ms: f64, lead_in_ms: u32) -> (FileMeasurement, Vec<u8>) {
        let song = simfile::parse_ssc(&ssc());
        let bundle = writer::write_song(
            &song,
            &WriteOptions {
                lead_in_ms,
                ..WriteOptions::default()
            },
        )
        .expect("write");
        let file = bundle.files[0].clone();
        let pcm = bursts(&note_times(&song), shift_ms, 42.0);
        let analysis = SongAnalysis::new(&AudioView {
            samples: &pcm,
            sample_rate: RATE,
            channels: 2,
        });
        let lead_in_s = f64::from(lead_in_ms) / 1000.0;
        (
            analysis.measure(&song, &file.bytes, lead_in_s, &Params::default()),
            file.bytes,
        )
    }

    #[test]
    fn file_slots_and_declared_chart_follow_the_writer() {
        let song = simfile::parse_ssc(&ssc());
        let bundle = writer::write_song(&song, &WriteOptions::default()).expect("write");
        let slots = file_slots(&bundle.files[0].bytes);
        assert_eq!(slots, vec![3]);
        let c = declared_chart(&song, 3).expect("Hard");
        assert_eq!(c.notes.len(), song.charts[0].notes.len());
        assert!(declared_chart(&song, 4).is_none());
        assert_eq!(first_anchor(&bundle.files[0].bytes), Some(250));
        assert_eq!(tempo_tps(&bundle.files[0].bytes), Some(1000));
        assert_eq!(first_anchor(&[]), None);
    }

    #[test]
    fn lead_in_does_not_move_the_measurement() {
        // Same audio, a file with and without 2 s of lead-in: the lead-in is
        // in both the anchors and (by subtraction) the audio time base.
        let (a, _) = measure_with(0.0, 0);
        let (b, bytes) = measure_with(0.0, 2000);
        let (ma, mb) = (
            a.estimate.measured_ms.expect("a"),
            b.estimate.measured_ms.expect("b"),
        );
        assert!((ma - mb).abs() < 0.05, "{ma} vs {mb}");
        assert_eq!(b.td0, Some(2250));
        assert_eq!(first_anchor(&bytes), Some(2250));
    }

    #[test]
    fn audio_late_by_x_applies_x_minus_baseline() {
        let (base, _) = measure_with(0.0, 2000);
        let m0 = base.estimate.measured_ms.expect("baseline");
        // The audio's hits 17 ms later than the chart: measured moves +17,
        // and the correction is measured − T.
        let (late, _) = measure_with(17.0, 2000);
        let m = late.estimate.measured_ms.expect("late");
        assert!((m - m0 - 17.0).abs() <= 0.5, "{m} vs {m0}");
        let want = (m - TARGET_OFFSET_MS).round() as i32;
        assert_eq!(late.estimate.outcome, Outcome::Apply { delta_ms: want });
    }

    #[test]
    fn unreadable_file_or_audio_is_refused() {
        let song = simfile::parse_ssc(&ssc());
        let pcm = bursts(&note_times(&song), 0.0, 42.0);
        let analysis = SongAnalysis::new(&AudioView {
            samples: &pcm,
            sample_rate: RATE,
            channels: 2,
        });
        let m = analysis.measure(&song, b"not an ssq", 0.0, &Params::default());
        assert_eq!(m.estimate.outcome, Outcome::Refused(Refusal::TooFewEvents));
        let silent = SongAnalysis::new(&AudioView {
            samples: &[],
            sample_rate: RATE,
            channels: 2,
        });
        assert!(silent.envelope().is_none());
        let bundle = writer::write_song(&song, &WriteOptions::default()).expect("write");
        let m = silent.measure(&song, &bundle.files[0].bytes, 0.0, &Params::default());
        assert_eq!(m.estimate.outcome, Outcome::Refused(Refusal::NoAudio));
    }
}
