//! Pure model for series_expansion's enhanced (config-defined) VERSION layout.
//!
//! Validates the untyped `series_expansion.custom_series_enhanced` JSON block
//! into an [`EnhancedPlan`] and derives everything the engine side needs from
//! it: the build order (GROUP tabs, cells and row breaks), the 0x88-stride
//! filter-table row values, GROUP-tab membership, where every cell lands for
//! scrolling, the per-song names for custom series, and the label textures.
//!
//! Rules (design R5–R8, R10, R11, R14; addendum §8):
//! - `num_columns` 1–5 (= the cells' template index); anything else ⇒ 2.
//!   `num_group_columns` 1–5 (the tabs' template); absent ⇒ 3, invalid ⇒ 3.
//! - `filters[]`: cells in display order. A cell needs `label` (non-empty
//!   ASCII, no NUL), `series_start` (0–255), optional `series_end` (0–255,
//!   default start, ≥ start), `texture` (`[a-z0-9_]+`) and, only with the
//!   stock tabs, an optional `group` (`gold|white|classic|none`).
//! - `groups[]` (optional): config-defined GROUP tabs, same shape minus
//!   `label` (`texture`, `series_start`, optional `series_end`). A tab selects
//!   every cell whose `series_start` lies in its range. Absent ⇒ the three
//!   stock tabs (stock art; membership by the cell's `group` or its start).
//! - Either list may hold row breaks: `{ "type": "BREAK", "thickness": N }`
//!   (`type` optional when `thickness` is given; thickness 0–255 px, default
//!   0). A break ends the current line and adds `thickness` px below it.
//! - Invalid entries are skipped, one warning each.
//! - Raw 16 is folded into 15 (the game maps 16 → 15 before any filter sees
//!   it and names both "DDR 2014"); the stock default group uses the raw start.
//! - The layout mirrors the item GridPanel's flow ([`Flow`]). Tabs stay fixed
//!   and must end by [`MAX_GROUP_BOTTOM`]; cells must start by
//!   [`MAX_CELL_TOP`] (lazy visuals, below) and number at most 64 (the saved
//!   u64). Anything past a limit is dropped with one warning.
//!
//! Dependency-free apart from `serde_json` (no `crate::` imports) so
//! `scripts/validate_series_expansion.sh` can mount it on the host.

use serde_json::{Map, Value};

/// Saved selections are one u64 per filter category: index = bit.
pub const MAX_CELLS: usize = 64;
/// Grid rows below one tab row whose buttons get a visual. The game creates
/// a FilterButton's movie only while its *layout* rect intersects the
/// virtual screen band (top < 864 in 1280×720 with the 20 % margin), and
/// scrolling moves BM2D layers, not layout. With the item grid's tab row at
/// y ≈ 220, entry row k's top is ≈ 246 + 26k, so rows 0..=23 qualify.
/// Cabinet 2026-09-27: 20 rows at one column all rendered and scrolled.
pub const MAX_GRID_ROWS: usize = 24;
pub const DEFAULT_COLUMNS: u8 = 2;
/// Stock tab template (three 72-px tabs fill the 216-px line).
pub const DEFAULT_GROUP_COLUMNS: u8 = 3;
/// Stock VERSION entry count; the table is padded to at least this many rows
/// (plus the sentinel) so a stale stock index can only ever hit an inert row.
pub const STOCK_ENTRY_COUNT: usize = 9;
/// First raw series value past the stock range (WORLD = 21).
pub const FIRST_CUSTOM_SERIES: u8 = 22;

/// Item GridPanel size (`filter_root` layer `switch_usr/dummy_choice_usr`).
pub const GRID_WIDTH: f64 = 216.0;
pub const GRID_HEIGHT: f64 = 266.0;
/// Height of every `filter_switch_base0N` template.
pub const CELL_HEIGHT: f64 = 26.0;
/// Lowest grid-relative top a cell may have and still get a visual: the
/// [`MAX_GRID_ROWS`]th row below one tab row (26 + 23 × 26 = 624).
pub const MAX_CELL_TOP: f64 = CELL_HEIGHT * MAX_GRID_ROWS as f64;
/// Tabs never scroll, so they must end by here — leaving at least one cell
/// row inside the item area below them.
pub const MAX_GROUP_BOTTOM: f64 = GRID_HEIGHT - CELL_HEIGHT;
/// Break thickness limit (px).
pub const MAX_BREAK_THICKNESS: u64 = 255;
/// Press id (`g`, handed to the tab factory) of the first config-defined tab.
/// Stock tabs keep the stock group index 0..=2; config tabs start above it
/// so the stock press body — which indexes the stock three-entry group table
/// — can never be handed one.
pub const CUSTOM_TAB_ID_BASE: i32 = 3;

/// Stock GROUP tab, numbered like the stock group table and the tab press's `g`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Classic = 0,
    White = 1,
    Gold = 2,
}

impl Group {
    pub fn from_index(g: i32) -> Option<Group> {
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

    /// Inclusive raw span of the stock tab.
    fn span(self) -> (u8, u8) {
        match self {
            Group::Classic => (1, 13),
            Group::White => (14, 17),
            Group::Gold => (18, 21),
        }
    }
}

/// Stock tabs in stock creation order.
const STOCK_TABS: [Group; 3] = [Group::Gold, Group::White, Group::Classic];

/// One validated cell. `start`/`end` are inclusive, already normalised
/// (no 16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub label: String,
    pub start: u8,
    pub end: u8,
    pub texture: String,
    /// Stock-tab membership (stock tabs only; `None` with config tabs).
    pub group: Option<Group>,
}

