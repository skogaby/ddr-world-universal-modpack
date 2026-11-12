//! The option rows' PURE decisions (design 2026-09-30 §4.5): which rows a
//! grouped catalog produces (one SOURCE row per kind with custom content, one
//! MODEL row per source), what a side's `(source value, model value)` asks the
//! pick for ([`Request`]), which key a focused row previews, and each row's
//! label / bound. `options.rs` is the engine-facing shell around these
//! (registration, `custom_options` reads, the versus mirror); everything here
//! runs on plain values so `scripts/validate_background_dancers.sh` can test
//! it beside `catalog.rs`.
//!
//! Row ids: `background_dancer` / `background_stage` are the STOCK model rows
//! (unchanged since before sources, so cached stock values keep meaning),
//! `<base>_<slug>` one per custom source, `<base>_source` the source row
//! (`sources::model_row_id` / `source_row_id`).

use super::catalog::{Catalog, Kind, RANDOM};
use super::sources::{model_row_id, source_row_id};

/// The base ids the two row families derive from.
pub const OPT_DANCER: &str = "background_dancer";
pub const OPT_STAGE: &str = "background_stage";

pub fn base_id(kind: Kind) -> &'static str {
    match kind {
        Kind::Dancer => OPT_DANCER,
        Kind::Stage => OPT_STAGE,
    }
}

/// What a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRole {
    /// The DANCER SOURCE / STAGE SOURCE row (0 RANDOM · 1 STOCK · 2… custom).
    Source,
    /// The model row of `sources(kind)[source]` (0 RANDOM · 1..=count).
    Model { source: usize },
}

/// One registered row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowInfo {
    pub id: String,
    pub kind: Kind,
    pub role: RowRole,
}

/// The rows a catalog produces, in registration order: per kind (dancers
/// then stages) the source row first — only when the kind has a custom
/// source — then one model row per source in catalog order (STOCK first).
/// A parent must be registered before its `ShowWhen` children, hence the
/// order.
pub fn build_rows(cat: &Catalog) -> Vec<RowInfo> {
    let mut rows = Vec::new();
    for kind in [Kind::Dancer, Kind::Stage] {
        let base = base_id(kind);
        if cat.has_custom(kind) {
            rows.push(RowInfo {
                id: source_row_id(base),
                kind,
                role: RowRole::Source,
            });
        }
        for (i, src) in cat.sources(kind).iter().enumerate() {
            rows.push(RowInfo {
                id: model_row_id(base, src.slug.as_deref()),
                kind,
                role: RowRole::Model { source: i },
            });
        }
    }
    rows
}

/// Find a row by id.
pub fn row<'a>(rows: &'a [RowInfo], id: &str) -> Option<&'a RowInfo> {
    rows.iter().find(|r| r.id == id)
}

/// The source row of a kind, if registered.
pub fn source_row<'a>(rows: &'a [RowInfo], kind: Kind) -> Option<&'a RowInfo> {
    rows.iter()
        .find(|r| r.kind == kind && r.role == RowRole::Source)
}

/// The model row of `sources(kind)[source]`.
pub fn model_row<'a>(rows: &'a [RowInfo], kind: Kind, source: usize) -> Option<&'a RowInfo> {
    rows.iter()
        .find(|r| r.kind == kind && r.role == RowRole::Model { source })
}

/// What the pick path asks for, per element (design §5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Source RANDOM (or no rows): today's global random draw.
    Any,
    /// Source `source`, model RANDOM: uniform within that source's `keys`.
    Within { source: String, keys: Vec<String> },
    /// An explicit model.
    Key(String),
}

/// A side's request for `kind`: `source_value` is the source row's value
/// (`None` when no source row is registered ⇒ STOCK semantics),
/// `model_value_of(id)` reads a model row's value. Source `0` ⇒ [`Request::Any`];
/// source `v ≥ 1` names `sources(kind)[v − 1]`: model `0` ⇒ `Within`, `k ≥ 1`
/// ⇒ `Key`; anything out of range (a stale cache the clamps did not catch,
/// a row that failed to register) ⇒ `Any`.
pub fn request_for(
    cat: &Catalog,
    rows: &[RowInfo],
    kind: Kind,
    source_value: Option<i32>,
    model_value_of: impl Fn(&str) -> Option<i32>,
) -> Request {
    let source = match source_value {
        None => 0usize,
        Some(v) if v == RANDOM => return Request::Any,
        Some(v) if v > RANDOM && (v as usize) <= cat.source_count(kind) => (v - 1) as usize,
        Some(_) => return Request::Any,
    };
    let Some(model) = model_row(rows, kind, source) else {
        return Request::Any;
    };
    match model_value_of(&model.id) {
        Some(v) if v == RANDOM => match cat.sources(kind).get(source) {
            Some(s) => Request::Within {
                source: s.label.clone(),
                keys: cat.keys(kind, source),
            },
            None => Request::Any,
        },
        Some(v) => match cat.key(kind, source, v) {
            Some(k) => Request::Key(k.to_string()),
            None => Request::Any,
        },
        None => Request::Any,
    }
}

