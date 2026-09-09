//! Bounded storage and sampling policy for the optional audio diagnostic.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

pub const VALID_ACTOR: u32 = 1;
pub const VALID_ANCHOR: u32 = 2;
pub const VALID_TICK: u32 = 4;
pub const VALID_OFFSETS: u32 = 8;
pub const VALID_OPTION: u32 = 16;
pub const VALID_RATE: u32 = 32;

pub const CSV_COLUMNS: &str = "kind,qpc,end_qpc,attempt,scene_epoch,scene,context_valid,id,result,side,name,valid,actor,anchor,frame_tick,judge_mc,stored_mc,sound_ms,input_ms,render_ms,bomb_frames,option_ms,rate_q31,segment,observations,max_gap_qpc,counter0,counter1,counter2,counter3,progress_wall_ms,progress_mc,trace_id,parent_id,thread_id,origin_attempt,origin_scene,origin_known,detail_valid,detail0,detail1,detail2,detail3,detail4,detail5,detail6,detail7";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub attempt: u64,
    pub scene: i32,
    pub epoch: u64,
    pub valid: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Scene,
    BankCreate,
    BankUnregister,
    PrepareRequest,
    ReadyObserved,
    StartRequest,
    StopRequest,
    AnchorDelivered,
    GameplaySample,
    Frame,
    Span,
    SpanSummary,
    Judgement,
    Scheduled,
    VoiceStart,
    SoundStop,
    CueDestroyed,
    OutputCursor,
    EngineStatus,
    /// Deterministic audio clock arm evidence (services/audio_clock).
    Onset,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::BankCreate => "bank_create",
            Self::BankUnregister => "bank_unregister",
            Self::PrepareRequest => "prepare_request",
            Self::ReadyObserved => "ready_observed",
            Self::StartRequest => "start_request",
            Self::StopRequest => "stop_request",
            Self::AnchorDelivered => "anchor_delivered",
            Self::GameplaySample => "gameplay_sample",
            Self::Frame => "frame",
            Self::Span => "span",
            Self::SpanSummary => "span_summary",
            Self::Judgement => "judgement",
            Self::Scheduled => "scheduled",
            Self::VoiceStart => "voice_start",
            Self::SoundStop => "sound_stop",
            Self::CueDestroyed => "cue_destroyed",
            Self::OutputCursor => "output_cursor",
            Self::EngineStatus => "engine_status",
            Self::Onset => "onset",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub valid: u32,
    pub actor: u64,
    pub anchor: i64,
    pub frame_tick: i64,
    pub music_count: i32,
    /// Stored by onUpdate AFTER judge dispatch; not the current judge argument.
    pub raw_count: i32,
    /// SOUND, INPUT, RENDER, BOMB (frames), Option JUDGEMENT (ms).
    pub offsets: [i32; 5],
    pub rate_q31: u64,
}

impl Sample {
    fn same_domain(&self, previous: &Self) -> bool {
        self.valid == previous.valid
            && self.actor == previous.actor
            && self.anchor == previous.anchor
            && self.offsets == previous.offsets
            && self.rate_q31 == previous.rate_q31
    }

