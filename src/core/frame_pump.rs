//! One queued batch per engine frame, with reentrant original forwarding.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

type Job = Box<dyn FnOnce() + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Poll,
    Jobs,
    Job,
}

/// Optional observation only; tokens may carry a clock stamp or an RAII span.
pub trait Observer {
    type Token;
    fn begin(&self, phase: Phase, id: u64) -> Self::Token;
    fn end(&self, token: Self::Token);
}

impl Observer for () {
    type Token = ();
    fn begin(&self, _: Phase, _: u64) {}
    fn end(&self, _: ()) {}
}

fn begin<O: Observer>(observer: Option<&O>, phase: Phase, id: u64) -> Option<O::Token> {
    observer.and_then(|o| catch_unwind(AssertUnwindSafe(|| o.begin(phase, id))).ok())
}

fn end<O: Observer>(observer: Option<&O>, token: Option<O::Token>) {
    if let (Some(o), Some(token)) = (observer, token) {
        let _ = catch_unwind(AssertUnwindSafe(|| o.end(token)));
    }
}

struct Queue {
    pending: Vec<Job>,
    spare: Vec<Job>,
}

pub struct FramePump {
    queue: Mutex<Queue>,
    running: AtomicBool,
    sequence: AtomicU64,
}

pub struct FrameStats {
    pub frame: u64,
    pub jobs: usize,
    pub queued: usize,
    pub poll_panicked: bool,
    pub job_panics: usize,
}

struct Entered<'a>(&'a AtomicBool);

impl Drop for Entered<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl FramePump {
    pub const fn new() -> Self {
        Self {
            queue: Mutex::new(Queue {
                pending: Vec::new(),
                spare: Vec::new(),
            }),
            running: AtomicBool::new(false),
            sequence: AtomicU64::new(0),
        }
    }

    pub fn enqueue(&self, job: impl FnOnce() + Send + 'static) {
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .push(Box::new(job));
    }

    /// The caller supplies a verified once-per-frame seam. Poll-enqueued jobs
    /// join this frame; jobs enqueued during the batch or render wait until the
    /// next frame. The reentrancy guard includes original rendering as well.
    pub fn dispatch(&self, poll: impl FnOnce(), render: impl FnOnce()) -> Option<FrameStats> {
        self.dispatch_observed(poll, render, None::<&()>)
    }