/// The key a focused row previews (design R21): a model row ⇒ its own value's
/// key; the source row ⇒ the EFFECTIVE pick — the selected source's model row
/// value — `None` for RANDOM at either level or an unknown id.
pub fn row_choice_key(
    cat: &Catalog,
    rows: &[RowInfo],
    id: &str,
    value_of: impl Fn(&str) -> Option<i32>,
) -> Option<String> {
    let r = row(rows, id)?;
    match r.role {
        RowRole::Model { source } => cat.key(r.kind, source, value_of(id)?).map(str::to_string),
        RowRole::Source => {
            let v = value_of(id)?;
            if v <= RANDOM || (v as usize) > cat.source_count(r.kind) {
                return None;
            }
            let source = (v - 1) as usize;
            let model = model_row(rows, r.kind, source)?;
            cat.key(r.kind, source, value_of(&model.id)?)
                .map(str::to_string)
        }
    }
}

/// A row's maximum value (the load clamp bound / the scalar `max`): the
/// source row's source count, a model row's entry count. `None` for an
/// unknown id.
pub fn row_max(cat: &Catalog, rows: &[RowInfo], id: &str) -> Option<usize> {
    let r = row(rows, id)?;
    Some(match r.role {
        RowRole::Source => cat.source_count(r.kind),
        RowRole::Model { source } => cat.count(r.kind, source),
    })
}

