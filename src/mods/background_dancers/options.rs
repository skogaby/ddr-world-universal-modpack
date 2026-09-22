//! The BACKGROUND DANCER / BACKGROUND STAGE option rows (design §4.2): two
//! `custom_options` SCALAR rows rendered through [`ScalarFormat::Dynamic`]
//! with the [`catalog`](super::catalog) labels (`RANDOM`, `EMI #2`,
//! `BOOM #3`, …), registered by the mod's `enable()` once the candidate
//! tables exist, in-game only (their previews are the point), persisted
//! LOCALLY per side (`PersistMode::Local` — the JSON cache, never the wire),
//! the stage row mirrored across players in versus (`versus_mirror`: one
//! stage per cabinet; the dancer row stays per side). Effective at the next
//! song: `lifecycle::window_entry` reads [`stage_choice`] /
//! [`dancer_choice`] and resolves them through `selection::resolve_choice`.
//!
//! Fail-open: without the scalar-row machinery the rows are simply absent
//! (one WARN) and gameplay keeps its random picks; a `Duplicate` registration
//! (mod re-enabled this boot) re-arms the existing rows. A LIVE enable
//! registers the rows but their label textures appear at the next launch —
//! the framework flushes the label atlas once at boot (one INFO, by design).

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;

use crate::services::custom_options::{
    self, PersistMode, RegisterError, RegisterSpec, ScalarFormat,
};
use crate::services::versus_mirror;
use crate::{log_info, log_warn};

use super::catalog::{self, Catalog, Kind, RANDOM};

pub const OPT_DANCER: &str = "background_dancer";
pub const OPT_STAGE: &str = "background_stage";

/// Coarse step (Start held) — 5 entries at a time through a 25/26-long list.
const STEP_COARSE: i32 = 5;

/// The catalog the rows index (set once per process: the candidate tables
/// are built once at the first enable and never change).
static CATALOG: OnceLock<Catalog> = OnceLock::new();
/// Live per-side row values (mirrors of the registry, written by `on_change`).
static DANCER_VALUE: [AtomicI32; 2] = [AtomicI32::new(RANDOM), AtomicI32::new(RANDOM)];
static STAGE_VALUE: [AtomicI32; 2] = [AtomicI32::new(RANDOM), AtomicI32::new(RANDOM)];
/// Both rows registered (and currently available) — the `*_choice` gate.
static ROWS_LIVE: AtomicBool = AtomicBool::new(false);

fn catalog() -> Option<&'static Catalog> {
    CATALOG.get()
}

/// The `DynamicLabelFn`: `RANDOM` for 0, the catalog label for `1..=count`,
/// `None` (⇒ the framework's integer text) beyond — never panics.
fn label(option_id: &str, value: i32) -> Option<String> {
    let kind = kind_of(option_id)?;
    catalog()?.label(kind, value).map(str::to_string)
}

fn kind_of(option_id: &str) -> Option<Kind> {
    match option_id {
        OPT_DANCER => Some(Kind::Dancer),
        OPT_STAGE => Some(Kind::Stage),
        _ => None,
    }
}

/// Which of our two rows an option id names (`None` for any other row) —
/// the preview driver's focus classifier.
pub fn kind_for_option(option_id: &str) -> Option<Kind> {
    kind_of(option_id)
}

fn store(id: &str, side: u8, value: i32) {
    if side >= 2 {
        return;
    }
    let slot = match kind_of(id) {
        Some(Kind::Dancer) => &DANCER_VALUE[side as usize],
        Some(Kind::Stage) => &STAGE_VALUE[side as usize],
        None => return,
    };
    slot.store(value, Ordering::Release);
}

fn on_dancer_change(side: u8, value: i32) {
    store(OPT_DANCER, side, value);
}

/// The stage row is cabinet-wide: mirror the edit to the other side while
/// the versus mirror is engaged (its unchanged-value check ends the
/// recursion at depth one).
fn on_stage_change(side: u8, value: i32) {
    store(OPT_STAGE, side, value);
    versus_mirror::mirror_edit(OPT_STAGE, side, value);
}

fn identity_transform(_id: &str, value: i32) -> i32 {
    value
}

/// Load-side transform: a cached value outside `0..=count` (the catalog
/// shrank, a hand-edited cache) lands on RANDOM. No catalog ⇒ RANDOM.
fn clamp_load(id: &str, value: i32) -> i32 {
    let count = kind_of(id)
        .and_then(|k| catalog().map(|c| c.count(k)))
        .unwrap_or(0);
    catalog::clamp_to_catalog(value, count)
}

