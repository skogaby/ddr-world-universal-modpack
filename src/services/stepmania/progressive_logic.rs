//! Pure decisions behind the progressive-bank read hook (design §4.8 item 3,
//! §5.6): whether a read must wait for the writer, how the per-fd cursor is
//! tracked, and when the wait gives up.
//!
//! The model is a file pre-sized to `planned_len` whose bytes become final
//! in two pieces: a contiguous prefix `[0, watermark)` (header, then the
//! main entry's blocks in file order, then the alignment gap) and —
//! optionally — a *tail* `[tail_start, planned_len)` written early (the
//! preview entry, which XACT primes at bank prepare). A read of
//! `[cursor, cursor + n)` is served without waiting when the writer is
//! `done`, when its end is under the watermark, or when it lies wholly in a
//! ready tail. Anything else waits — bounded; past the bound the read
//! passes through (zeros from the pre-sized file = brief silence rather
//! than a hang).
//!
//! `lseek` is NOT hooked: the cursor is advanced by what each read returned.
//! That matches the one consumer this exists for — the FileManager's single
//! I/O worker reads the XWB sequentially in 1 MiB chunks (cabinet fact E2)
//! — and is why the table is keyed by fd and reset on re-open.
//!
//! Harness-mounted: no `crate::` paths.

/// What the hook knows about the writer's progress at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub planned_len: u64,
    /// Final bytes from offset 0.
    pub watermark: u64,
    /// Offset the early-written tail starts at (`planned_len` = no tail).
    pub tail_start: u64,
    pub tail_ready: bool,
    pub done: bool,
}

impl Progress {
    /// The range `[cursor, cursor + nbytes)`, clamped to the file, is final.
    #[must_use]
    pub fn covers(&self, cursor: u64, nbytes: usize) -> bool {
        if self.done {
            return true;
        }
        let end = cursor.saturating_add(nbytes as u64).min(self.planned_len);
        if end <= self.watermark {
            return true;
        }
        // `end` is clamped to the file, so a read starting inside a ready
        // tail lies wholly within it.
        self.tail_ready && cursor >= self.tail_start
    }
}

/// `true` when a read of `nbytes` at `cursor` must wait for the writer.
#[must_use]
pub fn should_wait(cursor: u64, nbytes: usize, progress: &Progress) -> bool {
    if nbytes == 0 {
        return false;
    }
    !progress.covers(cursor, nbytes)
}

/// One step of the wait loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    /// Serve the read now.
    Ready,
    /// Sleep and poll again.
    Wait,
    /// The bound passed: pass the read through (and WARN once).
    TimedOut,
}

/// Decide the next action given the elapsed wait so far.
#[must_use]
pub fn poll(
    cursor: u64,
    nbytes: usize,
    progress: &Progress,
    waited_ms: u64,
    bound_ms: u64,
) -> Poll {
    if !should_wait(cursor, nbytes, progress) {
        Poll::Ready
    } else if waited_ms >= bound_ms {
        Poll::TimedOut
    } else {
        Poll::Wait
    }
}

/// Per-fd read position and wait statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FdCursor {
    pub pos: u64,
    pub waits: u32,
    pub waited_ms: u64,
    pub timed_out: bool,
}

impl FdCursor {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pos: 0,
            waits: 0,
            waited_ms: 0,
            timed_out: false,
        }
    }

    /// Advance by what the original read returned. AVS returns the byte
    /// count as `usize`; implausibly large values (error codes cast through)
    /// are treated as zero so a failed read never teleports the cursor.
    pub fn advance(&mut self, returned: usize, requested: usize) {
        if returned <= requested {
            self.pos = self.pos.saturating_add(returned as u64);
        }
    }

    /// Record one wait that ended after `ms`.
    pub fn note_wait(&mut self, ms: u64, timed_out: bool) {
        self.waits = self.waits.saturating_add(1);
        self.waited_ms = self.waited_ms.saturating_add(ms);
        self.timed_out |= timed_out;
    }

    /// The reader has consumed the whole planned file.
    #[must_use]
    pub fn reached_end(&self, planned_len: u64) -> bool {
        self.pos >= planned_len
    }
}

