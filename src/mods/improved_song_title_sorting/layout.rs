//! Pure model of the per-letter MUSIC TITLE menu: display order, cell
//! templates, label keys, the replacement filter table and the flow-layout
//! rows the game will produce.
//!
//! Selection index = the stock title class (`ChartMetadata+0x88`, computed
//! by the game's own classifier and left untouched): 0..=9 the kana lines
//! ア行..ワ行, 10..=35 A..Z, 36 other. It is also the bit in the saved
//! `filtersort/title` u64, so saved kana-line selections keep their stock
//! meaning; stock bits 10..=19 (the letter groups and OTHER) come back as the
//! single letters A..J.
//!
//! Display order (the game lays children out in creation order, wrapping
//! when the next cell would overflow the 216-px item grid): A..Z at the
//! 42-px template 5 (five per row), OTHER at the 108-px template 2 on Z's
//! row, an invisible full-width row break, then the ten kana lines at the
//! stock 54-px template 4 (four per row) with their stock labels.
//!
//! The table is an identity range table: row `i` starts at class `i`, so the
//! stock predicate's `start(i) <= class < start(i + 1)` matches exactly one
//! class, and the chip summary's runs of adjacent selection indices read
//! `A～C` / `あ～ろ` from the rows' first/last labels.
//!
//! No `crate::` imports: `scripts/validate_improved_song_title_sorting.sh`
//! mounts this file on the host.

/// Stride of the stock (and replacement) title table.
pub const ENTRY_STRIDE: usize = 0x88;
const FIELD_KEY: usize = 0x08;
const FIELD_START: usize = 0x30;
const FIELD_FIRST: usize = 0x38;
const FIELD_LAST: usize = 0x60;
/// MSVC `std::string`: 16-byte inline buffer, size, capacity.
const STR_SIZE: usize = 0x10;
const STR_CAPACITY: usize = 0x18;
const SSO_CAPACITY: usize = 0x0F;

/// First letter class (A); Z is `FIRST_LETTER + 25`.
pub const FIRST_LETTER: i32 = 10;
/// The classifier's catch-all class (digits, symbols, hiragana-initial yomi).
pub const OTHER: i32 = 36;
/// Selectable items = title classes (0..=36). Also the persisted count.
pub const CLASS_COUNT: u32 = 37;

/// Cell templates (`filter_switch_base0N`, N = cells per 216-px row).
pub const LETTER_TEMPLATE: i32 = 5;
pub const OTHER_TEMPLATE: i32 = 2;
pub const KANA_TEMPLATE: i32 = 4;

/// Width of the filter item grid (`switch_usr/dummy_choice_usr`).
pub const GRID_WIDTH: u32 = 216;
/// Visible grid rows (266 px / 26 px).
pub const VISIBLE_ROWS: usize = 10;

/// Stock kana lines in class order: label key (`title_<key>`, stock art) and
/// the chip summary's first/last labels (Shift-JIS, copied from the stock
/// table: あ～お … わ～ん).
const KANA: [(&str, [u8; 2], [u8; 2]); 10] = [
    ("line_a", [0x82, 0xA0], [0x82, 0xA8]),
    ("line_ka", [0x82, 0xA9], [0x82, 0xB1]),
    ("line_sa", [0x82, 0xB3], [0x82, 0xBB]),
    ("line_ta", [0x82, 0xBD], [0x82, 0xC6]),
    ("line_na", [0x82, 0xC8], [0x82, 0xCC]),
    ("line_ha", [0x82, 0xCD], [0x82, 0xD9]),
    ("line_ma", [0x82, 0xDC], [0x82, 0xE0]),
    ("line_ya", [0x82, 0xE2], [0x82, 0xE6]),
    ("line_ra", [0x82, 0xE7], [0x82, 0xEB]),
    ("line_wa", [0x82, 0xED], [0x82, 0xF1]),
];
/// Stock chip label of the OTHER class.
const OTHER_LABEL: &[u8] = b"Other";

