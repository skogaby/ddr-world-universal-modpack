//! Pure per-side state for Per-Song Judgement Offsets (design §Data Models →
//! In-memory / Wire string; requirements 1, 6, 7).
//!
//! The [`Store`] holds a CSV-derived *baseline* plus two per-side *session*
//! maps. Merge rules (design requirement 7):
//!
//! - boot: session\[side\] = baseline column (via [`Store::load_baseline`]);
//! - server profile load **replaces** that side's session map
//!   ([`Store::apply_server_string`] — even with an empty string), the CSV
//!   baseline untouched;
//! - explicit options-menu edits update the session map
//!   ([`Store::set_entry`] / [`Store::clear_entry`]) — the caller mirrors the
//!   edit into the CSV;
//! - card-in resets a side back to baseline ([`Store::reset_to_baseline`])
//!   before any new server data is applied.
//!
//! Session maps may hold codes the local CSV has never seen (server data from
//! another cabinet): they round-trip on the wire and never touch the CSV
//! unless locally edited.
//!
//! Wire format: `code|offset|code|offset|...`, entries sorted by code,
//! offsets as decimal integers, empty map ⇄ empty string, capped at
//! [`MAX_ENTRIES`].
//!
//! Dependency-free on purpose (std only — the host-test harness
//! `scripts/validate_judgement_offsets.sh` mounts this file standalone): no
//! logging, no `unsafe`, no game APIs. Callers translate [`DecodeStats`]
//! into their own one-shot WARN.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::csv::{CsvDoc, OFFSET_MAX, OFFSET_MIN};

/// Soft cap on encoded/decoded entries (design D12): keeps the wire string
/// far below the server's 64 KiB TEXT column.
pub const MAX_ENTRIES: usize = 2000;

/// What [`Store::apply_server_string`] had to tolerate. The caller
/// aggregates this into a single WARN; the pure layer never logs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DecodeStats {
    /// Pairs dropped (non-integer offset, empty code, dangling trailing
    /// code).
    pub skipped: u32,
    /// Offsets pulled back into the ±100 domain.
    pub clamped: u32,
    /// True when the string carried more than [`MAX_ENTRIES`] entries.
    pub truncated: bool,
}

impl DecodeStats {
    /// True when the string decoded without any tolerance events.
    pub fn is_clean(&self) -> bool {
        self.skipped == 0 && self.clamped == 0 && !self.truncated
    }
}

/// Per-side offset state. Pure — see the module docs for the merge rules.
#[derive(Debug, Default)]
pub struct Store {
    baseline: HashMap<String, [Option<i8>; 2]>,
    session: [HashMap<String, i8>; 2],
    armed: bool,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once [`Store::load_baseline`] ran — before that, persistence
    /// consumers must treat the store as absent (omit the wire field).
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Install the CSV image as the baseline and reset both session maps to
    /// their baseline columns.
    pub fn load_baseline(&mut self, doc: &CsvDoc) {
        self.baseline.clear();
        for row in doc.rows() {
            self.baseline.insert(row.code.clone(), row.offsets);
        }
        self.reset_to_baseline(0);
        self.reset_to_baseline(1);
        self.armed = true;
    }

    /// Session map = baseline column (card-in reset; also part of
    /// [`Store::load_baseline`]).
    pub fn reset_to_baseline(&mut self, side: usize) {
        let side = side.min(1);
        let map: HashMap<String, i8> = self
            .baseline
            .iter()
            .filter_map(|(code, offsets)| offsets[side].map(|v| (code.clone(), v)))
            .collect();
        self.session[side] = map;
    }

    /// Replace the side's session map with the decoded server string —
    /// including replacing with an empty map when `s` is empty (that is how
    /// "player deleted all offsets" round-trips).
    pub fn apply_server_string(&mut self, side: usize, s: &str) -> DecodeStats {
        let side = side.min(1);
        let (map, stats) = decode_wire(s);
        self.session[side] = map;
        stats
    }

    /// Set one song's override for a side (options-menu edit).
    pub fn set_entry(&mut self, side: usize, code: &str, value: i8) {
        let side = side.min(1);
        let value = value.clamp(OFFSET_MIN, OFFSET_MAX);
        self.session[side].insert(code.to_string(), value);
    }

    /// Remove one song's override for a side (parent row toggled OFF).
    pub fn clear_entry(&mut self, side: usize, code: &str) {
        self.session[side.min(1)].remove(code);
    }