/// Whether an fd-table entry can be dropped: the writer is done and the
/// reader has consumed the whole file (nothing left to wait for, nothing
/// left to track).
#[must_use]
pub fn entry_finished(cursor: &FdCursor, progress: &Progress) -> bool {
    progress.done && cursor.reached_end(progress.planned_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1 << 20;

    fn p(watermark: u64, done: bool) -> Progress {
        Progress {
            planned_len: 10 * MIB as u64,
            watermark,
            tail_start: 9 * MIB as u64,
            tail_ready: false,
            done,
        }
    }

    #[test]
    fn waits_until_the_watermark_covers_the_read_end() {
        let pr = p(3 * MIB as u64, false);
        assert!(!should_wait(0, MIB, &pr));
        assert!(
            !should_wait(2 * MIB as u64, MIB, &pr),
            "end == watermark is covered"
        );
        assert!(should_wait(2 * MIB as u64 + 1, MIB, &pr));
        assert!(should_wait(3 * MIB as u64, MIB, &pr));
        assert!(
            !should_wait(3 * MIB as u64, 0, &pr),
            "zero-length reads never wait"
        );
    }

    #[test]
    fn done_serves_everything_and_the_end_is_clamped() {
        let pr = p(0, true);
        assert!(!should_wait(0, MIB, &pr));
        assert!(!should_wait(9 * MIB as u64 + 512, 4 * MIB, &pr));
        // Not done: a read straddling EOF waits for the watermark to reach EOF.
        let pr = Progress {
            watermark: 10 * MIB as u64 - 1,
            ..p(0, false)
        };
        assert!(should_wait(9 * MIB as u64 + 512, 4 * MIB, &pr));
        let pr = Progress {
            watermark: 10 * MIB as u64,
            ..pr
        };
        assert!(!should_wait(9 * MIB as u64 + 512, 4 * MIB, &pr));
    }

    #[test]
    fn a_ready_tail_serves_reads_inside_it_only() {
        let pr = Progress {
            tail_ready: true,
            ..p(2048, false)
        };
        // Entirely within the tail → served even though the watermark is low.
        assert!(!should_wait(9 * MIB as u64, 64 * 1024, &pr));
        assert!(!should_wait(9 * MIB as u64 + MIB as u64 - 1, 1, &pr));
        // Starts before the tail → waits (the main entry is not there yet).
        assert!(should_wait(9 * MIB as u64 - 1, 64 * 1024, &pr));
        assert!(should_wait(2048, MIB, &pr));
        // Tail not ready → waits.
        let pr = Progress {
            tail_ready: false,
            ..pr
        };
        assert!(should_wait(9 * MIB as u64, 64 * 1024, &pr));
    }

    #[test]
    fn poll_sequence_with_a_fake_clock() {
        let mut pr = p(MIB as u64, false);
        let cursor = MIB as u64;
        let mut clock = 0u64;
        let mut steps = Vec::new();
        loop {
            let r = poll(cursor, MIB, &pr, clock, 5000);
            steps.push(r);
            match r {
                Poll::Wait => {
                    clock += 2;
                    if clock == 40 {
                        pr.watermark = 2 * MIB as u64; // writer catches up at 40 ms
                    }
                }
                _ => break,
            }
        }
        assert_eq!(steps.len(), 21, "20 waits of 2 ms, then ready");
        assert_eq!(steps.last(), Some(&Poll::Ready));

        // Never catches up → times out at the bound.
        let pr = p(MIB as u64, false);
        let mut clock = 0u64;
        let mut n = 0;
        let last = loop {
            let r = poll(cursor, MIB, &pr, clock, 5000);
            if r != Poll::Wait {
                break r;
            }
            n += 1;
            clock += 5;
        };
        assert_eq!(last, Poll::TimedOut);
        assert_eq!(n, 1000);
    }

    #[test]
    fn cursor_tracks_sequential_reads_and_ignores_error_returns() {
        let mut c = FdCursor::new();
        c.advance(MIB, MIB);
        c.advance(MIB, MIB);
        c.advance(512, MIB); // short final read
        assert_eq!(c.pos, 2 * MIB as u64 + 512);
        c.advance(usize::MAX, MIB); // -1 cast through usize
        assert_eq!(c.pos, 2 * MIB as u64 + 512);
        c.advance(0, MIB);
        assert_eq!(c.pos, 2 * MIB as u64 + 512);
        assert!(!c.reached_end(3 * MIB as u64));
        c.advance(MIB - 512, MIB);
        assert!(c.reached_end(3 * MIB as u64));

        c.note_wait(12, false);
        c.note_wait(30, true);
        assert_eq!((c.waits, c.waited_ms, c.timed_out), (2, 42, true));
    }

    #[test]
    fn entry_is_finished_only_when_done_and_fully_read() {
        let mut c = FdCursor::new();
        let planned = 3 * MIB as u64;
        let not_done = Progress {
            planned_len: planned,
            watermark: planned,
            tail_start: planned,
            tail_ready: false,
            done: false,
        };
        let done = Progress {
            done: true,
            ..not_done
        };
        c.advance(3 * MIB, 3 * MIB);
        assert!(!entry_finished(&c, &not_done));
        assert!(entry_finished(&c, &done));
        let c = FdCursor::new();
        assert!(!entry_finished(&c, &done));
    }
}
