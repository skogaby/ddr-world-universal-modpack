//! One-song simulation: the DLL's planner drives a model of the game's judge
//! (`docs/gauge_and_judge_scoring_research.md` §1–§2), whose verdicts feed the
//! `judge_submit` bookkeeping (§3) and the NORMAL gauge (§4).
//!
//! Model, per frame (the game's `GamePlayActor::judgeNotes` + freeze judge):
//!
//! 1. `planner::plan_frame` fills the bot panel (`is_held` / `was_just_pressed`
//!    / `event_mc`) — the flag block the game reads through `IFootPanel`.
//! 2. Tap walk over the Results in order, stopping at `mc < note.mc − 260`;
//!    unjudged kind-0 notes only. Shocks: a `was_just_pressed` on a pad panel
//!    inside `[note.mc − 34, note.mc + 84]` ⇒ N.G. once the window closes,
//!    else O.K. Arrows: every arrow panel must be held and not yet claimed by
//!    an earlier unjudged note this frame (earliest-note attribution, no
//!    window test at match); the panels are claimed either way; `event =
//!    max(event_mc)`; a jump's events must lie within 66 ms; the grade comes
//!    from `event − note.mc` and only grades 0..=3 may be accepted; **one
//!    accepted note per frame** (best grade, strict `<`). A matched-but-
//!    rejected note stays unjudged with its press kept. Any unjudged note with
//!    `mc > note.mc + 160` is Missed.
//! 3. Freeze judge: a kind-2 tail resolves when `cur_beat ≥ tail.beat` and its
//!    head is judged — O.K. if the head was hit (the bot holds every body),
//!    N.G. if the head was Missed.
//!
//! Known simplifications (documented in the report): the head-Missed tail is
//! resolved N.G. (the game may instead tap-Miss the tail — N.G. severity 9 vs
//! Miss 13 on the gauge, and the Miss column); a note that is both the frame's
//! accepted candidate and past `+160` is treated as a Miss (the game submits
//! both — a double count that the bot practically never triggers).

use crate::bot::planner::{self, NoteView, PanelFlags, SongState};
use crate::bot::skill::{self, Curve, Plan, Rng};
use crate::chart::{Chart, KIND_FREEZE_TAIL, KIND_TAP};
use crate::gauge::NormalGauge;
use crate::scoring::{self, Board, Scorecard, GOOD, MISS, NG, OK};

/// Simulation parameters shared by every job.
#[derive(Debug, Clone, Copy)]
pub struct SimConfig {
    /// Judge frame rate.
    pub fps: u32,
    /// S-Marvelous window for the presentation column.
    pub smarv_ms: i32,
}

/// The game's tap-judge constants (§1).
const WALK_CUTOFF_MS: i32 = 260;
const MISS_MS: i32 = 160;
const SHOCK_EARLY_MS: i32 = 34;
const SHOCK_LATE_MS: i32 = 84;
const JUMP_SPREAD_MS: i32 = 66;
/// Lead-in before the first note and tail after the last (the run keeps
/// judging until every note has a verdict).
const LEAD_IN_MS: i32 = 3000;
const TAIL_MS: i32 = 1500;

#[derive(Debug, Clone, Copy)]
struct Result {
    /// Judge timestamp (−1 = unjudged) — the accepted EVENT for taps.
    ts: i32,
    /// 0xFF = unjudged.
    grade: u8,
    /// Shock: a pad panel was pressed inside the window.
    shock_pressed: bool,
}

