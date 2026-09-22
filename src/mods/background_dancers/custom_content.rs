//! Custom dancers & stages from `data_mods` — the PURE half (design
//! 2026-09-22 D2–D8): classify the arc files a content directory holds, derive
//! the display label, parse the text sidecar grammar, apply the defaults and
//! turn everything into candidates + mounts + log lines. No filesystem, no
//! engine, no logging — the impure `custom_scan.rs` walks the folders, reads
//! the arc headers / rlists and feeds this planner, so the host harness can
//! test every rule here (`scripts/validate_background_dancers.sh` mounts this
//! file beside `selection.rs` / `catalog.rs`, reached via `super::`).
//!
//! Layout (ONE base, `data_mods/custom_models/` — maintainer: every custom
//! dancer and stage goes there, no per-character mod folders; models are
//! FOLDERS holding the arc's contents — nobody has to pack an arc, the
//! scanner packs a cache arc for the engine — a ready `.arc` is accepted too):
//! ```text
//! data_mods/custom_models/dancers/<Friendly Name>/pl_<key>/             body folder (required)
//!     pl_<key>.model, pl_<key>.b2it, *.dds …                            (the add-on's export layout — flat)
//!     — or —  data/chara/pl_<key>/…                                      (a literally unpacked arc)
//! data_mods/custom_models/dancers/<Friendly Name>/pl_<key>_<part>/      optional accessory parts
//! data_mods/custom_models/dancers/<Friendly Name>/chara_resources.rlist optional sidecar (or .rlist.txt)
//! data_mods/custom_models/stages/<Friendly Name>/mapset_<key>/          stage folder (required; `_g` ignored)
//!     gm_<key>_<part>/gm_<key>_<part>.model, *.dds …                     one dir per part
//!     camera/<shot>.camanm …  (any `*.camanm` anywhere in the folder = the stage's own camera set;
//!                              names containing `_non` are the cut-aways)
//!     — or —  data/map/…, data/camera/…
//! data_mods/custom_models/stages/<Friendly Name>/map_resources.rlist    optional sidecar (parts[:prio])
//! data_mods/custom_models/stages/<Friendly Name>/stage_camera_resources.rlist  optional (camera names)
//! ```
//! A model directly in `dancers/` / `stages/` takes the stock key rule for
//! its label (`_` → space); one in a subfolder takes the FOLDER NAME. The key
//! ALWAYS comes from the model folder / arc name — the engine keys models by
//! the member stems (`data/chara/pl_<key>/pl_<key>.model`), which the
//! discovery verifies ([`folder_member_path`] maps a folder file to the member
//! path the engine will see).
//!
//! Sidecar rows use the stock rlist grammar (`chara_resources`:
//! `[pl, sex, class, model_scale, shadow_scale, unlock]`; `map_resources`:
//! `[rgb, rgb, part[:prio]…]`; `stage_camera_resources`: camera names) so the
//! rows an A3 `startup.arc` repack carried can be dropped in verbatim; rows
//! for keys not in the same directory are ignored.

use super::catalog::{label_for, split_key, MAX_LABEL_BYTES};
use super::selection::{
    dancer_candidates, stage_candidates, DancerCandidate, StageCandidate, PART_NAMES,
};

/// `(key, fields)` — the `core::anm::rlist::Row` shape, spelled locally so the
/// harness mount needs no `core` import.
pub type Row = (String, Vec<String>);

/// Which table a content directory feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Dancer,
    Stage,
}

impl ContentKind {
    /// The mod-folder subdirectory the scanner walks.
    pub fn dir_name(self) -> &'static str {
        match self {
            ContentKind::Dancer => "dancers",
            ContentKind::Stage => "stages",
        }
    }
}

/// What an `.arc` filename says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArcRole {
    /// `pl_<key>.arc`.
    Body { key: String },
    /// `pl_<key>_<part>.arc` (`part` ∈ head|hips|chest|forearm|face + 2 digits).
    Part { key: String, part: String },
    /// `mapset_<key>.arc`.
    Stage { key: String },
    /// `mapset_<key>_g.arc` — the gold-cabinet variant the mod never loads.
    GoldStage { key: String },
    /// `pl_shadow00.arc` — the shared floor quad, never a dancer.
    Shadow,
    /// Anything else (not an arc, or an unrelated arc).
    Other,
}

/// A key is `[a-z0-9_]+` (the engine hashes the exact bytes; the stock keys
/// are lower-case alphanumerics).
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_part_suffix(p: &str) -> bool {
    let Some(digits) = p
        .strip_prefix("head")
        .or_else(|| p.strip_prefix("hips"))
        .or_else(|| p.strip_prefix("chest"))
        .or_else(|| p.strip_prefix("forearm"))
        .or_else(|| p.strip_prefix("face"))
    else {
        return false;
    };
    digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Classify a bare filename. Case-insensitive on the extension and the
/// `pl_`/`mapset_` prefixes; the key itself is taken as written (and must be
/// [`valid_key`] — an invalid key classifies as [`ArcRole::Other`]).
pub fn classify_arc_name(name: &str) -> ArcRole {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".arc") else {
        return ArcRole::Other;
    };
    if let Some(rest) = stem.strip_prefix("pl_") {
        if rest == "shadow00" {
            return ArcRole::Shadow;
        }
        if let Some((k, p)) = rest.rsplit_once('_') {
            if is_part_suffix(p) {
                return if valid_key(k) {
                    ArcRole::Part {
                        key: k.to_string(),
                        part: p.to_string(),
                    }
                } else {
                    ArcRole::Other
                };
            }
        }
        return if valid_key(rest) {
            ArcRole::Body {
                key: rest.to_string(),
            }
        } else {
            ArcRole::Other
        };
    }
    if let Some(rest) = stem.strip_prefix("mapset_") {
        if let Some(k) = rest.strip_suffix("_g") {
            return if valid_key(k) {
                ArcRole::GoldStage { key: k.to_string() }
            } else {
                ArcRole::Other
            };
        }
        return if valid_key(rest) {
            ArcRole::Stage {
                key: rest.to_string(),
            }
        } else {
            ArcRole::Other
        };
    }
    ArcRole::Other
}

