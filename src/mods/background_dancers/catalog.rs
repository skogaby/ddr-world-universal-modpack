//! The option-row CATALOG (design §4.2 / §5.1): the sorted, labelled list of
//! dancers and distinct stages the BACKGROUND DANCER / BACKGROUND STAGE rows
//! index into. Value `0` is RANDOM; value `k ≥ 1` names `entries[k − 1]`.
//!
//! Labels derive from the rlist key, which IS the arc stem (`pl_<key>.arc` /
//! `mapset_<key>.arc`): `UPPER(alphabetic prefix)`, plus ` #(digits + 1)`
//! only when that prefix has more than one variant in the catalog —
//! `emi00 → EMI #1`, `emi01 → EMI #2`, `babylon00 → BABYLON`,
//! `replicant05 → REPLICANT #6`, `crystaldium00 → CRYSTALDIUM`. Every label
//! must fit the scalar value text's 15-byte SSO budget (`MAX_LABEL_BYTES`);
//! the stock set peaks at 12 (`REPLICANT #6`).
//!
//! Dependency-free (std only) so the host harness mounts it beside
//! `selection.rs` (candidates come from there via `super::selection`).

use super::selection::{DancerCandidate, StageCandidate};

/// The row value that means "pick randomly this song".
pub const RANDOM: i32 = 0;
/// The scalar value-text budget (MSVC `std::string` SSO; longer heap-promotes
/// and leaks per push — see `custom_options::rows::destruct_sso_string`).
pub const MAX_LABEL_BYTES: usize = 15;
/// The text rendered for [`RANDOM`].
pub const RANDOM_LABEL: &str = "RANDOM";

/// Which row a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dancer,
    Stage,
}

/// One selectable entry: the rlist key and its display label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub key: String,
    pub label: String,
}

/// Both rows' entries, each sorted by key (byte order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub dancers: Vec<CatalogEntry>,
    pub stages: Vec<CatalogEntry>,
}

impl Catalog {
    fn entries(&self, kind: Kind) -> &[CatalogEntry] {
        match kind {
            Kind::Dancer => &self.dancers,
            Kind::Stage => &self.stages,
        }
    }

    /// Number of selectable entries (the row's max value).
    pub fn count(&self, kind: Kind) -> usize {
        self.entries(kind).len()
    }

    /// The entry a NON-RANDOM row value names (`None` for `RANDOM` and for
    /// out-of-range values — the caller renders `RANDOM`/falls back).
    pub fn entry(&self, kind: Kind, value: i32) -> Option<&CatalogEntry> {
        if value <= RANDOM {
            return None;
        }
        self.entries(kind).get((value - 1) as usize)
    }

    /// The display label of a row value: `RANDOM` for 0, the entry's label
    /// for `1..=count`, `None` beyond (the framework then shows the integer).
    pub fn label(&self, kind: Kind, value: i32) -> Option<&str> {
        if value == RANDOM {
            return Some(RANDOM_LABEL);
        }
        self.entry(kind, value).map(|e| e.label.as_str())
    }

    /// The rlist key of a NON-RANDOM row value.
    pub fn key(&self, kind: Kind, value: i32) -> Option<&str> {
        self.entry(kind, value).map(|e| e.key.as_str())
    }
}

/// Split an rlist key into `(UPPER(prefix), variant)`: the prefix is
/// everything before the trailing digit run, upper-cased; the variant is
/// that run + 1 (`emi01 → ("EMI", 2)`, `crystaldium00 → ("CRYSTALDIUM", 1)`,
/// no digits ⇒ 1).
pub fn split_key(key: &str) -> (String, u32) {
    let trimmed = key.trim_end_matches(|c: char| c.is_ascii_digit());
    let digits = &key[trimmed.len()..];
    let variant = digits.parse::<u32>().unwrap_or(0).saturating_add(1);
    (trimmed.to_ascii_uppercase(), variant)
}

/// The label for one family member: `PREFIX #k` when the family has more
/// than one variant, else the bare prefix.
pub fn label_for(prefix: &str, variant: u32, variants_in_family: usize) -> String {
    if variants_in_family > 1 {
        format!("{prefix} #{variant}")
    } else {
        prefix.to_string()
    }
}

/// Distinct keys, byte-sorted.
fn sorted_distinct(keys: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = keys.collect();
    v.sort();
    v.dedup();
    v
}

