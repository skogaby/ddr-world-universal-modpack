//! The per-side preview state machine — PURE (std only; the host harness
//! mounts it), generic over the preview identity `I` (the driver uses
//! `(Kind, key)`). It answers ONE question every frame: given what the
//! options UI last told us (which row is focused, which non-RANDOM value it
//! holds) and what is currently live, what should the driver do now —
//! nothing, tear the live preview down, or start `I`.
//!
//! Rules (design §4.6 / FR-10):
//! - a request for a non-RANDOM value of OUR row sets `wanted` and, when the
//!   value CHANGED, re-arms the settle deadline `now + SETTLE_MS`; identical
//!   requests (the getter fires every focus tick) leave the deadline alone;
//! - a request for RANDOM, or for another row, clears `wanted` (the live
//!   preview is torn down at the next poll);
//! - `on_clear` (modal closed / scene left) clears everything but `live`;
//! - `poll`: `wanted == live` ⇒ nothing; a live preview that no longer
//!   matches ⇒ `Teardown` (the driver calls back `mark_torn_down` once the
//!   scene is gone, then polls again); no live preview and a settled
//!   `wanted` ⇒ `Start`.

/// Milliseconds between the last value change and the (re)start.
pub const SETTLE_MS: u64 = 150;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action<I> {
    None,
    /// Tear the live preview down (the driver reports back with
    /// [`SlotState::mark_torn_down`]).
    Teardown,
    /// Start this preview (the driver reports back with
    /// [`SlotState::mark_started`]).
    Start(I),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotState<I: Clone + PartialEq> {
    /// One of OUR rows is focused (RANDOM included).
    focused: bool,
    /// The settled-or-settling target.
    wanted: Option<I>,
    /// `now_ms` at/after which `wanted` may start.
    settle_due: Option<u64>,
    /// What is live (or starting).
    live: Option<I>,
}

impl<I: Clone + PartialEq> Default for SlotState<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Clone + PartialEq> SlotState<I> {
    /// `const` so a slot can live in a `static` (the driver's per-side array).
    pub const fn new() -> Self {
        SlotState {
            focused: false,
            wanted: None,
            settle_due: None,
            live: None,
        }
    }

    /// The options UI asked our row (or another row) for its preview.
    /// `focused_ours` = the focused row is one of ours; `wanted` = its
    /// non-RANDOM identity (`None` for RANDOM or another row).
    pub fn on_request(&mut self, focused_ours: bool, wanted: Option<I>, now_ms: u64) {
        self.focused = focused_ours;
        if self.wanted != wanted {
            self.settle_due = wanted.as_ref().map(|_| now_ms.saturating_add(SETTLE_MS));
            self.wanted = wanted;
        }
    }

    /// Modal closed / scene left: nothing is wanted any more (the live
    /// preview is torn down at the next poll).
    pub fn on_clear(&mut self) {
        self.focused = false;
        self.wanted = None;
        self.settle_due = None;
    }

    /// What to do now.
    pub fn poll(&self, now_ms: u64) -> Action<I> {
        match (&self.wanted, &self.live) {
            (None, None) => Action::None,
            (None, Some(_)) => Action::Teardown,
            (Some(w), Some(l)) if w == l => Action::None,
            (Some(_), Some(_)) => Action::Teardown,
            (Some(w), None) => match self.settle_due {
                Some(due) if now_ms < due => Action::None,
                _ => Action::Start(w.clone()),
            },
        }
    }

    pub fn mark_started(&mut self, id: I) {
        self.live = Some(id);
    }

    pub fn mark_torn_down(&mut self) {
        self.live = None;
    }

    pub fn live(&self) -> Option<&I> {
        self.live.as_ref()
    }

    pub fn wanted(&self) -> Option<&I> {
        self.wanted.as_ref()
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Anything to drive: a live preview, or a pending start.
    pub fn is_idle(&self) -> bool {
        self.live.is_none() && self.wanted.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type S = SlotState<(u8, &'static str)>;
    const A: (u8, &str) = (0, "emi01");
    const B: (u8, &str) = (0, "rage00");
    const ST: (u8, &str) = (1, "boom00");

    #[test]
    fn settle_then_start() {
        let mut s = S::new();
        assert_eq!(s.poll(0), Action::None);
        assert!(s.is_idle());
        s.on_request(true, Some(A), 0);
        assert!(s.is_focused() && !s.is_idle());
        assert_eq!(s.poll(149), Action::None);
        assert_eq!(s.poll(150), Action::Start(A));
        s.mark_started(A);
        assert_eq!(s.live(), Some(&A));
        assert_eq!(s.poll(1000), Action::None);
    }

    #[test]
    fn value_change_rearms_but_repeats_do_not() {
        let mut s = S::new();
        s.on_request(true, Some(A), 0);
        s.on_request(true, Some(B), 100);
        assert_eq!(s.poll(149), Action::None);
        assert_eq!(s.poll(249), Action::None);
        assert_eq!(s.poll(250), Action::Start(B));
        // Focus ticks with the same value keep the deadline.
        let mut r = S::new();
        r.on_request(true, Some(A), 0);
        r.on_request(true, Some(A), 100);
        r.on_request(true, Some(A), 140);
        assert_eq!(r.poll(150), Action::Start(A));
    }

    #[test]
    fn random_or_other_row_tears_down() {
        let mut s = S::new();
        s.on_request(true, Some(A), 0);
        s.mark_started(A);
        // RANDOM on our row: focused stays true, nothing wanted.
        s.on_request(true, None, 500);
        assert!(s.is_focused());
        assert_eq!(s.poll(500), Action::Teardown);
        s.mark_torn_down();
        assert_eq!(s.poll(501), Action::None);
        assert!(s.is_idle());
        // Another row.
        s.on_request(true, Some(A), 600);
        s.mark_started(A);
        s.on_request(false, None, 700);
        assert!(!s.is_focused());
        assert_eq!(s.poll(700), Action::Teardown);
    }

    #[test]
    fn clear_then_torn_down_is_idle() {
        let mut s = S::new();
        s.on_request(true, Some(ST), 0);
        s.mark_started(ST);
        s.on_clear();
        assert!(!s.is_focused());
        assert_eq!(s.poll(1), Action::Teardown);
        s.mark_torn_down();
        assert_eq!(s.poll(2), Action::None);
        assert!(s.is_idle());
        // A clear with nothing live is a no-op.
        let mut e = S::new();
        e.on_request(true, Some(A), 0);
        e.on_clear();
        assert_eq!(e.poll(1000), Action::None);
        assert!(e.is_idle());
    }

    #[test]
    fn retarget_while_live_is_teardown_then_start() {
        let mut s = S::new();
        s.on_request(true, Some(A), 0);
        s.mark_started(A);
        s.on_request(true, Some(ST), 1000);
        // Immediately a teardown (no settle wait for the teardown itself).
        assert_eq!(s.poll(1001), Action::Teardown);
        s.mark_torn_down();
        // The start still honours the settle from the change instant.
        assert_eq!(s.poll(1100), Action::None);
        assert_eq!(s.poll(1150), Action::Start(ST));
        // Wanted == live ⇒ nothing.
        s.mark_started(ST);
        s.on_request(true, Some(ST), 2000);
        assert_eq!(s.poll(2000), Action::None);
    }
}