/// The sidecar list a filename names (case-insensitive; `.rlist` = binary
/// MRL0, `.rlist.txt` = the text twin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarList {
    Chara,
    Map,
    Camera,
}

/// `Some((list, is_text))` for the six sidecar filenames.
pub fn classify_sidecar_name(name: &str) -> Option<(SidecarList, bool)> {
    let lower = name.to_ascii_lowercase();
    let (stem, text) = match lower.strip_suffix(".rlist.txt") {
        Some(s) => (s, true),
        None => (lower.strip_suffix(".rlist")?, false),
    };
    let list = match stem {
        "chara_resources" => SidecarList::Chara,
        "map_resources" => SidecarList::Map,
        "stage_camera_resources" => SidecarList::Camera,
        _ => return None,
    };
    Some((list, text))
}

/// Classify a MODEL FOLDER name (`pl_peter00`, `mapset_griffin00`) like the
/// arc it stands for.
pub fn classify_folder_name(name: &str) -> ArcRole {
    classify_arc_name(&format!("{name}.arc"))
}

/// The arc member path a file inside a model folder stands for. `rel` is the
/// file's path relative to the folder (any separator). A path already under
/// `data/` is the literal unpacked-arc layout and passes through; otherwise
/// the add-on's flat export layout is mapped: a body / part folder's files
/// live at `data/chara/<folder>/<rel>`, a stage folder's `*.camanm` clips at
/// `data/camera/<key>/<basename>` (where `parse_pick` finds them by
/// basename) and everything else at `data/map/<rel>` (one `gm_<key>_<part>/`
/// dir per part).
pub fn folder_member_path(role: &ArcRole, folder: &str, rel: &str) -> String {
    let rel = rel.replace('\\', "/");
    let rel = rel.trim_start_matches('/');
    if rel.len() >= 5 && rel[..5].eq_ignore_ascii_case("data/") {
        return rel.to_string();
    }
    match role {
        ArcRole::Body { .. } | ArcRole::Part { .. } => format!("data/chara/{folder}/{rel}"),
        ArcRole::Stage { key } | ArcRole::GoldStage { key } => {
            let base = rel.rsplit('/').next().unwrap_or(rel);
            if base.to_ascii_lowercase().ends_with(".camanm") {
                format!("data/camera/{key}/{base}")
            } else {
                format!("data/map/{rel}")
            }
        }
        ArcRole::Shadow | ArcRole::Other => format!("data/{rel}"),
    }
}

/// The text twin of an rlist: one row per line, `key, field, field, …`;
/// blank lines and `#` comments skipped; every cell trimmed. A line with no
/// comma is a key with zero fields.
pub fn parse_text_rlist(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for raw in text.lines() {
        let line = match raw.find('#') {
            Some(i) => &raw[..i],
            None => raw,
        }
        .trim();
        if line.is_empty() {
            continue;
        }
        let mut cells = line.split(',').map(str::trim);
        let Some(key) = cells.next() else { continue };
        if key.is_empty() {
            continue;
        }
        rows.push((key.to_string(), cells.map(str::to_string).collect()));
    }
    rows
}

/// The stock male row the defaults come from (`rage00`/`zero00`/…).
pub const DEFAULT_DANCER_FIELDS: [&str; 6] = ["pl", "M", "A", "1.0", "0.8", "0.0"];

/// One arc the scanner saw — a ready `.arc` file, or a MODEL FOLDER the
/// scanner packed into a cache arc — with its member paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcFile {
    /// Arc filename (`pl_peter00.arc`; for a folder, `<folder>.arc`).
    pub name: String,
    /// Filesystem path of the arc the engine will read (what a mount binds
    /// the logical path to) — the cache arc for a folder source.
    pub path: String,
    /// What the user put on disk (the folder or the `.arc`) — log lines only.
    pub source: String,
    /// Member paths (arc header, or the folder walk mapped through
    /// [`folder_member_path`]); `None` = unreadable / not an arc.
    pub members: Option<Vec<String>>,
}

/// One directory the scanner walked: the `dancers/` or `stages/` root of a
/// base (`folder: None`) or one friendly-name subfolder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackDir {
    /// Filesystem path (logs only).
    pub dir: String,
    /// The friendly-name folder, `None` at the root.
    pub folder: Option<String>,
    pub arcs: Vec<ArcFile>,
    /// Sidecar rows found in this directory (binary + text concatenated).
    pub chara_rows: Vec<Row>,
    pub map_rows: Vec<Row>,
    pub camera_rows: Vec<Row>,
}

/// What the stock tables already hold — the planner extends, never shadows.
#[derive(Debug, Clone, PartialEq)]
pub struct StockContext {
    pub stage_keys: Vec<String>,
    pub dancer_keys: Vec<String>,
    /// `map_resources.rlist` row count — the first custom stage row index.
    pub next_stage_row: usize,
    /// `chara_resources.rlist` row count — the first custom dancer row index.
    pub next_dancer_row: usize,
    /// The camera set a custom stage borrows when it brings none (stock
    /// camera row of the first stock stage candidate; may be empty).
    pub default_camera_row: Vec<String>,
}

/// The planner's output: candidates to append, camera rows to place at the
/// stage rows, the option-row labels, the arc mounts, and the log lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub stages: Vec<StageCandidate>,
    /// `(row, camera names)` — one per custom stage, at that stage's row.
    pub camera_rows: Vec<(usize, Vec<String>)>,
    pub dancers: Vec<DancerCandidate>,
    /// `(key, label)` for every accepted entry (dancers and stages).
    pub labels: Vec<(String, String)>,
    /// `(logical game path, filesystem path)`.
    pub mounts: Vec<(String, String)>,
    /// INFO lines (one per accepted entry).
    pub notes: Vec<String>,
    /// WARN lines (one per rejected / defaulted item).
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty() && self.dancers.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

