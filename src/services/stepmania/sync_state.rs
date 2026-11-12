//! `sync.bin` — the auto-sync ledger — plus the pure decision rules the
//! runtime applies on top of it (`sync_apply.rs`). `std` only (no `crate::`
//! imports): harness-mounted by `scripts/validate_stepmania.sh`.
//!
//! ## The ledger: one record per SONG
//!
//! Keyed by the song's basename (`zab12c`):
//!
//! ```text
//! source_fingerprint  u64  SongEntry.fingerprint the measurement belongs to
//! state               Measured { delta_ms: i8 }   judgement-offset delta the song
//!                                                 plays with (0 = measured in sync)
//!                   | Refused { reason }          estimator refusal key, or a
//!                                                 measurer-side reason (decode_failed …)
//! ```
//!
//! Nothing on disk changes after indexing: the delta is applied at play time
//! through the game's JUDGEMENT OFFSET (`per_song_judgement_offsets`), with
//! `timing_music` positive = judged later, so `delta_ms = +correction_ms`
//! ("move the chart later by this much"). A song is measured once per
//! fingerprint; a record whose fingerprint no longer matches the index is
//! stale and dropped at load.
//!
//! Layout (little-endian): `b"DDRSMSY2"`, `u32` [`SYNC_VERSION`], `u32`
//! record count, then per record: `u8` name length + ASCII basename, `u64`
//! fingerprint, `u8` kind (0 = Measured, 1 = Refused), Measured: `i8`
//! delta / Refused: `u8` length + ASCII reason. Trailing bytes, a bad
//! magic/version, an over-long or non-`[a-z0-9_.]` string, an unknown kind
//! or a duplicate key make the whole file malformed, which callers treat as
//! empty (everything is re-measurable).
//!
//! ## Per-song decision ([`decide_song`])
//!
//! A song with several split files (`<b>_N.ssq`) is measured per file but
//! stored as ONE delta — the `<b>.ssq` file's, or the lowest `_N` when there
//! is no base file. Files whose deltas differ by more than [`DISAGREE_MS`]
//! set `disagree` (the caller WARNs once, naming the song).
//!
//! ## The live map and the freeze rule ([`LiveMap`])
//!
//! What a play actually uses is the live value, not the ledger: `basename →
//! {delta, frozen}`. A `publish` (any measurement) changes an UNFROZEN
//! entry; the first play decision `freeze`s the entry with whatever it has
//! (`Some(d)` or `None` = as-authored) and from then on every side and every
//! later play in the session gets that value — a late measurement only
//! reaches the ledger and applies from the next boot. Two plays of one song
//! in one session therefore never have different sync.

use std::collections::{BTreeMap, HashMap};

pub const SYNC_MAGIC: [u8; 8] = *b"DDRSMSY2";
pub const SYNC_VERSION: u32 = 2;
/// Longest basename / reason stored.
pub const MAX_NAME: usize = 64;
pub const MAX_REASON: usize = 32;
/// Corrupt-count guard.
pub const MAX_RECORDS: u32 = 1_000_000;
/// Split files whose deltas differ by more than this are flagged.
pub const DISAGREE_MS: i32 = 2;

/// What the measurement decided for one song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    /// Play with the judgement offset moved by `delta_ms` (positive = judged
    /// later); `0` = measured and already in sync.
    Measured { delta_ms: i8 },
    /// Play as authored; `reason` is a stable key.
    Refused { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRecord {
    pub source_fingerprint: u64,
    pub state: SyncState,
}

impl SyncRecord {
    /// The value the live map takes from this record: `Some(delta)` for a
    /// measurement, `None` (as authored) for a refusal.
    #[must_use]
    pub fn live_delta(&self) -> Option<i8> {
        match self.state {
            SyncState::Measured { delta_ms } => Some(delta_ms),
            SyncState::Refused { .. } => None,
        }
    }
}

/// The whole ledger, ordered by key so serialisation is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncTable {
    records: BTreeMap<String, SyncRecord>,
}

