//! Recursive additive merge for `mod-config.json` (design §4.6, R12/R14).
//!
//! For each key in the release object: absent in the user object → copy the
//! release subtree; present in both and both objects → recurse; otherwise the
//! user value wins (arrays and scalars are atomic; a type mismatch keeps the
//! user value). The one exception is `custom_options.option_menu_settings`,
//! merged element-wise by [`super::option_menu`]. The user's key order is
//! preserved (`serde_json` `preserve_order`) and new keys land at the end of
//! their parent object.

use serde_json::{Map, Value};

use super::option_menu;

/// What the merge added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MergeReport {
    /// Dotted paths of keys copied from the release (`mods.new-mod`,
    /// `gameplay_timing_fixes.audio_clock.x`, …).
    pub added: Vec<String>,
    /// Ids of `option_menu_settings` rows inserted.
    pub menu_rows_added: Vec<String>,
}

impl MergeReport {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.menu_rows_added.is_empty()
    }
}

/// Merge `release` into `user`, returning the merged document and a report.
pub fn merge_config(user: &Value, release: &Value) -> (Value, MergeReport) {
    let mut report = MergeReport::default();
    let merged = merge_value(user, release, "", &mut report);
    (merged, report)
}

fn merge_value(user: &Value, release: &Value, path: &str, report: &mut MergeReport) -> Value {
    let (Some(user_obj), Some(release_obj)) = (user.as_object(), release.as_object()) else {
        return user.clone();
    };
    let mut out: Map<String, Value> = user_obj.clone();
    for (key, release_val) in release_obj {
        let child_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        if path == "custom_options" && key == "option_menu_settings" {
            if let (Some(u), Some(r)) = (
                out.get(key).and_then(Value::as_array),
                release_val.as_array(),
            ) {
                let (rows, inserted) = option_menu::merge_option_menu_settings(u, r);
                report.menu_rows_added.extend(inserted);
                out.insert(key.clone(), Value::Array(rows));
                continue;
            }
            // User lacks the key → generic copy below; a non-array on either
            // side → keep the user value.
            if out.contains_key(key) {
                continue;
            }
        }
        match out.get(key) {
            None => {
                out.insert(key.clone(), release_val.clone());
                report.added.push(child_path);
            }
            Some(user_val) if user_val.is_object() && release_val.is_object() => {
                let merged = merge_value(user_val, release_val, &child_path, report);
                out.insert(key.clone(), merged);
            }
            Some(_) => {}
        }
    }
    Value::Object(out)
}