/// A folder name as a row label: `_` → space, ASCII only (other bytes
/// dropped), upper-cased, whitespace collapsed. `None` when nothing printable
/// survives (the caller falls back to the key rule).
pub fn label_from_folder(folder: &str) -> Option<String> {
    let mut out = String::with_capacity(folder.len());
    let mut pending_space = false;
    for c in folder.chars() {
        let c = if c == '_' { ' ' } else { c };
        if c.is_ascii_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if !c.is_ascii() || c.is_ascii_control() {
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c.to_ascii_uppercase());
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The key rule for flat arcs: `split_key` with `_` → space
/// (`peter_griffin00` → `PETER GRIFFIN`), `#k` only when the family has more
/// than one member among `family_keys`.
pub fn label_from_key(key: &str, family_keys: &[String]) -> String {
    let (prefix, variant) = split_key(key);
    let family = family_keys
        .iter()
        .filter(|k| split_key(k).0 == prefix)
        .count()
        .max(1);
    label_for(&prefix.replace('_', " "), variant, family)
}

/// Clamp a label to the SSO budget at a char boundary; `true` when cut.
pub fn fit_label(label: &mut String) -> bool {
    if label.len() <= MAX_LABEL_BYTES {
        return false;
    }
    let mut cut = MAX_LABEL_BYTES;
    while !label.is_char_boundary(cut) {
        cut -= 1;
    }
    label.truncate(cut);
    true
}

// ---------------------------------------------------------------------------
// Member checks
// ---------------------------------------------------------------------------

/// `data/chara/pl_<key>/pl_<key>.model` is present.
pub fn body_model_present(key: &str, members: &[String]) -> bool {
    let want = format!("data/chara/pl_{key}/pl_{key}.model");
    members.iter().any(|m| *m == want)
}

/// The stage's part names, from every `data/map/gm_<key>_<part>/gm_<key>_<part>.model`
/// member, in member order.
pub fn stage_parts_from_members(key: &str, members: &[String]) -> Vec<String> {
    let prefix = format!("data/map/gm_{key}_");
    let mut parts = Vec::new();
    for m in members {
        let Some(rest) = m.strip_prefix(&prefix) else {
            continue;
        };
        let Some((part, file)) = rest.split_once('/') else {
            continue;
        };
        if part.is_empty() || part.contains('/') {
            continue;
        }
        if file == format!("gm_{key}_{part}.model") && !parts.iter().any(|p| p == part) {
            parts.push(part.to_string());
        }
    }
    parts
}

/// The stems of every `*.camanm` member (any directory), in member order.
pub fn camera_names_from_members(members: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for m in members {
        let file = m.rsplit('/').next().unwrap_or(m);
        if let Some(stem) = file.strip_suffix(".camanm") {
            if !stem.is_empty() && !names.iter().any(|n| n == stem) {
                names.push(stem.to_string());
            }
        }
    }
    names
}

fn find_row<'a>(rows: &'a [Row], key: &str) -> Option<&'a Row> {
    rows.iter().find(|(k, _)| k == key)
}

// ---------------------------------------------------------------------------
// The planner
// ---------------------------------------------------------------------------

