//! Per-frame planner (design §4.7) — pure.
//!
//! Replaces the game's `AutoFootPanel::update` for the bot side: walks the
//! actor's Results window in note order and produces the flag block the bot
//! panel object exposes to the judge. Two facts about the judge shape every
//! rule here (design A.5):
//!
//! 1. It attributes a held panel to the EARLIEST unjudged note carrying that
//!    arrow, with no window test at match time — so per-panel judge events
//!    must strictly increase IN NOTE ORDER (a later note may never present an
//!    event earlier than the note before it on that panel), and only one
//!    event per panel may be live in a frame (the earliest note owns it).
//! 2. Only Marvelous..Good (±124 ms) are ever GRADED; an event 125..160 ms
//!    out is matched then rejected (note unjudged, press kept), and the note
//!    becomes a Miss only once `mc > note.mc + 160` — so a decided Miss blocks
//!    the following same-panel note's event until `note.mc + 161`, or the next
//!    press would "rescue" the missed note as a Good, and a floored event
//!    beyond ±124 is itself a Miss.
//!
//! Decisions are resolved ONCE, the first frame a note enters the walk window
//! (every earlier note has resolved by then), and are stable for the note's
//! life. Dependency-free so the host harness can mount it; the engine adapter
//! (`filler.rs`) builds `NoteView`s from the live `GameNote`s.

use super::skill::{self, Curve, Form, Plan, Rng};

/// The stock `update`'s press lookahead: a press is emitted from `E − 8`.
pub const LOOKAHEAD_MS: i32 = 8;
/// How far before its music count a note may be decided/emitted (covers the
/// earliest `d` the skill model can produce plus the lookahead).
pub const MAX_EARLY_MS: i32 = 200;
/// Outermost GRADED window (Good, ±124): a floored event beyond it can never
/// be graded, so the note is planned as a Miss.
pub const GOOD_WINDOW_MS: i32 = skill::GOOD_WINDOW_MS;
/// The judge marks a note Missed once `mc > note.mc + 160`; a Miss blocks its
/// panels until then + 1.
pub const MISS_WINDOW_MS: i32 = skill::MISS_WINDOW_MS;

/// One Results entry, as the filler reads it from the live actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteView {
    /// Position in the Results vector (stable for the song).
    pub idx: usize,
    /// `GameNote+0x00` kind byte: 0 = arrow/shock (tap-judged), 2 = freeze
    /// tail (resolved by the freeze judge — NEVER decided or pressed here),
    /// anything else = not ours.
    pub kind: i8,
    pub music_count: i32,
    pub beat_count: i32,
    /// Per-panel state (1 TRG, 4 REP = arrow panels; ≥ 2 = freeze body).
    pub state: [i32; 8],
    /// Per-panel freeze length in beats.
    pub length: [i32; 8],
    /// `result.ts < 0 && result.grade == 0xFF`.
    pub unjudged: bool,
}

/// The flag block for one frame. Field-identical to
/// `foot_panel_swap::BotPanelFlags` (the service copies it verbatim).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelFlags {
    pub is_held: [u8; 8],
    pub was_just_pressed: [u8; 8],
    pub event_mc: [i32; 8],
}

/// Per-song planner state.
#[derive(Debug, Clone)]
pub struct SongState {
    /// The decision per Results index. Once `resolved`, a `Hit`'s `d_ms` is
    /// the FINAL offset (`E − music_count`) after every floor was applied.
    pub plans: Vec<Option<Plan>>,
    resolved: Vec<bool>,
    tallied: Vec<bool>,
    /// Per panel: no event may land before this (set by decided Misses).
    pub blocked_until: [i32; 8],
    /// Per panel: the latest RESOLVED event so far, in note order.
    pub last_event: [i32; 8],
    /// First Results index that may still need work; never decreases.
    pub cursor: usize,
    /// Planned grade counts (0 Marvelous … 4 Boo, 5 Miss) of judged notes.
    tally: [u32; 6],
    /// The dancer's per-song lean/drift state, rolled at the first decision
    /// (a rebuilt `SongState` — song reset — re-rolls it).
    form: Option<Form>,
}

