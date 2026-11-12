//! Pure row / viewport math for `series_filter_scroll` (no `crate::`
//! imports, so `scripts/validate_series_expansion.sh` can mount it on the
//! host).
//!
//! Rows are numbered from the first scrollable line (0); each has a top in
//! px relative to row 0. Scrolled to row `s`, a row is shown while
//! `row >= s` and its bottom (`top(row) − top(s) + row_height`) stays within
//! the viewport.

/// Float slack for pixel compares (every value is integral in practice).
const EPS: f64 = 1e-6;

/// Top of every row `0..=max_row` from `(row, top)` entries: the smallest top
/// seen for the row; a row without entries sits one `row_height` below the
/// previous one (a factory returned no button).
pub fn row_tops(entries: impl Iterator<Item = (usize, f64)>, row_height: f64) -> Vec<f64> {
    let mut tops: Vec<Option<f64>> = Vec::new();
    for (row, top) in entries {
        if row >= tops.len() {
            tops.resize(row + 1, None);
        }
        if let Some(slot) = tops.get_mut(row) {
            *slot = Some(slot.map_or(top, |t| t.min(top)));
        }
    }
    let mut out = Vec::with_capacity(tops.len());
    let mut previous: Option<f64> = None;
    for top in tops {
        let t = top.unwrap_or_else(|| previous.map_or(0.0, |p| p + row_height));
        out.push(t);
        previous = Some(t);
    }
    out
}

/// Whether every row fits the viewport without scrolling.
pub fn fits(row_tops: &[f64], viewport: f64, row_height: f64) -> bool {
    match (row_tops.first(), row_tops.last()) {
        (Some(&first), Some(&last)) => last - first + row_height <= viewport + EPS,
        _ => true,
    }
}

/// Whether `row` is shown with the view scrolled to `scroll_row`.
pub fn is_visible(
    row: usize,
    scroll_row: usize,
    row_tops: &[f64],
    viewport: f64,
    row_height: f64,
) -> bool {
    match (row_tops.get(scroll_row), row_tops.get(row)) {
        (Some(&origin), Some(&top)) => {
            row >= scroll_row && top - origin + row_height <= viewport + EPS
        }
        _ => false,
    }
}

/// The scroll row after the cursor moved to `cursor_row`: up to the cursor
/// when it is above the view, else the fewest rows down that bring its
/// bottom inside the viewport.
pub fn follow(
    row_tops: &[f64],
    scroll_row: usize,
    cursor_row: usize,
    viewport: f64,
    row_height: f64,
) -> usize {
    if cursor_row < scroll_row {
        return cursor_row;
    }
    let Some(&cursor_top) = row_tops.get(cursor_row) else {
        return scroll_row;
    };
    let mut s = scroll_row;
    while s < cursor_row {
        match row_tops.get(s) {
            Some(&t) if cursor_top - t + row_height > viewport + EPS => s += 1,
            _ => break,
        }
    }
    s
}

/// Y offset (px) that moves row `scroll_row` onto row 0's position.
pub fn offset(row_tops: &[f64], scroll_row: usize) -> f64 {
    match (row_tops.first(), row_tops.get(scroll_row)) {
        (Some(&first), Some(&top)) => top - first,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f64 = 26.0;

    fn even(rows: usize) -> Vec<f64> {
        (0..rows).map(|r| r as f64 * H).collect()
    }

    #[test]
    fn row_tops_take_the_minimum_and_fill_gaps() {
        let tops = row_tops([(0, 0.0), (0, 0.0), (2, 57.0), (1, 26.0)].into_iter(), H);
        assert_eq!(tops, vec![0.0, 26.0, 57.0]);
        let tops = row_tops([(0, 0.0), (3, 90.0)].into_iter(), H);
        assert_eq!(tops, vec![0.0, 26.0, 52.0, 90.0]);
        assert!(row_tops(std::iter::empty(), H).is_empty());
    }

    #[test]
    fn evenly_spaced_rows_match_the_legacy_window() {
        // Legacy: 9 visible rows of 26 px, window [s, s + 9).
        let tops = even(15);
        let viewport = 9.0 * H;
        assert!(!fits(&tops, viewport, H));
        assert!(fits(&even(9), viewport, H));
        for s in 0..6 {
            for row in 0..15 {
                assert_eq!(
                    is_visible(row, s, &tops, viewport, H),
                    row >= s && row < s + 9,
                    "row {row} scroll {s}"
                );
            }
        }
        for s in 0..6 {
            for c in 0..15 {
                let old = if c < s {
                    c
                } else if c >= s + 9 {
                    c - 9 + 1
                } else {
                    s
                };
                assert_eq!(
                    follow(&tops, s, c, viewport, H),
                    old,
                    "cursor {c} scroll {s}"
                );
            }
        }
        assert_eq!(offset(&tops, 3), 78.0);
    }

    #[test]
    fn uneven_rows_follow_by_pixels() {
        // Rows at 0, 26, 57 (5-px break), 83, 109 (0-px break), 200 (65-px break).
        let tops = vec![0.0, 26.0, 57.0, 83.0, 109.0, 200.0];
        let viewport = 140.0;
        // Row 4 ends at 135: visible from scroll 0.
        assert!(is_visible(4, 0, &tops, viewport, H));
        assert!(!is_visible(5, 0, &tops, viewport, H));
        // Cursor on row 5 (bottom 226): scroll until 226 - top(s) <= 140
        // (row 3: 143, row 4: 117).
        assert_eq!(follow(&tops, 0, 5, viewport, H), 4);
        assert_eq!(offset(&tops, 4), 109.0);
        assert!(!is_visible(3, 4, &tops, viewport, H));
        assert!(is_visible(5, 4, &tops, viewport, H));
        // With a 3-px larger viewport row 3 suffices.
        assert_eq!(follow(&tops, 0, 5, 143.0, H), 3);
        // Back up.
        assert_eq!(follow(&tops, 4, 1, viewport, H), 1);
        // A viewport smaller than one row still shows the cursor's row.
        assert_eq!(follow(&tops, 0, 5, 10.0, H), 5);
        assert_eq!(follow(&tops, 0, 9, viewport, H), 0);
        assert!(!is_visible(9, 0, &tops, viewport, H));
    }
}
