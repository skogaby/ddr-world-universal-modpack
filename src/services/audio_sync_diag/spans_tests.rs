use super::*;
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct Fake {
    now: Cell<i64>,
    clocks: Cell<usize>,
    events: RefCell<Vec<Event>>,
}
impl Sink for Fake {
    fn clock(&self) -> i64 {
        self.clocks.set(self.clocks.get() + 1);
        self.now.get()
    }
    fn context(&self) -> Context {
        Context {
            attempt: 2,
            scene: 28,
            epoch: 3,
            valid: true,
        }
    }
    fn thread_id(&self) -> u32 {
        42
    }
    fn push(&self, event: Event) {
        self.events.borrow_mut().push(event);
    }
}

#[test]
fn fake_qpc_tracks_exact_sub_ms_boundaries_and_nested_actor_context() {
    let sink = Fake::default();
    sink.now.set(100);
    let outer = Span::judge(Some(&sink), 0x1234, 456);
    let outer_id = outer.id();
    assert_eq!(incoming_count(0x1234), Some(456));
    assert_eq!(incoming_count(0x5678), None);
    sink.now.set(110);
    let inner = Span::judge(Some(&sink), 0x5678, 999);
    assert_eq!(incoming_count(0x1234), None);
    assert_eq!(incoming_count(0x5678), Some(999));
    sink.now.set(150);
    drop(inner);
    assert_eq!(incoming_count(0x1234), Some(456));
    assert_eq!(
        std::thread::spawn(|| incoming_count(0x1234))
            .join()
            .unwrap(),
        None
    );
    sink.now.set(200);
    drop(outer);
    assert_eq!(incoming_count(0x1234), None);
    let events = sink.events.borrow();
    assert_eq!(
        (events[0].qpc, events[0].end_qpc, events[0].parent_id),
        (110, 150, outer_id)
    );
    assert_eq!((events[1].qpc, events[1].end_qpc), (100, 200));
    assert_eq!(events[1].origin_attempt, 2);
    assert_eq!(events[1].thread_id, 42);
    assert_eq!(sink.clocks.get(), 4);
}

#[test]
fn disabled_has_zero_clock_calls_and_unwind_restores_nesting() {
    let sink = Fake::default();
    drop(Span::begin(None, Scope::Frame, 0));
    drop(Span::judge(None, 1, 2));
    assert_eq!(sink.clocks.get(), 0);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _span = Span::judge(Some(&sink), 1, 2);
        panic!("game callback");
    }));
    assert_eq!(incoming_count(1), None);
    assert_eq!(sink.events.borrow().len(), 1);
}

#[test]
fn hit_validity_preserves_integer_error_and_never_calls_miss_a_physical_hit() {
    let hit = hit_event(0x1028, Some(-7), Some(1000), Some(993), Some(false), None).unwrap();
    assert_eq!(&hit.detail[..6], &[0, -7, 1000, 993, 0, 0]);
    assert_eq!(hit.detail_valid & 0x3f, 0x1f);
    assert_eq!(hit.detail[6], 1); // normal timing-bearing grade (not a physical timestamp)
    let miss = hit_event(0x102d, Some(180), None, None, Some(true), None).unwrap();
    assert_eq!(miss.detail[0], 5);
    assert_eq!(miss.detail[1], 180);
    assert_eq!(miss.detail[4], 1);
    assert_eq!(miss.detail[6], 2); // automatic Miss / window-edge value
    let ok = hit_event(0x102e, Some(99), Some(1000), None, None, None).unwrap();
    assert_eq!(ok.detail_valid & 2, 0);
    assert_eq!(ok.detail[6], 3); // freeze OK, no error
    let unknown = hit_event(0x1028, None, None, None, None, None).unwrap();
    assert_eq!(unknown.detail_valid, 0x41);
    for opcode in [0x1030, 0x1031, 0x1046, 0x1027] {
        assert!(hit_event(opcode, None, None, None, None, None).is_none());
    }
}

#[test]
fn real_frame_pump_fake_clock_separates_phases_and_next_frame_jobs() {
    use crate::core::frame_pump::FramePump;
    use std::sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    };
    struct ClockSink {
        now: Arc<AtomicI64>,
        events: RefCell<Vec<Event>>,
    }
    impl Sink for ClockSink {
        fn clock(&self) -> i64 {
            self.now.load(Ordering::Relaxed)
        }
        fn context(&self) -> Context {
            Context::default()
        }
        fn thread_id(&self) -> u32 {
            1
        }
        fn push(&self, e: Event) {
            self.events.borrow_mut().push(e);
        }
    }
    let now = Arc::new(AtomicI64::new(100));
    let sink = ClockSink {
        now: now.clone(),
        events: RefCell::new(Vec::new()),
    };
    let pump = FramePump::new();
    let clock = now.clone();
    let original_calls = Cell::new(0);
    let frame = Span::begin(Some(&sink), Scope::Frame, 0);
    let stats = pump
        .dispatch_observed(
            || {
                now.store(130, Ordering::Relaxed);
                pump.enqueue(move || {
                    clock.store(180, Ordering::Relaxed);
                });
            },
            || {
                let _original = Span::begin(Some(&sink), Scope::LayerOriginal, 0);
                original_calls.set(original_calls.get() + 1);
                now.store(400, Ordering::Relaxed);
                pump.enqueue(|| {});
            },
            Some(&FrameObserver(&sink)),
        )
        .unwrap();
    drop(frame);
    assert_eq!((stats.jobs, stats.queued, original_calls.get()), (1, 1, 1));
    let events = sink.events.borrow();
    let find = |scope: Scope| events.iter().find(|e| e.id == scope as i32).unwrap();
    assert_eq!(
        (find(Scope::Poll).qpc, find(Scope::Poll).end_qpc),
        (100, 130)
    );
    assert_eq!(
        (find(Scope::Jobs).qpc, find(Scope::Jobs).end_qpc),
        (130, 180)
    );
    assert_eq!((find(Scope::Job).qpc, find(Scope::Job).end_qpc), (130, 180));
    assert_eq!(
        (
            find(Scope::LayerOriginal).qpc,
            find(Scope::LayerOriginal).end_qpc
        ),
        (180, 400)
    );
    assert_eq!(find(Scope::Job).parent_id, find(Scope::Jobs).trace_id);
}

