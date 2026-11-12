//! The custom-models SCAN INDEX — the PURE codec of
//! `data_mods/_cache/custom_models/scan.idx` (2026-10-04). The impure walker
//! (`custom_scan.rs`) loads it once per scan and rewrites it when anything it
//! records changed; this file only turns it into text and back, so the host
//! harness (`scripts/validate_background_dancers.sh`) can test every rule.
//!
//! Why it exists: under CrossOver every file open / stat is a wineserver round
//! trip (~0.4 ms), so per-file work dominated the enable-time scan (20k files
//! ⇒ ~10 s). The index lets a warm boot touch NOTHING but the directory
//! listings it needs anyway (whose FindNextFile records already carry each
//! file's size + mtime):
//!
//! - `P` — a packed MODEL FOLDER: cache arc file name → the folder's
//!   fingerprint (the `CacheHasher` digest the `.hashed` sidecar also holds)
//!   and the cache arc's size. Replaces one `.hashed` read and one `is_file`
//!   stat per model; the cache directory's single listing confirms the arc
//!   exists with that size.
//! - `A` — a ready `.arc` input: filesystem path → its listing stamp + member
//!   paths (replaces the 64 KiB header read while the stamp holds).
//! - `S` + `R` rows — a sidecar rlist: path → stamp + parsed rows (replaces
//!   the read + parse while the stamp holds).
//!
//! Format: UTF-8 text, the [`HEADER`] line, then one record per line, fields
//! separated by TAB and escaped (`\\`, `\t`, `\n`, `\r`). An unknown header
//! decodes as an EMPTY index (everything is re-validated the slow way once);
//! a malformed record is dropped on its own (with its `R` children). Stamps
//! are `(size, mtime in ns since the Unix epoch)`; `mtime_ns == 0` means
//! unknown and never matches.

use std::collections::BTreeMap;

/// First line of a valid index (bump the version on any grammar change).
pub const HEADER: &str = "ddr-world-hook custom_models scan index v1";

/// `(key, fields)` — the `core::anm::rlist::Row` shape, spelled locally so the
/// harness mount needs no `core` import.
pub type Row = (String, Vec<String>);

/// A file's identity as a directory listing reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stamp {
    pub size: u64,
    /// Modification time, ns since the Unix epoch; `0` = unknown.
    pub mtime_ns: u64,
}

impl Stamp {
    /// Whether a cached value recorded under `self` is still valid for a file
    /// the listing now reports as `now` (both known and identical).
    pub fn still(&self, now: Stamp) -> bool {
        self.mtime_ns != 0 && *self == now
    }
}

/// A packed model folder's cache record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packed {
    /// The folder fingerprint (MD5 over source, member paths, input mtimes).
    pub hash: [u8; 16],
    /// Size of the cache arc when it was written.
    pub arc_size: u64,
}

/// A value derived from one input file, valid while its stamp holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached<T> {
    pub stamp: Stamp,
    pub value: T,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanIndex {
    /// Cache arc file name (`pl_x00-1a2b3c4d.arc`) → its record.
    pub packed: BTreeMap<String, Packed>,
    /// Ready arc filesystem path → its member paths.
    pub arcs: BTreeMap<String, Cached<Vec<String>>>,
    /// Sidecar filesystem path → its rows.
    pub sidecars: BTreeMap<String, Cached<Vec<Row>>>,
}

impl ScanIndex {
    /// The cached member list of the ready arc at `path`, if its stamp holds.
    pub fn arc_members(&self, path: &str, now: Stamp) -> Option<&Vec<String>> {
        self.arcs
            .get(path)
            .filter(|c| c.stamp.still(now))
            .map(|c| &c.value)
    }

    /// The cached rows of the sidecar at `path`, if its stamp holds.
    pub fn sidecar_rows(&self, path: &str, now: Stamp) -> Option<&Vec<Row>> {
        self.sidecars
            .get(path)
            .filter(|c| c.stamp.still(now))
            .map(|c| &c.value)
    }

    /// Whether the packed record for `arc_name` vouches for a cache arc with
    /// fingerprint `hash` that the cache listing reports at `listed_size`.
    /// `None` = no record (the caller falls back to the `.hashed` sidecar).
    pub fn packed_fresh(&self, arc_name: &str, hash: [u8; 16], listed_size: u64) -> Option<bool> {
        self.packed
            .get(arc_name)
            .map(|p| p.hash == hash && p.arc_size == listed_size)
    }

    pub fn is_empty(&self) -> bool {
        self.packed.is_empty() && self.arcs.is_empty() && self.sidecars.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Field escaping
// ---------------------------------------------------------------------------

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// `None` on a dangling or unknown escape.
fn unescape(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            _ => return None,
        }
    }
    Some(out)
}