/// Register both rows (mod enable). Returns whether the rows are live.
pub fn register(built: Catalog) -> bool {
    if !custom_options::row_injection_available() {
        log_warn!(
            "BackgroundDancers: scalar row machinery unavailable -- BACKGROUND DANCER / STAGE rows absent, picks stay random"
        );
        return false;
    }
    if built.count(Kind::Dancer) == 0 || built.count(Kind::Stage) == 0 {
        log_warn!(
            "BackgroundDancers: empty catalog ({} dancers / {} stages) -- rows not registered",
            built.count(Kind::Dancer),
            built.count(Kind::Stage)
        );
        return false;
    }
    let cat = CATALOG.get_or_init(|| built);

    // The base preview chrome (the `_TEMPLATE` with its marker cleared) must
    // exist on disk BEFORE registration so the atlas flush sees it — the
    // webui_options precedent. Fail-open: a missing template = blank box.
    for id in [OPT_DANCER, OPT_STAGE] {
        crate::mods::webui_options::preview_gen::generate_chrome(id);
    }

    let dancer_ok = register_one(
        OPT_DANCER,
        cat.count(Kind::Dancer),
        "Background Dancer",
        "The 3D dancer shown behind your lane; RANDOM picks a different one each song",
        on_dancer_change,
    );
    let stage_ok = register_one(
        OPT_STAGE,
        cat.count(Kind::Stage),
        "Background Stage",
        "The 3D stage shown behind your lane (shared by both players); RANDOM picks a different one each song",
        on_stage_change,
    );
    if !(dancer_ok && stage_ok) {
        // Leave whatever registered available (harmless), but never read it.
        ROWS_LIVE.store(false, Ordering::Release);
        return false;
    }
    versus_mirror::register(&[OPT_STAGE]);
    ROWS_LIVE.store(true, Ordering::Release);
    log_info!(
        "BackgroundDancers: option rows live -- BACKGROUND DANCER ({} entries) / BACKGROUND STAGE ({} entries, mirrored in versus); local persistence only, effective next song",
        cat.count(Kind::Dancer),
        cat.count(Kind::Stage)
    );
    true
}

/// One row. `Duplicate` (re-enable this boot) re-seeds the atomic from the
/// registry (the duplicate path does not re-fire `on_change`) and re-shows
/// the row; any other error ⇒ WARN + `false`.
fn register_one(
    id: &'static str,
    count: usize,
    display_name: &'static str,
    description: &'static str,
    on_change: fn(u8, i32),
) -> bool {
    let spec = RegisterSpec::scalar(id, RANDOM, count as i32, 1, ScalarFormat::Dynamic(label))
        .step_coarse(STEP_COARSE)
        .default_value(RANDOM)
        .persist_mode(PersistMode::Local)
        .persist_transform(identity_transform, clamp_load)
        .in_game_only()
        .display_name(display_name)
        .description(description)
        .on_change(on_change);
    match custom_options::register_option(spec) {
        Ok(_handle) => {
            log_info!(
                "BackgroundDancers: registered {} (0 = RANDOM, 1..={}; label textures appear at the next launch if this was a live enable)",
                id,
                count
            );
            true
        }
        Err(RegisterError::Duplicate { .. }) => {
            for side in 0..2u8 {
                on_change(side, custom_options::get_value(side, id).unwrap_or(RANDOM));
            }
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
/// and stop mirroring the stage row.
pub fn set_available(available: bool) {
    if catalog().is_none() {
        return;
    }
    if !available {
        versus_mirror::unregister(&[OPT_STAGE]);
    } else {
        versus_mirror::register(&[OPT_STAGE]);
    }
    for id in [OPT_DANCER, OPT_STAGE] {
        custom_options::set_option_available(id, available);
    }
    ROWS_LIVE.store(available, Ordering::Release);
}

/// Whether the rows are registered and shown.
pub fn rows_live() -> bool {
    ROWS_LIVE.load(Ordering::Acquire)
}

/// `side`'s BACKGROUND STAGE choice: the rlist key, or `None` for RANDOM /
/// rows unavailable / a stale value the catalog no longer covers.
pub fn stage_choice(side: u8) -> Option<String> {
    choice(Kind::Stage, &STAGE_VALUE, side)
}

/// `side`'s BACKGROUND DANCER choice (same contract as [`stage_choice`]).
pub fn dancer_choice(side: u8) -> Option<String> {
    choice(Kind::Dancer, &DANCER_VALUE, side)
}

/// The catalog key `side`'s row of `kind` currently holds (the two readers
/// above, by kind) — `None` for RANDOM / rows down / a stale value.
pub fn choice_key(kind: Kind, side: u8) -> Option<String> {
    match kind {
        Kind::Stage => stage_choice(side),
        Kind::Dancer => dancer_choice(side),
    }
}

fn choice(kind: Kind, values: &[AtomicI32; 2], side: u8) -> Option<String> {
    if side >= 2 || !rows_live() {
        return None;
    }
    let value = values[side as usize].load(Ordering::Acquire);
    catalog()?.key(kind, value).map(str::to_string)
}

/// The live row value of `side` (RANDOM when the rows are down) — for the
/// preview driver (a later step) and diagnostics.
pub fn value(kind: Kind, side: u8) -> i32 {
    if side >= 2 || !rows_live() {
        return RANDOM;
    }
    match kind {
        Kind::Dancer => DANCER_VALUE[side as usize].load(Ordering::Acquire),
        Kind::Stage => STAGE_VALUE[side as usize].load(Ordering::Acquire),
    }
}
