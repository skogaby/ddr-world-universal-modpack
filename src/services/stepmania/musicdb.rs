//! `<music>` synthesis and the `musicdb.xml` member transform — pure (design
//! §5.4, feasibility §4.2 / §7.3).
//!
//! [`entry_xml`] emits one element whose types and attributes mirror the
//! stock entries byte for byte in form (`__type="u32"` mcode, plain
//! `basename`/`title`/`title_yomi`/`artist`, `u16` bpms, `u8` series, `u32`
//! property, `s32[10]` limited_ary, `u8[10]` diffLv) and omits everything an
//! injected song must not carry (`region` — so it is never region-gated —
//! `movie*`, `eventno*`, `voice`, `bgstage`, `genreflag`, `bemaniflag`).
//! psmap limits: `basename` ≤ 6 bytes (the caller's identity guarantees it),
//! `title`/`title_yomi`/`artist` ≤ 127 UTF-8 bytes (clipped at a character
//! boundary BEFORE escaping — the limit is on the decoded string the
//! property import stores; an over-long node silently drops the whole song).
//! `diffLv` is never 255 and never all-zero (the builder would substitute
//! 20s); `limited_ary` is 0 where a chart exists and −1 elsewhere.
//!
//! [`transform`] is the `startup.arc` member transform body: it scans the
//! post-overlay base document for every `<mcode>` and `<basename>` (a plain
//! byte scan — no XML parser; the stock file is machine-written and an
//! operator omnimix overlay keeps its shape), drops injected entries that
//! collide (reported, never renumbered), and inserts the surviving elements
//! immediately before the final `</mdb>`.
//!
//! Purity contract: `std` + `super::model` only; mounted by
//! `scripts/validate_stepmania.sh` and `prep/` (the StepMania prep tool).

use std::collections::HashSet;

use super::model::{SongEntry, SLOTS};

/// psmap text limit (0x80 including the terminator).
pub const MAX_TEXT_BYTES: usize = 127;

/// Clip `s` to at most `max` bytes at a UTF-8 character boundary.
#[must_use]
pub fn clip_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Escape element text the way the stock file does: only `&`, `<` and `>`
/// (stock titles carry literal `'` and `"` — `Robert "RAab" Stevenson`,
/// `putyourfaithinme'jazzygroove'`). Control characters other than tab / LF /
/// CR are dropped.
#[must_use]
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if c.is_control() && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Stock-style `title_yomi`: lowercase ASCII alphanumerics of the title with
/// round-bracket parentheticals wrapped in `'…'` (`Put Your Faith In Me
/// (Jazzy Groove)` → `putyourfaithinme'jazzygroove'`; the cabinet-proven
/// spike entry rendered `[11] [140] Big Money (Medium)` as
/// `11140bigmoney'medium'`). Empty when the title has no ASCII alphanumerics
/// (the caller then omits the tag; vt+0x28 falls back to the title).
#[must_use]
pub fn title_yomi(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' => {
                if depth == 0 {
                    out.push('\'');
                }
                depth += 1;
            }
            ')' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        out.push('\'');
                    }
                }
            }
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            _ => {}
        }
    }
    if depth > 0 {
        out.push('\'');
    }
    // A yomi of only quotes carries nothing.
    if out.bytes().all(|b| b == b'\'') {
        return String::new();
    }
    out
}

fn text_tag(out: &mut String, tag: &str, value: &str) {
    let clipped = clip_utf8(value, MAX_TEXT_BYTES);
    out.push_str("    <");
    out.push_str(tag);
    out.push('>');
    out.push_str(&xml_escape(clipped));
    out.push_str("</");
    out.push_str(tag);
    out.push_str(">\n");
}