    /// The side's override for a song, if set.
    pub fn lookup(&self, side: usize, code: &str) -> Option<i8> {
        self.session[side.min(1)].get(code).copied()
    }

    /// Encode the side's session map for the wire: sorted by code, capped at
    /// [`MAX_ENTRIES`], deterministic. Codes containing `'|'` cannot occur
    /// (musicdb basenames) and are skipped defensively.
    pub fn encode_side(&self, side: usize) -> String {
        let mut entries: Vec<(&str, i8)> = self.session[side.min(1)]
            .iter()
            .filter(|(code, _)| !code.contains('|'))
            .map(|(code, v)| (code.as_str(), *v))
            .collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
        entries.truncate(MAX_ENTRIES);
        let mut out = String::with_capacity(entries.len() * 10);
        for (code, v) in entries {
            if !out.is_empty() {
                out.push('|');
            }
            out.push_str(code);
            out.push('|');
            out.push_str(&v.to_string());
        }
        out
    }

    /// Options-row seed values for a song: `(parent 0|1, child value)`.
    /// An entry (including value 0) seeds `(1, value)`; no entry seeds
    /// `(0, 0)`.
    pub fn row_seed(&self, side: usize, code: &str) -> (i32, i32) {
        match self.lookup(side, code) {
            Some(v) => (1, v as i32),
            None => (0, 0),
        }
    }

    /// Gameplay arm decision: `Some(offset)` only when the side entered, not
    /// course/event mode, the played song's code is known, and an entry
    /// exists for it.
    pub fn arm_decision(
        &self,
        side: usize,
        side_entered: bool,
        course_mode: bool,
        code: Option<&str>,
    ) -> Option<i8> {
        if !side_entered || course_mode {
            return None;
        }
        self.lookup(side, code?)
    }
}

/// Decode a wire string into a session map. See [`Store::apply_server_string`].
fn decode_wire(s: &str) -> (HashMap<String, i8>, DecodeStats) {
    let mut map = HashMap::new();
    let mut stats = DecodeStats::default();
    if s.is_empty() {
        return (map, stats);
    }
    let mut tokens = s.split('|');
    while let Some(code) = tokens.next() {
        let Some(offset_token) = tokens.next() else {
            // Dangling trailing code.
            stats.skipped += 1;
            break;
        };
        if code.is_empty() {
            stats.skipped += 1;
            continue;
        }
        let Ok(raw) = offset_token.trim().parse::<i64>() else {
            stats.skipped += 1;
            continue;
        };
        let clamped = raw.clamp(OFFSET_MIN as i64, OFFSET_MAX as i64);
        if clamped != raw {
            stats.clamped += 1;
        }
        if map.len() >= MAX_ENTRIES {
            stats.truncated = true;
            continue;
        }
        map.insert(code.to_string(), clamped as i8);
    }
    (map, stats)
}

