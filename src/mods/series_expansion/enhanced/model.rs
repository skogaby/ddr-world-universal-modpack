//! Pure model for series_expansion's enhanced (config-defined) VERSION layout.
//!
//! Validates the untyped `series_expansion.custom_series_enhanced` JSON block
//! into an [`EnhancedPlan`] and derives everything the engine side needs from
//! it: the 0x88-stride filter-table row values, GROUP-tab membership, the
//! per-song names for custom series, and the label texture names.
//!
//! Rules (design R5–R8, R10, R11, R14):
//! - `num_columns` 1–5 (= the game's cell template index); anything else ⇒ 2.
//! - Each `filters[]` cell needs `label` (non-empty ASCII, no NUL),
//!   `series_start` (0–255), optional `series_end` (0–255, default start,
//!   ≥ start), `texture` (`[a-z0-9_]+`) and an optional `group`
//!   (`gold|white|classic|none`). Invalid cells are skipped, one warning each.
//! - Raw 16 is folded into 15 (the game maps 16 → 15 before any filter sees
//!   it and names both "DDR 2014"); the default group uses the raw start.
//! - At most `min(64, 24 × num_columns)` cells: 64 = the saved-filter u64,
//!   24 = grid rows whose buttons get visuals (lazy, on-screen-gated).
//!
//! Dependency-free apart from `serde_json` (no `crate::` imports) so
//! `scripts/validate_series_expansion.sh` can mount it on the host.

use serde_json::Value;

/// Saved selections are one u64 per filter category: index = bit.
pub const MAX_CELLS: usize = 64;
/// Grid rows below the tab row whose buttons get a visual. The game creates
/// a FilterButton's movie only while its *layout* rect intersects the
/// virtual screen band (top < 864 in 1280×720 with the 20 % margin), and
/// scrolling moves BM2D layers, not layout. With the item grid's tab row at
/// y ≈ 220, entry row k's top is ≈ 246 + 26k, so rows 0..=23 qualify.
/// Cabinet 2026-09-27: 20 rows at one column all rendered and scrolled.
pub const MAX_GRID_ROWS: usize = 24;
pub const DEFAULT_COLUMNS: u8 = 2;
/// Stock VERSION entry count; the table is padded to at least this many rows
/// (plus the sentinel) so a stale stock index can only ever hit an inert row.
pub const STOCK_ENTRY_COUNT: usize = 9;
/// First raw series value past the stock range (WORLD = 21).
pub const FIRST_CUSTOM_SERIES: u8 = 22;

/// GROUP tab, numbered like the stock group table and the tab press's `g`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Classic = 0,
    White = 1,
    Gold = 2,
}

impl Group {
    pub fn from_index(g: u32) -> Option<Group> {
        match g {
            0 => Some(Group::Classic),
            1 => Some(Group::White),
            2 => Some(Group::Gold),
            _ => None,
        }
    }

    /// Stock label key suffix (`version_<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Group::Classic => "classic",
            Group::White => "white",
            Group::Gold => "gold",
        }
    }

    /// Group whose raw series span contains `raw`: CLASSIC 1–13, WHITE 14–17,
    /// GOLD 18–21; none for 0 and custom values.
    pub fn for_series(raw: u8) -> Option<Group> {
        match raw {
            1..=13 => Some(Group::Classic),
            14..=17 => Some(Group::White),
            18..=21 => Some(Group::Gold),
            _ => None,
        }
    }
}

/// One validated cell. `start`/`end` are inclusive, already normalised
/// (no 16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub label: String,
    pub start: u8,
    pub end: u8,
    pub texture: String,
    pub group: Option<Group>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnhancedPlan {
    /// 1–5; also the FilterButton template index.
    pub columns: u8,
    /// Display order; the index is the selection index and saved bit.
    pub cells: Vec<Cell>,
}

/// Values for one row of the mod-owned 0x88-stride filter table:
/// `+0x30 start`, `+0x34 end_excl` (signed compares `start <= v < end_excl`),
/// `+0x38`/`+0x60 label`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    pub start: i32,
    pub end_excl: i32,
    pub label: String,
}

impl TableRow {
    /// A row no value matches (sentinel and padding).
    pub fn inert() -> TableRow {
        TableRow {
            start: i32::MAX,
            end_excl: 0,
            label: String::new(),
        }
    }
}