fn hex16(h: &[u8; 16]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_hex16(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Encode / decode
// ---------------------------------------------------------------------------

/// The index as text (records in key order — deterministic, diff-friendly).
pub fn encode(index: &ScanIndex) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    for (name, p) in &index.packed {
        out.push_str(&format!(
            "P\t{}\t{}\t{}\n",
            escape(name),
            hex16(&p.hash),
            p.arc_size
        ));
    }
    for (path, c) in &index.arcs {
        out.push_str(&format!(
            "A\t{}\t{}\t{}",
            escape(path),
            c.stamp.size,
            c.stamp.mtime_ns
        ));
        for m in &c.value {
            out.push('\t');
            out.push_str(&escape(m));
        }
        out.push('\n');
    }
    for (path, c) in &index.sidecars {
        out.push_str(&format!(
            "S\t{}\t{}\t{}\t{}\n",
            escape(path),
            c.stamp.size,
            c.stamp.mtime_ns,
            c.value.len()
        ));
        for (key, fields) in &c.value {
            out.push_str("R\t");
            out.push_str(&escape(key));
            for f in fields {
                out.push('\t');
                out.push_str(&escape(f));
            }
            out.push('\n');
        }
    }
    out
}

fn fields(line: &str) -> Option<Vec<String>> {
    line.split('\t').map(unescape).collect()
}

fn stamp(size: &str, mtime: &str) -> Option<Stamp> {
    Some(Stamp {
        size: size.parse().ok()?,
        mtime_ns: mtime.parse().ok()?,
    })
}

/// Parse an index. Anything unusable decodes to less, never to an error: an
/// unknown header ⇒ empty; a malformed record ⇒ that record dropped.
pub fn decode(text: &str) -> ScanIndex {
    let mut index = ScanIndex::default();
    let mut lines = text.lines().peekable();
    if lines.next().map(str::trim_end) != Some(HEADER) {
        return index;
    }
    while let Some(line) = lines.next() {
        let Some(f) = fields(line) else { continue };
        match f.first().map(String::as_str) {
            Some("P") if f.len() == 4 => {
                let (Some(hash), Ok(arc_size)) = (parse_hex16(&f[2]), f[3].parse::<u64>()) else {
                    continue;
                };
                index.packed.insert(f[1].clone(), Packed { hash, arc_size });
            }
            Some("A") if f.len() >= 4 => {
                let Some(stamp) = stamp(&f[2], &f[3]) else {
                    continue;
                };
                index.arcs.insert(
                    f[1].clone(),
                    Cached {
                        stamp,
                        value: f[4..].to_vec(),
                    },
                );
            }
            Some("S") if f.len() == 5 => {
                let (Some(stamp), Ok(count)) = (stamp(&f[2], &f[3]), f[4].parse::<usize>()) else {
                    continue;
                };
                // Exactly `count` R lines must follow; a short / broken run
                // drops the record (and leaves the next record to the loop).
                let mut rows = Vec::with_capacity(count.min(4096));
                let mut ok = true;
                for _ in 0..count {
                    let row = lines
                        .peek()
                        .and_then(|l| fields(l))
                        .filter(|r| r.len() >= 2 && r[0] == "R");
                    match row {
                        Some(r) => {
                            lines.next();
                            rows.push((r[1].clone(), r[2..].to_vec()));
                        }
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    index
                        .sidecars
                        .insert(f[1].clone(), Cached { stamp, value: rows });
                }
            }
            // Orphan `R` lines (their `S` was malformed) and unknown kinds.
            _ => {}
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(size: u64, mtime_ns: u64) -> Stamp {
        Stamp { size, mtime_ns }
    }

    fn sample() -> ScanIndex {
        let mut ix = ScanIndex::default();
        ix.packed.insert(
            "pl_ddr3afro00-0123abcd.arc".into(),
            Packed {
                hash: [0xAB; 16],
                arc_size: 1_234_567,
            },
        );
        ix.packed.insert(
            "mapset_hp2stage046-ffffffff.arc".into(),
            Packed {
                hash: [0x01; 16],
                arc_size: 64,
            },
        );
        ix.arcs.insert(
            "./data_mods/custom_models/dancers/Custom/Peter Griffin/pl_peter00.arc".into(),
            Cached {
                stamp: st(4096, 1_791_130_883_123_456_700),
                value: vec![
                    "data/chara/pl_peter00/pl_peter00.model".into(),
                    "data/chara/pl_peter00/motion/a.anm".into(),
                ],
            },
        );
        // An arc with no members at all still round-trips.
        ix.arcs.insert(
            "./x/pl_empty00.arc".into(),
            Cached {
                stamp: st(16, 7),
                value: vec![],
            },
        );
        ix.sidecars.insert(
            "./data_mods/custom_models/dancers/3rd MIX/Afro/chara_resources.rlist.txt".into(),
            Cached {
                stamp: st(40, 99),
                value: vec![
                    (
                        "ddr3afro00".into(),
                        vec!["pl".into(), "M".into(), "A".into(), "1".into()],
                    ),
                    ("lonely".into(), vec![]),
                ],
            },
        );
        ix.sidecars.insert(
            "./s/map_resources.rlist".into(),
            Cached {
                stamp: st(1, 2),
                value: vec![],
            },
        );
        ix
    }

    #[test]
    fn round_trip() {
        let ix = sample();
        let text = encode(&ix);
        assert!(text.starts_with(HEADER));
        assert_eq!(decode(&text), ix);
        // Deterministic.
        assert_eq!(encode(&decode(&text)), text);
    }

    #[test]
    fn escapes_round_trip() {
        let mut ix = ScanIndex::default();
        let nasty = "a\tb\\c\nd\re ♥ \\t";
        ix.arcs.insert(
            nasty.into(),
            Cached {
                stamp: st(1, 1),
                value: vec![nasty.into(), String::new()],
            },
        );
        ix.sidecars.insert(
            "p".into(),
            Cached {
                stamp: st(1, 1),
                value: vec![(nasty.into(), vec![nasty.into(), String::new()])],
            },
        );
        let text = encode(&ix);
        // Exactly header + A + S + R lines: nothing leaked a raw newline.
        assert_eq!(text.lines().count(), 4, "{text:?}");
        assert_eq!(decode(&text), ix);
    }

    #[test]
    fn unknown_header_is_empty() {
        let text = encode(&sample()).replacen("v1", "v0", 1);
        assert!(decode(&text).is_empty());
        assert!(decode("").is_empty());
        assert!(decode("garbage\nP\tx\t00\t1\n").is_empty());
    }

    #[test]
    fn malformed_records_drop_alone() {
        let good_hash = "ab".repeat(16);
        let text = format!(
            "{HEADER}\n\
             P\tbad-hash.arc\tzz\t5\n\
             P\tshort.arc\t{good_hash}\n\
             P\tok.arc\t{good_hash}\t9\n\
             A\tbadstamp.arc\tx\t1\tm\n\
             A\tok.arc\t3\t4\tm1\tm2\n\
             S\tshort.rlist\t1\t2\t3\n\
             R\tk1\tf\n\
             P\tafter.arc\t{good_hash}\t1\n\
             S\tok.rlist\t5\t6\t1\n\
             R\tk\tf1\tf2\n\
             R\torphan\n\
             A\tbad\\escape\t1\t1\n\
             Z\twhat\n"
        );
        let ix = decode(&text);
        assert_eq!(
            ix.packed.keys().collect::<Vec<_>>(),
            vec!["after.arc", "ok.arc"]
        );
        assert_eq!(
            ix.arcs.get("ok.arc").map(|c| c.value.clone()),
            Some(vec!["m1".to_string(), "m2".to_string()])
        );
        assert!(!ix.arcs.contains_key("badstamp.arc"));
        // The short S run (3 promised, 1 given) is dropped; the record after it
        // still parses.
        assert!(!ix.sidecars.contains_key("short.rlist"));
        assert_eq!(
            ix.sidecars.get("ok.rlist").map(|c| c.value.clone()),
            Some(vec![(
                "k".to_string(),
                vec!["f1".to_string(), "f2".to_string()]
            )])
        );
        assert_eq!(ix.arcs.len(), 1);
    }

    #[test]
    fn stamp_lookups() {
        let ix = sample();
        let path = "./data_mods/custom_models/dancers/Custom/Peter Griffin/pl_peter00.arc";
        let now = st(4096, 1_791_130_883_123_456_700);
        assert_eq!(ix.arc_members(path, now).map(Vec::len), Some(2));
        // A touched file (same size, new mtime) or a resized one misses.
        assert!(ix.arc_members(path, st(4096, 1)).is_none());
        assert!(ix.arc_members(path, st(4097, now.mtime_ns)).is_none());
        assert!(ix.arc_members("other", now).is_none());
        // An unknown mtime never matches, not even itself.
        assert!(!st(5, 0).still(st(5, 0)));
        let sc = "./data_mods/custom_models/dancers/3rd MIX/Afro/chara_resources.rlist.txt";
        assert_eq!(ix.sidecar_rows(sc, st(40, 99)).map(Vec::len), Some(2));
        assert!(ix.sidecar_rows(sc, st(40, 98)).is_none());
    }

    #[test]
    fn packed_freshness() {
        let ix = sample();
        let name = "pl_ddr3afro00-0123abcd.arc";
        assert_eq!(ix.packed_fresh(name, [0xAB; 16], 1_234_567), Some(true));
        // Stale fingerprint, or the arc on disk is not the one recorded.
        assert_eq!(ix.packed_fresh(name, [0xAC; 16], 1_234_567), Some(false));
        assert_eq!(ix.packed_fresh(name, [0xAB; 16], 1_234_566), Some(false));
        // No record ⇒ undecided (the caller reads the .hashed sidecar).
        assert_eq!(ix.packed_fresh("new.arc", [0xAB; 16], 1), None);
    }

    #[test]
    fn hex_codec() {
        let h = [
            0x00, 0x01, 0x7f, 0x80, 0xff, 0x10, 0x20, 0x30, 0, 0, 0, 0, 0, 0, 0, 0xee,
        ];
        assert_eq!(parse_hex16(&hex16(&h)), Some(h));
        assert_eq!(parse_hex16("00"), None);
        assert_eq!(parse_hex16(&"g".repeat(32)), None);
        assert_eq!(parse_hex16(&"é".repeat(16)), None);
    }
}
