//! Fixed-size correlation and cursor continuity for passive XACT observations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy)]
pub struct CallTiming {
    pub entered: i64,
    pub original_begin: i64,
    pub original_end: i64,
}

impl CallTiming {
    pub fn pre_ticks(self) -> Option<i64> {
        (self.entered >= 0 && self.original_begin >= self.entered)
            .then(|| self.original_begin - self.entered)
    }
}

/// Keep expensive ownership inspection OUTSIDE the original-call interval,
/// while exposing the latency it introduced before that call.
pub fn observed_timed<A, R>(
    clock: impl Fn() -> i64,
    before: impl FnOnce(i64) -> A,
    original: impl FnOnce() -> R,
    after: impl FnOnce(Option<A>, &R, CallTiming),
) -> R {
    let stamp = || std::panic::catch_unwind(std::panic::AssertUnwindSafe(&clock)).unwrap_or(-1);
    let entered = stamp();
    let snapshot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| before(entered))).ok();
    let original_begin = stamp();
    let result = original();
    let original_end = stamp();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        after(
            snapshot,
            &result,
            CallTiming {
                entered,
                original_begin,
                original_end,
            },
        )
    }));
    result
}

/// Observer failures cannot skip/repeat original. Never catch and retry original.
pub fn observed<A, R>(
    before: impl FnOnce() -> A,
    original: impl FnOnce() -> R,
    after: impl FnOnce(Option<A>, &R),
) -> R {
    let snapshot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(before)).ok();
    let result = original();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| after(snapshot, &result)));
    result
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Origin {
    pub attempt: u64,
    pub scene: i32,
    pub epoch: u64,
    pub valid: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Owners {
    pub cue: usize,
    pub sound: usize,
    pub bank: usize,
}

impl Owners {
    fn bound(self) -> bool {
        self.cue != 0 && self.bank != 0
    }

    pub fn valid(self) -> bool {
        self.bound() && self.sound != 0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub generation: u64,
    pub epoch: u64,
    pub handle: i32,
    pub origin: Origin,
    pub name: [u8; 32],
    pub owners: Owners,
}

struct State {
    epoch: u64,
    entries: [Option<Token>; 256],
}

pub struct Correlation {
    state: Mutex<State>,
    epoch: AtomicU64,
    generation: AtomicU64,
    lost: AtomicU64,
}

impl Correlation {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                epoch: 1,
                entries: [None; 256],
            }),
            epoch: AtomicU64::new(1),
            generation: AtomicU64::new(1),
            lost: AtomicU64::new(0),
        }
    }

    fn edit<R>(&self, critical: bool, f: impl FnOnce(&mut State) -> R) -> Option<R> {
        let Ok(mut state) = self.state.try_lock() else {
            if critical {
                self.lost.fetch_add(1, Ordering::Relaxed);
                self.invalidate_all();
            }
            return None;
        };
        let epoch = self.epoch.load(Ordering::Acquire);
        if state.epoch != epoch {
            state.entries.fill(None);
            state.epoch = epoch;
        }
        let value = f(&mut state);
        (self.epoch.load(Ordering::Acquire) == epoch).then_some(value)
    }

    pub fn invalidate_all(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub fn lost(&self) -> u64 {
        self.lost.load(Ordering::Relaxed)
    }

    pub fn current(&self, token: Token) -> bool {
        token.epoch == self.epoch.load(Ordering::Acquire)
    }

    pub fn prepare(&self, slot: i32, handle: i32, name: [u8; 32], origin: Origin) {
        self.prepare_bound(slot, handle, name, origin, None);
    }

    pub fn prepare_bound(
        &self,
        slot: i32,
        handle: i32,
        name: [u8; 32],
        origin: Origin,
        owners: Option<Owners>,
    ) {
        self.edit(true, |state| {
            if let Some(entry) = state.entries.get_mut(handle as usize) {
                *entry = if slot == 5 && origin.valid {
                    Some(Token {
                        generation: self.generation.fetch_add(1, Ordering::Relaxed),
                        epoch: state.epoch,
                        handle,
                        origin,
                        name,
                        owners: owners.filter(|o| o.bound()).unwrap_or_default(),
                    })
                } else {
                    None
                };
            }
        });
    }

    pub fn bind(&self, handle: i32, owners: Owners) {
        self.edit(true, |state| {
            if let Some(entry) = state.entries.get_mut(handle as usize) {
                if let Some(token) = entry {
                    if !owners.bound() || (token.owners.bound() && token.owners != owners) {
                        *entry = None;
                    } else {
                        token.owners = owners;
                    }
                }
            }
        });
    }

    pub fn lookup(&self, owners: Owners) -> Option<Token> {
        if !owners.valid() {
            return None;
        }
        self.edit(false, |state| {
            let mut matched = state.entries.iter().flatten().filter(|t| {
                t.owners.cue == owners.cue
                    && t.owners.bank == owners.bank
                    && (t.owners.sound == 0 || t.owners.sound == owners.sound)
            });
            let mut token = *matched.next()?;
            token.owners = owners;
            matched.next().is_none().then_some(token)
        })
        .flatten()
    }

    pub fn stop_handle(&self, handle: i32) {
        self.edit(true, |state| {
            if let Some(entry) = state.entries.get_mut(handle as usize) {
                *entry = None;
            }
        });
    }

    fn retire(&self, matches: impl Fn(Token) -> bool) -> Option<Token> {
        self.edit(true, |state| {
            let mut found = None;
            let mut count = 0;
            for entry in &mut state.entries {
                if let Some(token) = *entry {
                    if matches(token) {
                        found = Some(token);
                        count += 1;
                        *entry = None;
                    } else if !token.owners.bound() {
                        // We cannot identify an unbound cue whose handle is now reusable.
                        *entry = None;
                    }
                }
            }
            if count == 1 {
                found
            } else {
                None
            }
        })
        .flatten()
    }

    pub fn stop_sound(&self, sound: usize) -> Option<Token> {
        self.retire(|t| sound != 0 && t.owners.sound == sound)
    }

    pub fn destroy_cue(&self, cue: usize) -> Option<Token> {
        self.retire(|t| cue != 0 && t.owners.cue == cue)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub backend: usize,
    pub buffer: usize,
    pub play: u32,
    pub write: u32,
    pub played: u64,
    pub ring: u32,
    pub hz: u32,
    pub align: u16,
}

impl Cursor {
    fn valid(self) -> bool {
        self.backend != 0
            && self.buffer != 0
            && self.ring != 0
            && self.hz != 0
            && self.align != 0
            && self.play < self.ring
            && self.write < self.ring
    }

    fn same_format(self, other: Self) -> bool {
        self.backend == other.backend
            && self.buffer == other.buffer
            && self.ring == other.ring
            && self.hz == other.hz
            && self.align == other.align
    }

    pub fn payload(self) -> [i64; 8] {
        [
            self.backend as i64,
            self.buffer as i64,
            self.play as i64,
            self.write as i64,
            self.played as i64,
            self.ring as i64,
            self.hz as i64,
            self.align as i64,
        ]
    }
}

#[derive(Default)]
pub struct CursorSampler {
    previous: Option<(i64, Cursor)>,
    emitted: Option<i64>,
    continuous: bool,
    failed: bool,
}

impl CursorSampler {
    pub fn invalidate(&mut self) {
        self.previous = None;
        self.continuous = false;
    }

    /// None = decimated; Some(bool) = emit, with continuity since the last emission.
    /// A successful cursor is still mixed output, never a per-song presentation clock.
    pub fn observe(&mut self, now: i64, frequency: i64, cursor: Option<Cursor>) -> Option<bool> {
        let cursor = cursor.filter(|c| c.valid() && now >= 0 && frequency > 0);
        let mut changed = self.failed != cursor.is_none();
        let continuous = match (self.previous, cursor) {
            (Some((before, old)), Some(new)) => {
                changed |= !old.same_format(new) || new.played < old.played;
                let gap = now as i128 - before as i128;
                !changed
                    && gap >= 0
                    && gap * new.hz as i128 * (new.align as i128)
                        < new.ring as i128 * frequency as i128
            }
            _ => false,
        };
        self.continuous &= continuous;
        self.previous = cursor.map(|c| (now, c));
        self.failed = cursor.is_none();
        let due = self.emitted.map_or(true, |before| {
            now < before || (now as i128 - before as i128) * 4 >= frequency as i128
        });
        if due {
            let valid = self.continuous;
            self.emitted = Some(now);
            self.continuous = cursor.is_some();
            Some(valid)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_interval_excludes_pre_observation_but_reports_its_cost() {
        let now = std::cell::Cell::new(100);
        let calls = std::cell::Cell::new(0);
        let result = observed_timed(
            || now.get(),
            |entered| {
                assert_eq!(entered, 100);
                now.set(600);
                42
            },
            || {
                calls.set(calls.get() + 1);
                now.set(850);
                7
            },
            |snapshot, result, timing| {
                assert_eq!(snapshot, Some(42));
                assert_eq!(*result, 7);
                assert_eq!(timing.original_begin, 600);
                assert_eq!(timing.original_end, 850);
                assert_eq!(timing.pre_ticks(), Some(500));
                panic!("observer must not retry original");
            },
        );
        assert_eq!(result, 7);
        assert_eq!(calls.get(), 1);
    }

    fn origin() -> Origin {
        Origin {
            attempt: 7,
            scene: 27,
            epoch: 10,
            valid: true,
        }
    }

    fn owners() -> Owners {
        Owners {
            cue: 0x1000,
            sound: 0x2000,
            bank: 0x3000,
        }
    }

    #[test]
    fn prepare_binds_before_unrelated_cue_cleanup_during_ready_dwell() {
        let map = Correlation::new();
        map.prepare_bound(
            5,
            3,
            [0; 32],
            origin(),
            Some(Owners {
                sound: 0,
                ..owners()
            }),
        );
        map.destroy_cue(0x9999);
        assert!(map.lookup(owners()).is_some());
        map.destroy_cue(owners().cue);
        assert!(map.lookup(owners()).is_none());
    }

    #[test]
    fn asynchronous_lookup_uses_bound_identity_not_thread_or_scene() {
        let map = Correlation::new();
        map.prepare(5, 3, [b'a'; 32], origin());
        map.bind(3, owners());
        let first = map.lookup(owners()).unwrap();
        assert_eq!(first.origin.attempt, 7);
        assert_eq!(first.handle, 3);
        assert_ne!(first.generation, 0);
        assert_eq!(map.lookup(owners()).unwrap().generation, first.generation);
        assert!(map
            .lookup(Owners {
                sound: 99,
                ..owners()
            })
            .is_none());
        assert!(map
            .lookup(Owners {
                bank: 99,
                ..owners()
            })
            .is_none());
    }

    #[test]
    fn game_side_binding_needs_only_stable_cue_and_bank() {
        let map = Correlation::new();
        map.prepare(5, 3, [0; 32], origin());
        map.bind(
            3,
            Owners {
                sound: 0,
                ..owners()
            },
        );
        let token =
            std::thread::scope(|scope| scope.spawn(|| map.lookup(owners())).join().unwrap())
                .unwrap();
        assert_eq!(token.owners, owners());
        assert!(map
            .lookup(Owners {
                bank: 42,
                ..owners()
            })
            .is_none());
        map.destroy_cue(owners().cue);
        assert!(map.lookup(owners()).is_none());
    }

    #[test]
    fn handle_reuse_requires_new_prepare_and_never_rebinds_old_token() {
        let map = Correlation::new();
        map.prepare(5, 3, [0; 32], origin());
        map.bind(3, owners());
        let first = map.lookup(owners()).unwrap().generation;
        map.bind(
            3,
            Owners {
                cue: 77,
                ..owners()
            },
        );
        assert!(map.lookup(owners()).is_none());
        map.prepare(5, 3, [0; 32], origin());
        map.bind(3, owners());
        assert!(map.lookup(owners()).unwrap().generation > first);
        map.prepare(2, 3, [0; 32], origin());
        assert!(map.lookup(owners()).is_none());
    }

    #[test]
    fn stop_destroy_unregister_and_unknown_context_fail_closed() {
        let map = Correlation::new();
        for action in 0..4 {
            map.prepare(5, 3, [0; 32], origin());
            map.bind(3, owners());
            match action {
                0 => map.stop_handle(3),
                1 => {
                    map.stop_sound(owners().sound);
                }
                2 => {
                    map.destroy_cue(owners().cue);
                }
                _ => map.invalidate_all(),
            }
            assert!(map.lookup(owners()).is_none());
        }
        map.prepare(
            5,
            3,
            [0; 32],
            Origin {
                valid: false,
                ..origin()
            },
        );
        map.bind(3, owners());
        assert!(map.lookup(owners()).is_none());
    }

    #[test]
    fn destruction_invalidates_unbound_handle_before_pointer_reuse() {
        let map = Correlation::new();
        map.prepare(5, 3, [0; 32], origin());
        map.destroy_cue(0xdead);
        map.bind(3, owners());
        assert!(map.lookup(owners()).is_none());
    }

    #[test]
    fn critical_contention_poisons_even_without_delivering_an_event() {
        let map = Correlation::new();
        map.prepare(5, 3, [0; 32], origin());
        map.bind(3, owners());
        let saved = map.lookup(owners()).unwrap();
        let guard = map.state.lock().unwrap();
        map.destroy_cue(owners().cue);
        assert!(!map.current(saved));
        drop(guard);
        assert!(map.lookup(owners()).is_none());
        map.prepare(5, 3, [0; 32], origin());
        map.bind(3, owners());
        assert!(map.lookup(owners()).is_some());
    }

    #[test]
    fn invalid_handles_and_null_ownership_never_match() {
        let map = Correlation::new();
        for handle in [-1, 256, i32::MAX] {
            map.prepare(5, handle, [0; 32], origin());
            map.bind(handle, owners());
        }
        assert!(map.lookup(owners()).is_none());
        map.prepare(5, 3, [0; 32], origin());
        map.bind(3, Owners { cue: 0, ..owners() });
        assert!(map.lookup(Owners { cue: 0, ..owners() }).is_none());
    }

    #[test]
    fn duplicate_live_bindings_are_unmatched_instead_of_first_match_wins() {
        let map = Correlation::new();
        for handle in [1, 2] {
            map.prepare(5, handle, [0; 32], origin());
            map.bind(handle, owners());
        }
        assert!(map.lookup(owners()).is_none());
    }

    #[test]
    fn observation_panic_never_skips_or_repeats_original() {
        let calls = std::cell::Cell::new(0);
        let got = observed(
            || panic!("pre"),
            || {
                calls.set(calls.get() + 1);
                17
            },
            |_, _| panic!("post"),
        );
        assert_eq!(got, 17);
        assert_eq!(calls.get(), 1);
    }

    fn cursor() -> Cursor {
        Cursor {
            backend: 1,
            buffer: 2,
            play: 90,
            write: 10,
            played: 90,
            ring: 100,
            hz: 1000,
            align: 2,
        }
    }

    #[test]
    fn cursor_wrap_decimation_and_failed_reads_preserve_honest_continuity() {
        let mut state = CursorSampler::default();
        assert_eq!(state.observe(0, 1000, Some(cursor())), Some(false));
        let next = Cursor {
            play: 10,
            played: 110,
            ..cursor()
        };
        assert_eq!(state.observe(10, 1000, Some(next)), None);
        // 250 ms is longer than this 50 ms ring: continuity must be unknown.
        assert_eq!(state.observe(260, 1000, Some(next)), Some(false));
        assert_eq!(state.observe(270, 1000, None), None);
        assert_eq!(state.observe(280, 1000, Some(next)), None);
        assert_eq!(state.observe(510, 1000, Some(next)), Some(false));
    }

    #[test]
    fn cursor_identity_format_and_reset_never_inherit_continuity() {
        let mut state = CursorSampler::default();
        let base = Cursor {
            ring: 20000,
            ..cursor()
        };
        state.observe(0, 1000, Some(base));
        assert_eq!(
            state.observe(
                250,
                1000,
                Some(Cursor {
                    played: 100,
                    ..base
                })
            ),
            Some(true)
        );
        for (index, changed) in [
            Cursor { buffer: 3, ..base },
            Cursor { hz: 2000, ..base },
            Cursor { align: 4, ..base },
            Cursor { played: 0, ..base },
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                state.observe(500 + index as i64 * 250, 1000, Some(changed)),
                Some(false)
            );
        }
    }

    #[test]
    fn repeated_cursor_failure_or_backend_churn_cannot_flood_the_recorder() {
        let mut state = CursorSampler::default();
        let mut count = 0;
        for time in 0..1000 {
            let value = (time % 3 != 0).then_some(Cursor {
                backend: (time % 2 + 1) as usize,
                ring: 20000,
                ..cursor()
            });
            if state.observe(time, 1000, value).is_some() {
                count += 1;
            }
        }
        assert!(
            count <= 4,
            "at most four retained cursor records per second, got {count}"
        );
    }

    #[test]
    fn cursor_contention_breaks_continuity_without_resetting_rate_budget() {
        let mut state = CursorSampler::default();
        let mut count = 0;
        for time in 0..1000 {
            state.invalidate();
            if let Some(continuous) = state.observe(time, 1000, Some(cursor())) {
                count += 1;
                assert!(!continuous);
            }
        }
        assert_eq!(count, 4);
    }
}