    /// Observation never wraps/repeats the original. Job IDs are one-based batch
    /// positions, not closure addresses; no identifiers are allocated on enqueue.
    pub fn dispatch_observed<O: Observer>(
        &self,
        poll: impl FnOnce(),
        render: impl FnOnce(),
        observer: Option<&O>,
    ) -> Option<FrameStats> {
        if self.running.swap(true, Ordering::Acquire) {
            render();
            return None;
        }
        let _entered = Entered(&self.running);
        let frame = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let poll_span = begin(observer, Phase::Poll, 0);
        let poll_panicked = catch_unwind(AssertUnwindSafe(poll)).is_err();
        end(observer, poll_span);
        let jobs_span = begin(observer, Phase::Jobs, 0);
        let mut batch = {
            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            let mut batch = std::mem::take(&mut queue.spare);
            std::mem::swap(&mut batch, &mut queue.pending);
            batch
        };
        let jobs = batch.len();
        let mut job_panics = 0;
        for (index, job) in batch.drain(..).enumerate() {
            let span = begin(observer, Phase::Job, index as u64 + 1);
            if catch_unwind(AssertUnwindSafe(job)).is_err() {
                job_panics += 1;
            }
            end(observer, span);
        }
        end(observer, jobs_span);
        render();
        let queued = {
            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.spare = batch;
            queue.pending.len()
        };
        Some(FrameStats {
            frame,
            jobs,
            queued,
            poll_panicked,
            job_panics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn empty_frames_poll_and_forward_without_a_widget_or_time_gate() {
        let pump = FramePump::new();
        let mut polls = 0;
        let mut renders = 0;
        for frame in 1..=100 {
            let stats = pump.dispatch(|| polls += 1, || renders += 1).unwrap();
            assert_eq!(stats.frame, frame);
            assert_eq!(stats.jobs, 0);
            assert_eq!(stats.queued, 0);
        }
        assert_eq!((polls, renders), (100, 100));
    }

    #[test]
    fn poll_jobs_join_this_batch_but_job_continuations_wait() {
        let pump = Arc::new(FramePump::new());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let p = Arc::clone(&pump);
        let s = Arc::clone(&seen);
        pump.enqueue(move || {
            s.lock().unwrap().push("A");
            p.enqueue(move || s.lock().unwrap().push("C"));
        });
        let s = Arc::clone(&seen);
        pump.enqueue(move || s.lock().unwrap().push("B"));
        let s = Arc::clone(&seen);
        let first = pump
            .dispatch(
                || {
                    seen.lock().unwrap().push("poll");
                    pump.enqueue(move || s.lock().unwrap().push("P"));
                },
                || seen.lock().unwrap().push("render"),
            )
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), ["poll", "A", "B", "P", "render"]);
        assert_eq!((first.jobs, first.queued), (3, 1));
        let next = pump.dispatch(|| {}, || {}).unwrap();
        assert_eq!(next.jobs, 1);
        assert_eq!(seen.lock().unwrap().last(), Some(&"C"));
    }

    #[test]
    fn reentrant_poll_job_and_render_only_forward_original() {
        let pump = Arc::new(FramePump::new());
        let renders = Arc::new(Mutex::new(0));
        let p = Arc::clone(&pump);
        let r = Arc::clone(&renders);
        pump.enqueue(move || {
            assert!(p
                .dispatch(|| panic!("nested poll"), || *r.lock().unwrap() += 1)
                .is_none());
        });
        let stats = pump
            .dispatch(
                || {
                    assert!(pump
                        .dispatch(|| panic!("nested poll"), || *renders.lock().unwrap() += 1)
                        .is_none());
                },
                || {
                    *renders.lock().unwrap() += 1;
                    assert!(pump
                        .dispatch(|| panic!("nested poll"), || *renders.lock().unwrap() += 1)
                        .is_none());
                },
            )
            .unwrap();
        assert_eq!(stats.frame, 1);
        assert_eq!(stats.jobs, 1);
        assert_eq!(*renders.lock().unwrap(), 4);
        assert_eq!(pump.dispatch(|| {}, || {}).unwrap().frame, 2);
    }

    #[test]
    fn panicking_work_does_not_skip_other_jobs_or_original() {
        let pump = FramePump::new();
        let done = Arc::new(Mutex::new(false));
        pump.enqueue(|| panic!("bad job"));
        let d = Arc::clone(&done);
        pump.enqueue(move || *d.lock().unwrap() = true);
        let mut rendered = false;
        let stats = pump
            .dispatch(|| panic!("bad poll"), || rendered = true)
            .unwrap();
        assert!(rendered && *done.lock().unwrap());
        assert!(stats.poll_panicked);
        assert_eq!(stats.job_panics, 1);
        assert_eq!(pump.dispatch(|| {}, || {}).unwrap().frame, 2);
    }

    #[test]
    fn original_enqueued_work_waits_and_guard_releases_on_unwind() {
        let pump = FramePump::new();
        let done = Arc::new(Mutex::new(false));
        let d = Arc::clone(&done);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pump.dispatch(
                || {},
                || {
                    pump.enqueue(move || *d.lock().unwrap() = true);
                    panic!("test-only original unwind");
                },
            );
        }));
        assert!(result.is_err());
        assert!(!*done.lock().unwrap());
        assert_eq!(pump.dispatch(|| {}, || {}).unwrap().jobs, 1);
        assert!(*done.lock().unwrap());
    }

    #[test]
    fn background_enqueue_does_not_wait_on_a_running_callback() {
        let pump = Arc::new(FramePump::new());
        let p = Arc::clone(&pump);
        pump.enqueue(move || {
            std::thread::spawn(move || p.enqueue(|| {})).join().unwrap();
        });
        let stats = pump.dispatch(|| {}, || {}).unwrap();
        assert_eq!((stats.jobs, stats.queued), (1, 1));
        assert_eq!(pump.dispatch(|| {}, || {}).unwrap().jobs, 1);
    }

    #[test]
    fn observed_phases_preserve_order_and_disabled_never_observes() {
        use std::cell::RefCell;
        struct Spy(RefCell<Vec<(Phase, u64, bool)>>);
        impl Observer for Spy {
            type Token = (Phase, u64);
            fn begin(&self, phase: Phase, id: u64) -> Self::Token {
                self.0.borrow_mut().push((phase, id, true));
                (phase, id)
            }
            fn end(&self, (phase, id): Self::Token) {
                self.0.borrow_mut().push((phase, id, false));
            }
        }
        let pump = FramePump::new();
        let spy = Spy(RefCell::new(Vec::new()));
        pump.dispatch_observed(|| {}, || {}, None::<&Spy>);
        assert!(spy.0.borrow().is_empty());
        pump.enqueue(|| {});
        pump.dispatch_observed(|| {}, || {}, Some(&spy));
        assert_eq!(
            *spy.0.borrow(),
            vec![
                (Phase::Poll, 0, true),
                (Phase::Poll, 0, false),
                (Phase::Jobs, 0, true),
                (Phase::Job, 1, true),
                (Phase::Job, 1, false),
                (Phase::Jobs, 0, false),
            ]
        );
    }

    #[test]
    fn failing_observer_and_reentry_cannot_skip_or_repeat_original() {
        struct Broken;
        impl Observer for Broken {
            type Token = ();
            fn begin(&self, _: Phase, _: u64) {
                panic!("diagnostic failure")
            }
            fn end(&self, _: ()) {
                panic!("diagnostic failure")
            }
        }
        let pump = FramePump::new();
        let renders = std::cell::Cell::new(0);
        let stats = pump
            .dispatch_observed(
                || {
                    assert!(pump
                        .dispatch_observed(
                            || panic!("nested poll"),
                            || renders.set(renders.get() + 1),
                            Some(&Broken)
                        )
                        .is_none());
                },
                || renders.set(renders.get() + 1),
                Some(&Broken),
            )
            .unwrap();
        assert!(!stats.poll_panicked);
        assert_eq!(renders.get(), 2);
    }
}