/// VERSION filter-table stride (stock layout).
pub const ENTRY_STRIDE: usize = 0x88;
const FIELD_KEY: usize = 0x08;
const FIELD_START: usize = 0x30;
/// Per-row exclusive end: 4 bytes of stock padding the predicate's second
/// compare is repointed at (`+0xB8` → `+0x34`).
const FIELD_END: usize = 0x34;
const FIELD_CODE: usize = 0x38;
const FIELD_DISPLAY: usize = 0x60;
/// MSVC `std::string`: 16-byte buffer (or heap pointer), size, capacity.
const STR_SIZE: usize = 0x10;
const STR_CAPACITY: usize = 0x18;
const SSO_CAPACITY: usize = 0x0F;

fn encode_string(out: &mut [u8], at: usize, text: &str, heap: &mut impl FnMut(&str) -> u64) {
    let bytes = text.as_bytes();
    if bytes.len() <= SSO_CAPACITY {
        out[at..at + bytes.len()].copy_from_slice(bytes);
        out[at + STR_CAPACITY..at + STR_CAPACITY + 8]
            .copy_from_slice(&(SSO_CAPACITY as u64).to_le_bytes());
    } else {
        out[at..at + 8].copy_from_slice(&heap(text).to_le_bytes());
        out[at + STR_CAPACITY..at + STR_CAPACITY + 8]
            .copy_from_slice(&(bytes.len() as u64).to_le_bytes());
    }
    out[at + STR_SIZE..at + STR_SIZE + 8].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
}

/// Bytes of one filter-table row. Strings of ≤ 15 bytes are stored inline;
/// longer ones call `heap(text)` for the address of a NUL-terminated copy the
/// caller keeps alive forever (capacity = length > 15 marks the heap form;
/// the game only ever copies these strings).
pub fn encode_row(row: &TableRow, mut heap: impl FnMut(&str) -> u64) -> [u8; ENTRY_STRIDE] {
    let mut out = [0u8; ENTRY_STRIDE];
    encode_string(&mut out, FIELD_KEY, "", &mut heap);
    out[FIELD_START..FIELD_START + 4].copy_from_slice(&row.start.to_le_bytes());
    out[FIELD_END..FIELD_END + 4].copy_from_slice(&row.end_excl.to_le_bytes());
    encode_string(&mut out, FIELD_CODE, &row.label, &mut heap);
    encode_string(&mut out, FIELD_DISPLAY, &row.label, &mut heap);
    out
}

/// Maximum cell count for a column count.
pub fn max_cells(columns: u8) -> usize {
    (MAX_GRID_ROWS * columns as usize).min(MAX_CELLS)
}

/// Label canvas width for a column count (the stock label slot of
/// `filter_switch_base0N`).
pub fn canvas_width(columns: u8) -> u32 {
    match columns {
        1 => 220,
        2 => 104,
        3 => 64,
        4 => 44,
        _ => 32,
    }
}

/// Highest jacket-thumbnail ARC index the loader may walk to. The loop's
/// `CMP RSI,imm8` sign-extends before an unsigned `JBE`, so 128+ never ends.
pub const THUMBNAIL_MAX_BOUND: u8 = 127;
/// Stock bound (series 0..=21).
pub const THUMBNAIL_STOCK_BOUND: u8 = 21;

/// Enhanced-mode thumbnail loop bound: the highest N in 22..=127 whose
/// `jacket_thumbnails_<region>_<N>.arc` exists (`exists(N)`), or `None` to
/// leave the stock bound. Never derived from cell ranges (a catch-all range
/// would otherwise walk to 255).
pub fn thumbnail_bound(exists: impl Fn(u8) -> bool) -> Option<u8> {
    (FIRST_CUSTOM_SERIES..=THUMBNAIL_MAX_BOUND)
        .rev()
        .find(|&n| exists(n))
}

/// Legacy-mode thumbnail loop bound for the highest configured
/// `series_value`: clamped to 127 (see [`THUMBNAIL_MAX_BOUND`]).
pub fn legacy_thumbnail_bound(max_series: u8) -> u8 {
    max_series.min(THUMBNAIL_MAX_BOUND)
}

/// Stock label texture whose atlas slot has the width's canvas size (the
/// clone donor; all five live in the stock IFS's `tex001` atlas).
pub fn label_donor(columns: u8) -> &'static str {
    match columns {
        1 => "sefi_event_league",
        2 => "sefi_version_world",
        3 => "sefi_version_gold",
        4 => "sefi_title_other",
        _ => "sefi_level_00",
    }
}