/// A row's value text: the source row renders `RANDOM` / `STOCK` / the
/// source labels, a model row `RANDOM` / its entries' labels; `None` beyond
/// the range or for an unknown id (the framework then shows the integer).
pub fn label_for_row(cat: &Catalog, rows: &[RowInfo], id: &str, value: i32) -> Option<String> {
    let r = row(rows, id)?;
    match r.role {
        RowRole::Source => cat.source_label(r.kind, value).map(str::to_string),
        RowRole::Model { source } => cat.label(r.kind, source, value).map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::super::catalog::build_catalog_with_custom;
    use super::super::selection::fixtures::{real_chara_rows, real_map_rows};
    use super::super::selection::{dancer_candidates, stage_candidates, DancerCandidate};
    use super::super::sources::{CustomEntry, SourceRef};
    use super::*;

    fn dancer(key: &str) -> DancerCandidate {
        dancer_candidates(
            &[(
                key.to_string(),
                ["pl", "M", "A", "1.0", "0.8", "0.0"]
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

    /// Dancers: STOCK (26), CUSTOM (teto00), DDR STRIKE (akira01, tracy01);
    /// stages: STOCK only.
    fn catalog() -> Catalog {
        let stages = stage_candidates(&real_map_rows(), |_| true);
        let mut dancers = dancer_candidates(&real_chara_rows(), |_| true);
        dancers.push(dancer("teto00"));
        dancers.push(dancer("akira01"));
        dancers.push(dancer("tracy01"));
        build_catalog_with_custom(
            &stages,
            &dancers,
            &[
                entry("teto00", "KASANE TETO", "custom", "CUSTOM"),
                entry("akira01", "AKIRA1", "ddr_strike", "DDR STRIKE"),
                entry("tracy01", "TRACY1", "ddr_strike", "DDR STRIKE"),
            ],
        )
    }

    fn ids(rows: &[RowInfo]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn row_table() {
        let cat = catalog();
        let rows = build_rows(&cat);
        assert_eq!(
            ids(&rows),
            vec![
                "background_dancer_source",
                "background_dancer",
                "background_dancer_custom",
                "background_dancer_ddr_strike",
                "background_stage",
            ]
        );
        assert_eq!(rows[0].role, RowRole::Source);
        assert_eq!(rows[1].role, RowRole::Model { source: 0 });
        assert_eq!(rows[3].role, RowRole::Model { source: 2 });
        assert_eq!(rows[4].kind, Kind::Stage);
        assert!(source_row(&rows, Kind::Stage).is_none());
        assert_eq!(
            model_row(&rows, Kind::Dancer, 2).map(|r| r.id.as_str()),
            Some("background_dancer_ddr_strike")
        );
        // Stock-only catalog: two rows, no source rows.
        let stock = build_catalog_with_custom(
            &stage_candidates(&real_map_rows(), |_| true),
            &dancer_candidates(&real_chara_rows(), |_| true),
            &[],
        );
        assert_eq!(
            ids(&build_rows(&stock)),
            vec!["background_dancer", "background_stage"]
        );
    }

    #[test]
    fn requests() {
        let cat = catalog();
        let rows = build_rows(&cat);
        let stock3 = cat.key(Kind::Dancer, 0, 3).unwrap().to_string();
        // No source row ⇒ STOCK semantics.
        let req = request_for(&cat, &rows, Kind::Dancer, None, |_| Some(3));
        assert_eq!(req, Request::Key(stock3.clone()));
        let req = request_for(&cat, &rows, Kind::Dancer, None, |_| Some(0));
        assert_eq!(
            req,
            Request::Within {
                source: "STOCK".into(),
                keys: cat.keys(Kind::Dancer, 0)
            }
        );
        // Source RANDOM ⇒ Any regardless of the model rows.
        assert_eq!(
            request_for(&cat, &rows, Kind::Dancer, Some(0), |_| Some(5)),
            Request::Any
        );
        // Source DDR STRIKE (value 3) + model RANDOM ⇒ Within.
        assert_eq!(
            request_for(&cat, &rows, Kind::Dancer, Some(3), |_| Some(0)),
            Request::Within {
                source: "DDR STRIKE".into(),
                keys: vec!["akira01".into(), "tracy01".into()]
            }
        );
        // Source DDR STRIKE + model 2 ⇒ TRACY1 — read from THAT row's id only.
        let req = request_for(&cat, &rows, Kind::Dancer, Some(3), |id| {
            assert_eq!(id, "background_dancer_ddr_strike");
            Some(2)
        });
        assert_eq!(req, Request::Key("tracy01".into()));
        // Out of range anywhere ⇒ Any.
        assert_eq!(
            request_for(&cat, &rows, Kind::Dancer, Some(9), |_| Some(1)),
            Request::Any
        );
        assert_eq!(
            request_for(&cat, &rows, Kind::Dancer, Some(3), |_| Some(99)),
            Request::Any
        );
        assert_eq!(
            request_for(&cat, &rows, Kind::Dancer, Some(-1), |_| Some(1)),
            Request::Any
        );
        assert_eq!(
            request_for(&cat, &rows, Kind::Dancer, Some(3), |_| None),
            Request::Any
        );
        // Stages have no source row: value 1 of the stock row.
        let boom = cat.key(Kind::Stage, 0, 1).unwrap().to_string();
        assert_eq!(
            request_for(&cat, &rows, Kind::Stage, None, |_| Some(1)),
            Request::Key(boom)
        );
    }

    #[test]
    fn preview_keys() {
        let cat = catalog();
        let rows = build_rows(&cat);
        let values = |id: &str| -> Option<i32> {
            match id {
                "background_dancer_source" => Some(3),
                "background_dancer_ddr_strike" => Some(1),
                "background_dancer" => Some(5),
                "background_dancer_custom" => Some(0),
                _ => None,
            }
        };
        assert_eq!(
            row_choice_key(&cat, &rows, "background_dancer_source", values),
            Some("akira01".into())
        );
        assert_eq!(
            row_choice_key(&cat, &rows, "background_dancer_ddr_strike", values),
            Some("akira01".into())
        );
        assert_eq!(
            row_choice_key(&cat, &rows, "background_dancer", values),
            cat.key(Kind::Dancer, 0, 5).map(str::to_string)
        );
        // A hidden row still previews its own value when asked; RANDOM ⇒ None.
        assert_eq!(
            row_choice_key(&cat, &rows, "background_dancer_custom", values),
            None
        );
        // Source RANDOM ⇒ None; source naming a RANDOM model row ⇒ None.
        let random_source = |id: &str| {
            if id == "background_dancer_source" {
                Some(0)
            } else {
                Some(1)
            }
        };
        assert_eq!(
            row_choice_key(&cat, &rows, "background_dancer_source", random_source),
            None
        );
        let custom_random = |id: &str| match id {
            "background_dancer_source" => Some(2),
            _ => Some(0),
        };
        assert_eq!(
            row_choice_key(&cat, &rows, "background_dancer_source", custom_random),
            None
        );
        // Unknown id / missing value ⇒ None.
        assert_eq!(row_choice_key(&cat, &rows, "nope", values), None);
        assert_eq!(
            row_choice_key(&cat, &rows, "background_stage", values),
            None
        );
    }

    #[test]
    fn bounds_and_labels() {
        let cat = catalog();
        let rows = build_rows(&cat);
        assert_eq!(row_max(&cat, &rows, "background_dancer_source"), Some(3));
        assert_eq!(row_max(&cat, &rows, "background_dancer"), Some(26));
        assert_eq!(row_max(&cat, &rows, "background_dancer_custom"), Some(1));
        assert_eq!(
            row_max(&cat, &rows, "background_dancer_ddr_strike"),
            Some(2)
        );
        assert_eq!(row_max(&cat, &rows, "background_stage"), Some(25));
        assert_eq!(row_max(&cat, &rows, "nope"), None);

        let l = |id: &str, v: i32| label_for_row(&cat, &rows, id, v);
        assert_eq!(l("background_dancer_source", 0).as_deref(), Some("RANDOM"));
        assert_eq!(l("background_dancer_source", 1).as_deref(), Some("STOCK"));
        assert_eq!(l("background_dancer_source", 2).as_deref(), Some("CUSTOM"));
        assert_eq!(
            l("background_dancer_source", 3).as_deref(),
            Some("DDR STRIKE")
        );
        assert_eq!(l("background_dancer_source", 4), None);
        assert_eq!(
            l("background_dancer_ddr_strike", 0).as_deref(),
            Some("RANDOM")
        );
        assert_eq!(
            l("background_dancer_ddr_strike", 1).as_deref(),
            Some("AKIRA1")
        );
        assert_eq!(l("background_dancer_ddr_strike", 3), None);
        assert_eq!(l("background_dancer", 1).as_deref(), Some("AFRO #1"));
        assert_eq!(l("nope", 0), None);
    }
}