    pub fn progress_ms(
        &self,
        previous: &Self,
        before: i64,
        now: i64,
        frequency: i64,
    ) -> Option<(f64, i64)> {
        let required = VALID_ACTOR | VALID_ANCHOR | VALID_RATE;
        if self.valid & required != required
            || !self.same_domain(previous)
            || self.music_count < previous.music_count
        {
            return None;
        }
        Some((
            elapsed_ms(before, now, frequency)?,
            i64::from(self.music_count) - i64::from(previous.music_count),
        ))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Event {
    pub kind: Kind,
    pub qpc: i64,
    pub end_qpc: i64,
    pub attempt: u64,
    pub scene_epoch: u64,
    pub scene: i32,
    pub context_valid: bool,
    pub id: i32,
    pub result: i32,
    pub side: i32,
    pub name: [u8; 32],
    pub sample: Sample,
    pub segment: u64,
    pub observations: u64,
    pub max_gap_qpc: i64,
    /// Frame counters: frame sequence, polls, jobs, queue depth (caller supplied).
    pub counters: [u64; 4],
    pub trace_id: u64,
    pub parent_id: u64,
    pub thread_id: u32,
    pub origin_attempt: u64,
    pub origin_scene: i32,
    pub origin_known: bool,
    pub detail_valid: u32,
    pub detail: [i64; 8],
}

const _: () = assert!(std::mem::size_of::<Event>() <= 512);

#[derive(Clone, Copy, Default)]
struct Summary {
    maximum: Event,
    count: u64,
    total: u64,
    minimum: i64,
    first: i64,
    last: i64,
    slow: u64,
    suppressed: u64,
    budget_start: i64,
    examples: u8,
    dirty: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Losses {
    pub suppressed_examples: u64,
    pub span_invocations: u64,
    pub span_contention: u64,
    pub invalid_spans: u64,
    pub sample_capacity: u64,
    pub context_contention: u64,
}

#[derive(Default)]
pub struct Sampler {
    first: Option<i64>,
    previous: Option<i64>,
    emitted: Option<i64>,
    key: u64,
    pub observations: u64,
    pub max_gap: i64,
}

impl Sampler {
    pub fn take(&mut self, now: i64, frequency: i64, key: u64) -> bool {
        if self.key != key {
            *self = Self {
                key,
                ..Self::default()
            };
        }
        let first = *self.first.get_or_insert(now);
        let gap = self.previous.and_then(|p| now.checked_sub(p)).unwrap_or(0);
        self.previous = Some(now);
        self.observations = self.observations.saturating_add(1);
        self.max_gap = self.max_gap.max(gap);
        let period = if elapsed_ms(first, now, frequency).unwrap_or(0.0) < 3000.0 {
            5.0
        } else {
            250.0
        };
        let due = self
            .emitted
            .and_then(|p| elapsed_ms(p, now, frequency))
            .map_or(true, |ms| ms >= period);
        if due || elapsed_ms(0, gap, frequency).is_some_and(|ms| ms >= 50.0) {
            self.emitted = Some(now);
            true
        } else {
            false
        }
    }
}

struct State<const N: usize> {
    ring: [Event; N],
    head: usize,
    len: usize,
    scene: i32,
    epoch: u64,
    attempt: u64,
    samplers: [Sampler; 3],
    previous: [Sample; 2],
    anchors: [(u64, i64); 2],
    segments: [u64; 2],
    ready: Option<(i32, i32)>,
    summaries: [Summary; 32],
    example_window: i64,
    examples: u8,
}

pub struct Recorder<const N: usize> {
    state: Mutex<State<N>>,
    frequency: i64,
    full: AtomicU64,
    busy: AtomicU64,
    context_lost: AtomicBool,
    suppressed_examples: AtomicU64,
    span_invocations: AtomicU64,
    span_contention: AtomicU64,
    invalid_spans: AtomicU64,
    sample_capacity: AtomicU64,
    context_contention: AtomicU64,
}

impl<const N: usize> Recorder<N> {
    pub fn new(frequency: i64, scene: i32) -> Self {
        Self {
            state: Mutex::new(State {
                ring: [Event::default(); N],
                head: 0,
                len: 0,
                scene,
                epoch: 0,
                attempt: 0,
                samplers: Default::default(),
                previous: Default::default(),
                anchors: [(0, 0); 2],
                segments: [0; 2],
                ready: None,
                summaries: [Summary::default(); 32],
                example_window: 0,
                examples: 0,
            }),
            frequency,
            full: AtomicU64::new(0),
            busy: AtomicU64::new(0),
            context_lost: AtomicBool::new(scene < 0),
            suppressed_examples: AtomicU64::new(0),
            span_invocations: AtomicU64::new(0),
            span_contention: AtomicU64::new(0),
            invalid_spans: AtomicU64::new(0),
            sample_capacity: AtomicU64::new(0),
            context_contention: AtomicU64::new(0),
        }
    }

    pub fn context(&self) -> Context {
        let Ok(state) = self.state.try_lock() else {
            self.context_contention.fetch_add(1, Ordering::Relaxed);
            return Context::default();
        };
        Context {
            attempt: state.attempt,
            scene: state.scene,
            epoch: state.epoch,
            valid: !self.context_lost.load(Ordering::Relaxed),
        }
    }

    /// One attempt at the lock. No retry, allocation, formatting or IO.
    pub fn record(&self, mut event: Event) -> bool {
        if event.kind == Kind::Span {
            self.span_invocations.fetch_add(1, Ordering::Relaxed);
        }
        let Ok(mut state) = self.state.try_lock() else {
            self.busy.fetch_add(1, Ordering::Relaxed);
            if event.kind == Kind::Span {
                self.span_contention.fetch_add(1, Ordering::Relaxed);
            }
            if event.kind == Kind::Scene {
                self.context_lost.store(true, Ordering::Relaxed);
            }
            return false;
        };
        if event.kind == Kind::Scene {
            if matches!(event.scene, 16 | 26)
                || (event.scene == 27 && state.scene != 26)
                || (event.scene == 28 && !matches!(state.scene, 26 | 27))
            {
                state.attempt = state.attempt.saturating_add(1);
            }
            state.scene = event.scene;
            state.epoch = state.epoch.saturating_add(1);
            state.samplers = Default::default();
            state.anchors = [(0, 0); 2];
            state.ready = None;
            // Following a missed scene, existing attempt attribution stays unknown.
            if event.scene == 26 {
                self.context_lost.store(false, Ordering::Relaxed);
            }
        }
        event.context_valid = !self.context_lost.load(Ordering::Relaxed);
        event.scene = state.scene;
        event.scene_epoch = state.epoch;
        event.attempt = state.attempt;
        // Slow examples and samples cannot consume the final quarter of the
        // ring. Lifecycle and per-hit events retain that headroom.
        let example_room = state.len < N.saturating_sub((N / 4).max(1));
        if event.kind == Kind::Span {
            let duration = event.end_qpc.checked_sub(event.qpc);
            if event.qpc < 0 || !duration.is_some_and(|d| d >= 0) || self.frequency <= 0 {
                self.invalid_spans.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            let duration = duration.unwrap_or(0);
            if event.qpc.saturating_sub(state.example_window) >= self.frequency {
                state.example_window = event.qpc;
                state.examples = 0;
            }
            let global_budget = state.examples < 64;
            let Some(summary) = state.summaries.get_mut(event.id as usize) else {
                self.invalid_spans.fetch_add(1, Ordering::Relaxed);
                return false;
            };
            if summary.count == 0 {
                summary.first = event.qpc;
                summary.minimum = duration;
                summary.maximum = event;
                summary.budget_start = event.qpc;
            }
            summary.count = summary.count.saturating_add(1);
            summary.total = summary.total.saturating_add(duration as u64);
            summary.minimum = summary.minimum.min(duration);
            summary.first = summary.first.min(event.qpc);
            summary.last = summary.last.max(event.qpc);
            summary.dirty = true;
            if duration > summary.maximum.end_qpc.saturating_sub(summary.maximum.qpc) {
                summary.maximum = event;
            }
            if (duration as i128) * 1_000_000 < (self.frequency as i128) * 250 {
                return false;
            }
            summary.slow = summary.slow.saturating_add(1);
            if event.qpc.saturating_sub(summary.budget_start) >= self.frequency {
                summary.budget_start = event.qpc;
                summary.examples = 0;
            }
            if !example_room || !global_budget || summary.examples >= 4 {
                summary.suppressed = summary.suppressed.saturating_add(1);
                self.suppressed_examples.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            summary.examples += 1;
            state.examples += 1;
        }
        if event.kind == Kind::PrepareRequest {
            state.ready = None;
        }
        if event.kind == Kind::AnchorDelivered && event.result == 1 {
            if let Some(anchor) = state.anchors.get_mut(event.side as usize) {
                *anchor = (event.sample.actor, event.sample.anchor);
            }
            if let Some(segment) = state.segments.get_mut(event.side as usize) {
                *segment = segment.saturating_add(1);
            }
        }
        if event.kind == Kind::ReadyObserved {
            let key = (event.id, event.result);
            if state.ready == Some(key) {
                return false;
            }
            state.ready = Some(key);
        }
        if event.kind == Kind::GameplaySample {
            let Ok(side) = usize::try_from(event.side) else {
                return false;
            };
            let Some(previous) = state.previous.get(side).copied() else {
                return false;
            };
            if state.anchors.get(side) != Some(&(event.sample.actor, event.sample.anchor)) {
                event.sample.valid &= !VALID_ANCHOR;
            }
            if !event.sample.same_domain(&previous)
                || event.sample.music_count < previous.music_count
            {
                state.segments[side] = state.segments[side].saturating_add(1);
            }
            state.previous[side] = event.sample;
            event.segment = state.segments[side];
            let key = event.segment;
            let sampler = &mut state.samplers[side];
            if !sampler.take(event.qpc, self.frequency, key) {
                return false;
            }
            event.observations = sampler.observations;
            event.max_gap_qpc = sampler.max_gap;
        }
        if event.kind == Kind::Frame {
            let key = event.scene_epoch;
            let sampler = &mut state.samplers[2];
            if !sampler.take(event.qpc, self.frequency, key) {
                return false;
            }
            event.observations = sampler.observations;
            event.max_gap_qpc = sampler.max_gap;
        }
        if matches!(
            event.kind,
            Kind::GameplaySample | Kind::Frame | Kind::OutputCursor
        ) && !example_room
        {
            self.sample_capacity.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        if state.len == N {
            self.full.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let index = (state.head + state.len) % N;
        state.ring[index] = event;
        state.len += 1;
        true
    }

    pub fn drain(&self, out: &mut [Event]) -> usize {
        let Ok(mut state) = self.state.try_lock() else {
            return 0;
        };
        let count = out.len().min(state.len);
        for target in out.iter_mut().take(count) {
            *target = state.ring[state.head];
            state.head = (state.head + 1) % N;
        }
        state.len -= count;
        count
    }

    pub fn drops(&self) -> (u64, u64) {
        (
            self.full.load(Ordering::Relaxed),
            self.busy.load(Ordering::Relaxed),
        )
    }

    /// Cumulative per-scope summaries, independent of the ring and example
    /// budgets. Origin/QPC/trace identifies the maximum-duration invocation;
    /// aggregate counts are process-wide, not assigned to that attempt.
    pub fn summaries(&self, out: &mut [Event]) -> usize {
        let Ok(mut state) = self.state.try_lock() else {
            return 0;
        };
        let mut count = 0;
        for summary in state.summaries.iter_mut().filter(|s| s.dirty) {
            let Some(target) = out.get_mut(count) else {
                break;
            };
            *target = Event {
                kind: Kind::SpanSummary,
                context_valid: false,
                attempt: 0,
                scene: -1,
                scene_epoch: 0,
                observations: summary.count,
                counters: [
                    summary.maximum.counters[0],
                    summary.total,
                    summary.slow,
                    summary.suppressed,
                ],
                detail_valid: 7,
                detail: [summary.minimum, summary.first, summary.last, 0, 0, 0, 0, 0],
                ..summary.maximum
            };
            summary.dirty = false;
            count += 1;
        }
        count
    }

    pub fn losses(&self) -> Losses {
        Losses {
            suppressed_examples: self.suppressed_examples.load(Ordering::Relaxed),
            span_invocations: self.span_invocations.load(Ordering::Relaxed),
            span_contention: self.span_contention.load(Ordering::Relaxed),
            invalid_spans: self.invalid_spans.load(Ordering::Relaxed),
            sample_capacity: self.sample_capacity.load(Ordering::Relaxed),
            context_contention: self.context_contention.load(Ordering::Relaxed),
        }
    }
}

pub fn elapsed_ms(before: i64, now: i64, frequency: i64) -> Option<f64> {
    let ticks = now.checked_sub(before)?;
    if before < 0 || ticks < 0 || frequency <= 0 {
        return None;
    }
    Some(ticks as f64 * 1000.0 / frequency as f64)
}

/// Original is outside observer containment, so it cannot accidentally be retried.
pub fn call_observed<A: Copy, R>(
    args: A,
    original: impl FnOnce(A) -> R,
    observe: impl FnOnce(A, &R),
) -> R {
    let result = original(args);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observe(args, &result)));
    result
}

pub struct CappedWriter<W> {
    pub inner: W,
    pub bytes: u64,
    limit: u64,
}

impl<W: Write> CappedWriter<W> {
    pub fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            bytes: 0,
            limit,
        }
    }
    pub fn write_record(&mut self, bytes: &[u8]) -> io::Result<bool> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.bytes) {
            return Ok(false);
        }
        self.inner.write_all(bytes)?;
        self.bytes += bytes.len() as u64;
        Ok(true)
    }
}

/// Writer-thread only. Empty cells mean unavailable, never fabricated zeros.
pub fn format_event(
    event: &Event,
    frequency: i64,
    previous: &mut [Option<Event>; 2],
    line: &mut String,
) {
    line.clear();
    let _ = write!(
        line,
        "{},{},{},{},{},{},{},{},{},{},",
        event.kind.label(),
        event.qpc,
        event.end_qpc,
        event.attempt,
        event.scene_epoch,
        event.scene,
        event.context_valid,
        event.id,
        event.result,
        event.side
    );
    for byte in event.name.iter().take_while(|b| **b != 0) {
        line.push(if byte.is_ascii_alphanumeric() || b"_-./".contains(byte) {
            *byte as char
        } else {
            '?'
        });
    }
    let s = &event.sample;
    let _ = write!(line, ",{},{},", s.valid, s.actor);
    for (valid, value) in [
        (VALID_ACTOR, s.anchor),
        (VALID_TICK, s.frame_tick),
        (
            if event.kind == Kind::GameplaySample {
                VALID_ACTOR
            } else {
                0
            },
            s.music_count as i64,
        ),
        (VALID_ACTOR, s.raw_count as i64),
        (VALID_OFFSETS, s.offsets[0] as i64),
        (VALID_OFFSETS, s.offsets[1] as i64),
        (VALID_OFFSETS, s.offsets[2] as i64),
        (VALID_OFFSETS, s.offsets[3] as i64),
        (VALID_OPTION, s.offsets[4] as i64),
        (VALID_RATE, s.rate_q31 as i64),
    ] {
        if s.valid & valid != 0 {
            let _ = write!(line, "{}", value);
        }
        line.push(',');
    }
    let _ = write!(
        line,
        "{},{},{},{},{},{},{},",
        event.segment,
        event.observations,
        event.max_gap_qpc,
        event.counters[0],
        event.counters[1],
        event.counters[2],
        event.counters[3]
    );
    let mut progression = None;
    if event.kind == Kind::GameplaySample {
        if let Some(slot) = previous.get_mut(event.side as usize) {
            if let Some(p) = slot.filter(|p| {
                p.context_valid
                    && event.context_valid
                    && p.attempt == event.attempt
                    && p.scene_epoch == event.scene_epoch
                    && p.segment == event.segment
            }) {
                progression = s.progress_ms(&p.sample, p.qpc, event.qpc, frequency);
            }
            *slot = Some(*event);
        }
    }
    if let Some((wall, mc)) = progression {
        let _ = write!(line, "{:.3},{}", wall, mc);
    } else {
        line.push(',');
    }
    let _ = write!(
        line,
        ",{},{},{},",
        event.trace_id, event.parent_id, event.thread_id
    );
    if event.origin_known {
        let _ = write!(line, "{},{}", event.origin_attempt, event.origin_scene);
    } else {
        line.push(',');
    }
    let _ = write!(line, ",{},{}", event.origin_known, event.detail_valid);
    for (index, value) in event.detail.iter().enumerate() {
        line.push(',');
        if event.detail_valid & (1 << index) != 0 {
            let _ = write!(line, "{}", value);
        }
    }
    line.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn event(kind: Kind, qpc: i64) -> Event {
        Event {
            kind,
            qpc,
            ..Event::default()
        }
    }

    #[test]
    fn ring_full_wrap_and_recovery() {
        let queue = Recorder::<2>::new(1000, 25);
        let mut out = [Event::default(); 3];
        assert_eq!(queue.drain(&mut out), 0);
        assert!(queue.record(event(Kind::BankCreate, 1)));
        assert!(queue.record(event(Kind::BankCreate, 2)));
        assert!(!queue.record(event(Kind::BankCreate, 3)));
        assert_eq!(queue.drops(), (1, 0));
        assert_eq!(queue.drain(&mut out[..1]), 1);
        assert_eq!(out[0].qpc, 1);
        assert!(queue.record(event(Kind::BankCreate, 4)));
        assert_eq!(queue.drain(&mut out), 2);
        assert_eq!([out[0].qpc, out[1].qpc], [2, 4]);
    }

    #[test]
    fn contention_drops_without_waiting_and_recovers() {
        let queue = Recorder::<2>::new(1000, 25);
        let guard = queue.state.lock().unwrap();
        assert!(!queue.record(event(Kind::StopRequest, 1)));
        assert_eq!(queue.drops(), (0, 1));
        drop(guard);
        assert!(queue.record(event(Kind::StopRequest, 2)));
    }

    #[test]
    fn concurrent_producers_account_for_every_record() {
        let queue = Arc::new(Recorder::<512>::new(1000, 25));
        let barrier = Arc::new(Barrier::new(5));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (queue, barrier) = (queue.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    for qpc in 0..500 {
                        queue.record(event(Kind::BankCreate, qpc));
                    }
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        let mut out = [Event::default(); 512];
        let stored = queue.drain(&mut out);
        let (full, busy) = queue.drops();
        assert_eq!(stored as u64 + full + busy, 2000);
    }

    #[test]
    fn same_song_and_handle_are_distinct_attempts() {
        let queue = Recorder::<16>::new(1000, 25);
        for scene in [26, 27, 28, 29, 30, 25, 26, 27, 28] {
            queue.record(Event {
                kind: Kind::Scene,
                scene,
                ..Event::default()
            });
        }
        let mut out = [Event::default(); 16];
        assert_eq!(queue.drain(&mut out), 9);
        assert_eq!([out[0].attempt, out[1].attempt, out[2].attempt], [1, 1, 1]);
        assert_eq!([out[6].attempt, out[7].attempt, out[8].attempt], [2, 2, 2]);
        assert_ne!(out[2].scene_epoch, out[8].scene_epoch);
        queue.record(Event {
            kind: Kind::Scene,
            scene: 27,
            ..Event::default()
        });
        queue.record(Event {
            kind: Kind::Scene,
            scene: 28,
            ..Event::default()
        });
        assert_eq!(queue.drain(&mut out), 2);
        assert_eq!(out[0].attempt, 3);
        assert_eq!(out[1].attempt, 3);
    }

    #[test]
    fn full_ring_does_not_lose_scene_identity() {
        let queue = Recorder::<1>::new(1000, 25);
        queue.record(event(Kind::BankCreate, 1));
        queue.record(Event {
            kind: Kind::Scene,
            scene: 26,
            ..Event::default()
        });
        let mut out = [Event::default(); 1];
        queue.drain(&mut out);
        queue.record(event(Kind::PrepareRequest, 2));
        queue.drain(&mut out);
        assert_eq!((out[0].attempt, out[0].scene), (1, 26));
    }

    #[test]
    fn cadence_is_dense_then_periodic_and_reports_gaps() {
        let mut sampler = Sampler::default();
        assert!(sampler.take(1000, 1000, 1));
        assert!(!sampler.take(1001, 1000, 1));
        assert!(sampler.take(1005, 1000, 1));
        assert!(sampler.take(5000, 1000, 1));
        assert!(!sampler.take(5005, 1000, 1));
        assert!(sampler.take(5105, 1000, 1)); // >50ms gap
        assert!(!sampler.take(5110, 1000, 1));
        assert!(sampler.take(5111, 1000, 2)); // discontinuity
    }

    #[test]
    fn pre_anchor_and_state_changes_break_progression() {
        let base = Sample {
            valid: VALID_ACTOR | VALID_RATE,
            actor: 7,
            anchor: 100,
            rate_q31: 1 << 31,
            music_count: 500,
            ..Sample::default()
        };
        assert!(base.progress_ms(&base, 1000, 1010, 1000).is_none());
        let base = Sample {
            valid: base.valid | VALID_ANCHOR,
            ..base
        };
        let next = Sample {
            music_count: 510,
            ..base
        };
        assert_eq!(next.progress_ms(&base, 1000, 1010, 1000), Some((10.0, 10)));
        for changed in [
            Sample {
                anchor: 101,
                ..next
            },
            Sample {
                rate_q31: 1 << 30,
                ..next
            },
            Sample {
                offsets: [1, 0, 0, 0, 0],
                ..next
            },
            Sample { actor: 8, ..next },
            Sample {
                music_count: -20,
                ..next
            },
        ] {
            assert!(changed.progress_ms(&base, 1000, 1010, 1000).is_none());
        }
    }

    #[test]
    fn qpc_conversion_is_unwrapped_and_rejects_invalid_clocks() {
        assert_eq!(elapsed_ms(i64::MAX - 30, i64::MAX, 3000), Some(10.0));
        assert_eq!(elapsed_ms(7, 6, 1000), None);
        assert_eq!(elapsed_ms(0, 1, 0), None);
        assert_eq!(elapsed_ms(-1, 1, 1000), None);
    }

    #[test]
    fn writer_cap_and_io_failure_are_fallible() {
        let mut writer = CappedWriter::new(Vec::new(), 4);
        assert_eq!(writer.write_record(b"abc\n").unwrap(), true);
        assert_eq!(writer.write_record(b"x").unwrap(), false);
        assert_eq!(writer.bytes, 4);
        assert_eq!(writer.inner, b"abc\n");
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::Other.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(CappedWriter::new(Broken, 4).write_record(b"a").is_err());
    }

    #[test]
    fn observer_failure_preserves_arguments_result_and_one_original_call() {
        let calls = std::cell::Cell::new(0);
        let result = call_observed(
            (5, 123usize),
            |args| {
                assert_eq!(args, (5, 123));
                calls.set(calls.get() + 1);
                -1
            },
            |_, _| panic!("observer failure"),
        );
        assert_eq!(result, -1);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn zero_capacity_is_a_drop_not_a_panic() {
        assert!(!Recorder::<0>::new(1000, 25).record(Event::default()));
    }

    #[test]
    fn anchor_must_be_observed_for_this_actor_and_scene() {
        let queue = Recorder::<8>::new(1000, 28);
        let sample = Sample {
            actor: 7,
            anchor: 9,
            valid: VALID_ACTOR | VALID_ANCHOR,
            ..Sample::default()
        };
        queue.record(Event {
            kind: Kind::GameplaySample,
            sample,
            side: 0,
            qpc: 1,
            ..Event::default()
        });
        queue.record(Event {
            kind: Kind::AnchorDelivered,
            sample,
            side: 0,
            result: 1,
            qpc: 2,
            ..Event::default()
        });
        queue.record(Event {
            kind: Kind::GameplaySample,
            sample,
            side: 0,
            qpc: 3,
            ..Event::default()
        });
        queue.record(Event {
            kind: Kind::Scene,
            scene: 29,
            qpc: 4,
            ..Event::default()
        });
        queue.record(Event {
            kind: Kind::GameplaySample,
            sample,
            side: 0,
            qpc: 5,
            ..Event::default()
        });
        let mut out = [Event::default(); 8];
        assert_eq!(queue.drain(&mut out), 5);
        assert_eq!(out[0].sample.valid & VALID_ANCHOR, 0);
        assert_ne!(out[2].sample.valid & VALID_ANCHOR, 0);
        assert_eq!(out[4].sample.valid & VALID_ANCHOR, 0);
    }

    #[test]
    fn csv_unavailable_is_blank_and_progress_never_crosses_attempts() {
        let mut previous = [None; 2];
        let mut line = String::new();
        format_event(&Event::default(), 1000, &mut previous, &mut line);
        let cells: Vec<_> = line.trim_end().split(',').collect();
        assert_eq!(cells.len(), 47);
        assert!(cells[13..23].iter().all(|s| s.is_empty()));
        assert_eq!(&cells[30..32], &["", ""]);
        let event = Event {
            kind: Kind::GameplaySample,
            qpc: 100,
            side: 0,
            attempt: 1,
            context_valid: true,
            sample: Sample {
                valid: VALID_ACTOR | VALID_ANCHOR | VALID_RATE,
                anchor: 50,
                rate_q31: 1 << 31,
                ..Sample::default()
            },
            ..Event::default()
        };
        format_event(&event, 1000, &mut previous, &mut line);
        format_event(&Event { qpc: 110, ..event }, 1000, &mut previous, &mut line);
        assert_eq!(
            &line.trim_end().split(',').collect::<Vec<_>>()[30..32],
            &["10.000", "0"]
        );
        format_event(
            &Event {
                qpc: 120,
                attempt: 2,
                ..event
            },
            1000,
            &mut previous,
            &mut line,
        );
        assert_eq!(
            &line.trim_end().split(',').collect::<Vec<_>>()[30..32],
            &["", ""]
        );
    }

    #[test]
    fn missed_scene_invalidates_attribution_until_a_fresh_attempt() {
        let queue = Recorder::<4>::new(1000, 28);
        let guard = queue.state.lock().unwrap();
        queue.record(Event {
            kind: Kind::Scene,
            scene: 29,
            ..Event::default()
        });
        drop(guard);
        queue.record(event(Kind::StopRequest, 10));
        queue.record(Event {
            kind: Kind::Scene,
            scene: 26,
            ..Event::default()
        });
        queue.record(event(Kind::PrepareRequest, 20));
        let mut out = [Event::default(); 4];
        assert_eq!(queue.drain(&mut out), 3);
        assert!(!out[0].context_valid);
        assert!(out[2].context_valid);
        assert_eq!(out[2].attempt, 1);
    }

    #[test]
    fn unavailable_scene_channel_never_claims_known_context() {
        let queue = Recorder::<1>::new(1000, -1);
        queue.record(event(Kind::PrepareRequest, 1));
        let mut out = [Event::default(); 1];
        queue.drain(&mut out);
        assert!(!out[0].context_valid);
    }

    #[test]
    fn context_is_one_atomic_snapshot_or_unknown() {
        let queue = Recorder::<4>::new(1_000_000, 25);
        assert_eq!(
            queue.context(),
            Context {
                attempt: 0,
                scene: 25,
                epoch: 0,
                valid: true
            }
        );
        let guard = queue.state.lock().unwrap();
        assert!(!queue.context().valid);
        assert_eq!(queue.losses().context_contention, 1);
        drop(guard);
        queue.record(Event {
            kind: Kind::Scene,
            scene: 26,
            ..Event::default()
        });
        assert_eq!(
            queue.context(),
            Context {
                attempt: 1,
                scene: 26,
                epoch: 1,
                valid: true
            }
        );
    }

    #[test]
    fn v2_trace_schema_keeps_validity_and_origin_independent() {
        assert!(std::mem::size_of::<Event>() <= 512);
        let mut line = String::new();
        let event = Event {
            trace_id: 9,
            parent_id: 7,
            thread_id: 3,
            origin_attempt: 4,
            origin_scene: 28,
            origin_known: true,
            detail_valid: 0b101,
            detail: [-12, 999, 0, 0, 0, 0, 0, 0],
            ..Event::default()
        };
        format_event(&event, 1_000_000, &mut [None; 2], &mut line);
        let cells = line.trim_end().split(',').collect::<Vec<_>>();
        assert_eq!(cells.len(), CSV_COLUMNS.split(',').count());
        assert_eq!(
            &cells[32..],
            &["9", "7", "3", "4", "28", "true", "5", "-12", "", "0", "", "", "", "", ""]
        );
    }

    #[test]
    fn span_summaries_count_every_invocation_and_reserve_critical_room() {
        let queue = Recorder::<4>::new(1_000_000, 28);
        for i in 0..20 {
            queue.record(Event {
                kind: Kind::Span,
                id: 1,
                qpc: i * 1000,
                end_qpc: i * 1000 + if i == 19 { 999 } else { 300 },
                trace_id: i as u64 + 1,
                ..Event::default()
            });
        }
        assert!(queue.record(event(Kind::Judgement, 30_000)));
        let mut summaries = [Event::default(); 32];
        assert_eq!(queue.summaries(&mut summaries), 1);
        let summary = summaries[0];
        assert_eq!(summary.kind, Kind::SpanSummary);
        assert_eq!(summary.observations, 20);
        assert_eq!(
            (summary.qpc, summary.end_qpc, summary.trace_id),
            (19_000, 19_999, 20)
        );
        assert_eq!(summary.counters[1], 19 * 300 + 999);
        assert_eq!(summary.counters[2], 20);
        assert_eq!(summary.counters[3], 17);
        assert_eq!(queue.losses().suppressed_examples, 17);
        assert_eq!(queue.losses().span_invocations, 20);
    }

    #[test]
    fn short_spans_are_summarized_and_contention_is_explicit() {
        let queue = Recorder::<8>::new(1_000_000, 28);
        let span = Event {
            kind: Kind::Span,
            id: 1,
            qpc: 100,
            end_qpc: 249,
            ..Event::default()
        };
        assert!(!queue.record(span));
        let guard = queue.state.lock().unwrap();
        assert!(!queue.record(span));
        drop(guard);
        let mut out = [Event::default(); 32];
        assert_eq!(queue.drain(&mut out), 0);
        assert_eq!(queue.summaries(&mut out), 1);
        assert_eq!(out[0].observations, 1);
        assert_eq!(out[0].counters[1], 149);
        assert_eq!(queue.losses().span_invocations, 2);
        assert_eq!(queue.losses().span_contention, 1);
    }

    #[test]
    fn nested_completion_order_keeps_summary_bounds_chronological() {
        let queue = Recorder::<8>::new(1_000_000, 28);
        for (qpc, end_qpc) in [(20, 30), (10, 40)] {
            queue.record(Event {
                kind: Kind::Span,
                id: 7,
                qpc,
                end_qpc,
                ..Event::default()
            });
        }
        let mut out = [Event::default(); 32];
        queue.summaries(&mut out);
        assert_eq!(&out[0].detail[1..3], &[10, 20]);
    }

    #[test]
    fn slow_budget_reopens_after_a_second_without_resetting_totals() {
        let queue = Recorder::<512>::new(1_000_000, 28);
        for qpc in [0, 1000, 2000, 3000, 4000, 1_000_000] {
            queue.record(Event {
                kind: Kind::Span,
                id: 2,
                qpc,
                end_qpc: qpc + 250,
                ..Event::default()
            });
        }
        let mut out = [Event::default(); 32];
        assert_eq!(queue.drain(&mut out), 5);
        queue.summaries(&mut out);
        assert_eq!(out[0].observations, 6);
        assert_eq!(out[0].counters[3], 1);
    }

    #[test]
    fn cumulative_summary_is_not_attributed_to_the_maximums_attempt() {
        let queue = Recorder::<8>::new(1_000_000, 28);
        queue.record(Event {
            kind: Kind::Span,
            id: 7,
            qpc: 10,
            end_qpc: 20,
            origin_attempt: 3,
            origin_scene: 28,
            origin_known: true,
            ..Event::default()
        });
        let mut out = [Event::default(); 32];
        queue.summaries(&mut out);
        assert!(!out[0].context_valid);
        assert_eq!(out[0].scene, -1);
        assert!(out[0].origin_known);
        assert_eq!(out[0].origin_attempt, 3);
    }
}