/// One grid child, in creation (= display) order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A filter button: selection index (= title class = saved bit), cell
    /// template and label key (texture `sefi_<label>`).
    Button {
        selection: i32,
        template: i32,
        label: String,
    },
    /// The game's invisible full-width, 1-px row break (`FilterHeader`).
    RowBreak,
}

fn letter(class: i32) -> char {
    (b'a' + (class - FIRST_LETTER) as u8) as char
}

/// Label key of a letter class (`title_<l>_5col`).
pub fn letter_label(class: i32) -> String {
    format!("title_{}_{}col", letter(class), LETTER_TEMPLATE)
}

/// Label key of the OTHER cell (`title_other_2col`; the stock
/// `sefi_title_other` is the 44-px template-4 art).
pub fn other_label() -> String {
    format!("title_other_{}col", OTHER_TEMPLATE)
}

/// The menu's children in display order.
pub fn items() -> Vec<Item> {
    let mut out: Vec<Item> = (FIRST_LETTER..OTHER)
        .map(|class| Item::Button {
            selection: class,
            template: LETTER_TEMPLATE,
            label: letter_label(class),
        })
        .collect();
    out.push(Item::Button {
        selection: OTHER,
        template: OTHER_TEMPLATE,
        label: other_label(),
    });
    out.push(Item::RowBreak);
    out.extend(
        KANA.iter()
            .enumerate()
            .map(|(class, (key, _, _))| Item::Button {
                selection: class as i32,
                template: KANA_TEMPLATE,
                label: format!("title_{}", key),
            }),
    );
    out
}

/// Cell width of a template (`filter_switch_base0N`'s AFP size).
pub fn cell_width(template: i32) -> u32 {
    match template {
        1 => 220,
        2 => 108,
        3 => 72,
        4 => 54,
        _ => 42,
    }
}

/// Label canvas width of a template (the stock label slot).
pub fn canvas_width(template: i32) -> u32 {
    match template {
        1 => 220,
        2 => 104,
        3 => 64,
        4 => 44,
        _ => 32,
    }
}

/// Stock label texture with the canvas size (clone donor for the
/// texturelist entry; all live in the stock IFS's `tex001` atlas).
pub fn label_donor(width: u32) -> &'static str {
    match width {
        220 => "sefi_event_league",
        104 => "sefi_version_world",
        64 => "sefi_version_gold",
        44 => "sefi_title_other",
        _ => "sefi_level_00",
    }
}

/// Mod-owned label textures: (texture name, canvas width), in item order.
/// The kana lines keep their stock art.
pub fn new_textures() -> Vec<(String, u32)> {
    items()
        .into_iter()
        .filter_map(|item| match item {
            Item::Button {
                template, label, ..
            } if template != KANA_TEMPLATE => {
                Some((format!("sefi_{}", label), canvas_width(template)))
            }
            _ => None,
        })
        .collect()
}

/// Grid rows the game's flow layout produces, as item indices: a cell starts
/// a new row when it would overflow [`GRID_WIDTH`] (strict `>`, zero gaps); a
/// row break is full-width, so it both ends the row before it and forces the
/// next item onto a new one. Row breaks take no index row of their own here.
pub fn grid_rows(items: &[Item]) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut used = 0u32;
    for (i, item) in items.iter().enumerate() {
        match item {
            Item::RowBreak => {
                if !current.is_empty() {
                    rows.push(std::mem::take(&mut current));
                }
                used = GRID_WIDTH;
            }
            Item::Button { template, .. } => {
                let w = cell_width(*template);
                if used + w > GRID_WIDTH {
                    if !current.is_empty() {
                        rows.push(std::mem::take(&mut current));
                    }
                    used = 0;
                }
                current.push(i);
                used += w;
            }
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
}

/// One replacement-table row (indexed by selection index).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    pub start: i32,
    /// Chip summary first / last label (raw bytes; Shift-JIS for kana).
    pub first: Vec<u8>,
    pub last: Vec<u8>,
}