pub fn simulate(
    chart: &Chart,
    level: u8,
    curve: &Curve,
    seed: u64,
    seed_idx: u32,
    cfg: &SimConfig,
) -> Scorecard {
    let notes = &chart.notes;
    let n = notes.len();
    let mut results = vec![
        Result {
            ts: -1,
            grade: 0xFF,
            shock_pressed: false,
        };
        n
    ];
    let mut views: Vec<NoteView> = notes
        .iter()
        .enumerate()
        .map(|(i, sn)| NoteView {
            idx: i,
            kind: sn.kind,
            music_count: sn.mc,
            beat_count: sn.beat,
            state: sn.state,
            length: sn.length,
            unjudged: true,
        })
        .collect();

    let mut st = SongState::new(n);
    let mut rng = Rng::new(seed);
    let mut flags = PanelFlags::default();
    let mut board = Board::new(chart.taps, chart.freezes, chart.shocks, cfg.smarv_ms);
    let mut gauge = NormalGauge::new();
    let mut mismatches = 0u32;
    let mut delta_hist = [0u32; 25];
    let mut failed_at: Option<i32> = None;
    let mut judged_count = 0usize;

    let first_mc = notes.first().map(|x| x.mc).unwrap_or(0);
    let last_mc = notes.last().map(|x| x.mc).unwrap_or(0);
    let mut frame: i64 = 0;
    let start = first_mc - LEAD_IN_MS;
    // The judge walk's cursor: first index that may still be unjudged.
    let mut walk_from = 0usize;

    loop {
        let mc = start + (frame * 1000 / cfg.fps as i64) as i32;
        frame += 1;
        if judged_count >= n || mc > last_mc + MISS_MS + TAIL_MS {
            break;
        }
        let cur_beat = chart.tempo.tick_at_ms(mc);

        // 1. The bot fills its panel.
        planner::plan_frame(&views, &mut st, &mut rng, curve, mc, cur_beat, &mut flags);

        // 2. Tap walk.
        while walk_from < n && results[walk_from].grade != 0xFF {
            walk_from += 1;
        }
        let mut claimed = [false; 8];
        let mut best: Option<(usize, u8, i32)> = None; // (idx, grade, event)
        let mut misses: Vec<usize> = Vec::new();
        let mut shocks_done: Vec<(usize, bool)> = Vec::new();
        for i in walk_from..n {
            let note = &notes[i];
            if mc < note.mc - WALK_CUTOFF_MS {
                break;
            }
            let r = results[i];
            if r.grade != 0xFF || note.kind != KIND_TAP {
                continue;
            }
            if note.is_shock() {
                if mc >= note.mc - SHOCK_EARLY_MS {
                    if mc > note.mc + SHOCK_LATE_MS {
                        shocks_done.push((i, r.shock_pressed));
                    } else {
                        let pad = if note.state[..4].iter().all(|&s| s == 1) {
                            0..4
                        } else {
                            4..8
                        };
                        if pad.clone().any(|p| flags.was_just_pressed[p] != 0) {
                            results[i].shock_pressed = true;
                        }
                    }
                }
                continue;
            }
            // Arrow note: every arrow panel held and unclaimed.
            let arrows: Vec<usize> = note.arrow_panels().collect();
            let all_held = arrows.iter().all(|&p| flags.is_held[p] != 0 && !claimed[p]);
            let mut matched = false;
            if all_held && !arrows.is_empty() {
                for &p in &arrows {
                    claimed[p] = true;
                }
                let ev_max = arrows.iter().map(|&p| flags.event_mc[p]).max().unwrap_or(0);
                let ev_min = arrows.iter().map(|&p| flags.event_mc[p]).min().unwrap_or(0);
                if ev_max - ev_min <= JUMP_SPREAD_MS {
                    let grade = skill::grade_for_offset(ev_max - note.mc);
                    if grade <= GOOD as u8 && best.is_none_or(|(_, g, _)| grade < g) {
                        best = Some((i, grade, ev_max));
                        matched = true;
                    }
                }
            }
            if !matched && mc > note.mc + MISS_MS {
                misses.push(i);
            }
        }

        // Miss marks (in walk order), then the frame's accepted note.
        for i in misses {
            judge(
                &mut results,
                &mut views,
                i,
                mc,
                MISS as u8,
                0,
                &mut board,
                &mut gauge,
                cur_beat,
                false,
            );
            judged_count += 1;
        }
        for (i, pressed) in shocks_done {
            let g = if pressed { NG } else { OK };
            judge(
                &mut results,
                &mut views,
                i,
                mc,
                g as u8,
                0,
                &mut board,
                &mut gauge,
                cur_beat,
                false,
            );
            judged_count += 1;
        }
        if let Some((i, grade, event)) = best {
            if results[i].grade == 0xFF {
                let d = event - notes[i].mc;
                judge(
                    &mut results,
                    &mut views,
                    i,
                    event,
                    grade,
                    d,
                    &mut board,
                    &mut gauge,
                    cur_beat,
                    false,
                );
                judged_count += 1;
                if let Some(slot) = delta_hist.get_mut(scoring::delta_bin(d)) {
                    *slot += 1;
                }
                // Model self-check: the planner's resolved plan predicts this
                // grade exactly.
                match st.plans.get(i).copied().flatten() {
                    Some(Plan::Hit { d_ms }) if skill::grade_for_offset(d_ms) == grade => {}
                    _ => mismatches += 1,
                }
            }
        }

        // 3. Freeze judge: tails whose body ended and whose head is judged.
        for i in walk_from..n {
            let note = &notes[i];
            if note.kind != KIND_FREEZE_TAIL || results[i].grade != 0xFF {
                continue;
            }
            if note.beat > cur_beat + 4096 * 4 {
                break; // far future (tails sort by beat)
            }
            let Some(h) = note.head else { continue };
            let head = results[h];
            if cur_beat >= note.beat && head.grade != 0xFF {
                let g = if head.grade == MISS as u8 { NG } else { OK };
                judge(
                    &mut results,
                    &mut views,
                    i,
                    mc,
                    g as u8,
                    0,
                    &mut board,
                    &mut gauge,
                    cur_beat,
                    true,
                );
                judged_count += 1;
            }
        }

        if gauge.died && failed_at.is_none() {
            failed_at = Some(mc - first_mc);
        }
    }

    // Anything left unjudged (should not happen) is a Miss so counts stay
    // consistent with the chart.
    for i in 0..n {
        if results[i].grade == 0xFF {
            let g = if notes[i].kind == KIND_FREEZE_TAIL {
                NG
            } else {
                MISS
            };
            judge(
                &mut results,
                &mut views,
                i,
                last_mc,
                g as u8,
                0,
                &mut board,
                &mut gauge,
                0,
                notes[i].kind == KIND_FREEZE_TAIL,
            );
        }
    }
    // Let the planner observe the final verdicts so its tally is complete
    // (the loop exits the frame the last note is judged).
    let end_mc = last_mc + MISS_MS + TAIL_MS;
    planner::plan_frame(
        &views,
        &mut st,
        &mut rng,
        curve,
        end_mc,
        chart.tempo.tick_at_ms(end_mc),
        &mut flags,
    );
    // Planner tally vs the judged Miss count (taps that were planned Miss and
    // judged Miss agree by construction; a planned Hit judged Miss is a
    // mismatch the accepted-note check above cannot see).
    let planned = st.tally();
    let judged_tap_miss = notes
        .iter()
        .zip(results.iter())
        .filter(|(nn, r)| nn.kind == KIND_TAP && !nn.is_shock() && r.grade == MISS as u8)
        .count() as u32;
    mismatches += planned[MISS].abs_diff(judged_tap_miss);

    let score = board.score();
    Scorecard {
        chart: chart.name.clone(),
        difficulty: chart.difficulty,
        level,
        seed_idx,
        note_count: n as u32,
        taps: chart.taps,
        freezes: chart.freezes,
        shocks: chart.shocks,
        duration_ms: chart.duration_ms(),
        counts: board.counts,
        smarv: board.smarv,
        max_combo: board.max_combo,
        score,
        ex: board.ex(),
        ex_max: board.ex_max(),
        fc: board.fc,
        smfc: board.is_smfc(),
        fast: board.fast,
        slow: board.slow,
        gauge_final: gauge.value,
        gauge_min: gauge.min_value,
        failed: gauge.died,
        failed_at_ms: failed_at,
        rank: scoring::rank(score, gauge.died),
        mismatches,
        delta_hist,
    }
}

