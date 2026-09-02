//! Selected-song publication (Training Mode v1, Step 3 — design §4.6/R13):
//! on every slot-5 dance-bank create (armed or not, preview or gameplay —
//! research §8.2: the preview player loads the SAME XWB through the same
//! create path) the wavebank create detour publishes the song's identity
//! digest and its MAIN entry's audio length. The most recent publication
//! while at song select is the highlighted song; `training_mode::bounds`
//! consumes it for the select-time effective clamp of the SONG START
//! TIME row.
//!
//! Detour-legal by construction: the cell is three atomics behind a
//! seqlock (single writer — the create detour on the game thread), the
//! reader retries and rejects torn states, and a parse failure publishes
//! nothing (the previous publication stays — an acceptable staleness for
//! an upper-bound-only UI clamp; the chart-derived runtime clamp remains
//! authoritative at gameplay entry).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::binding::{dance_bank_song_code, song_code_digest};
use crate::core::xact::xwb;

/// One settled publication (design §5's `SelectedSongInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedSongInfo {
    /// `song_code_digest` of the bank's song code (nonzero by construction).
    pub code_digest: u64,
    /// The MAIN entry's duration in milliseconds (audio length — an UPPER
    /// bound for the section rows; audio ≥ chart content, research §8.3).
    pub audio_len_ms: u32,
    /// The seqlock generation this read observed (even, > 0). Monotonic —
    /// consumers can detect re-publication between reads.
    pub generation: u32,
}

/// The seqlock cell. Generation 0 = never published; odd = write in
/// progress (reader rejects); even > 0 = settled. Single-writer (the
/// create detour on the game thread) — the two-phase write plus the
/// reader's generation re-check make a torn read impossible.
pub struct SelectedSongCell {
    generation: AtomicU32,
    code_digest: AtomicU64,
    audio_len_ms: AtomicU32,
}

impl SelectedSongCell {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            generation: AtomicU32::new(0),
            code_digest: AtomicU64::new(0),
            audio_len_ms: AtomicU32::new(0),
        }
    }

    /// First write half: advance the generation to odd ("writing").
    /// Split from [`finish_write`](Self::finish_write) so the torn-read
    /// guard is host-testable; production code uses [`publish`](Self::publish).
    pub(crate) fn begin_write(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Second write half: store the fields and settle the generation even.
    pub(crate) fn finish_write(&self, code_digest: u64, audio_len_ms: u32) {
        self.code_digest.store(code_digest, Ordering::Release);
        self.audio_len_ms.store(audio_len_ms, Ordering::Release);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Publish one settled `{code_digest, audio_len_ms}` (detour-legal:
    /// four atomic operations, no locks, no allocation).
    pub fn publish(&self, code_digest: u64, audio_len_ms: u32) {
        self.begin_write();
        self.finish_write(code_digest, audio_len_ms);
    }

    /// Read the latest settled publication, or `None` when nothing has
    /// ever been published or a write is in flight. Bounded retries — the
    /// writer is another thread's detour; a persistent mid-write state
    /// (impossible for the two-phase writer, but the reader must not spin
    /// forever on a hypothesis) degrades to `None`.
    #[must_use]
    pub fn read(&self) -> Option<SelectedSongInfo> {
        for _ in 0..8 {
            let before = self.generation.load(Ordering::Acquire);
            if before == 0 {
                return None;
            }
            if before % 2 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let code_digest = self.code_digest.load(Ordering::Acquire);
            let audio_len_ms = self.audio_len_ms.load(Ordering::Acquire);
            if self.generation.load(Ordering::Acquire) == before {
                return Some(SelectedSongInfo {
                    code_digest,
                    audio_len_ms,
                    generation: before,
                });
            }
        }
        None
    }
}

impl Default for SelectedSongCell {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide cell the create detour publishes into.
static CELL: SelectedSongCell = SelectedSongCell::new();

/// The latest settled publication (the highlighted song while at song
/// select), or `None` before the first dance-bank create of the boot.
#[must_use]
pub fn selected_song() -> Option<SelectedSongInfo> {
    CELL.read()
}

/// Derive a publication from a bank's virtual path + resident bytes:
/// dance-bank paths only (the slot-5 banks — everything else returns
/// `None`), parsed through the strict song-bank profile. The audio length
/// is the MAIN entry's duration in ms on its own sample grid.
#[must_use]
pub fn publication_from_bank(path: &str, bytes: &[u8]) -> Option<(u64, u32)> {
    let code = dance_bank_song_code(path)?;
    let bank = xwb::parse_song_bank(bytes).ok()?;
    // The parser resolves the `<code>` main entry (by name, or by duration
    // for the nameless World-era banks) — the same rule `plan_virtual_bank`
    // uses.
    let entry = &bank.entries[bank.main_entry_index()];
    let rate = entry.format.sample_rate();
    if rate == 0 {
        return None;
    }
    let ms = u64::from(entry.duration) * 1_000 / u64::from(rate);
    Some((song_code_digest(&code), u32::try_from(ms).ok()?))
}

/// Whether state stamped for song `stamp` may apply to the song whose
/// publication digest is `fresh` (the training rows' / pre-shift's
/// song-coherence rule): a known-fresh digest requires a matching stamp
/// (or no stamp at all — state armed on a cabinet whose banks never
/// parse keeps the pre-digest behavior; the chart-derived runtime clamps
/// still protect); an unknown fresh side fails OPEN for the same reason.
/// Guards the fast-confirm race where a song is confirmed before its
/// wheel-settle publication landed: the stamp still names the PREVIOUS
/// song, the create-time publication names the new one, and the mismatch
/// declines the stale state.
#[must_use]
pub fn digests_coherent(stamp: u64, fresh: Option<u64>) -> bool {
    match fresh {
        Some(digest) => stamp == 0 || stamp == digest,
        None => true,
    }
}

/// Parse-and-publish composition for the create detour: a resolvable
/// dance bank publishes; anything else (non-dance path, parse failure)
/// leaves the previous publication in place. Returns the published
/// digest (the just-created bank's identity — the freshest possible
/// "current song" for same-call coherence checks), or `None` when
/// nothing was published.
pub fn publish_from_bank(path: &str, bytes: &[u8]) -> Option<u64> {
    let (digest, audio_len_ms) = publication_from_bank(path, bytes)?;
    CELL.publish(digest, audio_len_ms);
    Some(digest)
}