fn valid_text(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

impl SyncTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    #[must_use]
    pub fn get(&self, basename: &str) -> Option<&SyncRecord> {
        self.records.get(basename)
    }

    /// The record for `basename` only when it belongs to `fingerprint`.
    #[must_use]
    pub fn current(&self, basename: &str, fingerprint: u64) -> Option<&SyncRecord> {
        self.records
            .get(basename)
            .filter(|r| r.source_fingerprint == fingerprint)
    }

    /// Insert or replace. `false` (nothing stored) for a key or reason the
    /// format cannot carry.
    pub fn insert(&mut self, basename: &str, record: SyncRecord) -> bool {
        if !valid_text(basename, MAX_NAME) {
            return false;
        }
        if let SyncState::Refused { reason } = &record.state {
            if !valid_text(reason, MAX_REASON) {
                return false;
            }
        }
        self.records.insert(basename.to_string(), record);
        true
    }

    pub fn remove(&mut self, basename: &str) -> Option<SyncRecord> {
        self.records.remove(basename)
    }

    /// Keep only the records `keep(basename, record)` accepts; returns how
    /// many were dropped.
    pub fn retain(&mut self, mut keep: impl FnMut(&str, &SyncRecord) -> bool) -> usize {
        let before = self.records.len();
        self.records.retain(|k, r| keep(k, r));
        before - self.records.len()
    }

    /// Drop every record whose basename is not in `fingerprints` or whose
    /// fingerprint differs from it (song changed or removed); returns how
    /// many were dropped.
    pub fn drop_stale(&mut self, fingerprints: &HashMap<String, u64>) -> usize {
        self.retain(|b, r| fingerprints.get(b) == Some(&r.source_fingerprint))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &SyncRecord)> {
        self.records.iter().map(|(k, r)| (k.as_str(), r))
    }

    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.records.len() * 24);
        out.extend_from_slice(&SYNC_MAGIC);
        out.extend_from_slice(&SYNC_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        for (name, r) in &self.records {
            out.push(name.len() as u8);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&r.source_fingerprint.to_le_bytes());
            match &r.state {
                SyncState::Measured { delta_ms } => {
                    out.push(0);
                    out.push(*delta_ms as u8);
                }
                SyncState::Refused { reason } => {
                    out.push(1);
                    out.push(reason.len() as u8);
                    out.extend_from_slice(reason.as_bytes());
                }
            }
        }
        out
    }

    /// Parse; `None` on any malformation (see the module docs).
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut c = Cursor {
            data: bytes,
            pos: 0,
        };
        if c.take(8)? != SYNC_MAGIC {
            return None;
        }
        if c.u32()? != SYNC_VERSION {
            return None;
        }
        let n = c.u32()?;
        if n > MAX_RECORDS {
            return None;
        }
        let mut records = BTreeMap::new();
        for _ in 0..n {
            let name = c.text(MAX_NAME)?;
            let source_fingerprint = c.u64()?;
            let state = match c.u8()? {
                0 => SyncState::Measured {
                    delta_ms: c.u8()? as i8,
                },
                1 => SyncState::Refused {
                    reason: c.text(MAX_REASON)?,
                },
                _ => return None,
            };
            let record = SyncRecord {
                source_fingerprint,
                state,
            };
            if records.insert(name, record).is_some() {
                return None;
            }
        }
        if c.pos != bytes.len() {
            return None;
        }
        Some(Self { records })
    }

    /// [`Self::parse`] with "absent or malformed ⇒ empty".
    #[must_use]
    pub fn parse_or_empty(bytes: Option<&[u8]>) -> Self {
        bytes.and_then(Self::parse).unwrap_or_default()
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        <[u8; N]>::try_from(self.take(N)?).ok()
    }
    fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.array::<4>().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.array::<8>().map(u64::from_le_bytes)
    }
    fn text(&mut self, max: usize) -> Option<String> {
        let len = usize::from(self.u8()?);
        let s = std::str::from_utf8(self.take(len)?).ok()?;
        valid_text(s, max).then(|| s.to_string())
    }
}

// ---------------------------------------------------------------------
// Per-song decision over the per-file measurements
// ---------------------------------------------------------------------

