//! Prioritised build scheduler — a small below-normal-priority worker pool
//! fed by wheel-card events and the highlight (design §4.7 `scheduler.rs`,
//! §3.3), so previews are ready before the wheel settles and the gameplay
//! bank never blocks an open.
//!
//! ```text
//! wheel_cards::on_card_bound(mcode)  ─► Preview + Jacket (visible class)
//! highlight changed (per frame)      ─► rehighlight(b): that song's jobs to the head
//! highlight dwell ≥ 1.5 s            ─► FullBank (speculative; cancelled when the
//!                                        highlight moves on in scene 25 uncommitted)
//! resolver, full-variant open        ─► FullBank (gameplay-load class, de-speculated)
//! resolver, preview / jacket open    ─► Preview / Jacket (highlighted class) + wait ≤ 300 ms
//! scene 25 → 26 with a dwell song     ─► sync_apply::request_on_decide (auto-sync measurement)
//! ```
//!
//! Ordering / dedupe / cancel live in the pure [`super::scheduler_logic`];
//! this file owns the threads, the tickets and the triggers. Workers run
//! `max(3, hardware_threads / 2)` strong at `THREAD_PRIORITY_BELOW_NORMAL`
//! — a bound wheel row's three previews encode concurrently without
//! starving the game. Each job logs one INFO with its duration and queue
//! wait.
//!
//! The frame callback and the card subscriber run on the game thread: they
//! read the highlight, do at most a couple of `is_file` stats and take the
//! queue lock briefly — no I/O beyond that.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::services::{input_manager, scene_manager, selectmusic_highlight, wheel_cards};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::cache::{self, BankVariant};
use super::index;
use super::progressive;
use super::providers;
use super::scheduler_logic::{Absorbed, Cancelled, JobKind, JobQueue, Urgency};
use super::sync_apply;

pub use super::scheduler_logic::pool_size;

/// Highlight dwell before the speculative full bank starts.
pub const SPECULATIVE_DWELL: Duration = Duration::from_millis(1500);
/// How long a resolver waits on a ticket before falling back.
pub const RESOLVER_WAIT: Duration = Duration::from_millis(300);
const TICKET_POLL: Duration = Duration::from_millis(2);

/// Handed to requesters; `done` flips when the job ends (however).
pub struct Ticket {
    pub done: AtomicBool,
    pub ok: AtomicBool,
    /// Cooperative cancel for a speculative full bank.
    pub cancel: AtomicBool,
    enqueued: Instant,
}

impl Ticket {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            done: AtomicBool::new(false),
            ok: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            enqueued: Instant::now(),
        })
    }

    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// Block (sleep-poll) until done or `max` elapsed; `true` when done.
    pub fn wait(&self, max: Duration) -> bool {
        let deadline = Instant::now() + max;
        while !self.is_done() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(TICKET_POLL);
        }
        true
    }
}

struct State {
    queue: JobQueue<Arc<Ticket>>,
    pool_started: bool,
    workers: usize,
}

static STATE: Mutex<State> = Mutex::new(State {
    queue: JobQueue::new(),
    pool_started: false,
    workers: 0,
});
static WAKE: Condvar = Condvar::new();

fn lock_state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Queue a job (dedupe by `(kind, basename)`); starts the pool on first
/// use. Returns the ticket to wait on.
pub fn enqueue(kind: JobKind, basename: &str, urgency: Urgency, speculative: bool) -> Arc<Ticket> {
    let mut st = lock_state();
    ensure_pool(&mut st);
    let (ticket, absorbed) = st
        .queue
        .enqueue(kind, basename, urgency, speculative, Ticket::new);
    if absorbed == Absorbed::Queued {
        WAKE.notify_one();
    }
    drop(st);
    if absorbed == Absorbed::Queued && urgency == Urgency::GameplayLoad {
        log_info!(
            "StepMania[sched]: full-bank '{}' requested by the gameplay load -- head of queue",
            basename
        );
    }
    ticket
}

/// The highlight moved to `basename`: promote its pending jobs.
pub fn rehighlight(basename: &str) {
    lock_state().queue.rehighlight(basename);
}

