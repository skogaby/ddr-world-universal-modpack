//! Elapsed deadlines and coalesced, cancellable render-thread continuations.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct Deadline(Instant);

#[derive(Debug, PartialEq, Eq)]
pub enum Wait<T> {
    Ready(T),
    Pending,
    TimedOut,
}

impl Deadline {
    pub fn new(now: Instant, timeout: Duration) -> Self {
        Self(now + timeout)
    }

    pub fn is_due(self, now: Instant) -> bool {
        now >= self.0
    }

    /// Observe readiness before expiring, including on a delayed boundary poll.
    pub fn poll<T>(self, ready: Option<T>, now: Instant) -> Wait<T> {
        match ready {
            Some(value) => Wait::Ready(value),
            None if self.is_due(now) => Wait::TimedOut,
            None => Wait::Pending,
        }
    }
}

/// One queued continuation per generation. Protect with the consumer's mutex;
/// reserve under the lock, then enqueue after releasing it. `begin` clears the
/// latch BEFORE work so requests arriving during a drain reserve the next pass.
pub struct PendingPump {
    generation: u64,
    pending: bool,
}

impl PendingPump {
    pub const fn new() -> Self {
        Self {
            generation: 0,
            pending: false,
        }
    }

    pub fn request(&mut self) -> Option<u64> {
        if self.pending {
            return None;
        }
        self.pending = true;
        Some(self.generation)
    }

    pub fn is_current(&self, generation: u64) -> bool {
        generation == self.generation
    }

    pub fn begin(&mut self, generation: u64) -> bool {
        if !self.is_current(generation) || !self.pending {
            return false;
        }
        self.pending = false;
        true
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn readiness_wins_before_at_and_after_deadline() {
        let start = Instant::now();
        let deadline = Deadline::new(start, Duration::from_secs(12));
        for seconds in [0, 11, 12, 13] {
            assert_eq!(
                deadline.poll(Some(42), start + Duration::from_secs(seconds)),
                Wait::Ready(42)
            );
        }
    }

    #[test]
    fn unresolved_deadlines_ignore_poll_count() {
        let start = Instant::now();
        for seconds in [2, 10, 12] {
            let duration = Duration::from_secs(seconds);
            let deadline = Deadline::new(start, duration);
            for hz in [1, 30, 60, 144, 600, 3600] {
                for tick in 0..seconds * hz {
                    let now = start + Duration::from_secs_f64(tick as f64 / hz as f64);
                    assert_eq!(deadline.poll::<()>(None, now), Wait::Pending);
                }
            }
            assert_eq!(deadline.poll::<()>(None, start + duration), Wait::TimedOut);
            assert_eq!(
                deadline.poll::<()>(None, start + duration + Duration::from_secs(1)),
                Wait::TimedOut
            );
        }
    }

    #[test]
    fn periodic_deadline_rearms_from_attempt_not_missed_frames() {
        let start = Instant::now();
        let mut next = None::<Deadline>;
        assert!(next.is_none_or(|d| d.is_due(start)));
        next = Some(Deadline::new(start, Duration::from_secs(2)));
        assert!(!next.unwrap().is_due(start + Duration::from_millis(1999)));
        assert!(next.unwrap().is_due(start + Duration::from_secs(2)));
        let delayed_attempt = start + Duration::from_secs(20);
        assert!(next.unwrap().is_due(delayed_attempt));
        next = Some(Deadline::new(delayed_attempt, Duration::from_secs(2)));
        assert!(!next.unwrap().is_due(delayed_attempt));
        assert!(next
            .unwrap()
            .is_due(delayed_attempt + Duration::from_secs(2)));
    }

    #[test]
    fn duplicate_requests_coalesce_until_execution_begins() {
        let mut pump = PendingPump::new();
        let token = pump.request().unwrap();
        for _ in 0..100 {
            assert_eq!(pump.request(), None);
        }
        assert!(pump.begin(token));
        assert!(!pump.begin(token));
        assert_eq!(pump.request(), Some(token));
    }

    #[test]
    fn completion_during_drain_and_self_requeue_share_one_continuation() {
        let mut pump = PendingPump::new();
        let token = pump.request().unwrap();
        assert!(pump.begin(token));
        // A synthesis completion after mailbox drain reserves the next pump.
        assert_eq!(pump.request(), Some(token));
        // The current pump's unresolved loads need the same continuation.
        assert_eq!(pump.request(), None);
        assert!(pump.begin(token));
    }

    #[test]
    fn cancellation_prevents_queued_activation_or_raise() {
        let mut pump = PendingPump::new();
        let old = pump.request().unwrap();
        pump.cancel();
        assert!(!pump.is_current(old));
        assert!(!pump.begin(old));
        let new = pump.request().unwrap();
        assert_ne!(old, new);
        assert!(pump.begin(new));
    }

    #[test]
    fn stale_callback_cannot_consume_reopened_sessions_pending_work() {
        let mut pump = PendingPump::new();
        let old = pump.request().unwrap();
        pump.cancel();
        let new = pump.request().unwrap();
        assert!(!pump.begin(old));
        assert_eq!(pump.request(), None);
        assert!(pump.begin(new));
    }

    #[test]
    fn execution_guard_refusal_does_not_leave_pending_latch_set() {
        let mut pump = PendingPump::new();
        let token = pump.request().unwrap();
        assert!(pump.begin(token));
        // Consumer finds the menu open / scene changed and returns here.
        // A later eligible request must still be able to reserve work.
        assert_eq!(pump.request(), Some(token));
    }

    #[test]
    fn concurrent_completions_reserve_only_one_pump() {
        let pump = Arc::new(Mutex::new(PendingPump::new()));
        for _ in 0..2 {
            let barrier = Arc::new(Barrier::new(8));
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let pump = Arc::clone(&pump);
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        pump.lock().unwrap().request()
                    })
                })
                .collect();
            let tokens: Vec<_> = handles
                .into_iter()
                .filter_map(|handle| handle.join().unwrap())
                .collect();
            assert_eq!(tokens.len(), 1);
            assert!(pump.lock().unwrap().begin(tokens[0]));
        }
    }
}