/// 2-space pretty JSON with a trailing newline.
pub fn to_pretty_json(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".to_string());
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn merged(user: Value, release: Value) -> (Value, MergeReport) {
        merge_config(&user, &release)
    }

    #[test]
    fn new_top_level_and_nested_keys_are_copied_and_reported() {
        let user =
            json!({"mods": {"a": true}, "gameplay_timing_fixes": {"audio_clock": {"mode": "fit"}}});
        let release = json!({
            "mods": {"a": false, "b": true},
            "gameplay_timing_fixes": {"audio_clock": {"mode": "anchor", "window_seconds": 10}, "assist_tick_alignment": true},
            "s_marvelous": {"window_ms": 12}
        });
        let (out, report) = merged(user, release);
        assert_eq!(out["mods"]["a"], true, "user scalar wins");
        assert_eq!(out["mods"]["b"], true);
        assert_eq!(out["gameplay_timing_fixes"]["audio_clock"]["mode"], "fit");
        assert_eq!(
            out["gameplay_timing_fixes"]["audio_clock"]["window_seconds"],
            10
        );
        assert_eq!(out["gameplay_timing_fixes"]["assist_tick_alignment"], true);
        assert_eq!(out["s_marvelous"]["window_ms"], 12);
        assert_eq!(
            report.added,
            vec![
                "mods.b",
                "gameplay_timing_fixes.audio_clock.window_seconds",
                "gameplay_timing_fixes.assist_tick_alignment",
                "s_marvelous"
            ]
        );
        assert!(report.menu_rows_added.is_empty());
    }

    #[test]
    fn arrays_are_atomic_and_type_mismatch_keeps_user() {
        let user = json!({"fps_unlock": {"presets": [60, 120], "selected": "sixty"}, "layeredfs": "weird"});
        let release = json!({"fps_unlock": {"presets": [60, 120, 144, 165], "selected": 60}, "layeredfs": {"verbose": false}});
        let (out, report) = merged(user.clone(), release);
        assert_eq!(out, user);
        assert!(report.is_empty());
    }

    #[test]
    fn user_key_order_preserved_and_new_keys_appended() {
        let user: Value =
            serde_json::from_str(r#"{"z": 1, "a": 2, "m": {"y": 1, "b": 2}}"#).unwrap();
        let release: Value =
            serde_json::from_str(r#"{"a": 9, "new": 3, "m": {"b": 9, "c": 4}, "z": 9}"#).unwrap();
        let (out, _) = merged(user, release);
        let keys: Vec<&String> = out.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["z", "a", "m", "new"]);
        let inner: Vec<&String> = out["m"].as_object().unwrap().keys().collect();
        assert_eq!(inner, ["y", "b", "c"]);
    }

    #[test]
    fn whole_subtree_copied_when_user_lacks_custom_options() {
        let user = json!({"mods": {}});
        let release = json!({"custom_options": {"persist_json": true, "option_menu_settings": [{"id": "header_a"}, {"id": "x"}]}});
        let (out, report) = merged(user, release.clone());
        assert_eq!(out["custom_options"], release["custom_options"]);
        assert_eq!(report.added, vec!["custom_options"]);
        assert!(report.menu_rows_added.is_empty());
    }

    #[test]
    fn option_menu_settings_delegates_and_reports_rows() {
        let user = json!({"custom_options": {"option_menu_settings": [{"id": "header_a"}, {"id": "x", "overlay": false}]}});
        let release = json!({"custom_options": {"option_menu_settings": [{"id": "header_a"}, {"id": "x"}, {"id": "y"}]}});
        let (out, report) = merged(user, release);
        let ids: Vec<&str> = out["custom_options"]["option_menu_settings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["header_a", "x", "y"]);
        assert_eq!(
            out["custom_options"]["option_menu_settings"][1]["overlay"], false,
            "user flags untouched"
        );
        assert_eq!(report.menu_rows_added, vec!["y"]);
        assert!(report.added.is_empty());
    }

    #[test]
    fn option_menu_settings_non_array_keeps_user() {
        let user = json!({"custom_options": {"option_menu_settings": "oops"}});
        let release = json!({"custom_options": {"option_menu_settings": [{"id": "x"}]}});
        let (out, report) = merged(user.clone(), release);
        assert_eq!(out, user);
        assert!(report.is_empty());
    }

    #[test]
    fn non_object_roots_keep_user() {
        let (out, report) = merged(json!([1, 2]), json!({"a": 1}));
        assert_eq!(out, json!([1, 2]));
        assert!(report.is_empty());
    }

    #[test]
    fn idempotent_and_pretty_output() {
        let user = json!({"a": {"b": 1}});
        let release = json!({"a": {"b": 2, "c": 3}, "d": [1]});
        let (once, _) = merged(user, release.clone());
        let (twice, report) = merged(once.clone(), release);
        assert_eq!(once, twice);
        assert!(report.is_empty());
        let text = to_pretty_json(&once);
        assert!(text.ends_with("}\n"));
        assert!(text.contains("\n  \"a\": {\n    \"b\": 1,"));
    }

    #[test]
    fn committed_mod_config_self_merge_is_a_no_op() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../mod-config.json");
        let text = std::fs::read_to_string(path).expect("repo mod-config.json");
        let doc: Value = serde_json::from_str(&text).unwrap();
        let (out, report) = merge_config(&doc, &doc);
        assert_eq!(out, doc);
        assert!(report.is_empty(), "{report:?}");
        assert!(
            doc["custom_options"]["option_menu_settings"]
                .as_array()
                .map_or(0, Vec::len)
                > 30
        );
    }
}
