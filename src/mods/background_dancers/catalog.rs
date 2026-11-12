//! The option-row CATALOG (design §4.2 / §5.1; source-grouped since
//! 2026-09-30): the sorted, labelled entries the BACKGROUND DANCER /
//! BACKGROUND STAGE rows index into, grouped by SOURCE — `sources(kind)[0]`
//! is always STOCK (the game's own models), then one [`SourceCatalog`] per
//! custom source (`data_mods/custom_models/<kind>/<Source>/…`, the implicit
//! `CUSTOM` included), sorted by label. A source row's value `v` names
//! `RANDOM` (0), `STOCK` (1) or `sources(kind)[v − 1]`; a model row's value
//! `k` names `RANDOM` (0) or that source's `entries[k − 1]`.
//!
//! Stock labels derive from the rlist key, which IS the arc stem
//! (`pl_<key>.arc` / `mapset_<key>.arc`): `UPPER(alphabetic prefix)`, plus
//! ` #(digits + 1)` only when that prefix has more than one variant in the
//! block — `emi00 → EMI #1`, `emi01 → EMI #2`, `babylon00 → BABYLON`,
//! `replicant05 → REPLICANT #6`, `crystaldium00 → CRYSTALDIUM`. Custom
//! entries carry the planner's label (folder name or key rule). Every label
//! must fit the scalar value text's 15-byte SSO budget (`MAX_LABEL_BYTES`);
//! the stock set peaks at 12 (`REPLICANT #6`).
//!
//! The STOCK block is byte-identical to the pre-sources catalog, so stock
//! row values never move when custom content is added or the toggle flips.
//!
//! Dependency-free (std only) so the host harness mounts it beside
//! `selection.rs` / `sources.rs` (candidates and entries come from there via
//! `super::`).

use super::selection::{DancerCandidate, StageCandidate};
use super::sources::CustomEntry;

/// The row value that means "pick randomly this song".
pub const RANDOM: i32 = 0;
/// The scalar value-text budget (MSVC `std::string` SSO; longer heap-promotes
/// and leaks per push — see `custom_options::rows::destruct_sso_string`).
pub const MAX_LABEL_BYTES: usize = 15;
/// The text rendered for [`RANDOM`].
pub const RANDOM_LABEL: &str = "RANDOM";
/// The label of the stock source (source row value 1).
pub const STOCK_LABEL: &str = "STOCK";

/// Which row family a value belongs to.
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

/// One source's entries: STOCK (`slug: None`) or a custom source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCatalog {
    /// `None` = STOCK; else the source's slug (the model row's id suffix).
    pub slug: Option<String>,
    /// `STOCK`, `CUSTOM`, `DDR STRIKE`, … (≤ 15 bytes).
    pub label: String,
    pub entries: Vec<CatalogEntry>,
}

/// Both row families' sources; `[0]` is always STOCK.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub dancers: Vec<SourceCatalog>,
    pub stages: Vec<SourceCatalog>,
}

impl Catalog {
    /// The sources of a kind, STOCK first.
    pub fn sources(&self, kind: Kind) -> &[SourceCatalog] {
        match kind {
            Kind::Dancer => &self.dancers,
            Kind::Stage => &self.stages,
        }
    }

    /// Whether any custom source of this kind exists (⇒ a source row).
    pub fn has_custom(&self, kind: Kind) -> bool {
        self.sources(kind).len() > 1
    }

    /// Number of sources (the source row's max value: 1 = STOCK, 2… custom).
    pub fn source_count(&self, kind: Kind) -> usize {
        self.sources(kind).len()
    }

    /// The source row's text: `RANDOM` for 0, `STOCK` for 1, the custom
    /// source labels for `2..=source_count`, `None` beyond.
    pub fn source_label(&self, kind: Kind, value: i32) -> Option<&str> {
        if value == RANDOM {
            return Some(RANDOM_LABEL);
        }
        if value < RANDOM {
            return None;
        }
        self.sources(kind)
            .get((value - 1) as usize)
            .map(|s| s.label.as_str())
    }