/// One `<music>` element (indented like the stock file: two spaces for the
/// element, four for its children, LF line ends).
#[must_use]
pub fn entry_xml(e: &SongEntry, series: u8, property_bit: Option<u32>) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("  <music>\n");
    out.push_str(&format!("    <mcode __type=\"u32\">{}</mcode>\n", e.mcode));
    out.push_str(&format!(
        "    <basename>{}</basename>\n",
        xml_escape(&e.basename)
    ));
    let title = if e.title.trim().is_empty() {
        e.folder.as_str()
    } else {
        e.title.as_str()
    };
    text_tag(&mut out, "title", title);
    // Sort key from the romanised title (stock yomi is ASCII) so a native
    // CJK display title still sorts/filters by its transliteration.
    let yomi_src = if e.title_sort.trim().is_empty() {
        title
    } else {
        e.title_sort.as_str()
    };
    let yomi = title_yomi(yomi_src);
    if !yomi.is_empty() {
        text_tag(&mut out, "title_yomi", &yomi);
    }
    text_tag(&mut out, "artist", &e.artist);
    out.push_str(&format!(
        "    <bpmmax __type=\"u16\">{}</bpmmax>\n",
        e.bpm_max
    ));
    out.push_str(&format!(
        "    <bpmmin __type=\"u16\">{}</bpmmin>\n",
        e.bpm_min
    ));
    out.push_str(&format!("    <series __type=\"u8\">{}</series>\n", series));
    let property: u32 = property_bit
        .filter(|b| *b < 32)
        .map(|b| 1u32 << b)
        .unwrap_or(0);
    out.push_str(&format!(
        "    <property __type=\"u32\">{}</property>\n",
        property
    ));
    let limited: Vec<String> = e
        .levels
        .iter()
        .map(|&l| {
            if l == 0 {
                "-1".to_string()
            } else {
                "0".to_string()
            }
        })
        .collect();
    out.push_str(&format!(
        "    <limited_ary __type=\"s32\" __count=\"{}\">{}</limited_ary>\n",
        SLOTS,
        limited.join(" ")
    ));
    let levels: Vec<String> = e
        .levels
        .iter()
        .map(|&l| l.clamp(0, 19).to_string())
        .collect();
    out.push_str(&format!(
        "    <diffLv __type=\"u8\" __count=\"{}\">{}</diffLv>\n",
        SLOTS,
        levels.join(" ")
    ));
    out.push_str("  </music>\n");
    out
}

/// Identities present in the base document.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BaseIds {
    pub mcodes: HashSet<u32>,
    pub basenames: HashSet<String>,
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn scan_tag_values<'a>(xml: &'a [u8], open_prefix: &[u8], close: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let Some(start) = find_from(xml, open_prefix, pos) {
        let Some(gt) = find_from(xml, b">", start) else {
            break;
        };
        let value_start = gt + 1;
        let Some(end) = find_from(xml, close, value_start) else {
            break;
        };
        out.push(&xml[value_start..end]);
        pos = end + close.len();
    }
    out
}

/// Every `<mcode …>N</mcode>` and `<basename>x</basename>` in the document.
#[must_use]
pub fn scan_base_ids(xml: &[u8]) -> BaseIds {
    let mut ids = BaseIds::default();
    for v in scan_tag_values(xml, b"<mcode", b"</mcode>") {
        if let Ok(s) = std::str::from_utf8(v) {
            if let Ok(n) = s.trim().parse::<u32>() {
                ids.mcodes.insert(n);
            }
        }
    }
    for v in scan_tag_values(xml, b"<basename", b"</basename>") {
        if let Ok(s) = std::str::from_utf8(v) {
            ids.basenames.insert(s.trim().to_string());
        }
    }
    ids
}

/// Why an entry was left out of the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    McodeInBase,
    BasenameInBase,
    NoDeclaredSlot,
    /// Appending it would push the document past the arena text budget.
    ArenaBudget,
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::McodeInBase => "mcode already in the base musicdb",
            Self::BasenameInBase => "basename already in the base musicdb",
            Self::NoDeclaredSlot => "no declared slot",
            Self::ArenaBudget => "musicdb.xml would exceed the property arena budget",
        })
    }
}

/// Outcome of [`transform_report`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransformReport {
    /// The rewritten document; `None` when nothing was injected (no
    /// surviving entry, or no `</mdb>` to insert before).
    pub xml: Option<Vec<u8>>,
    /// Indices (into the input slice) of the entries that were injected.
    pub injected: Vec<usize>,
    /// Indices and reasons of the entries that were not.
    pub dropped: Vec<(usize, DropReason)>,
    /// Whether the document had no `</mdb>` close tag.
    pub no_close_tag: bool,
}

impl TransformReport {
    /// How many entries the arena budget left out.
    #[must_use]
    pub fn over_budget(&self) -> usize {
        self.dropped
            .iter()
            .filter(|(_, r)| *r == DropReason::ArenaBudget)
            .count()
    }
}