/// The highlight left `basename` without a commit: drop or flag its
/// speculative full bank.
pub fn cancel_speculative(basename: &str) {
    let found = lock_state().queue.cancel_speculative(basename);
    match found {
        Cancelled::Dropped(ticket) => {
            ticket.cancel.store(true, Ordering::Release);
            ticket.done.store(true, Ordering::Release);
            log_info!(
                "StepMania[sched]: speculative full-bank '{}' dropped before it ran",
                basename
            );
        }
        Cancelled::Running(ticket) => {
            ticket.cancel.store(true, Ordering::Release);
            if let Some(h) = progressive::active(basename) {
                h.cancel.store(true, Ordering::Release);
            }
            log_info!(
                "StepMania[sched]: speculative full-bank '{}' cancel requested (running)",
                basename
            );
        }
        Cancelled::None => {}
    }
}

/// `(pending, running, workers)` for status lines.
#[must_use]
pub fn stats() -> (usize, usize, usize) {
    let st = lock_state();
    (st.queue.pending_len(), st.queue.running_len(), st.workers)
}

fn ensure_pool(st: &mut State) {
    if st.pool_started {
        return;
    }
    st.pool_started = true;
    let n = pool_size(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
    );
    let mut spawned = 0usize;
    for i in 0..n {
        let r = std::thread::Builder::new()
            .name(format!("stepmania-build-{i}"))
            .stack_size(4 << 20)
            .spawn(worker_main);
        match r {
            Ok(_) => spawned += 1,
            Err(e) => log_warn!("StepMania[sched]: worker {} failed to start: {}", i, e),
        }
    }
    st.workers = spawned;
    log_info!(
        "StepMania[sched]: pool started -- {} worker(s), below-normal priority",
        spawned
    );
}

fn lower_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

fn worker_main() {
    lower_priority();
    loop {
        let job = {
            let mut st = lock_state();
            loop {
                if let Some(job) = st.queue.pop() {
                    break job;
                }
                st = WAKE.wait(st).unwrap_or_else(|p| p.into_inner());
            }
        };
        let queued_ms = job.payload.enqueued.elapsed().as_millis();
        let started = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_job(job.kind, &job.basename, &job.payload)
        }));
        let ok = matches!(outcome, Ok(Ok(_)));
        match &outcome {
            Ok(Ok(detail)) => log_info!(
                "StepMania[sched]: {} '{}' done -- {} ({} ms, queued {} ms{})",
                job.kind,
                job.basename,
                detail,
                started.elapsed().as_millis(),
                queued_ms,
                if job.speculative { ", speculative" } else { "" }
            ),
            Ok(Err(e)) => log_warn!(
                "StepMania[sched]: {} '{}' failed -- {} ({} ms, queued {} ms)",
                job.kind,
                job.basename,
                e,
                started.elapsed().as_millis(),
                queued_ms
            ),
            Err(_) => log_warn!(
                "StepMania[sched]: {} '{}' panicked ({} ms) -- song flagged audio_failed",
                job.kind,
                job.basename,
                started.elapsed().as_millis()
            ),
        }
        if outcome.is_err() && job.kind != JobKind::Jacket {
            index::mark_audio_failed(&job.basename);
        }
        job.payload.ok.store(ok, Ordering::Release);
        job.payload.done.store(true, Ordering::Release);
        lock_state().queue.finish(job.kind, &job.basename);
        if job.kind == JobKind::FullBank {
            progressive::retry_pending_renames();
        }
    }
}

fn run_job(kind: JobKind, basename: &str, ticket: &Ticket) -> Result<String, String> {
    let entry = index::entry_by_basename(basename).ok_or("not in the index")?;
    match kind {
        JobKind::Preview => providers::run_preview_job(&entry),
        JobKind::Jacket => providers::run_jacket_job(&entry),
        JobKind::FullBank => providers::run_full_bank_job(&entry, ticket),
    }
}

// ---------------------------------------------------------------------
// Triggers (game thread)
// ---------------------------------------------------------------------

struct Dwell {
    mcode: Option<i32>,
    basename: Option<String>,
    since: Option<Instant>,
    speculated: bool,
}

static DWELL: Mutex<Dwell> = Mutex::new(Dwell {
    mcode: None,
    basename: None,
    since: None,
    speculated: false,
});
static TRIGGERS_ARMED: AtomicBool = AtomicBool::new(false);