/// Commit one verdict: results + planner view + `judge_submit` bookkeeping
/// (combo message BEFORE the grade reaches the gauge, as the game orders it).
#[allow(clippy::too_many_arguments)]
fn judge(
    results: &mut [Result],
    views: &mut [NoteView],
    i: usize,
    ts: i32,
    grade: u8,
    delta_ms: i32,
    board: &mut Board,
    gauge: &mut NormalGauge,
    cur_beat: i32,
    is_tail: bool,
) {
    results[i].ts = ts;
    results[i].grade = grade;
    views[i].unjudged = false;
    let (combo, max_combo) = board.submit(grade as usize, delta_ms, is_tail);
    gauge.set_combo(combo as i32, max_combo as i32);
    // The gauge's `0x1045` music count is the frame's mc; for the accepted
    // tap `ts` is the event, which differs by ≤ 8 ms — use `ts` for both.
    gauge.apply(grade, delta_ms, cur_beat, ts);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{SimNote, TempoMap};
    use crate::core::ssq::timing::TempoConverter;

    fn tempo_120() -> TempoMap {
        // Build via a real tempo chunk so the converter path is exercised.
        let mut b = Vec::new();
        b.extend(28u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1000u16.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(0i32.to_le_bytes());
        b.extend(4096i32.to_le_bytes());
        b.extend(0i32.to_le_bytes());
        b.extend(2000i32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        TempoMap::from_converter(&TempoConverter::from_ssq(&b).unwrap()).unwrap()
    }

    fn tap(beat: i32, tempo: &TempoMap, panels: &[usize]) -> SimNote {
        let mut state = [0; 8];
        for &p in panels {
            state[p] = 1;
        }
        SimNote {
            kind: KIND_TAP,
            beat,
            mc: tempo.ms_at_tick(beat),
            state,
            length: [0; 8],
            head: None,
        }
    }

    fn chart(notes: Vec<SimNote>) -> Chart {
        let tempo = tempo_120();
        let taps = notes
            .iter()
            .filter(|n| n.kind == KIND_TAP && !n.is_shock())
            .count() as u32;
        let shocks = notes
            .iter()
            .filter(|n| n.kind == KIND_TAP && n.is_shock())
            .count() as u32;
        let freezes = notes.iter().filter(|n| n.kind == KIND_FREEZE_TAIL).count() as u32;
        Chart {
            name: "test".into(),
            difficulty: crate::chart::Difficulty::Expert,
            tempo,
            notes,
            taps,
            freezes,
            shocks,
        }
    }

    fn cfg() -> SimConfig {
        SimConfig {
            fps: 60,
            smarv_ms: 12,
        }
    }

    /// A zero-lean, zero-jitter curve makes every hit exact — the model must
    /// produce an MFC.
    fn perfect_curve() -> Curve {
        Curve {
            lean_ms: 0.0,
            tight_ms: 0.0,
            drift_ms: 0.0,
            loose_ms: 0.0,
            p_tight: 1.0,
            p_miss: 0.0,
            form_sd: 0.0,
            late_bias_sd: 0.0,
            sign_stickiness: 1.0,
        }
    }

    /// Every note a flubbed Miss.
    fn all_miss_curve() -> Curve {
        Curve {
            p_miss: 1.0,
            ..perfect_curve()
        }
    }

    #[test]
    fn perfect_bot_full_combos_a_stream() {
        let t = tempo_120();
        let notes: Vec<SimNote> = (0..64)
            .map(|i| tap(i * 512, &t, &[(i % 4) as usize]))
            .collect();
        let c = chart(notes);
        let card = simulate(&c, 10, &perfect_curve(), 1, 0, &cfg());
        assert_eq!(card.counts[0], 64, "{:?}", card.counts);
        assert_eq!(card.score, 1_000_000);
        assert_eq!(card.fc, Some(scoring::FullCombo::Marvelous));
        assert!(card.smfc);
        assert!(!card.failed);
        assert_eq!(card.mismatches, 0);
        assert_eq!(card.max_combo, 64);
    }

    #[test]
    fn jumps_are_one_note() {
        let t = tempo_120();
        let notes = vec![
            tap(0, &t, &[0, 3]),
            tap(1024, &t, &[1, 2]),
            tap(2048, &t, &[0]),
        ];
        let c = chart(notes);
        let card = simulate(&c, 10, &perfect_curve(), 1, 0, &cfg());
        assert_eq!(card.counts[0], 3);
        assert_eq!(card.max_combo, 3);
        assert_eq!(card.score, 1_000_000);
    }

    #[test]
    fn freeze_ok_when_head_hit_ng_when_missed() {
        let t = tempo_120();
        let mut head = tap(0, &t, &[2]);
        head.state[2] = 4;
        head.length[2] = 2048;
        let tail = SimNote {
            kind: KIND_FREEZE_TAIL,
            beat: 2048,
            mc: t.ms_at_tick(2048),
            state: head.state,
            length: [0; 8],
            head: Some(0),
        };
        let c = chart(vec![head, tail, tap(4096, &t, &[0])]);
        let card = simulate(&c, 10, &perfect_curve(), 1, 0, &cfg());
        assert_eq!(card.counts[OK], 1);
        assert_eq!(card.counts[NG], 0);
        assert_eq!(card.fc, Some(scoring::FullCombo::Marvelous));
        assert_eq!(card.score, 1_000_000);

        // A curve that always misses ⇒ head Miss, tail N.G.
        let all_miss = all_miss_curve();
        let card = simulate(&c, 1, &all_miss, 1, 0, &cfg());
        assert_eq!(card.counts[MISS], 2);
        assert_eq!(card.counts[NG], 1);
        assert_eq!(card.fc, None);
        assert_eq!(card.mismatches, 0);
    }

    #[test]
    fn shocks_are_avoided_and_count_ok() {
        let t = tempo_120();
        let mut shock = tap(2048, &t, &[]);
        shock.state[..4].copy_from_slice(&[1, 1, 1, 1]);
        let c = chart(vec![tap(0, &t, &[0]), shock, tap(4096, &t, &[3])]);
        let card = simulate(&c, 10, &perfect_curve(), 1, 0, &cfg());
        assert_eq!(card.counts[OK], 1);
        assert_eq!(card.counts[NG], 0);
        assert_eq!(card.counts[0], 2);
        assert_eq!(card.score, 1_000_000);
        assert_eq!(card.fc, Some(scoring::FullCombo::Marvelous));
    }

    #[test]
    fn all_miss_bot_fails_a_long_song() {
        let t = tempo_120();
        // 200 notes at 8th-note spacing (250 ms) ⇒ 50 s.
        let notes: Vec<SimNote> = (0..200)
            .map(|i| tap(i * 512, &t, &[(i % 4) as usize]))
            .collect();
        let c = chart(notes);
        let all_miss = all_miss_curve();
        let card = simulate(&c, 1, &all_miss, 7, 0, &cfg());
        assert_eq!(card.counts[MISS], 200);
        assert!(card.failed);
        assert_eq!(card.rank, "E");
        assert_eq!(card.score, 0);
        assert!(card.failed_at_ms.is_some());
    }

    #[test]
    fn planned_grades_match_judged_grades_over_random_play() {
        // The self-check across levels on a dense synthetic chart.
        let t = tempo_120();
        let notes: Vec<SimNote> = (0..400)
            .map(|i| {
                let panels: &[usize] = match i % 7 {
                    0 => &[0, 3],
                    1 => &[1],
                    2 => &[2],
                    3 => &[3],
                    4 => &[0],
                    5 => &[1, 2],
                    _ => &[2],
                };
                tap(i * 256, &t, panels) // 16ths at 120 BPM = 125 ms apart
            })
            .collect();
        let c = chart(notes);
        for level in [1u8, 3, 5, 8, 10] {
            let card = simulate(
                &c,
                level,
                &skill::curve(level),
                99 + level as u64,
                0,
                &cfg(),
            );
            assert_eq!(card.mismatches, 0, "L{level}: {card:?}");
            let total: u32 = card.counts.iter().sum();
            assert_eq!(total, 400, "every note judged at L{level}");
        }
    }
}