/// The module-level store instance shared by the mod's UI, gameplay, and
/// persistence layers. Thin wrapper — all logic lives on [`Store`].
pub fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    let mut guard = STORE
        .get_or_init(|| Mutex::new(Store::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

#[cfg(test)]
mod tests {
    use super::super::csv;
    use super::*;

    fn store_with_baseline(text: &str) -> Store {
        let (doc, stats) = csv::parse(text);
        assert!(stats.is_clean());
        let mut store = Store::new();
        store.load_baseline(&doc);
        store
    }

    #[test]
    fn wire_round_trip() {
        let mut store = Store::new();
        store.set_entry(0, "puty", 11);
        store.set_entry(0, "neg", -100);
        store.set_entry(0, "zero", 0);
        let encoded = store.encode_side(0);
        assert_eq!(encoded, "neg|-100|puty|11|zero|0", "sorted + stable");
        assert_eq!(store.encode_side(0), encoded, "deterministic");

        let mut other = Store::new();
        let stats = other.apply_server_string(1, &encoded);
        assert!(stats.is_clean());
        for (code, v) in [("puty", 11), ("neg", -100), ("zero", 0)] {
            assert_eq!(other.lookup(1, code), Some(v));
        }
    }

    #[test]
    fn empty_string_semantics() {
        let mut store = Store::new();
        store.set_entry(0, "puty", 11);
        let stats = store.apply_server_string(0, "");
        assert!(stats.is_clean());
        assert_eq!(store.lookup(0, "puty"), None, "server-cleared");
        assert_eq!(store.encode_side(0), "");
    }

    #[test]
    fn malformed_wire_tolerance() {
        let mut store = Store::new();
        let stats = store.apply_server_string(0, "puty|11|bad|xx|aaaa|999|dangling");
        assert_eq!(store.lookup(0, "puty"), Some(11));
        assert_eq!(store.lookup(0, "bad"), None, "non-integer pair skipped");
        assert_eq!(store.lookup(0, "aaaa"), Some(100), "999 clamps to 100");
        assert_eq!(stats.clamped, 1);
        assert_eq!(stats.skipped, 2, "bad pair + dangling code");
        assert!(!stats.truncated);
    }

    #[test]
    fn merge_semantics() {
        // Baseline {A:5} on both sides.
        let mut store = store_with_baseline("A,5,5\n");
        assert_eq!(store.lookup(0, "A"), Some(5));
        assert_eq!(store.lookup(1, "A"), Some(5));

        // Edit adds B on side 0 only.
        store.set_entry(0, "B", -3);
        assert_eq!(store.lookup(0, "B"), Some(-3));
        assert_eq!(store.lookup(1, "B"), None);

        // Server load replaces side 0 entirely; side 1 untouched.
        store.apply_server_string(0, "C|3");
        assert_eq!(store.lookup(0, "A"), None);
        assert_eq!(store.lookup(0, "B"), None);
        assert_eq!(store.lookup(0, "C"), Some(3));
        assert_eq!(store.lookup(1, "A"), Some(5));

        // Card-in reset restores the baseline column.
        store.reset_to_baseline(0);
        assert_eq!(store.lookup(0, "A"), Some(5));
        assert_eq!(store.lookup(0, "C"), None);
    }

    #[test]
    fn baseline_sides_are_independent() {
        let store = store_with_baseline("solo,7,\nother,,-2\n");
        assert_eq!(store.lookup(0, "solo"), Some(7));
        assert_eq!(store.lookup(1, "solo"), None);
        assert_eq!(store.lookup(0, "other"), None);
        assert_eq!(store.lookup(1, "other"), Some(-2));
    }

    #[test]
    fn unknown_code_preservation() {
        let mut store = store_with_baseline("A,5,\n");
        store.apply_server_string(0, "A|5|othercab|9");
        let encoded = store.encode_side(0);
        assert_eq!(encoded, "A|5|othercab|9", "unknown code round-trips");
    }

    #[test]
    fn cap_enforcement() {
        let mut store = Store::new();
        for i in 0..(MAX_ENTRIES + 1) {
            store.set_entry(0, &format!("song{i:04}"), 1);
        }
        let encoded = store.encode_side(0);
        assert_eq!(encoded.split('|').count(), MAX_ENTRIES * 2);
        // song0000..song1999 survive; song2000 (sorted last) is dropped.
        assert!(!encoded.contains("song2000"));

        // Decode-side cap: build a 2001-entry string by hand.
        let mut big = String::new();
        for i in 0..(MAX_ENTRIES + 1) {
            if !big.is_empty() {
                big.push('|');
            }
            big.push_str(&format!("s{i:04}|1"));
        }
        let mut other = Store::new();
        let stats = other.apply_server_string(0, &big);
        assert!(stats.truncated);
        assert_eq!(other.lookup(0, "s0000"), Some(1));
        assert_eq!(other.lookup(0, "s2000"), None);
    }

    #[test]
    fn decision_helpers() {
        let mut store = Store::new();
        store.set_entry(0, "puty", 0);
        store.set_entry(0, "neg", -7);

        // row_seed: entry (incl. 0) => (1, v); none => (0, 0).
        assert_eq!(store.row_seed(0, "puty"), (1, 0));
        assert_eq!(store.row_seed(0, "neg"), (1, -7));
        assert_eq!(store.row_seed(0, "unset"), (0, 0));
        assert_eq!(store.row_seed(1, "puty"), (0, 0), "per-side");

        // arm_decision truth table.
        let cases: [(bool, bool, Option<&str>, Option<i8>); 6] = [
            (true, false, Some("neg"), Some(-7)),
            (true, false, Some("puty"), Some(0)),
            (true, false, Some("unset"), None),
            (true, false, None, None),
            (true, true, Some("neg"), None),
            (false, false, Some("neg"), None),
        ];
        for (entered, course, code, expected) in cases {
            assert_eq!(
                store.arm_decision(0, entered, course, code),
                expected,
                "entered={entered} course={course} code={code:?}"
            );
        }
    }

    #[test]
    fn armed_gate() {
        let mut store = Store::new();
        assert!(!store.is_armed());
        store.load_baseline(&csv::parse("").0);
        assert!(store.is_armed());
    }
}
