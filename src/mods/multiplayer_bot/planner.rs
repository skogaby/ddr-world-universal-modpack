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
//!
//! **Target Score replay** (`SongState::with_ghost`): the raw decision of
//! every tap comes from `ghost::decide_tap` on the note's ghost byte instead
//! of the skill model; a ghost N.G. on a freeze tail drops the body hold of
//! that freeze (head AND tail entries — both emit the hold), and a ghost N.G.
//! on a shock presses ONE shock panel from the note's music count (the shock
//! judge keys on `wasJustPressed`; the tap judge on `isHeld`, which stays 0).
//! Everything else — floors, one-event-per-panel, the tally — is unchanged.

use super::ghost;
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

/// The Target Score replay's decision source: the human's ghost bytes (one
/// grade class per Results index) and the armed S-Marvelous floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostPlan {
    pub bytes: Vec<u8>,
    /// Armed S-Marvelous window (`0` = none): Marvelous samples above it.
    pub smarv_floor: i32,
}

/// Forward scan bound for a freeze head's tail lookup (entries).
const TAIL_LOOKUP_LIMIT: usize = 512;

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
    /// Target Score replay source (`None` = the skill model).
    ghost: Option<GhostPlan>,
    /// Per Results index: this freeze entry's body hold is DROPPED (a ghost
    /// N.G. on the tail). Set on both the head and its tail.
    drop_hold: Vec<bool>,
    /// Taps whose resolved grade (after the floors) differs from the ghost's
    /// — the replay-fidelity counter of the song-end diagnostic.
    repro_miss: u32,
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
            ghost: None,
            drop_hold: vec![false; capacity],
            repro_miss: 0,
        }
    }

    /// A Target Score replay: decisions come from `bytes[idx]` (see the
    /// module docs). The caller has already checked `bytes.len()` against
    /// the Results count; an index past the vector falls back to the skill
    /// model note by note (defensive — it cannot happen after that check).
    pub fn with_ghost(capacity: usize, bytes: Vec<u8>, smarv_floor: i32) -> Self {
        let mut st = Self::new(capacity);
        st.ghost = Some(GhostPlan { bytes, smarv_floor });
        st
    }

    /// Whether this song replays a ghost.
    pub fn is_ghost(&self) -> bool {
        self.ghost.is_some()
    }

    /// The ghost byte for a Results index (Target mode only).
    fn ghost_byte(&self, idx: usize) -> Option<u8> {
        self.ghost.as_ref()?.bytes.get(idx).copied()
    }

    /// Planned grade counts of the notes observed judged so far.
    pub fn tally(&self) -> [u32; 6] {
        self.tally
    }

    /// Taps whose resolved grade differs from the ghost's (Target mode).
    pub fn repro_miss(&self) -> u32 {
        self.repro_miss
    }

    /// Whether `idx`'s freeze body hold is dropped (test/diagnostic view).
    pub fn hold_dropped(&self, idx: usize) -> bool {
        self.drop_hold.get(idx).copied().unwrap_or(false)
    }

    fn is_resolved(&self, idx: usize) -> bool {
        self.resolved.get(idx).copied().unwrap_or(false)
    }

    fn ensure(&mut self, idx: usize) {
        if idx >= self.plans.len() {
            self.plans.resize(idx + 1, None);
            self.resolved.resize(idx + 1, false);
            self.tallied.resize(idx + 1, false);
        }
        if idx >= self.drop_hold.len() {
            self.drop_hold.resize(idx + 1, false);
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

/// A freeze head: a tap note with a held panel (`length > 0`).
fn is_freeze_head(n: &NoteView) -> bool {
    n.kind == 0 && n.length.iter().any(|&l| l > 0)
}

/// The Results index of `head`'s tail: the first later kind-2 entry at the
/// head's end beat sharing a body panel (bounded scan). `notes` is the whole
/// Results view in chart order.
fn find_tail(notes: &[NoteView], head_pos: usize, head: &NoteView) -> Option<usize> {
    let end_beat = head.beat_count.saturating_add(max_length(head));
    notes
        .iter()
        .skip(head_pos + 1)
        .take(TAIL_LOOKUP_LIMIT)
        .find(|t| {
            t.kind == 2
                && t.beat_count == end_beat
                && (0..8).any(|p| head.length[p] > 0 && t.state[p] >= 2)
        })
        .map(|t| t.idx)
}

/// Resolve `idx`'s decision (once): decide if needed, then apply the
/// per-panel floors in note order; a floor past the Boo window turns the hit
/// into a Miss. Updates `blocked_until` / `last_event` accordingly. In Target
/// mode the raw decision is the ghost byte's band sample, and a resolved
/// grade that differs from the ghost's counts a reproduction miss.
fn resolve(st: &mut SongState, n: &NoteView, rng: &mut Rng, c: &Curve) -> Plan {
    let idx = n.idx;
    st.ensure(idx);
    if st.resolved.get(idx).copied().unwrap_or(false) {
        if let Some(Some(p)) = st.plans.get(idx) {
            return *p;
        }
    }
    let ghost_byte = st.ghost_byte(idx);
    let raw = match st.plans.get(idx).copied().flatten() {
        Some(p) => p,
        None => {
            let smarv_floor = st.ghost.as_ref().map_or(0, |g| g.smarv_floor);
            let form = st.form.get_or_insert_with(|| Form::new(rng, c));
            match ghost_byte {
                Some(byte) => ghost::decide_tap(rng, form, c, byte, smarv_floor),
                None => skill::decide(rng, form, c),
            }
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
    if let Some(byte) = ghost_byte {
        let resolved_grade = match plan {
            Plan::Hit { d_ms } => skill::grade_for_offset(d_ms),
            Plan::Miss => skill::GRADE_MISS,
        };
        if resolved_grade != ghost::expected_tap_grade(byte) {
            st.repro_miss = st.repro_miss.saturating_add(1);
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

/// Target mode: when a freeze head resolves, look up its tail's ghost byte;
/// an N.G. drops the body hold on both entries (the freeze judge resolves
/// N.G. as soon as any body panel is released — no hold at all is enough).
fn plan_freeze_hold(st: &mut SongState, notes: &[NoteView], head_pos: usize, head: &NoteView) {
    if !st.is_ghost() || !is_freeze_head(head) {
        return;
    }
    let Some(tail_idx) = find_tail(notes, head_pos, head) else {
        return;
    };
    if st.ghost_byte(tail_idx) != Some(ghost::GRADE_NG) {
        return;
    }
    st.ensure(head.idx);
    st.ensure(tail_idx);
    if let Some(d) = st.drop_hold.get_mut(head.idx) {
        *d = true;
    }
    if let Some(d) = st.drop_hold.get_mut(tail_idx) {
        *d = true;
    }
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

    for (pos, n) in notes.iter().enumerate().skip(st.cursor) {
        if n.music_count > horizon {
            break;
        }

        // Freeze body — identical to the stock update: hold every body panel
        // while the body runs, judged or not. Target mode drops the hold of
        // a freeze the ghost N.G.'d.
        if freeze_body_running(n, cur_beat) && !st.hold_dropped(n.idx) {
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
        // also covers, which is what keeps every shock avoided). Target
        // mode reproduces a ghost N.G. by pressing ONE shock panel from the
        // note's music count (inside the judge's [mc−34, mc+84] window).
        if is_shock(n) {
            for p in 0..8 {
                out.was_just_pressed[p] = u8::from(n.state[p] != 1);
            }
            if st.ghost_byte(n.idx) == Some(ghost::GRADE_NG) && mc >= n.music_count {
                if let Some(p) = (0..8).find(|&p| n.state[p] == 1) {
                    out.was_just_pressed[p] = 1;
                }
            }
            continue;
        }

        if !st.is_resolved(n.idx) {
            plan_freeze_hold(st, notes, pos, n);
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

    // ── Target Score replay ─────────────────────────────────────────────

    /// A freeze on panel `p` from `mc` lasting `len` beats: the head (kind 0,
    /// state REP + length) at `idx` and its tail (kind 2, same states) at
    /// `idx + 1`.
    fn freeze(idx: usize, mc: i32, p: usize, len: i32) -> [NoteView; 2] {
        let mut head = note(idx, mc, &[]);
        head.state[p] = 4;
        head.length[p] = len;
        let mut tail = NoteView {
            idx: idx + 1,
            kind: 2,
            music_count: mc + len * 10,
            beat_count: head.beat_count + len,
            state: head.state,
            length: [0; 8],
            unjudged: true,
        };
        tail.state[p] = 4;
        [head, tail]
    }

    fn shock(idx: usize, mc: i32) -> NoteView {
        let mut n = note(idx, mc, &[]);
        n.state = [1, 1, 1, 1, 0, 0, 0, 0];
        n
    }

    #[test]
    fn ghost_taps_resolve_to_the_ghost_grade() {
        // Marvelous, Perfect, Great, Good, Miss — spaced so no floor applies.
        let notes = [
            note(0, 1000, &[0]),
            note(1, 2000, &[1]),
            note(2, 3000, &[2]),
            note(3, 4000, &[3]),
            note(4, 5000, &[0]),
        ];
        for seed in 0..50u64 {
            let mut st = SongState::with_ghost(5, vec![0, 1, 2, 3, 5], 0);
            let (c, _) = fixed();
            let mut rng = Rng::new(seed);
            let mut out = PanelFlags::default();
            plan_frame(&notes, &mut st, &mut rng, &c, 5000, 0, &mut out);
            for (i, expect) in [0u8, 1, 2, 3].iter().enumerate() {
                match st.plans[i] {
                    Some(Plan::Hit { d_ms }) => {
                        assert_eq!(
                            skill::grade_for_offset(d_ms),
                            *expect,
                            "note {i} seed {seed}"
                        )
                    }
                    other => panic!("note {i}: {other:?}"),
                }
            }
            assert_eq!(st.plans[4], Some(Plan::Miss));
            assert_eq!(st.repro_miss(), 0);
            assert!(st.is_ghost());
        }
    }

    #[test]
    fn ghost_marvelous_respects_the_smarv_floor() {
        let notes: Vec<NoteView> = (0..200)
            .map(|i| note(i, 1000 + i as i32 * 500, &[i % 4]))
            .collect();
        let mut st = SongState::with_ghost(200, vec![0; 200], 12);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        for mc in (1000..=101_000).step_by(100) {
            plan_frame(&notes, &mut st, &mut rng, &c, mc, 0, &mut out);
        }
        let mut edge = false;
        for (i, p) in st.plans.iter().enumerate() {
            let Some(Plan::Hit { d_ms }) = p else {
                panic!("note {i}: {p:?}");
            };
            assert!(d_ms.abs() > 12 && d_ms.abs() <= 17, "note {i}: {d_ms}");
            edge |= d_ms.abs() == 17;
        }
        assert!(edge);
        assert_eq!(st.repro_miss(), 0);
    }

    #[test]
    fn ghost_repro_miss_counts_floor_induced_grade_changes() {
        // A ghost Miss at 1000 then a ghost Marvelous at 1030 on the same
        // panel: the floor (1161) pushes the second past ±124 ⇒ a Miss the
        // ghost did not have.
        let notes = [note(0, 1000, &[3]), note(1, 1030, &[3])];
        let mut st = SongState::with_ghost(2, vec![5, 0], 0);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1200, 0, &mut out);
        assert_eq!(st.plans[0], Some(Plan::Miss));
        assert_eq!(st.plans[1], Some(Plan::Miss));
        assert_eq!(st.repro_miss(), 1);
    }

    #[test]
    fn ghost_index_past_the_vector_falls_back_to_the_skill_model() {
        let notes = [note(0, 1000, &[0]), note(1, 2000, &[1])];
        let mut st = SongState::with_ghost(2, vec![5], 0); // one byte short
        let (c, mut rng) = fixed(); // L10: never misses
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 2000, 0, &mut out);
        assert_eq!(st.plans[0], Some(Plan::Miss));
        assert!(matches!(st.plans[1], Some(Plan::Hit { .. })));
        assert_eq!(st.repro_miss(), 0, "only ghost-decided taps are compared");
    }

    #[test]
    fn ghost_freeze_ng_drops_the_body_hold_on_head_and_tail() {
        let [head, tail] = freeze(0, 1000, 2, 50);
        let notes = [head, tail];
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();

        // O.K. in the ghost: stock behaviour — head pressed, body held.
        let mut st = SongState::with_ghost(2, vec![0, 6], 0);
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 100, &mut out);
        assert_eq!(out.was_just_pressed[2], 1);
        assert!(!st.hold_dropped(0) && !st.hold_dropped(1));
        let mut judged = notes;
        judged[0].unjudged = false;
        plan_frame(&judged, &mut st, &mut rng, &c, 1100, 120, &mut out);
        assert_eq!(out.was_just_pressed[2], 1, "body held after the head");

        // N.G. in the ghost: the head is still tapped (its Marvelous event
        // lands somewhere in 983..=1017), the body never held.
        let mut st = SongState::with_ghost(2, vec![0, 7], 0);
        let mut out = PanelFlags::default();
        let mut tapped = false;
        for mc in 975..=1020 {
            plan_frame(&notes, &mut st, &mut rng, &c, mc, 100, &mut out);
            tapped |= out.is_held[2] == 1;
        }
        assert!(st.hold_dropped(0) && st.hold_dropped(1));
        assert!(tapped, "the head tap still lands");
        let mut judged = notes;
        judged[0].unjudged = false;
        plan_frame(&judged, &mut st, &mut rng, &c, 1100, 120, &mut out);
        assert_eq!(out.was_just_pressed[2], 0, "no body hold ⇒ N.G.");
        assert_eq!(out.is_held[2], 0);
        // Tail's own hold emission is dropped too.
        plan_frame(&judged, &mut st, &mut rng, &c, 1400, 140, &mut out);
        assert_eq!(out.was_just_pressed[2], 0);

        // Skill-model songs never touch the hold.
        let mut st = SongState::new(2);
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 100, &mut out);
        assert!(!st.hold_dropped(0));
    }

    #[test]
    fn ghost_freeze_tail_lookup_needs_the_matching_tail() {
        // A tail at the wrong beat is not this head's tail: hold kept.
        let [head, mut tail] = freeze(0, 1000, 2, 50);
        tail.beat_count += 1;
        let notes = [head, tail];
        let mut st = SongState::with_ghost(2, vec![0, 7], 0);
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 100, &mut out);
        assert!(!st.hold_dropped(0));
        assert_eq!(out.was_just_pressed[2], 1);
    }

    #[test]
    fn ghost_shock_ng_presses_one_shock_panel_from_the_note() {
        let notes = [shock(0, 1000)];
        let (c, mut rng) = fixed();
        let mut out = PanelFlags::default();

        // O.K. in the ghost (or no ghost): stock avoidance.
        for mut st in [SongState::with_ghost(1, vec![6], 0), SongState::new(1)] {
            plan_frame(&notes, &mut st, &mut rng, &c, 1000, 0, &mut out);
            assert_eq!(out.was_just_pressed[..4], [0, 0, 0, 0]);
            assert_eq!(out.was_just_pressed[4..], [1, 1, 1, 1]);
        }

        // N.G. in the ghost: before the note nothing; from mc, panel 0.
        let mut st = SongState::with_ghost(1, vec![7], 0);
        plan_frame(&notes, &mut st, &mut rng, &c, 999, 0, &mut out);
        assert_eq!(out.was_just_pressed[..4], [0, 0, 0, 0]);
        plan_frame(&notes, &mut st, &mut rng, &c, 1000, 0, &mut out);
        assert_eq!(out.was_just_pressed[..4], [1, 0, 0, 0]);
        assert_eq!(out.was_just_pressed[4..], [1, 1, 1, 1]);
        assert_eq!(out.is_held, [0; 8], "a shock press is never a tap event");
        assert_eq!(out.event_mc, [0; 8]);
        assert_eq!(st.plans[0], None, "shocks are still never decided");
    }

    // The end-to-end properties (per-panel monotonic events over random
    // streams, every note reaching a verdict, tally == note count) are
    // checked against a faithful judge model over the REAL chart corpus in
    // `tools/bot_sim` — see its `judge_model` tests.
}
