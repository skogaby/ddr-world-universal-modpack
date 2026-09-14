//! Element-wise merge of `custom_options.option_menu_settings` (design §4.7,
//! R13).
//!
//! A "header" is an ordinary row whose id starts with `header_`; grouping is
//! array position only. Every release row absent from the user list is inserted
//! inside the section of the header it sits under in the release: right after
//! the nearest preceding release sibling the user still keeps in that section,
//! else at the section's end — or directly after the header when the row
//! immediately follows its header in the release. Headers themselves are
//! inserted by the same rule. Existing user rows, their order and their flags
//! are never touched; rows the release no longer ships are kept.

use std::ops::Range;

use serde_json::Value;

fn id(row: &Value) -> Option<String> {
    row.get("id")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase())
}

fn is_header(row: &Value) -> bool {
    id(row).is_some_and(|s| s.starts_with("header_"))
}

/// Index of the first row in `rows` with this (lower-cased) id.
fn pos(rows: &[Value], target: &str) -> Option<usize> {
    rows.iter().position(|r| id(r).as_deref() == Some(target))
}

/// Rows after the header at `header_pos`, up to the next header (exclusive).
fn section(rows: &[Value], header_pos: usize) -> Range<usize> {
    let start = header_pos + 1;
    let end = rows[start..]
        .iter()
        .position(is_header)
        .map_or(rows.len(), |i| start + i);
    start..end
}

/// Rows before the first header (the whole list when there is none).
fn pre_header(rows: &[Value]) -> Range<usize> {
    0..rows.iter().position(is_header).unwrap_or(rows.len())
}