    fn source(&self, kind: Kind, source: usize) -> Option<&SourceCatalog> {
        self.sources(kind).get(source)
    }

    /// Number of selectable entries of a source (that model row's max value).
    pub fn count(&self, kind: Kind, source: usize) -> usize {
        self.source(kind, source).map_or(0, |s| s.entries.len())
    }

    /// The entry a NON-RANDOM model-row value names (`None` for `RANDOM`, an
    /// unknown source and out-of-range values — the caller renders
    /// `RANDOM` / falls back).
    pub fn entry(&self, kind: Kind, source: usize, value: i32) -> Option<&CatalogEntry> {
        if value <= RANDOM {
            return None;
        }
        self.source(kind, source)?.entries.get((value - 1) as usize)
    }

    /// The display label of a model-row value: `RANDOM` for 0, the entry's
    /// label for `1..=count`, `None` beyond (the framework then shows the
    /// integer).
    pub fn label(&self, kind: Kind, source: usize, value: i32) -> Option<&str> {
        if value == RANDOM {
            return Some(RANDOM_LABEL);
        }
        self.entry(kind, source, value).map(|e| e.label.as_str())
    }

    /// The rlist key of a NON-RANDOM model-row value.
    pub fn key(&self, kind: Kind, source: usize, value: i32) -> Option<&str> {
        self.entry(kind, source, value).map(|e| e.key.as_str())
    }

    /// Every key of a source in row order — the within-source RANDOM pool.
    pub fn keys(&self, kind: Kind, source: usize) -> Vec<String> {
        self.source(kind, source)
            .map(|s| s.entries.iter().map(|e| e.key.clone()).collect())
            .unwrap_or_default()
    }

    /// The pre-sources FLAT list: the STOCK block, then every custom entry
    /// sorted by label then key (dedup by key) — what one flat row per kind
    /// indexed before 2026-09-30.
    pub fn flat_entries(&self, kind: Kind) -> Vec<CatalogEntry> {
        let sources = self.sources(kind);
        let mut out: Vec<CatalogEntry> = sources
            .first()
            .map(|s| s.entries.clone())
            .unwrap_or_default();
        let mut custom: Vec<CatalogEntry> = sources
            .iter()
            .skip(1)
            .flat_map(|s| s.entries.iter().cloned())
            .collect();
        custom.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.key.cmp(&b.key)));
        custom.dedup_by(|a, b| a.key == b.key);
        out.extend(custom);
        out
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

fn fit(label: &mut String) {
    if label.len() > MAX_LABEL_BYTES {
        let mut cut = MAX_LABEL_BYTES;
        while !label.is_char_boundary(cut) {
            cut -= 1;
        }
        label.truncate(cut);
    }
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
            fit(&mut label);
            CatalogEntry { key, label }
        })
        .collect()
}

/// Build the stock-only catalog from the candidate tables: dancers = every
/// candidate (keys are unique in `chara_resources.rlist`), stages = DISTINCT
/// keys (`boom00` / `monitor00` each have two rlist rows; `dummy00` never
/// reaches the candidates). Both sorted by key so a family's variants sit
/// together. One source per kind, labelled `STOCK`.
pub fn build_catalog(stages: &[StageCandidate], dancers: &[DancerCandidate]) -> Catalog {
    build_catalog_with_custom(stages, dancers, &[])
}