impl SongState {
    /// `capacity` = the expected note count (the vectors grow on demand).
    pub fn new(capacity: usize) -> Self {
        SongState {
            plans: vec![None; capacity],
            resolved: vec![false; capacity],
            tallied: vec![false; capacity],
            blocked_until: [0; 8],
            last_event: [i32::MIN; 8],
            cursor: 0,
            tally: [0; 6],
            form: None,
        }
    }

    /// Planned grade counts of the notes observed judged so far.
    pub fn tally(&self) -> [u32; 6] {
        self.tally
    }

    fn ensure(&mut self, idx: usize) {
        if idx >= self.plans.len() {
            self.plans.resize(idx + 1, None);
            self.resolved.resize(idx + 1, false);
            self.tallied.resize(idx + 1, false);
        }
    }

    /// Count a judged note's planned grade exactly once.
    fn tally_judged(&mut self, idx: usize) {
        self.ensure(idx);
        if let Some(t) = self.tallied.get_mut(idx) {
            if *t {
                return;
            }
            *t = true;
        }
        let grade = match self.plans.get(idx).copied().flatten() {
            Some(Plan::Hit { d_ms }) => skill::grade_for_offset(d_ms) as usize,
            Some(Plan::Miss) => skill::GRADE_MISS as usize,
            None => return, // shock / never decided — not the bot's grade
        };
        if let Some(slot) = self.tally.get_mut(grade) {
            *slot += 1;
        }
    }
}

fn max_length(n: &NoteView) -> i32 {
    n.length.iter().copied().max().unwrap_or(0)
}

/// The stock freeze-body condition: the note's body is still running.
fn freeze_body_running(n: &NoteView, cur_beat: i32) -> bool {
    cur_beat < n.beat_count.saturating_add(max_length(n))
}

/// A judged freeze head whose body still runs must keep being walked (the
/// body hold is emitted every frame, exactly like the stock `update`).
fn freeze_active(n: &NoteView, cur_beat: i32) -> bool {
    n.state.iter().any(|&s| s >= 2) && freeze_body_running(n, cur_beat)
}

/// The game's shock test: every panel of a pad is TRG.
fn is_shock(n: &NoteView) -> bool {
    n.state[..4].iter().all(|&s| s == 1) || n.state[4..].iter().all(|&s| s == 1)
}

fn is_arrow_panel(state: i32) -> bool {
    state == 1 || state == 4
}

/// Resolve `idx`'s decision (once): decide if needed, then apply the
/// per-panel floors in note order; a floor past the Boo window turns the hit
/// into a Miss. Updates `blocked_until` / `last_event` accordingly.
fn resolve(st: &mut SongState, n: &NoteView, rng: &mut Rng, c: &Curve) -> Plan {
    let idx = n.idx;
    st.ensure(idx);
    if st.resolved.get(idx).copied().unwrap_or(false) {
        if let Some(Some(p)) = st.plans.get(idx) {
            return *p;
        }
    }
    let raw = match st.plans.get(idx).copied().flatten() {
        Some(p) => p,
        None => {
            let form = st.form.get_or_insert_with(|| Form::new(rng, c));
            skill::decide(rng, form, c)
        }
    };
    let arrows = (0..8).filter(|&p| is_arrow_panel(n.state[p]));
    let plan = match raw {
        Plan::Miss => Plan::Miss,
        Plan::Hit { d_ms } => {
            let mut e = n.music_count.saturating_add(d_ms);
            for p in arrows.clone() {
                e = e
                    .max(st.blocked_until[p])
                    .max(st.last_event[p].saturating_add(1));
            }
            if e > n.music_count.saturating_add(GOOD_WINDOW_MS) {
                Plan::Miss
            } else {
                Plan::Hit {
                    d_ms: e - n.music_count,
                }
            }
        }
    };
    match plan {
        Plan::Miss => {
            let floor = n.music_count.saturating_add(MISS_WINDOW_MS + 1);
            for p in arrows {
                st.blocked_until[p] = st.blocked_until[p].max(floor);
            }
        }
        Plan::Hit { d_ms } => {
            let e = n.music_count.saturating_add(d_ms);
            for p in arrows {
                st.last_event[p] = e;
            }
        }
    }
    if let Some(slot) = st.plans.get_mut(idx) {
        *slot = Some(plan);
    }
    if let Some(r) = st.resolved.get_mut(idx) {
        *r = true;
    }
    plan
}