/// In-game texture name of a cell's label.
pub fn texture_name(texture: &str, columns: u8) -> String {
    format!("sefi_version_{}_{}col", texture, columns)
}

/// Source PNG file names under `data_mods/custom_series/series_labels/`, in
/// resolution order: the per-width art, then the unsuffixed fallback.
pub fn source_candidates(texture: &str, columns: u8) -> [String; 2] {
    [
        format!("{}.png", texture_name(texture, columns)),
        format!("sefi_version_{}.png", texture),
    ]
}

fn fold_sixteen(raw: u8) -> u8 {
    if raw == 16 {
        15
    } else {
        raw
    }
}

fn is_texture_key(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn series_value(v: Option<&Value>) -> Result<Option<u8>, String> {
    match v {
        None => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) if n <= 255 => Ok(Some(n as u8)),
            _ => Err(format!("{} is not an integer 0-255", v)),
        },
    }
}

fn parse_cell(i: usize, v: &Value) -> Result<Cell, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("filters[{}]: not an object — cell skipped", i))?;

    let label = match obj.get("label").and_then(Value::as_str) {
        Some(l) if !l.trim().is_empty() && l.is_ascii() && !l.contains('\0') => l.to_string(),
        _ => {
            return Err(format!(
                "filters[{}]: label must be non-empty ASCII text — cell skipped",
                i
            ))
        }
    };
    let start = series_value(obj.get("series_start"))
        .map_err(|e| format!("filters[{}]: series_start {} — cell skipped", i, e))?
        .ok_or_else(|| format!("filters[{}]: series_start missing — cell skipped", i))?;
    let end = series_value(obj.get("series_end"))
        .map_err(|e| format!("filters[{}]: series_end {} — cell skipped", i, e))?
        .unwrap_or(start);
    if start > end {
        return Err(format!(
            "filters[{}]: series_start {} > series_end {} — cell skipped",
            i, start, end
        ));
    }
    let texture = match obj.get("texture").and_then(Value::as_str) {
        Some(t) if is_texture_key(t) => t.to_string(),
        other => {
            return Err(format!(
                "filters[{}]: texture {:?} must match [a-z0-9_]+ — cell skipped",
                i, other
            ))
        }
    };
    let group = match obj.get("group") {
        None => Group::for_series(start),
        Some(g) => match g.as_str() {
            Some("gold") => Some(Group::Gold),
            Some("white") => Some(Group::White),
            Some("classic") => Some(Group::Classic),
            Some("none") => None,
            _ => {
                return Err(format!(
                    "filters[{}]: group {} must be gold|white|classic|none — cell skipped",
                    i, g
                ))
            }
        },
    };
    Ok(Cell {
        label,
        start: fold_sixteen(start),
        end: fold_sixteen(end),
        texture,
        group,
    })
}

/// Validate a `custom_series_enhanced` value. `Err` only when it is not a
/// JSON object (the caller falls back); otherwise returns the plan and one
/// warning per defaulted field, skipped cell or dropped overflow.
pub fn parse(v: &Value) -> Result<(EnhancedPlan, Vec<String>), String> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("custom_series_enhanced must be an object, got {}", v))?;
    let mut warnings = Vec::new();

    let columns = match obj.get("num_columns").and_then(Value::as_u64) {
        Some(n) if (1..=5).contains(&n) => n as u8,
        _ => {
            warnings.push(format!(
                "num_columns {} is not an integer 1-5 — using {}",
                obj.get("num_columns")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "missing".into()),
                DEFAULT_COLUMNS
            ));
            DEFAULT_COLUMNS
        }
    };

    let mut cells = Vec::new();
    match obj.get("filters").and_then(Value::as_array) {
        Some(list) => {
            for (i, cell) in list.iter().enumerate() {
                match parse_cell(i, cell) {
                    Ok(c) => cells.push(c),
                    Err(w) => warnings.push(w),
                }
            }
        }
        None => warnings.push("filters is missing or not a list — no cells".into()),
    }

    let cap = max_cells(columns);
    if cells.len() > cap {
        warnings.push(format!(
            "{} cells exceed the limit of {} for {} column(s) — extra cells dropped",
            cells.len(),
            cap,
            columns
        ));
        cells.truncate(cap);
    }

    Ok((EnhancedPlan { columns, cells }, warnings))
}