/// Turn the scanned content directories into candidates. Dirs/arcs are
/// taken in the given order (the scanner sorts them), so row indices and
/// mounts are deterministic. Every rejection is one WARN line; the stock
/// tables are never touched (a key colliding with a stock or an earlier
/// custom key is refused).
pub fn plan(dancer_dirs: &[PackDir], stage_dirs: &[PackDir], stock: &StockContext) -> Plan {
    let mut out = Plan::default();
    let mut stage_keys: Vec<String> = stock.stage_keys.clone();
    let mut dancer_keys: Vec<String> = stock.dancer_keys.clone();
    // Key-rule labels are resolved after the pass so the `#k` family count
    // covers every flat arc of the kind.
    let mut pending_key_labels: Vec<(String, ContentKind)> = Vec::new();

    // ---- dancers ----
    for dir in dancer_dirs {
        let mut accepted_here: Vec<String> = Vec::new();
        for arc in &dir.arcs {
            let ArcRole::Body { key } = classify_arc_name(&arc.name) else {
                continue;
            };
            let Some(members) = arc.members.as_ref() else {
                out.warnings
                    .push(format!("{}: not a readable arc -- skipped", arc.source));
                continue;
            };
            if !body_model_present(&key, members) {
                out.warnings.push(format!(
                    "{}: no data/chara/pl_{key}/pl_{key}.model member (the key must match the model inside) -- skipped",
                    arc.source
                ));
                continue;
            }
            if dancer_keys.iter().any(|k| *k == key) {
                out.warnings.push(format!(
                    "{}: dancer key {:?} already exists (stock or an earlier folder) -- skipped",
                    arc.source, key
                ));
                continue;
            }
            // Sidecar row, else the defaults (one INFO-level note).
            let (fields, from_sidecar): (Vec<String>, bool) = match find_row(&dir.chara_rows, &key)
            {
                Some((_, f)) => (f.clone(), true),
                None => (
                    DEFAULT_DANCER_FIELDS
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                    false,
                ),
            };
            let parsed = dancer_candidates(&[(key.clone(), fields.clone())], |_| true);
            let cand = match parsed.into_iter().next() {
                Some(c) => c,
                None => {
                    out.warnings.push(format!(
                        "{}: chara_resources row for {:?} is malformed ({:?}) -- using the defaults {:?}",
                        arc.source, key, fields, DEFAULT_DANCER_FIELDS
                    ));
                    match dancer_candidates(
                        &[(
                            key.clone(),
                            DEFAULT_DANCER_FIELDS
                                .iter()
                                .map(|s| s.to_string())
                                .collect(),
                        )],
                        |_| true,
                    )
                    .into_iter()
                    .next()
                    {
                        Some(c) => c,
                        None => continue,
                    }
                }
            };
            let row = stock.next_dancer_row + out.dancers.len();
            let label = match dir.folder.as_deref().and_then(label_from_folder) {
                Some(l) => Some(l),
                None => {
                    pending_key_labels.push((key.clone(), ContentKind::Dancer));
                    None
                }
            };
            out.mounts
                .push((format!("data/arc/{}", arc.name), arc.path.clone()));
            out.notes.push(format!(
                "custom dancer {} ({}) from {} -- sex {} class {} scale {}/{}{}",
                label.as_deref().unwrap_or("<key rule>"),
                key,
                arc.source,
                match cand.sex {
                    super::selection::Sex::Male => "M",
                    super::selection::Sex::Female => "F",
                },
                cand.class,
                cand.model_scale,
                cand.shadow_scale,
                if from_sidecar {
                    " (sidecar row)"
                } else {
                    " (no sidecar row -- stock male defaults)"
                }
            ));
            if let Some(l) = label {
                out.labels.push((key.clone(), l));
            }
            out.dancers.push(DancerCandidate { row, ..cand });
            dancer_keys.push(key.clone());
            accepted_here.push(key);
        }
        // Part arcs: only for a body accepted in the SAME directory, and only
        // the parts the dancer actor knows.
        for arc in &dir.arcs {
            let ArcRole::Part { key, part } = classify_arc_name(&arc.name) else {
                continue;
            };
            if !accepted_here.iter().any(|k| *k == key) {
                out.warnings.push(format!(
                    "{}: part arc for {:?} without an accepted pl_{key}.arc in the same folder -- ignored",
                    arc.source, key
                ));
                continue;
            }
            if !PART_NAMES.contains(&part.as_str()) {
                out.warnings.push(format!(
                    "{}: part {:?} is not one the dancer actor attaches ({:?}) -- ignored",
                    arc.source, part, PART_NAMES
                ));
                continue;
            }
            let Some(members) = arc.members.as_ref() else {
                out.warnings.push(format!(
                    "{}: not a readable arc -- part ignored",
                    arc.source
                ));
                continue;
            };
            let want = format!("data/chara/pl_{key}_{part}/pl_{key}_{part}.model");
            if !members.iter().any(|m| *m == want) {
                out.warnings
                    .push(format!("{}: no {want} member -- part ignored", arc.source));
                continue;
            }
            out.mounts
                .push((format!("data/arc/{}", arc.name), arc.path.clone()));
        }
    }

    // ---- stages ----
    for dir in stage_dirs {
        for arc in &dir.arcs {
            let ArcRole::Stage { key } = classify_arc_name(&arc.name) else {
                continue;
            };
            let Some(members) = arc.members.as_ref() else {
                out.warnings
                    .push(format!("{}: not a readable arc -- skipped", arc.source));
                continue;
            };
            let member_parts = stage_parts_from_members(&key, members);
            if member_parts.is_empty() {
                out.warnings.push(format!(
                    "{}: no data/map/gm_{key}_<part>/gm_{key}_<part>.model member (the key must match the models inside) -- skipped",
                    arc.source
                ));
                continue;
            }
            if stage_keys.iter().any(|k| *k == key) {
                out.warnings.push(format!(
                    "{}: stage key {:?} already exists (stock or an earlier folder) -- skipped",
                    arc.source, key
                ));
                continue;
            }
            // Parts: the sidecar row's list (validated against the members),
            // else every model in the arc with no priority.
            let (mut parts, parts_src): (Vec<(String, Option<i32>)>, &str) = match find_row(
                &dir.map_rows,
                &key,
            ) {
                Some((_, fields)) => {
                    let cands = stage_candidates(&[(key.clone(), fields.clone())], |_| true);
                    match cands.into_iter().next() {
                        Some(c) => (c.parts, "sidecar row"),
                        None => {
                            out.warnings.push(format!(
                                    "{}: map_resources row for {:?} has no parts ({:?}) -- using the arc's models",
                                    arc.source, key, fields
                                ));
                            (
                                member_parts.iter().map(|p| (p.clone(), None)).collect(),
                                "arc members",
                            )
                        }
                    }
                }
                None => (
                    member_parts.iter().map(|p| (p.clone(), None)).collect(),
                    "arc members",
                ),
            };
            let before = parts.len();
            parts.retain(|(p, _)| {
                let keep = member_parts.iter().any(|m| m == p);
                if !keep {
                    out.warnings.push(format!(
                        "{}: part {:?} listed for {:?} has no model in the arc -- dropped",
                        arc.source, p, key
                    ));
                }
                keep
            });
            if parts.is_empty() {
                out.warnings.push(format!(
                    "{}: none of the {before} listed parts exist in the arc -- skipped",
                    arc.source
                ));
                continue;
            }
            // Camera set: sidecar row, else the arc's own camanm clips, else
            // the borrowed stock set.
            let (camera, camera_src): (Vec<String>, &str) = match find_row(&dir.camera_rows, &key) {
                Some((_, f)) if f.iter().any(|n| !n.trim().is_empty()) => {
                    (f.clone(), "sidecar row")
                }
                _ => {
                    let own = camera_names_from_members(members);
                    if own.is_empty() {
                        (stock.default_camera_row.clone(), "borrowed stock set")
                    } else {
                        (own, "the arc's own camanm clips")
                    }
                }
            };
            let row = stock.next_stage_row + out.stages.len();
            let label = match dir.folder.as_deref().and_then(label_from_folder) {
                Some(l) => Some(l),
                None => {
                    pending_key_labels.push((key.clone(), ContentKind::Stage));
                    None
                }
            };
            out.mounts
                .push((format!("data/arc/{}", arc.name), arc.path.clone()));
            out.notes.push(format!(
                "custom stage {} ({}) from {} -- {} part(s) [{}] ({}), camera set: {} name(s) ({})",
                label.as_deref().unwrap_or("<key rule>"),
                key,
                arc.source,
                parts.len(),
                parts
                    .iter()
                    .map(|(p, prio)| match prio {
                        Some(n) => format!("{p}:{n}"),
                        None => p.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                parts_src,
                camera.len(),
                camera_src
            ));
            if let Some(l) = label {
                out.labels.push((key.clone(), l));
            }
            out.camera_rows.push((row, camera));
            out.stages.push(StageCandidate {
                key: key.clone(),
                row,
                parts,
            });
            stage_keys.push(key);
        }
    }

    // Key-rule labels (flat arcs), with the `#k` family count per kind.
    for (key, kind) in &pending_key_labels {
        let family: Vec<String> = pending_key_labels
            .iter()
            .filter(|(_, k)| k == kind)
            .map(|(k, _)| k.clone())
            .collect();
        out.labels.push((key.clone(), label_from_key(key, &family)));
    }
    // Fit every label to the SSO budget (one WARN per cut).
    for (key, label) in out.labels.iter_mut() {
        let full = label.clone();
        if fit_label(label) {
            out.warnings.push(format!(
                "label {full:?} for {key:?} is longer than {MAX_LABEL_BYTES} bytes -- shown as {label:?}"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::selection::Sex;
    use super::*;

    fn arc(name: &str, dir: &str, members: Option<Vec<&str>>) -> ArcFile {
        ArcFile {
            name: name.to_string(),
            path: format!("{dir}/{name}"),
            source: format!("{dir}/{name}"),
            members: members.map(|m| m.iter().map(|s| s.to_string()).collect()),
        }
    }

    fn body_members(key: &str) -> Vec<&'static str> {
        // Leaked on purpose: test fixture strings.
        let v = vec![
            format!("data/chara/pl_{key}/pg_body.dds"),
            format!("data/chara/pl_{key}/pl_{key}.b2it"),
            format!("data/chara/pl_{key}/pl_{key}.grp2it"),
            format!("data/chara/pl_{key}/pl_{key}.model"),
        ];
        v.into_iter()
            .map(|s| Box::leak(s.into_boxed_str()) as &'static str)
            .collect()
    }

    fn stage_members(key: &str, parts: &[&str], camanm: &[&str]) -> Vec<&'static str> {
        let mut v: Vec<String> = Vec::new();
        for p in parts {
            v.push(format!("data/map/gm_{key}_{p}/gm_{key}_{p}.grp2it"));
            v.push(format!("data/map/gm_{key}_{p}/gm_{key}_{p}.model"));
            v.push(format!("data/map/gm_{key}_{p}/tex_{p}.dds"));
        }
        for c in camanm {
            v.push(format!("data/camera/long/{c}/{c}.camanm"));
        }
        v.into_iter()
            .map(|s| Box::leak(s.into_boxed_str()) as &'static str)
            .collect()
    }

    fn stock() -> StockContext {
        StockContext {
            stage_keys: vec!["boom00".into(), "monitor00".into()],
            dancer_keys: vec!["rage00".into(), "emi00".into()],
            next_stage_row: 34,
            next_dancer_row: 26,
            default_camera_row: vec!["st001_st02".into(), "st001_non01".into()],
        }
    }

    #[test]
    fn classify_names() {
        assert_eq!(
            classify_arc_name("pl_peter00.arc"),
            ArcRole::Body {
                key: "peter00".into()
            }
        );
        assert_eq!(
            classify_arc_name("PL_Peter00.ARC"),
            ArcRole::Body {
                key: "peter00".into()
            }
        );
        assert_eq!(
            classify_arc_name("pl_peter00_head00.arc"),
            ArcRole::Part {
                key: "peter00".into(),
                part: "head00".into()
            }
        );
        assert_eq!(
            classify_arc_name("pl_emi00_face02.arc"),
            ArcRole::Part {
                key: "emi00".into(),
                part: "face02".into()
            }
        );
        // An underscore INSIDE the key is not a part separator.
        assert_eq!(
            classify_arc_name("pl_peter_griffin00.arc"),
            ArcRole::Body {
                key: "peter_griffin00".into()
            }
        );
        assert_eq!(
            classify_arc_name("pl_peter_griffin00_forearm00.arc"),
            ArcRole::Part {
                key: "peter_griffin00".into(),
                part: "forearm00".into()
            }
        );
        assert_eq!(classify_arc_name("pl_shadow00.arc"), ArcRole::Shadow);
        assert_eq!(
            classify_arc_name("mapset_griffin00.arc"),
            ArcRole::Stage {
                key: "griffin00".into()
            }
        );
        assert_eq!(
            classify_arc_name("mapset_griffin00_g.arc"),
            ArcRole::GoldStage {
                key: "griffin00".into()
            }
        );
        assert_eq!(classify_arc_name("mc_male.arc"), ArcRole::Other);
        assert_eq!(classify_arc_name("readme.txt"), ArcRole::Other);
        assert_eq!(classify_arc_name("pl_.arc"), ArcRole::Other);
        assert_eq!(classify_arc_name("pl_Bad Key.arc"), ArcRole::Other);
    }

    #[test]
    fn classify_sidecars() {
        assert_eq!(
            classify_sidecar_name("chara_resources.rlist"),
            Some((SidecarList::Chara, false))
        );
        assert_eq!(
            classify_sidecar_name("MAP_RESOURCES.rlist.txt"),
            Some((SidecarList::Map, true))
        );
        assert_eq!(
            classify_sidecar_name("stage_camera_resources.rlist"),
            Some((SidecarList::Camera, false))
        );
        assert_eq!(classify_sidecar_name("music_camera_resources.rlist"), None);
        assert_eq!(classify_sidecar_name("notes.txt"), None);
    }

    #[test]
    fn folder_member_paths() {
        let body = ArcRole::Body {
            key: "peter00".into(),
        };
        assert_eq!(
            folder_member_path(&body, "pl_peter00", "pl_peter00.model"),
            "data/chara/pl_peter00/pl_peter00.model"
        );
        assert_eq!(
            folder_member_path(&body, "pl_peter00", "pg_body.dds"),
            "data/chara/pl_peter00/pg_body.dds"
        );
        // Literal unpacked arc passes through (any separator).
        assert_eq!(
            folder_member_path(
                &body,
                "pl_peter00",
                "data\\chara\\pl_peter00\\pl_peter00.model"
            ),
            "data/chara/pl_peter00/pl_peter00.model"
        );
        let part = ArcRole::Part {
            key: "peter00".into(),
            part: "head00".into(),
        };
        assert_eq!(
            folder_member_path(&part, "pl_peter00_head00", "pl_peter00_head00.model"),
            "data/chara/pl_peter00_head00/pl_peter00_head00.model"
        );
        let stage = ArcRole::Stage {
            key: "griffin00".into(),
        };
        assert_eq!(
            folder_member_path(
                &stage,
                "mapset_griffin00",
                "gm_griffin00_room/gm_griffin00_room.model"
            ),
            "data/map/gm_griffin00_room/gm_griffin00_room.model"
        );
        assert_eq!(
            folder_member_path(&stage, "mapset_griffin00", "camera/griffin_st01.camanm"),
            "data/camera/griffin00/griffin_st01.camanm"
        );
        assert_eq!(
            folder_member_path(&stage, "mapset_griffin00", "griffin_non01.CAMANM"),
            "data/camera/griffin00/griffin_non01.CAMANM"
        );
        assert_eq!(
            folder_member_path(
                &stage,
                "mapset_griffin00",
                "data/map/gm_griffin00_room/x.dds"
            ),
            "data/map/gm_griffin00_room/x.dds"
        );
        assert_eq!(
            classify_folder_name("pl_peter00"),
            ArcRole::Body {
                key: "peter00".into()
            }
        );
        assert_eq!(
            classify_folder_name("mapset_griffin00"),
            ArcRole::Stage {
                key: "griffin00".into()
            }
        );
        assert_eq!(classify_folder_name("textures"), ArcRole::Other);
    }

    #[test]
    fn stage_folder_camera_clips_become_the_camera_set() {
        // A flat stage folder as the scanner maps it: parts + 4 camanm shots.
        let stage = ArcRole::Stage {
            key: "griffin00".into(),
        };
        let members: Vec<String> = [
            "gm_griffin00_footpanel/gm_griffin00_footpanel.model",
            "gm_griffin00_room/gm_griffin00_room.model",
            "gm_griffin00_room/lr_palette.dds",
            "camera/griffin_st01.camanm",
            "camera/griffin_st02.camanm",
            "camera/griffin_non01.camanm",
        ]
        .iter()
        .map(|r| folder_member_path(&stage, "mapset_griffin00", r))
        .collect();
        let dir = PackDir {
            dir: "./data_mods/custom_models/stages/Griffin House".into(),
            folder: Some("Griffin House".into()),
            arcs: vec![ArcFile {
                name: "mapset_griffin00.arc".into(),
                path: "./data_mods/_cache/custom_models/mapset_griffin00-abcd1234.arc".into(),
                source: "./data_mods/custom_models/stages/Griffin House/mapset_griffin00".into(),
                members: Some(members),
            }],
            ..Default::default()
        };
        let p = plan(&[], &[dir], &stock());
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(
            p.camera_rows[0].1,
            vec!["griffin_st01", "griffin_st02", "griffin_non01"]
        );
        assert_eq!(
            p.stages[0].parts,
            vec![("footpanel".to_string(), None), ("room".to_string(), None)]
        );
        // The mount points at the CACHE arc; the note names the folder.
        assert_eq!(
            p.mounts,
            vec![(
                "data/arc/mapset_griffin00.arc".to_string(),
                "./data_mods/_cache/custom_models/mapset_griffin00-abcd1234.arc".to_string()
            )]
        );
        assert!(p.notes[0].contains("stages/Griffin House/mapset_griffin00"));
        assert!(p.notes[0].contains("the arc's own camanm clips"));
    }

    #[test]
    fn text_rlist_grammar() {
        let rows = parse_text_rlist(
            "# comment\n\npeter00, pl, M, A, 1.0, 0.8, 0.0   # trailing\n griffin00 ,000000,000000, room, footpanel\nlonely\n, nokey\n",
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, "peter00");
        assert_eq!(rows[0].1, vec!["pl", "M", "A", "1.0", "0.8", "0.0"]);
        assert_eq!(rows[1].0, "griffin00");
        assert_eq!(rows[1].1, vec!["000000", "000000", "room", "footpanel"]);
        assert_eq!(rows[2], ("lonely".to_string(), vec![]));
    }

    #[test]
    fn labels() {
        assert_eq!(
            label_from_folder("Peter Griffin"),
            Some("PETER GRIFFIN".into())
        );
        assert_eq!(
            label_from_folder("griffin_house"),
            Some("GRIFFIN HOUSE".into())
        );
        assert_eq!(
            label_from_folder("  Café   Dancer\t#2 "),
            Some("CAF DANCER #2".into())
        );
        assert_eq!(label_from_folder("日本語"), None);
        assert_eq!(label_from_folder(""), None);
        assert_eq!(
            label_from_key("peter_griffin00", &["peter_griffin00".into()]),
            "PETER GRIFFIN"
        );
        assert_eq!(
            label_from_key("peter00", &["peter00".into(), "peter01".into()]),
            "PETER #1"
        );
        let mut long = "AVERYVERYLONGDANCERNAME".to_string();
        assert!(fit_label(&mut long));
        assert_eq!(long, "AVERYVERYLONGDA");
        let mut short = "PETER GRIFFIN".to_string();
        assert!(!fit_label(&mut short));
    }

    #[test]
    fn member_helpers() {
        let m: Vec<String> = stage_members("griffin00", &["footpanel", "room"], &["griff_st01"])
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            stage_parts_from_members("griffin00", &m),
            vec!["footpanel", "room"]
        );
        assert!(stage_parts_from_members("boom00", &m).is_empty());
        assert_eq!(camera_names_from_members(&m), vec!["griff_st01"]);
        let b: Vec<String> = body_members("peter00")
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(body_model_present("peter00", &b));
        assert!(!body_model_present("peter01", &b));
    }

    #[test]
    fn proof_of_concept_content_plans_as_expected() {
        // The A3 Peter Griffin / Griffin House arcs under the custom_models base with
        // the A3 rlists beside the stage (no dancer sidecar → defaults).
        let dancer_dir = PackDir {
            dir: "./data_mods/custom_models/dancers/Peter Griffin".into(),
            folder: Some("Peter Griffin".into()),
            arcs: vec![arc(
                "pl_peter00.arc",
                "./data_mods/custom_models/dancers/Peter Griffin",
                Some(body_members("peter00")),
            )],
            ..Default::default()
        };
        let stage_dir = PackDir {
            dir: "./data_mods/custom_models/stages/Griffin House".into(),
            folder: Some("Griffin House".into()),
            arcs: vec![
                arc(
                    "mapset_griffin00.arc",
                    "./data_mods/custom_models/stages/Griffin House",
                    Some(stage_members("griffin00", &["footpanel", "room"], &[])),
                ),
                arc(
                    "mapset_griffin00_g.arc",
                    "./data_mods/custom_models/stages/Griffin House",
                    Some(stage_members("griffin00", &["footpanel", "room"], &[])),
                ),
            ],
            map_rows: parse_text_rlist(
                "boom00, 000000, 000000, bg, stage\ngriffin00, 000000, 000000, room, footpanel",
            ),
            camera_rows: parse_text_rlist("griffin00, st001_st02, st001_st03, st001_non01"),
            ..Default::default()
        };
        let p = plan(&[dancer_dir], &[stage_dir], &stock());
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.dancers.len(), 1);
        let d = &p.dancers[0];
        assert_eq!(d.key, "peter00");
        assert_eq!(d.row, 26);
        assert_eq!(d.sex, Sex::Male);
        assert_eq!((d.model_scale, d.shadow_scale), (1.0, 0.8));
        assert_eq!(p.stages.len(), 1);
        let s = &p.stages[0];
        assert_eq!(s.key, "griffin00");
        assert_eq!(s.row, 34);
        // Sidecar order wins (room before footpanel), no priorities.
        assert_eq!(
            s.parts,
            vec![("room".to_string(), None), ("footpanel".to_string(), None)]
        );
        assert_eq!(
            p.camera_rows,
            vec![(
                34,
                vec![
                    "st001_st02".to_string(),
                    "st001_st03".to_string(),
                    "st001_non01".to_string()
                ]
            )]
        );
        assert_eq!(
            p.labels,
            vec![
                ("peter00".to_string(), "PETER GRIFFIN".to_string()),
                ("griffin00".to_string(), "GRIFFIN HOUSE".to_string())
            ]
        );
        // The `_g` arc is never mounted.
        assert_eq!(
            p.mounts,
            vec![
                (
                    "data/arc/pl_peter00.arc".to_string(),
                    "./data_mods/custom_models/dancers/Peter Griffin/pl_peter00.arc".to_string()
                ),
                (
                    "data/arc/mapset_griffin00.arc".to_string(),
                    "./data_mods/custom_models/stages/Griffin House/mapset_griffin00.arc"
                        .to_string()
                ),
            ]
        );
        assert_eq!(p.notes.len(), 2);
        assert!(p.notes[0].contains("stock male defaults"));
        assert!(p.notes[1].contains("sidecar row"));
    }

    #[test]
    fn flat_arcs_take_the_key_rule_and_borrow_the_stock_camera() {
        let dancer_dir = PackDir {
            dir: "./data_mods/custom_models/dancers".into(),
            folder: None,
            arcs: vec![
                arc(
                    "pl_peter_griffin00.arc",
                    "./data_mods/custom_models/dancers",
                    Some(body_members("peter_griffin00")),
                ),
                arc(
                    "pl_bob00.arc",
                    "./data_mods/custom_models/dancers",
                    Some(body_members("bob00")),
                ),
                arc(
                    "pl_bob01.arc",
                    "./data_mods/custom_models/dancers",
                    Some(body_members("bob01")),
                ),
                arc(
                    "pl_bob00_head00.arc",
                    "./data_mods/custom_models/dancers",
                    Some(vec!["data/chara/pl_bob00_head00/pl_bob00_head00.model"]),
                ),
            ],
            chara_rows: parse_text_rlist("bob00, pl, F, A, 0.9, 0.75, 0.0"),
            ..Default::default()
        };
        let stage_dir = PackDir {
            dir: "./data_mods/custom_models/stages".into(),
            folder: None,
            arcs: vec![arc(
                "mapset_room00.arc",
                "./data_mods/custom_models/stages",
                Some(stage_members("room00", &["bg", "floor"], &[])),
            )],
            ..Default::default()
        };
        let p = plan(&[dancer_dir], &[stage_dir], &stock());
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.dancers.len(), 3);
        assert_eq!(p.dancers[1].sex, Sex::Female);
        assert_eq!(p.dancers[1].model_scale, 0.9);
        assert_eq!(p.dancers[2].sex, Sex::Male);
        let labels: Vec<(&str, &str)> = p
            .labels
            .iter()
            .map(|(k, l)| (k.as_str(), l.as_str()))
            .collect();
        assert!(labels.contains(&("peter_griffin00", "PETER GRIFFIN")));
        assert!(labels.contains(&("bob00", "BOB #1")));
        assert!(labels.contains(&("bob01", "BOB #2")));
        assert!(labels.contains(&("room00", "ROOM")));
        // Stage: parts from members, camera borrowed.
        assert_eq!(
            p.stages[0].parts,
            vec![("bg".to_string(), None), ("floor".to_string(), None)]
        );
        assert_eq!(p.camera_rows[0].1, stock().default_camera_row);
        // The head part mounted (body accepted in the same dir).
        assert!(p
            .mounts
            .iter()
            .any(|(g, _)| g == "data/arc/pl_bob00_head00.arc"));
    }

    #[test]
    fn own_camanm_clips_win_over_the_borrowed_set() {
        let stage_dir = PackDir {
            dir: "./data_mods/custom_models/stages/Room".into(),
            folder: Some("Room".into()),
            arcs: vec![arc(
                "mapset_room00.arc",
                "./data_mods/custom_models/stages/Room",
                Some(stage_members(
                    "room00",
                    &["bg"],
                    &["room_st01", "room_st02", "room_non01"],
                )),
            )],
            ..Default::default()
        };
        let p = plan(&[], &[stage_dir], &stock());
        assert_eq!(
            p.camera_rows[0].1,
            vec!["room_st01", "room_st02", "room_non01"]
        );
        assert!(p.notes[0].contains("the arc's own camanm clips"));
    }

    #[test]
    fn rejections_warn_and_never_touch_stock() {
        let dancer_dir = PackDir {
            dir: "./data_mods/custom_models/dancers".into(),
            folder: None,
            arcs: vec![
                // Stock key collision.
                arc(
                    "pl_emi00.arc",
                    "./data_mods/custom_models/dancers",
                    Some(body_members("emi00")),
                ),
                // Unreadable.
                arc("pl_zzz00.arc", "./data_mods/custom_models/dancers", None),
                // Key ≠ model inside.
                arc(
                    "pl_aaa00.arc",
                    "./data_mods/custom_models/dancers",
                    Some(body_members("bbb00")),
                ),
                // Part without a body.
                arc(
                    "pl_ccc00_head00.arc",
                    "./data_mods/custom_models/dancers",
                    Some(vec!["data/chara/pl_ccc00_head00/pl_ccc00_head00.model"]),
                ),
                // Malformed sidecar row → defaults + WARN.
                arc(
                    "pl_ddd00.arc",
                    "./data_mods/custom_models/dancers",
                    Some(body_members("ddd00")),
                ),
            ],
            chara_rows: parse_text_rlist("ddd00, pl, X, A, nope, 0.8, 0.0"),
            ..Default::default()
        };
        let stage_dir = PackDir {
            dir: "./data_mods/custom_models/stages".into(),
            folder: None,
            arcs: vec![
                arc(
                    "mapset_boom00.arc",
                    "./data_mods/custom_models/stages",
                    Some(stage_members("boom00", &["bg"], &[])),
                ),
                arc(
                    "mapset_eee00.arc",
                    "./data_mods/custom_models/stages",
                    Some(stage_members("eee00", &["bg"], &[])),
                ),
            ],
            // Lists a part the arc lacks (dropped) and one it has.
            map_rows: parse_text_rlist("eee00, 000000, 000000, ghost, bg:-2"),
            ..Default::default()
        };
        let p = plan(&[dancer_dir], &[stage_dir], &stock());
        assert_eq!(p.dancers.len(), 1);
        assert_eq!(p.dancers[0].key, "ddd00");
        assert_eq!(p.dancers[0].sex, Sex::Male);
        assert_eq!(p.stages.len(), 1);
        assert_eq!(p.stages[0].key, "eee00");
        assert_eq!(p.stages[0].parts, vec![("bg".to_string(), Some(-2))]);
        let joined = p.warnings.join("\n");
        assert!(joined.contains("emi00\" already exists"), "{joined}");
        assert!(
            joined.contains("pl_zzz00.arc: not a readable arc"),
            "{joined}"
        );
        assert!(
            joined.contains("pl_aaa00.arc: no data/chara/pl_aaa00"),
            "{joined}"
        );
        assert!(
            joined.contains("without an accepted pl_ccc00.arc"),
            "{joined}"
        );
        assert!(
            joined.contains("row for \"ddd00\" is malformed"),
            "{joined}"
        );
        assert!(joined.contains("boom00\" already exists"), "{joined}");
        assert!(
            joined.contains("\"ghost\" listed for \"eee00\" has no model"),
            "{joined}"
        );
        assert_eq!(p.warnings.len(), 7, "{joined}");
    }

    #[test]
    fn duplicate_custom_key_across_folders_keeps_the_first() {
        let a = PackDir {
            dir: "./data_mods/custom_models/dancers/One".into(),
            folder: Some("One".into()),
            arcs: vec![arc(
                "pl_dup00.arc",
                "./data_mods/custom_models/dancers/One",
                Some(body_members("dup00")),
            )],
            ..Default::default()
        };
        let b = PackDir {
            dir: "./data_mods/custom_models/dancers/Two".into(),
            folder: Some("Two".into()),
            arcs: vec![arc(
                "pl_dup00.arc",
                "./data_mods/custom_models/dancers/Two",
                Some(body_members("dup00")),
            )],
            ..Default::default()
        };
        let p = plan(&[a, b], &[], &stock());
        assert_eq!(p.dancers.len(), 1);
        assert_eq!(p.labels, vec![("dup00".to_string(), "ONE".to_string())]);
        assert_eq!(p.warnings.len(), 1);
        assert_eq!(p.mounts.len(), 1);
    }

    #[test]
    fn long_folder_names_are_cut_with_a_warning() {
        let d = PackDir {
            dir: "./data_mods/custom_models/dancers/An Extremely Long Character Name".into(),
            folder: Some("An Extremely Long Character Name".into()),
            arcs: vec![arc(
                "pl_long00.arc",
                "./data_mods/custom_models/dancers/An Extremely Long Character Name",
                Some(body_members("long00")),
            )],
            ..Default::default()
        };
        let p = plan(&[d], &[], &stock());
        assert_eq!(p.labels[0].1, "AN EXTREMELY LO");
        assert_eq!(p.labels[0].1.len(), MAX_LABEL_BYTES);
        assert!(p.warnings.iter().any(|w| w.contains("longer than")));
    }
}
