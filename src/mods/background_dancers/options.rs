//! The DANCER SOURCE / STAGE SOURCE and BACKGROUND DANCER / BACKGROUND STAGE
//! option rows (design §4.2; source-grouped since 2026-09-30 §4.5): per
//! kind, ONE source row (0 RANDOM · 1 STOCK · 2… the custom sources under
//! `data_mods/custom_models/<kind>/<Source>/`) — registered only when a
//! custom source exists — and ONE model row PER SOURCE (0 RANDOM · 1..=count),
//! each shown while the source row names its source (`ShowWhen::Equals`), so
//! a player sees the source row plus the one model row it controls; source
//! RANDOM hides them all. `background_dancer` / `background_stage` stay the
//! STOCK model rows (cached stock values keep their meaning);
//! `<base>_<slug>` is a custom source's row (its last pick is remembered per
//! player); `<base>_source` the source row. Every row of a kind renders the
//! kind's shipped label / preview chrome through the framework's texture
//! aliases (the source row keeps its own `DANCER SOURCE` label). All rows are
//! `custom_options` SCALAR rows with `ScalarFormat::Dynamic` labels, in-game
//! only (their live 3D previews are the point), persisted LOCALLY per side
//! (`PersistMode::Local` — the JSON cache, never the wire). The stage rows
//! are cabinet-wide: mirrored across players in versus (`versus_mirror`,
//! driven from a value-changed OBSERVER — a plain `on_change` fn cannot know
//! which of N runtime-named rows it belongs to); the dancer rows stay per
//! side. Effective at the next song: `lifecycle::window_entry` reads
//! [`stage_request`] / [`dancer_request`] and resolves them through
//! `selection::resolve_choice`.
//!
//! The decisions are pure and host-tested in `options_logic.rs`; this file is
//! the engine-facing shell: registration, `custom_options::get_value` reads
//! (no per-side atomics — the registry is the one source of truth), the
//! mirror observer, the preview reader.
//!
//! Fail-open: without the scalar-row machinery the rows are simply absent
//! (one WARN) and gameplay keeps its random picks; a `Duplicate` registration
//! (mod re-enabled this boot) re-shows the existing rows. A LIVE enable
//! registers the rows but their label textures appear at the next launch —
//! the framework flushes the label atlas once at boot (one INFO, by design).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::services::custom_options::{
    self, PersistMode, RegisterError, RegisterSpec, ScalarFormat, ShowWhen,
};
use crate::services::versus_mirror;
use crate::{log_info, log_warn};

use super::catalog::{self, Catalog, Kind, RANDOM};
use super::options_logic::{self, RowInfo, RowRole};

pub use super::options_logic::{Request, OPT_DANCER, OPT_STAGE};

/// Coarse step (Start held) on a model row — 5 entries at a time.
const STEP_COARSE_MODEL: i32 = 5;
/// Coarse step on a source row — a handful of sources, no acceleration.
const STEP_COARSE_SOURCE: i32 = 1;

/// The grouped catalog the rows index (set once per process: the candidate
/// tables are built once at the first enable and never change).
static CATALOG: OnceLock<Catalog> = OnceLock::new();
/// The row table derived from it, and the same ids leaked once for the
/// framework's `&'static str` contract (parallel to `ROWS`).
static ROWS: OnceLock<Vec<RowInfo>> = OnceLock::new();
static IDS: OnceLock<Vec<&'static str>> = OnceLock::new();
/// Every row registered (and currently available) — the reader gate.
static ROWS_LIVE: AtomicBool = AtomicBool::new(false);
/// The versus-mirror observer is subscribed once per process.
static MIRROR_SUBSCRIBED: AtomicBool = AtomicBool::new(false);

fn catalog() -> Option<&'static Catalog> {
    CATALOG.get()
}

fn rows() -> &'static [RowInfo] {
    ROWS.get().map(Vec::as_slice).unwrap_or(&[])
}

fn ids() -> &'static [&'static str] {
    IDS.get().map(Vec::as_slice).unwrap_or(&[])
}

/// The leaked id of a row (registration needs `&'static str`).
fn static_id(row: &RowInfo) -> Option<&'static str> {
    ids().iter().copied().find(|id| *id == row.id)
}

/// The `DynamicLabelFn`: `RANDOM` / `STOCK` / the source labels for a source
/// row, `RANDOM` / the entries' labels for a model row, `None` (⇒ the
/// framework's integer text) beyond — never panics.
fn label(option_id: &str, value: i32) -> Option<String> {
    let cat = catalog()?;
    options_logic::label_for_row(cat, rows(), option_id, value)
}

