//! Pure resolver chain for `services::ssq_path` — no game or crate
//! dependencies, host-tested via `scripts/validate_stepmania.sh`.
//!
//! A [`Chain`] holds `(priority, resolver)` pairs sorted by ascending
//! priority (stable: equal priorities keep registration order). `resolve`
//! asks each resolver in turn and returns the FIRST `Some`; `None` from
//! every resolver means "let the game's original builder answer". The
//! answer type is generic so the chain can be tested without the game's
//! `Choice` enum.

/// `fn(basename bytes, difficulty 0..4) -> Option<answer>`.
pub type Resolver<C> = fn(&[u8], usize) -> Option<C>;

/// Ordered resolver list. Cheap to clone (fn pointers), so the live chain
/// is replaced wholesale on each registration and read lock-free.
#[derive(Debug, Clone)]
pub struct Chain<C> {
    entries: Vec<(u8, Resolver<C>)>,
}

impl<C> Default for Chain<C> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<C> Chain<C> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `resolver` after every entry with priority ≤ `priority`
    /// (ascending order; FIFO among equals).
    pub fn register(&mut self, priority: u8, resolver: Resolver<C>) {
        let pos = self
            .entries
            .iter()
            .position(|(p, _)| *p > priority)
            .unwrap_or(self.entries.len());
        self.entries.insert(pos, (priority, resolver));
    }

    /// First `Some` in priority order; `None` when no resolver answers.
    #[must_use]
    pub fn resolve(&self, basename: &[u8], difficulty: usize) -> Option<C> {
        self.entries
            .iter()
            .find_map(|(_, r)| r(basename, difficulty))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Registered priorities in chain order (for the install log).
    #[must_use]
    pub fn priorities(&self) -> Vec<u8> {
        self.entries.iter().map(|(p, _)| *p).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers_a(b: &[u8], _d: usize) -> Option<u8> {
        (b == b"a").then_some(1)
    }
    fn answers_b(b: &[u8], _d: usize) -> Option<u8> {
        (b == b"b").then_some(2)
    }
    fn answers_all(_b: &[u8], d: usize) -> Option<u8> {
        Some(10 + d as u8)
    }
    fn answers_a_late(b: &[u8], _d: usize) -> Option<u8> {
        (b == b"a").then_some(99)
    }

    #[test]
    fn empty_chain_forwards_everything() {
        let c: Chain<u8> = Chain::new();
        assert!(c.is_empty());
        assert_eq!(c.resolve(b"acef", 3), None);
    }

    #[test]
    fn registration_order_is_by_ascending_priority() {
        let mut c = Chain::new();
        c.register(20, answers_all);
        c.register(10, answers_a);
        c.register(15, answers_b);
        assert_eq!(c.priorities(), vec![10, 15, 20]);
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn first_answer_wins() {
        let mut c = Chain::new();
        c.register(20, answers_all);
        c.register(10, answers_a);
        // `a` is answered by the priority-10 resolver even though 20 also would.
        assert_eq!(c.resolve(b"a", 3), Some(1));
        // `b` falls through to the catch-all.
        assert_eq!(c.resolve(b"b", 4), Some(14));
    }

    #[test]
    fn none_from_every_resolver_means_original() {
        let mut c = Chain::new();
        c.register(10, answers_a);
        c.register(20, answers_b);
        assert_eq!(c.resolve(b"a", 0), Some(1));
        assert_eq!(c.resolve(b"b", 0), Some(2));
        assert_eq!(c.resolve(b"zzzz", 0), None);
    }

    #[test]
    fn equal_priorities_keep_registration_order() {
        let mut c = Chain::new();
        c.register(10, answers_a);
        c.register(10, answers_a_late);
        assert_eq!(c.resolve(b"a", 0), Some(1));
        let mut c = Chain::new();
        c.register(10, answers_a_late);
        c.register(10, answers_a);
        assert_eq!(c.resolve(b"a", 0), Some(99));
    }

    #[test]
    fn clone_is_an_independent_snapshot() {
        let mut c = Chain::new();
        c.register(10, answers_a);
        let snap = c.clone();
        c.register(5, answers_all);
        assert_eq!(snap.resolve(b"q", 0), None);
        assert_eq!(c.resolve(b"q", 0), Some(10));
    }
}
