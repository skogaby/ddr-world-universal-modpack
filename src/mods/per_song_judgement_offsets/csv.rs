//! Pure CSV layer for `judgement_offsets.csv` (Per-Song Judgement Offsets,
//! design §Data Models → CSV).
//!
//! Schema: header `code,p1_offset,p2_offset`; `code` is the musicdb basename
//! (opaque ASCII, never contains commas or quotes — documented assumption);
//! offset cells are optional integers clamped to ±100 (blank = unset for that
//! side). The file is machine-managed: the boot crawl appends missing codes
//! (never modifying existing rows) and options-menu edits upsert single
//! cells.
//!
//! Dependency-free on purpose: host-tested through the harness mount
//! (`scripts/validate_judgement_offsets.sh`) — no logging, no `unsafe`, no
//! game APIs. Callers translate [`ParseStats`] into their own one-shot WARN.

use std::collections::HashMap;

/// Offset value domain (design D4): milliseconds, clamped inclusive.
pub const OFFSET_MIN: i8 = -100;
/// See [`OFFSET_MIN`].
pub const OFFSET_MAX: i8 = 100;

/// The header line [`serialize`] always emits (and [`parse`] tolerates
/// missing).
pub const HEADER: &str = "code,p1_offset,p2_offset";

/// One CSV row: a song code plus per-side optional offsets (index 0 = P1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvRow {
    pub code: String,
    pub offsets: [Option<i8>; 2],
}

/// Ordered document model. Row order is preserved across parse/serialize;
/// appends land at the end. An internal code → index map keeps dedupe and
/// upsert O(1).
#[derive(Debug, Default, Clone)]
pub struct CsvDoc {
    rows: Vec<CsvRow>,
    index: HashMap<String, usize>,
}

/// What [`parse`] had to tolerate. The caller aggregates this into a single
/// WARN; the pure layer never logs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParseStats {
    /// Values pulled back into the ±100 domain.
    pub clamped: u32,
    /// Lines dropped entirely (non-integer cell, too many cells).
    pub skipped: u32,
    /// Later occurrences of an already-seen code (first wins).
    pub duplicates: u32,
    /// 1-based line numbers of skipped/duplicate lines, capped at
    /// [`ParseStats::BAD_LINE_CAP`].
    pub bad_lines: Vec<u32>,
}

impl ParseStats {
    /// Retention cap for [`ParseStats::bad_lines`] — enough for a WARN.
    pub const BAD_LINE_CAP: usize = 8;

    /// True when the input parsed without any tolerance events.
    pub fn is_clean(&self) -> bool {
        self.clamped == 0 && self.skipped == 0 && self.duplicates == 0
    }

    fn note_bad_line(&mut self, line_no: u32) {
        if self.bad_lines.len() < Self::BAD_LINE_CAP {
            self.bad_lines.push(line_no);
        }
    }
}

/// Result of parsing a single offset cell.
enum CellParse {
    Blank,
    /// Parsed value plus whether clamping was applied.
    Value(i8, bool),
    Bad,
}

fn parse_cell(cell: &str) -> CellParse {
    let cell = cell.trim();
    if cell.is_empty() {
        return CellParse::Blank;
    }
    match cell.parse::<i64>() {
        Ok(v) => {
            let clamped = v.clamp(OFFSET_MIN as i64, OFFSET_MAX as i64);
            CellParse::Value(clamped as i8, clamped != v)
        }
        Err(_) => CellParse::Bad,
    }
}

impl CsvDoc {
    /// Rows in file order.
    pub fn rows(&self) -> &[CsvRow] {
        &self.rows
    }

    /// Look up a row by code.
    pub fn get(&self, code: &str) -> Option<&CsvRow> {
        self.index.get(code).map(|&i| &self.rows[i])
    }

    /// Append codes not already present, with blank offsets. Existing rows
    /// are never touched. Returns how many were appended.
    pub fn append_missing<'a>(&mut self, codes: impl Iterator<Item = &'a str>) -> usize {
        let mut appended = 0;
        for code in codes {
            if self.index.contains_key(code) {
                continue;
            }
            self.push_row(CsvRow {
                code: code.to_string(),
                offsets: [None, None],
            });
            appended += 1;
        }
        appended
    }

    /// Update one side's cell for `code`, appending the row when absent.
    pub fn upsert(&mut self, code: &str, side: usize, value: Option<i8>) {
        debug_assert!(side < 2);
        let side = side.min(1);
        let value = value.map(|v| v.clamp(OFFSET_MIN, OFFSET_MAX));
        match self.index.get(code) {
            Some(&i) => self.rows[i].offsets[side] = value,
            None => {
                let mut offsets = [None, None];
                offsets[side] = value;
                self.push_row(CsvRow {
                    code: code.to_string(),
                    offsets,
                });
            }
        }
    }

    fn push_row(&mut self, row: CsvRow) {
        self.index.insert(row.code.clone(), self.rows.len());
        self.rows.push(row);
    }
}

/// Parse a whole file. Tolerates: missing header, CRLF, padded cells, fewer
/// than three cells (missing cells = blank). Skips lines with a non-integer
/// cell or more than three cells; first occurrence of a duplicate code wins.
pub fn parse(text: &str) -> (CsvDoc, ParseStats) {
    let mut doc = CsvDoc::default();
    let mut stats = ParseStats::default();
    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx as u32 + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cells = line.split(',');
        let code = cells.next().unwrap_or("").trim();
        if idx == 0 && code.eq_ignore_ascii_case("code") {
            continue; // header
        }
        if code.is_empty() {
            stats.skipped += 1;
            stats.note_bad_line(line_no);
            continue;
        }
        let mut offsets = [None, None];
        let mut bad = false;
        for slot in offsets.iter_mut() {
            match cells.next().map(parse_cell) {
                None | Some(CellParse::Blank) => {}
                Some(CellParse::Value(v, clamped)) => {
                    if clamped {
                        stats.clamped += 1;
                    }
                    *slot = Some(v);
                }
                Some(CellParse::Bad) => {
                    bad = true;
                    break;
                }
            }
        }
        if bad || cells.next().is_some() {
            stats.skipped += 1;
            stats.note_bad_line(line_no);
            continue;
        }
        if doc.index.contains_key(code) {
            stats.duplicates += 1;
            stats.note_bad_line(line_no);
            continue;
        }
        doc.push_row(CsvRow {
            code: code.to_string(),
            offsets,
        });
    }
    (doc, stats)
}