/// One GROUP tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    /// `FilterButton+0xC8` label key (texture = `sefi_` + key).
    pub label_key: String,
    /// Label art to prepare; `None` for the stock tab art.
    pub texture: Option<String>,
    /// `g` handed to the tab factory; the press detour keys membership on it.
    pub press_id: i32,
    /// Inclusive, normalised series range the tab selects (config tabs: the
    /// cells whose `start` lies inside; stock tabs: informational).
    pub start: u8,
    pub end: u8,
}

/// One item-grid child, in creation (= layout) order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// `tabs[i]`.
    Tab(usize),
    /// `cells[i]` (selection index `i`).
    Cell(usize),
    /// Full-width invisible spacer this many px tall (a forced line break).
    Break(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnhancedPlan {
    /// 1–5; the cells' FilterButton template index.
    pub columns: u8,
    /// 1–5; the tabs' template index.
    pub group_columns: u8,
    /// `groups` was given: tabs are config-defined and select by range.
    pub custom_groups: bool,
    pub tabs: Vec<Tab>,
    /// Display order; the index is the selection index and saved bit.
    pub cells: Vec<Cell>,
    /// Build order: tabs and their breaks, then cells and theirs.
    pub slots: Vec<Slot>,
}

/// Where a cell sits for scrolling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellPlacement {
    /// Cell line, counted from the first cell line (0).
    pub row: usize,
    /// Top in px relative to the first cell line's top.
    pub top: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollLayout {
    /// Per cell, in selection-index order.
    pub cells: Vec<CellPlacement>,
    /// Px from the first cell line's top to the item area's bottom (at
    /// least one row): a line is shown while its bottom stays inside.
    pub viewport: f64,
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

/// Cell width of template `columns` (`filter_switch_base0N`: 220/108/72/54/42).
pub fn template_width(columns: u8) -> f64 {
    match columns {
        1 => 220.0,
        2 => 108.0,
        3 => 72.0,
        4 => 54.0,
        _ => 42.0,
    }
}

/// The item GridPanel's flow layout (`FUN_18004AD10`; gaps 0, left-aligned):
/// a child that would overflow the line starts the next one, which begins
/// below the tallest child of the previous line. A full-width child
/// therefore ends its line and pushes the next child onto another one.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flow {
    cursor: f64,
    line_height: f64,
    line_top: f64,
}

impl Flow {
    /// Top of a `width`-wide child placed next.
    pub fn peek(&self, width: f64) -> f64 {
        if self.cursor + width > GRID_WIDTH {
            self.line_top + self.line_height
        } else {
            self.line_top
        }
    }

    /// Place a `width` × `height` child; returns its top.
    pub fn place(&mut self, width: f64, height: f64) -> f64 {
        if self.cursor + width > GRID_WIDTH {
            self.line_top += self.line_height;
            self.cursor = 0.0;
            self.line_height = 0.0;
        }
        self.cursor += width;
        self.line_height = self.line_height.max(height);
        self.line_top
    }

    /// Whether a `width`-wide child would join the current (non-empty) line.
    fn joins_line(&self, width: f64) -> bool {
        self.cursor > 0.0 && self.cursor + width <= GRID_WIDTH
    }
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

/// In-game texture name of a label at a column count.
pub fn texture_name(texture: &str, columns: u8) -> String {
    format!("sefi_version_{}_{}col", texture, columns)
}

/// `FilterButton+0xC8` key for a label (texture = `sefi_` + key).
fn label_key_for(texture: &str, columns: u8) -> String {
    format!("version_{}_{}col", texture, columns)
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

fn column_count(
    obj: &Map<String, Value>,
    key: &str,
    fallback: u8,
    warnings: &mut Vec<String>,
) -> u8 {
    match obj.get(key) {
        Some(v) => match v.as_u64() {
            Some(n) if (1..=5).contains(&n) => n as u8,
            _ => {
                warnings.push(format!(
                    "{} {} is not an integer 1-5 — using {}",
                    key, v, fallback
                ));
                fallback
            }
        },
        None => fallback,
    }
}

/// One `groups[]` / `filters[]` entry before item validation.
enum Entry<'a> {
    Break(u8),
    Item(&'a Map<String, Value>),
}

/// Classify entry `i` of `list`: a row break is `{ "type": "BREAK",
/// "thickness": N }` — `type` (any case) may be omitted when `thickness` is
/// present; `thickness` defaults to 0.
fn entry<'a>(list: &str, i: usize, v: &'a Value) -> Result<Entry<'a>, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("{}[{}]: not an object — entry skipped", list, i))?;
    let is_break = match obj.get("type") {
        None => obj.contains_key("thickness"),
        Some(Value::String(t)) if t.eq_ignore_ascii_case("break") => true,
        Some(t) => {
            return Err(format!(
                "{}[{}]: unknown type {} (only \"BREAK\") — entry skipped",
                list, i, t
            ))
        }
    };
    if !is_break {
        return Ok(Entry::Item(obj));
    }
    match obj.get("thickness") {
        None => Ok(Entry::Break(0)),
        Some(t) => match t.as_u64() {
            Some(n) if n <= MAX_BREAK_THICKNESS => Ok(Entry::Break(n as u8)),
            _ => Err(format!(
                "{}[{}]: break thickness {} is not an integer 0-{} — break skipped",
                list, i, t, MAX_BREAK_THICKNESS
            )),
        },
    }
}

/// `series_start` / `series_end` / `texture`, shared by cells and tabs.
/// Returns raw (unfolded) start and end.
fn range_and_texture(
    list: &str,
    i: usize,
    obj: &Map<String, Value>,
) -> Result<(u8, u8, String), String> {
    let start = series_value(obj.get("series_start"))
        .map_err(|e| format!("{}[{}]: series_start {} — entry skipped", list, i, e))?
        .ok_or_else(|| format!("{}[{}]: series_start missing — entry skipped", list, i))?;
    let end = series_value(obj.get("series_end"))
        .map_err(|e| format!("{}[{}]: series_end {} — entry skipped", list, i, e))?
        .unwrap_or(start);
    if start > end {
        return Err(format!(
            "{}[{}]: series_start {} > series_end {} — entry skipped",
            list, i, start, end
        ));
    }
    let texture = match obj.get("texture").and_then(Value::as_str) {
        Some(t) if is_texture_key(t) => t.to_string(),
        other => {
            return Err(format!(
                "{}[{}]: texture {:?} must match [a-z0-9_]+ — entry skipped",
                list, i, other
            ))
        }
    };
    Ok((start, end, texture))
}

/// A cell. `stock_groups`: parse the per-cell `group` override (stock tabs);
/// otherwise the caller has already reported any `group` key as ignored.
fn parse_cell(i: usize, obj: &Map<String, Value>, stock_groups: bool) -> Result<Cell, String> {
    let label = match obj.get("label").and_then(Value::as_str) {
        Some(l) if !l.trim().is_empty() && l.is_ascii() && !l.contains('\0') => l.to_string(),
        _ => {
            return Err(format!(
                "filters[{}]: label must be non-empty ASCII text — cell skipped",
                i
            ))
        }
    };
    let (start, end, texture) = range_and_texture("filters", i, obj)?;
    let group = if !stock_groups {
        None
    } else {
        match obj.get("group") {
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
        }
    };
    Ok(Cell {
        label,
        start: fold_sixteen(start),
        end: fold_sixteen(end),
        texture,
        group,
    })
}

/// A config-defined tab (`label` is not used: tabs only select cells).
fn parse_tab(
    i: usize,
    obj: &Map<String, Value>,
    group_columns: u8,
    press_id: i32,
) -> Result<Tab, String> {
    let (start, end, texture) = range_and_texture("groups", i, obj)?;
    Ok(Tab {
        label_key: label_key_for(&texture, group_columns),
        texture: Some(texture),
        press_id,
        start: fold_sixteen(start),
        end: fold_sixteen(end),
    })
}

fn stock_tab(group: Group) -> Tab {
    let (start, end) = group.span();
    Tab {
        label_key: format!("version_{}", group.key()),
        texture: None,
        press_id: group as i32,
        start,
        end,
    }
}

/// Validate a `custom_series_enhanced` value. `Err` only when it is not a
/// JSON object (the caller falls back); otherwise returns the plan and one
/// warning per defaulted field, skipped entry or dropped overflow.
pub fn parse(v: &Value) -> Result<(EnhancedPlan, Vec<String>), String> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("custom_series_enhanced must be an object, got {}", v))?;
    let mut warnings = Vec::new();

    let columns = match obj.get("num_columns") {
        None => {
            warnings.push(format!("num_columns missing — using {}", DEFAULT_COLUMNS));
            DEFAULT_COLUMNS
        }
        Some(_) => column_count(obj, "num_columns", DEFAULT_COLUMNS, &mut warnings),
    };
    let group_columns = column_count(
        obj,
        "num_group_columns",
        DEFAULT_GROUP_COLUMNS,
        &mut warnings,
    );
    let group_entries = match obj.get("groups") {
        None => None,
        Some(Value::Array(list)) => Some(list),
        Some(other) => {
            warnings.push(format!(
                "groups {} is not a list — using the stock GROUP tabs",
                other
            ));
            None
        }
    };
    let custom_groups = group_entries.is_some();

    let mut flow = Flow::default();
    let mut slots = Vec::new();
    let mut tabs: Vec<Tab> = Vec::new();

    // ── GROUP tabs (fixed; never scrolled) ──
    let tab_width = template_width(group_columns);
    match group_entries {
        None => {
            for group in STOCK_TABS {
                flow.place(tab_width, CELL_HEIGHT);
                slots.push(Slot::Tab(tabs.len()));
                tabs.push(stock_tab(group));
            }
        }
        Some(list) => {
            // (first dropped entry, entries dropped)
            let mut overflow: Option<(usize, usize)> = None;
            for (i, value) in list.iter().enumerate() {
                let parsed = entry("groups", i, value);
                if let Some((_, dropped)) = overflow.as_mut() {
                    if parsed.is_ok() {
                        *dropped += 1;
                    }
                    continue;
                }
                match parsed {
                    Err(w) => warnings.push(w),
                    Ok(Entry::Break(t)) => {
                        flow.place(GRID_WIDTH, t as f64);
                        slots.push(Slot::Break(t));
                    }
                    Ok(Entry::Item(item)) => {
                        let press_id = CUSTOM_TAB_ID_BASE + tabs.len() as i32;
                        match parse_tab(i, item, group_columns, press_id) {
                            Err(w) => warnings.push(w),
                            Ok(_) if flow.peek(tab_width) + CELL_HEIGHT > MAX_GROUP_BOTTOM => {
                                overflow = Some((i, 1));
                            }
                            Ok(tab) => {
                                flow.place(tab_width, CELL_HEIGHT);
                                slots.push(Slot::Tab(tabs.len()));
                                tabs.push(tab);
                            }
                        }
                    }
                }
            }
            if let Some((first, dropped)) = overflow {
                warnings.push(format!(
                    "groups from entry {} on would end below {} px of the {}-px item area (tabs never scroll) — {} entr{} dropped",
                    first,
                    MAX_GROUP_BOTTOM,
                    GRID_HEIGHT,
                    dropped,
                    if dropped == 1 { "y" } else { "ies" }
                ));
            }
        }
    }

    // A cell that would share the last tab line starts its own instead.
    let cell_width = template_width(columns);
    if !tabs.is_empty() && flow.joins_line(cell_width) {
        flow.place(GRID_WIDTH, 0.0);
        slots.push(Slot::Break(0));
    }

    // ── cells ──
    let mut cells = Vec::new();
    match obj.get("filters").and_then(Value::as_array) {
        Some(list) => {
            let mut overflow: Option<(usize, String)> = None;
            for (i, value) in list.iter().enumerate() {
                let parsed = entry("filters", i, value);
                if let Some((dropped, _)) = overflow.as_mut() {
                    if parsed.is_ok() {
                        *dropped += 1;
                    }
                    continue;
                }
                match parsed {
                    Err(w) => warnings.push(w),
                    Ok(Entry::Break(t)) => {
                        flow.place(GRID_WIDTH, t as f64);
                        slots.push(Slot::Break(t));
                    }
                    Ok(Entry::Item(item)) => {
                        if custom_groups && item.contains_key("group") {
                            warnings.push(format!(
                                "filters[{}]: group is ignored with config groups (tabs select by series range)",
                                i
                            ));
                        }
                        match parse_cell(i, item, !custom_groups) {
                            Err(w) => warnings.push(w),
                            Ok(_) if cells.len() >= MAX_CELLS => {
                                overflow = Some((
                                    1,
                                    format!("filters past the {}th cell exceed the saved-filter limit of {} cells", MAX_CELLS, MAX_CELLS),
                                ));
                            }
                            Ok(_) if flow.peek(cell_width) > MAX_CELL_TOP => {
                                overflow = Some((
                                    1,
                                    format!(
                                        "filters from entry {} on would start below {} px, past the lowest row the game draws",
                                        i, MAX_CELL_TOP
                                    ),
                                ));
                            }
                            Ok(cell) => {
                                flow.place(cell_width, CELL_HEIGHT);
                                slots.push(Slot::Cell(cells.len()));
                                cells.push(cell);
                            }
                        }
                    }
                }
            }
            if let Some((dropped, why)) = overflow {
                warnings.push(format!(
                    "{} — {} entr{} dropped",
                    why,
                    dropped,
                    if dropped == 1 { "y" } else { "ies" }
                ));
            }
        }
        None => warnings.push("filters is missing or not a list — no cells".into()),
    }

    Ok((
        EnhancedPlan {
            columns,
            group_columns,
            custom_groups,
            tabs,
            cells,
            slots,
        },
        warnings,
    ))
}

