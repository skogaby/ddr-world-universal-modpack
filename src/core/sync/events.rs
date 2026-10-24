//! Weighted chart events for correlation against the audio.
//!
//! The estimator correlates the audio against every note position of
//! every difficulty rather than a regular beat grid: a chart's rests,
//! breaks and syncopation make the pattern aperiodic, which keeps
//! aliased alignments from scoring as well as the true one.
//!
//! Which note kinds count is decided against what `core::ssq::writer`
//! emits as a *step row* (`prepare_chart`), so the analysis sees the
//! notes the game judges: `Tap`, `HoldHead`, `RollHead` (written as a
//! freeze) and `Shock` count; `Mine` does not (not a musical hit, and the
//! writer puts it in the separate mine chunk); `Lift` and `Fake` do not
//! (the writer drops both — StepMania never judges a fake, and DDR has no
//! lift); a note inside one of the chart's `#FAKES` ranges does not (the
//! writer drops it, same rule as `writer::in_fake_range`).

use std::collections::{BTreeMap, BTreeSet};

use crate::core::sm::model::{Beat, Chart, NoteKind, Timing};

use super::TimeMap;

/// One chart position the audio is correlated against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncEvent {
    /// When the position is heard, in audio seconds.
    pub time_s: f64,
    /// How many charts place a note there. Positions every difficulty
    /// agrees on are usually the strongest musical hits.
    pub weight: f64,
}

/// Union of the judged note positions of `charts`, placed on `map`.
///
/// `song_timing` resolves each chart's effective timing (its `#FAKES`
/// ranges). Each chart contributes at most once per beat, and the weight
/// of a position is the number of charts that place a note there. Events
/// are returned in beat order. See the module docs for which kinds count.
#[must_use]
pub fn chart_events(charts: &[&Chart], song_timing: &Timing, map: &TimeMap) -> Vec<SyncEvent> {
    let mut weights: BTreeMap<Beat, u32> = BTreeMap::new();
    for chart in charts {
        let timing = chart.effective_timing(song_timing);
        let beats: BTreeSet<Beat> = chart
            .notes
            .iter()
            .filter(|n| {
                matches!(
                    n.kind,
                    NoteKind::Tap
                        | NoteKind::HoldHead { .. }
                        | NoteKind::RollHead { .. }
                        | NoteKind::Shock { .. }
                )
            })
            .filter(|n| !in_fake_range(timing, n.beat))
            .map(|n| n.beat)
            .collect();
        for beat in beats {
            *weights.entry(beat).or_insert(0) += 1;
        }
    }
    weights
        .into_iter()
        .map(|(beat, count)| SyncEvent {
            time_s: map.seconds_at(beat.as_rational().as_f64()),
            weight: f64::from(count),
        })
        .collect()
}

/// `beat ∈ [start, start + len)` of a positive-length `#FAKES` range — the
/// writer's rule (`core::ssq::writer::in_fake_range`).
fn in_fake_range(timing: &Timing, beat: Beat) -> bool {
    timing.fakes.iter().any(|(start, len)| {
        if len.as_rational().num() <= 0 || beat < *start {
            return false;
        }
        match start.as_rational().add(&len.as_rational()) {
            Ok(end) => beat.as_rational() < end,
            Err(_) => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sm::model::{Difficulty, Note, PanelSet, ShockSide, Style};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn note(beat: i64, kind: NoteKind, panels: u8) -> Result<Note, Box<dyn std::error::Error>> {
        Ok(Note {
            beat: Beat::from_measure_ticks(beat * 1024).map_err(|e| format!("{e:?}"))?,
            kind,
            panels: PanelSet::from_bits(Style::Single, panels),
        })
    }

    fn chart(difficulty: Difficulty, notes: Vec<Note>) -> Chart {
        Chart {
            style: Style::Single,
            difficulty,
            meter: 5,
            description: String::new(),
            timing: None,
            scrolls: None,
            speeds: None,
            notes,
            warnings: Vec::new(),
        }
    }

    /// 120 BPM from beat 0: half a second per beat.
    fn half_second_beats() -> Result<TimeMap, Box<dyn std::error::Error>> {
        TimeMap::from_tempo_pairs(&[(0, 0), (4096, 2000)], 1000).ok_or_else(|| "map".into())
    }

    #[test]
    fn weights_count_charts_not_notes() -> TestResult {
        let hold = NoteKind::HoldHead {
            length: Beat::from_measure_ticks(1024).map_err(|e| format!("{e:?}"))?,
        };
        let a = chart(
            Difficulty::Basic,
            vec![
                note(4, NoteKind::Tap, 0x01)?,
                note(4, hold, 0x08)?,
                note(6, NoteKind::Tap, 0x02)?,
            ],
        );
        let b = chart(
            Difficulty::Expert,
            vec![
                note(4, NoteKind::Tap, 0x04)?,
                note(5, NoteKind::Mine, 0x01)?,
            ],
        );
        let events = chart_events(&[&a, &b], &Timing::default(), &half_second_beats()?);
        assert_eq!(
            events,
            vec![
                SyncEvent {
                    time_s: 2.0,
                    weight: 2.0
                },
                SyncEvent {
                    time_s: 3.0,
                    weight: 1.0
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn mines_only_chart_contributes_nothing() -> TestResult {
        let c = chart(Difficulty::Basic, vec![note(1, NoteKind::Mine, 0x01)?]);
        assert!(chart_events(&[&c], &Timing::default(), &half_second_beats()?).is_empty());
        Ok(())
    }

    #[test]
    fn shocks_count() -> TestResult {
        let shock = NoteKind::Shock {
            side: ShockSide::P1Only,
        };
        let c = chart(Difficulty::Basic, vec![note(2, shock, 0x0F)?]);
        assert_eq!(
            chart_events(&[&c], &Timing::default(), &half_second_beats()?),
            vec![SyncEvent {
                time_s: 1.0,
                weight: 1.0
            }]
        );
        Ok(())
    }

    #[test]
    fn rolls_count_lifts_fakes_and_fake_ranges_do_not() -> TestResult {
        let roll = NoteKind::RollHead {
            length: Beat::from_measure_ticks(512).map_err(|e| format!("{e:?}"))?,
        };
        let mut c = chart(
            Difficulty::Basic,
            vec![
                note(1, roll, 0x01)?,
                note(2, NoteKind::Lift, 0x02)?,
                note(3, NoteKind::Fake, 0x04)?,
                note(5, NoteKind::Tap, 0x08)?,
                note(6, NoteKind::Tap, 0x08)?,
            ],
        );
        // `#FAKES` [5, 6): the beat-5 tap is never judged, beat 6 is.
        c.timing = Some(Timing {
            fakes: vec![(Beat::from_integer(5), Beat::from_integer(1))],
            ..Timing::default()
        });
        let times: Vec<f64> = chart_events(&[&c], &Timing::default(), &half_second_beats()?)
            .iter()
            .map(|e| e.time_s)
            .collect();
        assert_eq!(times, vec![0.5, 3.0]);
        Ok(())
    }
}