/// One file's measurement reduced to what the ledger needs: `Ok(delta_ms)`
/// (`0` for an unchanged file) or `Err(reason)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResult {
    /// `0` for `<b>.ssq`, `N` for `<b>_N.ssq`.
    pub index: u8,
    pub outcome: Result<i32, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongDecision {
    pub state: SyncState,
    /// The file whose result was taken.
    pub picked: u8,
    /// Two measured files differ by more than [`DISAGREE_MS`].
    pub disagree: bool,
}

/// `delta_ms` as the ledger stores it (the estimator caps corrections far
/// below this; the clamp only guards a caller bug).
#[must_use]
pub fn delta_to_i8(delta_ms: i32) -> i8 {
    delta_ms.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8
}

/// The song's record from its files' results (module docs); `None` for no
/// files.
#[must_use]
pub fn decide_song(files: &[FileResult]) -> Option<SongDecision> {
    let pick = files
        .iter()
        .find(|f| f.index == 0)
        .or_else(|| files.iter().min_by_key(|f| f.index))?;
    let state = match &pick.outcome {
        Ok(d) => SyncState::Measured {
            delta_ms: delta_to_i8(*d),
        },
        Err(reason) => SyncState::Refused {
            reason: reason.clone(),
        },
    };
    let measured: Vec<i32> = files
        .iter()
        .filter_map(|f| f.outcome.as_ref().ok().copied())
        .collect();
    let disagree = measured
        .iter()
        .any(|a| measured.iter().any(|b| (a - b).abs() > DISAGREE_MS));
    Some(SongDecision {
        state,
        picked: pick.index,
        disagree,
    })
}

// ---------------------------------------------------------------------
// Live map + freeze rule
// ---------------------------------------------------------------------

/// What a play uses for one song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveEntry {
    /// `Some(delta)` = measured, `None` = as authored (refused / unmeasured
    /// at the freeze).
    pub delta: Option<i8>,
    /// Decided for this session: publishes no longer change `delta`.
    pub frozen: bool,
}

/// What a [`LiveMap::publish`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publish {
    /// The live value changed (unfrozen entry, or a new one).
    Applied,
    /// The entry was frozen: the live value is unchanged, the ledger alone
    /// carries the measurement (applies from the next boot).
    Late,
}

/// `basename → LiveEntry`; an ABSENT basename is "not measured yet".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LiveMap {
    entries: HashMap<String, LiveEntry>,
}