/// Label a sorted, distinct key list (family variant counts over the list
/// itself). Labels are truncated to [`MAX_LABEL_BYTES`] defensively — a
/// truncation is a data bug the tests pin against for the stock set.
fn label_keys(keys: Vec<String>) -> Vec<CatalogEntry> {
    let split: Vec<(String, u32)> = keys.iter().map(|k| split_key(k)).collect();
    keys.into_iter()
        .zip(split.iter())
        .map(|(key, (prefix, variant))| {
            let family = split.iter().filter(|(p, _)| p == prefix).count();
            let mut label = label_for(prefix, *variant, family);
            if label.len() > MAX_LABEL_BYTES {
                let mut cut = MAX_LABEL_BYTES;
                while !label.is_char_boundary(cut) {
                    cut -= 1;
                }
                label.truncate(cut);
            }
            CatalogEntry { key, label }
        })
        .collect()
}

/// Build the catalog from the candidate tables: dancers = every candidate
/// (keys are unique in `chara_resources.rlist`), stages = DISTINCT keys
/// (`boom00` / `monitor00` each have two rlist rows; `dummy00` never reaches
/// the candidates). Both sorted by key so a family's variants sit together.
pub fn build_catalog(stages: &[StageCandidate], dancers: &[DancerCandidate]) -> Catalog {
    build_catalog_with_custom(stages, dancers, &[])
}

/// [`build_catalog`] plus the CUSTOM entries (design 2026-09-22 D8): the
/// stock block first — every candidate key NOT in `custom`, labelled by the
/// key rule and sorted by key exactly as before, so stock row values never
/// move when custom content is added or the toggle flips — then the custom
/// entries whose key IS a candidate of that kind, carrying their explicit
/// `(key, label)` (folder / key-rule name from `custom_content`), sorted by
/// label then key. A custom label is fitted to [`MAX_LABEL_BYTES`] here too
/// (the planner already did; defensive).
pub fn build_catalog_with_custom(
    stages: &[StageCandidate],
    dancers: &[DancerCandidate],
    custom: &[(String, String)],
) -> Catalog {
    let is_custom = |key: &str| custom.iter().any(|(k, _)| k == key);
    let stock_dancers = label_keys(sorted_distinct(
        dancers
            .iter()
            .filter(|d| !is_custom(&d.key))
            .map(|d| d.key.clone()),
    ));
    let stock_stages = label_keys(sorted_distinct(
        stages
            .iter()
            .filter(|s| !is_custom(&s.key))
            .map(|s| s.key.clone()),
    ));
    let custom_block = |present: &dyn Fn(&str) -> bool| -> Vec<CatalogEntry> {
        let mut v: Vec<CatalogEntry> = custom
            .iter()
            .filter(|(k, _)| present(k))
            .map(|(k, l)| {
                let mut label = l.clone();
                if label.len() > MAX_LABEL_BYTES {
                    let mut cut = MAX_LABEL_BYTES;
                    while !label.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    label.truncate(cut);
                }
                CatalogEntry {
                    key: k.clone(),
                    label,
                }
            })
            .collect();
        v.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.key.cmp(&b.key)));
        v.dedup_by(|a, b| a.key == b.key);
        v
    };
    let mut out = Catalog {
        dancers: stock_dancers,
        stages: stock_stages,
    };
    out.dancers
        .extend(custom_block(&|k| dancers.iter().any(|d| d.key == k)));
    out.stages
        .extend(custom_block(&|k| stages.iter().any(|s| s.key == k)));
    out
}

/// Load-side clamp for a cached row value: `0..=count` passes through, any
/// other value (a catalog that shrank, a hand-edited cache) becomes `RANDOM`.
pub fn clamp_to_catalog(value: i32, count: usize) -> i32 {
    if value >= RANDOM && (value as i64) <= count as i64 {
        value
    } else {
        RANDOM
    }
}

#[cfg(test)]
mod tests {
    use super::super::selection::fixtures::{real_chara_rows, real_map_rows};
    use super::super::selection::{dancer_candidates, stage_candidates};
    use super::*;

    #[test]
    fn split_key_cases() {
        assert_eq!(split_key("emi01"), ("EMI".to_string(), 2));
        assert_eq!(split_key("emi00"), ("EMI".to_string(), 1));
        assert_eq!(split_key("crystaldium00"), ("CRYSTALDIUM".to_string(), 1));
        assert_eq!(split_key("boom06"), ("BOOM".to_string(), 7));
        assert_eq!(split_key("replicant05"), ("REPLICANT".to_string(), 6));
        // No digits ⇒ variant 1; an inner digit stays in the prefix.
        assert_eq!(split_key("abc"), ("ABC".to_string(), 1));
        assert_eq!(split_key("st001x2"), ("ST001X".to_string(), 3));
        assert_eq!(split_key(""), (String::new(), 1));
    }