/// [`build_catalog`] plus the CUSTOM entries grouped by SOURCE (design
/// 2026-09-30 D4/D5): the STOCK source first — every candidate key NOT in
/// `custom`, labelled by the key rule and sorted by key exactly as before, so
/// stock row values never move when custom content is added or the toggle
/// flips — then one source per distinct `source.slug` among the custom
/// entries whose key IS a candidate of that kind, sources sorted by
/// `(label, slug)`, entries within a source sorted by `(label, key)` and
/// deduplicated by key. Every label is fitted to [`MAX_LABEL_BYTES`] here too
/// (the planner already did; defensive).
pub fn build_catalog_with_custom(
    stages: &[StageCandidate],
    dancers: &[DancerCandidate],
    custom: &[CustomEntry],
) -> Catalog {
    let is_custom = |key: &str| custom.iter().any(|e| e.key == key);
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
    let custom_sources = |present: &dyn Fn(&str) -> bool| -> Vec<SourceCatalog> {
        let mut out: Vec<SourceCatalog> = Vec::new();
        for e in custom.iter().filter(|e| present(&e.key)) {
            let mut label = e.label.clone();
            fit(&mut label);
            let entry = CatalogEntry {
                key: e.key.clone(),
                label,
            };
            match out
                .iter_mut()
                .find(|s| s.slug.as_deref() == Some(e.source.slug.as_str()))
            {
                Some(s) => s.entries.push(entry),
                None => {
                    let mut src_label = e.source.label.clone();
                    fit(&mut src_label);
                    out.push(SourceCatalog {
                        slug: Some(e.source.slug.clone()),
                        label: src_label,
                        entries: vec![entry],
                    });
                }
            }
        }
        for s in out.iter_mut() {
            s.entries
                .sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.key.cmp(&b.key)));
            s.entries.dedup_by(|a, b| a.key == b.key);
        }
        out.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.slug.cmp(&b.slug)));
        out
    };
    let stock = |entries: Vec<CatalogEntry>| SourceCatalog {
        slug: None,
        label: STOCK_LABEL.to_string(),
        entries,
    };
    let mut out = Catalog {
        dancers: vec![stock(stock_dancers)],
        stages: vec![stock(stock_stages)],
    };
    out.dancers
        .extend(custom_sources(&|k| dancers.iter().any(|d| d.key == k)));
    out.stages
        .extend(custom_sources(&|k| stages.iter().any(|s| s.key == k)));
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
    use super::super::sources::SourceRef;
    use super::*;

    const STOCK: usize = 0;

    fn entry(key: &str, label: &str, slug: &str, source_label: &str) -> CustomEntry {
        CustomEntry {
            key: key.to_string(),
            label: label.to_string(),
            source: SourceRef {
                slug: slug.to_string(),
                label: source_label.to_string(),
            },
        }
    }

    fn dancer(key: &str, sex: &str) -> DancerCandidate {
        dancer_candidates(
            &[(
                key.to_string(),
                ["pl", sex, "A", "1.0", "0.8", "0.0"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            )],
            |_| true,
        )
        .into_iter()
        .next()
        .unwrap()
    }

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
        assert_eq!(cat.source_count(Kind::Dancer), 1);
        assert_eq!(cat.source_count(Kind::Stage), 1);
        assert!(!cat.has_custom(Kind::Dancer));
        assert_eq!(cat.sources(Kind::Dancer)[STOCK].slug, None);
        assert_eq!(cat.sources(Kind::Dancer)[STOCK].label, "STOCK");
        assert_eq!(cat.count(Kind::Dancer, STOCK), 26);
        assert_eq!(cat.count(Kind::Stage, STOCK), 25);

        // Sorted by key, distinct.
        for entries in [
            &cat.sources(Kind::Dancer)[STOCK].entries,
            &cat.sources(Kind::Stage)[STOCK].entries,
        ] {
            let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(keys, sorted, "sorted + distinct");
        }
        let stage_entries = &cat.sources(Kind::Stage)[STOCK].entries;
        assert!(stage_entries.iter().all(|e| e.key != "dummy00"));
        assert_eq!(
            stage_entries.iter().filter(|e| e.key == "boom00").count(),
            1
        );
        assert_eq!(
            stage_entries
                .iter()
                .filter(|e| e.key == "monitor00")
                .count(),
            1
        );

        let dancer_labels: Vec<&str> = cat.sources(Kind::Dancer)[STOCK]
            .entries
            .iter()
            .map(|e| e.label.as_str())
            .collect();
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

        let stage_labels: Vec<&str> = stage_entries.iter().map(|e| e.label.as_str()).collect();
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
        assert_eq!(cat.key(Kind::Stage, STOCK, 1), Some("boom00"));
        assert_eq!(cat.label(Kind::Stage, STOCK, 1), Some("BOOM #1"));
        let emi2 = cat.sources(Kind::Dancer)[STOCK]
            .entries
            .iter()
            .position(|e| e.key == "emi01")
            .expect("emi01");
        assert_eq!(
            cat.label(Kind::Dancer, STOCK, emi2 as i32 + 1),
            Some("EMI #2")
        );
    }

    #[test]
    fn stock_labels_fit_the_sso_budget_untruncated() {
        let cat = stock_catalog();
        for e in cat.sources(Kind::Dancer)[STOCK]
            .entries
            .iter()
            .chain(cat.sources(Kind::Stage)[STOCK].entries.iter())
        {
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
        assert!(RANDOM_LABEL.len() <= MAX_LABEL_BYTES);
        assert!(STOCK_LABEL.len() <= MAX_LABEL_BYTES);
        // The longest stock label.
        assert_eq!(
            cat.sources(Kind::Stage)[STOCK]
                .entries
                .iter()
                .map(|e| e.label.len())
                .max(),
            Some("REPLICANT #6".len())
        );
    }

    #[test]
    fn oversized_labels_are_truncated_defensively() {
        let dancers = vec![dancer("averyverylongdancername00", "F")];
        let cat = build_catalog(&[], &dancers);
        let e = &cat.sources(Kind::Dancer)[STOCK].entries[0];
        assert_eq!(e.label.len(), MAX_LABEL_BYTES);
        assert_eq!(e.label, "AVERYVERYLONGDA");
        // Custom labels and source labels are fitted too.
        let cat = build_catalog_with_custom(
            &[],
            &[dancer("x00", "M")],
            &[entry(
                "x00",
                "AN EXTREMELY LONG NAME",
                "s",
                "AN EXTREMELY LONG SOURCE",
            )],
        );
        assert_eq!(cat.label(Kind::Dancer, 1, 1), Some("AN EXTREMELY LO"));
        assert_eq!(cat.source_label(Kind::Dancer, 2), Some("AN EXTREMELY LO"));
    }

    #[test]
    fn value_accessors() {
        let cat = stock_catalog();
        assert_eq!(cat.label(Kind::Dancer, STOCK, 0), Some("RANDOM"));
        assert_eq!(cat.label(Kind::Stage, STOCK, 0), Some("RANDOM"));
        assert_eq!(cat.key(Kind::Dancer, STOCK, 0), None);
        assert_eq!(cat.label(Kind::Dancer, STOCK, 26), Some("ZUKIN"));
        assert_eq!(cat.label(Kind::Dancer, STOCK, 27), None);
        assert_eq!(cat.label(Kind::Dancer, STOCK, -1), None);
        assert_eq!(cat.label(Kind::Stage, STOCK, 25), Some("SPEAKER"));
        assert_eq!(cat.label(Kind::Stage, STOCK, 26), None);
        assert_eq!(
            cat.entry(Kind::Stage, STOCK, 25).map(|e| e.key.as_str()),
            Some("speaker00")
        );
        // An unknown source index is empty, never a panic.
        assert_eq!(cat.count(Kind::Dancer, 7), 0);
        assert_eq!(cat.label(Kind::Dancer, 7, 1), None);
        assert_eq!(cat.label(Kind::Dancer, 7, 0), Some("RANDOM"));
        assert!(cat.keys(Kind::Dancer, 7).is_empty());
        // Source row values on a stock-only catalog.
        assert_eq!(cat.source_label(Kind::Dancer, 0), Some("RANDOM"));
        assert_eq!(cat.source_label(Kind::Dancer, 1), Some("STOCK"));
        assert_eq!(cat.source_label(Kind::Dancer, 2), None);
        assert_eq!(cat.source_label(Kind::Dancer, -1), None);
        assert_eq!(Catalog::default().count(Kind::Dancer, STOCK), 0);
        assert_eq!(Catalog::default().label(Kind::Dancer, STOCK, 1), None);
        assert_eq!(Catalog::default().source_label(Kind::Dancer, 1), None);
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

    /// The pre-sources fixture: two custom dancers (folder names deliberately
    /// out of key order) and one custom stage, as the planner appends them —
    /// now all in the implicit CUSTOM source.
    fn fixture() -> (Vec<StageCandidate>, Vec<DancerCandidate>, Vec<CustomEntry>) {
        let mut stages = stage_candidates(&real_map_rows(), |_| true);
        let mut dancers = dancer_candidates(&real_chara_rows(), |_| true);
        dancers.push(dancer("peter00", "M"));
        dancers.push(dancer("aaa00", "F"));
        stages.push(StageCandidate {
            key: "griffin00".to_string(),
            row: 34,
            parts: vec![("room".to_string(), None)],
            flight: false,
        });
        let custom = vec![
            entry("peter00", "PETER GRIFFIN", "custom", "CUSTOM"),
            entry("aaa00", "ZED", "custom", "CUSTOM"),
            entry("griffin00", "GRIFFIN HOUSE", "custom", "CUSTOM"),
            // A label for a key that is NOT a candidate is dropped.
            entry("ghost00", "GHOST", "custom", "CUSTOM"),
        ];
        (stages, dancers, custom)
    }

    #[test]
    fn custom_entries_group_after_the_untouched_stock_block() {
        let (stages, dancers, custom) = fixture();
        let stock = stock_catalog();
        let cat = build_catalog_with_custom(&stages, &dancers, &custom);
        // STOCK source byte-identical and first.
        assert_eq!(
            cat.sources(Kind::Dancer)[STOCK],
            stock.sources(Kind::Dancer)[STOCK]
        );
        assert_eq!(
            cat.sources(Kind::Stage)[STOCK],
            stock.sources(Kind::Stage)[STOCK]
        );
        assert_eq!(cat.count(Kind::Dancer, STOCK), 26);
        // One custom source per kind, sorted by LABEL (not key) inside.
        assert!(cat.has_custom(Kind::Dancer));
        assert_eq!(cat.source_count(Kind::Dancer), 2);
        assert_eq!(cat.source_label(Kind::Dancer, 2), Some("CUSTOM"));
        assert_eq!(cat.sources(Kind::Dancer)[1].slug.as_deref(), Some("custom"));
        assert_eq!(cat.count(Kind::Dancer, 1), 2);
        assert_eq!(cat.label(Kind::Dancer, 1, 1), Some("PETER GRIFFIN"));
        assert_eq!(cat.key(Kind::Dancer, 1, 1), Some("peter00"));
        assert_eq!(cat.label(Kind::Dancer, 1, 2), Some("ZED"));
        assert_eq!(cat.key(Kind::Dancer, 1, 2), Some("aaa00"));
        assert_eq!(cat.keys(Kind::Dancer, 1), vec!["peter00", "aaa00"]);
        assert_eq!(cat.count(Kind::Stage, 1), 1);
        assert_eq!(cat.label(Kind::Stage, 1, 1), Some("GRIFFIN HOUSE"));
        assert_eq!(cat.key(Kind::Stage, 1, 1), Some("griffin00"));
        // Stock values still clamp the same way with custom content OFF.
        assert_eq!(
            clamp_to_catalog(27, stock.count(Kind::Dancer, STOCK)),
            RANDOM
        );
    }

    #[test]
    fn flat_view_is_the_pre_sources_list() {
        let (stages, dancers, custom) = fixture();
        let stock = stock_catalog();
        let cat = build_catalog_with_custom(&stages, &dancers, &custom);
        let flat = cat.flat_entries(Kind::Dancer);
        assert_eq!(&flat[..26], &stock.sources(Kind::Dancer)[STOCK].entries[..]);
        assert_eq!(flat.len(), 28);
        assert_eq!(flat[26].label, "PETER GRIFFIN");
        assert_eq!(flat[27].label, "ZED");
        let flat = cat.flat_entries(Kind::Stage);
        assert_eq!(flat.len(), 26);
        assert_eq!(flat[25].label, "GRIFFIN HOUSE");
        // Across several sources the flat view interleaves by label, as the
        // one flat row did.
        let dancers = vec![dancer("b00", "M"), dancer("a00", "M"), dancer("c00", "M")];
        let cat = build_catalog_with_custom(
            &[],
            &dancers,
            &[
                entry("b00", "BRAVO", "umx2", "UMX2"),
                entry("a00", "ALPHA", "ddr_strike", "DDR STRIKE"),
                entry("c00", "CHARLIE", "custom", "CUSTOM"),
            ],
        );
        let flat = cat.flat_entries(Kind::Dancer);
        let labels: Vec<&str> = flat.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["ALPHA", "BRAVO", "CHARLIE"]);
    }

    #[test]
    fn sources_sort_by_label_and_kinds_stay_separate() {
        let dancers = vec![
            dancer("s2a00", "M"),
            dancer("s2b00", "M"),
            dancer("u00", "M"),
            dancer("c00", "M"),
            dancer("dup00", "M"),
        ];
        let stages = vec![StageCandidate {
            key: "room00".to_string(),
            row: 34,
            parts: vec![("bg".to_string(), None)],
            flight: false,
        }];
        let custom = vec![
            entry("u00", "AFRO", "umx2", "UMX2"),
            entry("s2b00", "ALICE1", "ddr_strike", "DDR STRIKE"),
            entry("s2a00", "AKIRA1", "ddr_strike", "DDR STRIKE"),
            entry("c00", "TETO", "custom", "CUSTOM"),
            // Same key twice (the planner never does this; the catalog dedups).
            entry("dup00", "ONE", "umx2", "UMX2"),
            entry("dup00", "ONE", "umx2", "UMX2"),
            // A stage in a stage-only source.
            entry("room00", "ROOM", "grove", "GROVE"),
        ];
        let cat = build_catalog_with_custom(&stages, &dancers, &custom);
        let dancer_sources: Vec<&str> = cat
            .sources(Kind::Dancer)
            .iter()
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(
            dancer_sources,
            vec!["STOCK", "CUSTOM", "DDR STRIKE", "UMX2"]
        );
        assert_eq!(cat.source_label(Kind::Dancer, 0), Some("RANDOM"));
        assert_eq!(cat.source_label(Kind::Dancer, 1), Some("STOCK"));
        assert_eq!(cat.source_label(Kind::Dancer, 2), Some("CUSTOM"));
        assert_eq!(cat.source_label(Kind::Dancer, 3), Some("DDR STRIKE"));
        assert_eq!(cat.source_label(Kind::Dancer, 4), Some("UMX2"));
        assert_eq!(cat.source_label(Kind::Dancer, 5), None);
        // Entries within a source sorted by label then key.
        assert_eq!(cat.keys(Kind::Dancer, 2), vec!["s2a00", "s2b00"]);
        assert_eq!(cat.label(Kind::Dancer, 2, 1), Some("AKIRA1"));
        // Dedup by key.
        assert_eq!(cat.keys(Kind::Dancer, 3), vec!["u00", "dup00"]);
        // The dancer-only sources do not appear under stages; the stage-only
        // one does not appear under dancers.
        let stage_sources: Vec<&str> = cat
            .sources(Kind::Stage)
            .iter()
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(stage_sources, vec!["STOCK", "GROVE"]);
        assert_eq!(cat.count(Kind::Stage, STOCK), 0);
        assert_eq!(cat.keys(Kind::Stage, 1), vec!["room00"]);
    }
}