/// Produce this frame's flag block (design §4.7). `notes` is the actor's
/// Results window in chart order; `mc` the current music count; `cur_beat`
/// the actor's beat position (freeze-hold input).
pub fn plan_frame(
    notes: &[NoteView],
    st: &mut SongState,
    rng: &mut Rng,
    c: &Curve,
    mc: i32,
    cur_beat: i32,
    out: &mut PanelFlags,
) {
    *out = PanelFlags::default();

    // Advance the cursor past leading notes that need no more work: judged,
    // with no freeze body still running. Tally them as they retire.
    while let Some(n) = notes.get(st.cursor) {
        if n.unjudged || freeze_active(n, cur_beat) {
            break;
        }
        st.tally_judged(n.idx);
        st.cursor += 1;
    }

    let horizon = mc.saturating_add(LOOKAHEAD_MS + MAX_EARLY_MS);
    let mut panel_taken = [false; 8];

    for n in notes.iter().skip(st.cursor) {
        if n.music_count > horizon {
            break;
        }

        // Freeze body — identical to the stock update: hold every body panel
        // while the body runs, judged or not.
        if freeze_body_running(n, cur_beat) {
            for p in 0..8 {
                if n.state[p] >= 2 {
                    out.was_just_pressed[p] = 1;
                }
            }
        }

        if !n.unjudged {
            st.tally_judged(n.idx);
            continue;
        }
        // Only kind-0 notes are tap-judged; a freeze tail (kind 2) is
        // resolved by the freeze judge from the hold state, so it is neither
        // decided nor pressed (the body hold above is all it needs).
        if n.kind != 0 {
            continue;
        }

        // Shock — identical to the stock update (assignment, not OR: the
        // stock deliberately clears a just-pressed arrow panel that the shock
        // also covers, which is what keeps every shock avoided).
        if is_shock(n) {
            for p in 0..8 {
                out.was_just_pressed[p] = u8::from(n.state[p] != 1);
            }
            continue;
        }

        let plan = resolve(st, n, rng, c);
        let Plan::Hit { d_ms } = plan else {
            continue;
        };
        let e = n.music_count.saturating_add(d_ms);
        if mc < e - LOOKAHEAD_MS {
            continue;
        }
        // One live event per panel per frame: the earliest note owns it. A
        // due note that cannot emit (a panel already taken — a jump waiting
        // on a shared panel) still RESERVES every panel it carries: a later
        // note emitting on one of them would hand its press to this note (the
        // judge attributes a held panel to the earliest unjudged note).
        let blocked = (0..8).any(|p| is_arrow_panel(n.state[p]) && panel_taken[p]);
        for p in (0..8).filter(|&p| is_arrow_panel(n.state[p])) {
            panel_taken[p] = true;
            if blocked {
                continue;
            }
            out.is_held[p] = 1;
            out.was_just_pressed[p] = 1;
            out.event_mc[p] = e;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(idx: usize, mc: i32, panels: &[usize]) -> NoteView {
        let mut state = [0i32; 8];
        for &p in panels {
            state[p] = 1;
        }
        NoteView {
            idx,
            kind: 0,
            music_count: mc,
            beat_count: mc / 10,
            state,
            length: [0; 8],
            unjudged: true,
        }
    }

    /// A curve that never misses and an RNG whose gaussian is irrelevant:
    /// tests pre-seed `plans` so `decide` is never consulted.
    fn fixed() -> (Curve, Rng) {
        (skill::curve(10), Rng::new(1))
    }

    fn st_with(plans: &[Plan]) -> SongState {
        let mut st = SongState::new(plans.len());
        for (i, p) in plans.iter().enumerate() {
            st.plans[i] = Some(*p);
        }
        st
    }

    #[test]
    fn single_note_late_hit_presses_at_e_minus_lookahead() {
        let notes = [note(0, 1000, &[2])];
        let mut st = st_with(&[Plan::Hit { d_ms: 30 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        // E = 1030; nothing before 1022.
        for mc in [900, 1000, 1021] {
            plan_frame(&notes, &mut st, &mut rng, &c, mc, 0, &mut out);
            assert_eq!(out, PanelFlags::default(), "mc={mc}");
        }
        for mc in [1022, 1030, 1100] {
            plan_frame(&notes, &mut st, &mut rng, &c, mc, 0, &mut out);
            assert_eq!(out.is_held[2], 1, "mc={mc}");
            assert_eq!(out.was_just_pressed[2], 1, "mc={mc}");
            assert_eq!(out.event_mc[2], 1030, "mc={mc}");
            for p in (0..8).filter(|&p| p != 2) {
                assert_eq!(out.is_held[p], 0);
                assert_eq!(out.event_mc[p], 0);
            }
        }
        assert_eq!(st.last_event[2], 1030);
    }

    #[test]
    fn single_note_early_hit() {
        let notes = [note(0, 1000, &[0])];
        let mut st = st_with(&[Plan::Hit { d_ms: -40 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 951, 0, &mut out);
        assert_eq!(out, PanelFlags::default());
        plan_frame(&notes, &mut st, &mut rng, &c, 952, 0, &mut out);
        assert_eq!(out.event_mc[0], 960);
        assert_eq!(out.is_held[0], 1);
    }

    #[test]
    fn miss_never_presses_and_blocks_panel() {
        let notes = [note(0, 1000, &[1])];
        let mut st = st_with(&[Plan::Miss]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        for mc in [800, 1000, 1100, 1200] {
            plan_frame(&notes, &mut st, &mut rng, &c, mc, 0, &mut out);
            assert_eq!(out, PanelFlags::default(), "mc={mc}");
        }
        assert_eq!(st.blocked_until[1], 1161);
        assert_eq!(st.blocked_until[0], 0);
    }

    #[test]
    fn miss_blocks_the_next_same_panel_note() {
        // A misses at 1000; B (planned exact) at 1100 on the same panel.
        let notes = [note(0, 1000, &[3]), note(1, 1100, &[3])];
        let mut st = st_with(&[Plan::Miss, Plan::Hit { d_ms: 0 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1100, 0, &mut out);
        // B's event was pushed to >= 1161; not yet visible at 1100.
        assert_eq!(out.is_held[3], 0);
        plan_frame(&notes, &mut st, &mut rng, &c, 1153, 0, &mut out);
        assert_eq!(out.is_held[3], 1);
        assert_eq!(out.event_mc[3], 1161);
        // The plan was rewritten to the resolved offset (stable E).
        assert_eq!(st.plans[1], Some(Plan::Hit { d_ms: 61 }));

        // With B far enough away it is unaffected.
        let notes = [note(0, 1000, &[3]), note(1, 1300, &[3])];
        let mut st = st_with(&[Plan::Miss, Plan::Hit { d_ms: 0 }]);
        plan_frame(&notes, &mut st, &mut rng, &c, 1292, 0, &mut out);
        assert_eq!(out.event_mc[3], 1300);
    }

    #[test]
    fn pushed_event_past_miss_window_becomes_miss() {
        // A misses at 1000; B at 1030 on the same panel: B's floor is 1161,
        // 131 ms late — beyond the graded ±124 ⇒ B is a Miss too (the judge
        // would match and reject it forever). A 1040 B (121 ms) survives.
        let notes = [note(0, 1000, &[3]), note(1, 1030, &[3])];
        let mut st = st_with(&[Plan::Miss, Plan::Hit { d_ms: 0 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1200, 0, &mut out);
        assert_eq!(out, PanelFlags::default());
        assert_eq!(st.plans[1], Some(Plan::Miss));
        assert_eq!(st.blocked_until[3], 1191);

        let notes = [note(0, 1000, &[3]), note(1, 1040, &[3])];
        let mut st = st_with(&[Plan::Miss, Plan::Hit { d_ms: 0 }]);
        plan_frame(&notes, &mut st, &mut rng, &c, 1160, 0, &mut out);
        assert_eq!(st.plans[1], Some(Plan::Hit { d_ms: 121 }));
        assert_eq!(out.event_mc[3], 1161);
    }

    #[test]
    fn events_are_monotonic_in_note_order_not_emission_order() {
        // A (1000, planned +120 ⇒ 1120, a late Good) precedes B (1050,
        // planned −50 ⇒ 1000) on the same panel. Emitting B's 1000 first
        // would hand the press to A (the judge attributes a held panel to the
        // EARLIEST unjudged note). B must be floored to 1121 (a Great).
        let notes = [note(0, 1000, &[1]), note(1, 1050, &[1])];
        let mut st = st_with(&[Plan::Hit { d_ms: 120 }, Plan::Hit { d_ms: -50 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        for mc in [992, 1000, 1100, 1111] {
            plan_frame(&notes, &mut st, &mut rng, &c, mc, 0, &mut out);
            assert_eq!(out, PanelFlags::default(), "mc={mc}");
        }
        // A emits first.
        plan_frame(&notes, &mut st, &mut rng, &c, 1112, 0, &mut out);
        assert_eq!(out.event_mc[1], 1120);
        // A judged; B emits at its floored event, never the same frame as A.
        let mut judged = notes;
        judged[0].unjudged = false;
        plan_frame(&judged, &mut st, &mut rng, &c, 1113, 0, &mut out);
        assert_eq!(out.event_mc[1], 1121);
        assert_eq!(st.plans[1], Some(Plan::Hit { d_ms: 71 }));
    }

    #[test]
    fn one_event_per_panel_per_frame() {
        // Both due on the same frame (E_A = 1005, E_B = 1006 after
        // flooring): only A's event may occupy the panel; B waits a frame.
        let notes = [note(0, 1000, &[2]), note(1, 1001, &[2])];
        let mut st = st_with(&[Plan::Hit { d_ms: 5 }, Plan::Hit { d_ms: 0 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 0, &mut out);
        assert_eq!(out.event_mc[2], 1005);
        // B resolved (floored to A's E + 1) on its first walk, but not emitted.
        assert_eq!(st.plans[1], Some(Plan::Hit { d_ms: 5 }));
        let mut judged = notes;
        judged[0].unjudged = false;
        plan_frame(&judged, &mut st, &mut rng, &c, 1008, 0, &mut out);
        assert_eq!(out.event_mc[2], 1006);
        assert_eq!(st.plans[1], Some(Plan::Hit { d_ms: 5 }));
    }

    #[test]
    fn jump_shares_one_event() {
        let notes = [note(0, 1000, &[0, 3])];
        let mut st = st_with(&[Plan::Hit { d_ms: 12 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1010, 0, &mut out);
        assert_eq!(out.event_mc[0], 1012);
        assert_eq!(out.event_mc[3], 1012);
        assert_eq!(out.is_held[0], 1);
        assert_eq!(out.is_held[3], 1);
        assert_eq!(out.is_held[1], 0);
    }

    #[test]
    fn jump_event_respects_the_later_blocked_panel() {
        // Panel 0 blocked until 1050 by an earlier miss; a jump on 0+3 at
        // 1000 planned exact must move BOTH panels' event to 1050.
        let notes = [note(0, 900, &[0]), note(1, 1000, &[0, 3])];
        let mut st = st_with(&[Plan::Miss, Plan::Hit { d_ms: 0 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1060, 0, &mut out);
        assert_eq!(out.event_mc[0], 1061);
        assert_eq!(out.event_mc[3], 1061);
    }

    #[test]
    fn freeze_body_holds_while_in_length() {
        let mut n = note(0, 1000, &[2]);
        n.state[2] = 2; // freeze body marker
        n.length[2] = 50; // beats
        n.beat_count = 100;
        n.unjudged = false; // already judged head — body still holds
        let notes = [n];
        let mut st = SongState::new(1);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 120, &mut out);
        assert_eq!(out.was_just_pressed[2], 1);
        assert_eq!(out.is_held[2], 0);
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 150, &mut out);
        assert_eq!(out.was_just_pressed[2], 0);
    }

    #[test]
    fn freeze_tail_is_never_decided_or_pressed() {
        let mut tail = note(1, 1500, &[2]);
        tail.kind = 2;
        let notes = [note(0, 1000, &[2]), tail];
        let mut st = st_with(&[Plan::Hit { d_ms: 0 }]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        let mut judged = notes;
        judged[0].unjudged = false;
        plan_frame(&judged, &mut st, &mut rng, &c, 1500, 0, &mut out);
        assert_eq!(out, PanelFlags::default());
        assert_eq!(st.plans.get(1).copied().flatten(), None);
    }

    #[test]
    fn shock_presses_only_non_shock_panels() {
        let mut n = note(0, 1000, &[]);
        n.state = [1, 1, 1, 1, 0, 0, 0, 0]; // P1 pad shock
        let notes = [n];
        let mut st = SongState::new(1);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 0, &mut out);
        assert_eq!(out.was_just_pressed[..4], [0, 0, 0, 0]);
        assert_eq!(out.was_just_pressed[4..], [1, 1, 1, 1]);
        assert_eq!(out.is_held, [0; 8]);
        assert_eq!(st.plans[0], None, "shocks are never decided");
    }

    #[test]
    fn cursor_skips_judged_and_lookahead_bounds_the_walk() {
        let notes = [
            note(0, 1000, &[0]),
            note(1, 1100, &[1]),
            note(2, 5000, &[2]),
        ];
        let mut st = st_with(&[
            Plan::Hit { d_ms: 0 },
            Plan::Hit { d_ms: 0 },
            Plan::Hit { d_ms: 0 },
        ]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        let mut judged = notes;
        judged[0].unjudged = false;
        plan_frame(&judged, &mut st, &mut rng, &c, 1100, 0, &mut out);
        assert_eq!(st.cursor, 1);
        // Note 2 is 3900 ms away — beyond LOOKAHEAD + MAX_EARLY: untouched.
        assert_eq!(out.event_mc[2], 0);
        judged[1].unjudged = false;
        plan_frame(&judged, &mut st, &mut rng, &c, 1200, 0, &mut out);
        assert_eq!(st.cursor, 2);
        plan_frame(&judged, &mut st, &mut rng, &c, 1100, 0, &mut out);
        assert_eq!(st.cursor, 2, "cursor never regresses");
    }

    #[test]
    fn plans_grow_for_unexpected_indices() {
        let notes = [note(7, 1000, &[0])];
        let mut st = SongState::new(0);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 0, &mut out);
        assert!(st.plans.len() >= 8);
    }

    #[test]
    fn tally_counts_each_note_once() {
        let mut notes = vec![
            note(0, 1000, &[0]),
            note(1, 1200, &[1]),
            note(2, 1400, &[2]),
            note(3, 1600, &[3]),
        ];
        let mut st = st_with(&[
            Plan::Hit { d_ms: 5 },   // Marv
            Plan::Hit { d_ms: -30 }, // Perfect
            Plan::Miss,
            Plan::Hit { d_ms: 100 }, // Good
        ]);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        // Resolve every plan by walking once with all notes in the window.
        plan_frame(&notes, &mut st, &mut rng, &c, 1700, 0, &mut out);
        assert_eq!(st.tally(), [0; 6]);
        // Judge them (in any order) — each is tallied exactly once.
        for n in notes.iter_mut() {
            n.unjudged = false;
        }
        plan_frame(&notes, &mut st, &mut rng, &c, 1800, 0, &mut out);
        plan_frame(&notes, &mut st, &mut rng, &c, 1900, 0, &mut out);
        assert_eq!(st.tally(), [1, 1, 0, 1, 0, 1]);
    }

    // The end-to-end properties (per-panel monotonic events over random
    // streams, every note reaching a verdict, tally == note count) are
    // checked against a faithful judge model over the REAL chart corpus in
    // `tools/bot_sim` — see its `judge_model` tests.
}