/// Inject `entries` before the final `</mdb>`, dropping base collisions.
/// `text_budget` (bytes; `None` = unbounded) caps the rewritten document:
/// entries are appended in order until the next one would exceed it, and
/// the rest are dropped as [`DropReason::ArenaBudget`] — a document larger
/// than the game's property arena loads NO songs, so a smaller library
/// beats a complete one.
#[must_use]
pub fn transform_report(
    xml: &[u8],
    entries: &[SongEntry],
    series: u8,
    property_bit: Option<u32>,
    text_budget: Option<usize>,
) -> TransformReport {
    let mut report = TransformReport::default();
    let base = scan_base_ids(xml);
    // The insertion adds at most one LF besides the fragment.
    let budget = text_budget.map(|b| b.saturating_sub(xml.len() + 1));
    let mut fragment = String::new();
    for (i, e) in entries.iter().enumerate() {
        if e.levels.iter().all(|&l| l == 0) {
            report.dropped.push((i, DropReason::NoDeclaredSlot));
            continue;
        }
        if base.mcodes.contains(&e.mcode) {
            report.dropped.push((i, DropReason::McodeInBase));
            continue;
        }
        if base.basenames.contains(&e.basename) {
            report.dropped.push((i, DropReason::BasenameInBase));
            continue;
        }
        let element = entry_xml(e, series, property_bit);
        if budget.is_some_and(|b| fragment.len() + element.len() > b) {
            report.dropped.push((i, DropReason::ArenaBudget));
            continue;
        }
        fragment.push_str(&element);
        report.injected.push(i);
    }
    if report.injected.is_empty() {
        return report;
    }
    let needle = b"</mdb>";
    let Some(pos) = xml.windows(needle.len()).rposition(|w| w == needle) else {
        report.no_close_tag = true;
        report.injected.clear();
        return report;
    };
    let mut out = Vec::with_capacity(xml.len() + fragment.len() + 1);
    out.extend_from_slice(&xml[..pos]);
    // Keep the insertion on its own line when the close tag was glued to
    // the previous element.
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(fragment.as_bytes());
    out.extend_from_slice(&xml[pos..]);
    report.xml = Some(out);
    report
}

