//! Pure priority queue behind `scheduler.rs` — ordering, dedupe, promotion
//! and speculative cancel, with no threads or I/O so the harness can drive
//! it (design §4.7 `scheduler.rs`).
//!
//! Jobs are keyed by `(kind, basename)`; the queue holds at most one pending
//! and one running job per key. Ordering is `(urgency, seq)`: the urgency
//! class first (gameplay-load full bank, highlighted preview, visible
//! previews, highlighted jacket, visible jackets, speculative full bank),
//! then enqueue order — so a freshly bound wheel row's three previews run in
//! binding order and the highlighted song's always jumps ahead. The payload
//! `T` is whatever the owner needs per job (the real scheduler stores an
//! `Arc<Ticket>`); it is handed back on dedupe so a second requester waits
//! on the first job.
//!
//! Harness-mounted: no `crate::` paths.

use std::fmt;

/// What a job produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// `xwb/<b>.p.xwb` — the wheel-settle preview bank.
    Preview,
    /// `jacket/<b>_jk.arc`.
    Jacket,
    /// `xwb/<b>.xwb` — the progressive gameplay bank.
    FullBank,
}

impl JobKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Jacket => "jacket",
            Self::FullBank => "full-bank",
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Priority class; lower runs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Urgency {
    /// The gameplay load asked for this full bank (DancePlaySequence).
    GameplayLoad = 0,
    HighlightedPreview = 1,
    VisiblePreview = 2,
    HighlightedJacket = 3,
    VisibleJacket = 4,
    /// Full bank started on highlight dwell, cancellable.
    SpeculativeFullBank = 5,
}

impl Urgency {
    /// The class a `kind` gets when its song is merely visible.
    #[must_use]
    pub const fn visible(kind: JobKind) -> Self {
        match kind {
            JobKind::Preview => Self::VisiblePreview,
            JobKind::Jacket => Self::VisibleJacket,
            JobKind::FullBank => Self::SpeculativeFullBank,
        }
    }

    /// The class a `kind` gets when its song is highlighted.
    #[must_use]
    pub const fn highlighted(kind: JobKind) -> Self {
        match kind {
            JobKind::Preview => Self::HighlightedPreview,
            JobKind::Jacket => Self::HighlightedJacket,
            JobKind::FullBank => Self::SpeculativeFullBank,
        }
    }

    /// Highlighted classes drop back to their visible class when the
    /// highlight moves on.
    #[must_use]
    pub const fn demoted(self) -> Self {
        match self {
            Self::HighlightedPreview => Self::VisiblePreview,
            Self::HighlightedJacket => Self::VisibleJacket,
            other => other,
        }
    }
}

#[derive(Debug, Clone)]
struct Pending<T> {
    kind: JobKind,
    basename: String,
    urgency: Urgency,
    seq: u64,
    speculative: bool,
    payload: T,
}

#[derive(Debug, Clone)]
struct Running<T> {
    kind: JobKind,
    basename: String,
    speculative: bool,
    payload: T,
}

/// How an `enqueue` was absorbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absorbed {
    /// A new pending job.
    Queued,
    /// Merged into a pending job for the same key (payload is the existing one).
    MergedPending,
    /// The same key is running already (payload is the running job's).
    AlreadyRunning,
}

/// A job popped for execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job<T> {
    pub kind: JobKind,
    pub basename: String,
    pub urgency: Urgency,
    pub speculative: bool,
    pub seq: u64,
    pub payload: T,
}

/// What `cancel_speculative` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cancelled<T> {
    /// Removed from the pending list before it ran.
    Dropped(T),
    /// Already running — the owner must flag its payload.
    Running(T),
    /// No speculative full-bank job for that song.
    None,
}

#[derive(Debug, Clone, Default)]
pub struct JobQueue<T> {
    pending: Vec<Pending<T>>,
    running: Vec<Running<T>>,
    next_seq: u64,
}

