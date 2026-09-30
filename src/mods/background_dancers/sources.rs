//! Custom-content SOURCES — the pure identity layer (design 2026-09-30): a
//! source is one folder level above the friendly folders under
//! `data_mods/custom_models/{dancers,stages}/` (`<kind>/<Source>/<Friendly>/pl_<key>/`),
//! identified by the SLUG of its folder name and labelled through the folder
//! label rule (≤ 15 bytes). Anything placed the old way — a friendly folder or
//! a model directly under `<kind>/` — belongs to the implicit source
//! [`IMPLICIT_SOURCE`] (`CUSTOM`); a real folder of that name is the same
//! source (same slug ⇒ one source). The slug `source` is reserved (it would
//! collide with the source row's own option id) and a name with nothing
//! printable has no slug: both are refused and the caller skips the folder.
//!
//! Also here: the directory-role classification the impure walker applies to
//! a listing (from names alone, so it is testable), the planned-entry type
//! the catalog groups, and the option-row id rules (`background_dancer` for
//! STOCK, `background_dancer_<slug>` per source, `background_dancer_source`).
//!
//! Dependency-free (std + the sibling `custom_content` helpers) so
//! `scripts/validate_background_dancers.sh` mounts it beside `custom_content.rs`.

use super::custom_content::{
    classify_arc_name, classify_folder_name, fit_label, label_from_folder, ArcRole,
};

/// The implicit source for legacy placements — and the folder name that
/// merges into it (`Custom/`).
pub const IMPLICIT_SOURCE: &str = "Custom";

/// Slugs that would collide with the source rows' own ids.
pub const RESERVED_SLUGS: &[&str] = &["source"];

/// Slug length cap (row ids stay short snake_case).
pub const MAX_SLUG_BYTES: usize = 32;

/// A resolved source: stable id + display label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    /// `[a-z0-9_]+`, ≤ [`MAX_SLUG_BYTES`] — the persistence / option-id key.
    pub slug: String,
    /// The row text (`CUSTOM`, `DDR STRIKE`), ≤ 15 bytes.
    pub label: String,
}

/// One accepted custom dancer or stage, as the planner emits it and the
/// catalog groups it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomEntry {
    /// The rlist key (arc stem).
    pub key: String,
    /// The entry's own row label (folder name or key rule), ≤ 15 bytes.
    pub label: String,
    pub source: SourceRef,
}

/// The slug of a folder name: ASCII-lowercased, every run of characters
/// outside `[a-z0-9]` collapsed to one `_`, `_` trimmed from both ends,
/// capped at [`MAX_SLUG_BYTES`] (re-trimmed). `None` when nothing survives.
pub fn slug(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    let mut pending = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending = true;
        }
    }
    if out.len() > MAX_SLUG_BYTES {
        out.truncate(MAX_SLUG_BYTES);
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// The source a folder name resolves to: `None` (root / legacy placement) is
/// the implicit [`IMPLICIT_SOURCE`]; `Some(folder)` takes the folder's slug
/// and label. `Err(reason)` — human-readable, WARN-ready — for a reserved
/// slug or a name with nothing printable.
pub fn resolve_source(folder: Option<&str>) -> Result<SourceRef, String> {
    let name = folder.unwrap_or(IMPLICIT_SOURCE);
    let Some(slug) = slug(name) else {
        return Err(format!(
            "source folder {name:?} has no printable ASCII letters or digits to name it by"
        ));
    };
    if RESERVED_SLUGS.contains(&slug.as_str()) {
        return Err(format!(
            "source folder {name:?} would take the reserved id {slug:?} (rename it)"
        ));
    }
    let Some(mut label) = label_from_folder(name) else {
        return Err(format!("source folder {name:?} has no printable label"));
    };
    fit_label(&mut label);
    Ok(SourceRef { slug, label })
}

/// What a directory directly under `dancers/` or `stages/` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirRole {
    /// A model folder (`pl_*` / `mapset_*`) — content of its parent.
    Model,
    /// A source folder: holds at least one friendly folder.
    Source,
    /// A friendly folder: holds models directly (today's two-level layout).
    Friendly,
    /// Nothing usable inside — skipped silently.
    Ignored,
}

/// Classify a directory from three facts the walker computes from names:
/// `Model` wins, then `Source` (any friendly child), then `Friendly` (model
/// content of its own), else `Ignored`.
pub fn dir_role(
    is_model_folder: bool,
    has_friendly_child: bool,
    has_model_content: bool,
) -> DirRole {
    if is_model_folder {
        DirRole::Model
    } else if has_friendly_child {
        DirRole::Source
    } else if has_model_content {
        DirRole::Friendly
    } else {
        DirRole::Ignored
    }
}

/// Whether a folder NAME is a model folder (body, part, stage or the gold
/// stage variant) — the walker's partition test.
pub fn is_model_folder_name(name: &str) -> bool {
    matches!(
        classify_folder_name(name),
        ArcRole::Body { .. }
            | ArcRole::Part { .. }
            | ArcRole::Stage { .. }
            | ArcRole::GoldStage { .. }
    )
}