/// `CLASS_COUNT + 1` rows: one per class (start = class) and the sentinel
/// the predicate reads as the last row's exclusive end (and the chip summary
/// probes, as it walks `0..=count`).
pub fn table_rows() -> Vec<TableRow> {
    let mut rows: Vec<TableRow> = KANA
        .iter()
        .enumerate()
        .map(|(class, (_, first, last))| TableRow {
            start: class as i32,
            first: first.to_vec(),
            last: last.to_vec(),
        })
        .collect();
    rows.extend((FIRST_LETTER..OTHER).map(|class| {
        let upper = vec![letter(class).to_ascii_uppercase() as u8];
        TableRow {
            start: class,
            first: upper.clone(),
            last: upper,
        }
    }));
    rows.push(TableRow {
        start: OTHER,
        first: OTHER_LABEL.to_vec(),
        last: OTHER_LABEL.to_vec(),
    });
    rows.push(TableRow {
        start: CLASS_COUNT as i32,
        first: Vec::new(),
        last: Vec::new(),
    });
    rows
}

/// Inline (SSO) MSVC string; `None` if longer than 15 bytes.
fn encode_sso(out: &mut [u8], at: usize, bytes: &[u8]) -> Option<()> {
    if bytes.len() > SSO_CAPACITY {
        return None;
    }
    out[at..at + bytes.len()].copy_from_slice(bytes);
    out[at + STR_SIZE..at + STR_SIZE + 8].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
    out[at + STR_CAPACITY..at + STR_CAPACITY + 8]
        .copy_from_slice(&(SSO_CAPACITY as u64).to_le_bytes());
    Some(())
}