impl LiveMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn get(&self, basename: &str) -> Option<LiveEntry> {
        self.entries.get(basename).copied()
    }

    /// Measured (or frozen as authored): nothing to wait for.
    #[must_use]
    pub fn is_measured(&self, basename: &str) -> bool {
        self.entries.contains_key(basename)
    }

    #[must_use]
    pub fn is_frozen(&self, basename: &str) -> bool {
        self.entries.get(basename).is_some_and(|e| e.frozen)
    }

    /// A ledger record at load: inserted unfrozen when the basename is
    /// absent; `false` (nothing changed) when an entry already exists.
    pub fn seed(&mut self, basename: &str, delta: Option<i8>) -> bool {
        if self.entries.contains_key(basename) {
            return false;
        }
        self.entries.insert(
            basename.to_string(),
            LiveEntry {
                delta,
                frozen: false,
            },
        );
        true
    }

    /// A measurement result: changes the live value unless the entry is
    /// frozen (then [`Publish::Late`], value untouched).
    pub fn publish(&mut self, basename: &str, delta: Option<i8>) -> Publish {
        match self.entries.get_mut(basename) {
            Some(e) if e.frozen => Publish::Late,
            Some(e) => {
                e.delta = delta;
                Publish::Applied
            }
            None => {
                self.entries.insert(
                    basename.to_string(),
                    LiveEntry {
                        delta,
                        frozen: false,
                    },
                );
                Publish::Applied
            }
        }
    }

    /// The play decision: freeze the entry with whatever it has (an absent
    /// basename freezes as `None` = as authored) and return that value.
    pub fn freeze(&mut self, basename: &str) -> Option<i8> {
        let e = self
            .entries
            .entry(basename.to_string())
            .or_insert(LiveEntry {
                delta: None,
                frozen: false,
            });
        e.frozen = true;
        e.delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(fp: u64, d: i8) -> SyncRecord {
        SyncRecord {
            source_fingerprint: fp,
            state: SyncState::Measured { delta_ms: d },
        }
    }

    fn refused(fp: u64, why: &str) -> SyncRecord {
        SyncRecord {
            source_fingerprint: fp,
            state: SyncState::Refused {
                reason: why.to_string(),
            },
        }
    }

    fn table() -> SyncTable {
        let mut t = SyncTable::new();
        assert!(t.insert("zab12c", measured(7, 9)));
        assert!(t.insert("z00001", refused(8, "ambiguous_peak")));
        assert!(t.insert("z00002", measured(9, 0)));
        assert!(t.insert("z00003", measured(10, -60)));
        t
    }

    #[test]
    fn round_trips_deterministically() {
        let t = table();
        let bytes = t.serialize();
        assert_eq!(SyncTable::parse(&bytes), Some(t.clone()));
        assert_eq!(&bytes[..8], b"DDRSMSY2");
        // Insertion order does not matter.
        let mut u = SyncTable::new();
        for (k, r) in t.iter().collect::<Vec<_>>().into_iter().rev() {
            u.insert(k, r.clone());
        }
        assert_eq!(u.serialize(), bytes);
        assert_eq!(SyncTable::new().serialize().len(), 16);
        assert_eq!(
            SyncTable::parse(&SyncTable::new().serialize()),
            Some(SyncTable::new())
        );
        // Negative deltas survive the byte cast.
        assert_eq!(t.get("z00003").map(SyncRecord::live_delta), Some(Some(-60)));
        assert_eq!(t.get("z00001").map(SyncRecord::live_delta), Some(None));
    }

    #[test]
    fn malformed_input_is_empty() {
        let bytes = table().serialize();
        assert!(SyncTable::parse(&[]).is_none());
        for cut in [1, 8, 12, 15, bytes.len() - 1] {
            assert!(SyncTable::parse(&bytes[..cut]).is_none(), "cut {cut}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(SyncTable::parse(&extra).is_none());
        let mut magic = bytes.clone();
        magic[0] ^= 1;
        assert!(SyncTable::parse(&magic).is_none());
        let mut version = bytes.clone();
        version[8] = 1;
        assert!(SyncTable::parse(&version).is_none());
        // Unknown state kind: first record "z00001" (1 + 6) + fp (8) → kind.
        let mut kind = bytes.clone();
        kind[16 + 7 + 8] = 7;
        assert!(SyncTable::parse(&kind).is_none());
        // Bad key characters.
        let mut key = bytes.clone();
        key[17] = b'Z';
        assert!(SyncTable::parse(&key).is_none());
        // Absurd count.
        let mut count = bytes.clone();
        count[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(SyncTable::parse(&count).is_none());
        assert_eq!(SyncTable::parse_or_empty(Some(&extra)), SyncTable::new());
        assert_eq!(SyncTable::parse_or_empty(None), SyncTable::new());
        assert_eq!(SyncTable::parse_or_empty(Some(&bytes)), table());
    }

    #[test]
    fn insert_rejects_unstorable_text() {
        let mut t = SyncTable::new();
        assert!(!t.insert("Upper", measured(1, 1)));
        assert!(!t.insert("", measured(1, 1)));
        assert!(!t.insert(&"a".repeat(65), measured(1, 1)));
        assert!(!t.insert("z", refused(1, "has space")));
        assert!(!t.insert("z", refused(1, &"r".repeat(33))));
        assert!(t.is_empty());
    }

    #[test]
    fn current_and_stale_drop_respect_the_fingerprint() {
        let t = table();
        assert!(t.current("zab12c", 7).is_some());
        assert!(t.current("zab12c", 8).is_none(), "other inputs");
        assert!(t.current("nope", 7).is_none());
        let mut t = t;
        let fingerprints: HashMap<String, u64> = [
            ("zab12c".to_string(), 7u64), // kept
            ("z00001".to_string(), 99),   // song changed → dropped
            // z00002 removed from the index → dropped
            ("z00003".to_string(), 10), // kept
        ]
        .into_iter()
        .collect();
        assert_eq!(t.drop_stale(&fingerprints), 2);
        assert_eq!(t.len(), 2);
        assert!(t.get("zab12c").is_some() && t.get("z00003").is_some());
    }

    fn ok(index: u8, d: i32) -> FileResult {
        FileResult {
            index,
            outcome: Ok(d),
        }
    }

    fn err(index: u8, why: &str) -> FileResult {
        FileResult {
            index,
            outcome: Err(why.to_string()),
        }
    }

    #[test]
    fn decide_song_picks_the_base_file_else_the_lowest_split() {
        assert_eq!(decide_song(&[]), None);
        let one = decide_song(&[ok(0, 9)]).unwrap();
        assert_eq!(one.state, SyncState::Measured { delta_ms: 9 });
        assert_eq!(one.picked, 0);
        assert!(!one.disagree);
        // Split files only: lowest N wins; agreement within 2 ms.
        let split = decide_song(&[ok(4, -3), ok(2, -1), ok(5, -2)]).unwrap();
        assert_eq!(split.state, SyncState::Measured { delta_ms: -1 });
        assert_eq!(split.picked, 2);
        assert!(!split.disagree);
        // Base present but listed last: still the base.
        let base = decide_song(&[ok(3, 12), ok(0, 10)]).unwrap();
        assert_eq!(base.state, SyncState::Measured { delta_ms: 10 });
        assert_eq!(base.picked, 0);
        assert!(!base.disagree, "2 ms apart is agreement");
        // Disagreement > 2 ms is flagged, the pick unchanged.
        let far = decide_song(&[ok(0, 10), ok(3, 13)]).unwrap();
        assert_eq!(far.state, SyncState::Measured { delta_ms: 10 });
        assert!(far.disagree);
        // The picked file refused → the song is refused (other files ignored).
        let refused_pick = decide_song(&[err(0, "too_few_events"), ok(3, 5)]).unwrap();
        assert_eq!(
            refused_pick.state,
            SyncState::Refused {
                reason: "too_few_events".into()
            }
        );
        assert!(!refused_pick.disagree);
        // Another file refused does not refuse the song.
        let other_refused = decide_song(&[ok(0, 5), err(3, "no_audio")]).unwrap();
        assert_eq!(other_refused.state, SyncState::Measured { delta_ms: 5 });
        // Out-of-range deltas are clamped into the ledger's i8.
        assert_eq!(delta_to_i8(200), 127);
        assert_eq!(delta_to_i8(-200), -128);
        assert_eq!(delta_to_i8(-60), -60);
    }

    #[test]
    fn freeze_rule() {
        let mut m = LiveMap::new();
        assert!(!m.is_measured("a"));
        // Seed from the ledger: unfrozen; a second seed is a no-op.
        assert!(m.seed("a", Some(9)));
        assert!(!m.seed("a", Some(1)));
        assert_eq!(
            m.get("a"),
            Some(LiveEntry {
                delta: Some(9),
                frozen: false
            })
        );
        // Publish before the freeze changes the value.
        assert_eq!(m.publish("a", Some(11)), Publish::Applied);
        assert_eq!(m.get("a").and_then(|e| e.delta), Some(11));
        // The play decision freezes it; later publishes are late.
        assert_eq!(m.freeze("a"), Some(11));
        assert!(m.is_frozen("a"));
        assert_eq!(m.publish("a", Some(-4)), Publish::Late);
        assert_eq!(m.freeze("a"), Some(11), "second side / replay: same value");
        assert_eq!(m.get("a").and_then(|e| e.delta), Some(11));
        // Unmeasured at the decision: frozen as authored (None), stays so.
        assert_eq!(m.freeze("b"), None);
        assert!(m.is_measured("b") && m.is_frozen("b"));
        assert_eq!(m.publish("b", Some(7)), Publish::Late);
        assert_eq!(m.freeze("b"), None);
        // A refusal publishes None and freezes as None.
        assert_eq!(m.publish("c", None), Publish::Applied);
        assert!(m.is_measured("c"));
        assert_eq!(m.freeze("c"), None);
        // Publishing a brand-new basename creates an unfrozen entry.
        assert_eq!(m.publish("d", Some(3)), Publish::Applied);
        assert_eq!(
            m.get("d"),
            Some(LiveEntry {
                delta: Some(3),
                frozen: false
            })
        );
        assert_eq!(m.len(), 4);
    }
}