impl EnhancedPlan {
    fn slot_size(&self, slot: Slot) -> (f64, f64) {
        match slot {
            Slot::Tab(_) => (template_width(self.group_columns), CELL_HEIGHT),
            Slot::Cell(_) => (template_width(self.columns), CELL_HEIGHT),
            Slot::Break(t) => (GRID_WIDTH, t as f64),
        }
    }

    /// Grid-relative top of every slot, as the game's flow layout places it.
    pub fn slot_tops(&self) -> Vec<f64> {
        let mut flow = Flow::default();
        self.slots
            .iter()
            .map(|&s| {
                let (w, h) = self.slot_size(s);
                flow.place(w, h)
            })
            .collect()
    }

    /// Scroll rows and tops of every cell, and the viewport below the tabs.
    pub fn scroll_layout(&self) -> ScrollLayout {
        let mut cells = vec![CellPlacement { row: 0, top: 0.0 }; self.cells.len()];
        let mut origin: Option<f64> = None;
        let mut row = 0usize;
        let mut last_top = 0.0f64;
        for (&slot, top) in self.slots.iter().zip(self.slot_tops()) {
            let Slot::Cell(i) = slot else { continue };
            match origin {
                None => origin = Some(top),
                Some(_) if top > last_top => row += 1,
                Some(_) => {}
            }
            last_top = top;
            if let Some(p) = cells.get_mut(i) {
                *p = CellPlacement {
                    row,
                    top: top - origin.unwrap_or(top),
                };
            }
        }
        let viewport = (GRID_HEIGHT - origin.unwrap_or(0.0)).max(CELL_HEIGHT);
        ScrollLayout { cells, viewport }
    }