    #[test]
    fn label_for_cases() {
        assert_eq!(label_for("EMI", 2, 3), "EMI #2");
        assert_eq!(label_for("CLUB", 1, 1), "CLUB");
        assert_eq!(label_for("BOOM", 7, 7), "BOOM #7");
        assert_eq!(label_for("RAGE", 1, 2), "RAGE #1");
    }

    fn stock_catalog() -> Catalog {
        let stages = stage_candidates(&real_map_rows(), |_| true);
        let dancers = dancer_candidates(&real_chara_rows(), |_| true);
        build_catalog(&stages, &dancers)
    }

    #[test]
    fn stock_catalog_shape_and_labels() {
        let cat = stock_catalog();
        assert_eq!(cat.count(Kind::Dancer), 26);
        assert_eq!(cat.count(Kind::Stage), 25);

        // Sorted by key, distinct.
        for entries in [&cat.dancers, &cat.stages] {
            let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(keys, sorted, "sorted + distinct");
        }
        assert!(cat.stages.iter().all(|e| e.key != "dummy00"));
        assert_eq!(cat.stages.iter().filter(|e| e.key == "boom00").count(), 1);
        assert_eq!(
            cat.stages.iter().filter(|e| e.key == "monitor00").count(),
            1
        );

        let dancer_labels: Vec<&str> = cat.dancers.iter().map(|e| e.label.as_str()).collect();
        for want in [
            "AFRO #1", "AFRO #2", "ALICE #1", "ALICE #2", "BABYLON", "BONNIE", "CONCENT", "EMI #1",
            "EMI #2", "EMI #3", "GUS", "JENNY #1", "JENNY #2", "JULIO", "PIX", "RAGE #1",
            "RAGE #2", "RINON #1", "RINON #2", "RINON #3", "RUBY", "YUNI #1", "YUNI #2", "YUNI #3",
            "ZERO", "ZUKIN",
        ] {
            assert!(
                dancer_labels.contains(&want),
                "missing {want}: {dancer_labels:?}"
            );
        }
        // The first entries in byte order (the demo's cycle `RANDOM, AFRO #1, AFRO #2, ALICE #1, …`).
        assert_eq!(
            &dancer_labels[..4],
            &["AFRO #1", "AFRO #2", "ALICE #1", "ALICE #2"]
        );

        let stage_labels: Vec<&str> = cat.stages.iter().map(|e| e.label.as_str()).collect();
        for want in [
            "BOOM #1",
            "BOOM #2",
            "BOOM #3",
            "BOOM #4",
            "BOOM #5",
            "BOOM #6",
            "BOOM #7",
            "CLUB",
            "CRYSTALDIUM",
            "CYBER",
            "DAWNSTREET",
            "DISCO",
            "FLOOR",
            "LOVESWEETS",
            "MONITOR #1",
            "MONITOR #2",
            "MONITOR #3",
            "MONITOR #4",
            "REPLICANT #1",
            "REPLICANT #2",
            "REPLICANT #3",
            "REPLICANT #4",
            "REPLICANT #5",
            "REPLICANT #6",
            "SPEAKER",
        ] {
            assert!(
                stage_labels.contains(&want),
                "missing {want}: {stage_labels:?}"
            );
        }

        // Key ↔ label pairing.
        assert_eq!(cat.key(Kind::Stage, 1), Some("boom00"));
        assert_eq!(cat.label(Kind::Stage, 1), Some("BOOM #1"));
        let emi2 = cat
            .dancers
            .iter()
            .position(|e| e.key == "emi01")
            .expect("emi01");
        assert_eq!(cat.label(Kind::Dancer, emi2 as i32 + 1), Some("EMI #2"));
    }

    #[test]
    fn stock_labels_fit_the_sso_budget_untruncated() {
        let cat = stock_catalog();
        for e in cat.dancers.iter().chain(cat.stages.iter()) {
            assert!(
                e.label.len() <= MAX_LABEL_BYTES,
                "{} -> {:?} is {} bytes",
                e.key,
                e.label,
                e.label.len()
            );
            // No truncation happened: the label re-derives from the key.
            let (prefix, _) = split_key(&e.key);
            assert!(e.label.starts_with(&prefix), "{e:?}");
        }
        assert_eq!(RANDOM_LABEL.len() <= MAX_LABEL_BYTES, true);
        // The longest stock label.
        assert_eq!(
            cat.stages.iter().map(|e| e.label.len()).max(),
            Some("REPLICANT #6".len())
        );
    }

