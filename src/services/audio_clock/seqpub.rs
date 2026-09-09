//! Lock-free single-writer publication of `N` 64-bit words — the codebase's
//! seqlock idiom (per-word atomics inside an even/odd sequence bracket: no
//! non-atomic data races, no locks on the render thread). Pure; host-tested.

use std::sync::atomic::{AtomicU64, Ordering};

/// Lock-free single-writer publication of `N` 64-bit words (the codebase's
/// seqlock idiom: per-word atomics inside an even/odd sequence bracket — no
/// non-atomic data races, no locks on the render thread).
pub struct SeqPub<const N: usize> {
    seq: AtomicU64,
    words: [AtomicU64; N],
}

impl<const N: usize> SeqPub<N> {
    pub const fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
            words: [const { AtomicU64::new(0) }; N],
        }
    }

    /// Single writer only.
    pub fn write(&self, words: &[u64; N]) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Release);
        for (slot, value) in self.words.iter().zip(words.iter()) {
            slot.store(*value, Ordering::Relaxed);
        }
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Consistent snapshot (bounded spin against the tiny writer section).
    pub fn read(&self) -> [u64; N] {
        loop {
            let before = self.seq.load(Ordering::Acquire);
            if before & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let mut out = [0u64; N];
            for (dst, slot) in out.iter_mut().zip(self.words.iter()) {
                *dst = slot.load(Ordering::Relaxed);
            }
            if self.seq.load(Ordering::Acquire) == before {
                return out;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_words() {
        let p: SeqPub<3> = SeqPub::new();
        assert_eq!(p.read(), [0, 0, 0]);
        p.write(&[1, u64::MAX, 7]);
        assert_eq!(p.read(), [1, u64::MAX, 7]);
    }

    #[test]
    fn readers_never_observe_torn_writes() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        let p: Arc<SeqPub<4>> = Arc::new(SeqPub::new());
        let stop = Arc::new(AtomicBool::new(false));
        let readers: Vec<_> = (0..3)
            .map(|_| {
                let p = Arc::clone(&p);
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Acquire) {
                        let w = p.read();
                        // Every published word set is [k, k, k, k].
                        assert!(w.iter().all(|x| *x == w[0]), "torn read {w:?}");
                    }
                })
            })
            .collect();
        for k in 1..50_000u64 {
            p.write(&[k, k, k, k]);
        }
        stop.store(true, Ordering::Release);
        for r in readers {
            r.join().unwrap();
        }
    }
}
