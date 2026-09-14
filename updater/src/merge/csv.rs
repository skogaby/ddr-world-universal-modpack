//! Cell-level merge for `judgement_offsets.csv` (design §4.8, R17).
//!
//! For every release row: a user row with the same code has each BLANK cell
//! filled from the release and each non-blank cell left alone; codes the user
//! lacks are appended in release order. Row order is the user's; the file is
//! rewritten only when something changed. Why cell-level: the DLL's boot crawl
//! appends a blank row for every song, so a row-level rule would never deliver
//! a new community offset for an existing song.

use super::csv_grammar::CsvDoc;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CsvReport {
    pub cells_filled: usize,
    pub rows_appended: usize,
}

impl CsvReport {
    pub fn changed(&self) -> bool {
        self.cells_filled + self.rows_appended > 0
    }
}

/// Merge `release` into `user` in place.
pub fn merge_csv(user: &mut CsvDoc, release: &CsvDoc) -> CsvReport {
    let mut report = CsvReport::default();
    for r in release.rows() {
        match user.get(&r.code) {
            Some(existing) => {
                let existing = existing.offsets;
                for side in 0..2 {
                    if existing[side].is_none() && r.offsets[side].is_some() {
                        user.upsert(&r.code, side, r.offsets[side]);
                        report.cells_filled += 1;
                    }
                }
            }
            None => {
                // Appends the row (blank cells stay blank); the second upsert
                // fills P2 on the row the first one created.
                user.upsert(&r.code, 0, r.offsets[0]);
                user.upsert(&r.code, 1, r.offsets[1]);
                report.rows_appended += 1;
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::super::csv_grammar::{parse, serialize};
    use super::*;

    fn doc(text: &str) -> CsvDoc {
        let (d, stats) = parse(text);
        assert!(stats.is_clean(), "{stats:?}");
        d
    }

    #[test]
    fn blank_cells_filled_non_blank_kept_per_side() {
        let mut user = doc("a,,5\nb,7,\nc,1,2\n");
        let release = doc("a,3,9\nb,8,8\nc,0,0\n");
        let report = merge_csv(&mut user, &release);
        assert_eq!(
            report,
            CsvReport {
                cells_filled: 2,
                rows_appended: 0
            }
        );
        assert_eq!(user.get("a").unwrap().offsets, [Some(3), Some(5)]);
        assert_eq!(user.get("b").unwrap().offsets, [Some(7), Some(8)]);
        assert_eq!(user.get("c").unwrap().offsets, [Some(1), Some(2)]);
    }

    #[test]
    fn missing_rows_appended_in_release_order_after_user_rows() {
        let mut user = doc("z,1,1\na,,\n");
        let release = doc("n2,4,4\na,2,2\nn1,,\nn3,-5,\n");
        let report = merge_csv(&mut user, &release);
        assert_eq!(
            report,
            CsvReport {
                cells_filled: 2,
                rows_appended: 3
            }
        );
        assert_eq!(
            serialize(&user),
            "code,p1_offset,p2_offset\nz,1,1\na,2,2\nn2,4,4\nn1,,\nn3,-5,\n"
        );
    }

    #[test]
    fn no_change_when_release_adds_nothing() {
        let mut user = doc("a,1,1\nb,2,2\n");
        let before = serialize(&user);
        let release = doc("a,9,9\n");
        let report = merge_csv(&mut user, &release);
        assert!(!report.changed());
        assert_eq!(serialize(&user), before);
    }

    #[test]
    fn blank_release_cells_never_clear_user_values() {
        let mut user = doc("a,1,1\n");
        let release = doc("a,,\n");
        let report = merge_csv(&mut user, &release);
        assert!(!report.changed());
        assert_eq!(user.get("a").unwrap().offsets, [Some(1), Some(1)]);
    }

    #[test]
    fn committed_csv_self_merge_is_a_no_op_and_serialize_is_stable() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../judgement_offsets.csv");
        let text = std::fs::read_to_string(path).expect("repo judgement_offsets.csv");
        let (release, stats) = parse(&text);
        assert!(stats.is_clean(), "{stats:?}");
        assert!(release.rows().len() > 1000);
        let mut user = release.clone();
        let report = merge_csv(&mut user, &release);
        assert!(!report.changed());
        assert_eq!(serialize(&user), serialize(&release));
        // An old-release user file (first half of the rows) gains exactly the rest.
        let half = release.rows().len() / 2;
        let head: String = text
            .lines()
            .take(half + 1)
            .map(|l| format!("{l}\n"))
            .collect();
        let (mut old_user, _) = parse(&head);
        let report = merge_csv(&mut old_user, &release);
        assert_eq!(report.rows_appended, release.rows().len() - half);
        assert_eq!(serialize(&old_user), serialize(&release));
    }
}