    #[test]
    fn oversized_labels_are_truncated_defensively() {
        let dancers = dancer_candidates(
            &[(
                "averyverylongdancername00".to_string(),
                ["pl", "F", "A", "1.0", "0.8", "0.0"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            )],
            |_| true,
        );
        let cat = build_catalog(&[], &dancers);
        assert_eq!(cat.dancers[0].label.len(), MAX_LABEL_BYTES);
        assert_eq!(cat.dancers[0].label, "AVERYVERYLONGDA");
    }

    #[test]
    fn value_accessors() {
        let cat = stock_catalog();
        assert_eq!(cat.label(Kind::Dancer, 0), Some("RANDOM"));
        assert_eq!(cat.label(Kind::Stage, 0), Some("RANDOM"));
        assert_eq!(cat.key(Kind::Dancer, 0), None);
        assert_eq!(cat.label(Kind::Dancer, 26), Some("ZUKIN"));
        assert_eq!(cat.label(Kind::Dancer, 27), None);
        assert_eq!(cat.label(Kind::Dancer, -1), None);
        assert_eq!(cat.label(Kind::Stage, 25), Some("SPEAKER"));
        assert_eq!(cat.label(Kind::Stage, 26), None);
        assert_eq!(
            cat.entry(Kind::Stage, 25).map(|e| e.key.as_str()),
            Some("speaker00")
        );
        assert_eq!(Catalog::default().count(Kind::Dancer), 0);
        assert_eq!(Catalog::default().label(Kind::Dancer, 1), None);
    }

    #[test]
    fn clamp_edges() {
        assert_eq!(clamp_to_catalog(-1, 25), RANDOM);
        assert_eq!(clamp_to_catalog(0, 25), 0);
        assert_eq!(clamp_to_catalog(1, 25), 1);
        assert_eq!(clamp_to_catalog(25, 25), 25);
        assert_eq!(clamp_to_catalog(26, 25), RANDOM);
        assert_eq!(clamp_to_catalog(3, 0), RANDOM);
        assert_eq!(clamp_to_catalog(i32::MAX, 25), RANDOM);
        assert_eq!(clamp_to_catalog(i32::MIN, 25), RANDOM);
    }

    #[test]
    fn custom_entries_append_after_the_untouched_stock_block() {
        let mut stages = stage_candidates(&real_map_rows(), |_| true);
        let mut dancers = dancer_candidates(&real_chara_rows(), |_| true);
        let stock = build_catalog(&stages, &dancers);
        // Two custom dancers (folder names deliberately out of key order) and
        // one custom stage, as the planner would append them.
        dancers.extend(dancer_candidates(
            &[
                (
                    "peter00".to_string(),
                    ["pl", "M", "A", "1.0", "0.8", "0.0"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
                (
                    "aaa00".to_string(),
                    ["pl", "F", "A", "0.9", "0.75", "0.0"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            ],
            |_| true,
        ));
        stages.push(StageCandidate {
            key: "griffin00".to_string(),
            row: 34,
            parts: vec![("room".to_string(), None)],
        });
        let custom = vec![
            ("peter00".to_string(), "PETER GRIFFIN".to_string()),
            ("aaa00".to_string(), "ZED".to_string()),
            ("griffin00".to_string(), "GRIFFIN HOUSE".to_string()),
            // A label for a key that is NOT a candidate is dropped.
            ("ghost00".to_string(), "GHOST".to_string()),
        ];
        let cat = build_catalog_with_custom(&stages, &dancers, &custom);
        // Stock block byte-identical and first.
        assert_eq!(&cat.dancers[..26], &stock.dancers[..]);
        assert_eq!(&cat.stages[..25], &stock.stages[..]);
        // Custom block after it, sorted by LABEL (not key).
        assert_eq!(cat.count(Kind::Dancer), 28);
        assert_eq!(cat.label(Kind::Dancer, 27), Some("PETER GRIFFIN"));
        assert_eq!(cat.key(Kind::Dancer, 27), Some("peter00"));
        assert_eq!(cat.label(Kind::Dancer, 28), Some("ZED"));
        assert_eq!(cat.key(Kind::Dancer, 28), Some("aaa00"));
        assert_eq!(cat.count(Kind::Stage), 26);
        assert_eq!(cat.label(Kind::Stage, 26), Some("GRIFFIN HOUSE"));
        assert_eq!(cat.key(Kind::Stage, 26), Some("griffin00"));
        // Stock values still clamp the same way with custom content OFF.
        assert_eq!(clamp_to_catalog(27, stock.count(Kind::Dancer)), RANDOM);
    }
}