/// Merge `release` rows into `user`. Returns the merged list and the ids of
/// the rows inserted, in insertion order.
pub fn merge_option_menu_settings(user: &[Value], release: &[Value]) -> (Vec<Value>, Vec<String>) {
    let mut out: Vec<Value> = user.to_vec();
    let mut inserted = Vec::new();

    for (ri, r) in release.iter().enumerate() {
        let Some(rid) = id(r) else {
            continue; // rows without a string id are opaque: never inserted
        };
        if pos(&out, &rid).is_some() {
            continue;
        }

        // The header this row sits under in the release, if any.
        let h = release[..ri].iter().rposition(is_header);
        let (region, header_pos) = match h {
            Some(hi) => {
                // Every earlier release row is already in `out` (processed in
                // order), so the header is present unless it has no id.
                match id(&release[hi]).and_then(|hid| pos(&out, &hid)) {
                    Some(hp) => (section(&out, hp), Some(hp)),
                    None => (pre_header(&out), None),
                }
            }
            None => (pre_header(&out), None),
        };

        let siblings = &release[h.map_or(0, |hi| hi + 1)..ri];
        let anchor = siblings.iter().rev().find_map(|s| {
            let sid = id(s)?;
            pos(&out, &sid).filter(|p| region.contains(p))
        });

        let insert_at = match (anchor, siblings.is_empty(), header_pos) {
            (Some(p), _, _) => p + 1,
            (None, true, Some(hp)) => hp + 1,
            (None, true, None) => region.start,
            (None, false, _) => region.end,
        };
        out.insert(insert_at, r.clone());
        inserted.push(rid);
    }

    (out, inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows(ids: &[&str]) -> Vec<Value> {
        ids.iter()
            .map(|i| {
                let real = match *i {
                    "hA" => "header_a",
                    "hB" => "header_b",
                    other => other,
                };
                json!({"id": real})
            })
            .collect()
    }

    fn ids(rows: &[Value]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                let s = r["id"].as_str().unwrap().to_string();
                match s.as_str() {
                    "header_a" => "hA".to_string(),
                    "header_b" => "hB".to_string(),
                    _ => s,
                }
            })
            .collect()
    }

    fn check(release: &[&str], user: &[&str], expected: &[&str]) {
        let (out, _) = merge_option_menu_settings(&rows(user), &rows(release));
        assert_eq!(ids(&out), expected, "release={release:?} user={user:?}");
    }

    #[test]
    fn worked_examples_from_the_design() {
        check(
            &["hA", "a", "b", "N"],
            &["hA", "a", "b"],
            &["hA", "a", "b", "N"],
        );
        check(
            &["hA", "N", "a", "b"],
            &["hA", "a", "b"],
            &["hA", "N", "a", "b"],
        );
        check(
            &["hA", "a", "b", "N", "hB", "c"],
            &["hB", "c", "hA", "a", "b"],
            &["hB", "c", "hA", "a", "b", "N"],
        );
        check(
            &["hA", "a", "b", "N"],
            &["hA", "a", "hB", "c", "b"],
            &["hA", "a", "N", "hB", "c", "b"],
        );
        check(
            &["hA", "a", "hB", "c"],
            &["hA", "a"],
            &["hA", "a", "hB", "c"],
        );
        check(&["x", "hA", "a"], &["hA", "a"], &["x", "hA", "a"]);
        check(&["hA", "a"], &["a"], &["hA", "a"]);
    }

    #[test]
    fn siblings_all_moved_out_of_section_falls_back_to_section_end() {
        // Release: hA a b N ; user moved a and b under hB and has x under hA.
        check(
            &["hA", "a", "b", "N", "hB"],
            &["hA", "x", "hB", "a", "b"],
            &["hA", "x", "N", "hB", "a", "b"],
        );
    }

    #[test]
    fn case_insensitive_ids_and_dropped_rows_kept() {
        let user = vec![
            json!({"id": "HEADER_A"}),
            json!({"id": "Old_Row"}),
            json!({"id": "A"}),
        ];
        let release = rows(&["hA", "a", "N"]);
        let (out, inserted) = merge_option_menu_settings(&user, &release);
        let got: Vec<&str> = out.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(got, ["HEADER_A", "Old_Row", "A", "N"]);
        assert_eq!(inserted, ["n"]);
    }

    #[test]
    fn duplicate_user_ids_first_wins_for_positioning() {
        let user = rows(&["hA", "a", "a", "hB"]);
        let release = rows(&["hA", "a", "N"]);
        let (out, _) = merge_option_menu_settings(&user, &release);
        // anchor = first `a` (index 1) → insert at 2; the duplicate stays.
        assert_eq!(ids(&out), ["hA", "a", "N", "a", "hB"]);
    }

    #[test]
    fn rows_without_id_are_opaque() {
        let user = vec![
            json!({"id": "header_a"}),
            json!({"note": "no id"}),
            json!({"id": "a"}),
        ];
        let release = vec![
            json!({"id": "header_a"}),
            json!({"bogus": 1}),
            json!({"id": "a"}),
            json!({"id": "n"}),
        ];
        let (out, inserted) = merge_option_menu_settings(&user, &release);
        assert_eq!(out.len(), 4);
        assert_eq!(out[1], json!({"note": "no id"}));
        assert_eq!(out[3]["id"], "n");
        assert_eq!(inserted, ["n"]);
    }

    #[test]
    fn inserted_rows_copied_verbatim_and_user_flags_untouched() {
        let user = vec![
            json!({"id": "header_a"}),
            json!({"id": "a", "overlay": false, "in_game": true}),
        ];
        let release = vec![
            json!({"id": "header_a", "overlay": true}),
            json!({"id": "a", "overlay": true, "in_game": true}),
            json!({"id": "n", "overlay": false, "in_game": false}),
        ];
        let (out, _) = merge_option_menu_settings(&user, &release);
        assert_eq!(out[0], json!({"id": "header_a"}));
        assert_eq!(
            out[1],
            json!({"id": "a", "overlay": false, "in_game": true})
        );
        assert_eq!(
            out[2],
            json!({"id": "n", "overlay": false, "in_game": false})
        );
    }

    #[test]
    fn empty_user_list_reproduces_release() {
        let release = rows(&["x", "hA", "a", "b", "hB", "c"]);
        let (out, inserted) = merge_option_menu_settings(&[], &release);
        assert_eq!(ids(&out), ["x", "hA", "a", "b", "hB", "c"]);
        assert_eq!(inserted.len(), 6);
    }

    fn committed_settings() -> Vec<Value> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../mod-config.json");
        let text = std::fs::read_to_string(path).expect("repo mod-config.json");
        let doc: Value = serde_json::from_str(&text).unwrap();
        doc["custom_options"]["option_menu_settings"]
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn committed_list_self_merge_is_a_no_op() {
        let list = committed_settings();
        let (out, inserted) = merge_option_menu_settings(&list, &list);
        assert_eq!(out, list);
        assert!(inserted.is_empty());
    }

    #[test]
    fn committed_list_with_any_single_row_removed_is_reproduced_exactly() {
        let list = committed_settings();
        assert!(list.len() > 30);
        for i in 0..list.len() {
            let mut user = list.clone();
            let removed = user.remove(i);
            let (out, inserted) = merge_option_menu_settings(&user, &list);
            assert_eq!(
                out, list,
                "removing row {i} ({}) did not round-trip",
                removed["id"]
            );
            assert_eq!(
                inserted,
                vec![removed["id"].as_str().unwrap().to_ascii_lowercase()]
            );
        }
    }

    #[test]
    fn committed_list_with_two_adjacent_rows_removed_is_reproduced_exactly() {
        let list = committed_settings();
        for i in 0..list.len() - 1 {
            let mut user = list.clone();
            user.remove(i);
            user.remove(i);
            let (out, _) = merge_option_menu_settings(&user, &list);
            assert_eq!(out, list, "removing rows {i},{} did not round-trip", i + 1);
        }
    }
}