impl EnhancedPlan {
    /// Cell (selection) indices a press of GROUP tab `g` selects.
    pub fn members(&self, g: u32) -> Vec<u32> {
        let Some(group) = Group::from_index(g) else {
            return Vec::new();
        };
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.group == Some(group))
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Filter-table rows: one per cell, then inert rows up to
    /// `max(N, STOCK_ENTRY_COUNT) + 1` (the chip builder probes index N).
    pub fn table_rows(&self) -> Vec<TableRow> {
        let mut rows: Vec<TableRow> = self
            .cells
            .iter()
            .map(|c| TableRow {
                start: c.start as i32,
                end_excl: c.end as i32 + 1,
                label: c.label.clone(),
            })
            .collect();
        let total = self.cells.len().max(STOCK_ENTRY_COUNT) + 1;
        rows.resize(total, TableRow::inert());
        rows
    }

    /// Per-song names for custom series: each raw value ≥ 22 named by the
    /// first cell covering exactly that value.
    pub fn custom_names(&self) -> Vec<(u8, &str)> {
        let mut out: Vec<(u8, &str)> = Vec::new();
        for c in &self.cells {
            if c.start == c.end
                && c.start >= FIRST_CUSTOM_SERIES
                && !out.iter().any(|(v, _)| *v == c.start)
            {
                out.push((c.start, c.label.as_str()));
            }
        }
        out
    }

    /// Button label key for cell `i` (`FilterButton+0xC8`; texture = `sefi_` + key).
    pub fn label_key(&self, i: usize) -> String {
        match self.cells.get(i) {
            Some(c) => format!("version_{}_{}col", c.texture, self.columns),
            None => String::new(),
        }
    }

    /// Distinct in-game label texture names, in first-use order.
    pub fn label_stems(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in &self.cells {
            let name = texture_name(&c.texture, self.columns);
            if !out.contains(&name) {
                out.push(name);
            }
        }
        out
    }

