//! Enabled-only wall-time spans and nested judge argument context.

use super::model::{Context, Event, Kind};
use std::cell::Cell;
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Stable CSV IDs. These describe wall time, never GPU execution time.
#[repr(i32)]
#[derive(Clone, Copy, Debug)]
pub enum Scope {
    Frame = 1,
    Poll = 2,
    Jobs = 3,
    Job = 4,
    LayerOriginal = 5,
    Topmost = 6,
    Judge = 7,
    JudgePre = 8,
    JudgeOriginal = 9,
    JudgePost = 10,
    JudgePreCallback = 11,
    JudgePostCallback = 12,
    FrameCallback = 13,
    InputCallback = 14,
    InputExclusive = 15,
    SubmitPre = 16,
    SubmitOriginal = 17,
    SubmitPost = 18,
    JudgeSample = 19,
    HitSample = 20,
    FrameSample = 21,
    Submit = 22,
}

pub trait Sink {
    fn clock(&self) -> i64;
    fn context(&self) -> Context;
    fn thread_id(&self) -> u32;
    fn push(&self, event: Event);
}

pub struct FrameObserver<'a>(pub &'a dyn Sink);

impl<'a> crate::core::frame_pump::Observer for FrameObserver<'a> {
    type Token = Span<'a>;

    fn begin(&self, phase: crate::core::frame_pump::Phase, id: u64) -> Self::Token {
        use crate::core::frame_pump::Phase;
        Span::begin(
            Some(self.0),
            match phase {
                Phase::Poll => Scope::Poll,
                Phase::Jobs => Scope::Jobs,
                Phase::Job => Scope::Job,
            },
            id,
        )
    }

    fn end(&self, token: Self::Token) {
        drop(token);
    }
}

#[derive(Clone, Copy)]
struct Local {
    trace: u64,
    judge: Option<(usize, i32)>,
}

thread_local! {
    static LOCAL: Cell<Local> = const { Cell::new(Local { trace: 0, judge: None }) };
    static EMITTING: Cell<bool> = const { Cell::new(false) };
}
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Non-Send: a nested context must be restored on its originating thread.
pub struct Span<'a> {
    sink: Option<&'a dyn Sink>,
    event: Event,
    previous: Option<Local>,
    _thread: PhantomData<Rc<()>>,
}

impl<'a> Span<'a> {
    pub fn begin(sink: Option<&'a dyn Sink>, scope: Scope, id: u64) -> Self {
        Self::enter(sink, scope, id, None)
    }

    pub fn judge(sink: Option<&'a dyn Sink>, actor: usize, count: i32) -> Self {
        let mut span = Self::enter(sink, Scope::Judge, 0, Some((actor, count)));
        span.event.sample.actor = actor as u64;
        span.event.detail_valid = 1;
        span.event.detail[0] = count as i64;
        span
    }

    fn enter(
        sink: Option<&'a dyn Sink>,
        scope: Scope,
        id: u64,
        judge: Option<(usize, i32)>,
    ) -> Self {
        let mut span = Self {
            sink: None,
            event: Event::default(),
            previous: None,
            _thread: PhantomData,
        };
        let Some(sink) = sink else { return span };
        // No sink/clock activity during emission, even if an observer reenters.
        if EMITTING.try_with(Cell::get).unwrap_or(true) {
            return span;
        }
        // Stamp before metadata/TLS work. A failed clock still establishes the
        // real nested judge argument; inheriting the outer count would lie.
        let qpc = catch_unwind(AssertUnwindSafe(|| sink.clock())).unwrap_or(-1);
        let (context, thread_id) =
            catch_unwind(AssertUnwindSafe(|| (sink.context(), sink.thread_id())))
                .unwrap_or_default();
        let trace_id = NEXT.fetch_add(1, Ordering::Relaxed);
        let Ok(previous) = LOCAL.try_with(|local| {
            let previous = local.get();
            local.set(Local {
                trace: trace_id,
                judge: judge.or(previous.judge),
            });
            previous
        }) else {
            return span;
        };
        span.previous = Some(previous);
        span.sink = Some(sink);
        span.event = Event {
            kind: Kind::Span,
            id: scope as i32,
            qpc,
            trace_id,
            parent_id: previous.trace,
            thread_id,
            origin_attempt: context.attempt,
            origin_scene: context.scene,
            origin_known: context.valid,
            counters: [id, 0, 0, 0],
            ..Event::default()
        };
        span
    }