impl<T: Clone> JobQueue<T> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            running: Vec::new(),
            next_seq: 0,
        }
    }

    /// Add (or merge into) the job for `(kind, basename)`. `make` builds
    /// the payload only for a genuinely new job. A non-speculative request
    /// clears the speculative flag of the job it merges into (pending or
    /// running) so a later highlight move cannot cancel it; the urgency is
    /// the more urgent of the two.
    pub fn enqueue(
        &mut self,
        kind: JobKind,
        basename: &str,
        urgency: Urgency,
        speculative: bool,
        make: impl FnOnce() -> T,
    ) -> (T, Absorbed) {
        if let Some(r) = self
            .running
            .iter_mut()
            .find(|r| r.kind == kind && r.basename == basename)
        {
            r.speculative &= speculative;
            return (r.payload.clone(), Absorbed::AlreadyRunning);
        }
        if let Some(p) = self
            .pending
            .iter_mut()
            .find(|p| p.kind == kind && p.basename == basename)
        {
            if urgency < p.urgency {
                p.urgency = urgency;
            }
            p.speculative &= speculative;
            return (p.payload.clone(), Absorbed::MergedPending);
        }
        let payload = make();
        let seq = self.next_seq;
        self.next_seq += 1;
        self.pending.push(Pending {
            kind,
            basename: basename.to_string(),
            urgency,
            seq,
            speculative,
            payload: payload.clone(),
        });
        (payload, Absorbed::Queued)
    }

    /// The most urgent pending job (ties by enqueue order), moved to running.
    pub fn pop(&mut self) -> Option<Job<T>> {
        let idx = self
            .pending
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| (p.urgency, p.seq))
            .map(|(i, _)| i)?;
        let p = self.pending.swap_remove(idx);
        self.running.push(Running {
            kind: p.kind,
            basename: p.basename.clone(),
            speculative: p.speculative,
            payload: p.payload.clone(),
        });
        Some(Job {
            kind: p.kind,
            basename: p.basename,
            urgency: p.urgency,
            speculative: p.speculative,
            seq: p.seq,
            payload: p.payload,
        })
    }

    /// A popped job finished (however it ended).
    pub fn finish(&mut self, kind: JobKind, basename: &str) {
        self.running
            .retain(|r| !(r.kind == kind && r.basename == basename));
    }

    /// The highlight moved to `basename`: every pending highlighted-class
    /// job of another song drops to its visible class; this song's pending
    /// preview / jacket become highlighted-class. Pending full banks are
    /// untouched.
    pub fn rehighlight(&mut self, basename: &str) {
        for p in &mut self.pending {
            if p.basename == basename {
                p.urgency = p.urgency.min(Urgency::highlighted(p.kind));
            } else {
                p.urgency = p.urgency.demoted();
            }
        }
    }

    /// Drop (or report running) the speculative full-bank job for
    /// `basename`. Non-speculative jobs are never touched.
    pub fn cancel_speculative(&mut self, basename: &str) -> Cancelled<T> {
        if let Some(i) = self
            .pending
            .iter()
            .position(|p| p.kind == JobKind::FullBank && p.basename == basename && p.speculative)
        {
            return Cancelled::Dropped(self.pending.swap_remove(i).payload);
        }
        if let Some(r) = self
            .running
            .iter()
            .find(|r| r.kind == JobKind::FullBank && r.basename == basename && r.speculative)
        {
            return Cancelled::Running(r.payload.clone());
        }
        Cancelled::None
    }

    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn running_len(&self) -> usize {
        self.running.len()
    }

    /// Whether `(kind, basename)` is pending or running.
    #[must_use]
    pub fn contains(&self, kind: JobKind, basename: &str) -> bool {
        self.pending
            .iter()
            .any(|p| p.kind == kind && p.basename == basename)
            || self
                .running
                .iter()
                .any(|r| r.kind == kind && r.basename == basename)
    }

    /// Pending keys in pop order (for diagnostics / tests).
    #[must_use]
    pub fn pending_order(&self) -> Vec<(JobKind, String)> {
        let mut v: Vec<&Pending<T>> = self.pending.iter().collect();
        v.sort_by_key(|p| (p.urgency, p.seq));
        v.into_iter()
            .map(|p| (p.kind, p.basename.clone()))
            .collect()
    }
}