/// The member-transform shape: `Some(new bytes)` or `None` for "unchanged".
#[must_use]
pub fn transform(
    xml: &[u8],
    entries: &[SongEntry],
    series: u8,
    property_bit: Option<u32>,
    text_budget: Option<usize>,
) -> Option<Vec<u8>> {
    transform_report(xml, entries, series, property_bit, text_budget).xml
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> SongEntry {
        SongEntry {
            pack: "Pack".into(),
            folder: "Song".into(),
            basename: "zab12c".into(),
            mcode: 9_000_001,
            title: "Put Your Faith In Me (Jazzy Groove)".into(),
            artist: "UZI-LAY".into(),
            bpm_min: 150,
            bpm_max: 150,
            levels: [0, 0, 0, 0, 11, 0, 0, 0, 0, 0],
            ..SongEntry::default()
        }
    }

    const STOCK: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mdb>\n  <music>\n    <mcode __type=\"u32\">38660</mcode>\n    <basename>drat</basename>\n    <title>DanRevoAthlon</title>\n  </music>\n</mdb>\n";

    #[test]
    fn entry_matches_the_spike_shape() {
        let xml = entry_xml(&entry(), 30, None);
        let expected = "  <music>\n    <mcode __type=\"u32\">9000001</mcode>\n    <basename>zab12c</basename>\n    <title>Put Your Faith In Me (Jazzy Groove)</title>\n    <title_yomi>putyourfaithinme'jazzygroove'</title_yomi>\n    <artist>UZI-LAY</artist>\n    <bpmmax __type=\"u16\">150</bpmmax>\n    <bpmmin __type=\"u16\">150</bpmmin>\n    <series __type=\"u8\">30</series>\n    <property __type=\"u32\">0</property>\n    <limited_ary __type=\"s32\" __count=\"10\">-1 -1 -1 -1 0 -1 -1 -1 -1 -1</limited_ary>\n    <diffLv __type=\"u8\" __count=\"10\">0 0 0 0 11 0 0 0 0 0</diffLv>\n  </music>\n";
        assert_eq!(xml, expected);
        let with_bit = entry_xml(&entry(), 22, Some(10));
        assert!(with_bit.contains("<property __type=\"u32\">1024</property>"));
    }

    #[test]
    fn yomi_comes_from_the_sort_title() {
        let mut e = entry();
        e.title = "予感".into();
        e.title_sort = "Yokan".into();
        let xml = entry_xml(&e, 30, None);
        assert!(xml.contains("<title>予感</title>"));
        assert!(xml.contains("<title_yomi>yokan</title_yomi>"));
        e.title_sort.clear();
        assert!(!entry_xml(&e, 30, None).contains("title_yomi"));
    }

    #[test]
    fn text_is_clipped_at_a_utf8_boundary_and_escaped() {
        // 42 × "予" = 126 bytes; one more crosses 127 mid-character.
        let long = "予".repeat(43);
        let clipped = clip_utf8(&long, MAX_TEXT_BYTES);
        assert_eq!(clipped.len(), 126);
        assert_eq!(clipped.chars().count(), 42);
        assert_eq!(
            xml_escape("a & b <c> \"d\" 'e'"),
            "a &amp; b &lt;c&gt; \"d\" 'e'"
        );
        let mut e = entry();
        e.title = format!("{}<&>", "x".repeat(130));
        let xml = entry_xml(&e, 1, None);
        let title_line = xml.lines().find(|l| l.contains("<title>")).unwrap();
        assert_eq!(
            title_line,
            format!("    <title>{}</title>", "x".repeat(127))
        );
    }

    #[test]
    fn yomi_rules() {
        assert_eq!(
            title_yomi("Put Your Faith In Me (Jazzy Groove)"),
            "putyourfaithinme'jazzygroove'"
        );
        assert_eq!(
            title_yomi("[11] [140] Big Money (Medium)"),
            "11140bigmoney'medium'"
        );
        assert_eq!(title_yomi("予"), "");
        assert_eq!(title_yomi("(unclosed"), "'unclosed'");
    }

    #[test]
    fn transform_inserts_before_mdb_and_drops_collisions() {
        let a = entry();
        let mut b = entry();
        b.basename = "zzzzzz".into();
        b.mcode = 38660; // collides with stock drat
        let mut c = entry();
        c.basename = "drat".into();
        c.mcode = 9_000_003;
        let mut d = entry();
        d.basename = "zdddd1".into();
        d.mcode = 9_000_004;
        d.levels = [0; 10];
        let report = transform_report(STOCK.as_bytes(), &[a, b, c, d], 30, Some(10), None);
        assert_eq!(report.injected, vec![0]);
        assert_eq!(
            report.dropped,
            vec![
                (1, DropReason::McodeInBase),
                (2, DropReason::BasenameInBase),
                (3, DropReason::NoDeclaredSlot)
            ]
        );
        let out = String::from_utf8(report.xml.unwrap()).unwrap();
        assert!(out.ends_with("  </music>\n</mdb>\n"));
        assert_eq!(out.matches("<music>").count(), 2);
        assert!(out.starts_with(STOCK.split("</mdb>").next().unwrap()));
        // Nothing to inject → unchanged.
        assert!(transform(STOCK.as_bytes(), &[], 30, None, None).is_none());
        // No close tag → unchanged, flagged.
        let r = transform_report(b"<mdb>", &[entry()], 30, None, None);
        assert!(r.xml.is_none() && r.no_close_tag);
    }

    #[test]
    fn arena_budget_caps_the_document() {
        let mut entries = Vec::new();
        for i in 0..5u32 {
            let mut e = entry();
            e.basename = format!("zb{i:04}");
            e.mcode = 9_100_000 + i;
            entries.push(e);
        }
        let one = entry_xml(&entries[0], 30, None).len();
        // Room for the stock document, the LF and exactly three entries.
        let budget = STOCK.len() + 1 + 3 * one;
        let report = transform_report(STOCK.as_bytes(), &entries, 30, None, Some(budget));
        assert_eq!(report.injected, vec![0, 1, 2]);
        assert_eq!(
            report.dropped,
            vec![(3, DropReason::ArenaBudget), (4, DropReason::ArenaBudget)]
        );
        assert_eq!(report.over_budget(), 2);
        assert!(report.xml.as_ref().unwrap().len() <= budget);
        // A budget the stock document already exceeds injects nothing.
        let none = transform_report(STOCK.as_bytes(), &entries, 30, None, Some(STOCK.len()));
        assert!(none.xml.is_none());
        assert_eq!(none.over_budget(), 5);
        // Unbounded keeps everything.
        let all = transform_report(STOCK.as_bytes(), &entries, 30, None, None);
        assert_eq!(all.injected.len(), 5);
    }

    #[test]
    fn base_scan_reads_mcodes_and_basenames() {
        let ids = scan_base_ids(STOCK.as_bytes());
        assert!(ids.mcodes.contains(&38660));
        assert!(ids.basenames.contains("drat"));
        assert_eq!(ids.mcodes.len(), 1);
    }
}