/// Serialize with the header, preserving row order; `None` cells are blank.
/// LF line endings; trailing newline.
pub fn serialize(doc: &CsvDoc) -> String {
    let mut out = String::with_capacity(doc.rows.len() * 16 + HEADER.len() + 1);
    out.push_str(HEADER);
    out.push('\n');
    for row in &doc.rows {
        out.push_str(&row.code);
        for cell in row.offsets {
            out.push(',');
            if let Some(v) = cell {
                out.push_str(&v.to_string());
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_from(text: &str) -> CsvDoc {
        let (doc, stats) = parse(text);
        assert!(stats.is_clean(), "expected clean parse, got {stats:?}");
        doc
    }

    #[test]
    fn round_trip_preserves_content() {
        let text = "code,p1_offset,p2_offset\nputy,11,11\nneg,-100,\nblank,,\n";
        let doc = doc_from(text);
        assert_eq!(serialize(&doc), text);
    }

    #[test]
    fn parse_without_header() {
        let with = doc_from("code,p1_offset,p2_offset\nputy,11,11\n");
        let without = doc_from("puty,11,11\n");
        assert_eq!(with.rows(), without.rows());
        // Serialize re-adds the header.
        assert!(serialize(&without).starts_with(HEADER));
    }

    #[test]
    fn parse_crlf_and_whitespace() {
        let (doc, stats) = parse("code,p1_offset,p2_offset\r\n puty , 11 ,\t-3 \r\n");
        assert!(stats.is_clean());
        assert_eq!(
            doc.get("puty").unwrap().offsets,
            [Some(11), Some(-3)],
            "padded CRLF cells must load"
        );
    }

    #[test]
    fn parse_clamps_out_of_range() {
        let (doc, stats) = parse("a,250,-999\n");
        assert_eq!(doc.get("a").unwrap().offsets, [Some(100), Some(-100)]);
        assert_eq!(stats.clamped, 2);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn parse_skips_garbage_lines() {
        let (doc, stats) = parse("good,1,2\nbad,xx,3\ntoo,1,2,3\n");
        assert_eq!(doc.rows().len(), 1);
        assert_eq!(stats.skipped, 2);
        assert_eq!(stats.bad_lines, vec![2, 3]);
    }

    #[test]
    fn parse_first_duplicate_wins() {
        let (doc, stats) = parse("dup,1,\ndup,9,9\n");
        assert_eq!(doc.get("dup").unwrap().offsets, [Some(1), None]);
        assert_eq!(stats.duplicates, 1);
        assert_eq!(doc.rows().len(), 1);
    }

    #[test]
    fn append_missing_appends_only_new() {
        let mut doc = doc_from("a,5,\nb,,7\n");
        let appended = doc.append_missing(["a", "c", "d"].into_iter());
        assert_eq!(appended, 2);
        let codes: Vec<&str> = doc.rows().iter().map(|r| r.code.as_str()).collect();
        assert_eq!(codes, ["a", "b", "c", "d"]);
        assert_eq!(doc.get("a").unwrap().offsets, [Some(5), None]);
        assert_eq!(doc.get("c").unwrap().offsets, [None, None]);
    }

    #[test]
    fn upsert_updates_and_appends() {
        let mut doc = doc_from("x,1,2\n");
        doc.upsert("x", 0, Some(-7));
        assert_eq!(doc.get("x").unwrap().offsets, [Some(-7), Some(2)]);
        doc.upsert("y", 1, Some(3));
        assert_eq!(doc.get("y").unwrap().offsets, [None, Some(3)]);
        doc.upsert("x", 1, None);
        assert_eq!(doc.get("x").unwrap().offsets, [Some(-7), None]);
        assert_eq!(doc.rows().len(), 2);
    }

    #[test]
    fn empty_and_header_only_inputs() {
        for text in ["", "code,p1_offset,p2_offset\n"] {
            let (doc, stats) = parse(text);
            assert!(doc.rows().is_empty());
            assert!(stats.is_clean());
        }
    }

    /// Compatibility proof for the repo-committed pre-seeded CSV
    /// (`judgement_offsets.csv`, generated by
    /// `scripts/gen_judgement_offsets_csv.py`). The host harness exports
    /// `JUDGEMENT_OFFSETS_CSV` pointing at it; skipped when unset (e.g. a
    /// bare `cargo test` on an x86 host).
    #[test]
    fn committed_preseed_csv_parses_clean() {
        let Some(path) = std::env::var_os("JUDGEMENT_OFFSETS_CSV") else {
            return;
        };
        let text = std::fs::read_to_string(&path).expect("read committed CSV");
        let (doc, stats) = parse(&text);
        assert!(
            stats.is_clean(),
            "committed CSV must parse clean: {stats:?}"
        );
        assert!(!doc.rows().is_empty(), "committed CSV must carry rows");
        // P1 = P2 for every seeded row (script contract).
        for row in doc.rows() {
            assert_eq!(row.offsets[0], row.offsets[1], "P1=P2 for {}", row.code);
        }
    }
}