/// Worker-pool size: at least three (one wheel row of previews runs
/// concurrently), otherwise half the hardware threads.
#[must_use]
pub fn pool_size(available_parallelism: usize) -> usize {
    (available_parallelism / 2).max(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> JobQueue<u32> {
        JobQueue::new()
    }

    #[test]
    fn pops_by_urgency_then_enqueue_order() {
        let mut q = q();
        let mut n = 0u32;
        let mut mk = || {
            n += 1;
            n
        };
        q.enqueue(JobKind::Jacket, "a", Urgency::VisibleJacket, false, &mut mk);
        q.enqueue(
            JobKind::Preview,
            "a",
            Urgency::VisiblePreview,
            false,
            &mut mk,
        );
        q.enqueue(
            JobKind::Preview,
            "b",
            Urgency::VisiblePreview,
            false,
            &mut mk,
        );
        q.enqueue(
            JobKind::FullBank,
            "c",
            Urgency::SpeculativeFullBank,
            true,
            &mut mk,
        );
        q.enqueue(
            JobKind::Preview,
            "c",
            Urgency::HighlightedPreview,
            false,
            &mut mk,
        );
        q.enqueue(
            JobKind::FullBank,
            "d",
            Urgency::GameplayLoad,
            false,
            &mut mk,
        );
        let order: Vec<(JobKind, String)> = std::iter::from_fn(|| q.pop())
            .map(|j| (j.kind, j.basename))
            .collect();
        assert_eq!(
            order,
            vec![
                (JobKind::FullBank, "d".into()),
                (JobKind::Preview, "c".into()),
                (JobKind::Preview, "a".into()),
                (JobKind::Preview, "b".into()),
                (JobKind::Jacket, "a".into()),
                (JobKind::FullBank, "c".into()),
            ]
        );
        assert_eq!(q.running_len(), 6);
        q.finish(JobKind::Preview, "a");
        assert_eq!(q.running_len(), 5);
    }

    #[test]
    fn dedupes_pending_and_running_and_hands_back_the_first_payload() {
        let mut q = q();
        let (t1, a1) = q.enqueue(JobKind::Preview, "a", Urgency::VisiblePreview, false, || 1);
        let (t2, a2) = q.enqueue(
            JobKind::Preview,
            "a",
            Urgency::HighlightedPreview,
            false,
            || 2,
        );
        assert_eq!((t1, a1), (1, Absorbed::Queued));
        assert_eq!((t2, a2), (1, Absorbed::MergedPending));
        assert_eq!(q.pending_len(), 1);
        let job = q.pop().unwrap();
        assert_eq!(
            job.urgency,
            Urgency::HighlightedPreview,
            "merge raised the urgency"
        );
        let (t3, a3) = q.enqueue(JobKind::Preview, "a", Urgency::VisiblePreview, false, || 3);
        assert_eq!((t3, a3), (1, Absorbed::AlreadyRunning));
        assert_eq!(q.pending_len(), 0);
        // A different kind for the same song is a different job.
        let (_, a4) = q.enqueue(JobKind::Jacket, "a", Urgency::VisibleJacket, false, || 4);
        assert_eq!(a4, Absorbed::Queued);
        assert!(q.contains(JobKind::Preview, "a"));
        assert!(q.contains(JobKind::Jacket, "a"));
        assert!(!q.contains(JobKind::FullBank, "a"));
    }

    #[test]
    fn rehighlight_promotes_one_song_and_demotes_the_rest() {
        let mut q = q();
        let mut n = 0u32;
        let mut mk = || {
            n += 1;
            n
        };
        q.enqueue(
            JobKind::Preview,
            "a",
            Urgency::HighlightedPreview,
            false,
            &mut mk,
        );
        q.enqueue(
            JobKind::Jacket,
            "a",
            Urgency::HighlightedJacket,
            false,
            &mut mk,
        );
        q.enqueue(
            JobKind::Preview,
            "b",
            Urgency::VisiblePreview,
            false,
            &mut mk,
        );
        q.enqueue(JobKind::Jacket, "b", Urgency::VisibleJacket, false, &mut mk);
        q.enqueue(
            JobKind::FullBank,
            "a",
            Urgency::SpeculativeFullBank,
            true,
            &mut mk,
        );
        q.rehighlight("b");
        assert_eq!(
            q.pending_order(),
            vec![
                (JobKind::Preview, "b".into()),
                (JobKind::Preview, "a".into()),
                (JobKind::Jacket, "b".into()),
                (JobKind::Jacket, "a".into()),
                (JobKind::FullBank, "a".into()),
            ]
        );
    }

    #[test]
    fn cancel_speculative_only_touches_speculative_full_banks() {
        let mut q = q();
        q.enqueue(
            JobKind::FullBank,
            "a",
            Urgency::SpeculativeFullBank,
            true,
            || 1,
        );
        q.enqueue(JobKind::Preview, "a", Urgency::VisiblePreview, false, || 2);
        assert_eq!(q.cancel_speculative("a"), Cancelled::Dropped(1));
        assert_eq!(q.cancel_speculative("a"), Cancelled::None);
        assert_eq!(q.pending_len(), 1, "the preview stays");

        // Running speculative → reported, owner flags the payload.
        q.enqueue(
            JobKind::FullBank,
            "b",
            Urgency::SpeculativeFullBank,
            true,
            || 3,
        );
        let job = q.pop().unwrap();
        assert_eq!(
            job.kind,
            JobKind::Preview,
            "the preview outranks the speculative bank"
        );
        let job = q.pop().unwrap();
        assert_eq!((job.kind, job.payload), (JobKind::FullBank, 3));
        assert_eq!(q.cancel_speculative("b"), Cancelled::Running(3));

        // A gameplay-load request over a speculative job de-speculates it.
        q.enqueue(
            JobKind::FullBank,
            "c",
            Urgency::SpeculativeFullBank,
            true,
            || 4,
        );
        let (t, a) = q.enqueue(JobKind::FullBank, "c", Urgency::GameplayLoad, false, || 5);
        assert_eq!((t, a), (4, Absorbed::MergedPending));
        assert_eq!(q.cancel_speculative("c"), Cancelled::None);
        let job = q.pop().unwrap();
        assert_eq!(job.urgency, Urgency::GameplayLoad);
        assert!(!job.speculative);
        // Same for a running one.
        q.enqueue(
            JobKind::FullBank,
            "d",
            Urgency::SpeculativeFullBank,
            true,
            || 6,
        );
        let _ = q.pop();
        let (_, a) = q.enqueue(JobKind::FullBank, "d", Urgency::GameplayLoad, false, || 7);
        assert_eq!(a, Absorbed::AlreadyRunning);
        assert_eq!(q.cancel_speculative("d"), Cancelled::None);
    }

    #[test]
    fn pool_size_is_at_least_three() {
        assert_eq!(pool_size(1), 3);
        assert_eq!(pool_size(4), 3);
        assert_eq!(pool_size(6), 3);
        assert_eq!(pool_size(8), 4);
        assert_eq!(pool_size(16), 8);
    }
}