    /// One-line description for the boot log.
    pub fn summary(&self) -> String {
        let count = |g: Option<Group>| self.cells.iter().filter(|c| c.group == g).count();
        format!(
            "{} columns, {} cells (GOLD {}, WHITE {}, CLASSIC {}, no group {})",
            self.columns,
            self.cells.len(),
            count(Some(Group::Gold)),
            count(Some(Group::White)),
            count(Some(Group::Classic)),
            count(None)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cell(label: &str, start: u64, texture: &str) -> serde_json::Value {
        json!({ "label": label, "series_start": start, "texture": texture })
    }

    fn block(columns: serde_json::Value, filters: Vec<serde_json::Value>) -> serde_json::Value {
        json!({ "num_columns": columns, "filters": filters })
    }

    fn plan_of(v: serde_json::Value) -> (EnhancedPlan, Vec<String>) {
        parse(&v).expect("object")
    }

    fn canonical() -> EnhancedPlan {
        let text = include_str!("testdata/canonical_enhanced.json");
        let root: serde_json::Value = serde_json::from_str(text).expect("fixture json");
        let (plan, warnings) = parse(&root["custom_series_enhanced"]).expect("object");
        assert!(
            warnings.is_empty(),
            "canonical fixture warnings: {warnings:?}"
        );
        plan
    }

    #[test]
    fn minimal_cell_defaults_end_to_start() {
        let (plan, warnings) = plan_of(block(json!(3), vec![cell("WORLD", 21, "world")]));
        assert!(warnings.is_empty());
        assert_eq!(plan.columns, 3);
        assert_eq!(
            plan.cells,
            vec![Cell {
                label: "WORLD".into(),
                start: 21,
                end: 21,
                texture: "world".into(),
                group: Some(Group::Gold),
            }]
        );
    }

    #[test]
    fn empty_filters_and_non_objects() {
        let (plan, warnings) = plan_of(block(json!(2), vec![]));
        assert!(plan.cells.is_empty() && warnings.is_empty());
        assert!(parse(&json!([1, 2])).is_err());
        assert!(parse(&json!("x")).is_err());
        assert!(parse(&serde_json::Value::Null).is_err());
    }

    #[test]
    fn missing_or_bad_filters_is_an_empty_list_with_a_warning() {
        let (plan, warnings) = plan_of(json!({ "num_columns": 2 }));
        assert!(plan.cells.is_empty());
        assert_eq!(warnings.len(), 1);
        let (plan, warnings) = plan_of(json!({ "num_columns": 2, "filters": {} }));
        assert!(plan.cells.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn num_columns_validation() {
        for good in 1..=5u64 {
            let (plan, warnings) = plan_of(block(json!(good), vec![]));
            assert_eq!(plan.columns as u64, good);
            assert!(warnings.is_empty());
        }
        for bad in [
            json!(0),
            json!(6),
            json!("3"),
            json!(2.5),
            json!(-1),
            serde_json::Value::Null,
        ] {
            let (plan, warnings) = plan_of(block(bad.clone(), vec![]));
            assert_eq!(plan.columns, DEFAULT_COLUMNS, "{bad}");
            assert_eq!(warnings.len(), 1, "{bad}");
        }
        let (plan, warnings) = plan_of(json!({ "filters": [] }));
        assert_eq!(plan.columns, DEFAULT_COLUMNS);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_cells_are_skipped_one_warning_each() {
        let bad = vec![
            json!({ "label": "A", "series_start": 5, "series_end": 4, "texture": "a" }),
            json!({ "label": "A", "series_start": 256, "texture": "a" }),
            json!({ "label": "A", "series_start": -1, "texture": "a" }),
            json!({ "label": "A", "series_start": 1.5, "texture": "a" }),
            json!({ "label": "A", "series_start": 3, "series_end": 300, "texture": "a" }),
            json!({ "label": "A", "texture": "a" }),
            json!({ "label": "A", "series_start": 3, "texture": "Bad-Name" }),
            json!({ "label": "A", "series_start": 3 }),
            json!({ "label": "", "series_start": 3, "texture": "a" }),
            json!({ "label": "ÄÖ", "series_start": 3, "texture": "a" }),
            json!({ "label": "A\u{0}B", "series_start": 3, "texture": "a" }),
            json!({ "series_start": 3, "texture": "a" }),
            json!({ "label": "A", "series_start": 3, "texture": "a", "group": "silver" }),
            json!({ "label": "A", "series_start": 3, "texture": "a", "group": "GOLD" }),
            json!(42),
        ];
        let count = bad.len();
        let mut filters = bad;
        filters.push(cell("OK", 3, "ok"));
        let (plan, warnings) = plan_of(block(json!(2), filters));
        assert_eq!(plan.cells.len(), 1);
        assert_eq!(plan.cells[0].texture, "ok");
        assert_eq!(warnings.len(), count, "{warnings:#?}");
    }

    #[test]
    fn cell_cap_follows_columns() {
        let many = |n: usize| {
            (0..n)
                .map(|i| cell("A", (i % 21 + 1) as u64, "a"))
                .collect()
        };
        let (plan, warnings) = plan_of(block(json!(1), many(30)));
        assert_eq!(plan.cells.len(), MAX_GRID_ROWS);
        assert_eq!(warnings.len(), 1);
        let (plan, warnings) = plan_of(block(json!(2), many(48)));
        assert_eq!(plan.cells.len(), 48);
        assert!(warnings.is_empty());
        let (plan, warnings) = plan_of(block(json!(5), many(70)));
        assert_eq!(plan.cells.len(), MAX_CELLS);
        assert_eq!(warnings.len(), 1);
        assert_eq!(max_cells(3), MAX_CELLS);
        assert_eq!(max_cells(2), 2 * MAX_GRID_ROWS);
    }

    #[test]
    fn sixteen_is_folded_into_fifteen() {
        let filters = vec![
            json!({ "label": "A", "series_start": 16, "texture": "a" }),
            json!({ "label": "B", "series_start": 14, "series_end": 16, "texture": "b" }),
            json!({ "label": "C", "series_start": 16, "series_end": 20, "texture": "c" }),
            json!({ "label": "D", "series_start": 15, "series_end": 16, "texture": "d" }),
        ];
        let (plan, warnings) = plan_of(block(json!(2), filters));
        assert!(warnings.is_empty());
        let ranges: Vec<(u8, u8)> = plan.cells.iter().map(|c| (c.start, c.end)).collect();
        assert_eq!(ranges, vec![(15, 15), (14, 15), (15, 20), (15, 15)]);
        // Default group comes from the raw start: 16 is WHITE (14-17).
        assert_eq!(plan.cells[0].group, Some(Group::White));
        assert_eq!(plan.cells[2].group, Some(Group::White));
    }

    #[test]
    fn default_groups_and_overrides() {
        let starts = [1u64, 13, 14, 17, 18, 21, 0, 22, 255];
        let expected = [
            Some(Group::Classic),
            Some(Group::Classic),
            Some(Group::White),
            Some(Group::White),
            Some(Group::Gold),
            Some(Group::Gold),
            None,
            None,
            None,
        ];
        let filters = starts.iter().map(|&s| cell("A", s, "a")).collect();
        let (plan, _) = plan_of(block(json!(2), filters));
        let groups: Vec<Option<Group>> = plan.cells.iter().map(|c| c.group).collect();
        assert_eq!(groups, expected);

        let filters = vec![
            json!({ "label": "R", "series_start": 30, "texture": "r", "group": "gold" }),
            json!({ "label": "W", "series_start": 21, "texture": "w", "group": "none" }),
            json!({ "label": "C", "series_start": 18, "texture": "c", "group": "classic" }),
            json!({ "label": "H", "series_start": 1, "texture": "h", "group": "white" }),
        ];
        let (plan, warnings) = plan_of(block(json!(2), filters));
        assert!(warnings.is_empty());
        let groups: Vec<Option<Group>> = plan.cells.iter().map(|c| c.group).collect();
        assert_eq!(
            groups,
            vec![
                Some(Group::Gold),
                None,
                Some(Group::Classic),
                Some(Group::White)
            ]
        );
    }

    #[test]
    fn canonical_members_per_tab() {
        let plan = canonical();
        assert_eq!(plan.columns, 3);
        assert_eq!(plan.cells.len(), 20);
        assert_eq!(plan.members(Group::Gold as u32), vec![0, 1, 2, 3]);
        assert_eq!(plan.members(Group::White as u32), vec![4, 5, 6]);
        assert_eq!(
            plan.members(Group::Classic as u32),
            (7..20).collect::<Vec<u32>>()
        );
        assert!(plan.members(3).is_empty());
        assert_eq!(
            plan.summary(),
            "3 columns, 20 cells (GOLD 4, WHITE 3, CLASSIC 13, no group 0)"
        );
    }

    #[test]
    fn table_rows_pad_to_the_stock_count_plus_sentinel() {
        let inert = TableRow {
            start: i32::MAX,
            end_excl: 0,
            label: String::new(),
        };
        let rows = |n: usize| {
            let filters = (0..n)
                .map(|i| cell("A", (i % 21 + 1) as u64, "a"))
                .collect();
            plan_of(block(json!(3), filters)).0.table_rows()
        };
        assert_eq!(rows(0), vec![inert.clone(); 10]);
        assert_eq!(rows(1).len(), 10);
        assert_eq!(rows(9).len(), 10);
        assert_eq!(rows(20).len(), 21);
        assert_eq!(rows(64).len(), 65);
        assert_eq!(rows(20)[20], inert);
        assert_eq!(rows(1)[1..], vec![inert; 9][..]);

        let (plan, _) = plan_of(block(
            json!(2),
            vec![
                json!({ "label": "A LONG LABEL NAME", "series_start": 9, "series_end": 10, "texture": "sn" }),
            ],
        ));
        assert_eq!(
            plan.table_rows()[0],
            TableRow {
                start: 9,
                end_excl: 11,
                label: "A LONG LABEL NAME".into()
            }
        );
        let (plan, _) = plan_of(block(
            json!(2),
            vec![json!({ "label": "X", "series_start": 255, "texture": "x" })],
        ));
        assert_eq!(plan.table_rows()[0].end_excl, 256);
    }

    #[test]
    fn encode_row_layout() {
        let u64_at = |b: &[u8], at: usize| u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        let i32_at = |b: &[u8], at: usize| i32::from_le_bytes(b[at..at + 4].try_into().unwrap());

        let mut heap_calls = Vec::new();
        let short = TableRow {
            start: 21,
            end_excl: 22,
            label: "WORLD".into(),
        };
        let b = encode_row(&short, |t| {
            heap_calls.push(t.to_string());
            0
        });
        assert!(heap_calls.is_empty());
        assert_eq!(i32_at(&b, 0x30), 21);
        assert_eq!(i32_at(&b, 0x34), 22);
        // key: empty SSO
        assert_eq!(b[0x08], 0);
        assert_eq!(u64_at(&b, 0x18), 0);
        assert_eq!(u64_at(&b, 0x20), 15);
        // code + display: inline "WORLD"
        for at in [0x38, 0x60] {
            assert_eq!(&b[at..at + 5], b"WORLD");
            assert_eq!(b[at + 5], 0);
            assert_eq!(u64_at(&b, at + 0x10), 5);
            assert_eq!(u64_at(&b, at + 0x18), 15);
        }
        assert_eq!(u64_at(&b, 0x00), 0);

        let long = TableRow {
            start: 9,
            end_excl: 11,
            label: "SuperNOVA-SuperNOVA2".into(),
        };
        let b = encode_row(&long, |t| {
            assert_eq!(t, "SuperNOVA-SuperNOVA2");
            0xDEAD_BEEF
        });
        for at in [0x38, 0x60] {
            assert_eq!(u64_at(&b, at), 0xDEAD_BEEF);
            assert_eq!(u64_at(&b, at + 0x10), 20);
            assert_eq!(u64_at(&b, at + 0x18), 20);
        }

        let fifteen = TableRow {
            start: 0,
            end_excl: 1,
            label: "X3 VS 2ndMIX 12".into(),
        };
        let b = encode_row(&fifteen, |_| panic!("15 bytes must stay inline"));
        assert_eq!(u64_at(&b, 0x38 + 0x18), 15);

        let inert = encode_row(&TableRow::inert(), |_| {
            panic!("inert rows have no heap strings")
        });
        assert_eq!(i32_at(&inert, 0x30), i32::MAX);
        assert_eq!(i32_at(&inert, 0x34), 0);
    }

    #[test]
    fn custom_names_take_the_first_exact_single_value_cell() {
        let filters = vec![
            json!({ "label": "RANGE", "series_start": 30, "series_end": 31, "texture": "r" }),
            json!({ "label": "RUBY", "series_start": 30, "texture": "ruby" }),
            json!({ "label": "RUBY 2", "series_start": 30, "texture": "ruby2" }),
            json!({ "label": "SAPPHIRE", "series_start": 31, "texture": "sapphire" }),
            json!({ "label": "WORLD", "series_start": 21, "texture": "world" }),
        ];
        let (plan, _) = plan_of(block(json!(2), filters));
        assert_eq!(plan.custom_names(), vec![(30, "RUBY"), (31, "SAPPHIRE")]);
    }

    #[test]
    fn thumbnail_bounds() {
        assert_eq!(thumbnail_bound(|_| false), None);
        assert_eq!(thumbnail_bound(|n| n == 22), Some(22));
        assert_eq!(thumbnail_bound(|n| n == 22 || n == 30), Some(30));
        assert_eq!(thumbnail_bound(|n| n <= 21), None);
        assert_eq!(thumbnail_bound(|n| n >= 22), Some(127));
        for (max, want) in [
            (21u8, 21u8),
            (30, 30),
            (127, 127),
            (128, 127),
            (200, 127),
            (255, 127),
        ] {
            assert_eq!(legacy_thumbnail_bound(max), want);
        }
    }

    #[test]
    fn label_names_and_sources() {
        let filters = vec![
            cell("WORLD", 21, "world"),
            cell("WORLD AGAIN", 21, "world"),
            cell("A3", 20, "a3"),
        ];
        let (plan, _) = plan_of(block(json!(4), filters));
        assert_eq!(plan.label_key(0), "version_world_4col");
        assert_eq!(plan.label_key(2), "version_a3_4col");
        assert_eq!(texture_name("a3", 4), "sefi_version_a3_4col");
        assert_eq!(
            plan.label_stems(),
            vec![
                "sefi_version_world_4col".to_string(),
                "sefi_version_a3_4col".to_string()
            ]
        );
        assert_eq!(
            source_candidates("world_ruby", 3),
            [
                "sefi_version_world_ruby_3col.png".to_string(),
                "sefi_version_world_ruby.png".to_string()
            ]
        );
        let widths: Vec<u32> = (1..=5).map(canvas_width).collect();
        assert_eq!(widths, vec![220, 104, 64, 44, 32]);
        let donors: Vec<&str> = (1..=5).map(label_donor).collect();
        assert_eq!(
            donors,
            vec![
                "sefi_event_league",
                "sefi_version_world",
                "sefi_version_gold",
                "sefi_title_other",
                "sefi_level_00"
            ]
        );
    }
}