/// Bytes of one table row (empty key, range start, first/last labels; every
/// string inline). `None` if a label does not fit the 15-byte inline buffer.
pub fn encode_row(row: &TableRow) -> Option<[u8; ENTRY_STRIDE]> {
    let mut out = [0u8; ENTRY_STRIDE];
    encode_sso(&mut out, FIELD_KEY, b"")?;
    out[FIELD_START..FIELD_START + 4].copy_from_slice(&row.start.to_le_bytes());
    encode_sso(&mut out, FIELD_FIRST, &row.first)?;
    encode_sso(&mut out, FIELD_LAST, &row.last)?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buttons(items: &[Item]) -> Vec<(i32, i32, String)> {
        items
            .iter()
            .filter_map(|i| match i {
                Item::Button {
                    selection,
                    template,
                    label,
                } => Some((*selection, *template, label.clone())),
                Item::RowBreak => None,
            })
            .collect()
    }

    #[test]
    fn every_class_is_selectable_exactly_once() {
        let mut selections: Vec<i32> = buttons(&items()).iter().map(|b| b.0).collect();
        selections.sort();
        assert_eq!(selections, (0..CLASS_COUNT as i32).collect::<Vec<_>>());
    }

    #[test]
    fn letters_come_first_then_other_then_kana() {
        let items = items();
        let b = buttons(&items);
        assert_eq!(b[0], (10, 5, "title_a_5col".to_string()));
        assert_eq!(b[25], (35, 5, "title_z_5col".to_string()));
        assert_eq!(b[26], (36, 2, "title_other_2col".to_string()));
        assert_eq!(b[27], (0, 4, "title_line_a".to_string()));
        assert_eq!(b[36], (9, 4, "title_line_wa".to_string()));
        assert_eq!(items[27], Item::RowBreak);
    }

    #[test]
    fn grid_rows_match_the_requested_layout() {
        let items = items();
        let rows: Vec<Vec<i32>> = grid_rows(&items)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|i| match &items[i] {
                        Item::Button { selection, .. } => *selection,
                        Item::RowBreak => -1,
                    })
                    .collect()
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                vec![10, 11, 12, 13, 14], // A B C D E
                vec![15, 16, 17, 18, 19], // F G H I J
                vec![20, 21, 22, 23, 24], // K L M N O
                vec![25, 26, 27, 28, 29], // P Q R S T
                vec![30, 31, 32, 33, 34], // U V W X Y
                vec![35, 36],             // Z OTHER
                vec![0, 1, 2, 3],         // ア カ サ タ
                vec![4, 5, 6, 7],         // ナ ハ マ ヤ
                vec![8, 9],               // ラ ワ
            ]
        );
        assert!(rows.len() <= VISIBLE_ROWS);
    }

    #[test]
    fn without_the_row_break_a_kana_cell_would_join_zs_row() {
        let items: Vec<Item> = items()
            .into_iter()
            .filter(|i| *i != Item::RowBreak)
            .collect();
        let rows = grid_rows(&items);
        assert_eq!(rows[5].len(), 3, "Z + OTHER + ア fit in 216 px");
    }

    #[test]
    fn new_textures_are_letters_and_other() {
        let tex = new_textures();
        assert_eq!(tex.len(), 27);
        assert_eq!(tex[0], ("sefi_title_a_5col".to_string(), 32));
        assert_eq!(tex[26], ("sefi_title_other_2col".to_string(), 104));
        assert!(tex.iter().all(|(name, _)| !name.contains("line_")));
    }

    #[test]
    fn table_is_an_identity_range_table_with_sentinel() {
        let rows = table_rows();
        assert_eq!(rows.len(), CLASS_COUNT as usize + 1);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row.start, i as i32);
        }
        assert_eq!(rows[0].first, vec![0x82, 0xA0]); // あ
        assert_eq!(rows[9].last, vec![0x82, 0xF1]); // ん
        assert_eq!(rows[10].first, b"A".to_vec());
        assert_eq!(rows[35].last, b"Z".to_vec());
        assert_eq!(rows[36].first, b"Other".to_vec());
        assert!(rows[37].first.is_empty());
    }

    #[test]
    fn predicate_semantics_match_exactly_one_class() {
        let rows = table_rows();
        for sel in 0..CLASS_COUNT as usize {
            let matches: Vec<i32> = (0..CLASS_COUNT as i32)
                .filter(|&v| rows[sel].start <= v && v < rows[sel + 1].start)
                .collect();
            assert_eq!(matches, vec![sel as i32]);
        }
    }

    #[test]
    fn rows_encode_inline() {
        let rows = table_rows();
        let bytes = encode_row(&rows[1]).expect("fits");
        assert_eq!(&bytes[FIELD_START..FIELD_START + 4], &1i32.to_le_bytes());
        assert_eq!(&bytes[FIELD_FIRST..FIELD_FIRST + 2], &[0x82, 0xA9]);
        assert_eq!(bytes[FIELD_FIRST + STR_SIZE], 2);
        assert_eq!(bytes[FIELD_LAST + STR_CAPACITY], SSO_CAPACITY as u8);
        assert_eq!(bytes[FIELD_KEY + STR_CAPACITY], SSO_CAPACITY as u8);
        assert!(rows.iter().all(|r| encode_row(r).is_some()));
        let long = TableRow {
            start: 0,
            first: vec![b'x'; 16],
            last: Vec::new(),
        };
        assert!(encode_row(&long).is_none());
    }

    #[test]
    fn label_keys_are_distinct_from_stock_names() {
        let stock = [
            "title_abc",
            "title_def",
            "title_ghi",
            "title_jkl",
            "title_mno",
            "title_pqr",
            "title_stu",
            "title_vwx",
            "title_yz",
            "title_other",
        ];
        for (_, _, label) in buttons(&items()) {
            if !label.starts_with("title_line_") {
                assert!(!stock.contains(&label.as_str()), "{label}");
            }
        }
    }

    #[test]
    fn donors_exist_for_the_used_widths() {
        assert_eq!(label_donor(32), "sefi_level_00");
        assert_eq!(label_donor(104), "sefi_version_world");
    }
}