    /// Cell (selection) indices a press of the tab with id `press_id` selects:
    /// config tabs by range (`start` inside the tab's), stock tabs by the
    /// cell's `group`.
    pub fn members(&self, press_id: i32) -> Vec<u32> {
        let Some(tab) = self.tabs.iter().find(|t| t.press_id == press_id) else {
            return Vec::new();
        };
        let stock = Group::from_index(press_id);
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                if self.custom_groups {
                    tab.start <= c.start && c.start <= tab.end
                } else {
                    stock.is_some() && c.group == stock
                }
            })
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// [`members`](Self::members) indexed by press id (0 up to the highest).
    pub fn members_by_press_id(&self) -> Vec<Vec<i32>> {
        let len = self
            .tabs
            .iter()
            .map(|t| t.press_id.max(0) as usize + 1)
            .max()
            .unwrap_or(0);
        (0..len)
            .map(|id| {
                self.members(id as i32)
                    .into_iter()
                    .map(|i| i as i32)
                    .collect()
            })
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
            Some(c) => label_key_for(&c.texture, self.columns),
            None => String::new(),
        }
    }

    /// Label art to prepare — `(texture, column count)`, distinct, in
    /// first-use order: config tabs at `group_columns`, then cells at
    /// `columns`. Stock tabs use the stock art and are not listed.
    pub fn label_textures(&self) -> Vec<(String, u8)> {
        let tabs = self
            .tabs
            .iter()
            .filter_map(|t| t.texture.clone().map(|tex| (tex, self.group_columns)));
        let cells = self.cells.iter().map(|c| (c.texture.clone(), self.columns));
        let mut out: Vec<(String, u8)> = Vec::new();
        for pair in tabs.chain(cells) {
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
        out
    }

    /// Distinct in-game label texture names, in first-use order.
    pub fn label_stems(&self) -> Vec<String> {
        self.label_textures()
            .iter()
            .map(|(tex, cols)| texture_name(tex, *cols))
            .collect()
    }

    /// One-line description for the boot log.
    pub fn summary(&self) -> String {
        let breaks = self
            .slots
            .iter()
            .filter(|s| matches!(s, Slot::Break(_)))
            .count();
        let tabs = if self.custom_groups {
            let members: Vec<String> = self
                .tabs
                .iter()
                .map(|t| self.members(t.press_id).len().to_string())
                .collect();
            format!(
                "{} config GROUP tab(s) at {} columns (members {})",
                self.tabs.len(),
                self.group_columns,
                if members.is_empty() {
                    "-".to_string()
                } else {
                    members.join("/")
                }
            )
        } else {
            let count = |g: Option<Group>| self.cells.iter().filter(|c| c.group == g).count();
            format!(
                "stock GROUP tabs at {} columns (GOLD {}, WHITE {}, CLASSIC {}, no group {})",
                self.group_columns,
                count(Some(Group::Gold)),
                count(Some(Group::White)),
                count(Some(Group::Classic)),
                count(None)
            )
        };
        format!(
            "{} columns, {} cells, {} break(s), {}",
            self.columns,
            self.cells.len(),
            breaks,
            tabs
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

    fn brk(thickness: u64) -> serde_json::Value {
        json!({ "type": "BREAK", "thickness": thickness })
    }

    fn tab(texture: &str, start: u64, end: u64) -> serde_json::Value {
        json!({ "texture": texture, "series_start": start, "series_end": end })
    }

    fn block(columns: serde_json::Value, filters: Vec<serde_json::Value>) -> serde_json::Value {
        json!({ "num_columns": columns, "filters": filters })
    }

    fn plan_of(v: serde_json::Value) -> (EnhancedPlan, Vec<String>) {
        parse(&v).expect("object")
    }

    fn canonical_value() -> serde_json::Value {
        let text = include_str!("testdata/canonical_enhanced.json");
        let root: serde_json::Value = serde_json::from_str(text).expect("fixture json");
        root["custom_series_enhanced"].clone()
    }

    fn canonical() -> EnhancedPlan {
        let (plan, warnings) = parse(&canonical_value()).expect("object");
        assert!(
            warnings.is_empty(),
            "canonical fixture warnings: {warnings:?}"
        );
        plan
    }

    /// The canonical fixture without its `groups` (stock tabs).
    fn canonical_stock() -> EnhancedPlan {
        let mut v = canonical_value();
        let obj = v.as_object_mut().expect("object");
        obj.remove("groups");
        obj.remove("num_group_columns");
        let (plan, warnings) = parse(&v).expect("object");
        assert!(warnings.is_empty(), "{warnings:?}");
        plan
    }

    fn rows(plan: &EnhancedPlan) -> Vec<(usize, f64)> {
        plan.scroll_layout()
            .cells
            .iter()
            .map(|p| (p.row, p.top))
            .collect()
    }

    #[test]
    fn minimal_cell_defaults_end_to_start() {
        let (plan, warnings) = plan_of(block(json!(3), vec![cell("WORLD", 21, "world")]));
        assert!(warnings.is_empty());
        assert_eq!(plan.columns, 3);
        assert_eq!(plan.group_columns, DEFAULT_GROUP_COLUMNS);
        assert!(!plan.custom_groups);
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
        // Stock tabs fill their line: no separator before the first cell.
        assert_eq!(
            plan.slots,
            vec![Slot::Tab(0), Slot::Tab(1), Slot::Tab(2), Slot::Cell(0)]
        );
    }

    #[test]
    fn empty_filters_and_non_objects() {
        let (plan, warnings) = plan_of(block(json!(2), vec![]));
        assert!(plan.cells.is_empty() && warnings.is_empty());
        assert_eq!(plan.tabs.len(), 3);
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
    fn num_group_columns_validation() {
        for good in 1..=5u64 {
            let (plan, warnings) =
                plan_of(json!({ "num_columns": 2, "num_group_columns": good, "filters": [] }));
            assert_eq!(plan.group_columns as u64, good);
            assert!(warnings.is_empty());
        }
        for bad in [json!(0), json!(9), json!("4"), json!(1.5)] {
            let (plan, warnings) =
                plan_of(json!({ "num_columns": 2, "num_group_columns": bad, "filters": [] }));
            assert_eq!(plan.group_columns, DEFAULT_GROUP_COLUMNS, "{bad}");
            assert_eq!(warnings.len(), 1, "{bad}");
        }
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
            json!({ "label": "A", "series_start": 3, "texture": "a", "type": "FILTER" }),
            json!({ "type": "BREAK", "thickness": 256 }),
            json!({ "type": "BREAK", "thickness": -1 }),
            json!({ "thickness": 2.5 }),
            json!({ "type": 7 }),
            json!(42),
        ];
        let count = bad.len();
        let mut filters = bad;
        filters.push(cell("OK", 3, "ok"));
        let (plan, warnings) = plan_of(block(json!(2), filters));
        assert_eq!(plan.cells.len(), 1);
        assert_eq!(plan.cells[0].texture, "ok");
        assert_eq!(warnings.len(), count, "{warnings:#?}");
        assert!(!plan.slots.iter().any(|s| matches!(s, Slot::Break(_))));
    }

    #[test]
    fn break_forms() {
        let filters = vec![
            cell("A", 1, "a"),
            json!({ "type": "BREAK", "thickness": 5 }),
            cell("B", 2, "b"),
            json!({ "type": "break" }),
            cell("C", 3, "c"),
            json!({ "thickness": 12 }),
            cell("D", 4, "d"),
            json!({ "type": "Break", "thickness": 0 }),
        ];
        let (plan, warnings) = plan_of(block(json!(3), filters));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(plan.cells.len(), 4);
        assert_eq!(
            plan.slots[3..],
            [
                Slot::Cell(0),
                Slot::Break(5),
                Slot::Cell(1),
                Slot::Break(0),
                Slot::Cell(2),
                Slot::Break(12),
                Slot::Cell(3),
                Slot::Break(0),
            ]
        );
    }

    #[test]
    fn flow_mirrors_the_grid_panel() {
        let mut flow = Flow::default();
        // 220-px template 1 overflows 216 on its own: each item wraps, the
        // first one to y 0.
        assert_eq!(flow.place(220.0, 26.0), 0.0);
        assert_eq!(flow.place(220.0, 26.0), 26.0);
        // A full-width break after a full line starts the next line.
        assert_eq!(flow.place(216.0, 5.0), 52.0);
        assert_eq!(flow.place(108.0, 26.0), 57.0);
        assert_eq!(flow.place(108.0, 26.0), 57.0);
        assert_eq!(flow.peek(42.0), 83.0);
        // Consecutive breaks add up; a zero break adds nothing.
        let mut flow = Flow::default();
        flow.place(216.0, 3.0);
        flow.place(216.0, 0.0);
        flow.place(216.0, 4.0);
        assert_eq!(flow.place(72.0, 26.0), 7.0);
    }

    #[test]
    fn breaks_end_rows_and_add_height() {
        // 3 columns: A B C | D (break 5) | E F (break 0) | G
        let filters = vec![
            cell("A", 1, "a"),
            cell("B", 2, "b"),
            cell("C", 3, "c"),
            cell("D", 4, "d"),
            brk(5),
            cell("E", 5, "e"),
            cell("F", 6, "f"),
            brk(0),
            cell("G", 7, "g"),
        ];
        let (plan, warnings) = plan_of(block(json!(3), filters));
        assert!(warnings.is_empty());
        assert_eq!(
            rows(&plan),
            vec![
                (0, 0.0),
                (0, 0.0),
                (0, 0.0),
                (1, 26.0),
                (2, 57.0),
                (2, 57.0),
                (3, 83.0)
            ]
        );
        // Tabs occupy 0..26; the first cell line starts right below.
        assert_eq!(plan.scroll_layout().viewport, GRID_HEIGHT - 26.0);
        // A break after an already-full line only adds its thickness.
        let (plan, _) = plan_of(block(
            json!(3),
            vec![
                cell("A", 1, "a"),
                cell("B", 2, "b"),
                cell("C", 3, "c"),
                brk(0),
                cell("D", 4, "d"),
            ],
        ));
        assert_eq!(rows(&plan)[3], (1, 26.0));
    }

    #[test]
    fn leading_breaks_move_the_scroll_origin() {
        let (plan, _) = plan_of(block(json!(2), vec![brk(10), cell("A", 1, "a")]));
        let layout = plan.scroll_layout();
        assert_eq!(layout.cells, vec![CellPlacement { row: 0, top: 0.0 }]);
        assert_eq!(layout.viewport, GRID_HEIGHT - 36.0);
        assert_eq!(plan.slot_tops(), vec![0.0, 0.0, 0.0, 26.0, 36.0]);
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
        let (plan, warnings) = plan_of(block(json!(3), many(64)));
        assert_eq!(plan.cells.len(), MAX_CELLS);
        assert!(warnings.is_empty());
    }

    #[test]
    fn cell_cap_counts_breaks_as_height() {
        // 1 column: 20 rows fit below the tabs (tops 26..=520); a 100-px
        // break pushes row k to 126 + 26k, so only rows up to 624 remain.
        let mut filters: Vec<serde_json::Value> = vec![brk(100)];
        filters.extend((0..24).map(|i| cell("A", (i % 21 + 1) as u64, "a")));
        let (plan, warnings) = plan_of(block(json!(1), filters));
        assert_eq!(plan.cells.len(), 20);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let last = plan.slot_tops().into_iter().last().expect("slots");
        assert!(last <= MAX_CELL_TOP, "{last}");
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
    fn stock_tabs_members_and_keys() {
        let plan = canonical_stock();
        assert!(!plan.custom_groups);
        assert_eq!(plan.columns, 3);
        assert_eq!(plan.cells.len(), 20);
        let keys: Vec<(&str, i32)> = plan
            .tabs
            .iter()
            .map(|t| (t.label_key.as_str(), t.press_id))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("version_gold", 2),
                ("version_white", 1),
                ("version_classic", 0)
            ]
        );
        assert!(plan.tabs.iter().all(|t| t.texture.is_none()));
        assert_eq!(plan.members(Group::Gold as i32), vec![0, 1, 2, 3]);
        assert_eq!(plan.members(Group::White as i32), vec![4, 5, 6]);
        assert_eq!(
            plan.members(Group::Classic as i32),
            (7..20).collect::<Vec<u32>>()
        );
        assert!(plan.members(3).is_empty());
        let by_id = plan.members_by_press_id();
        assert_eq!(by_id.len(), 3);
        assert_eq!(by_id[2], vec![0, 1, 2, 3]);
        assert_eq!(
            plan.summary(),
            "3 columns, 20 cells, 0 break(s), stock GROUP tabs at 3 columns (GOLD 4, WHITE 3, CLASSIC 13, no group 0)"
        );
    }

    #[test]
    fn canonical_config_tabs_match_the_stock_membership() {
        let plan = canonical();
        let stock = canonical_stock();
        assert!(plan.custom_groups);
        assert_eq!(plan.group_columns, 4);
        assert_eq!(plan.cells, {
            let mut cells = stock.cells.clone();
            for c in &mut cells {
                c.group = None;
            }
            cells
        });
        let keys: Vec<(&str, i32)> = plan
            .tabs
            .iter()
            .map(|t| (t.label_key.as_str(), t.press_id))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("version_group_gold_4col", 3),
                ("version_group_white_4col", 4),
                ("version_group_classic_4col", 5),
                ("version_no_flare_4col", 6)
            ]
        );
        for (tab, group) in plan.tabs.iter().zip(STOCK_TABS) {
            assert_eq!(plan.members(tab.press_id), stock.members(group as i32));
        }
        // NO FLARE (22-255): no canonical cell to select.
        assert!(plan.members(6).is_empty());
        let by_id = plan.members_by_press_id();
        assert_eq!(by_id.len(), 7);
        assert!(by_id[..3].iter().all(Vec::is_empty));
        // Four 54-px tabs fill the line like the stock three: no separator,
        // cells at y 26, same scroll rows.
        assert!(!plan.slots.iter().any(|s| matches!(s, Slot::Break(_))));
        assert_eq!(plan.slot_tops()[4], 26.0);
        assert_eq!(rows(&plan), rows(&stock));
        assert_eq!(
            plan.scroll_layout().viewport,
            stock.scroll_layout().viewport
        );
        assert_eq!(
            plan.summary(),
            "3 columns, 20 cells, 0 break(s), 4 config GROUP tab(s) at 4 columns (members 4/3/13/0)"
        );
    }

    #[test]
    fn config_tabs_select_by_range_and_may_overlap() {
        let v = json!({
            "num_columns": 2,
            "num_group_columns": 4,
            "groups": [
                tab("gold", 18, 21),
                { "texture": "all", "series_start": 0, "series_end": 255, "label": "ignored" },
                tab("no_flare", 22, 255),
                tab("sixteen", 16, 16)
            ],
            "filters": [
                cell("WORLD", 21, "world"),
                cell("RUBY", 30, "ruby"),
                json!({ "label": "2014", "series_start": 15, "series_end": 16, "texture": "2014" }),
                json!({ "label": "X", "series_start": 11, "texture": "x", "group": "gold" }),
            ]
        });
        let (plan, warnings) = plan_of(v);
        assert_eq!(warnings.len(), 1, "{warnings:?}"); // the ignored `group`
        assert!(plan.cells.iter().all(|c| c.group.is_none()));
        let ids: Vec<i32> = plan.tabs.iter().map(|t| t.press_id).collect();
        assert_eq!(ids, vec![3, 4, 5, 6]);
        assert_eq!(plan.members(3), vec![0]);
        assert_eq!(plan.members(4), vec![0, 1, 2, 3]);
        assert_eq!(plan.members(5), vec![1]);
        // 16 folds to 15 on both sides.
        assert_eq!(plan.members(6), vec![2]);
        assert!(plan.members(2).is_empty());
        // Four 54-px tabs fill 216: the first cell wraps on its own.
        assert_eq!(plan.slots[4], Slot::Cell(0));
    }

    #[test]
    fn config_tabs_wrap_breaks_and_separator() {
        // Four tabs at 3 columns: two tab lines; the fourth leaves room on
        // its line, so a 0-px separator keeps the first cell off it.
        let v = json!({
            "num_columns": 5,
            "groups": [tab("a", 1, 1), tab("b", 2, 2), tab("c", 3, 3), tab("d", 4, 4)],
            "filters": [cell("A", 1, "a"), cell("B", 2, "b")]
        });
        let (plan, warnings) = plan_of(v);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            plan.slots,
            vec![
                Slot::Tab(0),
                Slot::Tab(1),
                Slot::Tab(2),
                Slot::Tab(3),
                Slot::Break(0),
                Slot::Cell(0),
                Slot::Cell(1),
            ]
        );
        // The separator starts a new line below the fourth tab's.
        assert_eq!(
            plan.slot_tops(),
            vec![0.0, 0.0, 0.0, 26.0, 52.0, 52.0, 52.0]
        );
        assert_eq!(plan.scroll_layout().viewport, GRID_HEIGHT - 52.0);

        // A break inside the groups and one ending them (no separator added).
        let v = json!({
            "num_columns": 5,
            "num_group_columns": 5,
            "groups": [tab("a", 1, 1), brk(4), tab("b", 2, 2), brk(3)],
            "filters": [cell("A", 1, "a")]
        });
        let (plan, _) = plan_of(v);
        assert_eq!(
            plan.slots,
            vec![
                Slot::Tab(0),
                Slot::Break(4),
                Slot::Tab(1),
                Slot::Break(3),
                Slot::Cell(0)
            ]
        );
        assert_eq!(plan.slot_tops(), vec![0.0, 26.0, 30.0, 56.0, 59.0]);
    }

    #[test]
    fn empty_groups_mean_no_tabs() {
        let v = json!({ "num_columns": 2, "groups": [], "filters": [cell("A", 1, "a")] });
        let (plan, warnings) = plan_of(v);
        assert!(warnings.is_empty());
        assert!(plan.custom_groups && plan.tabs.is_empty());
        assert_eq!(plan.slots, vec![Slot::Cell(0)]);
        assert_eq!(plan.scroll_layout().viewport, GRID_HEIGHT);
        assert!(plan.members_by_press_id().is_empty());
        assert!(plan.label_stems() == vec!["sefi_version_a_2col".to_string()]);
    }

    #[test]
    fn bad_groups_fall_back_to_stock_tabs() {
        let v = json!({ "num_columns": 2, "groups": { "a": 1 }, "filters": [] });
        let (plan, warnings) = plan_of(v);
        assert_eq!(warnings.len(), 1);
        assert!(!plan.custom_groups);
        assert_eq!(plan.tabs.len(), 3);

        let v = json!({
            "num_columns": 2,
            "groups": [
                { "series_start": 1, "texture": "Bad" },
                { "texture": "a" },
                { "texture": "a", "series_start": 9, "series_end": 3 },
                tab("ok", 1, 13)
            ],
            "filters": []
        });
        let (plan, warnings) = plan_of(v);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert_eq!(plan.tabs.len(), 1);
        assert_eq!(plan.tabs[0].press_id, CUSTOM_TAB_ID_BASE);
    }

    #[test]
    fn tabs_past_the_fixed_area_are_dropped() {
        // 1 column: tab lines at 0, 26, … 208 end by 234 <= 240; the tenth
        // (top 234) would end at 260.
        let groups: Vec<serde_json::Value> = (0..12)
            .map(|i| tab("t", i as u64 + 1, i as u64 + 1))
            .collect();
        let v = json!({ "num_columns": 2, "num_group_columns": 1, "groups": groups, "filters": [cell("A", 1, "a")] });
        let (plan, warnings) = plan_of(v);
        assert_eq!(plan.tabs.len(), 9);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let layout = plan.scroll_layout();
        assert_eq!(layout.viewport, GRID_HEIGHT - 234.0);

        // A huge group break never leaves the cells without a viewport row.
        let v = json!({
            "num_columns": 2,
            "groups": [brk(255), tab("t", 1, 1)],
            "filters": [cell("A", 1, "a")]
        });
        let (plan, warnings) = plan_of(v);
        assert!(plan.tabs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert_eq!(plan.scroll_layout().viewport, CELL_HEIGHT);
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
        let cells: Vec<f64> = (1..=5).map(template_width).collect();
        assert_eq!(cells, vec![220.0, 108.0, 72.0, 54.0, 42.0]);
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

    #[test]
    fn tab_labels_use_the_group_width() {
        let v = json!({
            "num_columns": 2,
            "num_group_columns": 4,
            "groups": [tab("world", 21, 21), tab("group_gold", 18, 21)],
            "filters": [cell("WORLD", 21, "world"), cell("A3", 20, "a3")]
        });
        let (plan, _) = plan_of(v);
        assert_eq!(plan.tabs[0].label_key, "version_world_4col");
        assert_eq!(
            plan.label_textures(),
            vec![
                ("world".to_string(), 4),
                ("group_gold".to_string(), 4),
                ("world".to_string(), 2),
                ("a3".to_string(), 2)
            ]
        );
        assert_eq!(
            plan.label_stems(),
            vec![
                "sefi_version_world_4col".to_string(),
                "sefi_version_group_gold_4col".to_string(),
                "sefi_version_world_2col".to_string(),
                "sefi_version_a3_2col".to_string()
            ]
        );
    }
}