/// Subscribe to the card events, the per-frame highlight poll and the scene
/// changes. Idempotent; call once the services are up (mod init).
pub fn init_triggers() {
    if TRIGGERS_ARMED.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut armed = Vec::new();
    if wheel_cards::is_available() {
        wheel_cards::on_card_bound(on_card_bound);
        armed.push("card-bound");
    } else {
        log_warn!(
            "StepMania[sched]: wheel_cards unavailable -- previews/jackets build on open (no prefetch)"
        );
    }
    if input_manager::is_available() && selectmusic_highlight::is_available() {
        input_manager::on_frame(Arc::new(on_frame));
        armed.push("highlight");
    } else {
        log_warn!(
            "StepMania[sched]: highlight poll unavailable -- no re-prioritisation / speculative banks"
        );
    }
    if scene_manager::is_available() {
        scene_manager::on_scene_change(Box::new(on_scene_change));
        armed.push("scene");
    }
    log_info!(
        "StepMania[sched]: triggers armed [{}]; pool of {} on first job",
        armed.join(", "),
        pool_size(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        )
    );
}

fn preview_missing(basename: &str) -> bool {
    !cache::xwb_path(basename, BankVariant::Preview).is_file()
}

fn jacket_missing(basename: &str) -> bool {
    !cache::jacket_path(basename).is_file() || cache::jacket_placeholder_marker(basename).is_file()
}

fn full_missing(basename: &str) -> bool {
    !cache::xwb_path(basename, BankVariant::Full).is_file()
        && progressive::active(basename).is_none()
}

/// `wheel_cards` subscriber: a card bound to an injected song → prefetch.
fn on_card_bound(ev: &wheel_cards::CardBound) {
    if ev.mcode < 0 {
        return;
    }
    let Some(entry) = index::entry_by_mcode(ev.mcode as u32) else {
        return;
    };
    let b = entry.basename.as_str();
    if preview_missing(b) {
        enqueue(JobKind::Preview, b, Urgency::VisiblePreview, false);
    }
    if jacket_missing(b) {
        enqueue(JobKind::Jacket, b, Urgency::VisibleJacket, false);
    }
}

/// Per-frame highlight poll (scene 25 only). An unreadable highlight is
/// "no change" — the chain can be mid-rebind for a frame.
fn on_frame() {
    if !scene_manager::is_available() || scene_manager::current_scene() != scene::SONG_SELECT {
        return;
    }
    let Some(mcode) = selectmusic_highlight::highlighted_mcode() else {
        return;
    };
    let Ok(mut d) = DWELL.lock() else {
        return;
    };
    if Some(mcode) != d.mcode {
        if d.speculated {
            if let Some(prev) = d.basename.as_deref() {
                cancel_speculative(prev);
            }
        }
        d.mcode = Some(mcode);
        d.since = Some(Instant::now());
        d.speculated = false;
        d.basename = index::entry_by_mcode(mcode as u32).map(|e| e.basename.clone());
        if let Some(b) = d.basename.clone() {
            drop(d);
            rehighlight(&b);
            if preview_missing(&b) {
                enqueue(JobKind::Preview, &b, Urgency::HighlightedPreview, false);
            }
            if jacket_missing(&b) {
                enqueue(JobKind::Jacket, &b, Urgency::HighlightedJacket, false);
            }
        }
        return;
    }
    if d.speculated {
        return;
    }
    let Some(b) = d.basename.clone() else {
        return;
    };
    if d.since.is_some_and(|s| s.elapsed() >= SPECULATIVE_DWELL) {
        d.speculated = true;
        drop(d);
        if full_missing(&b) {
            enqueue(JobKind::FullBank, &b, Urgency::SpeculativeFullBank, true);
        }
    }
}

/// Leaving the wheel: a decide (→ 26) keeps the speculative bank — it is
/// about to be requested for real — and is the auto-sync on-decide trigger
/// for the committed (dwell) song; any other exit cancels the bank.
fn on_scene_change(prev: i32, next: i32) {
    let Ok(mut d) = DWELL.lock() else {
        return;
    };
    let speculated = std::mem::take(&mut d.speculated);
    let basename = d.basename.take();
    d.mcode = None;
    d.since = None;
    drop(d);
    if prev != scene::SONG_SELECT {
        return;
    }
    let Some(b) = basename else {
        return;
    };
    if next == scene::SONG_TO_STAGE_INTERSTITIAL {
        sync_apply::request_on_decide(&b);
    } else if speculated {
        cancel_speculative(&b);
    }
}