/// Load-side transform: a cached value outside `0..=max` (a source or entry
/// that went away, a hand-edited cache) lands on RANDOM. Unknown row ⇒ RANDOM.
fn clamp_load(id: &str, value: i32) -> i32 {
    let max = catalog()
        .and_then(|cat| options_logic::row_max(cat, rows(), id))
        .unwrap_or(0);
    catalog::clamp_to_catalog(value, max)
}

fn identity_transform(_id: &str, value: i32) -> i32 {
    value
}

/// Which row family an option id belongs to (`None` for any other row) — the
/// preview driver's focus classifier; source rows included.
pub fn kind_for_option(option_id: &str) -> Option<Kind> {
    options_logic::row(rows(), option_id).map(|r| r.kind)
}

fn is_stage_row(option_id: &str) -> bool {
    kind_for_option(option_id) == Some(Kind::Stage)
}

fn stage_ids() -> Vec<&'static str> {
    rows()
        .iter()
        .filter(|r| r.kind == Kind::Stage)
        .filter_map(static_id)
        .collect()
}

/// Register every row (mod enable). Returns whether the rows are live.
pub fn register(built: Catalog) -> bool {
    if !custom_options::row_injection_available() {
        log_warn!(
            "BackgroundDancers: scalar row machinery unavailable -- BACKGROUND DANCER / STAGE rows absent, picks stay random"
        );
        return false;
    }
    if built.count(Kind::Dancer, 0) == 0 || built.count(Kind::Stage, 0) == 0 {
        log_warn!(
            "BackgroundDancers: empty stock catalog ({} dancers / {} stages) -- rows not registered",
            built.count(Kind::Dancer, 0),
            built.count(Kind::Stage, 0)
        );
        return false;
    }
    let cat = CATALOG.get_or_init(|| built);
    let rows = ROWS.get_or_init(|| options_logic::build_rows(cat));
    IDS.get_or_init(|| {
        rows.iter()
            .map(|r| Box::leak(r.id.clone().into_boxed_str()) as &'static str)
            .collect()
    });

    // The base preview chrome (the `_TEMPLATE` with its marker cleared) must
    // exist on disk BEFORE registration so the atlas flush sees it — the
    // webui_options precedent. Every row of a kind aliases this one chrome.
    for id in [OPT_DANCER, OPT_STAGE] {
        crate::mods::webui_options::preview_gen::generate_chrome(id);
    }

    let mut all_ok = true;
    for row in rows {
        let Some(id) = static_id(row) else {
            all_ok = false;
            continue;
        };
        let Some(max) = options_logic::row_max(cat, rows, &row.id) else {
            all_ok = false;
            continue;
        };
        let base = options_logic::base_id(row.kind);
        let (display_name, description): (&'static str, &'static str) = match (row.role, row.kind) {
            (RowRole::Source, Kind::Dancer) => (
                "Dancer Source",
                "Which set the BACKGROUND DANCER row picks from: the game's own (STOCK), one custom source, or RANDOM across all of them",
            ),
            (RowRole::Source, Kind::Stage) => (
                "Stage Source",
                "Which set the BACKGROUND STAGE row picks from: the game's own (STOCK), one custom source, or RANDOM across all of them",
            ),
            (RowRole::Model { .. }, Kind::Dancer) => (
                "Background Dancer",
                "The 3D dancer shown behind your lane; RANDOM picks a different one from the selected source each song",
            ),
            (RowRole::Model { .. }, Kind::Stage) => (
                "Background Stage",
                "The 3D stage shown behind your lane (shared by both players); RANDOM picks a different one from the selected source each song",
            ),
        };
        let spec = match row.role {
            RowRole::Source => {
                RegisterSpec::scalar(id, RANDOM, max as i32, 1, ScalarFormat::Dynamic(label))
                    .step_coarse(STEP_COARSE_SOURCE)
                    .preview_texture_like(base)
            }
            RowRole::Model { source } => {
                let show_when = match options_logic::source_row(rows, row.kind) {
                    Some(parent) => match static_id(parent) {
                        Some(parent_id) => ShowWhen::Equals {
                            parent_id: parent_id.to_string(),
                            value: source as i32 + 1,
                        },
                        None => ShowWhen::Always,
                    },
                    None => ShowWhen::Always,
                };
                RegisterSpec::scalar(id, RANDOM, max as i32, 1, ScalarFormat::Dynamic(label))
                    .step_coarse(STEP_COARSE_MODEL)
                    .show_when(show_when)
                    .label_texture_like(base)
                    .preview_texture_like(base)
            }
        };
        if !register_one(id, spec, max, display_name, description) {
            all_ok = false;
        }
    }
    if !all_ok {
        // Leave whatever registered available (harmless), but never read it.
        ROWS_LIVE.store(false, Ordering::Release);
        return false;
    }
    versus_mirror::register(&stage_ids());
    if !MIRROR_SUBSCRIBED.swap(true, Ordering::AcqRel) {
        custom_options::subscribe_value_changed(Arc::new(|id: &str, side: u8, value: i32| {
            // The stage rows are cabinet-wide: while the versus mirror is
            // engaged an edit on one side propagates to the other
            // (`set_value`'s unchanged-value check ends the echo at depth
            // one, exactly as the old `on_change` tail did).
            if rows_live() && is_stage_row(id) {
                versus_mirror::mirror_edit(id, side, value);
            }
        }));
    }
    ROWS_LIVE.store(true, Ordering::Release);
    let describe = |kind: Kind| -> String {
        let counts: Vec<String> = cat
            .sources(kind)
            .iter()
            .map(|s| format!("{} {}", s.label, s.entries.len()))
            .collect();
        if cat.has_custom(kind) {
            format!(
                "source row + {} model rows ({})",
                cat.source_count(kind),
                counts.join(", ")
            )
        } else {
            format!("stock row only ({})", counts.join(", "))
        }
    };
    log_info!(
        "BackgroundDancers: option rows live -- DANCER: {}; STAGE: {} (stage rows mirrored in versus); local persistence only, effective next song",
        describe(Kind::Dancer),
        describe(Kind::Stage)
    );
    true
}

/// One row: the common tail of every spec (RANDOM default, local
/// persistence with the load clamp, in-game only, the overlay strings), then
/// registration. `Duplicate` (re-enable this boot) re-shows the row; any
/// other error ⇒ WARN + `false`.
fn register_one(
    id: &'static str,
    spec: RegisterSpec,
    max: usize,
    display_name: &'static str,
    description: &'static str,
) -> bool {
    let spec = spec
        .default_value(RANDOM)
        .persist_mode(PersistMode::Local)
        .persist_transform(identity_transform, clamp_load)
        .in_game_only()
        .display_name(display_name)
        .description(description);
    match custom_options::register_option(spec) {
        Ok(_handle) => {
            log_info!(
                "BackgroundDancers: registered {} (0 = RANDOM, 1..={}; label textures appear at the next launch if this was a live enable)",
                id,
                max
            );
            true
        }
        Err(RegisterError::Duplicate { .. }) => {
            custom_options::set_option_available(id, true);
            true
        }
        Err(e) => {
            log_warn!(
                "BackgroundDancers: {} row registration failed: {e} -- picks stay random",
                id
            );
            false
        }
    }
}

/// Mod disable: hide the rows (registration, values and persistence stay)
/// and stop mirroring the stage rows.
pub fn set_available(available: bool) {
    if catalog().is_none() {
        return;
    }
    if !available {
        versus_mirror::unregister(&stage_ids());
    } else {
        versus_mirror::register(&stage_ids());
    }
    for id in ids() {
        custom_options::set_option_available(id, available);
    }
    ROWS_LIVE.store(available, Ordering::Release);
}

/// Whether the rows are registered and shown.
pub fn rows_live() -> bool {
    ROWS_LIVE.load(Ordering::Acquire)
}

fn value_of(side: u8, id: &str) -> Option<i32> {
    custom_options::get_value(side, id)
}

/// What `side`'s rows of `kind` ask the pick for: `Any` (source RANDOM, rows
/// down, or anything stale), `Within` a source, or an explicit `Key`.
fn request(kind: Kind, side: u8) -> Request {
    if side >= 2 || !rows_live() {
        return Request::Any;
    }
    let Some(cat) = catalog() else {
        return Request::Any;
    };
    let rows = rows();
    let source_value = options_logic::source_row(rows, kind).and_then(|r| value_of(side, &r.id));
    options_logic::request_for(cat, rows, kind, source_value, |id| value_of(side, id))
}

/// `side`'s BACKGROUND STAGE request (the stage rows are mirrored in versus,
/// so both sides agree).
pub fn stage_request(side: u8) -> Request {
    request(Kind::Stage, side)
}

/// `side`'s BACKGROUND DANCER request.
pub fn dancer_request(side: u8) -> Request {
    request(Kind::Dancer, side)
}

/// The key the focused row `option_id` previews for `side` (design R21): a
/// model row's own value, a source row's EFFECTIVE pick; `None` for RANDOM
/// at either level, rows down, or a row that is not ours.
pub fn row_choice_key(option_id: &str, side: u8) -> Option<String> {
    if side >= 2 || !rows_live() {
        return None;
    }
    let cat = catalog()?;
    options_logic::row_choice_key(cat, rows(), option_id, |id| value_of(side, id))
}