/// Whether a listing holds model content: any directory that is a model
/// folder, or any file that is a body / part / stage arc. `files` and `dirs`
/// are bare names.
pub fn has_model_content(files: &[String], dirs: &[String]) -> bool {
    dirs.iter().any(|d| is_model_folder_name(d))
        || files.iter().any(|f| {
            matches!(
                classify_arc_name(f),
                ArcRole::Body { .. } | ArcRole::Part { .. } | ArcRole::Stage { .. }
            )
        })
}

/// The source row's option id for a kind's base id (`background_dancer` →
/// `background_dancer_source`).
pub fn source_row_id(base: &str) -> String {
    format!("{base}_source")
}

/// A model row's option id: the base id itself for STOCK (`None`), else
/// `<base>_<slug>`.
pub fn model_row_id(base: &str, slug: Option<&str>) -> String {
    match slug {
        None => base.to_string(),
        Some(s) => format!("{base}_{s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn slug_cases() {
        assert_eq!(slug("DDR STRIKE").as_deref(), Some("ddr_strike"));
        assert_eq!(slug("J.C.").as_deref(), Some("j_c"));
        assert_eq!(slug("UMX2").as_deref(), Some("umx2"));
        assert_eq!(slug("  Custom  ").as_deref(), Some("custom"));
        assert_eq!(slug("Maid-Zukin").as_deref(), Some("maid_zukin"));
        assert_eq!(slug("___"), None);
        assert_eq!(slug(""), None);
        assert_eq!(slug("♥♥"), None);
        // Cap at 32 bytes, no trailing underscore after the cut.
        let long = slug("abcdefghij abcdefghij abcdefghij abcdefghij").unwrap();
        assert!(long.len() <= MAX_SLUG_BYTES, "{long}");
        assert!(!long.ends_with('_'));
        assert_eq!(long, "abcdefghij_abcdefghij_abcdefghij");
    }

    #[test]
    fn resolve_source_cases() {
        let custom = SourceRef {
            slug: "custom".into(),
            label: "CUSTOM".into(),
        };
        assert_eq!(resolve_source(None), Ok(custom.clone()));
        assert_eq!(resolve_source(Some("Custom")), Ok(custom.clone()));
        assert_eq!(resolve_source(Some("CUSTOM ")), Ok(custom));
        assert_eq!(
            resolve_source(Some("DDR STRIKE")),
            Ok(SourceRef {
                slug: "ddr_strike".into(),
                label: "DDR STRIKE".into(),
            })
        );
        // Label rule: `_` → space, upper, ≤ 15 bytes.
        let long = resolve_source(Some("An_Extremely Long Source Name")).unwrap();
        assert_eq!(long.label, "AN EXTREMELY LO");
        assert_eq!(long.slug, "an_extremely_long_source_name");
        // Reserved / unprintable.
        let err = resolve_source(Some("Source")).unwrap_err();
        assert!(err.contains("\"Source\""), "{err}");
        assert!(err.contains("reserved"), "{err}");
        let err = resolve_source(Some("♥♥")).unwrap_err();
        assert!(err.contains("♥♥"), "{err}");
    }

    #[test]
    fn dir_role_truth_table() {
        for &(m, f, c, want) in &[
            (true, true, true, DirRole::Model),
            (true, false, false, DirRole::Model),
            (false, true, true, DirRole::Source),
            (false, true, false, DirRole::Source),
            (false, false, true, DirRole::Friendly),
            (false, false, false, DirRole::Ignored),
        ] {
            assert_eq!(dir_role(m, f, c), want, "{m} {f} {c}");
        }
    }

    #[test]
    fn model_content_detection() {
        let none: Vec<String> = Vec::new();
        assert!(has_model_content(&none, &strings(&["pl_teto00"])));
        assert!(has_model_content(&none, &strings(&["mapset_griffin00"])));
        assert!(has_model_content(&strings(&["pl_x.arc"]), &none));
        assert!(has_model_content(&strings(&["PL_X_HEAD00.ARC"]), &none));
        assert!(has_model_content(&none, &strings(&["mapset_y_g"])));
        assert!(!has_model_content(
            &strings(&["readme.txt", "chara_resources.rlist.txt"]),
            &strings(&["textures_src"])
        ));
        assert!(!has_model_content(&none, &none));
        // The shadow quad is never a model.
        assert!(!has_model_content(&strings(&["pl_shadow00.arc"]), &none));
    }

    #[test]
    fn row_ids() {
        assert_eq!(model_row_id("background_dancer", None), "background_dancer");
        assert_eq!(
            model_row_id("background_dancer", Some("ddr_strike")),
            "background_dancer_ddr_strike"
        );
        assert_eq!(source_row_id("background_stage"), "background_stage_source");
        // The reserved slug is exactly the one that would collide.
        assert_eq!(
            model_row_id("background_stage", Some("source")),
            source_row_id("background_stage")
        );
    }
}