#[test]
fn judge_sample_is_after_original_posts_and_outer_span() {
    let sink = Fake::default();
    dispatch_judge(
        Some(&sink),
        1,
        123,
        || {
            sink.now.set(20);
            77
        },
        || {
            assert_eq!(incoming_count(1), Some(123));
            sink.now.set(30);
        },
        |value| {
            assert_eq!(value, 77);
            sink.now.set(40);
        },
        |entry| {
            assert_eq!(incoming_count(1), None);
            let mut sample = Span::begin(Some(&sink), Scope::JudgeSample, 0);
            sample.set_parent(entry.trace_id);
            sink.now.set(100);
        },
    );
    let events = sink.events.borrow();
    assert_eq!(
        events
            .iter()
            .map(|e| (e.id, e.qpc, e.end_qpc))
            .collect::<Vec<_>>(),
        [
            (8, 0, 20),
            (9, 20, 30),
            (10, 30, 40),
            (7, 0, 40),
            (19, 40, 100)
        ]
    );
    assert_eq!(events[4].parent_id, events[3].trace_id);
}

#[test]
fn judge_dispatch_failure_and_reentry_forward_exactly_once() {
    let sink = Fake::default();
    let originals = Cell::new(0);
    let tails = Cell::new(0);
    dispatch_judge(
        Some(&sink),
        1,
        2,
        || panic!("pre work"),
        || {
            originals.set(originals.get() + 1);
            dispatch_judge(
                Some(&sink),
                3,
                4,
                || (),
                || {
                    assert_eq!(incoming_count(1), None);
                    assert_eq!(incoming_count(3), Some(4));
                    originals.set(originals.get() + 1);
                },
                |_| panic!("post work"),
                |_| {
                    tails.set(tails.get() + 1);
                },
            );
            assert_eq!(incoming_count(1), Some(2));
        },
        |_: ()| panic!("must not have a pre result"),
        |_| {
            tails.set(tails.get() + 1);
            panic!("sampling failure");
        },
    );
    assert_eq!(originals.get(), 2);
    assert_eq!(tails.get(), 2);
    assert_eq!(incoming_count(1), None);
}

#[test]
fn disabled_judge_dispatch_has_no_sink_or_sample_activity() {
    let sink = Fake::default();
    let calls = RefCell::new(Vec::new());
    dispatch_judge(
        None,
        1,
        2,
        || {
            calls.borrow_mut().push("pre");
            3
        },
        || {
            calls.borrow_mut().push("original");
        },
        |value| {
            assert_eq!(value, 3);
            calls.borrow_mut().push("post");
        },
        |_| panic!("disabled sample"),
    );
    assert_eq!(*calls.borrow(), ["pre", "original", "post"]);
    assert_eq!(sink.clocks.get(), 0);
}

#[test]
fn sink_failure_and_recursive_emission_do_not_leak_context() {
    struct Recursive {
        clocks: Cell<usize>,
    }
    impl Sink for Recursive {
        fn clock(&self) -> i64 {
            self.clocks.set(self.clocks.get() + 1);
            1
        }
        fn context(&self) -> Context {
            Context::default()
        }
        fn thread_id(&self) -> u32 {
            1
        }
        fn push(&self, _: Event) {
            drop(Span::judge(Some(self), 99, 999));
            panic!("writer observer failure");
        }
    }
    let sink = Recursive {
        clocks: Cell::new(0),
    };
    drop(Span::judge(Some(&sink), 1, 123));
    assert_eq!(incoming_count(1), None);
    assert_eq!(incoming_count(99), None);
    assert_eq!(sink.clocks.get(), 2);
    drop(Span::begin(Some(&sink), Scope::Frame, 0));
    assert_eq!(sink.clocks.get(), 4);
}

#[test]
fn failed_entry_clock_never_reuses_outer_judge_count() {
    let sink = Fake::default();
    let outer = Span::judge(Some(&sink), 1, 100);
    sink.now.set(-1);
    let inner = Span::judge(Some(&sink), 1, 200);
    assert_eq!(incoming_count(1), Some(200));
    sink.now.set(50);
    drop(inner);
    assert_eq!(incoming_count(1), Some(100));
    drop(outer);
    assert_eq!(sink.events.borrow()[0].qpc, -1);
}