    pub fn id(&self) -> u64 {
        self.event.trace_id
    }
    pub fn event(&self) -> Event {
        self.event
    }
    pub fn set_parent(&mut self, parent: u64) {
        self.event.parent_id = parent;
    }
}

/// The production judge dispatch skeleton. Only observation/mod work is
/// contained; the original is called exactly once and outside every catch.
pub fn dispatch_judge<T>(
    sink: Option<&dyn Sink>,
    actor: usize,
    count: i32,
    pre: impl FnOnce() -> T,
    original: impl FnOnce(),
    post: impl FnOnce(T),
    sample: impl FnOnce(Event),
) {
    let judge = Span::judge(sink, actor, count);
    let entry = judge.event();
    let callbacks = {
        let _pre = Span::begin(sink, Scope::JudgePre, 0);
        catch_unwind(AssertUnwindSafe(pre)).ok()
    };
    {
        let _original = Span::begin(sink, Scope::JudgeOriginal, 0);
        original();
    }
    {
        let _post = Span::begin(sink, Scope::JudgePost, 0);
        if let Some(callbacks) = callbacks {
            let _ = catch_unwind(AssertUnwindSafe(|| post(callbacks)));
        }
    }
    drop(judge);
    if sink.is_some() {
        let _ = catch_unwind(AssertUnwindSafe(|| sample(entry)));
    }
}

impl Drop for Span<'_> {
    fn drop(&mut self) {
        let Some(sink) = self.sink else { return };
        let end = catch_unwind(AssertUnwindSafe(|| sink.clock())).unwrap_or(-1);
        if let Some(previous) = self.previous {
            let _ = LOCAL.try_with(|local| local.set(previous));
        }
        self.event.end_qpc = end;
        let Ok(false) = EMITTING.try_with(|flag| flag.replace(true)) else {
            return;
        };
        let _ = catch_unwind(AssertUnwindSafe(|| sink.push(self.event)));
        let _ = EMITTING.try_with(|flag| flag.set(false));
    }
}

/// Only the innermost judge on THIS thread with the SAME actor is authoritative.
pub fn incoming_count(actor: usize) -> Option<i32> {
    LOCAL
        .try_with(|local| {
            local
                .get()
                .judge
                .filter(|(a, _)| *a == actor && actor != 0)
                .map(|(_, n)| n)
        })
        .ok()
        .flatten()
}

/// Pure classification. Miss retains its engine window-edge value but is not a
/// physical hit time. Freeze OK has no error; shock/cancel are uninterpreted.
pub fn hit_event(
    opcode: u32,
    error: Option<i32>,
    expected: Option<i32>,
    incoming: Option<i32>,
    dead: Option<bool>,
    finished: Option<bool>,
) -> Option<Event> {
    let grade = opcode.wrapping_sub(0x1028);
    if grade > 6 {
        return None;
    }
    let mut event = Event {
        kind: Kind::Judgement,
        id: opcode as i32,
        ..Event::default()
    };
    for (index, value) in [
        Some(grade as i64),
        error.filter(|_| grade != 6).map(i64::from),
        expected.map(i64::from),
        incoming.map(i64::from),
        dead.map(i64::from),
        finished.map(i64::from),
        Some(if grade == 6 {
            3
        } else if grade == 5 {
            2
        } else {
            1
        }),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(value) = value {
            event.detail[index] = value;
            event.detail_valid |= 1 << index;
        }
    }
    Some(event)
}

#[cfg(test)]
#[path = "spans_tests.rs"]
mod tests;
